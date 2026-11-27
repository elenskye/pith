//! OpenAI Responses（流式 SSE），照 Peck 的 `convert/responses.rs` 改的。
//!
//! 不让 OpenAI 存对话（`store: false`），思考要带着密文回来：请求里 `include: ["reasoning.encrypted_content"]`，
//! 回放时把 `reasoning` 项（摘要 + 密文）和 `function_call` 项原样放回 `input`，不带 `id`。
//! 缓存是自动的；同一段对话带同一个 `prompt_cache_key`，命中更稳。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::catalog::{ModelSpec, Thinking};
use super::{Delta, Message, Opaque, Origin, Part, Reply, Request, Role, Stop, Tokens, WireStream, parse_tool_input, system_text};

pub const URL: &str = "https://api.openai.com/v1/responses";

pub fn encode(req: &Request, spec: &ModelSpec, origin: &Origin) -> Value {
    let mut input = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::User => user_items(m, &mut input),
            Role::Assistant => assistant_items(m, origin, &mut input),
        }
    }
    let mut body = json!({
        "model": spec.api_model,
        "input": input,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "max_output_tokens": req.max_output_tokens,
    });
    let system = system_text(req);
    if !system.is_empty() {
        body["instructions"] = json!(system);
    }
    if let Some(k) = &req.cache_key {
        body["prompt_cache_key"] = json!(k);
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "type": "function", "name": t.name, "description": t.description, "parameters": t.input_schema, "strict": false })).collect());
        if req.no_tools {
            body["tool_choice"] = json!("none");
        }
    }
    match (spec.thinking, &req.effort) {
        (_, Some(e)) => body["reasoning"] = json!({ "summary": "auto", "effort": e }),
        (Thinking::Switch, None) => body["reasoning"] = json!({ "effort": "none" }),
        (Thinking::Always, None) => body["reasoning"] = json!({ "summary": "auto" }),
        (Thinking::Never, None) => {}
    }
    body
}

fn user_items(m: &Message, input: &mut Vec<Value>) {
    let mut texts = Vec::new();
    for part in &m.parts {
        match part {
            Part::ToolResult { call_id, output, .. } => input.push(json!({ "type": "function_call_output", "call_id": call_id, "output": output })),
            Part::Text { text } if !text.is_empty() => texts.push(json!({ "type": "input_text", "text": text })),
            _ => {}
        }
    }
    if !texts.is_empty() {
        input.push(json!({ "type": "message", "role": "user", "content": texts }));
    }
}

fn assistant_items(m: &Message, origin: &Origin, input: &mut Vec<Value>) {
    for part in &m.parts {
        match part {
            Part::Reasoning { opaque, .. } => {
                let Some(data) = opaque.as_ref().and_then(|o| o.for_origin(origin)) else { continue };
                let mut item = json!({ "type": "reasoning", "summary": data.get("summary").cloned().filter(Value::is_array).unwrap_or(json!([])) });
                if let Some(secret) = data.get("encrypted_content").filter(|v| v.is_string()) {
                    item["encrypted_content"] = secret.clone();
                }
                input.push(item);
            }
            Part::Text { text } if !text.is_empty() => input.push(json!({ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": text }] })),
            Part::ToolCall { call_id, name, input: args, .. } => input.push(json!({ "type": "function_call", "call_id": call_id, "name": name, "arguments": args.to_string() })),
            Part::Native { opaque } => {
                if let Some(d) = opaque.for_origin(origin) {
                    input.push(d.clone());
                }
            }
            _ => {}
        }
    }
}

pub struct ResponsesStream {
    origin: Origin,
    items: BTreeMap<u64, Value>,
    output: Option<Vec<Value>>,
    tokens: Tokens,
    status: Option<String>,
    incomplete: Option<String>,
    model: Option<String>,
    request_id: Option<String>,
    error: Option<String>,
    chars: usize,
}

impl ResponsesStream {
    pub fn new(origin: Origin, request_id: Option<String>) -> Self {
        ResponsesStream { origin, items: BTreeMap::new(), output: None, tokens: Tokens::default(), status: None, incomplete: None, model: None, request_id, error: None, chars: 0 }
    }

    fn take_response(&mut self, r: &Value) {
        if let Some(m) = r["model"].as_str() {
            self.model = Some(m.into());
        }
        if let Some(id) = r["id"].as_str().filter(|_| self.request_id.is_none()) {
            self.request_id = Some(id.into());
        }
        if let Some(o) = r["output"].as_array().filter(|o| !o.is_empty()) {
            self.output = Some(o.clone());
        }
        let u = &r["usage"];
        if u.is_object() {
            // input_tokens 里含缓存读和缓存写，拆开记
            let d = &u["input_tokens_details"];
            let cached = d["cached_tokens"].as_u64().unwrap_or(0);
            let written = d["cache_write_tokens"].as_u64().unwrap_or(0);
            self.tokens = Tokens { input: u["input_tokens"].as_u64().unwrap_or(0).saturating_sub(cached + written), output: u["output_tokens"].as_u64().unwrap_or(0), cache_read: cached, cache_write: written };
        }
    }
}

fn error_text(e: &Value) -> String {
    let m = e["message"].as_str().or_else(|| e["error"]["message"].as_str());
    match m {
        Some(m) => format!("OpenAI 在回答中途报错：{}", super::short(m, 160)),
        None => "OpenAI 在回答中途报错。".into(),
    }
}

impl WireStream for ResponsesStream {
    fn feed(&mut self, data: &Value) -> Vec<Delta> {
        let delta = |d: &Value| d["delta"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
        match data["type"].as_str().unwrap_or("") {
            "response.created" | "response.in_progress" => {
                self.take_response(&data["response"]);
                vec![]
            }
            "response.output_item.added" => {
                let item = &data["item"];
                if item["type"] == "function_call" {
                    return vec![Delta::ToolCall { call_id: item["call_id"].as_str().unwrap_or("").into(), name: item["name"].as_str().unwrap_or("").into() }];
                }
                vec![]
            }
            "response.output_text.delta" => delta(data).map(|t| {
                self.chars += t.chars().count();
                vec![Delta::Text(t)]
            }).unwrap_or_default(),
            "response.reasoning_summary_text.delta" => delta(data).map(|t| {
                self.chars += t.chars().count();
                vec![Delta::Reasoning(t)]
            }).unwrap_or_default(),
            "response.function_call_arguments.delta" => {
                self.chars += data["delta"].as_str().map(|t| t.chars().count()).unwrap_or(0);
                vec![]
            }
            "response.output_item.done" => {
                let index = data["output_index"].as_u64().unwrap_or(self.items.len() as u64);
                self.items.insert(index, data["item"].clone());
                vec![]
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                let r = &data["response"];
                self.take_response(r);
                self.status = r["status"].as_str().map(str::to_string).or_else(|| data["type"].as_str().map(|t| t.trim_start_matches("response.").to_string()));
                self.incomplete = r["incomplete_details"]["reason"].as_str().map(str::to_string);
                if data["type"] == "response.failed" {
                    self.error = Some(error_text(&r["error"]));
                }
                vec![]
            }
            "error" => {
                self.error = Some(error_text(data));
                vec![]
            }
            _ => vec![],
        }
    }

    fn done(&self) -> bool {
        self.status.is_some() || self.error.is_some()
    }

    fn chars(&self) -> usize {
        self.chars
    }

    fn finish(self: Box<Self>) -> Result<Reply, String> {
        let this = *self;
        if let Some(e) = this.error {
            return Err(e);
        }
        let Some(status) = this.status else { return Err("回答没有收完就断开了。".into()) };
        let output = this.output.unwrap_or_else(|| this.items.into_values().collect());
        let mut parts = Vec::new();
        let mut refused = false;
        for item in &output {
            match item["type"].as_str().unwrap_or("") {
                "reasoning" => {
                    let summary = item["summary"].as_array().cloned().unwrap_or_default();
                    let text = summary.iter().filter_map(|s| s["text"].as_str()).collect::<Vec<_>>().join("\n\n");
                    let mut data = json!({ "type": "reasoning", "summary": summary });
                    if let Some(secret) = item.get("encrypted_content").filter(|v| v.is_string()) {
                        data["encrypted_content"] = secret.clone();
                    }
                    parts.push(Part::Reasoning { text, opaque: Some(Opaque { origin: this.origin.clone(), data }) });
                }
                "message" => {
                    for c in item["content"].as_array().into_iter().flatten() {
                        match c["type"].as_str().unwrap_or("") {
                            "output_text" => parts.push(Part::Text { text: c["text"].as_str().unwrap_or("").into() }),
                            "refusal" => {
                                refused = true;
                                parts.push(Part::Text { text: c["refusal"].as_str().unwrap_or("").into() });
                            }
                            _ => {}
                        }
                    }
                }
                "function_call" => parts.push(Part::ToolCall {
                    call_id: item["call_id"].as_str().unwrap_or("").into(),
                    name: item["name"].as_str().unwrap_or("").into(),
                    input: parse_tool_input(item["arguments"].as_str().unwrap_or("")),
                    signature: None,
                }),
                _ => {}
            }
        }
        let stop = if refused {
            Stop::Refusal
        } else if status == "incomplete" {
            match this.incomplete.as_deref() {
                Some("max_output_tokens") => Stop::MaxTokens,
                Some("content_filter") => Stop::Refusal,
                Some(other) => Stop::Other(other.into()),
                None => Stop::Other("incomplete".into()),
            }
        } else if parts.iter().any(|p| matches!(p, Part::ToolCall { .. })) {
            Stop::ToolUse
        } else {
            Stop::EndTurn
        };
        let model = this.model.unwrap_or_else(|| this.origin.model.clone());
        Ok(Reply { message: Message { role: Role::Assistant, parts }, stop, tokens: this.tokens, model, request_id: this.request_id, refusal: refused.then(|| "OpenAI 拒绝回答".to_string()) })
    }
}

pub fn error_message(status: u16, body: &Value) -> String {
    let detail = body.pointer("/error/message").and_then(Value::as_str).map(|m| format!("（{}）", super::short(m, 200))).unwrap_or_default();
    match status {
        401 => "OpenAI 的 API 密钥不对或已失效。到「设置 › 模型与费用」重新填写。".into(),
        402 => "OpenAI 账户余额不足。到 platform.openai.com 的 Billing 里处理后再试。".into(),
        403 => format!("这个密钥没有权限调用这个模型{detail}。"),
        404 => format!("找不到这个模型{detail}。"),
        429 if detail.contains("quota") => format!("OpenAI 的额度用完了{detail}。"),
        429 => "调用太频繁，被限流了。等一两分钟再试。".into(),
        s if s >= 500 => format!("OpenAI 的服务出错（{s}）。等一会儿再试。"),
        _ => format!("请求没有被接受{detail}。"),
    }
}
