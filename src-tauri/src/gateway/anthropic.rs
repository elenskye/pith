//! Anthropic Messages（流式 SSE），照 Peck 的 `convert/anthropic.rs` 改的。
//!
//! - 思考一直开着（Claude 5.5 一族关不掉），`thinking: adaptive`、显示摘要；深浅用 `output_config.effort`（Opus 5.5 默认 medium，这里每档写明）。
//! - 缓存断点（`decisions/0036`）：系统提示里标了 `cache` 的块、最后一条站主消息、它前一条站主消息，最多 4 个；
//!   顺序固定为 工具 → 系统 → 对话，对话只往后追加。
//! - 工具开 `eager_input_streaming`（流式时参数边写边来）；参数读不成完整 JSON 的交给循环回一个出错的结果。
//! - Opus 5.5、Sonnet 5.5 开服务端的拒答回退（`fallbacks: "default"` + beta 头）；回答里的 `fallback` 块和别的不认识的块
//!   原样存成 `Part::Native`。思考块和这些块换了 Claude 模型也原样放回（API 读不了的自己丢、不计费），换到别家才丢。
//! - 不发 `temperature`（API 版本 2026-10-21 起一律 400）。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::catalog::ModelSpec;
use super::{Delta, Message, Opaque, Origin, Part, Reply, Request, Role, Stop, Tokens, WireStream, parse_tool_input};

pub const URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = crate::research::anthropic::API_VERSION;
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// 统一的请求 → Messages 的 body
pub fn encode(req: &Request, spec: &ModelSpec, origin: &Origin) -> Value {
    let mut body = json!({ "model": spec.api_model, "max_tokens": req.max_output_tokens, "stream": true });
    let mut breakpoints = 0;
    if !req.system.is_empty() {
        let blocks: Vec<Value> = req
            .system
            .iter()
            .map(|b| {
                let mut v = json!({ "type": "text", "text": b.text });
                if b.cache && breakpoints < 2 {
                    v["cache_control"] = json!({ "type": "ephemeral" });
                    breakpoints += 1;
                }
                v
            })
            .collect();
        body["system"] = Value::Array(blocks);
    }
    let mut messages: Vec<Value> = req
        .messages
        .iter()
        .filter_map(|m| {
            let (role, content) = match m.role {
                Role::Assistant => ("assistant", assistant_blocks(m, origin)),
                Role::User => ("user", user_blocks(m)),
            };
            (!content.is_empty()).then(|| json!({ "role": role, "content": content }))
        })
        .collect();
    // 对话的断点：最后一条站主消息（下一次请求从它接着命中），再加它前一条（工具来回多、超过回看的 20 块时也能命中）
    let users: Vec<usize> = messages.iter().enumerate().filter(|(_, m)| m["role"] == "user").map(|(i, _)| i).collect();
    for &i in users.iter().rev().take(2) {
        if breakpoints >= 4 {
            break;
        }
        if let Some(b) = messages[i]["content"].as_array_mut().and_then(|c| c.last_mut()) {
            b["cache_control"] = json!({ "type": "ephemeral" });
            breakpoints += 1;
        }
    }
    body["messages"] = Value::Array(std::mem::take(&mut messages));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema, "eager_input_streaming": true })).collect();
        body["tools"] = Value::Array(tools);
        if req.no_tools {
            body["tool_choice"] = json!({ "type": "none" });
        }
    }
    body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
    if let Some(e) = &req.effort {
        body["output_config"] = json!({ "effort": e });
    }
    if spec.fallback {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Anthropic 自己的私有块（思考、回退的块）：换了 Claude 模型也原样放回，读不了的由 API 自己丢掉、不计费（现行文档的要求）；
/// 别家的丢掉
fn ours<'a>(o: &'a Opaque, target: &Origin) -> Option<&'a Value> {
    (o.origin.provider == target.provider).then_some(&o.data)
}

/// 一条消息 → content 块
fn assistant_blocks(m: &Message, target: &Origin) -> Vec<Value> {
    let mut out = Vec::new();
    for part in &m.parts {
        match part {
            Part::Reasoning { text, opaque } => {
                let Some(data) = opaque.as_ref().and_then(|o| ours(o, target)) else { continue };
                if data["type"] == "redacted_thinking" {
                    out.push(data.clone());
                } else {
                    out.push(json!({ "type": "thinking", "thinking": text, "signature": data["signature"] }));
                }
            }
            Part::Text { text } if !text.is_empty() => out.push(json!({ "type": "text", "text": text })),
            Part::Text { .. } => {}
            Part::ToolCall { call_id, name, input, .. } => out.push(json!({ "type": "tool_use", "id": call_id, "name": name, "input": input })),
            Part::ToolResult { call_id, output, is_error } => out.push(tool_result(call_id, output, *is_error)),
            Part::Native { opaque } => {
                if let Some(d) = ours(opaque, target) {
                    out.push(d.clone());
                }
            }
        }
    }
    out
}

fn tool_result(call_id: &str, output: &str, is_error: bool) -> Value {
    let mut b = json!({ "type": "tool_result", "tool_use_id": call_id, "content": output });
    if is_error {
        b["is_error"] = json!(true);
    }
    b
}

/// 站主这一条：工具结果放前面（Messages 要求），再放文字
fn user_blocks(m: &Message) -> Vec<Value> {
    let mut results = Vec::new();
    let mut texts = Vec::new();
    for part in &m.parts {
        match part {
            Part::ToolResult { call_id, output, is_error } => results.push(tool_result(call_id, output, *is_error)),
            Part::Text { text } if !text.is_empty() => texts.push(json!({ "type": "text", "text": text })),
            _ => {}
        }
    }
    results.extend(texts);
    results
}

enum Block {
    Thinking { text: String, signature: String },
    Redacted(Value),
    Text(String),
    ToolUse { id: String, name: String, json: String },
    /// 别的块（fallback、服务端工具……）：原件 + 流进来的参数
    Raw { data: Value, json: String },
}

pub struct AnthropicStream {
    origin: Origin,
    blocks: BTreeMap<u64, Block>,
    tokens: Tokens,
    stop: Option<Stop>,
    refusal: Option<String>,
    model: Option<String>,
    request_id: Option<String>,
    error: Option<String>,
    stopped: bool,
    chars: usize,
}

impl AnthropicStream {
    pub fn new(origin: Origin, request_id: Option<String>) -> Self {
        AnthropicStream { origin, blocks: BTreeMap::new(), tokens: Tokens::default(), stop: None, refusal: None, model: None, request_id, error: None, stopped: false, chars: 0 }
    }

    fn usage(&mut self, u: &Value) {
        let n = |k: &str| u[k].as_u64();
        let t = &mut self.tokens;
        if let Some(v) = n("input_tokens") {
            t.input = t.input.max(v);
        }
        if let Some(v) = n("output_tokens") {
            t.output = t.output.max(v);
        }
        if let Some(v) = n("cache_read_input_tokens") {
            t.cache_read = t.cache_read.max(v);
        }
        if let Some(v) = n("cache_creation_input_tokens") {
            t.cache_write = t.cache_write.max(v);
        }
    }
}

impl WireStream for AnthropicStream {
    fn feed(&mut self, data: &Value) -> Vec<Delta> {
        match data["type"].as_str().unwrap_or("") {
            "message_start" => {
                let m = &data["message"];
                self.model = m["model"].as_str().map(str::to_string);
                if let Some(id) = m["id"].as_str().filter(|_| self.request_id.is_none()) {
                    self.request_id = Some(id.to_string());
                }
                self.usage(&m["usage"]);
                vec![]
            }
            "content_block_start" => {
                let index = data["index"].as_u64().unwrap_or(self.blocks.len() as u64);
                let b = &data["content_block"];
                let (block, delta) = match b["type"].as_str().unwrap_or("") {
                    "thinking" => (Block::Thinking { text: b["thinking"].as_str().unwrap_or("").into(), signature: b["signature"].as_str().unwrap_or("").into() }, None),
                    "redacted_thinking" => (Block::Redacted(b.clone()), None),
                    "text" => (Block::Text(b["text"].as_str().unwrap_or("").into()), None),
                    "tool_use" => {
                        let (id, name) = (b["id"].as_str().unwrap_or("").to_string(), b["name"].as_str().unwrap_or("").to_string());
                        (Block::ToolUse { id: id.clone(), name: name.clone(), json: String::new() }, Some(Delta::ToolCall { call_id: id, name }))
                    }
                    _ => (Block::Raw { data: b.clone(), json: String::new() }, None),
                };
                self.blocks.insert(index, block);
                delta.into_iter().collect()
            }
            "content_block_delta" => {
                let index = data["index"].as_u64().unwrap_or(0);
                let d = &data["delta"];
                let Some(block) = self.blocks.get_mut(&index) else { return vec![] };
                let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
                match (block, d["type"].as_str().unwrap_or("")) {
                    (Block::Thinking { text, .. }, "thinking_delta") => {
                        let t = s("thinking");
                        text.push_str(&t);
                        self.chars += t.chars().count();
                        vec![Delta::Reasoning(t)]
                    }
                    (Block::Thinking { signature, .. }, "signature_delta") => {
                        signature.push_str(&s("signature"));
                        vec![]
                    }
                    (Block::Text(text), "text_delta") => {
                        let t = s("text");
                        text.push_str(&t);
                        self.chars += t.chars().count();
                        vec![Delta::Text(t)]
                    }
                    (Block::ToolUse { json, .. } | Block::Raw { json, .. }, "input_json_delta") => {
                        let t = s("partial_json");
                        self.chars += t.chars().count();
                        json.push_str(&t);
                        vec![]
                    }
                    _ => vec![],
                }
            }
            "message_delta" => {
                self.usage(&data["usage"]);
                if let Some(r) = data["delta"]["stop_reason"].as_str() {
                    self.stop = Some(stop_reason(r));
                }
                let details = &data["delta"]["stop_details"];
                if details.is_object() {
                    self.refusal = details["category"].as_str().or(details["explanation"].as_str()).map(str::to_string);
                }
                vec![]
            }
            "message_stop" => {
                self.stopped = true;
                vec![]
            }
            "error" => {
                let e = &data["error"];
                self.error = Some(format!("Anthropic 在回答中途报错（{}）：{}", e["type"].as_str().unwrap_or("error"), super::short(e["message"].as_str().unwrap_or(""), 160)));
                vec![]
            }
            _ => vec![],
        }
    }

    fn done(&self) -> bool {
        self.stopped || self.error.is_some()
    }

    fn chars(&self) -> usize {
        self.chars
    }

    fn finish(self: Box<Self>) -> Result<Reply, String> {
        let this = *self;
        if let Some(e) = this.error {
            return Err(e);
        }
        let Some(stop) = this.stop else { return Err("回答没有收完就断开了。".into()) };
        let opaque = |data: Value| Opaque { origin: this.origin.clone(), data };
        let mut parts = Vec::new();
        for (_, block) in this.blocks {
            match block {
                Block::Thinking { text, signature } => parts.push(Part::Reasoning { text, opaque: Some(opaque(json!({ "type": "thinking", "signature": signature }))) }),
                Block::Redacted(data) => parts.push(Part::Reasoning { text: String::new(), opaque: Some(opaque(data)) }),
                Block::Text(text) => parts.push(Part::Text { text }),
                Block::ToolUse { id, name, json } => parts.push(Part::ToolCall { call_id: id, name, input: parse_tool_input(&json), signature: None }),
                Block::Raw { mut data, json } => {
                    if !json.trim().is_empty()
                        && let Ok(input) = serde_json::from_str::<Value>(&json)
                    {
                        data["input"] = input;
                    }
                    parts.push(Part::Native { opaque: opaque(data) });
                }
            }
        }
        let model = this.model.unwrap_or_else(|| this.origin.model.clone());
        Ok(Reply { message: Message { role: Role::Assistant, parts }, stop, tokens: this.tokens, model, request_id: this.request_id, refusal: this.refusal })
    }
}

fn stop_reason(s: &str) -> Stop {
    match s {
        "end_turn" | "stop_sequence" => Stop::EndTurn,
        "tool_use" => Stop::ToolUse,
        "pause_turn" => Stop::Paused,
        "max_tokens" | "model_context_window_exceeded" => Stop::MaxTokens,
        "refusal" => Stop::Refusal,
        other => Stop::Other(other.into()),
    }
}

/// 出错的回复 → 给站主看的话
pub fn error_message(status: u16, body: &Value) -> String {
    let kind = body.pointer("/error/type").and_then(Value::as_str).unwrap_or("");
    let detail = body.pointer("/error/message").and_then(Value::as_str).map(|m| format!("（{}）", super::short(m, 200))).unwrap_or_default();
    match (status, kind) {
        (401, _) | (_, "authentication_error") => "Anthropic 的 API 密钥不对或已失效。到「设置 › 模型与费用」重新填写。".into(),
        (402, _) | (_, "billing_error") => "Anthropic 账户余额不足或没有绑定付款方式。到 console.anthropic.com 的 Billing 里处理后再试。".into(),
        (403, _) | (_, "permission_error") => format!("这个密钥没有权限调用这个模型{detail}。"),
        (404, _) | (_, "not_found_error") => format!("找不到这个模型{detail}。"),
        (413, _) | (_, "request_too_large") => "发送的内容太大了。".into(),
        (429, _) | (_, "rate_limit_error") => "调用太频繁，被限流了。等一两分钟再试。".into(),
        (529, _) | (_, "overloaded_error") => "Anthropic 的服务现在过载。等一会儿再试。".into(),
        (s, _) if s >= 500 => format!("Anthropic 的服务出错（{s}）。等一会儿再试。"),
        _ => format!("请求没有被接受{detail}。"),
    }
}
