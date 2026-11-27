//! 对话的一回（1.1.0 第 2 期）：站主说一句 → 后台线程跑 Agent 循环（`harness`）→ 存成一条消息。
//!
//! - 跑的时候界面每 0.2 秒问一次 `chat_live`：状态行到哪一格、正文写到哪了；点停是 `chat_stop`。
//! - 给模型看的对话由存下的消息拼出来：站主那一条用当时发出去的样子（`sent`），Pith 那一条用存下的 `transcript`，
//!   原样重放、只往后追加，缓存才命中。演示的回答（1.0.x 的 `mock: true`）不放进去。
//! - 模型：设置里选的对话模型和子代理模型，思考照设置里每个模型定的（`decisions/0041`）。开发版、测试用模拟模型（`harness::brain`）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use super::store::{self, Conversation, Role};
use crate::AppState;
use crate::gateway::catalog::Pick;
use crate::gateway::http::{Credential, HttpModel};
use crate::gateway::meter::Meter;
use crate::gateway::mock::MockModel;
use crate::gateway::{Message, Model, Part, Provider, Role as Who, keys};
use crate::harness::tools::{Env, KbFound, KbRead, LibHit, Toolbox};
use crate::kb::cite::{self, Cite};
use crate::harness::{self, Ctx, Ending, Event, Limits, Outcome, Step, prompt};

/// 正在回答的那一回（界面轮询看到的样子）
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveView {
    pub run: String,
    /// 谁在答（角色 uid）
    pub author: String,
    pub steps: Vec<Step>,
    /// 正在写的正文
    pub text: String,
    pub started_at: String,
}

struct Live {
    view: Arc<Mutex<LiveView>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Runs {
    live: Mutex<HashMap<String, Live>>,
}

impl Runs {
    pub fn running(&self, conv: &str) -> bool {
        self.live.lock().map(|l| l.contains_key(conv)).unwrap_or(false)
    }

    /// 正文里的段落编号（[P123]）已经换成 [1]
    pub fn view(&self, conv: &str) -> Option<LiveView> {
        let mut v = self.live.lock().ok()?.get(conv).and_then(|l| l.view.lock().ok().map(|v| v.clone()))?;
        v.text = cite::number_live(&v.text);
        Some(v)
    }

    pub fn stop(&self, conv: &str) {
        if let Ok(l) = self.live.lock()
            && let Some(r) = l.get(conv)
        {
            r.cancel.store(true, Ordering::SeqCst);
        }
    }
}

pub fn manage(app: &tauri::App) {
    app.manage(Runs::default());
}

// ---------- 给模型看的对话 ----------

/// 由存下的消息拼出给模型看的对话。站主连着说的几句并成一条（几家都认），开头不是站主的去掉
pub fn history(conv: &Conversation) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for m in &conv.messages {
        if m.author == "you" {
            let text = m.body.get("sent").or_else(|| m.body.get("text")).and_then(Value::as_str).unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.role == Who::User => last.parts.push(Part::Text { text: text.into() }),
                _ => out.push(Message::user(text)),
            }
        } else if let Some(turns) = m.body.get("transcript").and_then(|t| serde_json::from_value::<Vec<Message>>(t.clone()).ok()) {
            for t in turns {
                match (out.last_mut(), t.role) {
                    // 上一回停在工具结果（问题卡）、这一回又从站主开始：并成一条
                    (Some(last), Who::User) if last.role == Who::User => last.parts.extend(t.parts),
                    _ => out.push(t),
                }
            }
        } else if !m.body.get("mock").and_then(Value::as_bool).unwrap_or(false) {
            // 技能流程留下的（没有 transcript）：只留文字
            let text = m.body.get("paragraphs").and_then(Value::as_array).map(|ps| ps.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
            if !text.trim().is_empty() {
                out.push(Message { role: Who::Assistant, parts: vec![Part::Text { text }] });
            }
        }
    }
    while out.first().is_some_and(|m| m.role != Who::User) {
        out.remove(0);
    }
    out
}

/// 站主这一句存成什么样：`text` 给人看，`sent` 是发给模型的（带今天几号、勾了哪些知识库、`@` 请了谁）
pub fn you_body(c: &rusqlite::Connection, conv: &Conversation, text: &str, role: &str, today: &str) -> Result<Value, String> {
    let kbs = crate::kb::store::kbs_list(c)?;
    let names: Vec<String> = conv.kbs.iter().filter_map(|u| kbs.iter().find(|k| &k.uid == u).map(|k| k.name.clone())).collect();
    let asked = (role != conv.role).then(|| store::role_get(c, role).ok()).flatten();
    Ok(json!({ "text": text, "role": role, "sent": prompt::user_text(text, today, &names, asked.as_ref()) }))
}

// ---------- 存下的回答 ----------

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "note" => "笔记",
        "concept" => "概念卡",
        "excerpt" => "摘抄",
        _ => "存下的回答",
    }
}

/// 一回的结果 → 消息的 body（前端照它画：状态行、段落、页边注、问题卡、交付卡；`transcript` 只给下一回用）。
/// `cites` 是回答里的段落编号查出来的页边注（`kb::cite::cites`）：正文里的 [P123] 换成 [1]；
/// 交付卡存进知识库的正文也换好，后面附上出处
pub fn reply_body(o: &Outcome, model_label: &str, kb: Option<(&str, &str)>, cites: &[Cite]) -> Value {
    let text = cite::number(&o.text, &cite::order_of(cites));
    let paragraphs: Vec<String> = text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect();
    let delivery = o.delivery.as_ref().map(|d| {
        let body = if d.body.trim().is_empty() { &o.text } else { &d.body };
        json!({
            "kind": d.kind,
            "title": d.title,
            "desc": d.desc,
            "meta": format!("{} • {model_label}", kind_label(&d.kind)),
            "kb": kb.map(|(uid, name)| json!({ "uid": uid, "name": name })),
            "body": cite::with_sources(body, cites),
        })
    });
    json!({
        "steps": o.steps,
        "paragraphs": paragraphs,
        "cites": cites,
        "ask": o.ask,
        "delivery": delivery,
        "model": model_label,
        "costUsd": o.cost_usd,
        "tokens": o.tokens,
        "ending": o.ending,
        "note": o.note,
        "transcript": o.turns,
    })
}

/// 给前端的对话：去掉只给模型看的（`sent`、`transcript`）
pub fn for_ui(mut conv: Conversation, running: bool) -> Conversation {
    for m in &mut conv.messages {
        if let Some(o) = m.body.as_object_mut() {
            o.remove("sent");
            o.remove("transcript");
        }
    }
    conv.running = running;
    conv
}

// ---------- App 里的工具环境 ----------

/// 文献库里搜条目（只要条目，不要独立附件、笔记）
pub fn search_library(c: &rusqlite::Connection, query: &str, limit: usize) -> Result<Vec<LibHit>, String> {
    let mut q = crate::library::ListQuery::of(crate::library::Scope::All);
    q.q = query.trim().to_string();
    Ok(crate::library::items_list(c, &q)?
        .into_iter()
        .filter(|r| r.kind == "item")
        .take(limit)
        .map(|r| LibHit { id: r.id, title: r.title.unwrap_or_default(), creator: r.first_creator.map(|c| if r.creator_count > 1 { format!("{c} 等") } else { c }).unwrap_or_default(), year: r.year, container: r.container_title.unwrap_or_default(), item_type: r.item_type.unwrap_or_default() })
        .collect())
}

/// 一个条目的详情，写成给模型看的几行
pub fn describe_item(c: &rusqlite::Connection, id: i64) -> Result<String, String> {
    let d = crate::library::item_get(c, id).map_err(|_| format!("文献库里没有条目 {id}。"))?;
    let mut lines = vec![format!("[条目 {}] {}", d.id, d.title.clone().unwrap_or_default()), format!("类型：{}", d.item_type)];
    if !d.creators.is_empty() {
        lines.push(format!("作者：{}", d.creators.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join("；")));
    }
    for (k, v) in [("日期", &d.date), ("出处", &d.container_title), ("DOI", &d.doi), ("ISBN", &d.isbn), ("语言", &d.language)] {
        if let Some(v) = v.as_ref().filter(|v| !v.trim().is_empty()) {
            lines.push(format!("{k}：{v}"));
        }
    }
    if !d.tags.is_empty() {
        lines.push(format!("标签：{}", d.tags.join("、")));
    }
    lines.push(format!("附件 {} 个，笔记 {} 条", d.attachments.len(), d.notes.len()));
    if let Some(a) = d.abstract_text.as_ref().filter(|a| !a.trim().is_empty()) {
        lines.push(format!("摘要：{}", a.chars().take(3000).collect::<String>()));
    }
    if d.deleted_at.is_some() {
        lines.push("（这一条在回收站里）".into());
    }
    Ok(lines.join("\n"))
}

/// Agent 的工具碰到的外面：数据库、本机模型、联网（不碰 Tauri：App 和真实验收共用这一套）
pub struct ChatEnv<'a> {
    pub state: &'a AppState,
    /// 这段对话勾的知识库（空 = 全部）
    pub kbs: Vec<String>,
    pub conversation: Option<String>,
    pub cancel: &'a AtomicBool,
    /// 检索时发现缺向量：在后台补
    pub backfill: Box<dyn Fn() + Send + Sync + 'a>,
    /// 联网工具第一次用到时才造（才去读密钥）
    pub make_web: Box<dyn Fn() -> crate::web::Web + Send + Sync + 'a>,
    pub web: std::sync::OnceLock<crate::web::Web>,
}

impl ChatEnv<'_> {
    fn web(&self) -> &crate::web::Web {
        self.web.get_or_init(|| (self.make_web)())
    }
}

impl Env for ChatEnv<'_> {
    fn library_search(&self, query: &str, limit: usize) -> Result<Vec<LibHit>, String> {
        self.state.with_db(|c| search_library(c, query, limit))
    }

    fn library_item(&self, id: i64) -> Result<String, String> {
        self.state.with_db(|c| describe_item(c, id))
    }

    fn kb_search(&self, query: &str, limit: usize) -> Result<KbFound, String> {
        kb_search_in(self.state, &self.state.local, &self.kbs, query, limit, || (self.backfill)())
    }

    fn kb_read(&self, req: &KbRead) -> Result<String, String> {
        self.state.with_db(|c| crate::kb::read::read(c, req))
    }

    fn kb_chapter(&self, passage: i64) -> Result<(String, String), String> {
        self.state.with_db(|c| crate::kb::read::chapter_for_subagent(c, passage, false))
    }

    fn web_search(&self, query: &str, limit: usize) -> Result<(String, usize), String> {
        let f = crate::web::search(self.web(), query, limit, self.cancel)?;
        Ok((f.text, f.hits))
    }

    fn web_fetch(&self, url: &str, from: Option<i64>) -> Result<(String, String), String> {
        let page = match from {
            Some(b) => self.state.with_db(|c| crate::web::store::block(c, b))?.map(|(_, p)| p.id).ok_or_else(|| format!("没有编号 W{b} 的段落。"))?,
            None => crate::web::fetch(self.web(), self.state, url, self.conversation.as_deref(), self.cancel)?,
        };
        self.state.with_db(|c| Ok((crate::web::render(c, page, from)?, crate::web::store::page(c, page)?.map(|p| p.site).unwrap_or_default())))
    }
}

/// App 里造联网工具：密钥从钥匙串（正式版）或内存、文件来；Vertex 要项目填了、gcloud 登录过
fn web_kit(app: &AppHandle, provider: Provider, conversation: &str) -> crate::web::Web {
    let state: tauri::State<AppState> = app.state();
    let vertex = crate::gateway::commands::vertex_config(&state);
    let key = |k: crate::agent::secret::ApiKey| -> Option<String> { if keys::present(app, k) { keys::load(app, k).ok() } else { None } };
    crate::web::kit(&crate::web::Setup {
        root: &state.paths.root,
        real: crate::agent::use_real_model(),
        provider,
        key: &key,
        vertex: (vertex.is_set() && crate::gateway::vertex::adc_present()).then_some(vertex),
        budget_usd: crate::agent::budget(&state),
        prices: crate::research::commands::settings(&state).prices,
        conversation: Some(conversation),
    })
}

/// 在知识库里找（不碰 Tauri，测试直接调）。搜索词空着：列出有哪些材料。缺向量时 `backfill` 在后台补
pub fn kb_search_in(db: &impl crate::kb::Db, local: &crate::local::LocalModels, kbs: &[String], query: &str, limit: usize, backfill: impl FnOnce()) -> Result<KbFound, String> {
    if query.trim().is_empty() {
        let text = db.with(|c| crate::kb::retrieve::list_materials(c, kbs))?;
        return Ok(KbFound { text, hits: 0, semantic: false });
    }
    let f = crate::kb::retrieve::search(db, local, query, kbs, limit)?;
    if f.note.is_some() {
        backfill();
    }
    Ok(KbFound { text: crate::kb::retrieve::format(query, &f), hits: f.hits.len(), semantic: f.mode == "semantic" })
}

// ---------- 模型 ----------

fn build(app: &AppHandle, state: &AppState, pick: Pick) -> Result<Box<dyn Model>, String> {
    if !crate::agent::use_real_model() {
        let mut m = MockModel::with_brain(pick.model, harness::brain::brain());
        m.delay = if cfg!(test) { Duration::ZERO } else { Duration::from_millis(35) };
        return Ok(Box::new(m));
    }
    let credential = match keys::key_of(pick.model.provider) {
        Some(k) => Credential::Key(keys::load(app, k)?),
        None => Credential::Adc,
    };
    let vertex = crate::gateway::commands::vertex_config(state);
    Ok(Box::new(HttpModel::new(pick.model, credential, Some(&vertex)).map_err(|e| e.message)?))
}

/// 这一回用哪个模型：角色写了清单里的模型（`anthropic/claude-opus-5-5`）就用它，不然用设置里选的；思考照设置
fn picks(state: &AppState, role: &Role) -> (Pick, Pick) {
    let m = role.model.trim();
    crate::gateway::commands::picks(state, (!m.is_empty()).then_some(m))
}

pub fn ready(app: &AppHandle, state: &AppState, p: Provider) -> bool {
    !crate::agent::use_real_model() || crate::gateway::commands::ready(app, state, p)
}

// ---------- 跑一回 ----------

/// 在后台开始回答。`author` 是这一句请的角色（`@`），空的就是对话的角色
pub fn start(app: &AppHandle, conv: &str, author: &str) -> Result<(), String> {
    let runs: tauri::State<Runs> = app.state();
    let view = Arc::new(Mutex::new(LiveView { run: crate::common::new_uid()?, author: author.into(), started_at: crate::text::now_iso(), ..Default::default() }));
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut live = runs.live.lock().map_err(|_| "内部错误".to_string())?;
        if live.contains_key(conv) {
            return Err("上一句还在回答。等它答完，或者先点停。".into());
        }
        live.insert(conv.to_string(), Live { view: view.clone(), cancel: cancel.clone() });
    }
    let (app, conv, author) = (app.clone(), conv.to_string(), author.to_string());
    std::thread::spawn(move || {
        let body = answer(&app, &conv, &view, &cancel).unwrap_or_else(|e| json!({ "steps": [], "paragraphs": [], "ending": Ending::Error, "note": e }));
        let state: tauri::State<AppState> = app.state();
        if let Err(e) = state.with_db(|c| store::add_message(c, &conv, &author, &body, &crate::text::now_iso())) {
            eprintln!("存不进回答：{e}");
        }
        let runs: tauri::State<Runs> = app.state();
        if let Ok(mut live) = runs.live.lock() {
            live.remove(&conv);
        }
    });
    Ok(())
}

fn answer(app: &AppHandle, conv_uid: &str, view: &Arc<Mutex<LiveView>>, cancel: &AtomicBool) -> Result<Value, String> {
    let state: tauri::State<AppState> = app.state();
    let (conv, main_role, kbs) = state.with_db(|c| {
        let conv = store::get(c, conv_uid)?;
        let role = store::role_get(c, &conv.role).or_else(|_| store::role_get(c, "pith"))?;
        Ok((conv, role, crate::kb::store::kbs_list(c)?))
    })?;
    let (main, strong) = picks(&state, &main_role);
    if !ready(app, &state, main.model.provider) {
        return Err(match main.model.provider {
            Provider::Vertex => "Vertex AI 还不能用：到「设置 › 模型与费用」填项目 ID，并在终端里跑一次 gcloud auth application-default login。".into(),
            p => format!("还没有填 {} 的密钥。到「设置 › 模型与费用」填好，或者换一家。", p.label()),
        });
    }
    let model = build(app, &state, main)?;
    let strong_model = build(app, &state, strong)?;
    let mut meter = Meter::new(&state.paths.root, crate::agent::budget(&state), crate::research::commands::settings(&state).prices);
    meter.free = crate::gateway::commands::free(&state);
    let provider = main.model.provider;
    let env = ChatEnv {
        state: &state,
        kbs: conv.kbs.clone(),
        conversation: Some(conv_uid.to_string()),
        cancel,
        backfill: Box::new(|| crate::kb::index::start(app)),
        make_web: Box::new(move || web_kit(app, provider, conv_uid)),
        web: std::sync::OnceLock::new(),
    };
    let sink = |e: Event| {
        if let Ok(mut v) = view.lock() {
            match e {
                Event::Step(s) => v.steps.push(s),
                Event::Text(t) => v.text.push_str(&t),
                Event::Reset => v.text.clear(),
            }
        }
    };
    let ctx = Ctx {
        model: model.as_ref(),
        effort: main.effort,
        strong: Some((strong_model.as_ref(), strong.effort)),
        meter: &meter,
        conversation: Some(conv_uid),
        env: &env,
        cancel,
        sink: &sink,
        limits: Limits::default(),
    };
    let outcome = harness::run(&ctx, &prompt::system(&main_role), &Toolbox::main(), history(&conv), "chat");
    let kb = conv.kbs.first().and_then(|u| kbs.iter().find(|k| &k.uid == u)).map(|k| (k.uid.as_str(), k.name.as_str()));
    let cites = state.with_db(|c| cite::cites(c, &cite::handles(&outcome.text)))?;
    drop(env);
    Ok(reply_body(&outcome, main.model.label, kb, &cites))
}
