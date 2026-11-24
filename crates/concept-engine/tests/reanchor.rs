//! 重新解析后给旧卡片重新找锚点（`reanchor`）：同样的文本原位跟过去；段落号后移、一段拆成两段、段落被删、角色变了、
//! 译本重新解析；多处命中取最近的；上次没找到的下次再试；做完审计的结论和重新核对的一样，改坏了查得出。
//! 夹具的文字不进断言和输出（只比段落号、位置、状态）。

use concept_engine::align::{self, Alignment};
use concept_engine::audit;
use concept_engine::fixture;
use concept_engine::fold::{Folded, slice_chars};
use concept_engine::model::{Card, CardKind, Category, Check, Relation, RelationKind, Run, Scope, Sentence, Status, ZhStatus};
use concept_engine::prompts::Books;
use concept_engine::reanchor::{LOST_PREFIX, Outcome, reanchor, relocate};
use concept_engine::skill::Entry;
use concept_engine::text::{Paragraph, Role, Text};
use concept_engine::verify;

fn no_sources(_: &str) -> Option<String> {
    None
}

struct Shelf {
    en: Text,
    zh: Text,
    al: Alignment,
}

impl Shelf {
    fn new(en: Text, zh: Text) -> Shelf {
        let al = align::align(&en, &zh);
        Shelf { en, zh, al }
    }

    fn fixture() -> Shelf {
        Shelf::new(fixture::english(), fixture::chinese())
    }

    fn books(&self) -> Books<'_> {
        Books { original: &self.en, translation: Some(&self.zh), alignment: Some(&self.al) }
    }
}

/// 段落里第 `a..b` 个字
fn part(t: &Text, key: &str, a: usize, b: usize) -> String {
    slice_chars(&t.paragraph(key).unwrap().text, a, b).trim().to_string()
}

fn sentence(category: Category, paragraph: Option<&str>, quote: Option<String>) -> Sentence {
    let text = if category == Category::Original { quote.clone().unwrap() } else { "一句解释。".to_string() };
    Sentence { category, text, paragraph: paragraph.map(String::from), quote, zh: None, source: None, check: Check::default() }
}

fn checked(shelf: &Shelf, mut s: Sentence) -> Sentence {
    verify::check(&mut s, &shelf.books(), &no_sources).unwrap();
    s
}

/// 核对过的一次研究：一张卡片，范围是划选的 I.i.3 和 I.i.6
fn run_of(shelf: &Shelf, sentences: Vec<Sentence>) -> Run {
    let sentences = sentences.into_iter().map(|s| checked(shelf, s)).collect();
    let scope = Scope { entry: Entry::Selection, text: "EN".into(), translation: Some("ZH".into()), paragraphs: vec!["I.i.3".into(), "I.i.6".into()], unit: None, question: None, terms: Vec::new(), cards: Vec::new(), instruction: None, skip: Vec::new(), with: Vec::new() };
    Run { uid: "RUN".into(), scope: Some(scope), cards: vec![Card { id: "K1".into(), kind: CardKind::Concept, title: "卡片".into(), sentences, step: "concepts".into(), about: Vec::new() }], ..Default::default() }
}

/// 每类一句：原文（引文在 I.i.3 中间，带中译）、解释（只挂段落）、解释（带引文）、他人（译本的旁注）、
/// 推演（带前提）、推演（只挂段落）、解释（本来就没挂）
fn mixed(shelf: &Shelf) -> Run {
    let mut original = sentence(Category::Original, Some("I.i.3"), Some(part(&shelf.en, "I.i.3", 1700, 1790)));
    original.zh = Some(part(&shelf.zh, "I.i.3", 0, 10));
    let sidenote = shelf.zh.paragraph("I.i.1 旁注").unwrap().text.clone();
    run_of(
        shelf,
        vec![
            original,
            sentence(Category::Explanation, Some("I.i.6"), None),
            sentence(Category::Explanation, Some("I.i.3"), Some(part(&shelf.en, "I.i.3", 2000, 2040))),
            sentence(Category::Others, Some("译:I.i.1 旁注"), Some(sidenote)),
            sentence(Category::Speculation, Some("I.iii.1"), Some(part(&shelf.en, "I.iii.1", 100, 180))),
            sentence(Category::Speculation, Some("I.i.6"), None),
            sentence(Category::Explanation, None, None),
        ],
    )
}

fn keys(run: &Run) -> Vec<Option<&str>> {
    run.cards[0].sentences.iter().map(|s| s.paragraph.as_deref()).collect()
}

const INSERTED: &str = "A paragraph the new parser found in front of the chapter.";

#[test]
fn reflow_reproduces_the_parsed_fixture() {
    // 测试用的重新排号和解析器排出来的一样（不然下面测的不是真的段落号）
    for parsed in [fixture::english(), fixture::chinese()] {
        let mut again = parsed.clone();
        fixture::reflow(&mut again);
        assert!(again == parsed);
    }
}

#[test]
fn same_text_keeps_every_anchor() {
    let shelf = Shelf::fixture();
    let mut run = mixed(&shelf);
    let before = run.clone();
    assert_eq!(run.cards[0].sentences[0].check.zh_status, Some(ZhStatus::Verified));
    // 重新解析出来的是另一份一模一样的文本
    let again = Shelf::fixture();
    let report = reanchor(&mut run, shelf.books(), again.books(), &no_sources);
    assert!(run == before);
    assert_eq!((report.kept, report.moved, report.lost, report.zh_lost, report.scope_dropped), (6, 0, 0, 0, 0));
    assert!(report.changes.is_empty());
    assert!(audit::audit(&run, &again.books(), &no_sources).is_empty());
}

#[test]
fn anchors_follow_their_paragraph_when_numbers_shift() {
    let shelf = Shelf::fixture();
    // 一句带很短的引文、挂在 I.i.2：这几个字在 I.i.1 里也有
    let short = "division of labour";
    assert!(["I.i.1", "I.i.2"].iter().all(|k| Folded::new(&shelf.en.paragraph(k).unwrap().text).find(short).is_some()));
    let mut run = mixed(&shelf);
    let extra = checked(&shelf, sentence(Category::Explanation, Some("I.i.2"), Some(short.to_string())));
    run.cards[0].sentences.push(extra);
    let before = run.clone();
    // 新文本：第一章最前面多了一段，页码整体往后一页
    let mut en = shelf.en.clone();
    fixture::insert_before(&mut en, "I.i.1", INSERTED);
    let next = |s: &str| (s.parse::<u32>().unwrap() + 1).to_string();
    for p in &mut en.paragraphs {
        p.page = p.page.as_deref().map(next);
        p.breaks.iter_mut().for_each(|b| b.page = next(&b.page));
    }
    let new = Shelf::new(en, shelf.zh.clone());
    let report = reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    // 第一章的段落号都后移一位（第三章、译本的不变）；短引文跟着原来那一段走，没有留在新的 I.i.2
    assert_eq!(keys(&run), [Some("I.i.4"), Some("I.i.7"), Some("I.i.4"), Some("译:I.i.1 旁注"), Some("I.iii.1"), Some("I.i.7"), None, Some("I.i.3")]);
    let (was, now) = (&before.cards[0].sentences[0], &run.cards[0].sentences[0]);
    assert_eq!(now.check.span, was.check.span);
    let page = |s: &Sentence| s.check.pages.as_deref().map(|p| p.parse::<u32>().unwrap());
    assert_eq!(page(now), page(was).map(|p| p + 1));
    // 第一章多了一段，中英段数对不上了：v0.5 起这样的章照样对齐（`align` 的单调对齐），不再整章标「没有对齐」。
    // 夹具的中文是合成的、不是真译文，多出一段后对到的中文段不一定还含那句中译：如实标成「只显示整段」
    assert_eq!(now.check.zh_status, Some(ZhStatus::ParagraphOnly));
    // 只有挂在译本旁注上的那一句没变（译本没重新解析）；第三章那句段落号没变，页码变了
    assert_eq!((report.kept, report.moved, report.lost, report.zh_lost), (1, 6, 0, 1));
    assert_eq!(report.changes.iter().find(|c| c.sentence == Some(1)).map(|c| (c.outcome, c.from.as_deref(), c.to.as_deref(), c.zh_lost)), Some((Outcome::Moved, Some("I.i.3"), Some("I.i.4"), true)));
    // 范围里的段落号也跟着换
    assert_eq!(run.scope.as_ref().unwrap().paragraphs, ["I.i.4", "I.i.7"]);
    assert!(audit::audit(&run, &new.books(), &no_sources).is_empty());
    // 改坏：把重找的位置挪一个字、把段落号改回旧的，审计都查得出
    let mut bad = run.clone();
    bad.cards[0].sentences[0].check.span.as_mut().unwrap().start += 1;
    assert_eq!(audit::audit(&bad, &new.books(), &no_sources).len(), 1);
    let mut bad = run.clone();
    bad.cards[0].sentences[1].paragraph = Some("I.i.6".into());
    assert_eq!(audit::audit(&bad, &new.books(), &no_sources).len(), 1);
}

#[test]
fn a_split_paragraph_is_searched_by_quote() {
    let shelf = Shelf::fixture();
    let mut run = mixed(&shelf);
    // 再加一句引文在 I.i.3 前半段的、一句只挂 I.i.3 的
    for s in [sentence(Category::Original, Some("I.i.3"), Some(part(&shelf.en, "I.i.3", 200, 260))), sentence(Category::Explanation, Some("I.i.3"), None)] {
        run.cards[0].sentences.push(checked(&shelf, s));
    }
    let before = run.clone();
    // I.i.3 在第 1000 个字后面的空格处拆成两段
    let at = shelf.en.paragraph("I.i.3").unwrap().text.chars().enumerate().position(|(i, c)| i >= 1000 && c == ' ').unwrap();
    let mut en = shelf.en.clone();
    fixture::split(&mut en, "I.i.3", at);
    assert_eq!(en.paragraphs.len(), shelf.en.paragraphs.len() + 1);
    let new = Shelf::new(en, shelf.zh.clone());
    let report = reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    // 引文在后半段的：挂到 I.i.4，位置往前挪了；在前半段的留在 I.i.3；只挂段落、那一段文字变了的：没挂上
    assert_eq!(keys(&run), [Some("I.i.4"), Some("I.i.7"), Some("I.i.4"), Some("译:I.i.1 旁注"), Some("I.iii.1"), Some("I.i.7"), None, Some("I.i.3"), None]);
    let (was, now) = (&before.cards[0].sentences[0], &run.cards[0].sentences[0]);
    assert_eq!(now.check.span.unwrap().start, was.check.span.unwrap().start - at - 1);
    assert_eq!(now.check.pages, was.check.pages);
    let (was, now) = (&before.cards[0].sentences[7], &run.cards[0].sentences[7]);
    assert_eq!((now.check.span, &now.check.pages), (was.check.span, &was.check.pages));
    let lost = &run.cards[0].sentences[8];
    assert_eq!(lost.check.status, Status::Unanchored);
    let note = lost.check.note.as_deref().unwrap();
    assert!(note.starts_with(LOST_PREFIX) && note.contains("「I.i.3」"), "{note}");
    assert_eq!((report.moved, report.lost), (4, 1));
    // 范围：I.i.3 的文字不在了，去掉；I.i.6 换成 I.i.7
    assert_eq!((run.scope.as_ref().unwrap().paragraphs.clone(), report.scope_dropped), (vec!["I.i.7".to_string()], 1));
    // 解释句没挂上原文不算审计问题
    assert!(audit::audit(&run, &new.books(), &no_sources).is_empty());
}

#[test]
fn a_removed_paragraph_leaves_the_sentence_unanchored_until_it_comes_back() {
    let shelf = Shelf::fixture();
    let mut run = mixed(&shelf);
    let mut en = shelf.en.clone();
    fixture::remove(&mut en, "I.i.3");
    let new = Shelf::new(en, shelf.zh.clone());
    let report = reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    assert_eq!(keys(&run), [None, Some("I.i.5"), None, Some("译:I.i.1 旁注"), Some("I.iii.1"), Some("I.i.5"), None]);
    let s = &run.cards[0].sentences[0];
    assert_eq!((s.check.status, s.check.span, s.check.pages.as_deref(), s.check.zh_status), (Status::Unanchored, None, None, None));
    assert!(s.quote.is_some(), "引文留着");
    let note = s.check.note.as_deref().unwrap();
    assert!(note.starts_with(LOST_PREFIX) && note.contains("「I.i.3」") && note.contains("没挂上原文"), "{note}");
    assert_eq!((report.kept, report.moved, report.lost), (2, 2, 2));
    assert_eq!(report.changes.iter().filter(|c| c.outcome == Outcome::Lost).map(|c| (c.sentence, c.from.as_deref(), c.to.as_deref())).collect::<Vec<_>>(), [(Some(1), Some("I.i.3"), None), (Some(3), Some("I.i.3"), None)]);
    // 审计如实：原文句没挂上原文列出来，解释句不算
    let findings = audit::audit(&run, &new.books(), &no_sources);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].sentence == Some(1) && findings[0].message.contains("原文句没挂上原文") && findings[0].message.contains(LOST_PREFIX));
    // 还是这份少一段的文本再解析一次：没找到的原样留着（说明里还是最早的段落号）
    let stuck = run.clone();
    let report = reanchor(&mut run, new.books(), new.books(), &no_sources);
    assert!(run == stuck);
    assert_eq!((report.kept, report.moved, report.lost), (4, 0, 2));
    // 解析器修好了再重新解析：引文全书只有一处，重新挂上，和一开始一样
    let fixed = Shelf::fixture();
    let report = reanchor(&mut run, new.books(), fixed.books(), &no_sources);
    assert_eq!((report.lost, report.moved), (0, 4));
    let mut want = mixed(&fixed);
    want.scope.as_mut().unwrap().paragraphs = vec!["I.i.6".into()];
    assert!(run == want);
    assert!(audit::audit(&run, &fixed.books(), &no_sources).is_empty());
}

#[test]
fn several_matches_take_the_nearest() {
    let shelf = Shelf::fixture();
    let quote = part(&shelf.en, "I.ii.3", 300, 380);
    let body = |r: Role| r == Role::Body;
    let mut run = run_of(&shelf, vec![sentence(Category::Original, Some("I.ii.3"), Some(quote.clone()))]);
    // 新文本：I.ii.3 没了；同样的话出现在第一章开头和第三章开头的两个新段落里（离原位置近的是第三章那个）
    let mut en = shelf.en.clone();
    fixture::remove(&mut en, "I.ii.3");
    fixture::insert_before(&mut en, "I.i.1", &format!("Far away. {quote}"));
    fixture::insert_before(&mut en, "I.iii.1", &format!("Close by. {quote}"));
    let new = Shelf::new(en, shelf.zh.clone());
    assert_eq!(relocate(&shelf.en, &new.en, "I.ii.3", Some(&quote), &body).map(|p| p.key.as_str()), Some("I.iii.1"));
    reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    assert_eq!(keys(&run), [Some("I.iii.1")]);
    assert!(audit::audit(&run, &new.books(), &no_sources).is_empty());
    // 同一单元里有的，先取同一单元的（哪怕全书里别处更近）
    let mut en = new.en.clone();
    fixture::insert_before(&mut en, "I.ii.1", &format!("Same chapter. {quote}"));
    assert_eq!(relocate(&shelf.en, &en, "I.ii.3", Some(&quote), &body).map(|p| p.key.as_str()), Some("I.ii.1"));
    // 段落号相同的新段落里有，就用它（哪怕原来那一段的文字变了）
    let mut en = shelf.en.clone();
    let seq = en.paragraph("I.ii.3").unwrap().seq;
    en.paragraphs[seq].text.push_str(" (A new editorial remark.)");
    fixture::insert_before(&mut en, "I.ii.2", &format!("Same chapter. {quote}"));
    fixture::remove(&mut en, "I.ii.1");
    assert_eq!(relocate(&shelf.en, &en, "I.ii.3", Some(&quote), &body).map(|p| p.seq), Some(seq));
    // 角色不对的不算：正文被归成前后附，原文句挂不上，解释句可以
    let mut en = shelf.en.clone();
    en.paragraphs[seq].role = Role::Matter;
    fixture::reflow(&mut en);
    assert!(relocate(&shelf.en, &en, "I.ii.3", Some(&quote), &body).is_none());
    assert!(relocate(&shelf.en, &en, "I.ii.3", Some(&quote), &|_| true).is_some_and(|p| p.role == Role::Matter));
    // 没有引文的：原来那一段原样在才跟过去
    assert_eq!(relocate(&shelf.en, &new.en, "I.ii.2", None, &|_| true).map(|p| p.key.as_str()), Some("I.ii.2"));
    assert!(relocate(&shelf.en, &new.en, "I.ii.3", None, &|_| true).is_none());
    // 旧段落号在旧文本里就不存在（不知道原位置）：只认全书唯一的一处
    assert!(relocate(&shelf.en, &new.en, "IX.i.1", Some(&quote), &body).is_none());
    assert_eq!(relocate(&shelf.en, &shelf.en, "IX.i.1", Some(&quote), &body).map(|p| p.key.as_str()), Some("I.ii.3"));
}

#[test]
fn a_reparsed_translation_rechecks_the_chinese() {
    let shelf = Shelf::fixture();
    let mut run = mixed(&shelf);
    // 译本重新解析：I.i.3 的译文变了（中译句核不上了）；I.i.1 的旁注这次挂到了 I.i.2 后面
    let mut zh = shelf.zh.clone();
    let seq = zh.paragraph("I.i.3").unwrap().seq;
    zh.paragraphs[seq].text = format!("（新版）{}", slice_chars(&zh.paragraphs[seq].text, 10, 40));
    let seq = zh.paragraph("I.i.1 旁注").unwrap().seq;
    let note = zh.paragraphs.remove(seq);
    let after = zh.paragraph("I.i.2").unwrap().seq - 1;
    zh.paragraphs.insert(after + 1, Paragraph { attached: Some(after + 1), ..note });
    fixture::reflow(&mut zh);
    let new = Shelf::new(shelf.en.clone(), zh);
    let report = reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    let s = &run.cards[0].sentences[0];
    assert_eq!((s.paragraph.as_deref(), s.check.zh_paragraph.as_deref(), s.check.zh_status, s.check.zh_span), (Some("I.i.3"), Some("I.i.3"), Some(ZhStatus::ParagraphOnly), None));
    assert_eq!(run.cards[0].sentences[3].paragraph.as_deref(), Some("译:I.i.2 旁注"));
    assert_eq!((report.kept, report.moved, report.lost, report.zh_lost), (5, 1, 0, 1));
    assert!(report.changes.iter().any(|c| c.sentence == Some(1) && c.outcome == Outcome::Kept && c.zh_lost));
    // 原著没变：范围不动
    assert_eq!(run.scope.as_ref().unwrap().paragraphs, ["I.i.3", "I.i.6"]);
    assert!(audit::audit(&run, &new.books(), &no_sources).is_empty());
}

#[test]
fn chapter_scope_relations_and_targets_follow() {
    let shelf = Shelf::fixture();
    let mut run = mixed(&shelf);
    let unit = shelf.en.paragraph("I.i.1").unwrap().unit;
    let scope = run.scope.as_mut().unwrap();
    scope.entry = Entry::Chapter;
    scope.unit = Some("I.i".into());
    scope.paragraphs = shelf.en.body_of(unit).map(|p| p.key.clone()).collect();
    let note = run.cards[0].sentences[1].clone();
    run.relations.push(Relation { id: "R1".into(), from: "K1".into(), to: "K1".into(), kind: RelationKind::Cause, note, step: "relations".into() });
    // 入口选的卡片：一句跟得过去；一句挂在这本书里没有的段落（来自别的研究），原样留着
    let mut foreign = sentence(Category::Explanation, Some("IV.ii.9"), None);
    foreign.check.status = Status::Verified;
    run.targets.push(Card { id: "K9".into(), kind: CardKind::Concept, title: "别处的卡片".into(), sentences: vec![run.cards[0].sentences[0].clone(), foreign.clone()], step: "x".into(), about: Vec::new() });
    let mut en = shelf.en.clone();
    fixture::insert_before(&mut en, "I.i.1", INSERTED);
    let new = Shelf::new(en, shelf.zh.clone());
    let report = reanchor(&mut run, shelf.books(), new.books(), &no_sources);
    // 整章入口：范围是新文本里这一章的全部正文段
    assert_eq!((run.scope.as_ref().unwrap().unit.as_deref(), run.scope.as_ref().unwrap().paragraphs.len()), (Some("I.i"), 12));
    assert_eq!(run.relations[0].note.paragraph.as_deref(), Some("I.i.7"));
    assert!(report.changes.iter().any(|c| c.at == "R1" && c.sentence.is_none() && c.outcome == Outcome::Moved));
    assert_eq!(run.targets[0].sentences[0].paragraph.as_deref(), Some("I.i.4"));
    assert!(run.targets[0].sentences[1] == foreign);
    assert_eq!(report.lost, 0);
}

#[test]
fn sourced_quotes_are_left_alone() {
    let shelf = Shelf::fixture();
    let source = "A synthetic review written for this test. It says the division of labour has two sides.";
    let lookup = |id: &str| (id == "S1").then(|| source.to_string());
    let mut s = sentence(Category::Others, None, Some("the division of labour has two sides".into()));
    s.source = Some("S1".into());
    verify::check(&mut s, &shelf.books(), &lookup).unwrap();
    let mut run = run_of(&shelf, Vec::new());
    run.cards[0].sentences.push(s);
    let before = run.clone();
    let mut en = shelf.en.clone();
    fixture::remove(&mut en, "I.i.3");
    let new = Shelf::new(en, shelf.zh.clone());
    let report = reanchor(&mut run, shelf.books(), new.books(), &lookup);
    assert!(run.cards == before.cards);
    assert_eq!((report.kept, report.moved, report.lost), (0, 0, 0));
    assert!(audit::audit(&run, &new.books(), &lookup).is_empty());
}
