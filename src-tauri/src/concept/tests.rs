//! 概念研究 App 一侧的测试：存取（新库 / 从 v7 升级）、段落检索（中文搜英文原著）、OpenAI Responses 与网页搜索、Mathpix
//! （都连本地假服务器，不连外网），以及真实的接口实现跑一遍整章分析和反方（文献库 + 上网）→ 存库 → 审计 → 三种导出 → 加入文献库，
//! 每一笔费用都对上账本。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use concept_engine::align;
use concept_engine::fixture::{self, Flavor};
use concept_engine::flow::{self, Ports, Recorder};
use concept_engine::mock::MockModel;
use concept_engine::model::{CardKind, Category, Run, RunStatus, Scope, SourceOrigin, Status};
use concept_engine::prompts::Books;
use concept_engine::skill::{self, Entry};
use concept_engine::text::{self, Role};
use concept_engine::{audit, mathpix as text_mathpix};
use research_engine::model::ModelRef;
use research_engine::ports::{ErrorKind, Model, ModelRequest, Purpose, Search};
use rusqlite::Connection;
use serde_json::{Value, json};

use super::commands::{self, Format};
use super::library::{self as shelf, Copying, DbShelf};
use super::mathpix::Mathpix;
use super::responses::{self, OpenAiSearch, Responses};
use super::{fixtures, search, store};
use crate::agent::usage::{Ledger, Prices};
use crate::db::tests::temp_dir;
use crate::edit::{self, ItemPatch};
use crate::paths::DataPaths;
use crate::research::download::{Downloader, Http, Limits};
use crate::research::meter::Meter;
use crate::research::snapshot::Snapshots;
use crate::research::testing::{FakeServer, Resp};
use crate::{AppState, db};

const KEY: &str = "sk-proj-TESTKEY0123456789abcdefTESTKEY";

fn item_with_file(c: &Connection, attachments: &std::path::Path, dir: &std::path::Path, title: &str, name: &str, bytes: &[u8]) -> (i64, i64) {
    let id = edit::item_create(c, "book", None).unwrap();
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("title".to_string(), title.to_string());
    edit::item_update(c, id, &ItemPatch { fields, ..Default::default() }).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let (aid, _) = edit::attachment_add(c, attachments, Some(id), &path).unwrap();
    (id, aid)
}

/// 新库里两本书（英文夹具、合成中文），解析、存好、对齐
fn library_with_books(name: &str) -> (crate::db::tests::TempDir, AppState, String, String, i64) {
    let dir = temp_dir(name);
    let state = AppState::init(DataPaths::new(dir.join("data")));
    let (en, zh, en_item) = state
        .with_db(|c| {
            let attachments = state.paths.attachments.clone();
            let (en_item, en_att) = item_with_file(c, &attachments, &dir, "The Wealth of Nations (fixture)", "wealth.epub", &fixture::english_epub(Flavor::Epub3));
            let (zh_item, zh_att) = item_with_file(c, &attachments, &dir, "国富论（合成）", "guofu.epub", &fixture::chinese_epub());
            let f = shelf::attachment_file(c, &attachments, en_att)?;
            assert_eq!(f.ext, "epub");
            let (t, meta) = commands::parse_epub(&std::fs::read(&f.path).unwrap())?;
            let en = commands::save_text(c, &state.paths.backups, &t, &meta, Some(en_att), Some(en_item), &f.sha256, None)?;
            let f = shelf::attachment_file(c, &attachments, zh_att)?;
            let (t, meta) = commands::parse_epub(&std::fs::read(&f.path).unwrap())?;
            let zh = commands::save_text(c, &state.paths.backups, &t, &meta, Some(zh_att), Some(zh_item), &f.sha256, Some(&en))?;
            Ok((en, zh, en_item))
        })
        .unwrap();
    (dir, state, en, zh, en_item)
}

#[test]
fn texts_are_saved_linked_aligned_and_searchable() {
    let (dir, state, en, zh, _) = library_with_books("concept-texts");
    // 存之前做了「导入前」备份
    let backups: Vec<String> = std::fs::read_dir(dir.join("data/backups")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(backups.iter().any(|b| b.contains("before-import")), "{backups:?}");
    state
        .with_db(|c| {
            // 读回来和解析出来的一样
            let loaded = store::text_load(c, &en)?;
            let parsed = fixture::english();
            assert_eq!(loaded.paragraphs, parsed.paragraphs);
            assert_eq!(loaded.units, parsed.units);
            assert_eq!((loaded.numbering, loaded.page_source.as_str()), (parsed.numbering, "page-list"));
            // 互相关联，对齐存好了
            assert_eq!(store::partner(c, &en)?, Some((zh.clone(), true)));
            assert_eq!(store::partner(c, &zh)?, Some((en.clone(), false)));
            let al = store::alignment_load(c, &en, &zh)?.unwrap();
            assert_eq!(al, align::align(&parsed, &fixture::chinese()));
            // 检索：英文词、中文词（先搜中译再映射回原著）、旁注里的词（映射到它挂的正文段）
            let hits = commands::search_hits(c, &en, "pin-maker", 20)?;
            assert_eq!(hits.iter().map(|h| (h.key.as_str(), h.via_translation)).collect::<Vec<_>>(), [("I.i.3", false)]);
            let hits = commands::search_hits(c, &en, "制针", 20)?;
            assert!(hits.iter().any(|h| h.key == "I.i.3" && h.via_translation));
            let hits = commands::search_hits(c, &en, "分工；division of labour", 40)?;
            let chapters: std::collections::BTreeSet<&str> = hits.iter().map(|h| h.unit_key.as_str()).collect();
            assert!(chapters.contains("I.i") && chapters.contains("I.ii"), "{chapters:?}");
            assert!(commands::search_hits(c, &en, "  ", 20)?.is_empty());
            // 搜索索引被整个重建过（恢复备份）：对上主库
            c.execute_batch("DELETE FROM search.concept_paras").unwrap();
            assert!(search::ensure(c)?);
            assert!(!search::ensure(c)?);
            // 删掉译本：它的段落不在检索里了
            let zid = store::text_id(c, &zh)?;
            search::remove_text(c, zid)?;
            store::text_delete(c, &zh)?;
            assert!(!search::ensure(c)?);
            assert_eq!(store::partner(c, &en)?, None);
            Ok(())
        })
        .unwrap();
}

#[test]
fn concept_tables_upgrade_from_v7() {
    // v7 的库（有文献）升到最新，概念研究的表能用，外键连得上原来的文献
    let mut c = Connection::open_in_memory().unwrap();
    db::configure(&c).unwrap();
    db::upgrade(&mut c, &db::MIGRATIONS[..7], None).unwrap();
    assert_eq!(db::user_version(&c).unwrap(), 7);
    let item = edit::item_create(&c, "book", None).unwrap();
    assert_eq!(db::upgrade(&mut c, db::MIGRATIONS, None).unwrap(), db::latest());
    crate::search::attach_memory(&c).unwrap();
    let t = fixture::chinese();
    let uid = "TEXT-V7";
    store::text_save(&c, &store::NewText { uid, item_id: Some(item), attachment_id: None, source_sha256: "x", text: &t, meta: Default::default(), original_uid: None, now: "2026-11-13T10:00:00" }).unwrap();
    assert_eq!(store::text_load(&c, uid).unwrap().paragraphs, t.paragraphs);
    assert_eq!(store::texts_of_item(&c, item).unwrap(), [uid.to_string()]);
    // 文献删了：文本留着，只是不再连着
    c.execute("DELETE FROM items WHERE id = ?1", [item]).unwrap();
    assert_eq!(store::text_row_get(&c, uid).unwrap().item_id, None);
}

struct Rec<'a> {
    c: &'a Connection,
    text: String,
}

impl Recorder for Rec<'_> {
    fn save(&mut self, run: &Run) -> Result<(), String> {
        store::run_save(self.c, &self.text, run)
    }
}

/// 给「什么都有」的测试库（`transfer::tests::rich_library`，也是冻结样本库的来源）加上概念研究的数据：
/// 两份夹具文本（英文连到 `item`、中文连上原著并对齐）、一次整章分析（模拟模型）和一次反方（文献库 + 上网，有来源）。
pub(crate) fn sample(c: &Connection, backups: &std::path::Path, item: i64) {
    let (et, em) = commands::parse_epub(&fixture::english_epub(Flavor::Epub3)).unwrap();
    let en = commands::save_text(c, backups, &et, &em, None, Some(item), "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9", None).unwrap();
    let (zt, zm) = commands::parse_epub(&fixture::chinese_epub()).unwrap();
    let zh = commands::save_text(c, backups, &zt, &zm, None, None, "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0", Some(&en)).unwrap();
    let (et, zt) = (store::text_load(c, &en).unwrap(), store::text_load(c, &zh).unwrap());
    let al = store::alignment_load(c, &en, &zh).unwrap().unwrap();
    let books = Books { original: &et, translation: Some(&zt), alignment: Some(&al) };
    let skills = skill::builtin();
    let store_ = concept_engine::mock::Store::default();
    // 模拟文献库里的那条书评是别的库里的条目编号（外键对不上），这里只上网找
    let shelf = concept_engine::mock::MockShelf { store: store_.clone(), has_review: false };
    let fetch = concept_engine::mock::MockFetch { store: store_ };
    let model = MockModel::default();
    let run_skill = |name: &str, uid: &str, scope: Scope, targets: Vec<concept_engine::model::Card>| -> Run {
        let sk = skills.iter().find(|s| s.name == name).unwrap();
        let mut run = flow::new_run(uid, sk, scope, targets, ModelRef { provider: "mock".into(), model: "模拟模型".into() }, "medium", true, "2026-11-13T10:00:00.000Z".into()).unwrap();
        let mut rec = Rec { c, text: en.clone() };
        let now = || "2026-11-13T10:00:05.000Z".to_string();
        let progress = |_| {};
        let cancel = AtomicBool::new(false);
        let mut ports = Ports { model: &model, search: &concept_engine::mock::MockSearch, fetch: &fetch, shelf: &shelf, recorder: &mut rec, now: &now, progress: &progress, cancel: &cancel };
        flow::execute(&mut run, sk, &books, &mut ports).unwrap();
        assert_eq!(run.status, RunStatus::Done, "{:?}", run.error);
        run
    };
    let chapter = run_skill("chapter", "01JCONCEPTRUN0000000000001", chapter_scope(&et, "I.i", &zh, &en), Vec::new());
    let target = chapter.cards[0].clone();
    let mut sc = Scope { entry: Entry::Card, cards: vec![(chapter.uid.clone(), target.id.clone())], paragraphs: Vec::new(), unit: None, ..chapter_scope(&et, "I.i", &zh, &en) };
    sc.with = vec!["web".into()];
    let critics = run_skill("critics", "01JCONCEPTRUN0000000000002", sc, vec![target]);
    assert!(!critics.sources.is_empty());
}

fn chapter_scope(t: &text::Text, unit: &str, translation: &str, text_uid: &str) -> Scope {
    let u = t.chapters().into_iter().find(|u| u.key == unit).unwrap().index;
    Scope { entry: Entry::Chapter, text: text_uid.into(), translation: Some(translation.into()), paragraphs: t.body_of(u).map(|p| p.key.clone()).collect(), unit: Some(unit.into()), question: None, terms: Vec::new(), cards: Vec::new(), instruction: None, skip: Vec::new(), with: Vec::new() }
}

#[test]
fn runs_round_trip_through_the_database() {
    let (_dir, state, en, zh, _) = library_with_books("concept-runs");
    state
        .with_db(|c| {
            let (et, zt) = (store::text_load(c, &en)?, store::text_load(c, &zh)?);
            let al = store::alignment_load(c, &en, &zh)?.unwrap();
            let books = Books { original: &et, translation: Some(&zt), alignment: Some(&al) };
            let sk = skill::builtin().into_iter().next().unwrap();
            let mut run = flow::new_run("RUN-1", &sk, chapter_scope(&et, "I.i", &zh, &en), Vec::new(), ModelRef { provider: "mock".into(), model: "模拟模型".into() }, "medium", true, "2026-11-13T10:00:00".into())?;
            let store_ = concept_engine::mock::Store::default();
            let shelf = concept_engine::mock::MockShelf { store: store_.clone(), has_review: true };
            let fetch = concept_engine::mock::MockFetch { store: store_ };
            let mut rec = Rec { c, text: en.clone() };
            let now = || "2026-11-13T10:00:05".to_string();
            let progress = |_| {};
            let cancel = AtomicBool::new(false);
            let model = MockModel::default();
            let mut ports = Ports { model: &model, search: &concept_engine::mock::MockSearch, fetch: &fetch, shelf: &shelf, recorder: &mut rec, now: &now, progress: &progress, cancel: &cancel };
            flow::execute(&mut run, &sk, &books, &mut ports)?;
            assert_eq!(run.status, RunStatus::Done);
            let loaded = store::run_load(c, "RUN-1")?;
            assert_eq!(loaded, run);
            let list = store::runs_list(c, &en)?;
            assert_eq!(list.len(), 1);
            assert_eq!((list[0].unit.as_deref(), list[0].cards.len()), (Some("I.i"), 6));
            // Word 搜索窗：卡片里核对通过的原文（英文带页码）和中译
            let item: String = c.query_row("SELECT i.uid FROM items i JOIN concept_texts t ON t.item_id = i.id WHERE t.uid = ?1", [&en], |r| r.get(0)).unwrap();
            let quotes = commands::quotes_for_item(c, &item)?;
            assert!(quotes.iter().all(|q| q.pages.is_some() && q.paragraph.starts_with("I.")));
            assert!(quotes.iter().any(|q| q.paragraph == "I.i.1" && q.pages.as_deref() == Some("101")));
            let zh_item: String = c.query_row("SELECT i.uid FROM items i JOIN concept_texts t ON t.item_id = i.id WHERE t.uid = ?1", [&zh], |r| r.get(0)).unwrap();
            let zq = commands::quotes_for_item(c, &zh_item)?;
            assert!(!zq.is_empty() && zq.iter().all(|q| q.pages.is_none() && q.language == "zh"));
            // 启动时：进行中的标成已停止
            run.status = RunStatus::Running;
            store::run_save(c, &en, &run)?;
            assert_eq!(store::mark_interrupted(c)?, 1);
            assert_eq!(store::run_load(c, "RUN-1")?.status, RunStatus::Cancelled);
            store::run_delete(c, "RUN-1")?;
            assert!(store::runs_list(c, &en)?.is_empty());
            let n: i64 = c.query_row("SELECT COUNT(*) FROM concept_sentences", [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "删研究连带删卡片和句");
            Ok(())
        })
        .unwrap();
}

// ---------- OpenAI Responses ----------

fn meter(dir: &std::path::Path, budget: f64) -> Arc<Meter> {
    let mut m = Meter::new(dir, budget, Prices::default());
    m.now = || "2026-11-13T10:00:00".into();
    Arc::new(m)
}

fn responses_reply(text: &str, input: u64, cached: u64, output: u64) -> String {
    json!({
        "id": "resp_test", "object": "response", "status": "completed", "model": "gpt-6.1-sol-2026-11-01",
        "output": [
            { "type": "reasoning", "id": "rs_1", "summary": [] },
            { "type": "message", "id": "msg_1", "role": "assistant", "content": [ { "type": "output_text", "text": text, "annotations": [] } ] }
        ],
        "usage": { "input_tokens": input, "input_tokens_details": { "cached_tokens": cached }, "output_tokens": output, "output_tokens_details": { "reasoning_tokens": 300 }, "total_tokens": input + output }
    })
    .to_string()
}

fn req(max: u32) -> ModelRequest {
    ModelRequest { purpose: Purpose::Concept, system: "系统".into(), user: vec!["用户".into()], schema: json!({ "type": "object" }), max_tokens: max, typical_output_tokens: max / 3, web_searches: 0, tag: "chapter/concepts".into() }
}

#[test]
fn responses_request_shape_usage_and_cost() {
    let dir = temp_dir("concept-responses");
    let server = FakeServer::start(|_| Resp::json(200, &responses_reply("{\"cards\":[],\"relations\":[]}", 10_000, 4_000, 2_000)));
    let m = Responses::new(Some(&server.base), KEY.into(), "gpt-6.1-sol", "high", meter(&dir, 30.0));
    let reply = m.complete(&req(8000), &AtomicBool::new(false)).unwrap();
    assert_eq!(reply.text, "{\"cards\":[],\"relations\":[]}");
    let r = &server.requests()[0];
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/v1/responses"));
    assert_eq!(r.header("authorization"), Some(format!("Bearer {KEY}").as_str()));
    let b = r.json();
    assert_eq!(b["model"], "gpt-6.1-sol");
    assert_eq!(b["instructions"], "系统");
    assert_eq!(b["input"][0]["content"][0]["text"], "用户");
    assert_eq!(b["text"]["format"]["type"], "json_schema");
    assert_eq!(b["text"]["format"]["strict"], true);
    assert_eq!(b["reasoning"]["effort"], "high");
    assert_eq!(b["store"], false);
    assert_eq!(b["max_output_tokens"], 8000);
    // 费用：非缓存输入 × $2 + 缓存 × $0.10 + 输出 × $10（带日期的模型名按设置的模型计价）
    let want = (6_000.0 * 2.0 + 4_000.0 * 0.10 + 2_000.0 * 10.0) / 1e6;
    assert!((reply.usage.cost_usd - want).abs() < 1e-12);
    let calls = Ledger::new(&dir).all().unwrap();
    assert_eq!(calls.len(), 1);
    let c = &calls[0];
    assert_eq!((c.purpose.as_str(), c.provider.as_str(), c.kind.as_str(), c.tokens.input, c.tokens.cache_read, c.tokens.output), ("concept", "openai", "model", 6_000, 4_000, 2_000));
    assert!(c.note.as_deref().unwrap().contains("推理 300 token"));
    assert!((c.cost_usd - want).abs() < 1e-12);
}

#[test]
fn responses_long_context_pricing() {
    let p = Prices::default();
    let normal = responses::cost(&p, "gpt-6.1-sol", 200_000, 0, 10_000);
    assert!((normal - (0.4 + 0.1)).abs() < 1e-9);
    // 超过 272K：整次按输入 2 倍、输出 1.5 倍
    let long = responses::cost(&p, "gpt-6.1-sol", 300_000, 0, 10_000);
    assert!((long - (300_000.0 * 2.0 * 2.0 + 10_000.0 * 10.0 * 1.5) / 1e6).abs() < 1e-9);
    // 最坏情况：输入估计 ×1.1 越过 272K 也按加价算
    assert!(responses::worst_case(&p, "gpt-6.1-sol", 250_000, 16_000) > responses::cost(&p, "gpt-6.1-sol", 275_000, 0, 16_000) - 1e-9);
}

#[test]
fn responses_failures_are_reported_and_charged_correctly() {
    let dir = temp_dir("concept-responses-fail");
    // 截断：照样记账，不交结果
    let incomplete = json!({ "id": "r", "status": "incomplete", "incomplete_details": { "reason": "max_output_tokens" }, "model": "gpt-6.1-sol", "output": [], "usage": { "input_tokens": 1000, "output_tokens": 8000 } }).to_string();
    let s = FakeServer::start(move |_| Resp::json(200, &incomplete));
    let e = Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 30.0)).complete(&req(8000), &AtomicBool::new(false)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Failed);
    assert!(e.message.contains("截断") && e.usage.cost_usd > 0.0);
    // 拒答
    let refusal = json!({ "id": "r", "status": "completed", "model": "gpt-6.1-sol", "output": [ { "type": "message", "content": [ { "type": "refusal", "refusal": "no" } ] } ], "usage": { "input_tokens": 10, "output_tokens": 5 } }).to_string();
    let s = FakeServer::start(move |_| Resp::json(200, &refusal));
    assert!(Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 30.0)).complete(&req(8000), &AtomicBool::new(false)).unwrap_err().message.contains("拒绝"));
    // 密钥不对：整个停下，报错里没有密钥
    let s = FakeServer::start(|_| Resp::json(401, &json!({ "error": { "message": format!("Incorrect API key provided: {KEY}"), "code": "invalid_api_key" } }).to_string()));
    let e = Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 30.0)).complete(&req(8000), &AtomicBool::new(false)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Fatal);
    assert!(!e.message.contains("sk-"));
    // 余额不足（429 insufficient_quota）不重试，整个停下
    let s = FakeServer::start(|_| Resp::json(429, r#"{"error":{"message":"quota","code":"insufficient_quota"}}"#));
    let e = Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 30.0)).complete(&req(8000), &AtomicBool::new(false)).unwrap_err();
    assert_eq!((e.kind, s.requests().len()), (ErrorKind::Fatal, 1));
    // 服务出错：重试一次后成功
    let n = std::sync::atomic::AtomicU32::new(0);
    let s = FakeServer::start(move |_| if n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { Resp::json(503, "{}").with("retry-after", "0") } else { Resp::json(200, &responses_reply("{}", 10, 0, 5)) });
    Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 30.0)).complete(&req(8000), &AtomicBool::new(false)).unwrap();
    assert_eq!(s.requests().len(), 2);
    // 超预算：不发
    let s = FakeServer::start(|_| Resp::json(200, "{}"));
    let e = Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "medium", meter(&dir, 0.0001)).complete(&req(8000), &AtomicBool::new(false)).unwrap_err();
    assert_eq!((e.kind, s.requests().len()), (ErrorKind::Budget, 0));
}

#[test]
fn openai_web_search_tool() {
    let dir = temp_dir("concept-web-search");
    let reply = json!({
        "id": "r", "status": "completed", "model": "gpt-6.1-sol",
        "output": [
            { "type": "web_search_call", "id": "ws_1", "status": "completed", "action": { "type": "search", "query": "q", "sources": [ { "type": "url", "url": "https://example.org/a" } ] } },
            { "type": "message", "content": [ { "type": "output_text", "text": "Sources: a, b", "annotations": [ { "type": "url_citation", "url": "https://example.org/b", "title": "B" }, { "type": "url_citation", "url": "https://example.org/a", "title": "A" } ] } ] }
        ],
        "usage": { "input_tokens": 3000, "output_tokens": 200 }
    })
    .to_string();
    let s = FakeServer::start(move |_| Resp::json(200, &reply));
    let search = OpenAiSearch { model: Responses::new(Some(&s.base), KEY.into(), "gpt-6.1-sol", "low", meter(&dir, 30.0)) };
    let r = search.search("division of labour Adam Smith criticism", 3, &AtomicBool::new(false)).unwrap();
    // 搜索看过的网址在前，再是回答里引用的（去重）
    assert_eq!(r.hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), ["https://example.org/a", "https://example.org/b"]);
    assert_eq!(r.usage.searches, 1);
    assert!((r.usage.cost_usd - (0.01 + (3000.0 * 2.0 + 200.0 * 10.0) / 1e6)).abs() < 1e-12);
    let b = s.requests()[0].json();
    assert_eq!(b["tools"][0]["type"], "web_search");
    assert_eq!(b["store"], false);
    let calls = Ledger::new(&dir).all().unwrap();
    assert_eq!((calls[0].kind.as_str(), calls[0].purpose.as_str(), calls[0].searches), ("search", "concept_search", 1));
}

// ---------- Mathpix ----------

fn mathpix_server(fail: bool) -> FakeServer {
    let polls = std::sync::atomic::AtomicU32::new(0);
    FakeServer::start(move |r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", "/v3/pdf") => Resp::json(200, r#"{"pdf_id":"pdf-123"}"#),
        ("GET", "/v3/pdf/pdf-123") => {
            let n = polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if fail {
                Resp::json(200, r#"{"status":"error","error":"bad pdf"}"#)
            } else if n == 0 {
                Resp::json(200, r#"{"status":"split","num_pages":2,"percent_done":10}"#)
            } else {
                Resp::json(200, r#"{"status":"completed","num_pages":2,"num_pages_completed":2,"percent_done":100}"#)
            }
        }
        ("GET", "/v3/pdf/pdf-123.lines.json") => Resp::json(200, fixtures::SAMPLE_LINES),
        ("GET", "/v3/pdf/pdf-123.mmd") => Resp::new(200, "text/plain", fixtures::SAMPLE_MMD),
        ("DELETE", "/v3/pdf/pdf-123") => Resp::json(200, "{}"),
        _ => Resp::json(404, "{}"),
    })
}

#[test]
fn mathpix_upload_poll_download_delete() {
    let dir = temp_dir("concept-mathpix");
    let s = mathpix_server(false);
    let mut m = Mathpix::new(Some(&s.base), "pith_app:0123456789abcdef0123", meter(&dir, 30.0)).unwrap();
    m.poll = Duration::from_millis(10);
    let done = m.convert(b"%PDF-1.4 fake", "short.pdf", 2, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!((done.pdf_id.as_str(), done.pages, done.deleted), ("pdf-123", 2, true));
    assert!((done.cost_usd - 0.01).abs() < 1e-12);
    let reqs = s.requests();
    let up = &reqs[0];
    assert_eq!((up.header("app_id"), up.header("app_key")), (Some("pith_app"), Some("0123456789abcdef0123")));
    assert!(up.header("content-type").unwrap().starts_with("multipart/form-data; boundary="));
    assert!(up.body.contains("name=\"file\"; filename=\"short.pdf\"") && up.body.contains("name=\"options_json\"") && up.body.contains("%PDF-1.4 fake"));
    assert_eq!(reqs.iter().filter(|r| r.method == "GET" && r.path == "/v3/pdf/pdf-123").count(), 2);
    assert_eq!(reqs.last().unwrap().method, "DELETE");
    let calls = Ledger::new(&dir).all().unwrap();
    assert_eq!((calls[0].provider.as_str(), calls[0].kind.as_str(), calls[0].searches), ("mathpix", "convert", 2));
    // 行拼段落：标题、跨页的一段（页码 1–2）、第二段、脚注；页眉页码去掉
    let t = text_mathpix::from_lines(&done.lines, done.mmd.as_deref(), "short", None).unwrap();
    assert_eq!(t.paragraphs.iter().map(|p| p.role).collect::<Vec<_>>(), [Role::Heading, Role::Body, Role::Body, Role::Note]);
    let p1 = &t.paragraphs[1];
    assert_eq!(p1.text, "分工越细，每个人能做的活就越专，但前提是市场足够大，否则多做出来的东西卖不出去。");
    assert_eq!(t.paragraph_pages(p1.seq).as_deref(), Some("1–2"));
    assert_eq!((p1.key.as_str(), t.paragraphs[2].page.as_deref()), ("1", Some("2")));
}

#[test]
fn mathpix_failures_still_delete_and_charge() {
    let dir = temp_dir("concept-mathpix-fail");
    let s = mathpix_server(true);
    let mut m = Mathpix::new(Some(&s.base), "pith_app:0123456789abcdef0123", meter(&dir, 30.0)).unwrap();
    m.poll = Duration::from_millis(10);
    let e = m.convert(b"%PDF", "x.pdf", 3, &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(e.message.contains("bad pdf"));
    assert!(s.requests().iter().any(|r| r.method == "DELETE"));
    let calls = Ledger::new(&dir).all().unwrap();
    assert!(calls[0].estimated && (calls[0].cost_usd - 0.015).abs() < 1e-12);
    // 密钥不对：上传就被拒，不记钱
    let bad = FakeServer::start(|_| Resp::json(401, r#"{"error":"Invalid credentials"}"#));
    let m = Mathpix::new(Some(&bad.base), "pith_app:0123456789abcdef0123", meter(&dir, 30.0)).unwrap();
    assert_eq!(m.convert(b"%PDF", "x.pdf", 3, &AtomicBool::new(false), &|_| {}).unwrap_err().kind, ErrorKind::Fatal);
    assert_eq!(Ledger::new(&dir).all().unwrap().len(), 1);
    // 超预算：不上传
    let m = Mathpix::new(Some(&bad.base), "pith_app:0123456789abcdef0123", meter(&dir, 0.001)).unwrap();
    assert_eq!(m.convert(b"%PDF", "x.pdf", 3, &AtomicBool::new(false), &|_| {}).unwrap_err().kind, ErrorKind::Budget);
    assert_eq!(bad.requests().len(), 1);
    assert!(Mathpix::new(None, "no-pair", meter(&dir, 1.0)).is_err());
}

#[test]
fn mock_lines_from_local_pages() {
    let v = fixtures::lines_from_pages(&["Title line\n\nFirst para\nwraps here".into(), "Second page para.".into()]);
    let t = text_mathpix::from_lines(&v, None, "x", Some("en")).unwrap();
    // 第一页第二段没有句末标点：接到第二页
    assert_eq!(t.paragraphs.len(), 2);
    assert_eq!(t.paragraphs[1].text, "First para wraps here Second page para.");
    assert_eq!(t.paragraphs[1].breaks.len(), 1);
}

// ---------- 端到端：真实的接口实现 ----------

/// 假 OpenAI：按系统提示词里的技能和步骤，交出录制的回复；网页搜索给假网站上的网址
fn fake_openai(web: String) -> FakeServer {
    FakeServer::start(move |r| {
        let b = r.json();
        if b["tools"][0]["type"] == "web_search" {
            let reply = json!({ "id": "s", "status": "completed", "model": "gpt-6.1-sol", "output": [
                { "type": "web_search_call", "id": "ws", "status": "completed", "action": { "type": "search", "query": "q" } },
                { "type": "message", "content": [ { "type": "output_text", "text": "ok", "annotations": [ { "type": "url_citation", "url": format!("{web}/pin-figures"), "title": "Example critique" } ] } ] } ],
                "usage": { "input_tokens": 2000, "output_tokens": 100 } });
            return Resp::json(200, &reply.to_string());
        }
        let system = b["instructions"].as_str().unwrap_or("");
        let tag = [("# 技能：整章分析", "chapter"), ("# 技能：反方与后世评论", "critics")].iter().find(|(k, _)| system.contains(k)).map(|(_, v)| *v).unwrap_or("x");
        let step = [("# 这一步：概念", "concepts"), ("# 这一步：论证", "arguments"), ("# 这一步：关系", "relations"), ("# 这一步：摘观点", "comment")].iter().find(|(k, _)| system.contains(k)).map(|(_, v)| *v).unwrap_or("x");
        let text = concept_engine::mock::recorded(&format!("{tag}/{step}")).unwrap_or("{\"cards\":[],\"relations\":[]}");
        // 反方：录制回复里的来源 S2 在这次是网页
        Resp::json(200, &responses_reply(&serde_json::from_str::<Value>(text).unwrap().to_string(), 12_000, 2_000, 3_000))
    })
}

#[test]
fn end_to_end_with_real_ports() {
    let (dir, state, en, zh, en_item) = library_with_books("concept-e2e");
    // 假网站：评论网页
    let web = FakeServer::start(|r| if r.path == "/pin-figures" { Resp::new(200, "text/html; charset=utf-8", format!("<html><head><title>{}</title></head><body><p>{}</p></body></html>", concept_engine::mock::WEB_TITLE, concept_engine::mock::WEB_PAGE)) } else { Resp::new(404, "text/plain", "gone") });
    let openai = fake_openai(web.base.clone());
    let root = state.paths.root.clone();
    let m = meter(&root, 30.0);
    let model = Responses::new(Some(&openai.base), KEY.into(), "gpt-6.1-sol", "medium", m.clone());
    let search = OpenAiSearch { model: Responses::new(Some(&openai.base), KEY.into(), "gpt-6.1-sol", "low", m.clone()) };
    let fetch = Copying { inner: Downloader::new(Box::new(Http::new(Duration::from_secs(5))), &root, Limits { timeout: Duration::from_secs(5), host_gap: Duration::ZERO, allow_private: true, ..Default::default() }), from: Snapshots::new(&root), to: shelf::snapshots(&root) };
    // 文献库里有一条书评（标题、摘要里有检索词）
    state
        .with_db(|c| {
            let id = edit::item_create(c, "journalArticle", None)?;
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("title".to_string(), concept_engine::mock::LIBRARY_TITLE.to_string());
            fields.insert("abstractNote".to_string(), concept_engine::mock::LIBRARY_REVIEW.to_string());
            edit::item_update(c, id, &ItemPatch { fields, ..Default::default() })?;
            crate::search::update(c, &[crate::search::DocRef::item(id)])
        })
        .unwrap();
    let (et, zt, al) = state.with_db(|c| Ok((store::text_load(c, &en)?, store::text_load(c, &zh)?, store::alignment_load(c, &en, &zh)?.unwrap()))).unwrap();
    let books = Books { original: &et, translation: Some(&zt), alignment: Some(&al) };
    let db_shelf = DbShelf { state: &state, snapshots: shelf::snapshots(&root), exclude: vec![en_item] };
    let skills = skill::builtin();
    let run_skill = |name: &str, scope: Scope, targets, uid: &str| -> Run {
        let sk = skills.iter().find(|s| s.name == name).unwrap();
        let mut run = flow::new_run(uid, sk, scope, targets, model.info(), "medium", false, "2026-11-13T10:00:00".into()).unwrap();
        struct StateRec<'a>(&'a AppState, String);
        impl Recorder for StateRec<'_> {
            fn save(&mut self, run: &Run) -> Result<(), String> {
                self.0.with_db(|c| store::run_save(c, &self.1, run))
            }
        }
        let mut rec = StateRec(&state, en.clone());
        let now = || "2026-11-13T10:00:05".to_string();
        let progress = |_| {};
        let cancel = AtomicBool::new(false);
        let mut ports = Ports { model: &model, search: &search, fetch: &fetch, shelf: &db_shelf, recorder: &mut rec, now: &now, progress: &progress, cancel: &cancel };
        flow::execute(&mut run, sk, &books, &mut ports).unwrap();
        run
    };
    // 整章分析
    let chapter = run_skill("chapter", chapter_scope(&et, "I.i", &zh, &en), Vec::new(), "E2E-1");
    assert_eq!(chapter.status, RunStatus::Done, "{:?}", chapter.error);
    assert_eq!((chapter.cards.len(), chapter.relations.len(), chapter.rejected.len()), (6, 4, 0));
    // 反方：先查文献库，再上网（一个网页下载到、存了快照）
    let target = chapter.cards[0].clone();
    let mut sc = Scope { entry: Entry::Card, cards: vec![(chapter.uid.clone(), target.id.clone())], paragraphs: Vec::new(), unit: None, ..chapter_scope(&et, "I.i", &zh, &en) };
    sc.with = vec!["web".into()];
    let critics = run_skill("critics", sc, vec![target], "E2E-2");
    assert_eq!(critics.status, RunStatus::Done, "{:?}", critics.error);
    assert_eq!(critics.sources.iter().map(|s| s.origin).collect::<Vec<_>>(), [SourceOrigin::Library, SourceOrigin::Web]);
    assert!(critics.rejected.is_empty(), "{:?}", critics.rejected);
    assert_eq!(critics.cards.iter().filter(|c| c.kind == CardKind::Comment).count(), 2);
    assert!(critics.cards.iter().flat_map(|c| &c.sentences).filter(|s| s.category == Category::Others).all(|s| s.check.status == Status::Verified));
    // 存库、审计（从快照读来源正文）
    let loaded = state.with_db(|c| store::run_load(c, "E2E-2")).unwrap();
    assert_eq!(loaded, critics);
    let snaps = shelf::snapshots(&root);
    let texts: Vec<(String, String)> = critics.sources.iter().map(|s| (s.id.clone(), snaps.text(s.sha256.as_deref().unwrap()).unwrap())).collect();
    let lookup = |id: &str| texts.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone());
    assert!(audit::audit(&critics, &books, &lookup).is_empty());
    // 费用都在账本里：两次研究的步骤费用合计 = 账本合计
    let total: f64 = chapter.cost_usd() + critics.cost_usd();
    let ledger: f64 = Ledger::new(&root).all().unwrap().iter().map(|c| c.cost_usd).sum();
    assert!((total - ledger).abs() < 1e-9 && total > 0.0, "{total} vs {ledger}");
    // 三种导出
    let findings = Vec::new();
    for f in [Format::Markdown, Format::Html, Format::Docx] {
        let bytes = commands::export_bytes(&chapter, &books, &findings, f).unwrap();
        assert!(bytes.len() > 500);
    }
    let html = String::from_utf8(commands::export_bytes(&chapter, &books, &findings, Format::Html).unwrap()).unwrap();
    assert!(html.contains("<svg") && html.contains("data-card=\"K1\""));
    // 网上的来源加入文献库：网页条目 + 原件附件；再加一次连上已有的
    let added = crate::mutate(&state, |c| shelf::add_sources(c, &state.paths.attachments, &snaps, &critics, &["S2".into()])).unwrap();
    assert!(added[0].created && added[0].attachment_error.is_none(), "{added:?}");
    let again = crate::mutate(&state, |c| shelf::add_sources(c, &state.paths.attachments, &snaps, &store::run_load(c, "E2E-2")?, &["S2".into()])).unwrap();
    assert!(!again[0].created && again[0].item_id == added[0].item_id);
    let _ = dir;
}

#[test]
fn settings_defaults_and_checks() {
    let s = commands::Settings::default();
    assert_eq!((s.model.as_str(), s.effort.as_str(), s.pdf_page_limit, s.dedicated_search), ("gpt-6.1-sol", "medium", 60, true));
    assert_eq!((s.provider(), s.search_engine()), ("openai", commands::OPENAI_SEARCH));
    assert_eq!(s.keys_needed(true), [crate::agent::secret::ApiKey::Openai]);
    let opus = commands::Settings { model: commands::OPUS.into(), dedicated_search: false, ..Default::default() };
    assert_eq!((opus.provider(), opus.search_engine()), ("anthropic", commands::TAVILY));
    assert_eq!(opus.keys_needed(true), [crate::agent::secret::ApiKey::Anthropic, crate::agent::secret::ApiKey::Tavily]);
    assert!(commands::Settings { model: "gpt-4".into(), ..Default::default() }.check().is_err());
    assert!(commands::Settings { effort: "extreme".into(), ..Default::default() }.check().is_err());
    assert!(commands::Settings { pdf_page_limit: 0, ..Default::default() }.check().is_err());
    assert!(commands::Settings { openai_base: Some("http://x".into()), ..Default::default() }.check().is_err());
    assert!(s.check().is_ok());
}
