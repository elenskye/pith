//! DeepSeek 的 Chat Completions（流式 SSE），照 Peck 的 `convert/chat.rs` 改的。
//!
//! 思考开关是 `thinking: {type}`、强度 `reasoning_effort`；调用工具的那一轮 `reasoning_content` 要原样放回（来源一样才放）。
//! 缓存是自动的（硬盘缓存），命中数在 `prompt_cache_hit_tokens`。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::catalog::ModelSpec;
use super::{Delta, Message, Opaque, Origin, Part, Reply, Request, Role, Stop, Tokens, WireStream, parse_tool_input, system_text};

pub const URL: &str = "https://api.deepseek.com/chat/completions";

pub fn encode(req: &Request, spec: &ModelSpec, origin: &Origin) -> Value {
    let mut messages = Vec::new();
    let system = system_text(req);
    if !system.is_empty() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for m in &req.messages {
        match m.role {
            Role::User => {
                let mut texts = Vec::new();
                for part in &m.parts {
                    match part {
                        Part::ToolResult { call_id, output, .. } => messages.push(json!({ "role": "tool", "tool_call_id": call_id, "content": output })),
                        Part::Text { text } if !text.is_empty() => texts.push(text.as_str()),
                        _ => {}
                    }
                }
                if !texts.is_empty() {
                    messages.push(json!({ "role": "user", "content": texts.join("\n\n") }));
                }
            }
            Role::Assistant => messages.push(assistant_message(m, origin)),
        }
    }
    let mut body = json!({
        "model": spec.api_model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": req.max_output_tokens,
    });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.input_schema } })).collect());
        if req.no_tools {
            body["tool_choice"] = json!("none");
        }
    }
    match &req.effort {
        Some(e) => {
            body["thinking"] = json!({ "type": "enabled" });
            body["reasoning_effort"] = json!(e);
        }
        None => body["thinking"] = json!({ "type": "disabled" }),
    }
    body
}

fn assistant_message(m: &Message, origin: &Origin) -> Value {
    let (mut text, mut reasoning, mut calls) = (String::new(), String::new(), Vec::new());
    for part in &m.parts {
        match part {
            Part::Text { text: t } => text.push_str(t),
            Part::Reasoning { text: t, opaque } => {
                if opaque.as_ref().is_some_and(|o| o.origin == *origin) {
                    reasoning.push_str(t);
                }
            }
            Part::ToolCall { call_id, name, input, .. } => calls.push(json!({ "id": call_id, "type": "function", "function": { "name": name, "arguments": input.to_string() } })),
            _ => {}
        }
    }
    let mut out = json!({ "role": "assistant", "content": text });
    if !reasoning.is_empty() {
        out["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        out["tool_calls"] = Value::Array(calls);
    }
    out
}

struct Call {
    id: String,
    name: String,
    arguments: String,
    announced: bool,
}

pub struct ChatStream {
    origin: Origin,
    text: String,
    reasoning: String,
    calls: BTreeMap<u64, Call>,
    tokens: Tokens,
    finish: Option<String>,
    model: Option<String>,
    request_id: Option<String>,
    error: Option<String>,
    chars: usize,
}

impl ChatStream {
    pub fn new(origin: Origin, request_id: Option<String>) -> Self {
        ChatStream { origin, text: String::new(), reasoning: String::new(), calls: BTreeMap::new(), tokens: Tokens::default(), finish: None, model: None, request_id, error: None, chars: 0 }
    }
}

impl WireStream for ChatStream {
    fn feed(&mut self, data: &Value) -> Vec<Delta> {
        if let Some(e) = data.get("error").filter(|e| !e.is_null()) {
            self.error = Some(format!("DeepSeek 在回答中途报错：{}", super::short(e["message"].as_str().unwrap_or(""), 160)));
            return vec![];
        }
        if let Some(m) = data["model"].as_str() {
            self.model = Some(m.into());
        }
        if let Some(id) = data["id"].as_str().filter(|_| self.request_id.is_none()) {
            self.request_id = Some(id.into());
        }
        let u = &data["usage"];
        if u.is_object() {
            let hit = u["prompt_cache_hit_tokens"].as_u64().unwrap_or(0);
            let input = u["prompt_cache_miss_tokens"].as_u64().unwrap_or_else(|| u["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(hit));
            self.tokens = Tokens { input, output: u["completion_tokens"].as_u64().unwrap_or(0), cache_read: hit, cache_write: 0 };
        }
        let mut out = Vec::new();
        for choice in data["choices"].as_array().into_iter().flatten() {
            let d = &choice["delta"];
            if let Some(t) = d["reasoning_content"].as_str().filter(|t| !t.is_empty()) {
                self.reasoning.push_str(t);
                self.chars += t.chars().count();
                out.push(Delta::Reasoning(t.into()));
            }
            if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
                self.text.push_str(t);
                self.chars += t.chars().count();
                out.push(Delta::Text(t.into()));
            }
            for (n, call) in d["tool_calls"].as_array().into_iter().flatten().enumerate() {
                let index = call["index"].as_u64().unwrap_or(n as u64);
                let entry = self.calls.entry(index).or_insert_with(|| Call { id: String::new(), name: String::new(), arguments: String::new(), announced: false });
                if let Some(id) = call["id"].as_str().filter(|s| !s.is_empty()) {
                    entry.id = id.into();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    entry.name.push_str(name);
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    self.chars += args.chars().count();
                    entry.arguments.push_str(args);
                }
                if !entry.announced && !entry.id.is_empty() && !entry.name.is_empty() {
                    entry.announced = true;
                    out.push(Delta::ToolCall { call_id: entry.id.clone(), name: entry.name.clone() });
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                self.finish = Some(f.into());
            }
        }
        out
    }

    /// Chat Completions 以 `data: [DONE]` 收尾（`http` 认得它），用量在最后一块，所以这里不提前结束
    fn done(&self) -> bool {
        self.error.is_some()
    }

    fn chars(&self) -> usize {
        self.chars
    }

    fn finish(self: Box<Self>) -> Result<Reply, String> {
        let this = *self;
        if let Some(e) = this.error {
            return Err(e);
        }
        let Some(finish) = this.finish else { return Err("回答没有收完就断开了。".into()) };
        let mut parts = Vec::new();
        if !this.reasoning.is_empty() {
            parts.push(Part::Reasoning { text: this.reasoning, opaque: Some(Opaque { origin: this.origin.clone(), data: json!({ "type": "reasoning_content" }) }) });
        }
        if !this.text.is_empty() {
            parts.push(Part::Text { text: this.text });
        }
        for call in this.calls.into_values() {
            if call.id.is_empty() || call.name.is_empty() {
                return Err("工具调用的数据不完整，没执行。".into());
            }
            parts.push(Part::ToolCall { call_id: call.id, name: call.name, input: parse_tool_input(&call.arguments), signature: None });
        }
        let stop = match finish.as_str() {
            "stop" => Stop::EndTurn,
            "tool_calls" | "function_call" => Stop::ToolUse,
            "length" => Stop::MaxTokens,
            "content_filter" => Stop::Refusal,
            other => Stop::Other(other.into()),
        };
        let refusal = (stop == Stop::Refusal).then(|| "内容被过滤".to_string());
        let model = this.model.unwrap_or_else(|| this.origin.model.clone());
        Ok(Reply { message: Message { role: Role::Assistant, parts }, stop, tokens: this.tokens, model, request_id: this.request_id, refusal })
    }
}

pub fn error_message(status: u16, body: &Value) -> String {
    let detail = body.pointer("/error/message").and_then(Value::as_str).map(|m| format!("（{}）", super::short(m, 200))).unwrap_or_default();
    match status {
        401 => "DeepSeek 的 API 密钥不对或已失效。到「设置 › 模型与费用」重新填写。".into(),
        402 => "DeepSeek 账户余额不足。到 platform.deepseek.com 充值后再试。".into(),
        422 | 400 => format!("请求没有被接受{detail}。"),
        429 => "调用太频繁，被限流了。等一两分钟再试。".into(),
        s if s >= 500 => format!("DeepSeek 的服务出错（{s}）。等一会儿再试。"),
        _ => format!("请求没有被接受{detail}。"),
    }
}
