//! 整条工作流：录制的回复跑通四种入口（整章、划选、问题、追踪）、反方、推演、自由，一直到核对、审计、导出；
//! 改坏测试；取消、出错后接着做；发送前的估算。

use std::sync::atomic::AtomicBool;

use concept_engine::align::{self, Alignment};
use concept_engine::audit;
use concept_engine::fixture;
use concept_engine::flow::{self, NoRecorder, Ports, Recorder};
use concept_engine::mock::{self, MockFetch, MockModel, MockSearch, MockShelf, Store};
use concept_engine::model::{Card, CardKind, Category, Run, RunStatus, Scope, SourceOrigin, Status, ZhStatus};
use concept_engine::prompts::{Books, SourceText};
use concept_engine::render;
use concept_engine::skill::{self, Entry, Skill};
use concept_engine::text::{Role, Text};
use research_engine::model::ModelRef;
use research_engine::ports::{ErrorKind, Model};
use serde_json::Value;

struct World {
    en: Text,
    zh: Text,
    al: Alignment,
    store: Store,
}

impl World {
    fn new() -> World {
        let en = fixture::english();
        let zh = fixture::chinese();
        let al = align::align(&en, &zh);
        World { en, zh, al, store: Store::default() }
    }

    fn books(&self) -> Books<'_> {
        Books { original: &self.en, translation: Some(&self.zh), alignment: Some(&self.al) }
    }
}

fn skill(name: &str) -> Skill {
    skill::builtin().into_iter().find(|s| s.name == name).unwrap()
}

fn scope(entry: Entry, paragraphs: &[&str]) -> Scope {
    Scope { entry, text: "en".into(), translation: Some("zh".into()), paragraphs: paragraphs.iter().map(|s| s.to_string()).collect(), unit: None, question: None, terms: Vec::new(), cards: Vec::new(), instruction: None, skip: Vec::new(), with: Vec::new() }
}

fn chapter_scope(w: &World, unit: &str) -> Scope {
    let u = w.en.chapters().into_iter().find(|u| u.key == unit).unwrap().index;
    let keys: Vec<String> = w.en.body_of(u).map(|p| p.key.clone()).collect();
    Scope { unit: Some(unit.into()), paragraphs: keys, ..scope(Entry::Chapter, &[]) }
}

struct Saves(usize);

impl Recorder for Saves {
    fn save(&mut self, _: &Run) -> Result<(), String> {
        self.0 += 1;
        Ok(())
    }
}

fn run_with(w: &World, skill: &Skill, scope: Scope, targets: Vec<Card>, model: &dyn Model, has_review: bool) -> Run {
    let mut run = flow::new_run("u1", skill, scope, targets, ModelRef { provider: "mock".into(), model: "模拟模型".into() }, "medium", true, "2026-11-13T10:00:00".into()).unwrap();
    execute(w, skill, &mut run, model, has_review, &AtomicBool::new(false));
    run
}

fn execute(w: &World, skill: &Skill, run: &mut Run, model: &dyn Model, has_review: bool, cancel: &AtomicBool) -> usize {
    let shelf = MockShelf { store: w.store.clone(), has_review };
    let fetch = MockFetch { store: w.store.clone() };
    let mut rec = Saves(0);
    let now = || "2026-11-13T10:00:05".to_string();
    let progress = |_| {};
    let mut ports = Ports { model, search: &MockSearch, fetch: &fetch, shelf: &shelf, recorder: &mut rec, now: &now, progress: &progress, cancel };
    flow::execute(run, skill, &w.books(), &mut ports).unwrap();
    rec.0
}

fn lookup(w: &World, run: &Run) -> impl Fn(&str) -> Option<String> {
    let shelf = MockShelf { store: w.store.clone(), has_review: true };
    let texts: Vec<SourceText> = flow::source_texts(run, &shelf);
    move |id: &str| texts.iter().find(|t| t.id == id).map(|t| t.text.clone())
}

fn findings(w: &World, run: &Run) -> Vec<audit::Finding> {
    let l = lookup(w, run);
    audit::audit(run, &w.books(), &l)
}

fn chapter_run(w: &World) -> Run {
    run_with(w, &skill("chapter"), chapter_scope(w, "I.i"), Vec::new(), &MockModel::default(), true)
}

#[test]
fn whole_chapter_concepts_arguments_relations() {
    let w = World::new();
    let run = chapter_run(&w);
    assert_eq!(run.status, RunStatus::Done, "{:?}", run.error);
    assert!(run.rejected.is_empty(), "{:?}", run.rejected);
    let kinds: Vec<CardKind> = run.cards.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [CardKind::Concept, CardKind::Concept, CardKind::Concept, CardKind::Concept, CardKind::Argument, CardKind::Argument]);
    assert_eq!(run.cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["K1", "K2", "K3", "K4", "K5", "K6"]);
    assert_eq!(run.relations.len(), 4);
    assert_eq!(run.steps.len(), 3);
    // 原文句：逐字核对过，带位置和页码；中译句在对齐的中文段落里核对
    let k1 = &run.cards[0];
    let o = &k1.sentences[0];
    assert_eq!((o.category, o.check.status), (Category::Original, Status::Verified));
    assert_eq!(o.paragraph.as_deref(), Some("I.i.1"));
    assert_eq!(o.check.pages.as_deref(), Some("101"));
    assert_eq!(o.check.zh_status, Some(ZhStatus::Verified));
    assert_eq!(o.check.zh_paragraph.as_deref(), Some("I.i.1"));
    // 他人：译本的旁注
    let t = &k1.sentences[2];
    assert_eq!((t.category, t.check.status, t.paragraph.as_deref()), (Category::Others, Status::Verified, Some("译:I.i.1 旁注")));
    // 解释没挂上原文：照样显示，醒目地标出
    let k3 = &run.cards[2];
    assert_eq!(k3.sentences[1].check.status, Status::Unanchored);
    assert_eq!(k3.sentences[1].check.note.as_deref(), Some("没挂上原文"));
    // 没给中译句的原文：只显示整段译文
    let k5 = &run.cards[4];
    assert_eq!(k5.sentences[2].check.zh_status, Some(ZhStatus::ParagraphOnly));
    // 发出去的请求：第二步带着第一步的卡片
    assert!(findings(&w, &run).is_empty());
    // 导出三种（Word 在 App 一侧，用同一份 Doc）
    let books = w.books();
    let md = render::markdown(&run, &books, &[]);
    assert!(md.contains("### K1 division of labour / 分工") && md.contains("〔原文〕") && md.contains("**⚠ 没挂上原文**"));
    let html = render::html(&run, &books, &[]);
    assert!(html.contains("data-card=\"K5\"") && html.contains("id=\"card-K1\"") && html.contains("<mark id=\"m-K1-1\">"));
    assert!(!html.contains("<link") && !html.contains(" src=") && !html.contains("@import"), "单文件 HTML 不引外部资源");
    let doc = render::document(&run, &books, &[]);
    assert!(doc.sections.iter().any(|s| s.title.starts_with("概念卡")));
}

#[test]
fn optional_steps_can_be_skipped_and_requests_carry_earlier_cards() {
    let w = World::new();
    let model = MockModel::default();
    let mut sc = chapter_scope(&w, "I.i");
    sc.skip = vec!["relations".into()];
    let run = run_with(&w, &skill("chapter"), sc, Vec::new(), &model, true);
    assert_eq!((run.cards.len(), run.relations.len(), run.steps.len()), (6, 0, 2));
    let seen = model.seen.lock().unwrap();
    assert!(seen[1].user.iter().any(|u| u.contains("[K1] 概念卡：division of labour / 分工")));
    assert!(seen[0].user.iter().any(|u| u.contains("[I.i.11]")) && seen[0].user.iter().any(|u| u.contains("[译:I.i.1 旁注]")));
    assert!(seen[0].user.iter().any(|u| u.starts_with("## 对齐的中译")));
    // 三步都跳过：拒绝
    let mut none = chapter_scope(&w, "I.i");
    none.skip = vec!["concepts".into(), "arguments".into(), "relations".into()];
    assert!(flow::new_run("x", &skill("chapter"), none, Vec::new(), ModelRef::default(), "medium", true, String::new()).unwrap_err().contains("模型步骤"));
    // 入口不对、自由技能没写要求
    assert!(flow::new_run("x", &skill("chapter"), scope(Entry::Selection, &["I.i.1"]), Vec::new(), ModelRef::default(), "medium", true, String::new()).is_err());
    assert!(flow::new_run("x", &skill("free"), scope(Entry::Selection, &["I.i.1"]), Vec::new(), ModelRef::default(), "medium", true, String::new()).unwrap_err().contains("要求"));
}

#[test]
fn selection_question_and_concept_entries() {
    let w = World::new();
    // 划选一段
    let run = run_with(&w, &skill("passage"), scope(Entry::Selection, &["I.i.3"]), Vec::new(), &MockModel::default(), true);
    assert_eq!(run.status, RunStatus::Done);
    assert_eq!(run.cards.iter().map(|c| c.kind).collect::<Vec<_>>(), [CardKind::Argument, CardKind::Note]);
    let note = &run.cards[1].sentences[0];
    assert_eq!((note.category, note.check.status, note.quote.as_deref()), (Category::Explanation, Status::Verified, Some("the trade of a pin-maker")));
    assert!(note.check.span.is_some());
    // 问一个问题：第三章段数对不上，原文句没有对齐的中译
    let mut q = scope(Entry::Question, &["I.iii.1", "I.iii.2"]);
    q.question = Some("分工为什么受市场范围的限制？".into());
    let run = run_with(&w, &skill("ask"), q, Vec::new(), &MockModel::default(), true);
    assert_eq!(run.cards.len(), 1);
    assert_eq!(run.cards[0].title, "分工为什么受市场范围的限制？");
    assert!(run.cards[0].sentences.iter().filter(|s| s.category == Category::Original).all(|s| s.check.zh_status == Some(ZhStatus::Unaligned)));
    // 追踪概念
    let mut c = scope(Entry::Concept, &["I.i.1", "I.ii.1", "I.iii.1"]);
    c.terms = vec!["division of labour".into(), "分工".into()];
    let run = run_with(&w, &skill("trace"), c, Vec::new(), &MockModel::default(), true);
    assert_eq!(run.cards.iter().map(|c| c.kind).collect::<Vec<_>>(), [CardKind::Concept, CardKind::Note, CardKind::Note, CardKind::Note]);
    assert!(run.rejected.is_empty(), "{:?}", run.rejected);
    assert!(findings(&w, &run).is_empty());
}

#[test]
fn critics_from_the_library_then_the_web() {
    let w = World::new();
    let base = chapter_run(&w);
    let target = base.cards[0].clone();
    let mut sc = scope(Entry::Card, &[]);
    sc.cards = vec![(base.uid.clone(), target.id.clone())];
    // 只查文献库：第二条评论引的 S2 不存在，那一句拦下，卡片里只剩解释
    let run = run_with(&w, &skill("critics"), sc.clone(), vec![target.clone()], &MockModel::default(), true);
    assert_eq!(run.status, RunStatus::Done, "{:?}", run.error);
    assert_eq!(run.sources.len(), 1);
    assert_eq!((run.sources[0].origin, run.sources[0].item_id), (SourceOrigin::Library, Some(mock::LIBRARY_ITEM_ID)));
    assert_eq!(run.cards.len(), 2);
    assert!(run.cards[1].sentences.iter().all(|s| s.category == Category::Explanation));
    let c = &run.cards[0];
    assert_eq!((c.id.as_str(), c.kind, c.about.clone()), ("K2", CardKind::Comment, vec!["K1".to_string()]));
    assert_eq!((c.sentences[0].category, c.sentences[0].check.status, c.sentences[0].source.as_deref()), (Category::Others, Status::Verified, Some("S1")));
    assert_eq!(run.rejected.iter().map(|r| r.reason.as_str()).collect::<Vec<_>>(), ["来源「S2」不存在"]);
    assert_eq!(run.relations.len(), 2);
    assert!(findings(&w, &run).is_empty());
    // 也上网找：网页检索 + 下载（一个 404 记下原因），两条评论都核对通过
    let mut sc2 = sc.clone();
    sc2.with = vec!["web".into()];
    let run = run_with(&w, &skill("critics"), sc2, vec![target.clone()], &MockModel::default(), true);
    assert_eq!(run.sources.iter().map(|s| (s.origin, s.error.is_some())).collect::<Vec<_>>(), [(SourceOrigin::Library, false), (SourceOrigin::Web, false), (SourceOrigin::Web, true)]);
    assert_eq!(run.cards.len(), 2);
    assert_eq!(run.relations.len(), 2);
    assert!(run.rejected.is_empty(), "{:?}", run.rejected);
    assert!(findings(&w, &run).is_empty());
    // 文献库里什么也没有、也不上网：模型只能写解释类，拿不出他人类
    let run = run_with(&w, &skill("critics"), sc, vec![target], &MockModel::default(), false);
    assert!(run.sources.is_empty());
    assert!(run.cards.iter().flat_map(|c| &c.sentences).all(|s| s.category != Category::Others));
}

#[test]
fn counterfactual_and_free() {
    let w = World::new();
    let base = chapter_run(&w);
    let argument = base.cards[4].clone();
    let mut sc = scope(Entry::Card, &[]);
    sc.cards = vec![(base.uid.clone(), argument.id.clone())];
    sc.instruction = Some("如果市场很小".into());
    sc.paragraphs = vec!["I.iii.1".into()];
    let run = run_with(&w, &skill("counterfactual"), sc, vec![argument], &MockModel::default(), true);
    assert_eq!(run.status, RunStatus::Done);
    let c = &run.cards[0];
    assert_eq!(c.kind, CardKind::Counterfactual);
    // 推演的前提按原文核对
    assert_eq!((c.sentences[0].category, c.sentences[0].check.status), (Category::Speculation, Status::Speculative));
    assert!(c.sentences[0].check.span.is_some());
    assert_eq!(run.relations[0].to, "K5");
    let mut f = chapter_scope(&w, "I.i");
    f.instruction = Some("做一份术语表".into());
    let run = run_with(&w, &skill("free"), Scope { entry: Entry::Chapter, ..f }, Vec::new(), &MockModel::default(), true);
    assert_eq!(run.cards.len(), 1);
    assert_eq!(run.cards[0].kind, CardKind::Note);
}

/// 改坏模型的输出：取第一张卡第 i 句，用 f 改它
fn tampered(tag: &'static str, f: impl Fn(&mut Value) + Send + Sync + 'static) -> MockModel {
    MockModel::with_tamper(move |req, text| {
        if req.tag != tag {
            return text;
        }
        let mut v: Value = serde_json::from_str(&text).unwrap();
        f(&mut v);
        v.to_string()
    })
}

fn rejected_reasons(w: &World, model: MockModel) -> (Run, Vec<String>) {
    let mut sc = chapter_scope(w, "I.i");
    sc.skip = vec!["arguments".into(), "relations".into()];
    let run = run_with(w, &skill("chapter"), sc, Vec::new(), &model, true);
    let reasons = run.rejected.iter().map(|r| r.reason.clone()).collect();
    (run, reasons)
}

#[test]
fn tampered_model_output_is_stopped() {
    let w = World::new();
    // 引文改一个字
    let (run, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        let s = &mut v["cards"][1]["sentences"][0];
        let q = s["quote"].as_str().unwrap().replace("dexterity", "dexteriti");
        s["quote"] = q.clone().into();
        s["text"] = q.into();
    }));
    assert_eq!(r, ["引文在「I.i.6」里逐字找不到"]);
    assert_eq!(run.cards[1].sentences.len(), 1, "改坏的那句不进卡片");
    // 挂错段落
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| v["cards"][1]["sentences"][0]["paragraph"] = "I.i.7".into()));
    assert_eq!(r, ["引文不在「I.i.7」里（在「I.i.6」里找得到：挂错段落）"]);
    // 不存在的段落号：原文拦下；解释去掉锚点，标「没挂上原文」
    let (run, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        v["cards"][1]["sentences"][0]["paragraph"] = "I.i.99".into();
        v["cards"][1]["sentences"][1]["paragraph"] = "I.i.98".into();
    }));
    assert_eq!(r, ["段落号「I.i.99」不存在"]);
    let e = &run.cards[1].sentences[0];
    assert_eq!((e.category, e.check.status, e.paragraph.as_deref()), (Category::Explanation, Status::Unanchored, None));
    assert!(e.check.note.as_deref().unwrap().contains("I.i.98"));
    // 标成原文却没有锚点
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| v["cards"][1]["sentences"][0]["paragraph"] = Value::Null));
    assert_eq!(r, ["标成原文却没有锚点（没写段落号）"]);
    // 推演混进原文类：文字是推演，引文是真的；或者文字和引文都是编的
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        v["cards"][0]["sentences"][0]["text"] = "Without the division of labour the pin factory would make almost nothing.".into();
        let s = &mut v["cards"][1]["sentences"][0];
        s["text"] = "If every workman made whole pins, dexterity would never improve.".into();
        s["quote"] = s["text"].clone();
    }));
    assert_eq!(r, ["标成原文，但文字和引文不一致：原文类只能是逐字引文", "引文在「I.i.6」里逐字找不到"]);
    // 旁注、导读被标成原文
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        v["cards"][0]["sentences"][2]["category"] = "original".into();
        let s = &mut v["cards"][1]["sentences"][0];
        s["paragraph"] = "Editor’s Introduction ¶1".into();
        s["quote"] = "Smith regarded the division of labour as the great cause of improvement.".into();
        s["text"] = s["quote"].clone();
    }));
    assert_eq!(r, ["「译:I.i.1 旁注」是译本里的段落：原文只能挂原著正文", "「Editor’s Introduction ¶1」是导读，不是原著正文，不能标成原文"]);
    // 他人观点没有来源
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| v["cards"][0]["sentences"][2]["paragraph"] = Value::Null));
    assert_eq!(r, ["他人的观点没有来源"]);
    // 他人挂到原著正文上
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        let s = &mut v["cards"][0]["sentences"][2];
        s["paragraph"] = "I.i.1".into();
        s["quote"] = "the division of labour".into();
    }));
    assert_eq!(r, ["「I.i.1」是原著正文：原著的话要标成原文，不是他人"]);
    // 一句都不剩的卡片不要
    let (run, _) = rejected_reasons(&w, tampered("chapter/concepts", |v| {
        for s in v["cards"][2]["sentences"].as_array_mut().unwrap() {
            s["category"] = "original".into();
            s["paragraph"] = Value::Null;
        }
    }));
    assert_eq!(run.cards.len(), 3);
    assert!(run.cards.iter().all(|c| c.title != "machinery / 机械"));
    // 这一步不该出的卡片种类
    let (_, r) = rejected_reasons(&w, tampered("chapter/concepts", |v| v["cards"][3]["kind"] = "argument".into()));
    assert!(r.iter().all(|x| x == "这一步不出论证卡") && r.len() == 2);
}

#[test]
fn audit_finds_tampered_stored_results() {
    let w = World::new();
    let good = chapter_run(&w);
    assert!(findings(&w, &good).is_empty());
    let check = |f: &dyn Fn(&mut Run)| {
        let mut r = good.clone();
        f(&mut r);
        findings(&w, &r)
    };
    // 引文改了一个字
    let f = check(&|r| r.cards[1].sentences[0].quote = Some(r.cards[1].sentences[0].quote.clone().unwrap().replace("workmen", "workman")));
    assert_eq!((f.len(), f[0].at.as_str(), f[0].sentence), (1, "K2", Some(1)));
    // 锚点换了一段
    assert_eq!(check(&|r| r.cards[1].sentences[0].paragraph = Some("I.i.7".into())).len(), 1);
    // 没挂上的标成已核实
    let f = check(&|r| r.cards[2].sentences[1].check.status = Status::Verified);
    assert!(f[0].message.contains("已核实"));
    // 只改了存下的页码
    assert!(check(&|r| r.cards[0].sentences[0].check.pages = Some("999".into()))[0].message.contains("页码"));
    // 关系连到不存在的卡片
    assert!(check(&|r| r.relations[0].to = "K99".into())[0].message.contains("不存在的卡片"));
    // 存下的文本被换了：原著第一段少了几个字
    let mut en = w.en.clone();
    let i = en.paragraph("I.i.1").unwrap().seq;
    en.paragraphs[i].text = en.paragraphs[i].text.replace("dexterity, and ", "");
    let books = Books { original: &en, translation: Some(&w.zh), alignment: Some(&w.al) };
    let l = lookup(&w, &good);
    let f = audit::audit(&good, &books, &l);
    assert!(f.iter().any(|x| x.at == "K1"));
    // 审计的问题导出时写在最前面
    let html = render::html(&good, &books, &f);
    assert!(html.contains("审计发现问题"));
}

#[test]
fn cancel_and_resume_and_failure_mid_way() {
    let w = World::new();
    let sk = skill("chapter");
    let mut run = flow::new_run("u2", &sk, chapter_scope(&w, "I.i"), Vec::new(), ModelRef::default(), "medium", true, String::new()).unwrap();
    // 一开始就取消
    let saves = execute(&w, &sk, &mut run, &MockModel::default(), true, &AtomicBool::new(true));
    assert!(saves >= 1);
    assert_eq!((run.status, run.next_step, run.cards.len()), (RunStatus::Cancelled, 0, 0));
    // 第二步的输出读不懂：停在第二步，费用照记
    let bad = MockModel::with_tamper(|req, text| if req.tag == "chapter/arguments" { "不是 JSON".into() } else { text });
    execute(&w, &sk, &mut run, &bad, true, &AtomicBool::new(false));
    assert_eq!((run.status, run.next_step, run.cards.len()), (RunStatus::Failed, 1, 4));
    assert!(run.error.as_deref().unwrap().contains("重试"));
    assert!(!run.steps.last().unwrap().ok);
    // 接着做：第一步不重做
    execute(&w, &sk, &mut run, &MockModel::default(), true, &AtomicBool::new(false));
    assert_eq!((run.status, run.cards.len(), run.relations.len()), (RunStatus::Done, 6, 4));
    assert_eq!(run.steps.iter().filter(|s| s.id == "concepts").count(), 1);
    // 模型说密钥不对：整次停下
    let mut run2 = flow::new_run("u3", &sk, chapter_scope(&w, "I.i"), Vec::new(), ModelRef::default(), "medium", true, String::new()).unwrap();
    execute(&w, &sk, &mut run2, &mock::FailingModel(ErrorKind::Fatal), true, &AtomicBool::new(false));
    assert_eq!(run2.status, RunStatus::Failed);
    let _ = NoRecorder;
}

#[test]
fn estimates_before_sending() {
    let w = World::new();
    let sk = skill("chapter");
    let run = flow::new_run("u4", &sk, chapter_scope(&w, "I.i"), Vec::new(), ModelRef::default(), "medium", true, String::new()).unwrap();
    let plan = flow::planned(&sk, &run, &w.books(), &[]);
    assert_eq!(plan.iter().map(|p| p.step.as_str()).collect::<Vec<_>>(), ["concepts", "arguments", "relations"]);
    assert_eq!(plan.iter().map(|p| p.extra_input_tokens).collect::<Vec<_>>(), [0, 16_000, 28_000]);
    assert_eq!(plan[0].request.as_ref().unwrap().max_tokens, 16_000);
    // 反方：来源最多 5 条（再上网 3 条），按每条写满算
    let critics = skill("critics");
    let base = chapter_run(&w);
    let mut sc = scope(Entry::Card, &[]);
    sc.with = vec!["web".into()];
    let run = flow::new_run("u5", &critics, sc, vec![base.cards[0].clone()], ModelRef::default(), "medium", true, String::new()).unwrap();
    let plan = flow::planned(&critics, &run, &w.books(), &[]);
    assert_eq!(plan.len(), 3);
    assert_eq!(plan[2].extra_input_tokens, 8 * flow::SOURCE_TOKENS);
    assert_eq!(flow::search_terms(&run), ["division of labour", "分工"]);
}

#[test]
fn mock_model_makes_up_replies_for_other_chapters() {
    // 开发版里对别的章跑：录制的回复挂的段落这次没有，现编一份，照样核对通过
    let w = World::new();
    let run = run_with(&w, &skill("chapter"), chapter_scope(&w, "I.ii"), Vec::new(), &MockModel::default(), true);
    assert_eq!(run.status, RunStatus::Done);
    assert!(run.cards.len() >= 3);
    assert!(run.rejected.is_empty(), "{:?}", run.rejected);
    assert!(!run.relations.is_empty());
    assert!(w.zh.paragraphs.iter().any(|p| p.role == Role::Sidenote));
}
