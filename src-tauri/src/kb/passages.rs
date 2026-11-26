//! 段落块（migration 0011）：检索、读原文、引用都对着它。
//!
//! - **书**（来源是解析好的文本）：一段一块，段落号、页码、所在的章照文本（`concept_paragraphs` 原样抄一份，seq 一样，对齐靠它）。
//!   书的文本删了，知识库里这份还在；重新解析了就重抄（`sync_source`）。
//! - **别的材料**：正文按空行切，太短的并在一起、太长的在句末拆开；单独一行的 `#` 小标题是标题块，后面的块记在它名下。
//!   调查报告、概念卡没有正文时用原来的东西（报告的 Markdown、卡片的句子）。
//! - 重切时同一个位置（seq）的块留着原来的 id（编号 `[P123]` 不变），只改变了的字；多出来的删、少的补。

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::index;
use crate::common::err;

/// 一块（给界面、给工具）
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    pub id: i64,
    #[serde(skip)]
    pub material_id: i64,
    pub seq: i64,
    pub role: String,
    pub key: String,
    pub unit_key: String,
    pub unit_label: String,
    pub page: Option<String>,
    pub text: String,
}

/// 要写进去的一块
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub role: String,
    pub key: String,
    pub unit_key: String,
    pub unit_label: String,
    pub page: Option<String>,
    pub text: String,
}

impl Block {
    fn body(text: String, unit: &str) -> Block {
        Block { role: "body".into(), key: String::new(), unit_key: String::new(), unit_label: unit.into(), page: None, text }
    }
}

/// 不进检索的：标题、前后附（目录、版权页……）
pub fn searchable(role: &str) -> bool {
    matches!(role, "body" | "sidenote" | "note" | "guide")
}

// ---------- 切 ----------

const SHORT: usize = 280;
const LONG: usize = 1600;
const CUT: usize = 900;

/// 正文切块：按空行；单独一行的 # 小标题是标题块；太短的并，太长的在句末拆
pub fn chunk(body: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut unit = String::new();
    let mut pending = String::new();
    let flush = |pending: &mut String, unit: &str, out: &mut Vec<Block>| {
        let t = pending.trim().to_string();
        pending.clear();
        if t.is_empty() {
            return;
        }
        for piece in split_long(&t) {
            out.push(Block::body(piece, unit));
        }
    };
    for para in body.replace("\r\n", "\n").split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let one_line = !para.contains('\n');
        if one_line && para.starts_with('#') {
            flush(&mut pending, &unit, &mut out);
            let head = para.trim_start_matches('#').trim().to_string();
            if !head.is_empty() {
                unit = head.clone();
                out.push(Block { role: "heading".into(), key: String::new(), unit_key: String::new(), unit_label: head.clone(), page: None, text: head });
            }
            continue;
        }
        if !pending.is_empty() && pending.chars().count() + para.chars().count() > SHORT * 2 {
            flush(&mut pending, &unit, &mut out);
        }
        if !pending.is_empty() {
            pending.push_str("\n\n");
        }
        pending.push_str(para);
        if pending.chars().count() >= SHORT {
            flush(&mut pending, &unit, &mut out);
        }
    }
    flush(&mut pending, &unit, &mut out);
    out
}

/// 太长的一段在句末拆成约 900 字一块
fn split_long(t: &str) -> Vec<String> {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= LONG {
        return vec![t.to_string()];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while chars.len() - start > LONG {
        let mut end = start + CUT;
        let limit = (start + LONG).min(chars.len());
        while end < limit && !matches!(chars[end - 1], '。' | '！' | '？' | '.' | '!' | '?' | '\n' | '；' | ';') {
            end += 1;
        }
        out.push(chars[start..end].iter().collect::<String>().trim().to_string());
        start = end;
    }
    out.push(chars[start..].iter().collect::<String>().trim().to_string());
    out.retain(|s| !s.is_empty());
    out
}

/// 一本解析好的书：一段一块
fn from_text(c: &Connection, text_uid: &str) -> Result<Option<Vec<Block>>, String> {
    if crate::concept::store::text_id(c, text_uid).is_err() {
        return Ok(None);
    }
    let t = crate::concept::store::text_load(c, text_uid)?;
    Ok(Some(
        t.paragraphs
            .iter()
            .map(|p| {
                let u = &t.units[p.unit];
                Block { role: p.role.key().into(), key: p.key.clone(), unit_key: u.key.clone(), unit_label: u.label.clone(), page: t.paragraph_pages(p.seq), text: p.text.clone() }
            })
            .collect(),
    ))
}

/// 做完的调查：报告的 Markdown
fn from_report(c: &Connection, run_uid: &str) -> Option<String> {
    let run = crate::research::store::run_load(c, run_uid).ok()?;
    let doc = research_engine::render::document(&research_engine::report::build(&run, None));
    Some(research_engine::render::markdown(&doc))
}

/// 概念研究的一张卡：一句一行
fn from_card(c: &Connection, reference: &str) -> Option<String> {
    let (run, key) = reference.split_once('#')?;
    let mut st = c
        .prepare("SELECT s.body FROM concept_sentences s JOIN concept_cards k ON k.id = s.card_id JOIN concept_runs r ON r.id = k.run_id WHERE r.uid = ?1 AND k.key = ?2 ORDER BY s.position")
        .ok()?;
    let lines: Vec<String> = st.query_map(params![run, key], |r| r.get(0)).ok()?.filter_map(Result::ok).collect();
    (!lines.is_empty()).then(|| lines.join("\n\n"))
}

/// 一份材料该有的块。书的文本已经删了：None（留着原来抄下的）
fn blocks_of(c: &Connection, id: i64) -> Result<Option<Vec<Block>>, String> {
    let (body, source_kind, source_ref): (String, Option<String>, Option<String>) =
        c.query_row("SELECT body, source_kind, source_ref FROM kb_materials WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?;
    let source = || -> Option<String> {
        match (source_kind.as_deref(), source_ref.as_deref()) {
            (Some("research_run"), Some(r)) => from_report(c, r),
            (Some("concept_card"), Some(r)) => from_card(c, r),
            _ => None,
        }
    };
    if let (Some("concept_text"), Some(r)) = (source_kind.as_deref(), source_ref.as_deref()) {
        return from_text(c, r);
    }
    if !body.trim().is_empty() {
        return Ok(Some(chunk(&body)));
    }
    Ok(Some(source().map(|s| chunk(&s)).unwrap_or_default()))
}

/// 重切一份材料。同一个 seq 的块留着原来的 id；检索的索引跟着改（向量按文字的散列，变了的下次重算）。返回块数
pub fn sync(c: &Connection, material: i64) -> Result<usize, String> {
    let Some(blocks) = blocks_of(c, material)? else {
        return c.query_row("SELECT COUNT(*) FROM passages WHERE material_id = ?1", [material], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(err);
    };
    let old: std::collections::HashMap<i64, (i64, String, String)> = {
        let mut st = c.prepare("SELECT seq, id, role, text FROM passages WHERE material_id = ?1").map_err(err)?;
        st.query_map([material], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    let mut changed: Vec<(i64, String, String)> = Vec::new(); // (id, role, text)：索引要重写的
    {
        let mut upd = c.prepare("UPDATE passages SET role = ?2, key = ?3, unit_key = ?4, unit_label = ?5, page = ?6, text = ?7 WHERE id = ?1").map_err(err)?;
        let mut ins = c.prepare("INSERT INTO passages (material_id, seq, role, key, unit_key, unit_label, page, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)").map_err(err)?;
        for (seq, b) in blocks.iter().enumerate() {
            match old.get(&(seq as i64)) {
                Some((id, role, text)) => {
                    upd.execute(params![id, b.role, b.key, b.unit_key, b.unit_label, b.page, b.text]).map_err(err)?;
                    if role != &b.role || text != &b.text {
                        changed.push((*id, b.role.clone(), b.text.clone()));
                    }
                }
                None => {
                    ins.execute(params![material, seq as i64, b.role, b.key, b.unit_key, b.unit_label, b.page, b.text]).map_err(err)?;
                    changed.push((c.last_insert_rowid(), b.role.clone(), b.text.clone()));
                }
            }
        }
    }
    let gone: Vec<i64> = old.iter().filter(|(seq, _)| **seq >= blocks.len() as i64).map(|(_, (id, ..))| *id).collect();
    c.execute("DELETE FROM passages WHERE material_id = ?1 AND seq >= ?2", params![material, blocks.len() as i64]).map_err(err)?;
    index::update(c, &changed, &gone)?;
    Ok(blocks.len())
}

/// 来源变了（书重新解析过）：那份材料重切
pub fn sync_source(c: &Connection, kind: &str, reference: &str) -> Result<(), String> {
    if let Some(uid) = super::store::by_source(c, kind, reference)? {
        sync(c, super::store::material_id(c, &uid)?)?;
    }
    Ok(())
}

// ---------- 读 ----------

const COLUMNS: &str = "id, material_id, seq, role, key, unit_key, unit_label, page, text";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Passage> {
    Ok(Passage { id: r.get(0)?, material_id: r.get(1)?, seq: r.get(2)?, role: r.get(3)?, key: r.get(4)?, unit_key: r.get(5)?, unit_label: r.get(6)?, page: r.get(7)?, text: r.get(8)? })
}

pub fn get(c: &Connection, id: i64) -> Result<Option<Passage>, String> {
    c.query_row(&format!("SELECT {COLUMNS} FROM passages WHERE id = ?1"), [id], row).optional().map_err(err)
}

pub fn at(c: &Connection, material: i64, seq: i64) -> Result<Option<Passage>, String> {
    c.query_row(&format!("SELECT {COLUMNS} FROM passages WHERE material_id = ?1 AND seq = ?2"), params![material, seq], row).optional().map_err(err)
}

fn many(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<Vec<Passage>, String> {
    let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM passages WHERE {sql}")).map_err(err)?;
    st.query_map(args, row).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 一章的块。书按章（`unit_key`）；别的材料按小标题（`unit_label`），都没有就是整份
pub fn unit(c: &Connection, material: i64, unit_key: &str, unit_label: &str) -> Result<Vec<Passage>, String> {
    if !unit_key.is_empty() {
        many(c, "material_id = ?1 AND unit_key = ?2 ORDER BY seq", params![material, unit_key])
    } else {
        many(c, "material_id = ?1 AND unit_key = '' AND unit_label = ?2 ORDER BY seq", params![material, unit_label])
    }
}

/// 一块前后各 `around` 块（同一章里）
pub fn around(c: &Connection, p: &Passage, around: i64) -> Result<Vec<Passage>, String> {
    let all = many(c, "material_id = ?1 AND seq BETWEEN ?2 AND ?3 ORDER BY seq", params![p.material_id, p.seq - around, p.seq + around])?;
    Ok(all.into_iter().filter(|x| x.unit_key == p.unit_key && (!p.unit_key.is_empty() || x.unit_label == p.unit_label)).collect())
}

pub fn all(c: &Connection, material: i64) -> Result<Vec<Passage>, String> {
    many(c, "material_id = ?1 ORDER BY seq", [material])
}

/// 目录里的一章
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitInfo {
    pub key: String,
    pub label: String,
    /// 能读的块数（标题、前后附不算）
    pub count: usize,
    pub chars: usize,
    /// 第一块
    pub first: i64,
}

/// 一份材料的目录：按出现的顺序，一章一行（前后附、只有标题的不列）
pub fn outline(c: &Connection, material: i64) -> Result<Vec<UnitInfo>, String> {
    let mut out: Vec<UnitInfo> = Vec::new();
    for p in all(c, material)? {
        let (key, label) = (p.unit_key.clone(), p.unit_label.clone());
        let i = match out.iter().position(|u| u.key == key && u.label == label) {
            Some(i) => i,
            None => {
                out.push(UnitInfo { key, label, count: 0, chars: 0, first: p.id });
                out.len() - 1
            }
        };
        if searchable(&p.role) {
            out[i].count += 1;
            out[i].chars += p.text.chars().count();
        }
    }
    out.retain(|u| u.count > 0);
    Ok(out)
}
