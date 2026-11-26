//! Gemini 原生 `streamGenerateContent?alt=sse`，照 Peck 的 `convert/gemini.rs` 改的；API Key 和 Vertex AI 共用，只有地址和认证不同。
//!
//! thought signature 是回放的关键：调用工具的那一段必须原样带回，不然 400。
//! - 挂在工具调用上的：放进 `Part::ToolCall.signature`；
//! - 挂在文字或思考上的：那一块后面跟一个空文字的 `Part::Reasoning`，私有数据 `{"thoughtSignature": …, "on": "previous"}`，回放时贴回前一块。
//! 流里一块常被拆成好几段（同一种文字接着来），这里拼回去。Gemini 不总给工具调用 id，没给的编一个 `pith-call-<n>`，回放时不发。
//! 缓存是隐式的（同样的前缀自动命中），命中数在 `cachedContentTokenCount`。

use std::collections::HashMap;

use serde_json::{Value, json};

use super::catalog::ModelSpec;
use super::{Delta, Message, Opaque, Origin, Part, Reply, Request, Role, Stop, Tokens, WireStream, system_text};

pub const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const MADE_UP_ID: &str = "pith-call-";

pub fn url(model: &str) -> String {
    format!("{BASE}/models/{model}:streamGenerateContent?alt=sse")
}

pub fn encode(req: &Request, _spec: &ModelSpec, origin: &Origin) -> Value {
    let names: HashMap<String, String> = req.messages.iter().flat_map(|m| &m.parts).filter_map(|p| if let Part::ToolCall { call_id, name, .. } = p { Some((call_id.clone(), name.clone())) } else { None }).collect();
    let mut contents = Vec::new();
    for m in &req.messages {
        let parts = match m.role {
            Role::User => user_parts(m, &names),
            Role::Assistant => model_parts(m, origin),
        };
        if !parts.is_empty() {
            contents.push(json!({ "role": if m.role == Role::User { "user" } else { "model" }, "parts": parts }));
        }
    }
    let mut thinking = json!({ "includeThoughts": true });
    if let Some(e) = &req.effort {
        thinking["thinkingLevel"] = json!(e);
    }
    let mut body = json!({ "contents": contents, "generationConfig": { "maxOutputTokens": req.max_output_tokens, "thinkingConfig": thinking } });
    let system = system_text(req);
    if !system.is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    if !req.tools.is_empty() {
        let decls: Vec<Value> = req.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "parametersJsonSchema": t.input_schema })).collect();
        body["tools"] = json!([{ "functionDeclarations": decls }]);
        if req.no_tools {
            body["toolConfig"] = json!({ "functionCallingConfig": { "mode": "NONE" } });
        }
    }
    body
}

fn user_parts(m: &Message, names: &HashMap<String, String>) -> Vec<Value> {
    let (mut results, mut texts) = (Vec::new(), Vec::new());
    for part in &m.parts {
        match part {
            Part::ToolResult { call_id, output, is_error } => {
                let response = if *is_error { json!({ "error": output }) } else { json!({ "output": output }) };
                let mut r = json!({ "name": names.get(call_id).cloned().unwrap_or_default(), "response": response });
                if !call_id.starts_with(MADE_UP_ID) {
                    r["id"] = json!(call_id);
                }
                results.push(json!({ "functionResponse": r }));
            }
            Part::Text { text } if !text.is_empty() => texts.push(json!({ "text": text })),
            _ => {}
        }
    }
    results.extend(texts);
    results
}

fn model_parts(m: &Message, origin: &Origin) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for part in &m.parts {
        match part {
            Part::Reasoning { text, opaque } => {
                let Some(data) = opaque.as_ref().and_then(|o| o.for_origin(origin)) else { continue };
                if data["on"] == "previous" {
                    if let (Some(last), Some(sig)) = (out.last_mut(), data.get("thoughtSignature")) {
                        last["thoughtSignature"] = sig.clone();
                    }
                    continue;
                }
                let mut p = json!({ "text": text, "thought": true });
                if let Some(sig) = data.get("thoughtSignature") {
                    p["thoughtSignature"] = sig.clone();
                }
                out.push(p);
            }
            Part::Text { text } if !text.is_empty() => out.push(json!({ "text": text })),
            Part::ToolCall { call_id, name, input, signature } => {
                let mut call = json!({ "name": name, "args": input });
                if !call_id.starts_with(MADE_UP_ID) {
                    call["id"] = json!(call_id);
                }
                let mut p = json!({ "functionCall": call });
                if let Some(sig) = signature.as_ref().and_then(|o| o.for_origin(origin)).and_then(|d| d.get("thoughtSignature")) {
                    p["thoughtSignature"] = sig.clone();
                }
                out.push(p);
            }
            Part::Native { opaque } => {
                if let Some(d) = opaque.for_origin(origin) {
                    out.push(d.clone());
                }
            }
            _ => {}
        }
    }
    out
}

enum Piece {
    Thought { text: String, signature: Option<String> },
    Text { text: String, signature: Option<String> },
    Call { id: String, name: String, args: Value, signature: Option<String> },
}

pub struct GeminiStream {
    origin: Origin,
    pieces: Vec<Piece>,
    tokens: Tokens,
    finish: Option<String>,
    blocked: Option<String>,
    model: Option<String>,
    request_id: Option<String>,
    error: Option<String>,
    calls: usize,
    chars: usize,
}

impl GeminiStream {
    pub fn new(origin: Origin, request_id: Option<String>) -> Self {
        GeminiStream { origin, pieces: Vec::new(), tokens: Tokens::default(), finish: None, blocked: None, model: None, request_id, error: None, calls: 0, chars: 0 }
    }
}

fn signature_of(p: &Piece) -> Option<&String> {
    match p {
        Piece::Thought { signature, .. } | Piece::Text { signature, .. } | Piece::Call { signature, .. } => signature.as_ref(),
    }
}

fn set_signature(p: &mut Piece, sig: String) {
    match p {
        Piece::Thought { signature, .. } | Piece::Text { signature, .. } | Piece::Call { signature, .. } => *signature = Some(sig),
    }
}

impl WireStream for GeminiStream {
    fn feed(&mut self, data: &Value) -> Vec<Delta> {
        if let Some(e) = data.get("error").filter(|e| !e.is_null()) {
            self.error = Some(format!("Gemini 在回答中途报错：{}", super::short(e["message"].as_str().unwrap_or(""), 160)));
            return vec![];
        }
        if let Some(m) = data["modelVersion"].as_str() {
            self.model = Some(m.into());
        }
        if let Some(id) = data["responseId"].as_str().filter(|_| self.request_id.is_none()) {
            self.request_id = Some(id.into());
        }
        if let Some(r) = data["promptFeedback"]["blockReason"].as_str() {
            self.blocked = Some(r.into());
        }
        let u = &data["usageMetadata"];
        if u.is_object() {
            let cached = u["cachedContentTokenCount"].as_u64().unwrap_or(0);
            let thoughts = u["thoughtsTokenCount"].as_u64().unwrap_or(0);
            self.tokens = Tokens { input: u["promptTokenCount"].as_u64().unwrap_or(0).saturating_sub(cached), output: u["candidatesTokenCount"].as_u64().unwrap_or(0) + thoughts, cache_read: cached, cache_write: 0 };
        }
        let mut out = Vec::new();
        let candidate = &data["candidates"][0];
        for p in candidate["content"]["parts"].as_array().into_iter().flatten() {
            let signature = p["thoughtSignature"].as_str().map(str::to_string);
            if let Some(call) = p.get("functionCall") {
                self.calls += 1;
                let id = call["id"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{MADE_UP_ID}{}", self.calls));
                let name = call["name"].as_str().unwrap_or("").to_string();
                let args = call.get("args").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
                self.chars += args.to_string().chars().count();
                out.push(Delta::ToolCall { call_id: id.clone(), name: name.clone() });
                self.pieces.push(Piece::Call { id, name, args, signature });
                continue;
            }
            let Some(text) = p["text"].as_str() else {
                // 只带签名的空块：签名贴给前一块
                if let (Some(sig), Some(last)) = (signature, self.pieces.last_mut())
                    && signature_of(last).is_none()
                {
                    set_signature(last, sig);
                }
                continue;
            };
            let thought = p["thought"].as_bool().unwrap_or(false);
            if text.is_empty() && signature.is_none() {
                continue;
            }
            let extend = match self.pieces.last_mut() {
                Some(Piece::Thought { text: t, signature: None }) if thought => Some(t),
                Some(Piece::Text { text: t, signature: None }) if !thought => Some(t),
                _ => None,
            };
            match extend {
                Some(t) => t.push_str(text),
                None if text.is_empty() => {}
                None => self.pieces.push(if thought { Piece::Thought { text: text.into(), signature: None } } else { Piece::Text { text: text.into(), signature: None } }),
            }
            if let (Some(sig), Some(last)) = (signature, self.pieces.last_mut()) {
                set_signature(last, sig);
            }
            if !text.is_empty() {
                self.chars += text.chars().count();
                out.push(if thought { Delta::Reasoning(text.into()) } else { Delta::Text(text.into()) });
            }
        }
        if let Some(f) = candidate["finishReason"].as_str() {
            self.finish = Some(f.into());
        }
        out
    }

    /// Gemini 的流在连接关掉时结束（最后一块带 finishReason 和用量）
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
        if this.finish.is_none() && this.blocked.is_none() {
            return Err("回答没有收完就断开了。".into());
        }
        let opaque = |data: Value| Some(Opaque { origin: this.origin.clone(), data });
        let mut parts = Vec::new();
        let mut has_call = false;
        for piece in this.pieces {
            match piece {
                Piece::Thought { text, signature } => parts.push(Part::Reasoning { text, opaque: opaque(signature.map(|s| json!({ "thoughtSignature": s })).unwrap_or(json!({}))) }),
                Piece::Text { text, signature } => {
                    parts.push(Part::Text { text });
                    if let Some(sig) = signature {
                        parts.push(Part::Reasoning { text: String::new(), opaque: opaque(json!({ "thoughtSignature": sig, "on": "previous" })) });
                    }
                }
                Piece::Call { id, name, args, signature } => {
                    has_call = true;
                    parts.push(Part::ToolCall { call_id: id, name, input: args, signature: signature.and_then(|s| opaque(json!({ "thoughtSignature": s }))) });
                }
            }
        }
        let stop = if this.blocked.is_some() {
            Stop::Refusal
        } else {
            match this.finish.as_deref().unwrap_or("") {
                "STOP" if has_call => Stop::ToolUse,
                "STOP" | "FINISH_REASON_UNSPECIFIED" => Stop::EndTurn,
                "MAX_TOKENS" => Stop::MaxTokens,
                "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => Stop::Refusal,
                other => Stop::Other(other.to_lowercase()),
            }
        };
        let refusal = (stop == Stop::Refusal).then(|| this.blocked.clone().or(this.finish.clone()).unwrap_or_default());
        let model = this.model.unwrap_or_else(|| this.origin.model.clone());
        Ok(Reply { message: Message { role: Role::Assistant, parts }, stop, tokens: this.tokens, model, request_id: this.request_id, refusal })
    }
}

pub fn error_message(status: u16, body: &Value, vertex: bool) -> String {
    let who = if vertex { "Vertex AI" } else { "Gemini" };
    let detail = body.pointer("/error/message").or_else(|| body.pointer("/0/error/message")).and_then(Value::as_str).map(|m| format!("（{}）", super::short(m, 200))).unwrap_or_default();
    match status {
        401 if vertex => "Vertex AI 拒绝了认证：在终端跑一次 gcloud auth application-default login，再看项目有没有开 Vertex AI 和计费。".into(),
        403 if vertex => format!("这个项目没有权限调用这个模型{detail}：在 Google Cloud 控制台给项目开 Vertex AI API。"),
        400 if detail.contains("API key") => "Google 的 API Key 不对或已失效。到「设置 › 模型与费用」重新填写。".into(),
        401 | 403 => format!("{who} 的认证不对{detail}。到「设置 › 模型与费用」检查。"),
        404 => format!("找不到这个模型{detail}。"),
        429 => format!("{who} 调用太频繁或额度用完了{detail}。等一会儿再试。"),
        s if s >= 500 => format!("{who} 的服务出错（{s}）。等一会儿再试。"),
        _ => format!("请求没有被接受{detail}。"),
    }
}
