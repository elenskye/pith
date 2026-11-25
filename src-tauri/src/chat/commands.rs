//! 对话、角色、知识库的 Tauri 命令。规则都在 `store`、`mock` 里，这里只取连接、记时间。

use serde_json::{Value, json};
use tauri::State;

use super::store::{self, ConvSummary, Conversation, KnowledgeBase, Material, Role};
use super::mock;
use crate::AppState;
use crate::common::Tx;
use crate::text::now_iso;

/// 你说一句、Pith（或点名的角色）回一句：都在一个事务里
fn exchange(c: &rusqlite::Connection, uid: &str, text: &str, role_uid: &str) -> Result<(), String> {
    let conv = store::get(c, uid)?;
    let role = store::role_get(c, role_uid).or_else(|_| store::role_get(c, "pith"))?;
    let now = now_iso();
    store::add_message(c, uid, "you", &json!({ "text": text, "role": role.uid }), &now)?;
    let reply: Value = mock::answer(c, text, &role, &conv.kbs)?;
    store::add_message(c, uid, &role.uid, &reply, &now)
}

#[tauri::command]
pub fn chat_list(state: State<AppState>) -> Result<Vec<ConvSummary>, String> {
    state.with_db(|c| store::list(c))
}

#[tauri::command]
pub fn chat_get(state: State<AppState>, uid: String) -> Result<Conversation, String> {
    state.with_db(|c| store::get(c, &uid))
}

/// 开始一段对话。skill 是空的：Pith 直接回答（模拟）；点名了技能：先建对话，技能跑起来以后 `chat_link`
#[tauri::command]
pub fn chat_start(state: State<AppState>, text: String, role: String, kbs: Vec<String>, skill: Option<String>) -> Result<Conversation, String> {
    state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let uid = store::create(c, &text, &role, &kbs, skill.as_deref(), None, &now_iso())?;
        match skill {
            None => exchange(c, &uid, &text, &role)?,
            // 点名了技能：留下你说的那句（技能拿它当问题），回答由技能给
            Some(_) => store::add_message(c, &uid, "you", &json!({ "text": text, "role": role }), &now_iso())?,
        }
        tx.commit()?;
        store::get(c, &uid)
    })
}

/// 接着问。role 是这一句点名的角色（@），空的就用对话的角色
#[tauri::command]
pub fn chat_send(state: State<AppState>, uid: String, text: String, role: Option<String>) -> Result<Conversation, String> {
    state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let role = match role {
            Some(r) => r,
            None => store::get(c, &uid)?.role,
        };
        exchange(c, &uid, &text, &role)?;
        tx.commit()?;
        store::get(c, &uid)
    })
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
