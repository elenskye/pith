//! OpenAI Responses API（`decisions/0024`「模型与接口」）：概念研究的默认模型 GPT-6.1 Sol（`gpt-6.1-sol`）。
//! 新写的，不改 v0.3 `research/openai.rs`（那是 DeepSeek 用的 chat completions）。
//!
//! - `POST {base}/v1/responses`，不流式：`instructions` 是系统提示词，`input` 是一条用户消息；
//!   结构化输出 `text.format = {type: json_schema, name, schema, strict: true}`；`reasoning.effort`（默认 medium）；
//!   `store: false`（不让 OpenAI 存对话）；`max_output_tokens`（推理 token 也算在内，最坏费用按它算）。
//! - 用量：`usage.input_tokens`（含缓存命中 `input_tokens_details.cached_tokens`）、`usage.output_tokens`（含推理 `output_tokens_details.reasoning_tokens`）。
//! - 单价（2026-11-13）：输入 $2、缓存输入 $0.10、输出 $10（每百万 token）；单次输入超过 272K 时整次按输入 2 倍、输出 1.5 倍；
//!   网页搜索每千次 $10（搜到的内容按输入 token 计）。设置里覆盖了单价的按设置。
//! - 专用搜索：Responses 的 `web_search` 工具（`OpenAiSearch`），没开就用 Tavily。
//! - 截断（`status: incomplete`）、拒答（`refusal`）照样记账、不交结果；401 / 403 / 404 / 余额不足整个停下；
//!   408 / 409 / 429 / 5xx 重试三次；报错里不带密钥。
//!
//! 官方文档访问不了的细节照录制样例写，C4 用真实调用核对（列在 `HANDOFF-cloud.md`）。

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use research_engine::model::{Hit, ModelRef};
use research_engine::ports::{ErrorKind, Model, ModelReply, ModelRequest, PortError, Search, SearchReply};
use serde_json::{Value, json};

use crate::agent::usage::{self, Prices, Tokens};
use crate::research::meter::{self, Meter};

pub const PROVIDER: &str = "openai";
pub const DEFAULT_BASE: &str = "https://api.openai.com";
pub const DEFAULT_MODEL: &str = "gpt-6.1-sol";
/// 缓存命中的输入（美元 / 百万 token）
pub const SOL_CACHED_INPUT: f64 = 0.10;
/// 单次输入超过这么多 token：整次按输入 2 倍、输出 1.5 倍计
pub const LONG_CONTEXT: u64 = 272_000;
/// OpenAI 网页搜索：每次多少美元（每千次 $10）
pub const WEB_SEARCH_USD: f64 = 0.01;
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// 一次调用的费用。`input` 含缓存命中的 `cached`
pub fn cost(prices: &Prices, model: &str, input: u64, cached: u64, output: u64) -> f64 {
    let p = prices.model(model);
    let cached = cached.min(input);
    let cached_price = if model.starts_with(DEFAULT_MODEL) && !prices.models.contains_key(model) { SOL_CACHED_INPUT } else { p.input };
    let (mi, mo) = if input > LONG_CONTEXT { (2.0, 1.5) } else { (1.0, 1.0) };
    (((input - cached) as f64 * p.input + cached as f64 * cached_price) * mi + output as f64 * p.output * mo) / 1e6
}

/// 发送前的最坏情况：输入估计 ×1.1（超过 272K 就按加价算）+ 输出写满
pub fn worst_case(prices: &Prices, model: &str, input_estimate: u64, max_output: u64) -> f64 {
    let input = (input_estimate as f64 * 1.1).ceil() as u64;
    cost(prices, model, input, 0, max_output)
}

/// 一般情况（确认框里的「预计约」）
pub fn typical_case(prices: &Prices, model: &str, input_estimate: u64, typical_output: u64) -> f64 {
    cost(prices, model, input_estimate, 0, typical_output)
}

pub struct Responses {
    agent: ureq::Agent,
    base: String,
    key: String,
    pub model: String,
    pub effort: String,
    meter: Arc<Meter>,
}

pub fn error_message(status: u16, code: &str, message: &str) -> String {
    let detail = meter::detail(message);
    match (status, code) {
        (401, _) => "OpenAI 的 API 密钥不对或已失效。到「设置 › 密钥与费用」重新填写。".into(),
        (429, "insufficient_quota") => "OpenAI 账户余额不足或超过了用量上限。充值后点「继续」。".into(),
        (403, _) => format!("这个 OpenAI 密钥没有权限调用这个模型{detail}。"),
        (404, _) => format!("OpenAI 找不到这个模型或接口{detail}。到「设置 › 概念研究」检查模型名。"),
        (400 | 422, _) => format!("OpenAI 不接受这次请求{detail}。"),
        (429, _) => "OpenAI 说调用太频繁，被限流了。等一会儿再点「继续」。".into(),
        (s, _) if s >= 500 => format!("OpenAI 的服务出错（{s}）。等一会儿再点「继续」。"),
        (s, _) => format!("OpenAI 返回了 {s}{detail}。"),
    }
}

/// 回复里的文字（message 的 output_text 拼起来）、拒答、引用到的网址、网页搜索次数
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub text: String,
    pub refusal: Option<String>,
    pub citations: Vec<Hit>,
    pub searches: u64,
}

pub fn parse_output(v: &Value) -> Parsed {
    let mut p = Parsed::default();
    for item in v["output"].as_array().into_iter().flatten() {
        match item["type"].as_str().unwrap_or("") {
            "message" => {
                for c in item["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str().unwrap_or("") {
                        "output_text" => {
                            p.text.push_str(c["text"].as_str().unwrap_or(""));
                            for a in c["annotations"].as_array().into_iter().flatten().filter(|a| a["type"] == "url_citation") {
                                if let Some(url) = a["url"].as_str() {
                                    p.citations.push(Hit { url: url.into(), title: a["title"].as_str().unwrap_or("").into(), snippet: String::new(), published: None });
                                }
                            }
                        }
                        "refusal" => p.refusal = Some(c["refusal"].as_str().unwrap_or("").to_string()),
                        _ => {}
                    }
                }
            }
            "web_search_call" => {
                p.searches += 1;
                for s in item.pointer("/action/sources").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(url) = s["url"].as_str() {
                        p.citations.push(Hit { url: url.into(), title: s["title"].as_str().unwrap_or("").into(), snippet: String::new(), published: None });
                    }
                }
            }
            _ => {}
        }
    }
    p
}

impl Responses {
    pub fn new(base: Option<&str>, key: String, model: &str, effort: &str, meter: Arc<Meter>) -> Self {
        let base = base.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(DEFAULT_BASE).trim_end_matches('/').to_string();
        let effort = if EFFORTS.contains(&effort) { effort } else { "medium" };
        Responses { agent: meter::agent(Duration::from_secs(1200)), base, key, model: model.to_string(), effort: effort.to_string(), meter }
    }

    pub fn body(&self, req: &ModelRequest) -> Value {
        json!({
            "model": self.model,
            "instructions": req.system,
            "input": [ { "role": "user", "content": [ { "type": "input_text", "text": req.user.join("\n\n") } ] } ],
            "text": { "format": { "type": "json_schema", "name": "concept_cards", "schema": req.schema, "strict": true } },
            "reasoning": { "effort": self.effort },
            "store": false,
            "max_output_tokens": req.max_tokens,
        })
    }

    fn endpoint(&self) -> String {
        if self.base.ends_with("/responses") { self.base.clone() } else { format!("{}/v1/responses", self.base.trim_end_matches("/v1")) }
    }

    /// 回复说的模型（可能带日期）按设置里的模型计价
    fn priced_model<'a>(&'a self, served: &'a str) -> &'a str {
        if served.starts_with(&self.model) || self.meter.prices.models.contains_key(served) || !self.meter.prices.knows(served) { &self.model } else { served }
    }

    /// 发一次（含重试），读回整份 JSON。`purpose` 记进账本
    fn send(&self, body: &Value, input_estimate: u64, max_output: u64, kind: &str, note: Option<String>, cancel: &AtomicBool) -> Result<(Value, Tokens, f64, u64), PortError> {
        let auth = format!("Bearer {}", self.key);
        let purpose = if kind == usage::KIND_SEARCH { "concept_search" } else { "concept" };
        let mut attempt = 0;
        let mut resp = loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(PortError::new(ErrorKind::Cancelled, "已停止，没有产生费用。"));
            }
            let mut resp = meter::post_json(&self.agent, &self.endpoint(), &[("authorization", &auth), ("accept", "application/json")], body, "OpenAI")?;
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let wait = meter::retry_after(&resp);
            let v = meter::error_body(&mut resp);
            let code = v.pointer("/error/code").and_then(Value::as_str).unwrap_or("");
            let message = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
            if meter::retryable(status) && code != "insufficient_quota" && attempt < meter::WAITS.len() {
                meter::wait(wait.unwrap_or(meter::WAITS[attempt]), cancel)?;
                attempt += 1;
                continue;
            }
            let fatal = matches!(status, 401 | 403 | 404) || code == "insufficient_quota";
            return Err(PortError::new(if fatal { ErrorKind::Fatal } else { ErrorKind::Failed }, format!("{}这次没有产生费用。", error_message(status, code, message))));
        };
        let mut text = String::new();
        if let Err(e) = resp.body_mut().as_reader().take(16 * 1024 * 1024).read_to_string(&mut text) {
            // 读回答时断了：已经算过钱，按估计记
            let t = Tokens { input: input_estimate, output: max_output, cache_read: 0, cache_write: 0 };
            let c = cost(&self.meter.prices, &self.model, input_estimate, 0, max_output);
            let _ = self.meter.record(self.meter.call(purpose, PROVIDER, kind, &self.model, t, 0, c, true, Some("中途出错，按估计记".into()), None));
            return Err(PortError { kind: ErrorKind::Failed, message: format!("接收回答时连接断了：{e}。按估计记进了用量，可以点「继续」重试。"), usage: meter::usage_of(&t, 0, c) });
        }
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let get = |p: &str| v.pointer(p).and_then(Value::as_u64);
        let estimated = get("/usage/input_tokens").is_none();
        let (input, cached, output) = if estimated { (input_estimate, 0, max_output) } else { (get("/usage/input_tokens").unwrap_or(0), get("/usage/input_tokens_details/cached_tokens").unwrap_or(0), get("/usage/output_tokens").unwrap_or(0)) };
        let served = v["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(&self.model).to_string();
        let parsed = parse_output(&v);
        let searches = parsed.searches;
        let c = cost(&self.meter.prices, self.priced_model(&served), input, cached, output) + searches as f64 * WEB_SEARCH_USD;
        // 账本：不是缓存命中的输入记 input，命中的记 cache_read
        let tokens = Tokens { input: input.saturating_sub(cached), output, cache_read: cached.min(input), cache_write: 0 };
        let mut note = note;
        if estimated {
            note = Some("接口没给用量，按估计记".into());
        }
        if let Some(r) = get("/usage/output_tokens_details/reasoning_tokens").filter(|r| *r > 0) {
            note = Some(format!("{}推理 {r} token", note.map(|n| format!("{n}；")).unwrap_or_default()));
        }
        let request_id = v["id"].as_str().map(str::to_string);
        self.meter.record(self.meter.call(purpose, PROVIDER, kind, &served, tokens, searches, c, estimated, note, request_id))?;
        Ok((v, Tokens { input, output, cache_read: cached, cache_write: 0 }, c, searches))
    }
}

impl Model for Responses {
    fn info(&self) -> ModelRef {
        ModelRef { provider: PROVIDER.into(), model: self.model.clone() }
    }

    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError> {
        let input_estimate = meter::input_estimate(req);
        let worst = worst_case(&self.meter.prices, &self.model, input_estimate, req.max_tokens as u64);
        self.meter.check(worst, "这一步")?;
        let body = self.body(req);
        let (v, t, c, _) = self.send(&body, input_estimate, req.max_tokens as u64, usage::KIND_MODEL, Some(req.tag.clone()), cancel)?;
        let usage = research_engine::ports::Usage { input_tokens: t.input, output_tokens: t.output, searches: 0, cost_usd: c };
        let parsed = parse_output(&v);
        let status = v["status"].as_str().unwrap_or("completed");
        if status == "incomplete" {
            let why = v.pointer("/incomplete_details/reason").and_then(Value::as_str).unwrap_or("");
            let message = if why == "max_output_tokens" {
                format!("回答写到上限（{} token，推理也算在内）被截断了，这一步的结果不能用。已经产生的 ${c:.4} 记进了用量。可以在技能里调高 max_output_tokens 或调低推理深浅后重试。", req.max_tokens)
            } else {
                format!("回答没写完（{why}），这一步的结果不能用。已经产生的 ${c:.4} 记进了用量。")
            };
            return Err(PortError { kind: ErrorKind::Failed, message, usage });
        }
        if status == "failed" {
            return Err(PortError { kind: ErrorKind::Failed, message: format!("OpenAI 说这次失败了{}。费用 ${c:.4} 已记进用量。", meter::detail(v.pointer("/error/message").and_then(Value::as_str).unwrap_or(""))), usage });
        }
        if let Some(r) = parsed.refusal {
            return Err(PortError { kind: ErrorKind::Failed, message: format!("模型拒绝回答{}。费用 ${c:.4} 已记进用量。", meter::detail(&r)), usage });
        }
        if parsed.text.trim().is_empty() {
            return Err(PortError { kind: ErrorKind::Failed, message: format!("模型没有给出文字。费用 ${c:.4} 已记进用量，可以重试。"), usage });
        }
        let model = v["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(&self.model).to_string();
        Ok(ModelReply { text: parsed.text, model, usage, hits: Vec::new(), searched: Vec::new() })
    }
}

/// Responses 的 `web_search` 工具当成专用搜索：让模型搜一次，取它引用和看过的网址
pub struct OpenAiSearch {
    pub model: Responses,
}

impl OpenAiSearch {
    pub fn body(&self, query: &str, max_results: usize) -> Value {
        json!({
            "model": self.model.model,
            "instructions": "You are a search helper. Use the web_search tool once, then list the most relevant sources (title and URL) for the query. Do not answer the question itself.",
            "input": [ { "role": "user", "content": [ { "type": "input_text", "text": format!("Query: {query}\nList up to {max_results} sources.") } ] } ],
            "tools": [ { "type": "web_search" } ],
            "tool_choice": "required",
            "include": ["web_search_call.action.sources"],
            "reasoning": { "effort": "low" },
            "store": false,
            "max_output_tokens": 2000,
        })
    }
}

/// 一次搜索最多花多少（估费用）：搜一次 + 搜到的内容按输入算（按 2 万 token 估）+ 输出写满
pub fn search_worst(prices: &Prices, model: &str) -> f64 {
    WEB_SEARCH_USD + cost(prices, model, 20_000, 0, 2000)
}

impl Search for OpenAiSearch {
    fn engine(&self) -> String {
        "openai_web_search".into()
    }

    fn search(&self, query: &str, max_results: usize, cancel: &AtomicBool) -> Result<SearchReply, PortError> {
        let worst = search_worst(&self.model.meter.prices, &self.model.model);
        self.model.meter.check(worst, "这次网页搜索")?;
        let (v, t, c, searches) = self.model.send(&self.body(query, max_results), 20_000, 2000, usage::KIND_SEARCH, Some(format!("搜索「{query}」")), cancel)?;
        let mut hits: Vec<Hit> = Vec::new();
        for h in parse_output(&v).citations {
            if !hits.iter().any(|x| x.url == h.url) {
                hits.push(h);
            }
        }
        hits.truncate(max_results);
        Ok(SearchReply { hits, usage: research_engine::ports::Usage { input_tokens: t.input, output_tokens: t.output, searches: searches.max(1), cost_usd: c } })
    }
}
