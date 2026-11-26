//! 知识库的 Tauri 命令（`kb_*`）。规则在 `store`、`passages`、`read`、`index` 里，这里只取连接、记时间、种进来以后开始补向量。

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::index::{self, IndexStatus, Indexer};
use super::read::{self, View};
use super::store::{self, KnowledgeBase, Material};
use crate::AppState;
use crate::common::Tx;
use crate::text::now_iso;

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
    state.with_db(|c| {
        let tx = Tx::begin(c)?;
        store::kb_delete(c, &uid)?;
        tx.commit()
    })
}

#[tauri::command]
pub fn kb_materials(state: State<AppState>, kb: Option<String>) -> Result<Vec<Material>, String> {
    state.with_db(|c| store::materials(c, kb.as_deref()))
}

/// 种进知识库。kb 是空的：种进第一个知识库，没有就建一个「随手存」。种好在后台补向量
#[tauri::command(async)]
pub fn kb_material_add(app: AppHandle, state: State<AppState>, material: Material) -> Result<Material, String> {
    let saved = state.with_db(|c| {
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
    })?;
    index::start(&app);
    Ok(saved)
}

#[tauri::command(async)]
pub fn kb_material_update(app: AppHandle, state: State<AppState>, uid: String, title: String, body: String, stage: i64) -> Result<Material, String> {
    let m = state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let m = store::material_update(c, &uid, &title, &body, stage, &now_iso())?;
        tx.commit()?;
        Ok(m)
    })?;
    index::start(&app);
    Ok(m)
}

/// 从一个库里拿掉（kb 空着：哪个库都不要了）。哪个库都没有它了，这份材料就删掉
#[tauri::command]
pub fn kb_material_delete(state: State<AppState>, uid: String, kb: Option<String>) -> Result<(), String> {
    state.with_db(|c| {
        let tx = Tx::begin(c)?;
        store::material_remove(c, &uid, kb.as_deref())?;
        tx.commit()
    })
}

/// 再放进一个库
#[tauri::command]
pub fn kb_material_link(state: State<AppState>, uid: String, kb: String) -> Result<Material, String> {
    state.with_db(|c| store::material_link(c, &uid, &kb, &now_iso()))
}

/// 对话里取回的一页存进知识库（页边注的「存进知识库」）。kb 是空的：种进第一个知识库，没有就建「随手存」
#[tauri::command(async)]
pub fn kb_save_web(app: AppHandle, state: State<AppState>, page: i64, kb: Option<String>, conversation: Option<String>) -> Result<Material, String> {
    let saved = state.with_db(|c| {
        let tx = Tx::begin(c)?;
        let now = now_iso();
        // 没给库：这段对话勾的第一个库；也没勾就第一个库，没有就建「随手存」
        let picked = conversation.as_deref().and_then(|u| crate::chat::store::get(c, u).ok()).and_then(|conv| conv.kbs.into_iter().find(|k| store::kb_id(c, k).is_ok()));
        let kb = match kb.filter(|k| !k.is_empty()).or(picked) {
            Some(k) => k,
            None => match store::kbs_list(c)?.first() {
                Some(k) => k.uid.clone(),
                None => store::kb_create(c, "随手存", "对话里存下的回答和随手种进来的东西", &now)?,
            },
        };
        let m = store::save_web(c, page, &kb, conversation.as_deref(), &now)?;
        tx.commit()?;
        Ok(m)
    })?;
    index::start(&app);
    Ok(saved)
}

#[tauri::command]
pub fn kb_candidates(state: State<AppState>) -> Result<Vec<Material>, String> {
    state.with_db(|c| store::candidates(c))
}

/// 面板里读一份材料的原文：给了段落读它那一章，给了章（目录下标）读那一章
#[tauri::command(async)]
pub fn kb_read(state: State<AppState>, material: String, unit: Option<usize>, passage: Option<i64>) -> Result<View, String> {
    state.with_db(|c| {
        let id = store::material_id(c, &material)?;
        read::view(c, id, unit, passage)
    })
}

#[tauri::command(async)]
pub fn kb_index_status(state: State<AppState>, ix: State<Arc<Indexer>>) -> Result<IndexStatus, String> {
    index::status(&state, &state.local, &ix)
}

/// 在后台补齐向量（设置页的「建索引」）
#[tauri::command(async)]
pub fn kb_index_start(app: AppHandle, state: State<AppState>, ix: State<Arc<Indexer>>) -> Result<IndexStatus, String> {
    index::start(&app);
    index::status(&state, &state.local, &ix)
}
