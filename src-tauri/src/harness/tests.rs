//! Agent 循环的测试：脚本化的模拟模型，不联网。查、算、问、交付、并行、轮数上限、打转、月上限、拒答、停、子代理。

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use serde_json::json;

use super::tools::{Env, KbFound, KbRead, LibHit, Toolbox};
use super::*;
use crate::agent::usage::Prices;
use crate::db::tests::temp_dir;
use crate::gateway::catalog;
use crate::gateway::meter::Meter;
use crate::gateway::mock::{MockModel, Turn};
use crate::gateway::{GateError, Message, Part, SystemBlock};

struct FakeEnv;

impl Env for FakeEnv {
    fn library_search(&self, query: &str, limit: usize) -> Result<Vec<LibHit>, String> {
        let all = vec![
            LibHit { id: 1, title: "An Inquiry into the Nature and Causes of the Wealth of Nations".into(), creator: "Smith".into(), year: Some(1776), container: String::new(), item_type: "book".into() },
            LibHit { id: 2, title: "The Division of Labor Is Limited by the Extent of the Market".into(), creator: "Stigler".into(), year: Some(1951), container: "Journal of Political Economy".into(), item_type: "journalArticle".into() },
        ];
        Ok(all.into_iter().filter(|h| query.is_empty() || h.title.to_lowercase().contains(&query.to_lowercase())).take(limit).collect())
    }

    fn library_item(&self, id: i64) -> Result<String, String> {
        if id == 2 { Ok("Stigler (1951)，JPE 59(3)。摘要：……".into()) } else { Err("没有这个条目。".into()) }
    }

    fn kb_search(&self, query: &str, _limit: usize) -> Result<KbFound, String> {
        Ok(KbFound { text: format!("知识库里和「{query}」最相关的 1 段：\n\n[P7] 《国富论》 · I.iii.1 · 第 121 页\nAs it is the power of exchanging…"), hits: 1, semantic: true })
    }

    fn kb_read(&self, req: &KbRead) -> Result<String, String> {
        match req.passage {
            Some(7) => Ok(format!("[P6] I.ii.5\n……\n\n[P7] I.iii.1\nAs it is the power of exchanging…（{}）", req.scope)),
            _ => Err("没有这一段。".into()),
        }
    }

    fn kb_chapter(&self, passage: i64) -> Result<(String, String), String> {
        if passage == 7 { Ok(("第三章".into(), "《国富论》「第三章」：\n\n[P7] I.iii.1\nAs it is the power of exchanging…".into())) } else { Err("没有这一段。".into()) }
    }

    fn web_search(&self, query: &str, _limit: usize) -> Result<(String, usize), String> {
        Ok((format!("网上搜「{query}」的 1 条（模拟搜索）：\n\n1. 示例市统计局 — stats.example.gov\n   https://stats.example.gov/2024\n"), 1))
    }

    fn web_fetch(&self, url: &str, from: Option<i64>) -> Result<(String, String), String> {
        match (url, from) {
            (_, Some(9)) => Ok(("[W9] 后半页".into(), "stats.example.gov".into())),
            ("https://stats.example.gov/2024", None) => Ok(("「2024 年公报」：\n[W8] 16-24 岁失业率 15.7%。\n（后面还有 1 段没给出：from 写 W9 接着读。）".into(), "stats.example.gov".into())),
            _ => Err("连不上".into()),
        }
    }
}

fn spec() -> &'static catalog::ModelSpec {
    catalog::model("anthropic/claude-sonnet-5-5").unwrap()
}

fn strong_spec() -> &'static catalog::ModelSpec {
    catalog::model("anthropic/claude-opus-5-5").unwrap()
}

fn meter(name: &str) -> Meter {
    let mut m = Meter::new(&temp_dir(name), 30.0, Prices::default());
    m.now = || "2026-11-26T10:00:00.000Z".into();
    m
}

fn system() -> Vec<SystemBlock> {
    vec![SystemBlock { text: "规矩".into(), cache: true }]
}

struct Run {
    out: Outcome,
    events: Vec<Event>,
}

fn go(model: &MockModel, strong: Option<&MockModel>, meter: &Meter, limits: Limits, cancel: bool, question: &str) -> Run {
    let events = Mutex::new(Vec::new());
    let sink = |e: Event| events.lock().unwrap().push(e);
    let cancel = AtomicBool::new(cancel);
    let ctx = Ctx {
        model,
        effort: Some("medium"),
        strong: strong.map(|s| (s as &dyn crate::gateway::Model, Some("high"))),
        meter,
        conversation: Some("conv-1"),
        env: &FakeEnv,
        cancel: &cancel,
        sink: &sink,
        limits,
    };
    let out = run(&ctx, &system(), &Toolbox::main(), vec![Message::user(question)], "chat");
    Run { out, events: events.into_inner().unwrap() }
}

fn agents(steps: &[Step]) -> Vec<&str> {
    steps.iter().map(|s| s.agent.as_str()).collect()
}

#[test]
fn searches_then_answers_and_keeps_the_transcript() {
    let m = MockModel::scripted(spec(), vec![Turn { text: "我查一下。".into(), calls: vec![("library_search".into(), json!({ "query": "division" }))], ..Default::default() }, Turn::text("Stigler 1951 讲过这件事。")]);
    let meter = meter("harness-basic");
    let r = go(&m, None, &meter, Limits::default(), false, "分工受什么限制？");
    let o = &r.out;
    assert_eq!(o.ending, Ending::Done);
    assert_eq!(o.text, "Stigler 1951 讲过这件事。");
    assert_eq!(agents(&o.steps), ["think", "lib", "write"]);
    assert!(o.steps[1].text.contains("division") && o.steps[1].text.contains("1 条"));
    // 这一回新加的：模型 → 工具结果 → 模型
    assert_eq!(o.turns.len(), 3);
    match &o.turns[1].parts[0] {
        Part::ToolResult { output, is_error, .. } => assert!(!is_error && output.contains("[条目 2] Stigler （1951）")),
        other => panic!("{other:?}"),
    }
    // 第二次请求带着第一次的全部，没有改前面的（只往后追加）
    let reqs = m.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].messages[..1], reqs[0].messages[..]);
    assert_eq!(reqs[0].cache_key.as_deref(), Some("conv-1"));
    assert_eq!(reqs[0].effort.as_deref(), Some("medium"));
    assert!(!reqs[1].no_tools);
    // 过程话流出来以后清掉重来；最后的回答流出来
    let reset = r.events.iter().rposition(|e| *e == Event::Reset).expect("过程话清掉了");
    let streamed: String = r.events[reset..].iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect();
    assert_eq!(streamed, "Stigler 1951 讲过这件事。");
    // 每次调用记一行账（模拟的），记着对话
    let calls = meter.ledger.all().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.mock && c.purpose == "chat" && c.conversation.as_deref() == Some("conv-1")));
}

#[test]
fn read_only_tools_run_together_and_keep_their_order() {
    let m = MockModel::scripted(spec(), vec![Turn::calls(vec![("calculate", json!({ "expression": "1+1" })), ("calculate", json!({ "expression": "2*3" })), ("library_item", json!({ "id": 2 })), ("calculate", json!({ "expression": "1/0" }))]), Turn::text("好了")]);
    let r = go(&m, None, &meter("harness-parallel"), Limits::default(), false, "算");
    let results: Vec<(String, bool)> = r.out.turns[1].parts.iter().map(|p| match p {
        Part::ToolResult { output, is_error, .. } => (output.clone(), *is_error),
        _ => panic!(),
    }).collect();
    assert_eq!(results[0], ("1+1 = 2".into(), false));
    assert_eq!(results[1], ("2*3 = 6".into(), false));
    assert!(results[2].0.starts_with("Stigler"));
    assert!(results[3].1 && results[3].0.contains("除数"));
    assert_eq!(agents(&r.out.steps), ["think", "compute", "compute", "lib", "write"]);
}

#[test]
fn a_question_card_ends_the_turn() {
    let m = MockModel::scripted(spec(), vec![Turn { text: "你想从哪个方向看？".into(), calls: vec![("ask_user".into(), json!({ "question": "先看哪一个？", "options": ["斯密", "Stigler", ""] }))], ..Default::default() }, Turn::text("不该走到这里")]);
    let r = go(&m, None, &meter("harness-ask"), Limits::default(), false, "分工");
    assert_eq!(r.out.ending, Ending::Asked);
    assert_eq!(r.out.ask, Some(AskCard { question: "先看哪一个？".into(), options: vec!["斯密".into(), "Stigler".into()] }));
    assert_eq!(m.requests().len(), 1, "问了就停下，不再调模型");
    // 问题卡的调用也配上了结果（下一回的对话成立）
    assert!(matches!(r.out.turns.last().unwrap().parts[0], Part::ToolResult { .. }));
    assert_eq!(r.out.text, "你想从哪个方向看？");
}

#[test]
fn a_delivery_card_is_kept_and_the_model_goes_on() {
    let m = MockModel::scripted(spec(), vec![Turn::call("deliver", json!({ "kind": "concept", "title": "分工受市场范围限制", "summary": "斯密的推理", "body": "正文" })), Turn::text("卡片在下面。")]);
    let r = go(&m, None, &meter("harness-deliver"), Limits::default(), false, "做张卡");
    assert_eq!(r.out.delivery.as_ref().map(|d| (d.kind.as_str(), d.title.as_str(), d.body.as_str())), Some(("concept", "分工受市场范围限制", "正文")));
    assert_eq!(r.out.text, "卡片在下面。");
    assert_eq!(agents(&r.out.steps), ["think", "save", "write"]);
}

#[test]
fn round_limit_gives_one_last_round_without_tools() {
    let n = std::sync::atomic::AtomicUsize::new(0);
    let m = MockModel::with_brain(
        spec(),
        Box::new(move |req| {
            if req.no_tools {
                return Turn::text("先写到这里。");
            }
            let i = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Turn::call("calculate", json!({ "expression": format!("{i}+1") }))
        }),
    );
    let r = go(&m, None, &meter("harness-limit"), Limits { rounds: 3, ..Limits::default() }, false, "一直算");
    let reqs = m.requests();
    assert_eq!(reqs.len(), 4);
    assert_eq!(reqs.iter().map(|r| r.no_tools).collect::<Vec<_>>(), [false, false, false, true]);
    assert_eq!(r.out.ending, Ending::Limit);
    assert_eq!(r.out.text, "先写到这里。");
    assert!(r.out.note.unwrap().contains("3 轮"));
}

#[test]
fn the_same_call_with_the_same_result_is_flagged_then_wrapped_up() {
    let m = MockModel::with_brain(spec(), Box::new(|req| if req.no_tools { Turn::text("就这样吧。") } else { Turn::call("library_search", json!({ "query": "smith" })) }));
    let r = go(&m, None, &meter("harness-loop"), Limits::default(), false, "打转");
    let outputs: Vec<String> = r.out.turns.iter().filter(|t| t.role == crate::gateway::Role::User).map(|t| match &t.parts[0] {
        Part::ToolResult { output, .. } => output.clone(),
        _ => String::new(),
    }).collect();
    assert!(!outputs[1].contains("提醒"));
    assert!(outputs[2].contains("重复 3 次"));
    assert_eq!(outputs.len(), LOOP_STOP as usize);
    assert_eq!(m.requests().last().unwrap().no_tools, true);
    assert_eq!(r.out.ending, Ending::Looping);
}

#[test]
fn refusal_cancel_and_errors_end_with_a_note() {
    let m = MockModel::scripted(spec(), vec![Turn { text: String::new(), stop: Some(crate::gateway::Stop::Refusal), ..Default::default() }]);
    let r = go(&m, None, &meter("harness-refusal"), Limits::default(), false, "x");
    assert_eq!(r.out.ending, Ending::Refusal);
    assert!(r.out.note.unwrap().contains("拒绝"));

    let m = MockModel::scripted(spec(), vec![Turn::text("不该发出去")]);
    let r = go(&m, None, &meter("harness-cancel"), Limits::default(), true, "x");
    assert_eq!(r.out.ending, Ending::Stopped);
    assert!(m.requests().is_empty());

    let m = MockModel::scripted(spec(), vec![Turn { error: Some(GateError::new(crate::gateway::ErrorKind::Budget, "超过月上限")), ..Default::default() }]);
    let r = go(&m, None, &meter("harness-budget"), Limits::default(), false, "x");
    assert_eq!(r.out.ending, Ending::Budget);
    assert_eq!(r.out.note.as_deref(), Some("超过月上限"));

    // 参数被截断的、不认识的工具：回一个出错的结果，模型接着做
    let m = MockModel::scripted(spec(), vec![Turn::calls(vec![("calculate", json!({ crate::gateway::BAD_INPUT_KEY: "{\"expr" })), ("rm_rf", json!({}))]), Turn::text("好")]);
    let r = go(&m, None, &meter("harness-bad"), Limits::default(), false, "x");
    let errs: Vec<String> = r.out.turns[1].parts.iter().map(|p| if let Part::ToolResult { output, is_error: true, .. } = p { output.clone() } else { String::new() }).collect();
    assert!(errs[0].contains("不是完整的 JSON") && errs[1].contains("没有叫 rm_rf"));
    assert_eq!(r.out.ending, Ending::Done);
}

#[test]
fn a_subagent_reads_with_the_stronger_model_and_reports_back() {
    let main = MockModel::scripted(spec(), vec![Turn::calls(vec![("delegate", json!({ "kind": "read", "title": "读斯密", "task": "读第一篇第三章，交回要点", "materials": "I.iii.1 ……" })), ("delegate", json!({ "kind": "read", "title": "读 Stigler", "task": "读 Stigler 1951" }))]), Turn::text("两份报告都回来了。")]);
    let strong = MockModel::with_brain(strong_spec(), Box::new(|req| {
        let task = req.messages[0].text();
        if req.messages.len() == 1 { Turn::call("calculate", json!({ "expression": "1776-1951" })) } else { Turn::text(format!("报告：{}", task.chars().take(10).collect::<String>())) }
    }));
    let meter = meter("harness-sub");
    let r = go(&main, Some(&strong), &meter, Limits::default(), false, "斯密和 Stigler 怎么说");
    let o = &r.out;
    assert_eq!(o.subagents, 2);
    assert_eq!(o.text, "两份报告都回来了。");
    // 报告交回主对话
    let results = &o.turns[1].parts;
    assert!(matches!(&results[0], Part::ToolResult { output, is_error: false, .. } if output.contains("子代理「读斯密」交回的报告") && output.contains("任务：读第一篇第三章")));
    // 子代理：强一档、子代理的规矩、只有只读的工具、看不到主对话
    let sub_reqs = strong.requests();
    assert_eq!(sub_reqs.len(), 4);
    assert!(sub_reqs[0].system[0].text.contains("规矩") || sub_reqs[0].system[0].text.contains("子代理"));
    let names: Vec<&str> = sub_reqs[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["calculate", "kb_search", "kb_read", "library_search", "library_item", "web_search", "web_fetch"]);
    assert_eq!(sub_reqs[0].effort.as_deref(), Some("high"));
    assert!(sub_reqs[0].messages[0].text().starts_with("任务："));
    // 状态行：派的那一格，下面是它的步骤（带它的 id）
    let bg: Vec<&Step> = o.steps.iter().filter(|s| s.agent == "bg").collect();
    assert_eq!(bg.len(), 2);
    assert!(bg[0].text.contains("Claude Opus 5.5"));
    let subs: std::collections::HashSet<&str> = o.steps.iter().filter_map(|s| s.sub.as_deref()).collect();
    assert_eq!(subs, ["s1", "s2"].into_iter().collect());
    assert!(r.events.iter().any(|e| matches!(e, Event::Step(s) if s.sub.is_some() && s.agent == "compute")));
    // 账本：主对话两次、子代理各两次，都记着这段对话
    let calls = meter.ledger.all().unwrap();
    assert_eq!(calls.iter().filter(|c| c.purpose == "chat_subagent").count(), 4);
    assert_eq!(calls.iter().filter(|c| c.purpose == "chat").count(), 2);
}

#[test]
fn subagents_are_capped_per_answer() {
    let calls: Vec<(&str, serde_json::Value)> = (0..3).map(|i| ("delegate", json!({ "kind": "read", "title": format!("第 {i} 份"), "task": "读" }))).collect();
    let main = MockModel::scripted(spec(), vec![Turn::calls(calls), Turn::text("好")]);
    let strong = MockModel::with_brain(strong_spec(), Box::new(|_| Turn::text("报告")));
    let r = go(&main, Some(&strong), &meter("harness-sub-cap"), Limits { subagents: 2, ..Limits::default() }, false, "读三份");
    assert_eq!(r.out.subagents, 2);
    assert!(matches!(&r.out.turns[1].parts[2], Part::ToolResult { output, is_error: true, .. } if output.contains("到上限")));
}

#[test]
fn knowledge_base_tools_and_a_whole_chapter_for_a_subagent() {
    let main = MockModel::scripted(
        spec(),
        vec![
            Turn::call("kb_search", json!({ "query": "分工 市场" })),
            Turn::calls(vec![("kb_read", json!({ "passage": "P7", "scope": "chapter" })), ("delegate", json!({ "kind": "read", "title": "", "task": "读这一章，交回要点", "chapter": "[P7]" })), ("kb_read", json!({ "passage": "P99" }))]),
            Turn::text("分工受市场范围限制 [P7]。"),
        ],
    );
    let strong = MockModel::with_brain(strong_spec(), Box::new(|req| Turn::text(if req.messages[0].text().contains("[P7] I.iii.1") { "报告：拿到了整章 [P7]" } else { "报告：没有整章" })));
    let r = go(&main, Some(&strong), &meter("harness-kb"), Limits::default(), false, "分工受什么限制");
    let o = &r.out;
    assert_eq!(o.ending, Ending::Done);
    let texts: Vec<(&str, &str)> = o.steps.iter().filter(|s| s.sub.is_none()).map(|s| (s.agent.as_str(), s.text.as_str())).collect();
    assert!(texts.contains(&("search", "在知识库里找「分工 市场」：1 段")), "{texts:?}");
    assert!(texts.contains(&("read", "读了 P7 所在的整章")), "{texts:?}");
    // 子代理拿到整章原文（不经主对话的上下文），标题用章名，报告里照样带编号
    assert!(texts.iter().any(|(a, t)| *a == "bg" && t.contains("第三章")), "{texts:?}");
    let results = &o.turns[3].parts;
    assert!(matches!(&results[0], Part::ToolResult { output, is_error: false, .. } if output.contains("（chapter）")));
    assert!(matches!(&results[1], Part::ToolResult { output, is_error: false, .. } if output.contains("子代理「第三章」交回的报告") && output.contains("拿到了整章 [P7]")));
    // 没有的段落：出错的结果，模型接着做
    assert!(matches!(&results[2], Part::ToolResult { is_error: true, .. }));
    assert_eq!(o.text, "分工受市场范围限制 [P7]。");
}

#[test]
fn web_tools_search_fetch_and_continue() {
    let main = MockModel::scripted(
        spec(),
        vec![
            Turn::call("web_search", json!({ "query": "2024 青年失业率" })),
            Turn::calls(vec![("web_fetch", json!({ "url": "https://stats.example.gov/2024" })), ("web_fetch", json!({ "url": "https://stats.example.gov/2024", "from": "W9" })), ("web_fetch", json!({ "url": "https://nowhere.example" }))]),
            Turn::text("15.7% [W8]。"),
        ],
    );
    let r = go(&main, None, &meter("harness-web"), Limits::default(), false, "查一下 2024 年青年失业率");
    let o = &r.out;
    let texts: Vec<(&str, &str)> = o.steps.iter().map(|s| (s.agent.as_str(), s.text.as_str())).collect();
    assert!(texts.contains(&("web", "在网上搜「2024 青年失业率」：1 条")), "{texts:?}");
    assert!(texts.contains(&("web", "取回了 stats.example.gov 的网页")) && texts.contains(&("web", "接着读 stats.example.gov 的网页")), "{texts:?}");
    let results = &o.turns[3].parts;
    assert!(matches!(&results[0], Part::ToolResult { output, is_error: false, .. } if output.contains("[W8]")));
    assert!(matches!(&results[1], Part::ToolResult { output, is_error: false, .. } if output == "[W9] 后半页"));
    assert!(matches!(&results[2], Part::ToolResult { output, is_error: true, .. } if output.contains("没取到")));
    // 联网工具在「联网」插件里；子代理也能用
    let names: Vec<String> = Toolbox::subagent().specs().into_iter().map(|t| t.name).collect();
    assert!(names.contains(&"web_search".to_string()) && names.contains(&"web_fetch".to_string()));
}

#[test]
fn an_answer_written_alongside_the_delivery_card_is_kept() {
    // DeepSeek 的样子：查完，整篇回答和交付卡写在同一轮，最后一轮只说一句「上面放了交付卡」
    let main = MockModel::scripted(
        spec(),
        vec![
            Turn { text: "我先查一下。".into(), calls: vec![("library_search".into(), json!({ "query": "Smith" }))], ..Default::default() },
            Turn { text: "「看不见的手」只出现在 IV.ii.9。\n\n上下文在讲对外贸易。".into(), calls: vec![("deliver".into(), json!({ "kind": "answer", "title": "看不见的手", "summary": "出处" }))], ..Default::default() },
            Turn::text("上面放了一张交付卡。"),
        ],
    );
    let r = go(&main, None, &meter("harness-answer-with-card"), Limits::default(), false, "看不见的手在哪");
    assert_eq!(r.out.text, "「看不见的手」只出现在 IV.ii.9。\n\n上下文在讲对外贸易。\n\n上面放了一张交付卡。");
    // 过程话（跟着查的）清掉了，跟着交付卡写的没清
    let resets = r.events.iter().filter(|e| matches!(e, Event::Reset)).count();
    assert_eq!(resets, 1);
}
