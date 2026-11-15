//! 重新解析（`concept::reparse`）的测试：同样的内容重新解析什么都不变；段落号后移、一段拆成两段、段落被删、译本重新解析；
//! 对齐和段落检索跟着换；中途出错整个回滚；重找的结果被改坏审计查得出。夹具的文字不进断言和输出。

use std::sync::atomic::AtomicBool;

use concept_engine::align::{self, Alignment};
use concept_engine::audit::{self, Finding};
use concept_engine::fixture;
use concept_engine::flow::{self, Ports};
use concept_engine::fold::slice_chars;
use concept_engine::mock::{MockFetch, MockModel, MockSearch, MockShelf, Store};
use concept_engine::model::{Card, CardKind, Category, Check, Run, RunStatus, Scope, Sentence, Status, ZhStatus};
use concept_engine::prompts::Books;
use concept_engine::reanchor::{LOST_PREFIX, Outcome};
use concept_engine::skill::{self, Entry};
use concept_engine::text::{Numbering, Paragraph, Text};
use concept_engine::verify;
use research_engine::model::ModelRef;
use rusqlite::Connection;

use super::library as shelf;
use super::reparse::{self, Reparsed};
use super::tests::{Rec, chapter_scope, library_with_books};
use super::{commands, search, store};
use crate::AppState;

const CHAPTER: &str = "RUN-CHAPTER";
const MID: &str = "RUN-MID";
const OWN: &str = "RUN-ZH";
const INSERTED: &str = "A paragraph the new parser found in front of the chapter.";

fn no_sources(_: &str) -> Option<String> {
    None
}

/// 一份文本和研究它时用的另一本、对齐（同 `commands::load_books`）
struct Loaded {
    original: Text,
    translation: Option<Text>,
    alignment: Option<Alignment>,
}

impl Loaded {
    fn of(c: &Connection, uid: &str) -> Loaded {
        let original = store::text_load(c, uid).unwrap();
        match store::partner(c, uid).unwrap() {
            Some((t, true)) => Loaded { original, translation: Some(store::text_load(c, &t).unwrap()), alignment: store::alignment_load(c, uid, &t).unwrap() },
            _ => Loaded { original, translation: None, alignment: None },
        }
    }

    fn books(&self) -> Books<'_> {
        Books { original: &self.original, translation: self.translation.as_ref(), alignment: self.alignment.as_ref() }
    }
}

fn part(t: &Text, key: &str, a: usize, b: usize) -> String {
    slice_chars(&t.paragraph(key).unwrap().text, a, b).trim().to_string()
}

fn sentence(category: Category, paragraph: &str, quote: Option<String>) -> Sentence {
    let text = if category == Category::Original { quote.clone().unwrap() } else { "一句解释。".to_string() };
    Sentence { category, text, paragraph: Some(paragraph.to_string()), quote, zh: None, source: None, check: Check::default() }
}

/// 手写的一次研究（一张卡片），每句先在库里的书上核对过
fn handmade(c: &Connection, text_uid: &str, uid: &str, scope: &[&str], sentences: Vec<Sentence>) {
    let l = Loaded::of(c, text_uid);
    let sentences = sentences
        .into_iter()
        .map(|mut s| {
            verify::check(&mut s, &l.books(), &no_sources).unwrap();
            s
        })
        .collect();
    let scope = Scope { entry: Entry::Selection, text: text_uid.into(), translation: None, paragraphs: scope.iter().map(|s| s.to_string()).collect(), unit: None, question: None, terms: Vec::new(), cards: Vec::new(), instruction: None, skip: Vec::new(), with: Vec::new() };
    let run = Run { uid: uid.into(), skill: "passage".into(), skill_title: "精读一段".into(), skill_version: 1, scope: Some(scope), status: RunStatus::Done, created_at: "2026-11-14T10:00:00".into(), updated_at: "2026-11-14T10:00:00".into(), cards: vec![Card { id: "K1".into(), kind: CardKind::Concept, title: "手写的卡片".into(), sentences, step: "read".into(), about: Vec::new() }], ..Default::default() };
    store::run_save(c, text_uid, &run).unwrap();
}

/// 两本书（英文连着合成的中译）和三次研究：
/// - `CHAPTER`：模拟模型做的整章分析（第一章；有带中译的原文句、只挂段落的解释、挂译本旁注的他人句、关系的说明）；
/// - `MID`：英文上手写的一次，引文都在 I.i.3 中间（原文带中译、带引文的解释、只挂段落的解释、译本旁注、推演的前提）；
/// - `OWN`：直接在中文那份文本上做的一次（它自己当原著）
fn studied(name: &str) -> (crate::db::tests::TempDir, AppState, String, String) {
    let (dir, state, en, zh, _) = library_with_books(name);
    state
        .with_db(|c| {
            let l = Loaded::of(c, &en);
            let sk = skill::builtin().into_iter().find(|s| s.name == "chapter").unwrap();
            let mut run = flow::new_run(CHAPTER, &sk, chapter_scope(&l.original, "I.i", &zh, &en), Vec::new(), ModelRef { provider: "mock".into(), model: "模拟模型".into() }, "medium", true, "2026-11-14T09:00:00".into())?;
            let mocks = Store::default();
            let (library, fetch, model) = (MockShelf { store: mocks.clone(), has_review: false }, MockFetch { store: mocks }, MockModel::default());
            let mut rec = Rec { c, text: en.clone() };
            let (now, progress, cancel) = (|| "2026-11-14T09:00:05".to_string(), |_| {}, AtomicBool::new(false));
            let mut ports = Ports { model: &model, search: &MockSearch, fetch: &fetch, shelf: &library, recorder: &mut rec, now: &now, progress: &progress, cancel: &cancel };
            flow::execute(&mut run, &sk, &l.books(), &mut ports)?;
            assert_eq!((run.status, run.cards.len(), run.relations.len()), (RunStatus::Done, 6, 4));
            let (et, zt) = (&l.original, l.translation.as_ref().unwrap());
            let mut original = sentence(Category::Original, "I.i.3", Some(part(et, "I.i.3", 1700, 1790)));
            original.zh = Some(part(zt, "I.i.3", 0, 10));
            handmade(
                c,
                &en,
                MID,
                &["I.i.3"],
                vec![
                    original,
                    sentence(Category::Explanation, "I.i.3", Some(part(et, "I.i.3", 2000, 2040))),
                    sentence(Category::Explanation, "I.i.3", None),
                    sentence(Category::Others, "译:I.i.3 旁注", Some(zt.paragraph("I.i.3 旁注").unwrap().text.clone())),
                    sentence(Category::Speculation, "I.iii.1", Some(part(et, "I.iii.1", 100, 180))),
                ],
            );
            handmade(c, &zh, OWN, &["I.i.3"], vec![sentence(Category::Original, "I.i.3", Some(part(zt, "I.i.3", 0, 10))), sentence(Category::Explanation, "I.i.5", None), sentence(Category::Others, "I.i.1 旁注", Some(zt.paragraph("I.i.1 旁注").unwrap().text.clone()))]);
            Ok(())
        })
        .unwrap();
    (dir, state, en, zh)
}

fn reparse_with(state: &AppState, uid: &str, text: &Text) -> Result<Reparsed, String> {
    state.with_db(|c| {
        let row = store::text_row_get(c, uid)?;
        reparse::apply(c, &state.paths.backups, &shelf::snapshots(&state.paths.root), uid, text, &row.meta, &row.source_sha256)
    })
}

fn run(state: &AppState, uid: &str) -> Run {
    state.with_db(|c| store::run_load(c, uid)).unwrap()
}

/// 和 App 打开研究时一样：从库里的书重新审计
fn audit_of(state: &AppState, uid: &str) -> Vec<Finding> {
    state
        .with_db(|c| {
            let l = Loaded::of(c, &store::run_text(c, uid)?);
            Ok(audit::audit(&store::run_load(c, uid)?, &l.books(), &no_sources))
        })
        .unwrap()
}

/// 库里和这两本书有关的一切（比「什么都没变」用）
#[derive(PartialEq)]
struct Everything {
    texts: Vec<store::TextRow>,
    bodies: Vec<Text>,
    alignment: Option<Alignment>,
    runs: Vec<Run>,
    hits: Vec<(String, bool)>,
    indexed: i64,
}

fn everything(state: &AppState, en: &str, zh: &str) -> Everything {
    state
        .with_db(|c| {
            assert!(!search::ensure(c)?, "段落检索表和主库对得上");
            Ok(Everything {
                texts: store::texts_list(c)?,
                bodies: vec![store::text_load(c, en)?, store::text_load(c, zh)?],
                alignment: store::alignment_load(c, en, zh)?,
                runs: [CHAPTER, MID, OWN].iter().map(|u| store::run_load(c, u)).collect::<Result<_, _>>()?,
                hits: ["pin-maker", "制针", "parser"].iter().flat_map(|q| commands::search_hits(c, en, q, 20).unwrap()).map(|h| (h.key, h.via_translation)).collect(),
                indexed: c.query_row("SELECT COUNT(*) FROM search.concept_paras", [], |r| r.get(0)).unwrap(),
            })
        })
        .unwrap()
}

fn sentences<'a>(r: &'a Run, card: &str) -> &'a [Sentence] {
    &r.cards.iter().find(|c| c.id == card).unwrap().sentences
}

fn paragraphs(r: &Run) -> Vec<Option<&str>> {
    sentences(r, "K1").iter().map(|s| s.paragraph.as_deref()).collect()
}

/// 英文夹具：第一章最前面多一段，页码整体往后一页
fn shifted(old: &Text) -> Text {
    let mut t = old.clone();
    fixture::insert_before(&mut t, "I.i.1", INSERTED);
    let next = |s: &str| (s.parse::<u32>().unwrap() + 1).to_string();
    for p in &mut t.paragraphs {
        p.page = p.page.as_deref().map(next);
        p.breaks.iter_mut().for_each(|b| b.page = next(&b.page));
    }
    t
}

#[test]
fn reparsing_the_same_content_changes_nothing() {
    let (dir, state, en, zh) = studied("concept-reparse-same");
    let before = everything(&state, &en, &zh);
    let ids = state.with_db(|c| Ok((store::text_id(c, &en)?, store::text_id(c, &zh)?))).unwrap();
    // 对话框里的说明：这份附件解析过；重新解析会动到几次研究、几张卡片（译本还连着原著上的研究）
    let (en_att, zh_att) = (before.texts[0].attachment_id.unwrap(), before.texts[1].attachment_id.unwrap());
    state
        .with_db(|c| {
            assert_eq!(reparse::existing(c, en_att)?, Some(reparse::Existing { uid: en.clone(), runs: 2, cards: 7 }));
            assert_eq!(reparse::existing(c, zh_att)?, Some(reparse::Existing { uid: zh.clone(), runs: 3, cards: 8 }));
            assert_eq!((reparse::existing(c, 999_999)?, reparse::numbering_of(c, en_att)?), (None, Some(Numbering::Glasgow)));
            // 同一个附件不再存第二份
            assert!(reparse::refuse_second(c, en_att).unwrap_err().contains("重新解析"));
            assert!(reparse::refuse_second(c, 999_999).is_ok());
            Ok(())
        })
        .unwrap();
    // 原著：和对话框走同一条路，从附件文件重新解析
    let (text, _) = state.with_db(|c| commands::parse_epub(&std::fs::read(shelf::attachment_file(c, &state.paths.attachments, en_att)?.path).unwrap())).unwrap();
    let done = reparse_with(&state, &en, &text).unwrap();
    assert_eq!((done.uid.as_str(), done.runs.len(), done.cards, done.moved, done.lost, done.zh_lost, done.findings), (en.as_str(), 2, 7, 0, 0, 0, 0));
    assert!(done.kept > 20 && done.runs.iter().all(|r| r.changes.is_empty() && r.findings.is_empty() && r.text_uid == en));
    assert!(dir.join("data/backups").join(&done.backup).is_file() && done.backup.contains("before-import"));
    // 译本：它自己的研究和原著上的研究都过一遍
    let (text, _) = state.with_db(|c| commands::parse_epub(&std::fs::read(shelf::attachment_file(c, &state.paths.attachments, zh_att)?.path).unwrap())).unwrap();
    let done = reparse_with(&state, &zh, &text).unwrap();
    assert_eq!((done.runs.len(), done.cards, done.moved, done.lost, done.zh_lost, done.findings), (3, 8, 0, 0, 0, 0));
    assert_eq!(done.runs.iter().map(|r| (r.uid.as_str(), r.text_uid == zh)).collect::<Vec<_>>(), [(OWN, true), (CHAPTER, false), (MID, false)]);
    // uid、id、关联、文本、对齐、研究、检索都和原来一样；只有改的时间变了
    let after = everything(&state, &en, &zh);
    assert_eq!(state.with_db(|c| Ok((store::text_id(c, &en)?, store::text_id(c, &zh)?))).unwrap(), ids);
    assert_eq!(after.texts.len(), 2);
    for (a, b) in after.texts.iter().zip(&before.texts) {
        assert!(a.updated_at >= b.updated_at);
        assert!(store::TextRow { updated_at: b.updated_at.clone(), ..a.clone() } == *b);
    }
    assert!(after.bodies == before.bodies && after.alignment == before.alignment && after.runs == before.runs);
    assert_eq!((&after.hits, after.indexed), (&before.hits, before.indexed));
    assert_eq!(after.hits, [("I.i.3".to_string(), false), ("I.i.3".to_string(), true)]);
    for uid in [CHAPTER, MID, OWN] {
        assert!(audit_of(&state, uid).is_empty());
    }
    state.with_db(|c| Ok(assert_eq!((store::partner(c, &en)?, store::partner(c, &zh)?), (Some((zh.clone(), true)), Some((en.clone(), false)))))).unwrap();
}

#[test]
fn anchors_follow_when_paragraph_numbers_shift() {
    let (_dir, state, en, zh) = studied("concept-reparse-shift");
    let before = everything(&state, &en, &zh);
    let new = shifted(&before.bodies[0]);
    let done = reparse_with(&state, &en, &new).unwrap();
    // 没有丢的，审计没有问题；第一章中英段数对不上了，中译如实标成没有对齐
    assert_eq!((done.runs.len(), done.lost, done.findings), (2, 0, 0));
    // （中译原来核对通过的：整章分析 8 句 + 手写的 1 句）
    assert!(done.moved > 20 && done.zh_lost == 9, "{} {}", done.moved, done.zh_lost);
    let after = everything(&state, &en, &zh);
    assert!(after.bodies[0] == new && after.bodies[1] == before.bodies[1]);
    assert!(after.alignment.as_ref() == Some(&align::align(&new, &before.bodies[1])));
    let chapter = after.alignment.as_ref().unwrap().units.iter().find(|u| u.key == "I.i").unwrap();
    assert_eq!((chapter.aligned, chapter.count, chapter.other_count), (false, 12, 11));
    // 手写的那次：段落号后移一位，位置不变，页码往后一页；译本上的、第三章的段落号不变
    let (was, now) = (&before.runs[1], &after.runs[1]);
    assert_eq!(paragraphs(now), [Some("I.i.4"), Some("I.i.4"), Some("I.i.4"), Some("译:I.i.3 旁注"), Some("I.iii.1")]);
    let (a, b) = (&sentences(was, "K1")[0], &sentences(now, "K1")[0]);
    let page = |s: &Sentence| s.check.pages.as_deref().map(|p| p.parse::<u32>().unwrap());
    assert_eq!((b.check.span, page(b), b.check.zh_status, b.check.zh_paragraph.as_deref()), (a.check.span, page(a).map(|p| p + 1), Some(ZhStatus::Unaligned), None));
    assert_eq!((now.scope.as_ref().unwrap().paragraphs.as_slice(), done.runs[1].label.as_str()), (["I.i.4".to_string()].as_slice(), "I.i.4"));
    // 整章分析那次：每个挂在原著上的锚点后移一位；范围是新文本里这一章的 12 段
    let (was, now) = (&before.runs[0], &after.runs[0]);
    let shift = |k: &str| k.strip_prefix("I.i.").map(|n| format!("I.i.{}", n.parse::<u32>().unwrap() + 1)).unwrap_or_else(|| k.to_string());
    for (a, b) in was.cards.iter().flat_map(|c| &c.sentences).chain(was.relations.iter().map(|r| &r.note)).zip(now.cards.iter().flat_map(|c| &c.sentences).chain(now.relations.iter().map(|r| &r.note))) {
        assert_eq!(b.paragraph, a.paragraph.as_deref().map(shift));
        assert_eq!(b.check.status, a.check.status);
    }
    assert_eq!(now.scope.as_ref().unwrap().paragraphs.len(), 12);
    assert!(done.runs[0].changes.iter().any(|c| c.at == "R1" && c.outcome == Outcome::Moved));
    // 库里的锚点行就是新的段落号和页码
    let row: (String, i64, String) = state
        .with_db(|c| Ok(c.query_row("SELECT a.paragraph, a.span_start, a.pages FROM concept_anchors a JOIN concept_sentences s ON s.id = a.sentence_id JOIN concept_cards k ON k.id = s.card_id JOIN concept_runs r ON r.id = k.run_id WHERE r.uid = ?1 AND s.position = 0", [MID], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()))
        .unwrap();
    assert_eq!((row.0.as_str(), row.1 as usize, row.2.parse::<u32>().ok()), ("I.i.4", b.check.span.unwrap().start, page(b)));
    // 检索表里是新段落：新加的那一段搜得到，原来的段落换了段落号；中文词映射不回第一章了（没对齐）
    assert_eq!(after.hits, [("I.i.4".to_string(), false), ("I.i.1".to_string(), false)]);
    assert_eq!(after.indexed, before.indexed + 1);
    for uid in [CHAPTER, MID, OWN] {
        assert!(audit_of(&state, uid).is_empty());
    }
    // 改坏：把库里重找的结果挪一个位置，审计查得出
    state.with_db(|c| Ok(c.execute("UPDATE concept_anchors SET span_start = span_start + 1, span_end = span_end + 1 WHERE paragraph = 'I.i.4' AND span_start = ?1", [row.1]).unwrap())).unwrap();
    let findings = audit_of(&state, MID);
    assert_eq!(findings.iter().map(|f| (f.at.as_str(), f.sentence)).collect::<Vec<_>>(), [("K1", Some(1))]);
    // 改坏：段落号改回旧的
    state.with_db(|c| Ok(c.execute("UPDATE concept_anchors SET paragraph = 'I.i.3' WHERE paragraph = 'I.i.4' AND span_start IS NULL", []).unwrap())).unwrap();
    assert_eq!(audit_of(&state, MID).len(), 2);
}

#[test]
fn a_split_paragraph_is_found_by_quote() {
    let (_dir, state, en, zh) = studied("concept-reparse-split");
    let before = everything(&state, &en, &zh);
    // I.i.3 在第 1000 个字后面的空格处拆成两段：引文都落在后半段
    let at = before.bodies[0].paragraph("I.i.3").unwrap().text.chars().enumerate().position(|(i, c)| i >= 1000 && c == ' ').unwrap();
    let mut new = before.bodies[0].clone();
    fixture::split(&mut new, "I.i.3", at);
    let done = reparse_with(&state, &en, &new).unwrap();
    let after = everything(&state, &en, &zh);
    let (was, now) = (&before.runs[1], &after.runs[1]);
    // 带引文的挂到后半段（I.i.4），位置往前挪；只挂段落的那句，原来那一段的文字不在了：没挂上原文
    assert_eq!(paragraphs(now), [Some("I.i.4"), Some("I.i.4"), None, Some("译:I.i.3 旁注"), Some("I.iii.1")]);
    for i in [0, 1] {
        let (a, b) = (&sentences(was, "K1")[i], &sentences(now, "K1")[i]);
        assert_eq!((b.check.status, b.check.span.unwrap().start, &b.check.pages), (Status::Verified, a.check.span.unwrap().start - at - 1, &a.check.pages));
    }
    let lost = &sentences(now, "K1")[2];
    let note = lost.check.note.as_deref().unwrap();
    assert!(lost.check.status == Status::Unanchored && note.starts_with(LOST_PREFIX) && note.contains("「I.i.3」"), "{note}");
    let report = done.runs.iter().find(|r| r.uid == MID).unwrap();
    assert_eq!((report.moved, report.lost, report.scope_dropped, report.findings.len()), (2, 1, 1, 0));
    assert_eq!(report.changes.iter().filter(|c| c.outcome == Outcome::Lost).map(|c| (c.at.as_str(), c.sentence, c.from.as_deref(), c.to.as_deref())).collect::<Vec<_>>(), [("K1", Some(3), Some("I.i.3"), None)]);
    // 解释句没挂上原文不算审计问题
    assert_eq!((done.lost, done.findings), (1, 0));
    for uid in [CHAPTER, MID, OWN] {
        assert!(audit_of(&state, uid).is_empty());
    }
}

#[test]
fn a_removed_paragraph_is_reported_honestly() {
    let (_dir, state, en, zh) = studied("concept-reparse-removed");
    let before = everything(&state, &en, &zh);
    let mut new = before.bodies[0].clone();
    fixture::remove(&mut new, "I.i.3");
    let done = reparse_with(&state, &en, &new).unwrap();
    let after = everything(&state, &en, &zh);
    let now = &after.runs[1];
    assert_eq!(paragraphs(now), [None, None, None, Some("译:I.i.3 旁注"), Some("I.iii.1")]);
    let s = &sentences(now, "K1")[0];
    let note = s.check.note.as_deref().unwrap();
    assert_eq!((s.check.status, s.check.span, s.check.pages.as_deref(), s.check.zh_status), (Status::Unanchored, None, None, None));
    assert!(s.quote.is_some() && note.starts_with(LOST_PREFIX) && note.contains("「I.i.3」") && note.contains("没挂上原文"), "{note}");
    // 库里：状态是「没挂上原文」，段落号空了，说明在
    let row: (Option<String>, String, Option<String>) = state
        .with_db(|c| Ok(c.query_row("SELECT a.paragraph, a.status, a.note FROM concept_anchors a JOIN concept_sentences s ON s.id = a.sentence_id JOIN concept_cards k ON k.id = s.card_id JOIN concept_runs r ON r.id = k.run_id WHERE r.uid = ?1 AND s.position = 0", [MID], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()))
        .unwrap();
    assert_eq!((row.0, row.1.as_str(), row.2.as_deref()), (None, "unanchored", Some(note)));
    // 审计如实：原文句没挂上原文是一个问题（解释句不算）；和 App 打开这次研究时看到的一样
    let report = done.runs.iter().find(|r| r.uid == MID).unwrap();
    assert_eq!((report.lost, report.findings.len(), done.lost, done.findings), (3, 1, 3, 1));
    assert!(report.findings[0].at == "K1" && report.findings[0].sentence == Some(1) && report.findings[0].message.contains("原文句没挂上原文"));
    assert!(audit_of(&state, MID) == report.findings);
    assert!(audit_of(&state, CHAPTER).is_empty());
    // Word 搜索窗里「卡片里的原文」不再给没挂上的那一句
    let item: String = state.with_db(|c| Ok(c.query_row("SELECT i.uid FROM items i JOIN concept_texts t ON t.item_id = i.id WHERE t.uid = ?1", [&en], |r| r.get(0)).unwrap())).unwrap();
    let quotes = state.with_db(|c| commands::quotes_for_item(c, &item)).unwrap();
    assert!(quotes.iter().all(|q| q.run_uid != MID));
    // 解析器修好后再重新解析：引文全书只有一处，重新挂上，审计没有问题。
    // 只挂段落、没有引文的那句没法再找（不知道原来是哪一段的文字），还是没挂上；这一次没有新丢的
    let done = reparse_with(&state, &en, &before.bodies[0]).unwrap();
    assert_eq!((done.lost, done.findings), (0, 0));
    let again = run(&state, MID);
    assert_eq!(paragraphs(&again), [Some("I.i.3"), Some("I.i.3"), None, Some("译:I.i.3 旁注"), Some("I.iii.1")]);
    assert!(sentences(&again, "K1")[..2] == sentences(&before.runs[1], "K1")[..2]);
}

#[test]
fn a_reparsed_translation_rechecks_the_chinese() {
    let (_dir, state, en, zh) = studied("concept-reparse-translation");
    let before = everything(&state, &en, &zh);
    // 译本的新解析：I.i.3 的译文变了（原著研究里那句中译核不上了）；I.i.3 的旁注这次排在 I.i.4 后面
    let mut new = before.bodies[1].clone();
    let seq = new.paragraph("I.i.3").unwrap().seq;
    new.paragraphs[seq].text = format!("（新版）{}", slice_chars(&new.paragraphs[seq].text, 10, 40));
    let note = new.paragraphs.remove(new.paragraph("I.i.3 旁注").unwrap().seq);
    let after = new.paragraph("I.i.4").unwrap().seq;
    new.paragraphs.insert(after + 1, Paragraph { attached: Some(after), ..note });
    fixture::reflow(&mut new);
    assert!(new.paragraph("I.i.4 旁注").is_some() && new.paragraph("I.i.3 旁注").is_none());
    let done = reparse_with(&state, &zh, &new).unwrap();
    assert_eq!(done.runs.iter().map(|r| r.uid.as_str()).collect::<Vec<_>>(), [OWN, CHAPTER, MID]);
    let after = everything(&state, &en, &zh);
    // uid 和关联都在；原著没动；对齐按新的译本重算（段落的顺序号变了）
    assert!(after.texts.iter().map(|t| (&t.uid, &t.original_uid)).eq(before.texts.iter().map(|t| (&t.uid, &t.original_uid))));
    assert!(after.bodies[0] == before.bodies[0] && after.bodies[1] == new);
    assert!(after.alignment.as_ref() == Some(&align::align(&before.bodies[0], &new)) && after.alignment != before.alignment);
    // 原著上的研究：原文句还在原位，中译句在对齐的新段落里核不上了，只显示整段；挂译本旁注的那句跟到新的段落号
    let now = &after.runs[1];
    assert_eq!(paragraphs(now), [Some("I.i.3"), Some("I.i.3"), Some("I.i.3"), Some("译:I.i.4 旁注"), Some("I.iii.1")]);
    let (a, b) = (&sentences(&before.runs[1], "K1")[0], &sentences(now, "K1")[0]);
    assert_eq!((a.check.zh_status, b.check.zh_status, b.check.zh_span, b.check.zh_paragraph.as_deref()), (Some(ZhStatus::Verified), Some(ZhStatus::ParagraphOnly), None, Some("I.i.3")));
    assert_eq!((b.check.span, &b.check.pages, b.check.status), (a.check.span, &a.check.pages, Status::Verified));
    let report = done.runs.iter().find(|r| r.uid == MID).unwrap();
    assert_eq!((report.kept, report.moved, report.lost, report.zh_lost), (4, 1, 0, 1));
    // 整章分析那次：别的中译句照旧核对通过
    assert!(after.runs[0] == before.runs[0]);
    // 译本自己的研究：引文所在的段落文字变了，前 10 个字不在了
    let own = &after.runs[2];
    assert_eq!(paragraphs(own), [None, Some("I.i.5"), Some("I.i.1 旁注")]);
    assert_eq!(done.runs[0].lost, 1);
    // 检索表里是译本的新段落：中文词现在只在那条旁注里，旁注挂到了 I.i.4，经中译映射回原著的 I.i.4
    assert_eq!(after.hits, [("I.i.3".to_string(), false), ("I.i.4".to_string(), true)]);
    assert_eq!(after.indexed, before.indexed);
    assert_eq!(audit_of(&state, OWN).len(), 1);
    assert!(audit_of(&state, CHAPTER).is_empty() && audit_of(&state, MID).is_empty());
}

#[test]
fn a_failed_reparse_rolls_everything_back() {
    let (_dir, state, en, zh) = studied("concept-reparse-rollback");
    let before = everything(&state, &en, &zh);
    // 新文本里两段的引用名重了：换段落换到一半出错
    let mut broken = shifted(&before.bodies[0]);
    broken.paragraphs[20].key = broken.paragraphs[19].key.clone();
    assert!(reparse_with(&state, &en, &broken).is_err());
    assert!(everything(&state, &en, &zh) == before);
    // 文本、对齐、检索都换完了，存研究的时候出错
    state.with_db(|c| Ok(c.execute_batch("CREATE TEMP TRIGGER reparse_fails BEFORE INSERT ON main.concept_anchors BEGIN SELECT RAISE(ABORT, 'boom'); END;").unwrap())).unwrap();
    for (uid, text) in [(&en, shifted(&before.bodies[0])), (&zh, before.bodies[1].clone())] {
        let e = reparse_with(&state, uid, &text).unwrap_err();
        assert!(e.contains("boom"), "{e}");
        assert!(everything(&state, &en, &zh) == before);
    }
    // 没有这份文本：什么都不做
    assert!(reparse_with(&state, &en, &before.bodies[0]).is_err());
    assert!(state.with_db(|c| reparse::apply(c, &state.paths.backups, &shelf::snapshots(&state.paths.root), "NO-SUCH-TEXT", &before.bodies[0], &Default::default(), "x")).is_err());
    // 故障去掉以后照常能做
    state.with_db(|c| Ok(c.execute_batch("DROP TRIGGER reparse_fails").unwrap())).unwrap();
    let done = reparse_with(&state, &en, &shifted(&before.bodies[0])).unwrap();
    assert_eq!((done.lost, done.findings), (0, 0));
    assert!(everything(&state, &en, &zh) != before);
}

/// 真实数据上跑一次（默认忽略，不联网、不花钱）：`PITH_ACCEPT_DIR` 指向验收数据目录的**副本**（会被改），
/// 把里面每份 EPUB 文本从它的附件重新解析一遍。解析器没变时，每一句都应该原位跟过去、审计 0 个问题。只打印聚合数字
#[test]
#[ignore]
fn real_reparse() {
    let root = std::path::PathBuf::from(std::env::var("PITH_ACCEPT_DIR").expect("要设 PITH_ACCEPT_DIR（数据目录的副本）"));
    let state = AppState::init(crate::paths::DataPaths::new(root.clone()));
    let texts = state.with_db(|c| store::texts_list(c)).unwrap();
    let all_runs = |state: &AppState| -> Vec<Run> { state.with_db(|c| texts.iter().flat_map(|t| store::runs_list(c, &t.uid).unwrap()).map(|r| store::run_load(c, &r.uid)).collect()).unwrap() };
    let before = all_runs(&state);
    println!("{} 份文本，{} 次研究，{} 张卡片，{} 句", texts.len(), before.len(), before.iter().map(|r| r.cards.len()).sum::<usize>(), before.iter().flat_map(|r| &r.cards).map(|c| c.sentences.len()).sum::<usize>());
    for t in &texts {
        let Some(att) = t.attachment_id.filter(|_| t.format == "epub") else {
            println!("{}（{}）：不是 EPUB 或没有附件，跳过", t.language, t.format);
            continue;
        };
        let started = std::time::Instant::now();
        let done = state
            .with_db(|c| {
                let f = shelf::attachment_file(c, &state.paths.attachments, att)?;
                let (mut text, meta) = commands::parse_epub(&std::fs::read(&f.path).map_err(|e| e.to_string())?)?;
                if let Some(n) = reparse::numbering_of(c, att)?.filter(|n| text.numbering != Some(*n)) {
                    text.renumber(n);
                }
                let same = store::text_load(c, &t.uid)?.paragraphs == text.paragraphs;
                let existing = reparse::existing(c, att)?.unwrap();
                println!("{}（{}）：{} 段，和库里的{}；会动到 {} 次研究、{} 张卡片", t.language, t.format, text.paragraphs.len(), if same { "一样" } else { "不一样" }, existing.runs, existing.cards);
                reparse::apply(c, &state.paths.backups, &shelf::snapshots(&root), &t.uid, &text, &meta, &f.sha256)
            })
            .unwrap();
        println!("  重新解析：{} 次研究、{} 张卡片；原位 {}，位置变了 {}，没挂上 {}，中译核不上了 {}，审计问题 {}；用时 {:.1} 秒", done.runs.len(), done.cards, done.kept, done.moved, done.lost, done.zh_lost, done.findings, started.elapsed().as_secs_f64());
        if std::env::var("PITH_EXPECT_SAME").as_deref() == Ok("1") {
            assert_eq!((done.moved, done.lost, done.zh_lost, done.findings), (0, 0, 0, 0));
        }
    }
    let after = all_runs(&state);
    println!("研究和重新解析之前{}", if after == before { "逐句一样" } else { "不一样" });
    state.with_db(|c| Ok(println!("段落检索表{}；文本 {} 份", if search::ensure(c)? { "重建了（不该）" } else { "和主库对得上" }, store::texts_list(c)?.len()))).unwrap();
}
