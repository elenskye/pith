//! OpenAI 兼容的 chat completions（先接 DeepSeek，`decisions/0020`）：读网页、摘证据这类执行工作交给便宜的模型。
//!
//! - 默认 `https://api.deepseek.com`，模型 `deepseek-flash`；地址和模型名在设置里可以改。
//! - 要 JSON 输出（`response_format: json_object`）。这类接口要求提示词里出现「json」这个词，没有就补一句。
//! - 用量从 `usage` 读：`prompt_tokens`、`completion_tokens`，缓存命中的（`prompt_cache_hit_tokens`）单独记下；
//!   费用按全部输入 token 的原价算（缓存命中其实更便宜），宁可多记。
//! - 不流式：摘证据的输出不长（上限 8000 token），整份读回。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use research_engine::model::ModelRef;
use research_engine::ports::{ErrorKind, Model, ModelReply, ModelRequest, PortError};
use serde_json::{Value, json};

use super::meter::{self, Meter};
use crate::agent::usage::{self, Tokens};

pub const PROVIDER: &str = "openai_compatible";
pub const DEFAULT_BASE: &str = "https://api.deepseek.com";
pub const DEFAULT_MODEL: &str = "deepseek-flash";

pub struct OpenAiCompatible {
    agent: ureq::Agent,
    base: String,
    key: String,
    model: String,
    meter: Arc<Meter>,
}

/// 这个接口叫什么（报错时用）：DeepSeek 的地址或模型就叫 DeepSeek
fn name_of(base: &str, model: &str) -> &'static str {
    if base.contains("deepseek") || model.starts_with("deepseek") { "DeepSeek" } else { "执行模型的接口" }
}

pub fn error_message(who: &str, status: u16, message: &str) -> String {
    let detail = meter::detail(message);
    match status {
        401 => format!("{who} 的 API 密钥不对或已失效。到「设置 › 事实调查」重新填写。"),
        402 => format!("{who} 账户余额不足。充值后点「继续」。"),
        403 => format!("这个密钥没有权限调用这个模型{detail}。"),
        404 => format!("找不到这个模型或接口地址{detail}。到「设置 › 事实调查」检查模型名和地址。"),
        400 | 422 => format!("{who} 不接受这次请求{detail}。"),
        429 => format!("{who} 说调用太频繁，被限流了。等一会儿再点「继续」。"),
        s if s >= 500 => format!("{who} 的服务出错（{s}）。等一会儿再点「继续」。"),
        s => format!("{who} 返回了 {s}{detail}。"),
    }
}

impl OpenAiCompatible {
    pub fn new(base: Option<&str>, key: String, model: Option<&str>, meter: Arc<Meter>) -> Self {
        let base = base.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(DEFAULT_BASE).trim_end_matches('/').to_string();
        let model = model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or(DEFAULT_MODEL).to_string();
        OpenAiCompatible { agent: meter::agent(Duration::from_secs(600)), base, key, model, meter }
    }

    pub fn body(&self, req: &ModelRequest) -> Value {
        let mut system = req.system.clone();
        if !system.to_lowercase().contains("json") {
            system.push_str("\n\n只输出一个 json 对象。");
        }
        json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": req.user.join("\n\n") },
            ],
            "response_format": { "type": "json_object" },
            "max_tokens": req.max_tokens,
            "stream": false,
        })
    }

    fn endpoint(&self) -> String {
        // 地址可以写到 /v1 也可以不写
        if self.base.ends_with("/chat/completions") { self.base.clone() } else { format!("{}/chat/completions", self.base) }
    }
}

impl Model for OpenAiCompatible {
    fn info(&self) -> ModelRef {
        ModelRef { provider: PROVIDER.into(), model: self.model.clone() }
    }

    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError> {
        let who = name_of(&self.base, &self.model);
        let worst = meter::worst_case(&self.meter.prices, &self.model, req, 0.0);
        self.meter.check(worst, "这次调用")?;
        let body = self.body(req);
        let auth = format!("Bearer {}", self.key);
        let mut attempt = 0;
        let mut resp = loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(PortError::new(ErrorKind::Cancelled, "已停止，没有产生费用。"));
            }
            let mut resp = meter::post_json(&self.agent, &self.endpoint(), &[("authorization", &auth), ("accept", "application/json")], &body, who)?;
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let wait = meter::retry_after(&resp);
            let v = meter::error_body(&mut resp);
            let message = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
            if meter::retryable(status) && attempt < meter::WAITS.len() {
                meter::wait(wait.unwrap_or(meter::WAITS[attempt]), cancel)?;
                attempt += 1;
                continue;
            }
            let kind = if matches!(status, 401 | 402 | 403 | 404) { ErrorKind::Fatal } else { ErrorKind::Failed };
            return Err(PortError::new(kind, format!("{}这次没有产生费用。", error_message(who, status, message))));
        };
        let text = resp.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| {
            // 读回答时断了：已经算过钱，按估计记
            let t = Tokens { input: meter::input_estimate(req), output: req.max_tokens as u64, cache_read: 0, cache_write: 0 };
            let cost = self.meter.prices.tokens_usd(&self.model, t.input, t.output);
            let _ = self.meter.record(self.meter.call(req.purpose.key(), PROVIDER, usage::KIND_MODEL, &self.model, t, 0, cost, true, Some("中途出错，按估计记".into()), None));
            PortError { kind: ErrorKind::Failed, message: format!("接收回答时连接断了：{e}。按估计记进了用量，可以点「继续」重试。"), usage: meter::usage_of(&t, 0, cost) }
        })?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let get = |p: &str| v.pointer(p).and_then(Value::as_u64).unwrap_or(0);
        let (prompt, completion, hit) = (get("/usage/prompt_tokens"), get("/usage/completion_tokens"), get("/usage/prompt_cache_hit_tokens"));
        // 没给用量：按估计（偏高）记
        let estimated = v.pointer("/usage/prompt_tokens").is_none();
        let (prompt, completion) = if estimated { (meter::input_estimate(req), (text.chars().count() as u64 * 3 / 2).max(200)) } else { (prompt, completion) };
        let model = v["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(&self.model).to_string();
        let cost = self.meter.prices.tokens_usd(&model, prompt, completion);
        // 账本里：输入里不是缓存命中的部分记 input，命中的记 cache_read（显示用；费用按全部输入原价算）
        let tokens = Tokens { input: prompt.saturating_sub(hit), output: completion, cache_read: hit.min(prompt), cache_write: 0 };
        let finish = v.pointer("/choices/0/finish_reason").and_then(Value::as_str).unwrap_or("");
        let note = match finish {
            "length" => Some("回答写到上限被截断".to_string()),
            "content_filter" => Some("被接口的内容审查拦下".to_string()),
            _ if estimated => Some("接口没给用量，按估计记".to_string()),
            _ => None,
        };
        let request_id = v["id"].as_str().map(str::to_string);
        self.meter.record(self.meter.call(req.purpose.key(), PROVIDER, usage::KIND_MODEL, &model, tokens, 0, cost, estimated, note, request_id))?;
        let usage = meter::usage_of(&Tokens { input: prompt, output: completion, cache_read: 0, cache_write: 0 }, 0, cost);
        match finish {
            "length" => return Err(PortError { kind: ErrorKind::Failed, message: format!("回答太长，写到上限被截断了，这一段的结果不能用。已经产生的 ${cost:.4} 记进了用量。"), usage }),
            "content_filter" => return Err(PortError { kind: ErrorKind::Failed, message: format!("{who} 的内容审查拦下了这一段，没有结果。费用 ${cost:.4} 已记进用量。"), usage }),
            "insufficient_system_resource" => return Err(PortError { kind: ErrorKind::Failed, message: format!("{who} 的服务资源不足，回答没写完。等一会儿再点「继续」。"), usage }),
            _ => {}
        }
        let content = v.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or("").to_string();
        if content.trim().is_empty() {
            return Err(PortError { kind: ErrorKind::Failed, message: format!("{who} 返回了空的回答。费用 ${cost:.4} 已记进用量。"), usage });
        }
        Ok(ModelReply { text: content, model, usage, hits: Vec::new(), searched: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::research::testing::{FakeServer, Resp};

    const EXTRACT: &str = include_str!("../../fixtures/research/recorded/deepseek-extract.json");

    fn meter(dir: &std::path::Path) -> Arc<Meter> {
        let mut m = Meter::new(dir, 30.0, Default::default());
        m.now = || "2026-11-12T09:00:00".into();
        Arc::new(m)
    }

    fn request() -> ModelRequest {
        let source = research_engine::model::Source { id: "S1".into(), url: "https://stats.example.invalid/2024".into(), ..Default::default() };
        research_engine::prompts::extract_request("示例市青年就业怎么样？", &Default::default(), &source, "2024 年，全市 16–24 岁城镇青年调查失业率为 12.3%", (1, 1))
    }

    #[test]
    fn extract_call_against_a_fake_deepseek() {
        let dir = temp_dir("research-openai");
        let server = FakeServer::start(|_| Resp::json(200, EXTRACT));
        let m = OpenAiCompatible::new(Some(&server.base), "sk-0123456789abcdef0123".into(), None, meter(&dir));
        let reply = m.complete(&request(), &AtomicBool::new(false)).unwrap();
        let ex = research_engine::prompts::parse_extract(&reply.text).unwrap();
        assert_eq!((ex.facts.len(), ex.facts[0].value.as_str()), (1, "12.3"));
        assert_eq!(reply.model, "deepseek-flash");
        // 费用：deepseek-flash 高峰价 $0.30 / $1.20，缓存命中的也按原价
        let cost = (2400.0 * 0.30 + 380.0 * 1.20) / 1e6;
        assert!((reply.usage.cost_usd - cost).abs() < 1e-12);
        assert_eq!((reply.usage.input_tokens, reply.usage.output_tokens), (2400, 380));
        let req = &server.requests()[0];
        assert_eq!((req.path.as_str(), req.header("authorization")), ("/chat/completions", Some("Bearer sk-0123456789abcdef0123")));
        let body = req.json();
        assert_eq!(body["model"], "deepseek-flash");
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][1]["content"].as_str().unwrap().contains("<正文>"));
        let c = &crate::agent::usage::Ledger::new(&dir).all().unwrap()[0];
        assert_eq!((c.provider.as_str(), c.kind.as_str(), c.purpose.as_str()), ("openai_compatible", "model", "research_extract"));
        assert_eq!((c.tokens.input, c.tokens.cache_read, c.tokens.output), (1600, 800, 380));
        assert_eq!(c.request_id.as_deref(), Some("chatcmpl-fixture-1"));
    }

    #[test]
    fn json_word_is_added_when_missing_and_base_url_forms() {
        let dir = temp_dir("research-openai-body");
        let m = OpenAiCompatible::new(Some("https://api.deepseek.com/"), "k".into(), Some("deepseek-flash"), meter(&dir));
        let mut req = request();
        req.system = "写结论".into();
        assert!(m.body(&req)["messages"][0]["content"].as_str().unwrap().ends_with("只输出一个 json 对象。"));
        assert_eq!(m.endpoint(), "https://api.deepseek.com/chat/completions");
        let m = OpenAiCompatible::new(Some("https://llm.example.invalid/v1/chat/completions"), "k".into(), Some("other-model"), meter(&dir));
        assert_eq!(m.endpoint(), "https://llm.example.invalid/v1/chat/completions");
        assert_eq!(m.info().model, "other-model");
    }

    #[test]
    fn truncation_filter_and_errors() {
        let dir = temp_dir("research-openai-errors");
        let length = EXTRACT.replace("\"finish_reason\":\"stop\"", "\"finish_reason\":\"length\"");
        let filtered = EXTRACT.replace("\"finish_reason\":\"stop\"", "\"finish_reason\":\"content_filter\"");
        let server = FakeServer::start(move |req| match req.header("authorization") {
            Some("Bearer length") => Resp::json(200, &length),
            Some("Bearer filter") => Resp::json(200, &filtered),
            Some("Bearer bad") => Resp::json(401, r#"{"error":{"message":"Authentication Fails (sk-bad)","type":"authentication_error"}}"#),
            Some("Bearer broke") => Resp::json(402, r#"{"error":{"message":"Insufficient Balance"}}"#),
            Some("Bearer busy") => Resp::json(503, r#"{"error":{"message":"busy"}}"#).with("retry-after", "0"),
            _ => Resp::json(200, r#"{"id":"x","model":"deepseek-flash","choices":[{"message":{"content":""},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":0}}"#),
        });
        let call = |key: &str| OpenAiCompatible::new(Some(&server.base), key.into(), None, meter(&dir)).complete(&request(), &AtomicBool::new(false)).unwrap_err();
        let e = call("length");
        assert!(e.message.contains("截断") && e.usage.cost_usd > 0.0 && e.kind == ErrorKind::Failed);
        assert!(call("filter").message.contains("内容审查"));
        let e = call("bad");
        assert!(e.kind == ErrorKind::Fatal && e.message.contains("DeepSeek 的 API 密钥不对") && !e.message.contains("sk-bad"));
        assert!(call("broke").message.contains("余额不足"));
        let before = server.requests().len();
        assert!(call("busy").message.contains("服务出错（503）"));
        assert_eq!(server.requests().len() - before, 4);
        assert!(call("empty").message.contains("空的回答"));
        let ledger = crate::agent::usage::Ledger::new(&dir).all().unwrap();
        assert_eq!(ledger.iter().filter_map(|c| c.note.as_deref()).collect::<Vec<_>>(), ["回答写到上限被截断", "被接口的内容审查拦下"]);
        assert_eq!(ledger.len(), 3);
    }
}
