//! 知识库和材料的存取（migration 0010、0011）。
//!
//! - 一份材料可以在几个知识库里（`kb_members`）。`kb_materials.kb_id` 是它的「家」（第一次种进的库），一定也在 `kb_members` 里；
//!   从家里拿掉而别的库还有它时，家换到别的库，所以删掉一个库只删只在这个库里的材料。
//! - 同一个来源（书、调查、卡片、对话里的一回）全库只有一份材料：再种进别的库只是多一个库、照料一下。
//! - 种进来、改了正文：重新切段落块（`passages::sync`）。检索的索引和向量是派生的，在 `search.db`（`index`）。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::passages;
use crate::common::{err, new_uid};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBase {
    pub uid: String,
    pub name: String,
    pub note: String,
    pub count: i64,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    #[serde(default)]
    pub uid: String,
    /// 列一个库的材料时是那个库；别的时候是它的家
    #[serde(default)]
    pub kb: String,
    /// 它在哪几个库里（按库的顺序）
    #[serde(default)]
    pub kbs: Vec<String>,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub stage: i64,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    /// 从哪段对话存的
    #[serde(default)]
    pub conversation: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub tended_at: String,
    /// 切成了几块（书是段落数）
    #[serde(default)]
    pub passages: i64,
    #[serde(skip)]
    pub id: i64,
}

pub const KINDS: [&str; 10] = ["full", "excerpt", "note", "web", "file", "scan", "concept", "report", "wonder", "answer"];

pub(crate) fn kb_id(c: &Connection, uid: &str) -> Result<i64, String> {
    c.query_row("SELECT id FROM knowledge_bases WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "这个知识库不存在了。".to_string())
}

pub(crate) fn material_id(c: &Connection, uid: &str) -> Result<i64, String> {
    c.query_row("SELECT id FROM kb_materials WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "这份材料不存在了。".to_string())
}

pub fn kbs_list(c: &Connection) -> Result<Vec<KnowledgeBase>, String> {
    let mut st = c
        .prepare(
            "SELECT k.uid, k.name, k.note, (SELECT COUNT(*) FROM kb_members b WHERE b.kb_id = k.id), k.created_at
             FROM knowledge_bases k ORDER BY k.position, k.created_at, k.id",
        )
        .map_err(err)?;
    st.query_map([], |r| Ok(KnowledgeBase { uid: r.get(0)?, name: r.get(1)?, note: r.get(2)?, count: r.get(3)?, created_at: r.get(4)? }))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)
}

pub fn kb_create(c: &Connection, name: &str, note: &str, now: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("知识库要有名字。".into());
    }
    let uid = new_uid()?;
    c.execute(
        "INSERT INTO knowledge_bases (uid, name, note, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM knowledge_bases), ?4, ?4)",
        params![uid, name, note.trim(), now],
    )
    .map_err(err)?;
    Ok(uid)
}

pub fn kb_update(c: &Connection, uid: &str, name: &str, note: &str, now: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("知识库要有名字。".into());
    }
    let n = c.execute("UPDATE knowledge_bases SET name = ?2, note = ?3, updated_at = ?4 WHERE uid = ?1", params![uid, name, note.trim(), now]).map_err(err)?;
    if n == 0 { Err("这个知识库不存在了。".into()) } else { Ok(()) }
}

/// 删掉知识库。别的库也有的材料留着（家换过去），只在这个库里的一起删（原来的书、调查、对话不动）
pub fn kb_delete(c: &Connection, uid: &str) -> Result<(), String> {
    let Some(id) = c.query_row("SELECT id FROM knowledge_bases WHERE uid = ?1", [uid], |r| r.get::<_, i64>(0)).optional().map_err(err)? else {
        return Ok(());
    };
    rehome_from(c, id, None)?;
    c.execute("DELETE FROM knowledge_bases WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// 家在 `kb` 的材料（只看 `only` 这一份，或者全部）：别的库也有它就把家挪过去
fn rehome_from(c: &Connection, kb: i64, only: Option<i64>) -> Result<(), String> {
    c.execute(
        "UPDATE kb_materials SET kb_id = (
             SELECT b.kb_id FROM kb_members b JOIN knowledge_bases k ON k.id = b.kb_id
             WHERE b.material_id = kb_materials.id AND b.kb_id != ?1 ORDER BY k.position, k.id LIMIT 1)
         WHERE kb_id = ?1 AND (?2 IS NULL OR id = ?2)
           AND EXISTS (SELECT 1 FROM kb_members b WHERE b.material_id = kb_materials.id AND b.kb_id != ?1)",
        params![kb, only],
    )
    .map_err(err)?;
    Ok(())
}

const MATERIAL_COLUMNS: &str = "m.uid, k.uid, m.kind, m.title, m.sub, m.body, m.stage, m.source_kind, m.source_ref, c.uid, m.created_at, m.tended_at,
     (SELECT COUNT(*) FROM passages p WHERE p.material_id = m.id), m.id";

fn material_row(r: &rusqlite::Row) -> rusqlite::Result<Material> {
    Ok(Material {
        uid: r.get(0)?,
        kb: r.get(1)?,
        kbs: Vec::new(),
        kind: r.get(2)?,
        title: r.get(3)?,
        sub: r.get(4)?,
        body: r.get(5)?,
        stage: r.get(6)?,
        source_kind: r.get(7)?,
        source_ref: r.get(8)?,
        conversation: r.get(9)?,
        created_at: r.get(10)?,
        tended_at: r.get(11)?,
        passages: r.get(12)?,
        id: r.get(13)?,
    })
}

/// 每份材料在哪几个库（按库的顺序）
fn fill_kbs(c: &Connection, ms: &mut [Material]) -> Result<(), String> {
    let mut st = c.prepare("SELECT k.uid FROM kb_members b JOIN knowledge_bases k ON k.id = b.kb_id WHERE b.material_id = ?1 ORDER BY k.position, k.id").map_err(err)?;
    for m in ms {
        m.kbs = st.query_map([m.id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    }
    Ok(())
}

/// 一个知识库里的材料（kb 是空的：全部），最近照料的在前
pub fn materials(c: &Connection, kb: Option<&str>) -> Result<Vec<Material>, String> {
    let mut out = match kb {
        Some(kb) => {
            let sql = format!(
                "SELECT {MATERIAL_COLUMNS} FROM kb_members b JOIN kb_materials m ON m.id = b.material_id JOIN knowledge_bases k ON k.id = b.kb_id
                 LEFT JOIN conversations c ON c.id = m.conversation_id WHERE k.uid = ?1 ORDER BY m.tended_at DESC, m.id DESC"
            );
            let mut st = c.prepare(&sql).map_err(err)?;
            st.query_map([kb], material_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?
        }
        None => {
            let sql = format!("SELECT {MATERIAL_COLUMNS} FROM kb_materials m JOIN knowledge_bases k ON k.id = m.kb_id LEFT JOIN conversations c ON c.id = m.conversation_id ORDER BY m.tended_at DESC, m.id DESC");
            let mut st = c.prepare(&sql).map_err(err)?;
            st.query_map([], material_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?
        }
    };
    fill_kbs(c, &mut out)?;
    Ok(out)
}

pub fn material_get(c: &Connection, uid: &str) -> Result<Material, String> {
    let sql = format!("SELECT {MATERIAL_COLUMNS} FROM kb_materials m JOIN knowledge_bases k ON k.id = m.kb_id LEFT JOIN conversations c ON c.id = m.conversation_id WHERE m.uid = ?1");
    let mut m = c.query_row(&sql, [uid], material_row).optional().map_err(err)?.ok_or_else(|| "这份材料不存在了。".to_string())?;
    fill_kbs(c, std::slice::from_mut(&mut m))?;
    Ok(m)
}

/// 同一个来源的材料（全库一份）
pub fn by_source(c: &Connection, kind: &str, r: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT uid FROM kb_materials WHERE source_kind = ?1 AND source_ref = ?2 ORDER BY id LIMIT 1", params![kind, r], |r| r.get(0)).optional().map_err(err)
}

fn join(c: &Connection, kb: i64, material: i64, now: &str) -> Result<(), String> {
    c.execute("INSERT OR IGNORE INTO kb_members (kb_id, material_id, added_at) VALUES (?1, ?2, ?3)", params![kb, material, now]).map_err(err)?;
    c.execute("UPDATE knowledge_bases SET updated_at = ?2 WHERE id = ?1", params![kb, now]).map_err(err)?;
    Ok(())
}

/// 种进知识库。同一个来源全库只有一份：已经有了就把它放进这个库、照料一下，返回那一份
pub fn material_add(c: &Connection, m: &Material, now: &str) -> Result<Material, String> {
    if !KINDS.contains(&m.kind.as_str()) {
        return Err(format!("不认识的材料种类：{}", m.kind));
    }
    if m.title.trim().is_empty() {
        return Err("材料要有标题。".into());
    }
    let kb = kb_id(c, &m.kb)?;
    if let (Some(kind), Some(r)) = (&m.source_kind, &m.source_ref)
        && let Some(uid) = by_source(c, kind, r)?
    {
        let id = material_id(c, &uid)?;
        join(c, kb, id, now)?;
        c.execute("UPDATE kb_materials SET tended_at = ?2 WHERE id = ?1", params![id, now]).map_err(err)?;
        return material_get(c, &uid);
    }
    let conv = match &m.conversation {
        Some(uid) => Some(crate::chat::store::conv_id(c, uid)?),
        None => None,
    };
    let uid = new_uid()?;
    c.execute(
        "INSERT INTO kb_materials (uid, kb_id, kind, title, sub, body, stage, source_kind, source_ref, conversation_id, created_at, tended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![uid, kb, m.kind, m.title.trim(), m.sub.trim(), m.body, m.stage.clamp(0, 2), m.source_kind, m.source_ref, conv, now],
    )
    .map_err(err)?;
    let id = c.last_insert_rowid();
    join(c, kb, id, now)?;
    passages::sync(c, id)?;
    material_get(c, &uid)
}

/// 对话里取回的一页存进知识库：一份「网页快照」材料，块就是那一页的块。已经存过就放进这个库、照料一下
pub fn save_web(c: &Connection, page: i64, kb: &str, conversation: Option<&str>, now: &str) -> Result<Material, String> {
    let p = crate::web::store::page(c, page)?.ok_or("这一页不在了。")?;
    let blocks = crate::web::store::blocks(c, page)?;
    let excerpt: String = blocks.iter().filter(|b| b.role == "body").map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n").chars().take(240).collect();
    let conversation = match conversation {
        Some(uid) if crate::chat::store::conv_id(c, uid).is_ok() => Some(uid.to_string()),
        _ => None,
    };
    let m = Material {
        kb: kb.into(),
        kind: "web".into(),
        title: if p.title.trim().is_empty() { p.site.clone() } else { p.title.clone() },
        sub: format!("{} · 取于 {}", p.site, p.fetched_at.get(..10).unwrap_or("")),
        body: excerpt,
        stage: 0,
        source_kind: Some("web_page".into()),
        source_ref: Some(page.to_string()),
        conversation,
        ..Default::default()
    };
    material_add(c, &m, now)
}

/// 再放进一个库（已经在就只是照料一下）
pub fn material_link(c: &Connection, uid: &str, kb: &str, now: &str) -> Result<Material, String> {
    let (id, kb) = (material_id(c, uid)?, kb_id(c, kb)?);
    join(c, kb, id, now)?;
    c.execute("UPDATE kb_materials SET tended_at = ?2 WHERE id = ?1", params![id, now]).map_err(err)?;
    material_get(c, uid)
}

/// 改标题、正文、生长阶段；改了就算照料过。正文变了重新切段落块
pub fn material_update(c: &Connection, uid: &str, title: &str, body: &str, stage: i64, now: &str) -> Result<Material, String> {
    if title.trim().is_empty() {
        return Err("材料要有标题。".into());
    }
    let id = material_id(c, uid)?;
    let old: String = c.query_row("SELECT body FROM kb_materials WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
    c.execute("UPDATE kb_materials SET title = ?2, body = ?3, stage = ?4, tended_at = ?5 WHERE id = ?1", params![id, title.trim(), body, stage.clamp(0, 2), now]).map_err(err)?;
    if old != body {
        passages::sync(c, id)?;
    }
    material_get(c, uid)
}

/// 从一个库里拿掉（kb 是空的：哪个库都不要了）。哪个库都没有它了就删掉这份材料（原来的书、调查、对话不动）
pub fn material_remove(c: &Connection, uid: &str, kb: Option<&str>) -> Result<(), String> {
    let Some(id) = c.query_row("SELECT id FROM kb_materials WHERE uid = ?1", [uid], |r| r.get::<_, i64>(0)).optional().map_err(err)? else {
        return Ok(());
    };
    if let Some(kb) = kb {
        let kb = kb_id(c, kb)?;
        rehome_from(c, kb, Some(id))?;
        c.execute("DELETE FROM kb_members WHERE kb_id = ?1 AND material_id = ?2", params![kb, id]).map_err(err)?;
        let left: i64 = c.query_row("SELECT COUNT(*) FROM kb_members WHERE material_id = ?1", [id], |r| r.get(0)).map_err(err)?;
        if left > 0 {
            return Ok(());
        }
    }
    super::index::forget_material(c, id)?;
    c.execute("DELETE FROM kb_materials WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// 能种进知识库的东西：解析好的书（全文）、做完的调查（调查报告）、概念研究的卡片（概念卡）
pub fn candidates(c: &Connection) -> Result<Vec<Material>, String> {
    let mut out = Vec::new();
    let mut st = c.prepare("SELECT uid, title, language, meta FROM concept_texts ORDER BY created_at, id").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))).map_err(err)?;
    for row in rows {
        let (uid, title, language, meta) = row.map_err(err)?;
        let meta: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
        let by = meta.get("creators").or_else(|| meta.get("authors")).and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()).unwrap_or("");
        let lang = match language.as_str() {
            "zh" => "中文",
            "en" => "英文",
            "" => "",
            other => other,
        };
        let sub = [by, lang, "带段落号"].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        out.push(Material { kind: "full".into(), title: if title.is_empty() { "（没有书名）".into() } else { title }, sub, stage: 2, source_kind: Some("concept_text".into()), source_ref: Some(uid), ..Default::default() });
    }
    for q in crate::research::store::questions_list(c)? {
        if let Some(run) = q.runs.iter().find(|r| r.status == "done") {
            out.push(Material { kind: "report".into(), title: crate::chat::store::title_of(&q.text), sub: format!("事实调查 · 第 {} 次", run.seq), stage: 1, source_kind: Some("research_run".into()), source_ref: Some(run.uid.clone()), ..Default::default() });
        }
    }
    let mut st = c
        .prepare("SELECT r.uid, k.key, k.kind, k.title, t.title FROM concept_cards k JOIN concept_runs r ON r.id = k.run_id LEFT JOIN concept_texts t ON t.id = r.text_id WHERE r.status = 'done' ORDER BY r.created_at, k.position")
        .map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?))).map_err(err)?;
    for row in rows {
        let (run, key, kind, title, text) = row.map_err(err)?;
        let what = match kind.as_str() {
            "argument" => "论证卡",
            "comment" => "评论",
            "counterfactual" => "推演",
            "note" => "笔记",
            _ => "概念卡",
        };
        out.push(Material { kind: "concept".into(), title, sub: [what, text.as_deref().unwrap_or("")].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · "), stage: 1, source_kind: Some("concept_card".into()), source_ref: Some(format!("{run}#{key}")), ..Default::default() });
    }
    Ok(out)
}

// ---------- 1.1.0 的升级（migration 0011 之后，启动时做） ----------

const MIGRATED: &str = "kb.v11";
/// 解析好的书迁过来时放进的库
pub const SHELF: &str = "书架";

fn setting(c: &Connection, key: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(err)
}

/// 启动时：家一定在 `kb_members` 里；第一次（0011 之后）把同一来源的几份并成一份、解析好的书迁成全文材料；
/// 还没切段落块的材料切一遍。返回这次迁进来几本书
pub fn upgrade(c: &Connection, now: &str) -> Result<usize, String> {
    c.execute("INSERT OR IGNORE INTO kb_members (kb_id, material_id, added_at) SELECT kb_id, id, created_at FROM kb_materials", []).map_err(err)?;
    let mut planted = 0;
    if setting(c, MIGRATED)?.is_none() {
        merge_duplicates(c)?;
        let books: Vec<Material> = candidates(c)?.into_iter().filter(|m| m.kind == "full").collect();
        for b in books {
            let (Some(kind), Some(r)) = (&b.source_kind, &b.source_ref) else { continue };
            if by_source(c, kind, r)?.is_some() {
                continue;
            }
            let shelf = match kbs_list(c)?.into_iter().find(|k| k.name == SHELF) {
                Some(k) => k.uid,
                None => kb_create(c, SHELF, "解析好的书（1.1.0 从概念研究迁过来）", now)?,
            };
            material_add(c, &Material { kb: shelf, ..b }, now)?;
            planted += 1;
        }
        c.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, '1')", [MIGRATED]).map_err(err)?;
    }
    let bare: Vec<i64> = {
        let mut st = c.prepare("SELECT m.id FROM kb_materials m WHERE NOT EXISTS (SELECT 1 FROM passages p WHERE p.material_id = m.id) ORDER BY m.id").map_err(err)?;
        st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    for id in bare {
        passages::sync(c, id)?;
    }
    Ok(planted)
}

/// 0011 以前一个来源在每个库里各一份：并成最早的那一份，它进所有这些库，生长阶段取最高的、照料时间取最近的
fn merge_duplicates(c: &Connection) -> Result<(), String> {
    let groups: Vec<(String, String)> = {
        let mut st = c.prepare("SELECT source_kind, source_ref FROM kb_materials WHERE source_kind IS NOT NULL AND source_ref IS NOT NULL GROUP BY 1, 2 HAVING COUNT(*) > 1").map_err(err)?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    for (kind, r) in groups {
        let ids: Vec<i64> = {
            let mut st = c.prepare("SELECT id FROM kb_materials WHERE source_kind = ?1 AND source_ref = ?2 ORDER BY id").map_err(err)?;
            st.query_map(params![kind, r], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
        };
        let keep = ids[0];
        for other in &ids[1..] {
            c.execute("INSERT OR IGNORE INTO kb_members (kb_id, material_id, added_at) SELECT kb_id, ?1, added_at FROM kb_members WHERE material_id = ?2", params![keep, other]).map_err(err)?;
            c.execute(
                "UPDATE kb_materials SET stage = MAX(stage, (SELECT stage FROM kb_materials WHERE id = ?2)), tended_at = MAX(tended_at, (SELECT tended_at FROM kb_materials WHERE id = ?2)) WHERE id = ?1",
                params![keep, other],
            )
            .map_err(err)?;
            c.execute("DELETE FROM kb_materials WHERE id = ?1", [other]).map_err(err)?;
        }
    }
    Ok(())
}
