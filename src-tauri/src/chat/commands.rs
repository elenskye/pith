//! 对话、角色、知识库的 Tauri 命令。规则都在 `store`、`run` 里，这里只取连接、记时间。
//! 1.1.0 起回答由 Agent 循环在后台给（`run`）：`chat_start` / `chat_send` 存下站主这一句就返回，界面轮询 `chat_live`。

use tauri::{AppHandle, State};

use super::run::{self, LiveView, Runs};
use super::store::{self, ConvSummary, Conversation, KnowledgeBase, Material, Role};
use crate::AppState;
use crate::common::Tx;
use crate::text::now_iso;

fn today() -> String {
    now_iso().get(..10).unwrap_or("").to_string()
}

/// 存下站主这一句（带发给模型的样子）
fn say(c: &rusqlite::Connection, uid: &str, text: &str, role: &str) -> Result<(), String> {
    let conv = store::get(c, uid)?;
    let body = run::you_body(c, &conv, text, role, &today())?;
    store::add_message(c, uid, "you", &body, &now_iso())
}

#[tauri::command]
pub fn chat_list(state: State<AppState>) -> Result<Vec<ConvSummary>, String> {
    state.with_db(|c| store::list(c))
}

#[tauri::command]
pub fn chat_get(state: State<AppState>, runs: State<Runs>, uid: String) -> Result<Conversation, String> {
    let conv = state.with_db(|c| store::get(c, &uid))?;
    Ok(run::for_ui(conv, runs.running(&uid)))
}

/// 开始一段对话。skill 是空的：Pith 在后台回答；点名了技能：先建对话，技能跑起来以后 `chat_link`
#[tauri::command]
pub fn chat_start(app: AppHandle, state: State<AppState>, runs: State<Runs>, text: String, role: String, kbs: Vec<String>, skill: Option<String>) -> Result<Conversation, String> {
    let uid = state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let uid = store::create(c, &text, &role, &kbs, skill.as_deref(), None, &now_iso())?;
        say(c, &uid, &text, &role)?;
        tx.commit()?;
        Ok(uid)
    })?;
    if skill.is_none() {
        run::start(&app, &uid, &role)?;
    }
    let conv = state.with_db(|c| store::get(c, &uid))?;
    Ok(run::for_ui(conv, runs.running(&uid)))
}

/// 接着问。role 是这一句点名的角色（@），空的就用对话的角色
#[tauri::command]
pub fn chat_send(app: AppHandle, state: State<AppState>, runs: State<Runs>, uid: String, text: String, role: Option<String>) -> Result<Conversation, String> {
    if runs.running(&uid) {
        return Err("上一句还在回答。等它答完，或者先点停。".into());
    }
    let role = state.with_db(|c| {
        let role = match role {
            Some(r) => r,
            None => store::get(c, &uid)?.role,
        };
        say(c, &uid, &text, &role)?;
        Ok(role)
    })?;
    run::start(&app, &uid, &role)?;
    let conv = state.with_db(|c| store::get(c, &uid))?;
    Ok(run::for_ui(conv, runs.running(&uid)))
}

/// 正在回答的那一回到哪了（没在回答是 null）
#[tauri::command]
pub fn chat_live(runs: State<Runs>, uid: String) -> Option<LiveView> {
    runs.view(&uid)
}

/// 停下正在回答的那一回（已经写出来的照样存下）
#[tauri::command]
pub fn chat_stop(runs: State<Runs>, uid: String) {
    runs.stop(&uid);
}

/// 以前的调查、研究：接着问之前建一段对话连上它，返回对话的 uid
#[tauri::command]
pub fn chat_open_ref(state: State<AppState>, skill: String, ref_uid: String, title: String) -> Result<String, String> {
    state.with_db(|c| store::ensure_for_ref(c, &skill, &ref_uid, &title, &now_iso()))
}

#[tauri::command]
pub fn chat_link(state: State<AppState>, uid: String, skill: String, ref_uid: String) -> Result<(), String> {
    state.with_db(|c| store::link(c, &uid, &skill, &ref_uid, &now_iso()))
}

#[tauri::command]
pub fn chat_rename(state: State<AppState>, uid: String, title: String) -> Result<(), String> {
    state.with_db(|c| store::rename(c, &uid, &title))
}

#[tauri::command]
pub fn chat_delete(state: State<AppState>, uid: String) -> Result<(), String> {
    state.with_db(|c| store::delete(c, &uid))
}

#[tauri::command]
pub fn role_list(state: State<AppState>) -> Result<Vec<Role>, String> {
    state.with_db(|c| store::roles_list(c))
}

#[tauri::command]
pub fn role_save(state: State<AppState>, role: Role) -> Result<Role, String> {
    state.with_db(|c| store::role_save(c, &role, &now_iso()))
}

#[tauri::command]
pub fn role_delete(state: State<AppState>, uid: String) -> Result<(), String> {
    state.with_db(|c| store::role_delete(c, &uid))
}

#[tauri::command]
pub fn kb_list(state: State<AppState>) -> Result<Vec<KnowledgeBase>, String> {
    state.with_db(|c| store::kbs_list(c))
}

#[tauri::command]
pub fn kb_create(state: State<AppState>, name: String, note: String) -> Result<String, String> {
    state.with_db(|c| store::kb_create(c, &name, &note, &now_iso()))
}

#[tauri::command]
pub fn kb_update(state: State<AppState>, uid: String, name: String, note: String) -> Result<(), String> {
    state.with_db(|c| store::kb_update(c, &uid, &name, &note, &now_iso()))
}

#[tauri::command]
pub fn kb_delete(state: State<AppState>, uid: String) -> Result<(), String> {
    state.with_db(|c| store::kb_delete(c, &uid))
}

#[tauri::command]
pub fn kb_materials(state: State<AppState>, kb: Option<String>) -> Result<Vec<Material>, String> {
    state.with_db(|c| store::materials(c, kb.as_deref()))
}

/// 种进知识库。kb 是空的：种进第一个知识库，没有就建一个「随手存」
#[tauri::command]
pub fn kb_material_add(state: State<AppState>, material: Material) -> Result<Material, String> {
    state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let now = now_iso();
        let mut m = material;
        if m.kb.is_empty() {
            m.kb = match store::kbs_list(c)?.first() {
                Some(k) => k.uid.clone(),
                None => store::kb_create(c, "随手存", "对话里存下的回答和随手种进来的东西", &now)?,
            };
        }
        let saved = store::material_add(c, &m, &now)?;
        tx.commit()?;
        Ok(saved)
    })
}

#[tauri::command]
pub fn kb_material_update(state: State<AppState>, uid: String, title: String, body: String, stage: i64) -> Result<Material, String> {
    state.with_db(|c| store::material_update(c, &uid, &title, &body, stage, &now_iso()))
}

#[tauri::command]
pub fn kb_material_delete(state: State<AppState>, uid: String) -> Result<(), String> {
    state.with_db(|c| store::material_delete(c, &uid))
}

#[tauri::command]
pub fn kb_candidates(state: State<AppState>) -> Result<Vec<Material>, String> {
    state.with_db(|c| store::candidates(c))
}
