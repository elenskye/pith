//! 模拟接口：录制的模型回复（`fixtures/replies/<技能>-<步骤>.json`，引文都逐字取自夹具）、
//! 模拟的网页检索和下载（一个合成的评论网页）、模拟的文献库（一条合成的书评）。
//!
//! 模拟模型先用录制的回复；回复里挂的段落号、卡片编号在这次请求里没有时（开发版里对别的章、别的书跑），
//! 就按请求现编一份（引每段的第一句、写一句「模拟」的解释），保证界面上流程能点通。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use research_engine::model::{Hit, ModelRef, Source};
use research_engine::ports::{ErrorKind, Fetch, Fetched, Model, ModelReply, ModelRequest, PortError, Search, SearchReply, Usage};
use serde_json::{Value, json};

use crate::flow::{Shelf, ShelfHit};
use crate::model::CommentSource;

pub fn recorded(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "chapter/concepts" => include_str!("../fixtures/replies/chapter-concepts.json"),
        "chapter/arguments" => include_str!("../fixtures/replies/chapter-arguments.json"),
        "chapter/relations" => include_str!("../fixtures/replies/chapter-relations.json"),
        "passage/read" => include_str!("../fixtures/replies/passage-read.json"),
        "ask/answer" => include_str!("../fixtures/replies/ask-answer.json"),
        "trace/trace" => include_str!("../fixtures/replies/trace-trace.json"),
        "critics/comment" => include_str!("../fixtures/replies/critics-comment.json"),
        "counterfactual/speculate" => include_str!("../fixtures/replies/counterfactual-speculate.json"),
        "free/free" => include_str!("../fixtures/replies/free-free.json"),
        _ => return None,
    })
}

type Tamper = Box<dyn Fn(&ModelRequest, String) -> String + Send + Sync>;

pub struct MockModel {
    pub delay: Duration,
    /// 改坏测试：把回复改一改再交出去
    pub tamper: Option<Tamper>,
    /// 记下收到的请求（测试看发了什么）
    pub seen: Mutex<Vec<ModelRequest>>,
}

impl Default for MockModel {
    fn default() -> Self {
        MockModel { delay: Duration::ZERO, tamper: None, seen: Mutex::new(Vec::new()) }
    }
}

impl MockModel {
    pub fn with_tamper(f: impl Fn(&ModelRequest, String) -> String + Send + Sync + 'static) -> Self {
        MockModel { tamper: Some(Box::new(f)), ..Default::default() }
    }
}

/// 请求里出现的 `[KEY]`
fn keys_in(req: &ModelRequest) -> Vec<String> {
    let mut out = Vec::new();
    for u in &req.user {
        for line in u.lines() {
            if let Some(rest) = line.strip_prefix('[')
                && let Some((k, _)) = rest.split_once(']')
            {
                out.push(k.to_string());
            }
        }
    }
    out
}

/// 录制的回复用得上：它挂的原著段落（请求里带着原著正文时）、连的已有卡片，这次请求里都有。
/// 引的来源（S1……）不查：来源不在时由核对拦下（「来源不存在」），正是要测的
fn fits(reply: &Value, keys: &[String], has_original: bool) -> bool {
    let has = |k: &str| keys.iter().any(|x| x == k);
    let sentence_ok = |s: &Value| match s.get("paragraph").and_then(Value::as_str) {
        Some(p) if has_original && !p.starts_with("译:") => has(p),
        _ => true,
    };
    let cards_ok = reply["cards"].as_array().into_iter().flatten().flat_map(|c| c["sentences"].as_array().into_iter().flatten()).all(sentence_ok);
    let relations_ok = reply["relations"].as_array().into_iter().flatten().all(|r| {
        sentence_ok(&r["note"]) && [&r["from"], &r["to"]].iter().all(|end| end.as_str().is_none_or(|k| !k.starts_with('K') || has(k)))
    });
    cards_ok && relations_ok
}

/// 请求某一节里的 `[KEY] 文字`
fn section_lines<'a>(req: &'a ModelRequest, heading: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    for u in &req.user {
        if !u.starts_with(heading) {
            continue;
        }
        for line in u.lines().skip(1) {
            if let Some(rest) = line.strip_prefix('[')
                && let Some((k, t)) = rest.split_once("] ")
            {
                out.push((k, t));
            }
        }
    }
    out
}

fn first_sentence(t: &str) -> String {
    let cut = t.char_indices().find(|(i, c)| matches!(c, '.' | '。' | ';' | '；') && *i > 20).map(|(i, c)| i + c.len_utf8()).unwrap_or(t.len().min(t.char_indices().nth(120).map(|(i, _)| i).unwrap_or(t.len())));
    t[..cut].trim().to_string()
}

/// 按请求现编一份回复（开发版里用）
fn generate(req: &ModelRequest) -> Value {
    let kinds: Vec<String> = req.schema.pointer("/properties/cards/items/properties/kind/enum").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let only_relations = req.system.contains("这一步只出：（不出卡片）");
    let paragraphs = section_lines(req, "## 原著正文");
    let cards = section_lines(req, "## 已有的卡片");
    let sources = section_lines(req, "## 来源");
    let mut out_cards = Vec::new();
    let mut relations = Vec::new();
    if !only_relations {
        let kind = kinds.iter().find(|k| *k != "comment" && *k != "counterfactual").cloned().unwrap_or_else(|| kinds.first().cloned().unwrap_or("note".into()));
        if kinds.iter().any(|k| k == "comment") {
            for (i, (sid, _)) in sources.iter().enumerate().take(2) {
                let text = req.user.iter().find(|u| u.starts_with("## 来源")).and_then(|u| u.split(&format!("[{sid}]")).nth(1)).and_then(|rest| rest.lines().nth(1)).unwrap_or("").trim();
                let quote: String = text.chars().take(40).collect();
                out_cards.push(json!({ "key": format!("c{}", i + 1), "kind": "comment", "title": format!("模拟评论：来源 {sid}"), "about": cards.first().map(|(k, _)| vec![k.to_string()]).unwrap_or_default(),
                    "sentences": [ { "category": "others", "text": quote, "paragraph": null, "quote": quote, "zh": null, "source": sid }, { "category": "explanation", "text": "（模拟）这条来源对卡片的看法。", "paragraph": null, "quote": null, "zh": null, "source": null } ] }));
            }
        } else if kinds.iter().any(|k| k == "counterfactual") {
            let premise = paragraphs.first().map(|(k, t)| (k.to_string(), first_sentence(t)));
            out_cards.push(json!({ "key": "c1", "kind": "counterfactual", "title": "（模拟）如果这个前提不成立", "about": cards.first().map(|(k, _)| vec![k.to_string()]).unwrap_or_default(),
                "sentences": [ { "category": "speculation", "text": "（模拟）假设前提反过来。", "paragraph": premise.as_ref().map(|p| p.0.clone()), "quote": premise.map(|p| p.1), "zh": null, "source": null }, { "category": "speculation", "text": "（模拟）结论会跟着变。", "paragraph": null, "quote": null, "zh": null, "source": null } ] }));
        } else {
            for (i, (k, t)) in paragraphs.iter().enumerate().take(3) {
                let q = first_sentence(t);
                out_cards.push(json!({ "key": format!("c{}", i + 1), "kind": kind, "title": format!("（模拟）{k} 的要点"), "about": [],
                    "sentences": [ { "category": "original", "text": q, "paragraph": k, "quote": q, "zh": null, "source": null }, { "category": "explanation", "text": format!("（模拟）这一段的意思。"), "paragraph": k, "quote": null, "zh": null, "source": null } ] }));
            }
        }
    }
    if req.schema.to_string().contains("relations") && (only_relations || req.system.contains("以及关系")) {
        let ids: Vec<&str> = cards.iter().map(|(k, _)| *k).collect();
        for w in ids.windows(2).take(3) {
            relations.push(json!({ "from": w[0], "to": w[1], "kind": "example", "note": { "category": "explanation", "text": "（模拟）两张卡片的关系。", "paragraph": null, "quote": null, "zh": null, "source": null } }));
        }
        if !only_relations && let (Some(first), true) = (ids.first(), !out_cards.is_empty()) {
            relations.push(json!({ "from": "c1", "to": first, "kind": "revises", "note": { "category": "explanation", "text": "（模拟）新卡片针对的那张。", "paragraph": null, "quote": null, "zh": null, "source": null } }));
        }
    }
    json!({ "cards": out_cards, "relations": relations })
}

impl Model for MockModel {
    fn info(&self) -> ModelRef {
        ModelRef { provider: "mock".into(), model: "模拟模型".into() }
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError> {
        let steps = (self.delay.as_millis() / 50).max(1);
        for _ in 0..steps {
            if cancel.load(Ordering::SeqCst) {
                return Err(PortError::cancelled());
            }
            if !self.delay.is_zero() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if let Ok(mut s) = self.seen.lock() {
            s.push(req.clone());
        }
        let keys = keys_in(req);
        let has_original = req.user.iter().any(|u| u.starts_with("## 原著正文"));
        let text = match recorded(&req.tag).and_then(|r| serde_json::from_str::<Value>(r).ok()).filter(|v| fits(v, &keys, has_original)) {
            Some(v) => v.to_string(),
            None => generate(req).to_string(),
        };
        let text = match &self.tamper {
            Some(f) => f(req, text),
            None => text,
        };
        let usage = Usage { input_tokens: req.sent_chars() as u64, output_tokens: text.chars().count() as u64, searches: 0, cost_usd: 0.0 };
        Ok(ModelReply { text, model: "模拟模型".into(), usage, hits: Vec::new(), searched: Vec::new() })
    }
}

// ---------- 来源 ----------

/// 合成的书评（模拟文献库里的一条）
pub const LIBRARY_REVIEW: &str = "示例书评（合成，测试用）。A synthetic review of the Wealth of Nations, written for tests. The reviewer accepts the gains from specialisation, but warns that the division of labour, pushed far enough, makes the worker as stupid as it is possible for a human creature to become. 书评的作者认为，分工的好处和坏处要一起看。";
pub const LIBRARY_TITLE: &str = "示例书评：分工的两面（合成）";
pub const LIBRARY_ITEM_ID: i64 = 9001;
/// 合成的评论网页（模拟网页检索搜到的）
pub const WEB_URL: &str = "https://example.org/synthetic/pin-figures";
pub const WEB_TITLE: &str = "Example critique: where did the pin figures come from? (synthetic)";
pub const WEB_PAGE: &str = "Example critique (synthetic page for tests). Historians of economic thought have argued that the famous pin figures were copied from an encyclopaedia rather than observed. The critic does not deny that specialisation raises output.";

/// 不是真的 SHA-256：模拟接口里给快照起名用（FNV-1a，64 个十六进制字）
pub fn fake_sha(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}").repeat(4)
}

/// 快照：文字按 fake_sha 存在内存里（模拟的文献库和下载共用）
#[derive(Clone, Default)]
pub struct Store(pub Arc<Mutex<HashMap<String, String>>>);

impl Store {
    pub fn put(&self, text: &str) -> String {
        let sha = fake_sha(text);
        if let Ok(mut m) = self.0.lock() {
            m.insert(sha.clone(), text.to_string());
        }
        sha
    }

    pub fn get(&self, sha: &str) -> Option<String> {
        self.0.lock().ok().and_then(|m| m.get(sha).cloned())
    }
}

pub struct MockShelf {
    pub store: Store,
    /// 文献库里有没有那条书评
    pub has_review: bool,
}

impl Shelf for MockShelf {
    fn find(&self, terms: &[String], max: usize, _: &AtomicBool) -> Result<Vec<ShelfHit>, String> {
        let hit = terms.iter().any(|t| {
            let l = t.to_lowercase();
            l.contains("division") || l.contains("labour") || t.contains("分工") || t.contains("劳动")
        });
        if !self.has_review || !hit || max == 0 {
            return Ok(Vec::new());
        }
        Ok(vec![ShelfHit { item_id: LIBRARY_ITEM_ID, attachment_id: None, title: LIBRARY_TITLE.into(), text: LIBRARY_REVIEW.into() }])
    }

    fn keep(&self, text: &str) -> Result<String, String> {
        Ok(self.store.put(text))
    }

    fn text(&self, source: &CommentSource) -> Result<String, String> {
        source.sha256.as_deref().and_then(|s| self.store.get(s)).ok_or_else(|| "快照不见了".to_string())
    }
}

pub struct MockSearch;

impl Search for MockSearch {
    fn engine(&self) -> String {
        "mock".into()
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn search(&self, _query: &str, _max: usize, _: &AtomicBool) -> Result<SearchReply, PortError> {
        Ok(SearchReply {
            hits: vec![
                Hit { url: WEB_URL.into(), title: WEB_TITLE.into(), snippet: String::new(), published: None },
                Hit { url: "https://example.org/synthetic/missing".into(), title: "A page that is gone".into(), snippet: String::new(), published: None },
            ],
            usage: Usage { searches: 1, ..Default::default() },
        })
    }
}

pub struct MockFetch {
    pub store: Store,
}

impl Fetch for MockFetch {
    fn fetch(&self, url: &str, _: &AtomicBool) -> Result<Fetched, String> {
        if url != WEB_URL {
            return Err("404：找不到这个网页（模拟）".into());
        }
        let sha = self.store.put(WEB_PAGE);
        Ok(Fetched { final_url: url.into(), fetched_at: "2026-11-13T10:00:00".into(), sha256: sha, kind: "html".into(), content_type: Some("text/html".into()), ext: "html".into(), bytes: WEB_PAGE.len() as u64, title: Some(WEB_TITLE.into()), text: Ok(WEB_PAGE.into()), via: None })
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        source.sha256.as_deref().and_then(|s| self.store.get(s)).ok_or_else(|| "快照不见了".into())
    }
}

/// 模拟的拒答 / 出错（测试用）
pub struct FailingModel(pub ErrorKind);

impl Model for FailingModel {
    fn info(&self) -> ModelRef {
        ModelRef { provider: "mock".into(), model: "会出错的模拟模型".into() }
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn complete(&self, _: &ModelRequest, _: &AtomicBool) -> Result<ModelReply, PortError> {
        Err(PortError::new(self.0, "模拟：这一步出错了。"))
    }
}
