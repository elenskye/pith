//! 调用 Claude（Anthropic Messages API，`decisions/0018`）。Rust 没有官方 SDK，直接发 HTTPS 请求：
//! 只连 `api.anthropic.com` 这一个固定地址，流式接收（长回答不会撞上超时），输出用结构化输出（JSON Schema）约束。
//!
//! 模型只返回结构化数据；读回来以后还要过 Rust 这边的解析和核对（`extract`、`planner`），不直接执行。
//! `Model` 是接口：正式用 `Anthropic`，开发版、测试和指定了数据目录的打包版用 `Mock`（不联网、不花钱）。

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::usage::{self, Tokens};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";

pub struct Request {
    /// extract / plan：记进账本
    pub purpose: &'static str,
    pub system: String,
    /// 用户消息的各段文字（要求文档、论文摘要等）
    pub user: Vec<String>,
    /// 输出的 JSON Schema
    pub schema: Value,
    pub max_tokens: u32,
    /// low / medium / high / xhigh
    pub effort: &'static str,
    /// 模拟模型直接返回这段（由调用方按输入确定地算出）
    pub mock_reply: Option<String>,
}

impl Request {
    /// 发送的文字总量（估计 token 用）
    pub fn text_len_estimate(&self) -> u64 {
        usage::estimate_tokens(&self.system) + self.user.iter().map(|u| usage::estimate_tokens(u)).sum::<u64>() + usage::estimate_tokens(&self.schema.to_string())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub model: String,
    pub tokens: Tokens,
    pub stop_reason: String,
    pub request_id: Option<String>,
}

/// 调用失败。`spent`：已经产生费用但拿不到确切用量时的估计（中断、流中断开），记进账本
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub message: String,
    pub spent: Option<Tokens>,
    pub cancelled: bool,
}

impl Failure {
    fn plain(message: impl Into<String>) -> Self {
        Failure { message: message.into(), spent: None, cancelled: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    /// 已收到的回答字数（思考过程不显示，只算字数）
    pub thinking_chars: usize,
    pub output_chars: usize,
}

pub trait Model: Send + Sync {
    fn is_mock(&self) -> bool;
    fn complete(&self, key: &str, req: &Request, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<Reply, Failure>;
}

// ---------- 流（SSE）解析：纯函数，测试用录好的文本 ----------

#[derive(Default)]
struct Stream {
    reply: Reply,
    thinking_chars: usize,
    error: Option<(String, String)>,
    done: bool,
}

impl Stream {
    fn usage(&mut self, u: &Value) {
        let get = |k: &str| u.get(k).and_then(Value::as_u64);
        let t = &mut self.reply.tokens;
        // message_start 给输入量，message_delta 给累计的输出量；同一字段取大的
        if let Some(v) = get("input_tokens") {
            t.input = t.input.max(v);
        }
        if let Some(v) = get("output_tokens") {
            t.output = t.output.max(v);
        }
        if let Some(v) = get("cache_read_input_tokens") {
            t.cache_read = t.cache_read.max(v);
        }
        if let Some(v) = get("cache_creation_input_tokens") {
            t.cache_write = t.cache_write.max(v);
        }
    }

    fn feed(&mut self, data: &str) {
        let Ok(v) = serde_json::from_str::<Value>(data) else { return };
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                let m = &v["message"];
                if let Some(model) = m.get("model").and_then(Value::as_str) {
                    self.reply.model = model.to_string();
                }
                self.usage(&m["usage"]);
            }
            "content_block_start" => {
                let b = &v["content_block"];
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    self.reply.text.push_str(b.get("text").and_then(Value::as_str).unwrap_or(""));
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d.get("type").and_then(Value::as_str) {
                    Some("text_delta") => self.reply.text.push_str(d.get("text").and_then(Value::as_str).unwrap_or("")),
                    Some("thinking_delta") => self.thinking_chars += d.get("thinking").and_then(Value::as_str).map(|s| s.chars().count()).unwrap_or(0),
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(s) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.reply.stop_reason = s.to_string();
                }
                self.usage(&v["usage"]);
            }
            "message_stop" => self.done = true,
            "error" => {
                let e = &v["error"];
                self.error = Some((e.get("type").and_then(Value::as_str).unwrap_or("error").to_string(), e.get("message").and_then(Value::as_str).unwrap_or("").to_string()));
            }
            _ => {}
        }
    }
}

/// 读完一个流。`Err` 里带已经花掉的估计量。
fn read_stream(reader: impl Read, req_input_estimate: u64, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<Reply, Failure> {
    let mut s = Stream::default();
    // 拿不到确切用量时的估计：输入用接口给的（没给就用发送前的估计），输出按已收到的字数往高里估
    let spent = |s: &Stream| {
        let out_chars = (s.reply.text.chars().count() + s.thinking_chars) as u64;
        Tokens { input: s.reply.tokens.input.max(req_input_estimate), output: s.reply.tokens.output.max(out_chars * 3 / 2 + 200), ..s.reply.tokens }
    };
    let mut last_report = (0usize, 0usize);
    for line in BufReader::new(reader).lines() {
        if cancel.load(Ordering::SeqCst) {
            return Err(Failure { message: "已停止。已经生成的部分仍会计费，按估计记进了用量。".into(), spent: Some(spent(&s)), cancelled: true });
        }
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err(Failure { message: format!("接收回答时连接断了：{e}。已经生成的部分按估计记进了用量，可以重试。"), spent: Some(spent(&s)), cancelled: false }),
        };
        let Some(data) = line.strip_prefix("data:") else { continue };
        s.feed(data.trim_start());
        if let Some((kind, msg)) = &s.error {
            return Err(Failure { message: api_error_message(0, kind, msg), spent: Some(spent(&s)), cancelled: false });
        }
        let now = (s.thinking_chars, s.reply.text.chars().count());
        if now.0 + now.1 >= last_report.0 + last_report.1 + 200 {
            last_report = now;
            progress(Progress { thinking_chars: now.0, output_chars: now.1 });
        }
        if s.done {
            break;
        }
    }
    if !s.done {
        return Err(Failure { message: "回答没有收完就断开了。已经生成的部分按估计记进了用量，可以重试。".into(), spent: Some(spent(&s)), cancelled: false });
    }
    Ok(s.reply)
}

/// 接口错误换成给用户看的中文（原因 + 怎么办），不出现原始报错里的密钥等内容
fn api_error_message(status: u16, kind: &str, message: &str) -> String {
    let detail = if message.is_empty() { String::new() } else { format!("（{}）", message.chars().take(200).collect::<String>()) };
    match (status, kind) {
        (401, _) | (_, "authentication_error") => "API 密钥不对或已失效。到「设置 › 密钥与费用」重新填写。".into(),
        (402, _) | (_, "billing_error") => "Anthropic 账户余额不足或没有绑定付款方式。到 console.anthropic.com 的 Billing 里处理后再试。".into(),
        (403, _) | (_, "permission_error") => format!("这个密钥没有权限调用这个模型{detail}。"),
        (404, _) | (_, "not_found_error") => format!("找不到模型 {}{detail}。", usage::MODEL),
        (413, _) | (_, "request_too_large") => "发送的内容太大。换成「结构和片段」方式，或减少要求文档。".into(),
        (429, _) | (_, "rate_limit_error") => "调用太频繁，被限流了。等一两分钟再试。".into(),
        (529, _) | (_, "overloaded_error") => "Anthropic 的服务现在过载。等一会儿再试。".into(),
        (s, _) if s >= 500 => format!("Anthropic 的服务出错（{s}）。等一会儿再试。"),
        _ => format!("请求没有被接受{detail}。"),
    }
}

fn retryable(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
}

// ---------- 正式实现 ----------

pub struct Anthropic {
    agent: ureq::Agent,
}

impl Default for Anthropic {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(300)))
            // 整个回答最长等 30 分钟（思考 + 输出）
            .timeout_recv_body(Some(Duration::from_secs(1800)))
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(crate::metadata::net::APP_UA)
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .build()
            .into();
        Anthropic { agent }
    }
}

/// 不带拒答后备（用户 2026-11-11 定）：模型拒答就直接报错，让用户重试；只用 `decisions/0015` 定的那一个模型。
pub fn body(req: &Request) -> Value {
    json!({
        "model": usage::MODEL,
        "max_tokens": req.max_tokens,
        "stream": true,
        // Opus 5.5 的思考不能关，深度由 effort 控制；提炼和计划都是要准确的活，用 high。
        // 思考默认不回传文字，界面会几分钟没有动静：要它回传摘要（只用来显示进度的字数，费用不变）
        "thinking": { "type": "adaptive", "display": "summarized" },
        "output_config": { "effort": req.effort, "format": { "type": "json_schema", "schema": req.schema } },
        "system": req.system,
        "messages": [{ "role": "user", "content": req.user.iter().map(|t| json!({ "type": "text", "text": t })).collect::<Vec<_>>() }],
    })
}

impl Anthropic {
    fn post(&self, key: &str, req: &Request) -> Result<ureq::http::Response<ureq::Body>, Failure> {
        let r = self.agent.post(ENDPOINT).header("x-api-key", key).header("anthropic-version", API_VERSION).header("content-type", "application/json").header("accept", "text/event-stream");
        r.send(body(req).to_string()).map_err(|e| match e {
            ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => Failure::plain(format!("连不上 Anthropic（{e}）。检查网络后重试；这次没有产生费用。")),
            other => Failure::plain(format!("请求没发出去：{other}。这次没有产生费用。")),
        })
    }
}

impl Model for Anthropic {
    fn is_mock(&self) -> bool {
        false
    }

    fn complete(&self, key: &str, req: &Request, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<Reply, Failure> {
        let waits = [2u64, 6, 15];
        let mut attempt = 0;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Failure { message: "已停止，没有产生费用。".into(), spent: None, cancelled: true });
            }
            let mut resp = self.post(key, req)?;
            let status = resp.status().as_u16();
            let request_id = resp.headers().get("request-id").and_then(|v| v.to_str().ok()).map(String::from);
            if status == 200 {
                let mut reply = read_stream(resp.body_mut().as_reader(), req.text_len_estimate(), progress, cancel)?;
                reply.request_id = request_id;
                return Ok(reply);
            }
            let retry_after: Option<u64> = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok());
            let text = resp.body_mut().with_config().limit(256 * 1024).read_to_string().unwrap_or_default();
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let kind = v.pointer("/error/type").and_then(Value::as_str).unwrap_or("");
            let message = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
            if retryable(status) && attempt < waits.len() {
                let wait = retry_after.unwrap_or(waits[attempt]).min(30);
                attempt += 1;
                for _ in 0..wait * 10 {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(Failure { message: "已停止，没有产生费用。".into(), spent: None, cancelled: true });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
            return Err(Failure::plain(format!("{}这次没有产生费用。", api_error_message(status, kind, message))));
        }
    }
}

// ---------- 模拟模型 ----------

/// 不联网、不花钱：返回调用方按输入算好的结果，用量是编的（标成模拟，不计入预算）
#[derive(Default)]
pub struct Mock {
    /// 测试里让它失败
    pub fail: Option<Failure>,
    pub delay: Duration,
}

impl Model for Mock {
    fn is_mock(&self) -> bool {
        true
    }

    fn complete(&self, _key: &str, req: &Request, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<Reply, Failure> {
        if let Some(f) = &self.fail {
            return Err(f.clone());
        }
        let text = req.mock_reply.clone().ok_or_else(|| Failure::plain("模拟模型没有准备这类回答"))?;
        let total = text.chars().count();
        for step in 1..=4 {
            if cancel.load(Ordering::SeqCst) {
                return Err(Failure { message: "已停止（模拟模型，没有费用）。".into(), spent: None, cancelled: true });
            }
            std::thread::sleep(self.delay / 4);
            progress(Progress { thinking_chars: 300 * step, output_chars: total * step / 4 });
        }
        let tokens = Tokens { input: req.text_len_estimate(), output: usage::estimate_tokens(&text) + 400, cache_read: 0, cache_write: 0 };
        Ok(Reply { text, model: "模拟模型".into(), tokens, stop_reason: "end_turn".into(), request_id: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDED: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"usage":{"input_tokens":1200,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: ping
data: {"type":"ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"先看页面设置"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"{\"name\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"\"规范\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":345}}

event: message_stop
data: {"type":"message_stop"}
"#;

    fn quiet(_: Progress) {}

    #[test]
    fn reads_a_stream() {
        let r = read_stream(RECORDED.as_bytes(), 0, &quiet, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.text, r#"{"name":"规范"}"#);
        assert_eq!(r.model, "claude-opus-5-5");
        assert_eq!(r.tokens, Tokens { input: 1200, output: 345, cache_read: 0, cache_write: 0 });
        assert_eq!(r.stop_reason, "end_turn");
    }

    #[test]
    fn cut_off_stream_reports_what_was_spent() {
        let cut = RECORDED.split("event: message_delta").next().unwrap();
        let f = read_stream(cut.as_bytes(), 1500, &quiet, &AtomicBool::new(false)).unwrap_err();
        assert!(f.message.contains("没有收完") && !f.cancelled);
        let spent = f.spent.unwrap();
        // 输入取接口给的和发送前估计里大的；输出按已收到的字数往高里估
        assert_eq!(spent.input, 1500);
        assert!(spent.output >= 200);
    }

    #[test]
    fn stop_and_stream_errors() {
        let f = read_stream(RECORDED.as_bytes(), 0, &quiet, &AtomicBool::new(true)).unwrap_err();
        assert!(f.cancelled && f.spent.is_some());
        let err = "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":10}}}\n\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n";
        let f = read_stream(err.as_bytes(), 0, &quiet, &AtomicBool::new(false)).unwrap_err();
        assert!(f.message.contains("过载"), "{}", f.message);
    }

    #[test]
    fn request_body_shape() {
        let req = Request { purpose: "extract", system: "系统".into(), user: vec!["甲".into(), "乙".into()], schema: json!({ "type": "object" }), max_tokens: 32000, effort: "high", mock_reply: None };
        let b = body(&req);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["stream"], true);
        assert_eq!(b["output_config"]["effort"], "high");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["messages"][0]["content"].as_array().unwrap().len(), 2);
        // 思考不能关也不设预算；不带采样参数
        assert_eq!(b["thinking"], json!({ "type": "adaptive", "display": "summarized" }));
        for gone in ["temperature", "top_p", "tool_choice"] {
            assert!(b.get(gone).is_none(), "{gone}");
        }
        // 不带拒答后备：只用定下的那一个模型
        assert!(b.get("fallbacks").is_none());
    }

    #[test]
    fn errors_are_explained_in_chinese_without_echoing_secrets() {
        assert!(api_error_message(401, "authentication_error", "invalid x-api-key").contains("重新填写"));
        assert!(api_error_message(429, "rate_limit_error", "").contains("限流"));
        assert!(api_error_message(400, "invalid_request_error", "max_tokens: too large").contains("max_tokens"));
        assert!(!api_error_message(401, "authentication_error", "key sk-ant-xxx is invalid").contains("sk-ant"));
    }

    #[test]
    fn mock_returns_the_prepared_reply() {
        let req = Request { purpose: "plan", system: String::new(), user: vec![], schema: json!({}), max_tokens: 100, effort: "high", mock_reply: Some("{}".into()) };
        let r = Mock::default().complete("", &req, &quiet, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.text, "{}");
        assert!(Mock::default().complete("", &Request { mock_reply: None, ..req }, &quiet, &AtomicBool::new(false)).is_err());
    }
}
