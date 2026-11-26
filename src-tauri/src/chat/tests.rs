//! 对话、角色、知识库（fresh）。升级、导出恢复带上这几张表：`transfer/tests.rs` 的 rich_library 调 `sample`。

use rusqlite::Connection;
use serde_json::json;

use super::run;
use super::store::{self, Role};
use crate::kb::store::{self as kbs, Material};
use crate::db::tests::memory_db;

const NOW: &str = "2026-11-25T10:00:00.000Z";

/// 样本：一个知识库、两份材料（一份指回调查）、一段对话（问了一句、存了回答）、一个改过的内置角色、一个新建的角色
pub(crate) fn sample(c: &Connection) {
    let kb = kbs::kb_create(c, "读《国富论》", "斯密 1776", NOW).unwrap();
    kbs::material_add(c, &Material { kb: kb.clone(), kind: "excerpt".into(), title: "Stigler 1951 的摘抄".into(), body: "行业年轻时，企业什么都自己做；分工受市场范围限制。".into(), stage: 1, ..Default::default() }, NOW).unwrap();
    let conv = store::create(c, "斯密为什么说分工受市场范围限制？", "companion", std::slice::from_ref(&kb), None, None, NOW).unwrap();
    store::add_message(c, &conv, "you", &json!({ "text": "斯密为什么说分工受市场范围限制？" }), NOW).unwrap();
    let reply = json!({ "steps": [{ "agent": "think", "text": "想了想" }, { "agent": "write", "text": "写好了" }], "paragraphs": ["因为分工靠交换。"], "cites": [], "transcript": [{ "role": "assistant", "parts": [{ "type": "text", "text": "因为分工靠交换。" }] }] });
    store::add_message(c, &conv, "companion", &reply, NOW).unwrap();
    kbs::material_add(c, &Material { kb, kind: "answer".into(), title: "分工受市场范围限制".into(), body: "演示".into(), conversation: Some(conv), ..Default::default() }, NOW).unwrap();
    store::role_save(c, &Role { name: "审稿人".into(), mark: "审".into(), line: "只挑最要紧的".into(), ..store::role_get(c, "reviewer").unwrap() }, NOW).unwrap();
    store::role_save(c, &Role { uid: String::new(), name: "导师".into(), mark: String::new(), line: "看大方向".into(), prompt: String::new(), kb: String::new(), model: String::new(), skills: vec![], builtin: false, edited: false }, NOW).unwrap();
}

#[test]
fn roles_builtin_edit_reset_and_custom() {
    let c = memory_db();
    let names: Vec<String> = store::roles_list(&c).unwrap().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["Pith", "伴读", "审稿人", "编辑"]);

    let edited = store::role_save(&c, &Role { line: "说话再短一点".into(), ..store::role_get(&c, "reviewer").unwrap() }, NOW).unwrap();
    assert!(edited.builtin && edited.edited);
    assert_eq!(store::roles_list(&c).unwrap().len(), 4, "改内置的不多出一个");

    let mine = store::role_save(&c, &Role { uid: String::new(), name: "导师".into(), mark: String::new(), line: String::new(), prompt: String::new(), kb: String::new(), model: String::new(), skills: vec!["事实调查".into()], builtin: false, edited: false }, NOW).unwrap();
    assert_eq!(mine.mark, "导", "没给头像字就用名字的第一个字");
    assert!(!mine.builtin);
    assert_eq!(store::roles_list(&c).unwrap().last().unwrap().uid, mine.uid);
    assert!(store::role_save(&c, &Role { name: " ".into(), ..mine.clone() }, NOW).is_err());

    store::role_delete(&c, "reviewer").unwrap();
    let back = store::role_get(&c, "reviewer").unwrap();
    assert!(!back.edited && back.line == "专挑刺", "内置的删掉就是改回默认");
    store::role_delete(&c, &mine.uid).unwrap();
    assert!(store::role_get(&c, &mine.uid).is_err());
}

#[test]
fn history_replays_what_was_sent_and_the_transcripts() {
    use crate::gateway::{Message, Part, Role as Who};
    let c = memory_db();
    let kb = kbs::kb_create(&c, "读《国富论》", "", NOW).unwrap();
    let uid = store::create(&c, "分工", "pith", std::slice::from_ref(&kb), None, None, NOW).unwrap();
    let conv = store::get(&c, &uid).unwrap();
    // 站主这一句：给人看的和发给模型的分开存；@ 请了别的角色就在开头说
    let you = run::you_body(&c, &conv, "分工受什么限制？", "reviewer", "2026-11-26").unwrap();
    assert_eq!(you["text"], "分工受什么限制？");
    let sent = you["sent"].as_str().unwrap();
    assert!(sent.starts_with("（今天是 2026-11-26；这一问勾了知识库：读《国富论》。）") && sent.contains("「审稿人」") && sent.ends_with("分工受什么限制？"));
    store::add_message(&c, &uid, "you", &you, NOW).unwrap();
    // Pith 的一回：问了一张问题卡（停在工具结果上）
    let turns = vec![
        Message { role: Who::Assistant, parts: vec![Part::ToolCall { call_id: "c1".into(), name: "ask_user".into(), input: json!({ "question": "哪一个？" }), signature: None }] },
        Message { role: Who::User, parts: vec![Part::ToolResult { call_id: "c1".into(), output: "问题卡已经给站主了。".into(), is_error: false }] },
    ];
    store::add_message(&c, &uid, "reviewer", &json!({ "paragraphs": [], "transcript": turns }), NOW).unwrap();
    // 1.0.x 的演示回答不进
    store::add_message(&c, &uid, "pith", &json!({ "mock": true, "paragraphs": ["演示"] }), NOW).unwrap();
    store::add_message(&c, &uid, "you", &json!({ "text": "斯密", "sent": "（今天是 2026-11-26。）\n\n斯密" }), NOW).unwrap();
    let h = run::history(&store::get(&c, &uid).unwrap());
    assert_eq!(h.len(), 3);
    assert_eq!(h[0].text(), sent);
    // 回答问题卡的那一句并进工具结果那一条（几家都认）
    assert_eq!(h[2].role, Who::User);
    assert!(matches!(&h[2].parts[0], Part::ToolResult { .. }));
    assert_eq!(h[2].text(), "（今天是 2026-11-26。）\n\n斯密");

    // 给界面的：去掉只给模型看的
    let ui = run::for_ui(store::get(&c, &uid).unwrap(), true);
    assert!(ui.running);
    assert!(ui.messages.iter().all(|m| m.body.get("sent").is_none() && m.body.get("transcript").is_none()));
}

/// 文献库查询给 Agent 用（测试里直接连一个库）
struct DbEnv(std::sync::Mutex<Connection>);

impl crate::harness::tools::Env for DbEnv {
    fn library_search(&self, query: &str, limit: usize) -> Result<Vec<crate::harness::tools::LibHit>, String> {
        run::search_library(&self.0.lock().unwrap(), query, limit)
    }
    fn library_item(&self, id: i64) -> Result<String, String> {
        run::describe_item(&self.0.lock().unwrap(), id)
    }
    fn kb_search(&self, query: &str, limit: usize) -> Result<crate::harness::tools::KbFound, String> {
        let local = crate::local::LocalModels::new(crate::local::Mode::Off, std::path::PathBuf::from("/nonexistent"));
        run::kb_search_in(&self.0, &local, &[], query, limit, || {})
    }
    fn kb_read(&self, req: &crate::harness::tools::KbRead) -> Result<String, String> {
        crate::kb::read::read(&self.0.lock().unwrap(), req)
    }
    fn kb_chapter(&self, passage: i64) -> Result<(String, String), String> {
        crate::kb::read::chapter_for_subagent(&self.0.lock().unwrap(), passage, false)
    }
    fn web_search(&self, _query: &str, _limit: usize) -> Result<(String, usize), String> {
        Err("测试里不联网".into())
    }
    fn web_fetch(&self, _url: &str, _from: Option<i64>) -> Result<(String, String), String> {
        Err("测试里不联网".into())
    }
}

#[test]
fn two_turns_with_the_dev_brain_replay_cleanly() {
    use crate::gateway::{catalog, meter::Meter, mock::MockModel};
    use crate::harness::{self, Ctx, Ending, Limits, prompt, tools::Toolbox};
    let c = memory_db();
    let item = crate::edit::item_create(&c, "book", None).unwrap();
    crate::edit::items_set_field(&c, &[item], "title", "An Inquiry into the Nature and Causes of the Wealth of Nations").unwrap();
    crate::search::rebuild(&c).ok();
    let kb = kbs::kb_create(&c, "读书", "", NOW).unwrap();
    kbs::material_add(&c, &Material { kb, kind: "note".into(), title: "读书笔记".into(), body: "Wealth of Nations 讲什么：分工、交换和市场。".into(), ..Default::default() }, NOW).unwrap();
    let uid = store::create(&c, "分工", "pith", &[], None, None, NOW).unwrap();
    let env = DbEnv(std::sync::Mutex::new(c));
    let spec = catalog::model("anthropic/claude-sonnet-5-5").unwrap();
    let model = MockModel::with_brain(spec, harness::brain::brain());
    let mut meter = Meter::new(&crate::db::tests::temp_dir("chat-two-turns"), 30.0, Default::default());
    meter.now = || NOW.into();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let sink = |_e: harness::Event| {};
    let role = store::role_get(&env.0.lock().unwrap(), "pith").unwrap();
    let mut histories = Vec::new();
    let mut turns = Vec::new();
    for (n, q) in ["Wealth of Nations 讲什么？", "(120-100)/100 是多少"].iter().enumerate() {
        {
            let c = env.0.lock().unwrap();
            let conv = store::get(&c, &uid).unwrap();
            let you = run::you_body(&c, &conv, q, "pith", "2026-11-26").unwrap();
            store::add_message(&c, &uid, "you", &you, NOW).unwrap();
        }
        let conv = store::get(&env.0.lock().unwrap(), &uid).unwrap();
        let history = run::history(&conv);
        histories.push(history.clone());
        let ctx = Ctx { model: &model, effort: Some("medium"), strong: None, meter: &meter, conversation: Some(&uid), env: &env, cancel: &cancel, sink: &sink, limits: Limits::default() };
        let o = harness::run(&ctx, &prompt::system(&role), &Toolbox::main(), history, "chat");
        assert_eq!(o.ending, Ending::Done, "第 {} 句", n + 1);
        assert!(o.text.contains("模拟回答"));
        assert!(o.delivery.is_some());
        turns.push(o.turns.clone());
        let cites = crate::kb::cite::cites(&env.0.lock().unwrap(), &crate::kb::cite::handles(&o.text)).unwrap();
        let body = run::reply_body(&o, spec.label, None, &cites);
        store::add_message(&env.0.lock().unwrap(), &uid, "pith", &body, NOW).unwrap();
        assert!(o.steps.iter().any(|s| s.agent == "search" && s.text.starts_with("在知识库里找")), "{:?}", o.steps);
        if n == 0 {
            // 真的查了库里的那一条；知识库里找到的那一段换成了 [1]、页边注是它
            assert!(o.steps.iter().any(|s| s.agent == "lib" && s.text.contains("1 条")), "{:?}", o.steps);
            assert_eq!(cites.len(), 1, "{}", o.text);
            assert!(cites[0].text.contains("Wealth of Nations") && cites[0].kind == "笔记");
            assert!(body["paragraphs"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("[1]")));
            assert!(body["delivery"]["body"].as_str().unwrap().contains("出处：\n[1] 《读书笔记》"));
        } else {
            assert!(o.steps.iter().any(|s| s.agent == "compute" && s.text.contains("= 0.2")), "{:?}", o.steps);
        }
    }
    // 第二句发出去时：第一句（原样）+ 第一回的全部（原样）+ 第二句，只往后追加
    let mut expected = histories[0].clone();
    expected.extend(turns[0].clone());
    assert_eq!(histories[1][..expected.len()], expected[..]);
    assert_eq!(histories[1].len(), expected.len() + 1);
    assert!(histories[1].last().unwrap().text().ends_with("(120-100)/100 是多少"));
}

#[test]
fn reply_body_has_paragraphs_cards_and_the_transcript() {
    use crate::harness::{AskCard, Delivery, Ending, Outcome, Step};
    let o = Outcome {
        text: "第一段。\n\n  第二段 **要点**。\n\n".into(),
        steps: vec![Step::new("think", "想了想")],
        ask: Some(AskCard { question: "先看哪一个？".into(), options: vec!["斯密".into()] }),
        delivery: Some(Delivery { kind: "concept".into(), title: "分工".into(), desc: "一句话".into(), body: String::new() }),
        ending: Ending::Asked,
        cost_usd: 0.012,
        ..Default::default()
    };
    let b = run::reply_body(&o, "Claude Sonnet 5.5", Some(("kb1", "读《国富论》")), &[]);
    assert_eq!(b["paragraphs"], json!(["第一段。", "第二段 **要点**。"]));
    assert_eq!(b["ask"]["options"], json!(["斯密"]));
    assert_eq!(b["delivery"]["meta"], "概念卡 • Claude Sonnet 5.5");
    assert_eq!(b["delivery"]["kb"]["name"], "读《国富论》");
    assert_eq!(b["ending"], "asked");
    assert!(b["transcript"].is_array());
}

#[test]
fn conversation_messages_stage_and_link() {
    let c = memory_db();
    let uid = store::create(&c, "  一个很长很长的问题，  超过四十个字的话标题要截短，不然对话历史里放不下，所以要截短一点才好看  ", "pith", &[], None, None, NOW).unwrap();
    let conv = store::get(&c, &uid).unwrap();
    assert!(conv.title.ends_with("……") && conv.title.chars().count() == 42);
    for i in 0..6 {
        store::add_message(&c, &uid, "you", &json!({ "text": i }), &format!("2026-11-25T10:00:0{i}.000Z")).unwrap();
    }
    let conv = store::get(&c, &uid).unwrap();
    assert_eq!(conv.messages.iter().map(|m| m.position).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    assert_eq!(conv.stage, 1, "照料多了就长");
    assert_eq!(conv.updated_at, "2026-11-25T10:00:05.000Z");

    store::link(&c, &uid, "research", "RUN1", NOW).unwrap();
    assert_eq!(store::by_ref(&c, "research", "RUN1").unwrap(), Some(uid.clone()));
    assert_eq!(store::ensure_for_ref(&c, "research", "RUN1", "x", NOW).unwrap(), uid);
    store::rename(&c, &uid, "改个名").unwrap();
    assert_eq!(store::get(&c, &uid).unwrap().title, "改个名");
    store::delete(&c, &uid).unwrap();
    assert!(store::get(&c, &uid).is_err());
    assert!(store::link(&c, &uid, "research", "RUN1", NOW).is_err());
}

#[test]
fn history_lists_old_runs_until_they_get_a_conversation() {
    let dir = crate::db::tests::temp_dir("chat-history");
    let c = memory_db();
    let q = crate::research::store::question_create(&c, research_engine::fixture::QUESTION, "2026-11-12T08:59:00.000Z").unwrap();
    let first = crate::research::store::tests::investigate(&c, q, "01JRESEARCHRUN000000000001", 1, None);
    crate::research::store::tests::investigate(&c, q, "01JRESEARCHRUN000000000002", 2, Some(&first));
    let item = crate::edit::item_create(&c, "book", None).unwrap();
    crate::concept::tests::sample(&c, &dir, item);

    let list = store::list(&c).unwrap();
    let research: Vec<_> = list.iter().filter(|s| s.skill.as_deref() == Some("research")).collect();
    assert_eq!(research.len(), 1, "一个问题一条");
    assert_eq!(research[0].ref_uid.as_deref(), Some("01JRESEARCHRUN000000000002"), "连最新的那次");
    assert!(research[0].uid.is_none());
    let concept = list.iter().filter(|s| s.skill.as_deref() == Some("concept")).count();
    assert!(concept >= 1);

    let uid = store::ensure_for_ref(&c, "research", "01JRESEARCHRUN000000000002", &research[0].title, NOW).unwrap();
    let list = store::list(&c).unwrap();
    let research: Vec<_> = list.iter().filter(|s| s.skill.as_deref() == Some("research")).collect();
    assert_eq!(research.len(), 1, "有了对话就不再重复列");
    assert_eq!(research[0].uid.as_deref(), Some(uid.as_str()));

    // 能种进知识库的：两份文本、做完的调查、研究的卡片
    let cand = kbs::candidates(&c).unwrap();
    assert_eq!(cand.iter().filter(|m| m.kind == "full").count(), 2);
    assert_eq!(cand.iter().filter(|m| m.kind == "report").count(), 1);
    assert!(cand.iter().any(|m| m.kind == "concept" && m.source_ref.as_deref().is_some_and(|r| r.contains('#'))));
}

#[test]
fn sample_round_trips() {
    let c = memory_db();
    sample(&c);
    assert_eq!(store::list(&c).unwrap().len(), 1);
    assert_eq!(kbs::materials(&c, None).unwrap().len(), 2);
    assert_eq!(store::roles_list(&c).unwrap().len(), 5);
}
