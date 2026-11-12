//! 端到端：真实的接口实现（Anthropic 的流、OpenAI 兼容的 DeepSeek、Tavily、本机下载）全部连本地起的假服务器，不连外网。
//! 计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告 → 导出（Markdown / HTML / Word）→ 加入文献库 → 重查对比；
//! 每一笔费用都对得上账本。假服务器的回复用合成夹具（虚构的示例市）现算。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use research_engine::fixture::{self, QUESTION};
use research_engine::model::{CheckStatus, ModelRef, Run, Source, Status, StepKind};
use research_engine::ports::{Ports, Progress};
use research_engine::{audit, flow, render, report};
use rusqlite::Connection;
use serde_json::{Value, json};

use super::anthropic::Anthropic;
use super::commands::{Format, export_bytes};
use super::download::{Downloader, Http, Limits};
use super::meter::Meter;
use super::openai::OpenAiCompatible;
use super::snapshot::Snapshots;
use super::store::{self, tests::DbRecorder};
use super::tavily::{Depth, Tavily};
use super::testing::{FakeServer, Resp};
use super::{fixtures, library};
use crate::agent::usage::Ledger;
use crate::db::tests::{memory_db, temp_dir};

fn d(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

/// 一段文字回答写成 Anthropic 的流
fn sse(text: &str, input: u64, output: u64) -> String {
    let ev = |name: &str, v: Value| format!("event: {name}\ndata: {}\n\n", d(&v));
    let mid = text.char_indices().nth(text.chars().count() / 2).map(|(i, _)| i).unwrap_or(0);
    [
        ev("message_start", json!({ "type": "message_start", "message": { "model": "claude-opus-5-5", "usage": { "input_tokens": input, "output_tokens": 1 } } })),
        ev("content_block_start", json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "", "signature": "" } })),
        ev("content_block_delta", json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "sig" } })),
        ev("content_block_stop", json!({ "type": "content_block_stop", "index": 0 })),
        ev("content_block_start", json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } })),
        ev("content_block_delta", json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": &text[..mid] } })),
        ev("content_block_delta", json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": &text[mid..] } })),
        ev("content_block_stop", json!({ "type": "content_block_stop", "index": 1 })),
        ev("message_delta", json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": output } })),
        ev("message_stop", json!({ "type": "message_stop" })),
    ]
    .concat()
}

struct World {
    web: FakeServer,
    anthropic: FakeServer,
    deepseek: FakeServer,
    tavily: FakeServer,
    /// 第几轮（重查时网站和摘录都换成第 2 轮的）
    round: Arc<AtomicU32>,
}

fn world() -> World {
    let round = Arc::new(AtomicU32::new(1));

    let r = round.clone();
    let web = FakeServer::start(move |req| {
        let page = fixtures::page(&req.path, r.load(Ordering::SeqCst));
        Resp::new(page.status, page.content_type, page.body)
    });

    // Anthropic：按系统提示词分辨计划和结论；结论按请求里的事实表现拼
    let anthropic = FakeServer::start(|req| {
        let body = req.json();
        let system = body["system"].as_str().unwrap_or("");
        let user: String = body["messages"][0]["content"].as_array().map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        if system.contains("计划员") {
            Resp::sse(&sse(fixture::PLAN_REPLY, 2_100, 1_600))
        } else if system.contains("结论撰写人") {
            Resp::sse(&sse(&fixture::conclude_reply(&user), 6_400, 2_300))
        } else {
            Resp::json(400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"unexpected"}}"#)
        }
    });

    // DeepSeek：按请求里的网址给录制的摘录
    let r = round.clone();
    let deepseek = FakeServer::start(move |req| {
        let body = req.json();
        let user = body["messages"][1]["content"].as_str().unwrap_or("").to_string();
        let url = user.lines().find_map(|l| l.strip_prefix("网址：")).unwrap_or("").to_string();
        let first = !user.contains("这是正文的第 2 段") && !user.contains("这是正文的第 3 段");
        let content = match fixture::doc_for(&url).filter(|_| first) {
            Some(doc) => doc.extract_in(r.load(Ordering::SeqCst)).to_string(),
            None => d(&json!({ "publisher": "", "published": "", "facts": [] })),
        };
        let reply = json!({ "id": "chatcmpl-e2e", "model": "deepseek-flash", "choices": [{ "index": 0, "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }], "usage": { "prompt_tokens": 3_000, "completion_tokens": 600, "prompt_cache_hit_tokens": 1_000, "prompt_cache_miss_tokens": 2_000 } });
        Resp::json(200, &d(&reply))
    });

    // Tavily：搜到的网址指向上面那个网站
    let (r, web_base) = (round.clone(), web.base.clone());
    let tavily = FakeServer::start(move |req| {
        let q = req.json()["query"].as_str().unwrap_or("").to_string();
        let results: Vec<Value> = fixture::hits_for(&q, r.load(Ordering::SeqCst))
            .into_iter()
            .map(|p| json!({ "title": fixture::doc_for(p).map(|d| d.title).unwrap_or("示例市统计资料"), "url": format!("{web_base}{p}"), "content": "（合成的搜索结果）", "score": 0.5 }))
            .collect();
        Resp::json(200, &d(&json!({ "query": q, "results": results, "usage": { "credits": 1 }, "request_id": "tvly-e2e" })))
    });

    World { web, anthropic, deepseek, tavily, round }
}

/// 用真实的接口实现查一次（`previous`：重查）
fn investigate(w: &World, c: &Connection, dir: &std::path::Path, question_id: i64, uid: &str, previous: Option<&Run>) -> Run {
    let mut m = Meter::new(dir, 30.0, Default::default());
    m.now = || "2026-11-12T09:00:00.000Z".into();
    let meter = Arc::new(m);
    let planner = Anthropic::new(Some(&w.anthropic.base), "sk-ant-e2e".into(), "claude-opus-5-5", meter.clone());
    let worker = OpenAiCompatible::new(Some(&w.deepseek.base), "sk-e2e".into(), Some("deepseek-flash"), meter.clone());
    let search = Tavily::new(Some(&w.tavily.base), "tvly-e2e".into(), Depth::Basic, meter);
    let fetch = Downloader::new(Box::new(Http::new(Duration::from_secs(5))), dir, Limits { timeout: Duration::from_secs(5), host_gap: Duration::ZERO, allow_private: true, ..Default::default() });
    let cancel = AtomicBool::new(false);
    let mut recorder = DbRecorder { c, question_id, saves: 0 };
    let now = || "2026-11-12T09:00:00.000Z".to_string();
    let quiet = |_: Progress| {};
    let mut ports = Ports { planner: &planner, worker: &worker, search: &search, fetch: &fetch, recorder: &mut recorder, now: &now, progress: &quiet, cancel: &cancel };
    let mut config = store::tests::config();
    config.planner = ModelRef { provider: "anthropic".into(), model: "claude-opus-5-5".into() };
    config.worker = ModelRef { provider: "openai_compatible".into(), model: "deepseek-flash".into() };
    config.search_engine = "tavily".into();
    let mut run = match previous {
        Some(p) => flow::rerun_from(p, uid, config, now()),
        None => flow::plan(QUESTION, uid, 1, None, config, 0, &mut ports).unwrap(),
    };
    flow::execute(&mut run, &mut ports).unwrap();
    assert!(!run.mock);
    assert_eq!((run.status, run.error.clone()), (Status::Done, None));
    run
}

fn texts(dir: &std::path::Path) -> impl FnMut(&Source) -> Result<String, String> + '_ {
    let snapshots = Snapshots::new(dir);
    move |s: &Source| snapshots.text(s.sha256.as_deref().unwrap_or(""))
}

#[test]
fn whole_investigation_through_the_real_interfaces_and_fake_servers() {
    let (w, c, dir) = (world(), memory_db(), temp_dir("research-e2e"));
    let q = store::question_create(&c, QUESTION, "2026-11-12T09:00:00.000Z").unwrap();
    let run = investigate(&w, &c, &dir, q, "01RUNE2EAAAAAAAAAAAAAAAAAA", None);
    assert_eq!(store::run_load(&c, &run.uid).unwrap().status, Status::Done);

    // 检索：7 个搜索词都走了 Tavily
    assert_eq!(w.tavily.requests().len(), 7);
    assert!(w.tavily.requests().iter().all(|r| r.header("authorization") == Some("Bearer tvly-e2e")));
    // 下载：都是 Pith 自己去网站拿的；一个 404，一个扫描件取不出文字
    assert_eq!(run.sources.len(), 9);
    assert!(run.sources.iter().all(|s| s.url.starts_with(&w.web.base)));
    assert!(run.sources.iter().any(|s| s.fetch_error.as_deref().is_some_and(|e| e.contains("404"))));
    assert!(run.sources.iter().any(|s| s.text_error.as_deref().is_some_and(|e| e.contains("扫描件"))));
    assert_eq!(w.web.requests().len(), 9);
    // 每个来源的快照：原件的 SHA-256 和网站给的字节一致
    for s in run.sources.iter().filter(|s| s.sha256.is_some()) {
        let path = s.url.trim_start_matches(&w.web.base);
        assert_eq!(s.sha256.as_deref(), Some(zotero_import::util::sha256_hex(&fixtures::page(path, 1).body).as_str()), "{path}");
        assert!(s.fetched_at.is_some() && s.final_url.as_deref() == Some(s.url.as_str()));
    }
    // 摘证据（DeepSeek）7 次，核对：13 条核实、1 条没核上
    assert_eq!(w.deepseek.requests().len(), 7);
    assert_eq!((run.facts.len(), run.verified_facts().count()), (14, 13));
    assert_eq!(run.facts.iter().find(|f| f.check.status == CheckStatus::Unverified).unwrap().check.reason.as_deref(), Some("摘录在保存的正文里找不到"));
    // 结论（Opus 5.5）：5 条，数字全部对上
    assert_eq!(w.anthropic.requests().len(), 2);
    assert_eq!(run.conclusions.len(), 5);
    assert!(run.conclusions.iter().all(|k| k.issues.is_empty()));
    assert!(run.conclusions[0].text().contains("2024 年为 12.3%，比 2023 年的 13.4% 低 1.1 个百分点"));
    // 审计：从头再核一遍（读保存的快照），没有问题
    assert_eq!(audit::audit(&run, &mut texts(&dir)), []);
    let r = report::build(&run, None);
    assert!(audit::audit_order(&r).is_empty());

    // 费用：报告里的每一步 = 账本里的每一行
    let ledger = Ledger::new(&dir).all().unwrap();
    let count = |purpose: &str| ledger.iter().filter(|c| c.purpose == purpose).count();
    assert_eq!((count("research_plan"), count("research_search"), count("research_extract"), count("research_conclude")), (1, 7, 7, 1));
    assert!(ledger.iter().all(|c| !c.mock && !c.estimated));
    let total: f64 = ledger.iter().map(|c| c.cost_usd).sum();
    assert!((run.cost_usd() - total).abs() < 1e-9, "{} vs {total}", run.cost_usd());
    let opus = (2_100.0 * 4.0 + 1_600.0 * 20.0 + 6_400.0 * 4.0 + 2_300.0 * 20.0) / 1e6;
    let deepseek = 7.0 * (3_000.0 * 0.30 + 600.0 * 1.20) / 1e6;
    let tavily = 7.0 * 0.008;
    assert!((total - (opus + deepseek + tavily)).abs() < 1e-9);
    assert!((Ledger::new(&dir).month_total("2026-11").unwrap() - total).abs() < 1e-9);
    assert_eq!(run.steps.iter().filter(|s| s.kind == StepKind::Extract && s.provider == "openai_compatible").count(), 7);

    // 导出：三种都打得开，内容一致
    let md = String::from_utf8(export_bytes(&run, None, Format::Markdown).unwrap()).unwrap();
    let html = String::from_utf8(export_bytes(&run, None, Format::Html).unwrap()).unwrap();
    let word = docx_engine::package::Package::read(&export_bytes(&run, None, Format::Docx).unwrap()).unwrap();
    let word_text = docx_engine::outline::doc_text(&word).unwrap();
    for part in ["数字由示例市统计局发布", "默认等级表", "示例大学劳动经济研究所", "取文字失败"] {
        assert!(md.contains(part) && html.contains(part) && word_text.contains(part), "三种导出都要有「{part}」");
    }
    // 结论里的数字都能点回事实表
    let f = &run.conclusions[0].facts;
    assert!(md.contains(&format!("**13.4%**〔{}〕", f[1])) && html.contains(&format!("<a class=\"ref\" href=\"#{}\">13.4%</a>", f[1])), "{md}");
    assert!(word_text.contains("比 2023 年的 13.4% 低 1.1 个百分点"));
    assert!(html.starts_with("<!doctype html>") && !html.contains("<link") && !html.contains("src="));
    assert!(md.contains(&format!("{}/stats/bulletin-2024.html", w.web.base)));

    // 加入文献库：原件作为附件
    let attachments = dir.join("attachments");
    let snapshots = Snapshots::new(&dir);
    let (added, _) = library::add(&c, &attachments, &store::run_load(&c, &run.uid).unwrap(), &["S1".into(), "S3".into()], &|s| snapshots.original_of(s)).unwrap();
    assert!(added.iter().all(|a| a.created && a.attachment_id.is_some()));
    let pdf_sha: String = c.query_row("SELECT sha256 FROM attachments WHERE id = ?1", [added[1].attachment_id.unwrap()], |r| r.get(0)).unwrap();
    assert_eq!(Some(pdf_sha), run.source("S3").unwrap().sha256.clone());

    // 重查：网站上的公报修订了，网友博客没了，多了 2025 年上半年的数据
    w.round.store(2, Ordering::SeqCst);
    let previous = store::run_load(&c, &run.uid).unwrap();
    let second = investigate(&w, &c, &dir, q, "01RUNE2EBBBBBBBBBBBBBBBBBB", Some(&previous));
    assert_eq!(second.seq, 2);
    assert_eq!(w.anthropic.requests().len(), 3, "重查不再花钱拆问题");
    assert_eq!(audit::audit(&second, &mut texts(&dir)), []);
    let r = report::build(&second, Some(&previous));
    let diff = r.diff.clone().unwrap();
    assert_eq!(diff.changed.len(), 1);
    assert_eq!((diff.changed[0].old.value.as_str(), diff.changed[0].new.value.as_str()), ("12.3%", "12.6%"));
    assert_eq!(diff.added.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["11.9%"]);
    assert_eq!(diff.removed.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["30%"]);
    assert_eq!((diff.sources_added.len(), diff.sources_removed.len()), (1, 1));
    assert_eq!(diff.conclusions_added.len(), 3);
    // 修订后的公报是另一份快照；两份都还在
    assert_ne!(second.source("S1").unwrap().sha256, previous.source("S1").unwrap().sha256);
    assert_eq!(store::snapshot_shas(&c).unwrap().len(), 10);
    let md = render::markdown(&render::document(&r));
    assert!(md.contains("## 和上一次调查的对比") && md.contains("| 12.3% | **12.6%**"));
}

#[test]
fn a_tampered_snapshot_is_caught_after_the_fact() {
    let (w, c, dir) = (world(), memory_db(), temp_dir("research-e2e-tamper"));
    let q = store::question_create(&c, QUESTION, "t").unwrap();
    let run = investigate(&w, &c, &dir, q, "01RUNE2ECCCCCCCCCCCCCCCCCC", None);
    // 有人改了保存的正文（快照是只读的，要先解除）：审计查出来
    let sha = run.source("S1").unwrap().sha256.clone().unwrap();
    let path = Snapshots::new(&dir).text_file(&sha).unwrap();
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&path, perm).unwrap();
    let text = std::fs::read_to_string(&path).unwrap().replace("12.3%", "12.8%");
    std::fs::write(&path, text).unwrap();
    let found = audit::audit(&store::run_load(&c, &run.uid).unwrap(), &mut texts(&dir));
    assert!(found.iter().any(|f| f.target == "F1" && f.kind == audit::Kind::Quote), "{found:?}");
}
