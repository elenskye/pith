//! 对话、角色、知识库的存取（migration 0010，`decisions/0030`）。
//!
//! - 角色：内置四个写在这里（`BUILTIN`），改过的用同一个 uid 存进 roles 表；新建的也存进去。
//! - 对话：消息的 body 是 JSON（前端照它画：你说的话、Pith 的状态行、段落、编号、反问、交付卡）。
//!   以前的调查、概念研究没有对话，列表里照样列出来（`list`），点开、接着问时再建对话（`ensure_for_ref`）。
//! - 知识库：材料指回原来的东西（文本、调查、卡片、条目），删了原来的东西材料还在（只留标题和摘要）。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{err, new_uid};

// ---------- 角色 ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub uid: String,
    pub name: String,
    pub mark: String,
    pub line: String,
    pub prompt: String,
    pub kb: String,
    pub model: String,
    pub skills: Vec<String>,
    /// 内置的（不能删，只能改回默认）
    #[serde(default)]
    pub builtin: bool,
    /// 内置的改过了
    #[serde(default)]
    pub edited: bool,
}

/// 一个初始角色：uid、名字、头像字、一句话、提示词、默认知识库、常用技能
type BuiltinRole = (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static [&'static str]);

/// 四个初始角色（docs/decisions/0030「角色」）
const BUILTIN: [BuiltinRole; 4] = [
    ("pith", "Pith", "P", "研究助手 · 默认", "你是站主的研究助手。什么都能问：先给结论，再给依据和编号；该用哪个技能自己挑。", "上一次勾的", &[]),
    ("companion", "伴读", "伴", "读一本书的时候", "你陪站主读书：讲清一个概念的来龙去脉，反过来问他，帮他整理摘抄、做概念卡。不急着替他下结论。", "在读的那本书", &["问这本书", "整章分析", "追踪概念"]),
    ("reviewer", "审稿人", "审", "专挑刺", "你是站主的审稿人。先找这个判断最弱的地方，再找反例和后世的批评；有出处就给编号。站主的判断你先唱反调，但别为反对而反对。", "跟着对话", &["事实调查", "反方与后世评论", "假设推演"]),
    ("editor", "编辑", "编", "写论文的时候", "你是站主的编辑：守学校的规范和引文格式，润色时保留他的语气；改 Word 之前先把修订给他看。", "毕业论文", &["排版", "插入引文"]),
];

fn builtin(uid: &str) -> Option<Role> {
    BUILTIN.iter().find(|b| b.0 == uid).map(|&(uid, name, mark, line, prompt, kb, skills)| Role {
        uid: uid.into(),
        name: name.into(),
        mark: mark.into(),
        line: line.into(),
        prompt: prompt.into(),
        kb: kb.into(),
        model: String::new(),
        skills: skills.iter().map(|s| s.to_string()).collect(),
        builtin: true,
        edited: false,
    })
}

fn role_row(r: &rusqlite::Row) -> rusqlite::Result<Role> {
    let skills: String = r.get(7)?;
    Ok(Role {
        uid: r.get(0)?,
        name: r.get(1)?,
        mark: r.get(2)?,
        line: r.get(3)?,
        prompt: r.get(4)?,
        kb: r.get(5)?,
        model: r.get(6)?,
        skills: serde_json::from_str(&skills).unwrap_or_default(),
        builtin: false,
        edited: false,
    })
}

/// 全部角色：内置的四个在前（改过的用改过的），新建的按顺序在后
pub fn roles_list(c: &Connection) -> Result<Vec<Role>, String> {
    let mut st = c.prepare("SELECT uid, name, mark, line, prompt, kb, model, skills FROM roles ORDER BY position, created_at, id").map_err(err)?;
    let rows = st.query_map([], role_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut out: Vec<Role> = BUILTIN.iter().map(|b| builtin(b.0).expect("内置角色")).collect();
    for r in rows {
        match out.iter_mut().find(|b| b.uid == r.uid) {
            Some(b) => *b = Role { builtin: true, edited: true, ..r },
            None => out.push(r),
        }
    }
    Ok(out)
}

pub fn role_get(c: &Connection, uid: &str) -> Result<Role, String> {
    roles_list(c)?.into_iter().find(|r| r.uid == uid).ok_or_else(|| "这个角色不存在了。".to_string())
}

/// 存一个角色：uid 是空的就新建；内置的改过就存一份覆盖
pub fn role_save(c: &Connection, role: &Role, now: &str) -> Result<Role, String> {
    let name = role.name.trim();
    if name.is_empty() {
        return Err("角色要有名字。".into());
    }
    let uid = if role.uid.is_empty() { new_uid()? } else { role.uid.clone() };
    let mark = role.mark.trim().chars().next().map(String::from).unwrap_or_else(|| name.chars().next().unwrap_or('?').to_string());
    let skills = serde_json::to_string(&role.skills).map_err(err)?;
    c.execute(
        "INSERT INTO roles (uid, name, mark, line, prompt, kb, model, skills, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT COALESCE(MAX(position), 0) + 1 FROM roles), ?9, ?9)
         ON CONFLICT(uid) DO UPDATE SET name = ?2, mark = ?3, line = ?4, prompt = ?5, kb = ?6, model = ?7, skills = ?8, updated_at = ?9",
        params![uid, name, mark, role.line.trim(), role.prompt.trim(), role.kb.trim(), role.model.trim(), skills, now],
    )
    .map_err(err)?;
    role_get(c, &uid)
}

/// 删掉一个角色；内置的是改回默认
pub fn role_delete(c: &Connection, uid: &str) -> Result<(), String> {
    c.execute("DELETE FROM roles WHERE uid = ?1", [uid]).map_err(err)?;
    Ok(())
}

// ---------- 对话 ----------

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConvSummary {
    /// 以前的调查、研究还没有对话时是空的
    pub uid: Option<String>,
    pub title: String,
    pub role: String,
    pub skill: Option<String>,
    pub ref_uid: Option<String>,
    pub stage: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub position: i64,
    pub author: String,
    pub body: Value,
    pub created_at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub uid: String,
    pub title: String,
    pub role: String,
    pub kbs: Vec<String>,
    pub skill: Option<String>,
    pub ref_uid: Option<String>,
    pub stage: i64,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
}

pub(crate) fn conv_id(c: &Connection, uid: &str) -> Result<i64, String> {
    c.query_row("SELECT id FROM conversations WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "这段对话不存在了。".to_string())
}

/// 标题：第一句话，太长截到 40 个字
pub fn title_of(text: &str) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim();
    if t.chars().count() > 40 { format!("{}……", t.chars().take(40).collect::<String>()) } else if t.is_empty() { "新对话".into() } else { t.to_string() }
}

pub fn create(c: &Connection, title: &str, role: &str, kbs: &[String], skill: Option<&str>, ref_uid: Option<&str>, now: &str) -> Result<String, String> {
    let uid = new_uid()?;
    c.execute(
        "INSERT INTO conversations (uid, title, role_uid, kbs, skill, ref_uid, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![uid, title_of(title), role, serde_json::to_string(kbs).map_err(err)?, skill, ref_uid, now],
    )
    .map_err(err)?;
    Ok(uid)
}

pub fn add_message(c: &Connection, uid: &str, author: &str, body: &Value, now: &str) -> Result<(), String> {
    let id = conv_id(c, uid)?;
    c.execute(
        "INSERT INTO messages (conversation_id, position, author, body, created_at)
         VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM messages WHERE conversation_id = ?1), ?2, ?3, ?4)",
        params![id, author, body.to_string(), now],
    )
    .map_err(err)?;
    // 对话越常被照料长得越好：三问以上「生长」，十问以上「常青」
    c.execute(
        "UPDATE conversations SET updated_at = ?2,
           stage = (SELECT CASE WHEN n >= 20 THEN 2 WHEN n >= 6 THEN 1 ELSE 0 END FROM (SELECT COUNT(*) AS n FROM messages WHERE conversation_id = ?1))
         WHERE id = ?1",
        params![id, now],
    )
    .map_err(err)?;
    Ok(())
}

pub fn get(c: &Connection, uid: &str) -> Result<Conversation, String> {
    let id = conv_id(c, uid)?;
    let (title, role, kbs, skill, ref_uid, stage, created_at, updated_at): (String, String, String, Option<String>, Option<String>, i64, String, String) = c
        .query_row("SELECT title, role_uid, kbs, skill, ref_uid, stage, created_at, updated_at FROM conversations WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
        })
        .map_err(err)?;
    let mut st = c.prepare("SELECT position, author, body, created_at FROM messages WHERE conversation_id = ?1 ORDER BY position").map_err(err)?;
    let messages = st
        .query_map([id], |r| {
            let body: String = r.get(2)?;
            Ok(Message { position: r.get(0)?, author: r.get(1)?, body: serde_json::from_str(&body).unwrap_or(Value::Null), created_at: r.get(3)? })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(Conversation { uid: uid.into(), title, role, kbs: serde_json::from_str(&kbs).unwrap_or_default(), skill, ref_uid, stage, created_at, updated_at, messages })
}

/// 技能的那次运行有没有对话
pub fn by_ref(c: &Connection, skill: &str, ref_uid: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT uid FROM conversations WHERE skill = ?1 AND ref_uid = ?2 ORDER BY id LIMIT 1", [skill, ref_uid], |r| r.get(0)).optional().map_err(err)
}

/// 以前的调查、研究：点开接着问时建一段对话连上它
pub fn ensure_for_ref(c: &Connection, skill: &str, ref_uid: &str, title: &str, now: &str) -> Result<String, String> {
    match by_ref(c, skill, ref_uid)? {
        Some(uid) => Ok(uid),
        None => create(c, title, "pith", &[], Some(skill), Some(ref_uid), now),
    }
}

/// 对话开始时还没跑技能（先确认计划），跑起来以后连上
pub fn link(c: &Connection, uid: &str, skill: &str, ref_uid: &str, now: &str) -> Result<(), String> {
    let n = c.execute("UPDATE conversations SET skill = ?2, ref_uid = ?3, updated_at = ?4 WHERE uid = ?1", params![uid, skill, ref_uid, now]).map_err(err)?;
    if n == 0 { Err("这段对话不存在了。".into()) } else { Ok(()) }
}

pub fn rename(c: &Connection, uid: &str, title: &str) -> Result<(), String> {
    let n = c.execute("UPDATE conversations SET title = ?2 WHERE uid = ?1", params![uid, title_of(title)]).map_err(err)?;
    if n == 0 { Err("这段对话不存在了。".into()) } else { Ok(()) }
}

pub fn delete(c: &Connection, uid: &str) -> Result<(), String> {
    c.execute("DELETE FROM conversations WHERE uid = ?1", [uid]).map_err(err)?;
    Ok(())
}

/// 对话历史：对话，加上还没有对话的调查（每个问题最新的那次）和概念研究；新的在前
pub fn list(c: &Connection) -> Result<Vec<ConvSummary>, String> {
    let mut st = c.prepare("SELECT uid, title, role_uid, skill, ref_uid, stage, created_at, updated_at FROM conversations").map_err(err)?;
    let mut out = st
        .query_map([], |r| {
            Ok(ConvSummary { uid: Some(r.get(0)?), title: r.get(1)?, role: r.get(2)?, skill: r.get(3)?, ref_uid: r.get(4)?, stage: r.get(5)?, created_at: r.get(6)?, updated_at: r.get(7)? })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let linked = |out: &[ConvSummary], skill: &str, refs: &[&str]| out.iter().any(|s| s.skill.as_deref() == Some(skill) && s.ref_uid.as_deref().is_some_and(|r| refs.contains(&r)));
    for q in crate::research::store::questions_list(c)? {
        let refs: Vec<&str> = q.runs.iter().map(|r| r.uid.as_str()).collect();
        if refs.is_empty() || linked(&out, "research", &refs) {
            continue;
        }
        out.push(ConvSummary { uid: None, title: title_of(&q.text), role: "pith".into(), skill: Some("research".into()), ref_uid: Some(refs[0].into()), stage: 1, created_at: q.created_at, updated_at: q.updated_at });
    }
    let mut st = c
        .prepare("SELECT r.uid, r.skill_title, t.title, r.created_at, r.updated_at FROM concept_runs r LEFT JOIN concept_texts t ON t.id = r.text_id")
        .map_err(err)?;
    let runs = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))).map_err(err)?;
    for row in runs {
        let (uid, skill_title, text_title, created_at, updated_at) = row.map_err(err)?;
        if linked(&out, "concept", &[uid.as_str()]) {
            continue;
        }
        let title = match text_title.filter(|t| !t.is_empty()) {
            Some(t) => format!("{skill_title} · {t}"),
            None => skill_title,
        };
        out.push(ConvSummary { uid: None, title: title_of(&title), role: "companion".into(), skill: Some("concept".into()), ref_uid: Some(uid), stage: 1, created_at, updated_at });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

// ---------- 知识库 ----------

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
    #[serde(default)]
    pub kb: String,
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
}

pub const KINDS: [&str; 10] = ["full", "excerpt", "note", "web", "file", "scan", "concept", "report", "wonder", "answer"];

fn kb_id(c: &Connection, uid: &str) -> Result<i64, String> {
    c.query_row("SELECT id FROM knowledge_bases WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "这个知识库不存在了。".to_string())
}

pub fn kbs_list(c: &Connection) -> Result<Vec<KnowledgeBase>, String> {
    let mut st = c
        .prepare(
            "SELECT k.uid, k.name, k.note, (SELECT COUNT(*) FROM kb_materials m WHERE m.kb_id = k.id), k.created_at
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

/// 删掉知识库和里面的材料（材料只是指回原来的东西，原来的东西不动）
pub fn kb_delete(c: &Connection, uid: &str) -> Result<(), String> {
    c.execute("DELETE FROM knowledge_bases WHERE uid = ?1", [uid]).map_err(err)?;
    Ok(())
}

const MATERIAL_COLUMNS: &str = "m.uid, k.uid, m.kind, m.title, m.sub, m.body, m.stage, m.source_kind, m.source_ref, c.uid, m.created_at, m.tended_at";

fn material_row(r: &rusqlite::Row) -> rusqlite::Result<Material> {
    Ok(Material {
        uid: r.get(0)?,
        kb: r.get(1)?,
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
    })
}

/// 一个知识库里的材料（kb 是空的：全部），最近照料的在前
pub fn materials(c: &Connection, kb: Option<&str>) -> Result<Vec<Material>, String> {
    let sql = format!(
        "SELECT {MATERIAL_COLUMNS} FROM kb_materials m JOIN knowledge_bases k ON k.id = m.kb_id LEFT JOIN conversations c ON c.id = m.conversation_id
         WHERE (?1 IS NULL OR k.uid = ?1) ORDER BY m.tended_at DESC, m.id DESC"
    );
    let mut st = c.prepare(&sql).map_err(err)?;
    st.query_map([kb], material_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn material_get(c: &Connection, uid: &str) -> Result<Material, String> {
    let sql = format!("SELECT {MATERIAL_COLUMNS} FROM kb_materials m JOIN knowledge_bases k ON k.id = m.kb_id LEFT JOIN conversations c ON c.id = m.conversation_id WHERE m.uid = ?1");
    c.query_row(&sql, [uid], material_row).optional().map_err(err)?.ok_or_else(|| "这份材料不存在了。".to_string())
}

/// 种进知识库。同一个来源在同一个库里只种一次（再种就是照料一下，返回原来那份）
pub fn material_add(c: &Connection, m: &Material, now: &str) -> Result<Material, String> {
    if !KINDS.contains(&m.kind.as_str()) {
        return Err(format!("不认识的材料种类：{}", m.kind));
    }
    if m.title.trim().is_empty() {
        return Err("材料要有标题。".into());
    }
    let kb = kb_id(c, &m.kb)?;
    if let (Some(kind), Some(r)) = (&m.source_kind, &m.source_ref) {
        let had: Option<String> = c
            .query_row("SELECT uid FROM kb_materials WHERE kb_id = ?1 AND source_kind = ?2 AND source_ref = ?3", params![kb, kind, r], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if let Some(uid) = had {
            c.execute("UPDATE kb_materials SET tended_at = ?2 WHERE uid = ?1", params![uid, now]).map_err(err)?;
            return material_get(c, &uid);
        }
    }
    let conv = match &m.conversation {
        Some(uid) => Some(conv_id(c, uid)?),
        None => None,
    };
    let uid = new_uid()?;
    c.execute(
        "INSERT INTO kb_materials (uid, kb_id, kind, title, sub, body, stage, source_kind, source_ref, conversation_id, created_at, tended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![uid, kb, m.kind, m.title.trim(), m.sub.trim(), m.body, m.stage.clamp(0, 2), m.source_kind, m.source_ref, conv, now],
    )
    .map_err(err)?;
    c.execute("UPDATE knowledge_bases SET updated_at = ?2 WHERE id = ?1", params![kb, now]).map_err(err)?;
    material_get(c, &uid)
}

/// 改标题、正文、生长阶段；改了就算照料过
pub fn material_update(c: &Connection, uid: &str, title: &str, body: &str, stage: i64, now: &str) -> Result<Material, String> {
    if title.trim().is_empty() {
        return Err("材料要有标题。".into());
    }
    let n = c
        .execute("UPDATE kb_materials SET title = ?2, body = ?3, stage = ?4, tended_at = ?5 WHERE uid = ?1", params![uid, title.trim(), body, stage.clamp(0, 2), now])
        .map_err(err)?;
    if n == 0 {
        return Err("这份材料不存在了。".into());
    }
    material_get(c, uid)
}

pub fn material_delete(c: &Connection, uid: &str) -> Result<(), String> {
    c.execute("DELETE FROM kb_materials WHERE uid = ?1", [uid]).map_err(err)?;
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
            out.push(Material { kind: "report".into(), title: title_of(&q.text), sub: format!("事实调查 · 第 {} 次", run.seq), stage: 1, source_kind: Some("research_run".into()), source_ref: Some(run.uid.clone()), ..Default::default() });
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
