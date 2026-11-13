//! 概念研究的存取（`0008_concept.sql`）：文本（单元、段落、页码）、对齐、一次研究（步骤、卡片、句、锚点、关系、来源、拦下的句子）。
//! 每次保存一次研究，在一个事务里整份换掉它的子表行（同 v0.3 的 `research::store`）。

use std::collections::HashMap;

use concept_engine::align::{Alignment, UnitAlignment};
use concept_engine::model::{Card, CardKind, Category, Check, CommentSource, Rejected, Relation, RelationKind, Run, RunStatus, Scope, Sentence, SourceOrigin, Status, StepRecord, ZhStatus};
use concept_engine::text::{Numbering, PageBreak, Paragraph, Role, Text, Unit, UnitKind};
use research_engine::fold::Span;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::common::{Tx, err};

fn json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(err)
}

fn from_json<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(|e| format!("存下的数据读不懂：{e}"))
}

/// 枚举存成它的 serde 名字（snake_case）
fn word<T: Serialize>(v: &T) -> Result<String, String> {
    match serde_json::to_value(v).map_err(err)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(format!("不是一个词：{other}")),
    }
}

fn unword<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| format!("存下的数据读不懂（{s}）：{e}"))
}

// ---------- 文本 ----------

/// 存文本时附带的信息（不在 `Text` 里的）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Meta {
    pub creators: Vec<String>,
    pub publisher: String,
    pub date: String,
    pub identifiers: Vec<String>,
    pub warnings: Vec<String>,
    /// PDF：mathpix / local
    pub converter: Option<String>,
    pub pages: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRow {
    pub id: i64,
    pub uid: String,
    pub item_id: Option<i64>,
    pub attachment_id: Option<i64>,
    pub source_sha256: String,
    pub format: String,
    pub parser: String,
    pub title: String,
    pub language: String,
    pub numbering: String,
    pub page_source: String,
    pub meta: Meta,
    pub original_uid: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewText<'a> {
    pub uid: &'a str,
    pub item_id: Option<i64>,
    pub attachment_id: Option<i64>,
    pub source_sha256: &'a str,
    pub text: &'a Text,
    pub meta: Meta,
    pub original_uid: Option<&'a str>,
    pub now: &'a str,
}

pub fn text_id(c: &Connection, uid: &str) -> Result<i64, String> {
    c.query_row("SELECT id FROM concept_texts WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "找不到这份文本（可能已经删掉了）".to_string())
}

pub fn text_save(c: &Connection, n: &NewText) -> Result<i64, String> {
    let tx = Tx::begin(c)?;
    let original_id = match n.original_uid {
        Some(u) => Some(text_id(&tx, u)?),
        None => None,
    };
    let t = n.text;
    tx.execute(
        "INSERT INTO concept_texts (uid, item_id, attachment_id, source_sha256, format, parser, title, language, numbering, page_source, meta, original_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![n.uid, n.item_id, n.attachment_id, n.source_sha256, t.format, t.parser, t.title, t.language, t.numbering.unwrap_or(Numbering::Chapter).key(), t.page_source, json(&n.meta)?, original_id, n.now],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    for u in &t.units {
        tx.execute(
            "INSERT INTO concept_units (text_id, position, kind, label, key, depth, parent, book, chapter) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, u.index as i64, u.kind.key(), u.label, u.key, u.depth, u.parent.map(|p| p as i64), u.book, u.chapter],
        )
        .map_err(err)?;
    }
    {
        let mut st = tx.prepare("INSERT INTO concept_paragraphs (text_id, seq, unit, role, number, key, attached, body, page, breaks, marker, noterefs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)").map_err(err)?;
        for p in &t.paragraphs {
            st.execute(params![id, p.seq as i64, p.unit as i64, p.role.key(), p.number, p.key, p.attached.map(|a| a as i64), p.text, p.page, json(&p.breaks)?, p.marker, json(&p.noterefs)?]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(id)
}

fn text_row(r: &rusqlite::Row) -> rusqlite::Result<(TextRow, String)> {
    Ok((
        TextRow {
            id: r.get(0)?,
            uid: r.get(1)?,
            item_id: r.get(2)?,
            attachment_id: r.get(3)?,
            source_sha256: r.get(4)?,
            format: r.get(5)?,
            parser: r.get(6)?,
            title: r.get(7)?,
            language: r.get(8)?,
            numbering: r.get(9)?,
            page_source: r.get(10)?,
            meta: Meta::default(),
            original_uid: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        },
        r.get(11)?,
    ))
}

const TEXT_COLUMNS: &str = "t.id, t.uid, t.item_id, t.attachment_id, t.source_sha256, t.format, t.parser, t.title, t.language, t.numbering, t.page_source, t.meta, o.uid, t.created_at, t.updated_at";

pub fn texts_list(c: &Connection) -> Result<Vec<TextRow>, String> {
    let mut st = c.prepare(&format!("SELECT {TEXT_COLUMNS} FROM concept_texts t LEFT JOIN concept_texts o ON o.id = t.original_id ORDER BY t.created_at, t.id")).map_err(err)?;
    let rows = st.query_map([], text_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    rows.into_iter().map(|(mut t, meta)| {
        t.meta = from_json(&meta).unwrap_or_default();
        Ok(t)
    }).collect()
}

pub fn text_row_get(c: &Connection, uid: &str) -> Result<TextRow, String> {
    let (mut t, meta) = c
        .query_row(&format!("SELECT {TEXT_COLUMNS} FROM concept_texts t LEFT JOIN concept_texts o ON o.id = t.original_id WHERE t.uid = ?1"), [uid], text_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| "找不到这份文本（可能已经删掉了）".to_string())?;
    t.meta = from_json(&meta).unwrap_or_default();
    Ok(t)
}

/// 整份文本（单元、段落、页码）
pub fn text_load(c: &Connection, uid: &str) -> Result<Text, String> {
    let row = text_row_get(c, uid)?;
    let mut st = c.prepare("SELECT position, kind, label, key, depth, parent, book, chapter FROM concept_units WHERE text_id = ?1 ORDER BY position").map_err(err)?;
    let units = st
        .query_map([row.id], |r| {
            let kind: String = r.get(1)?;
            Ok(Unit { index: r.get::<_, i64>(0)? as usize, kind: UnitKind::from_key(&kind).unwrap_or(UnitKind::Part), label: r.get(2)?, key: r.get(3)?, depth: r.get(4)?, parent: r.get::<_, Option<i64>>(5)?.map(|p| p as usize), book: r.get(6)?, chapter: r.get(7)? })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut st = c.prepare("SELECT seq, unit, role, number, key, attached, body, page, breaks, marker, noterefs FROM concept_paragraphs WHERE text_id = ?1 ORDER BY seq").map_err(err)?;
    let raw = st
        .query_map([row.id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<u32>>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<i64>>(5)?, r.get::<_, String>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, String>(8)?, r.get::<_, Option<String>>(9)?, r.get::<_, String>(10)?))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut paragraphs = Vec::with_capacity(raw.len());
    for (seq, unit, role, number, key, attached, body, page, breaks, marker, noterefs) in raw {
        let breaks: Vec<PageBreak> = from_json(&breaks)?;
        let noterefs: Vec<(usize, String)> = from_json(&noterefs)?;
        paragraphs.push(Paragraph { seq: seq as usize, unit: unit as usize, role: Role::from_key(&role).unwrap_or(Role::Body), number, key, attached: attached.map(|a| a as usize), text: body, page, breaks, marker, noterefs });
    }
    Ok(Text {
        title: row.title,
        language: row.language,
        creators: row.meta.creators.clone(),
        publisher: row.meta.publisher.clone(),
        date: row.meta.date.clone(),
        identifiers: row.meta.identifiers.clone(),
        format: row.format,
        parser: row.parser,
        numbering: Numbering::from_key(&row.numbering),
        page_source: row.page_source,
        units,
        paragraphs,
        warnings: row.meta.warnings,
    })
}

pub fn text_link(c: &Connection, translation_uid: &str, original_uid: Option<&str>, now: &str) -> Result<(), String> {
    let original_id = match original_uid {
        Some(u) => Some(text_id(c, u)?),
        None => None,
    };
    c.execute("UPDATE concept_texts SET original_id = ?1, updated_at = ?2 WHERE uid = ?3", params![original_id, now, translation_uid]).map_err(err)?;
    Ok(())
}

pub fn text_delete(c: &Connection, uid: &str) -> Result<(), String> {
    let n = c.execute("DELETE FROM concept_texts WHERE uid = ?1", [uid]).map_err(err)?;
    if n == 0 {
        return Err("找不到这份文本（可能已经删掉了）".into());
    }
    Ok(())
}

/// 这个附件已经解析过的文本
pub fn text_of_attachment(c: &Connection, attachment_id: i64) -> Result<Option<String>, String> {
    c.query_row("SELECT uid FROM concept_texts WHERE attachment_id = ?1 ORDER BY id DESC LIMIT 1", [attachment_id], |r| r.get(0)).optional().map_err(err)
}

/// 一条文献解析过的文本（Word 搜索窗用）
pub fn texts_of_item(c: &Connection, item_id: i64) -> Result<Vec<String>, String> {
    let mut st = c.prepare("SELECT uid FROM concept_texts WHERE item_id = ?1 ORDER BY id").map_err(err)?;
    st.query_map([item_id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 和它关联的另一份文本：原著 → 译本（取最新的一份）；译本 → 原著
pub fn partner(c: &Connection, uid: &str) -> Result<Option<(String, bool)>, String> {
    let id = text_id(c, uid)?;
    let original: Option<String> = c.query_row("SELECT o.uid FROM concept_texts t JOIN concept_texts o ON o.id = t.original_id WHERE t.id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
    if let Some(o) = original {
        return Ok(Some((o, false)));
    }
    let translation: Option<String> = c.query_row("SELECT uid FROM concept_texts WHERE original_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| r.get(0)).optional().map_err(err)?;
    Ok(translation.map(|t| (t, true)))
}

// ---------- 对齐 ----------

pub fn alignment_save(c: &Connection, original_uid: &str, translation_uid: &str, a: &Alignment, now: &str) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let (o, t) = (text_id(&tx, original_uid)?, text_id(&tx, translation_uid)?);
    tx.execute("DELETE FROM concept_alignments WHERE original_id = ?1 AND translation_id = ?2", params![o, t]).map_err(err)?;
    tx.execute("INSERT INTO concept_alignments (original_id, translation_id, units, created_at) VALUES (?1, ?2, ?3, ?4)", params![o, t, json(&a.units)?, now]).map_err(err)?;
    let id = tx.last_insert_rowid();
    {
        let mut st = tx.prepare("INSERT INTO concept_aligned (alignment_id, original_seq, translation_seq) VALUES (?1, ?2, ?3)").map_err(err)?;
        for (a, b) in &a.pairs {
            st.execute(params![id, *a as i64, *b as i64]).map_err(err)?;
        }
    }
    tx.commit()
}

pub fn alignment_load(c: &Connection, original_uid: &str, translation_uid: &str) -> Result<Option<Alignment>, String> {
    let (o, t) = (text_id(c, original_uid)?, text_id(c, translation_uid)?);
    let Some((id, units)): Option<(i64, String)> = c.query_row("SELECT id, units FROM concept_alignments WHERE original_id = ?1 AND translation_id = ?2", params![o, t], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)? else { return Ok(None) };
    let units: Vec<UnitAlignment> = from_json(&units)?;
    let mut st = c.prepare("SELECT original_seq, translation_seq FROM concept_aligned WHERE alignment_id = ?1 ORDER BY original_seq").map_err(err)?;
    let pairs = st.query_map([id], |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as usize))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(Some(Alignment { units, pairs }))
}

// ---------- 一次研究 ----------

pub fn run_save(c: &Connection, text_uid: &str, run: &Run) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let text = text_id(&tx, text_uid)?;
    tx.execute(
        "INSERT INTO concept_runs (uid, text_id, skill, skill_title, skill_version, scope, status, next_step, error, model, effort, mock, targets, previous_uid, created_at, updated_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (uid) DO UPDATE SET scope = excluded.scope, status = excluded.status, next_step = excluded.next_step, error = excluded.error, model = excluded.model,
           effort = excluded.effort, mock = excluded.mock, targets = excluded.targets, updated_at = excluded.updated_at, finished_at = excluded.finished_at",
        params![run.uid, text, run.skill, run.skill_title, run.skill_version, json(&run.scope)?, word(&run.status)?, run.next_step as i64, run.error, json(&run.model)?, run.effort, run.mock, json(&run.targets)?, run.previous, run.created_at, run.updated_at, run.finished_at],
    )
    .map_err(err)?;
    let id: i64 = tx.query_row("SELECT id FROM concept_runs WHERE uid = ?1", [&run.uid], |r| r.get(0)).map_err(err)?;
    // 加入文献库时直接改的是库里的行：内存里这份没有的沿用库里的
    let added: HashMap<String, i64> = {
        let mut st = tx.prepare("SELECT key, added_item_id FROM concept_sources WHERE run_id = ?1 AND added_item_id IS NOT NULL").map_err(err)?;
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    for table in ["concept_cards", "concept_relations", "concept_sources", "concept_rejected", "concept_steps"] {
        tx.execute(&format!("DELETE FROM {table} WHERE run_id = ?1"), [id]).map_err(err)?;
    }
    for (i, s) in run.steps.iter().enumerate() {
        tx.execute(
            "INSERT INTO concept_steps (run_id, position, step, label, at, provider, model, input_tokens, output_tokens, searches, cost_usd, mock, ok, note, raw) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![id, i as i64, s.id, s.label, s.at, s.provider, s.model, s.input_tokens as i64, s.output_tokens as i64, s.searches as i64, s.cost_usd, s.mock, s.ok, s.note, s.raw],
        )
        .map_err(err)?;
    }
    for (i, card) in run.cards.iter().enumerate() {
        tx.execute("INSERT INTO concept_cards (run_id, key, position, kind, title, step, about) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![id, card.id, i as i64, card.kind.key(), card.title, card.step, json(&card.about)?]).map_err(err)?;
        let card_id = tx.last_insert_rowid();
        for (j, s) in card.sentences.iter().enumerate() {
            tx.execute("INSERT INTO concept_sentences (card_id, position, category, body, quote, zh, source_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![card_id, j as i64, s.category.key(), s.text, s.quote, s.zh, s.source]).map_err(err)?;
            let sid = tx.last_insert_rowid();
            let k = &s.check;
            tx.execute(
                "INSERT INTO concept_anchors (sentence_id, paragraph, status, span_start, span_end, pages, zh_paragraph, zh_status, zh_start, zh_end, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![sid, s.paragraph, word(&k.status)?, k.span.map(|x| x.start as i64), k.span.map(|x| x.end as i64), k.pages, k.zh_paragraph, k.zh_status.map(|z| word(&z)).transpose()?, k.zh_span.map(|x| x.start as i64), k.zh_span.map(|x| x.end as i64), k.note],
            )
            .map_err(err)?;
        }
    }
    for (i, r) in run.relations.iter().enumerate() {
        tx.execute("INSERT INTO concept_relations (run_id, key, position, from_key, to_key, kind, note, step) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![id, r.id, i as i64, r.from, r.to, r.kind.key(), json(&r.note)?, r.step]).map_err(err)?;
    }
    for (i, s) in run.sources.iter().enumerate() {
        let added_item = s.added_item.or_else(|| added.get(&s.id).copied());
        tx.execute(
            "INSERT INTO concept_sources (run_id, key, position, origin, title, item_id, attachment_id, url, final_url, sha256, ext, fetched_at, text_chars, error, added_item_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![id, s.id, i as i64, word(&s.origin)?, s.title, s.item_id, s.attachment_id, s.url, s.final_url, s.sha256, s.ext, s.fetched_at, s.text_chars as i64, s.error, added_item],
        )
        .map_err(err)?;
    }
    for (i, r) in run.rejected.iter().enumerate() {
        tx.execute("INSERT INTO concept_rejected (run_id, position, step, card, sentence, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, i as i64, r.step, r.card, json(&r.sentence)?, r.reason]).map_err(err)?;
    }
    tx.commit()
}

pub fn run_text(c: &Connection, uid: &str) -> Result<String, String> {
    c.query_row("SELECT t.uid FROM concept_runs r JOIN concept_texts t ON t.id = r.text_id WHERE r.uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "找不到这次研究（可能已经删掉了）".to_string())
}

pub fn run_load(c: &Connection, uid: &str) -> Result<Run, String> {
    type Head = (i64, String, String, u32, String, String, i64, Option<String>, String, String, bool, String, Option<String>, String, String, Option<String>);
    let h: Head = c
        .query_row(
            "SELECT id, skill, skill_title, skill_version, scope, status, next_step, error, model, effort, mock, targets, previous_uid, created_at, updated_at, finished_at FROM concept_runs WHERE uid = ?1",
            [uid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?)),
        )
        .optional()
        .map_err(err)?
        .ok_or_else(|| "找不到这次研究（可能已经删掉了）".to_string())?;
    let id = h.0;
    let scope: Option<Scope> = from_json(&h.4)?;
    let mut run = Run {
        uid: uid.to_string(),
        skill: h.1,
        skill_title: h.2,
        skill_version: h.3,
        scope,
        status: unword::<RunStatus>(&h.5)?,
        next_step: h.6 as usize,
        error: h.7,
        model: from_json(&h.8)?,
        effort: h.9,
        mock: h.10,
        targets: from_json(&h.11)?,
        previous: h.12,
        created_at: h.13,
        updated_at: h.14,
        finished_at: h.15,
        ..Default::default()
    };
    let mut st = c.prepare("SELECT step, label, at, provider, model, input_tokens, output_tokens, searches, cost_usd, mock, ok, note, raw FROM concept_steps WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    run.steps = st
        .query_map([id], |r| {
            Ok(StepRecord { id: r.get(0)?, label: r.get(1)?, at: r.get(2)?, provider: r.get(3)?, model: r.get(4)?, input_tokens: r.get::<_, i64>(5)? as u64, output_tokens: r.get::<_, i64>(6)? as u64, searches: r.get::<_, i64>(7)? as u64, cost_usd: r.get(8)?, mock: r.get(9)?, ok: r.get(10)?, note: r.get(11)?, raw: r.get(12)? })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = c.prepare("SELECT id, key, kind, title, step, about FROM concept_cards WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    let cards: Vec<(i64, String, String, String, String, String)> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut st = c
        .prepare(
            "SELECT s.category, s.body, s.quote, s.zh, s.source_key, a.paragraph, a.status, a.span_start, a.span_end, a.pages, a.zh_paragraph, a.zh_status, a.zh_start, a.zh_end, a.note
               FROM concept_sentences s JOIN concept_anchors a ON a.sentence_id = s.id WHERE s.card_id = ?1 ORDER BY s.position",
        )
        .map_err(err)?;
    for (cid, key, kind, title, step, about) in cards {
        type Raw = (String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>, Option<String>);
        let raw: Vec<Raw> = st
            .query_map([cid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let mut sentences = Vec::new();
        for (cat, body, quote, zh, source, paragraph, status, a, b, pages, zp, zs, za, zb, note) in raw {
            let span = |a: Option<i64>, b: Option<i64>| a.zip(b).map(|(a, b)| Span { start: a as usize, end: b as usize });
            sentences.push(Sentence {
                category: Category::from_key(&cat).ok_or_else(|| format!("存下的类别读不懂：{cat}"))?,
                text: body,
                paragraph,
                quote,
                zh,
                source,
                check: Check { status: unword::<Status>(&status)?, span: span(a, b), pages, zh_paragraph: zp, zh_status: zs.map(|z| unword::<ZhStatus>(&z)).transpose()?, zh_span: span(za, zb), note },
            });
        }
        run.cards.push(Card { id: key, kind: CardKind::from_key(&kind).ok_or_else(|| format!("存下的卡片种类读不懂：{kind}"))?, title, sentences, step, about: from_json(&about)? });
    }
    let mut st = c.prepare("SELECT key, from_key, to_key, kind, note, step FROM concept_relations WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    let rels: Vec<(String, String, String, String, String, String)> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (key, from, to, kind, note, step) in rels {
        run.relations.push(Relation { id: key, from, to, kind: RelationKind::from_key(&kind).ok_or_else(|| format!("存下的关系读不懂：{kind}"))?, note: from_json(&note)?, step });
    }
    let mut st = c.prepare("SELECT key, origin, title, item_id, attachment_id, url, final_url, sha256, ext, fetched_at, text_chars, error, added_item_id FROM concept_sources WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    type Src = (String, String, String, Option<i64>, Option<i64>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>, Option<i64>);
    let srcs: Vec<Src> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (key, origin, title, item_id, attachment_id, url, final_url, sha256, ext, fetched_at, text_chars, error, added_item) in srcs {
        run.sources.push(CommentSource { id: key, origin: unword::<SourceOrigin>(&origin)?, title, item_id, attachment_id, url, final_url, sha256, ext, fetched_at, text_chars: text_chars as usize, error, added_item });
    }
    let mut st = c.prepare("SELECT step, card, sentence, reason FROM concept_rejected WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    let rej: Vec<(String, String, String, String)> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (step, card, sentence, reason) in rej {
        run.rejected.push(Rejected { step, card, sentence: from_json(&sentence)?, reason });
    }
    Ok(run)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub uid: String,
    pub text_uid: String,
    pub skill: String,
    pub skill_title: String,
    pub status: RunStatus,
    pub mock: bool,
    /// 章的引用名（整章入口）或第一段的段落号
    pub unit: Option<String>,
    pub first_paragraph: Option<String>,
    pub label: String,
    pub created_at: String,
    pub cards: Vec<CardSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSummary {
    pub key: String,
    pub kind: CardKind,
    pub title: String,
}

/// 一份文本的各次研究（侧栏：书 → 章 → 卡片），新的在后
pub fn runs_list(c: &Connection, text_uid: &str) -> Result<Vec<RunSummary>, String> {
    let text = text_id(c, text_uid)?;
    let mut st = c.prepare("SELECT id, uid, skill, skill_title, status, mock, scope, created_at FROM concept_runs WHERE text_id = ?1 ORDER BY created_at, id").map_err(err)?;
    type Row = (i64, String, String, String, String, bool, String, String);
    let rows: Vec<Row> = st.query_map([text], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut cst = c.prepare("SELECT key, kind, title FROM concept_cards WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    let mut out = Vec::new();
    for (id, uid, skill, skill_title, status, mock, scope, created_at) in rows {
        let scope: Option<Scope> = from_json(&scope).unwrap_or(None);
        let cards = cst
            .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .map_err(err)?
            .filter_map(Result::ok)
            .filter_map(|(key, kind, title)| CardKind::from_key(&kind).map(|kind| CardSummary { key, kind, title }))
            .collect();
        let label = match &scope {
            Some(s) => s.question.clone().or_else(|| (!s.terms.is_empty()).then(|| s.terms.join(" / "))).or_else(|| s.instruction.clone()).unwrap_or_else(|| if s.paragraphs.len() > 1 { format!("{} 等 {} 段", s.paragraphs[0], s.paragraphs.len()) } else { s.paragraphs.first().cloned().unwrap_or_default() }),
            None => String::new(),
        };
        out.push(RunSummary { uid, text_uid: text_uid.to_string(), skill, skill_title, status: unword(&status)?, mock, unit: scope.as_ref().and_then(|s| s.unit.clone()), first_paragraph: scope.as_ref().and_then(|s| s.paragraphs.first().cloned()), label, created_at, cards });
    }
    Ok(out)
}

pub fn run_delete(c: &Connection, uid: &str) -> Result<(), String> {
    let n = c.execute("DELETE FROM concept_runs WHERE uid = ?1", [uid]).map_err(err)?;
    if n == 0 {
        return Err("找不到这次研究（可能已经删掉了）".into());
    }
    Ok(())
}

/// 加入文献库后记下对应的文献
pub fn source_set_added(c: &Connection, run_uid: &str, key: &str, item_id: i64) -> Result<(), String> {
    c.execute("UPDATE concept_sources SET added_item_id = ?1 WHERE key = ?2 AND run_id = (SELECT id FROM concept_runs WHERE uid = ?3)", params![item_id, key, run_uid]).map_err(err)?;
    Ok(())
}

/// 还在用的快照（删研究后清理用）
pub fn snapshot_shas(c: &Connection) -> Result<std::collections::HashSet<String>, String> {
    let mut st = c.prepare("SELECT DISTINCT sha256 FROM concept_sources WHERE sha256 IS NOT NULL").map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 启动时：异常退出时还在「进行中」的标成「已停止」，可以接着做
pub fn mark_interrupted(c: &Connection) -> Result<usize, String> {
    c.execute("UPDATE concept_runs SET status = 'cancelled', error = '上次 App 退出时这次研究还没做完，点「继续」接着做。' WHERE status = 'running'", []).map_err(err)
}
