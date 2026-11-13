//! Tavily 搜索（`decisions/0020`）：执行模型没有专用搜索时用它（DeepSeek 就是这样）。
//!
//! `POST https://api.tavily.com/search`，`Authorization: Bearer <密钥>`。基础搜索记 1 点、高级记 2 点，
//! 每点约 $0.008（免费额度也照价记进账本）。回复里有 `usage.credits` 就按它记，没有就按搜索深度算。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use research_engine::model::Hit;
use research_engine::ports::{ErrorKind, PortError, Search, SearchReply};
use serde_json::{Value, json};

use super::meter::{self, Meter};
use crate::agent::usage::{self, Tokens};

pub const PROVIDER: &str = "tavily";
pub const DEFAULT_BASE: &str = "https://api.tavily.com";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    #[default]
    Basic,
    Advanced,
}

impl Depth {
    pub fn credits(self) -> u64 {
        match self {
            Depth::Basic => 1,
            Depth::Advanced => 2,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Depth::Basic => "basic",
            Depth::Advanced => "advanced",
        }
    }
}

pub struct Tavily {
    agent: ureq::Agent,
    base: String,
    key: String,
    pub depth: Depth,
    meter: Arc<Meter>,
}

fn error_message(status: u16, message: &str) -> String {
    let detail = meter::detail(message);
    match status {
        401 => "Tavily 的 API 密钥不对或已失效。到「设置 › 密钥与费用」重新填写。".into(),
        432 | 433 => format!("Tavily 的额度用完了或超出了套餐的限制{detail}。到 app.tavily.com 查看后点「继续」。"),
        429 => "Tavily 说搜索太频繁，被限流了。等一会儿再点「继续」。".into(),
        400 => format!("Tavily 不接受这个搜索词{detail}。"),
        s if s >= 500 => format!("Tavily 的服务出错（{s}）。等一会儿再点「继续」。"),
        s => format!("Tavily 返回了 {s}{detail}。"),
    }
}

impl Tavily {
    pub fn new(base: Option<&str>, key: String, depth: Depth, meter: Arc<Meter>) -> Self {
        let base = base.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(DEFAULT_BASE).trim_end_matches('/').to_string();
        Tavily { agent: meter::agent(Duration::from_secs(60)), base, key, depth, meter }
    }

    pub fn body(&self, query: &str, max_results: usize) -> Value {
        json!({
            "query": query.trim(),
            "search_depth": self.depth.key(),
            "max_results": max_results.clamp(1, 20),
            "include_answer": false,
            "include_raw_content": false,
            "include_images": false,
            "include_usage": true,
        })
    }

    pub fn cost(&self, credits: u64) -> f64 {
        credits as f64 * self.meter.prices.tavily_credit()
    }
}

impl Search for Tavily {
    fn engine(&self) -> String {
        PROVIDER.into()
    }

    fn search(&self, query: &str, max_results: usize, cancel: &AtomicBool) -> Result<SearchReply, PortError> {
        self.meter.check(self.cost(self.depth.credits()), "这次搜索")?;
        let body = self.body(query, max_results);
        let auth = format!("Bearer {}", self.key);
        let mut attempt = 0;
        let mut resp = loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(PortError::new(ErrorKind::Cancelled, "已停止，没有产生费用。"));
            }
            let mut resp = meter::post_json(&self.agent, &format!("{}/search", self.base), &[("authorization", &auth)], &body, "Tavily")?;
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let wait = meter::retry_after(&resp);
            let v = meter::error_body(&mut resp);
            let message = v.pointer("/detail/error").or_else(|| v.get("detail")).or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("");
            if meter::retryable(status) && attempt < meter::WAITS.len() {
                meter::wait(wait.unwrap_or(meter::WAITS[attempt]), cancel)?;
                attempt += 1;
                continue;
            }
            let kind = if matches!(status, 401 | 403 | 432 | 433) { ErrorKind::Fatal } else { ErrorKind::Failed };
            return Err(PortError::new(kind, error_message(status, message)));
        };
        let text = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().map_err(|e| PortError::new(ErrorKind::Failed, format!("Tavily 的回复没有收完：{e}")))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| PortError::new(ErrorKind::Failed, format!("Tavily 的回复读不懂：{e}")))?;
        let credits = v.pointer("/usage/credits").and_then(Value::as_u64).unwrap_or(self.depth.credits());
        let cost = self.cost(credits);
        let request_id = v["request_id"].as_str().map(str::to_string);
        self.meter.record(self.meter.call("research_search", PROVIDER, usage::KIND_SEARCH, &format!("tavily-{}", self.depth.key()), Tokens::default(), credits, cost, false, None, request_id))?;
        let hits = v["results"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        let url = r["url"].as_str()?.trim().to_string();
                        Some(Hit { url, title: r["title"].as_str().unwrap_or("").trim().to_string(), snippet: r["content"].as_str().unwrap_or("").chars().take(300).collect(), published: r["published_date"].as_str().map(str::to_string) })
                    })
                    .take(max_results)
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchReply { hits, usage: meter::usage_of(&Tokens::default(), credits, cost) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::research::testing::{FakeServer, Resp};

    const RESULTS: &str = include_str!("../../fixtures/research/recorded/tavily-search.json");

    fn meter(dir: &std::path::Path) -> Arc<Meter> {
        let mut m = Meter::new(dir, 30.0, Default::default());
        m.now = || "2026-11-12T09:00:00".into();
        Arc::new(m)
    }

    #[test]
    fn search_against_a_fake_tavily() {
        let dir = temp_dir("research-tavily");
        let server = FakeServer::start(|_| Resp::json(200, RESULTS));
        let t = Tavily::new(Some(&server.base), "tvly-dev-fake".into(), Depth::Basic, meter(&dir));
        let r = t.search("示例市 青年失业率", 5, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), ["https://stats.example.invalid/bulletin-2024", "https://survey.example.invalid/youth-2024", "https://blog.example.invalid/rumor"]);
        assert_eq!(r.hits[0].published.as_deref(), Some("2025-02-20"));
        assert!(r.hits[1].snippet.starts_with("示例大学"));
        assert_eq!(r.usage.searches, 1);
        assert!((r.usage.cost_usd - 0.008).abs() < 1e-12);
        let req = &server.requests()[0];
        assert_eq!((req.path.as_str(), req.header("authorization")), ("/search", Some("Bearer tvly-dev-fake")));
        assert_eq!(req.json(), json!({ "query": "示例市 青年失业率", "search_depth": "basic", "max_results": 5, "include_answer": false, "include_raw_content": false, "include_images": false, "include_usage": true }));
        let c = &crate::agent::usage::Ledger::new(&dir).all().unwrap()[0];
        assert_eq!((c.provider.as_str(), c.kind.as_str(), c.searches, c.model.as_str()), ("tavily", "search", 1, "tavily-basic"));
        // 高级搜索 2 点；回复没给用量时按深度算
        let server = FakeServer::start(|_| Resp::json(200, r#"{"query":"q","results":[]}"#));
        let t = Tavily::new(Some(&server.base), "k".into(), Depth::Advanced, meter(&dir));
        let r = t.search("q", 50, &AtomicBool::new(false)).unwrap();
        assert!(r.hits.is_empty() && r.usage.searches == 2 && (r.usage.cost_usd - 0.016).abs() < 1e-12);
        assert_eq!(server.requests()[0].json()["max_results"], 20);
    }

    #[test]
    fn errors() {
        let dir = temp_dir("research-tavily-errors");
        let server = FakeServer::start(|req| match req.header("authorization") {
            Some("Bearer bad") => Resp::json(401, r#"{"detail":{"error":"Unauthorized: missing or invalid API key tvly-bad"}}"#),
            Some("Bearer out") => Resp::json(432, r#"{"detail":{"error":"This request exceeds your plan's set usage limit."}}"#),
            _ => Resp::json(400, r#"{"detail":{"error":"Query is too long."}}"#),
        });
        let call = |key: &str| Tavily::new(Some(&server.base), key.into(), Depth::Basic, meter(&dir)).search("q", 5, &AtomicBool::new(false)).unwrap_err();
        let e = call("bad");
        assert!(e.kind == ErrorKind::Fatal && e.message.contains("密钥不对") && !e.message.contains("tvly-bad"));
        assert!(call("out").message.contains("额度用完"));
        let e = call("x");
        assert!(e.kind == ErrorKind::Failed && e.message.contains("Query is too long"));
        assert!(crate::agent::usage::Ledger::new(&dir).all().unwrap().is_empty());
    }
}
