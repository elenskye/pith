//! 事实调查的记账与预算：每次调用模型或搜索之前按最坏情况估费用，加上本月已花的超过月度上限就不发；
//! 调用之后按接口返回的用量记进账本（`agent/usage.jsonl`，和排版 Agent 共用一本账、一个月度上限，`decisions/0020`）。
//!
//! 真实接口（`anthropic`、`openai`、`tavily`）自己调 `Meter::check` / `Meter::record`；
//! 模拟接口经 `Logged` 记一行「模拟」（不花钱，不计入上限），界面上的用量表照样看得到。

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use research_engine::model::ModelRef;
use research_engine::ports::{ErrorKind, Model, ModelReply, ModelRequest, PortError, Search, SearchReply, Usage};

use crate::agent::usage::{self, Call, Ledger, Prices, Tokens};

pub struct Meter {
    pub ledger: Ledger,
    pub budget_usd: f64,
    pub prices: Prices,
    pub now: fn() -> String,
}

impl Meter {
    pub fn new(data_dir: &std::path::Path, budget_usd: f64, prices: Prices) -> Self {
        Meter { ledger: Ledger::new(data_dir), budget_usd, prices, now: crate::text::now_iso }
    }

    fn month(&self) -> String {
        (self.now)().get(..7).unwrap_or("").to_string()
    }

    pub fn month_spent(&self) -> Result<f64, String> {
        self.ledger.month_total(&self.month())
    }

    /// 发送前：这次最多花 `worst_usd`，加上本月已花的会不会超过上限
    pub fn check(&self, worst_usd: f64, what: &str) -> Result<(), PortError> {
        let spent = self.month_spent().map_err(|e| PortError::new(ErrorKind::Fatal, format!("{e}。为了不超支，现在不发请求。")))?;
        if spent + worst_usd > self.budget_usd {
            return Err(PortError::new(
                ErrorKind::Budget,
                format!("{what}最多可能花 ${worst_usd:.2}，加上本月已花的 ${spent:.2} 会超过月度上限 ${:.2}，所以没有发送。可以在「设置 › 事实调查」调高上限后点「继续」。", self.budget_usd),
            ));
        }
        Ok(())
    }

    pub fn record(&self, call: Call) -> Result<(), PortError> {
        self.ledger.append(&call).map_err(|e| PortError::new(ErrorKind::Fatal, e))
    }

    /// 一行账：`provider` 接口、`kind` 模型 / 搜索
    #[allow(clippy::too_many_arguments)]
    pub fn call(&self, purpose: &str, provider: &str, kind: &str, model: &str, tokens: Tokens, searches: u64, cost_usd: f64, estimated: bool, note: Option<String>, request_id: Option<String>) -> Call {
        Call { at: (self.now)(), purpose: purpose.into(), provider: provider.into(), kind: kind.into(), searches, model: model.into(), tokens, cost_usd, estimated, mock: false, note, request_id }
    }
}

/// 发送前估计 token 数（不联网，偏高）
pub fn input_estimate(req: &ModelRequest) -> u64 {
    usage::estimate_tokens(&req.system) + req.user.iter().map(|u| usage::estimate_tokens(u)).sum::<u64>() + usage::estimate_tokens(&req.schema.to_string())
}

/// 一次模型调用的最坏情况：输入留一成余量 + 输出写满上限 + 自带搜索用满
pub fn worst_case(prices: &Prices, model: &str, req: &ModelRequest, search_usd: f64) -> f64 {
    let p = prices.model(model);
    (input_estimate(req) as f64 * 1.1 * p.input + req.max_tokens as f64 * p.output) / 1e6 + req.web_searches as f64 * search_usd
}

/// 一般情况（确认框里的「预计约」）
pub fn typical_case(prices: &Prices, model: &str, req: &ModelRequest, search_usd: f64) -> f64 {
    let p = prices.model(model);
    (input_estimate(req) as f64 * p.input + req.typical_output_tokens as f64 * p.output) / 1e6 + req.web_searches.min(3) as f64 * search_usd
}

// ---------- HTTP ----------

pub fn agent(timeout_recv_body: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(300)))
        .timeout_recv_body(Some(timeout_recv_body))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(crate::metadata::net::APP_UA)
        .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
        .build()
        .into()
}

pub fn retryable(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
}

/// 重试前等一会儿（可以被取消）
pub fn wait(seconds: u64, cancel: &AtomicBool) -> Result<(), PortError> {
    for _ in 0..seconds.min(30) * 10 {
        if cancel.load(Ordering::SeqCst) {
            return Err(PortError::new(ErrorKind::Cancelled, "已停止，没有产生费用。"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

pub const WAITS: [u64; 3] = [2, 6, 15];

/// 发一个 POST（JSON），连不上时说清楚、不带密钥
pub fn post_json(agent: &ureq::Agent, url: &str, headers: &[(&str, &str)], body: &serde_json::Value, who: &str) -> Result<ureq::http::Response<ureq::Body>, PortError> {
    let mut r = agent.post(url).header("content-type", "application/json");
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send(body.to_string()).map_err(|e| match e {
        ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => PortError::new(ErrorKind::Failed, format!("连不上 {who}（{e}）。检查网络后点「继续」重试；这次没有产生费用。")),
        other => PortError::new(ErrorKind::Failed, format!("请求没发出去：{other}。这次没有产生费用。")),
    })
}

/// 读出错误回复的正文（最多 256 KB）
pub fn error_body(resp: &mut ureq::http::Response<ureq::Body>) -> serde_json::Value {
    let mut text = String::new();
    let _ = resp.body_mut().as_reader().take(256 * 1024).read_to_string(&mut text);
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

pub fn retry_after(resp: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok())
}

/// 报错里只留前 200 字，不回显密钥的样子
pub fn detail(message: &str) -> String {
    let cleaned: String = message.split_whitespace().filter(|w| !(w.starts_with("sk-") || w.starts_with("tvly-"))).collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() { String::new() } else { format!("（{}）", cleaned.chars().take(200).collect::<String>()) }
}

// ---------- 模拟接口的记账 ----------

/// 模拟的模型：调用照常记一行账，标「模拟」、不算钱
pub struct Logged<'a, T> {
    pub inner: T,
    pub ledger: &'a Ledger,
    pub now: fn() -> String,
}

impl<T: Model> Model for Logged<'_, T> {
    fn info(&self) -> ModelRef {
        self.inner.info()
    }

    fn is_mock(&self) -> bool {
        self.inner.is_mock()
    }

    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError> {
        let reply = self.inner.complete(req, cancel)?;
        let info = self.inner.info();
        let call = Call {
            at: (self.now)(),
            purpose: req.purpose.key().into(),
            provider: info.provider,
            kind: usage::KIND_MODEL.into(),
            searches: reply.usage.searches,
            model: reply.model.clone(),
            tokens: Tokens { input: reply.usage.input_tokens, output: reply.usage.output_tokens, cache_read: 0, cache_write: 0 },
            cost_usd: 0.0,
            estimated: false,
            mock: true,
            note: None,
            request_id: None,
        };
        self.ledger.append(&call).map_err(|e| PortError::new(ErrorKind::Fatal, e))?;
        Ok(reply)
    }
}

impl<T: Search> Search for Logged<'_, T> {
    fn engine(&self) -> String {
        self.inner.engine()
    }

    fn is_mock(&self) -> bool {
        self.inner.is_mock()
    }

    fn search(&self, query: &str, max_results: usize, cancel: &AtomicBool) -> Result<SearchReply, PortError> {
        let reply = self.inner.search(query, max_results, cancel)?;
        let call = Call {
            at: (self.now)(),
            purpose: "research_search".into(),
            provider: self.inner.engine(),
            kind: usage::KIND_SEARCH.into(),
            searches: reply.usage.searches,
            model: self.inner.engine(),
            tokens: Tokens::default(),
            cost_usd: 0.0,
            estimated: false,
            mock: true,
            note: None,
            request_id: None,
        };
        self.ledger.append(&call).map_err(|e| PortError::new(ErrorKind::Fatal, e))?;
        Ok(reply)
    }
}

/// 把接口里的用量换成引擎的 `Usage`
pub fn usage_of(tokens: &Tokens, searches: u64, cost_usd: f64) -> Usage {
    Usage { input_tokens: tokens.input + tokens.cache_read + tokens.cache_write, output_tokens: tokens.output, searches, cost_usd }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use research_engine::ports::Purpose;

    fn req(max_tokens: u32, web_searches: u32) -> ModelRequest {
        ModelRequest { purpose: Purpose::Plan, system: "系统".repeat(500), user: vec!["问题".into()], schema: serde_json::json!({}), max_tokens, typical_output_tokens: 1000, web_searches, tag: "plan".into() }
    }

    #[test]
    fn budget_is_checked_against_the_worst_case_and_shared_with_the_format_agent() {
        let dir = temp_dir("research-meter");
        let mut m = Meter::new(&dir, 1.0, Prices::default());
        m.now = || "2026-11-12T09:00:00".into();
        // 排版 Agent 本月已经花了 $0.6
        m.ledger.append(&Call { at: "2026-11-02T10:00:00".into(), purpose: "extract".into(), provider: "anthropic".into(), kind: "model".into(), searches: 0, model: "claude-opus-5-5".into(), tokens: Tokens::default(), cost_usd: 0.6, estimated: false, mock: false, note: None, request_id: None }).unwrap();
        let small = worst_case(&m.prices, "claude-opus-5-5", &req(1000, 0), 0.01);
        assert!(small < 0.4);
        m.check(small, "这次计划").unwrap();
        let big = worst_case(&m.prices, "claude-opus-5-5", &req(32_000, 5), 0.01);
        assert!((big - (input_estimate(&req(32_000, 5)) as f64 * 1.1 * 4.0 / 1e6 + 0.64 + 0.05)).abs() < 1e-9);
        let e = m.check(big, "这次计划").unwrap_err();
        assert_eq!(e.kind, ErrorKind::Budget);
        assert!(e.message.contains("本月已花的 $0.60") && e.message.contains("月度上限 $1.00"));
        // 账本坏了：不当成没花钱
        std::fs::write(dir.join("agent/usage.jsonl"), "{坏了\n").unwrap();
        assert_eq!(m.check(0.01, "x").unwrap_err().kind, ErrorKind::Fatal);
        assert!(typical_case(&m.prices, "claude-opus-5-5", &req(32_000, 5), 0.01) < big);
    }

    #[test]
    fn error_details_never_echo_keys() {
        assert_eq!(detail("invalid key sk-abcdef123 given"), "（invalid key given）");
        assert_eq!(detail("tvly-xyz"), "");
        assert!(detail(&"字".repeat(500)).chars().count() < 210);
    }
}
