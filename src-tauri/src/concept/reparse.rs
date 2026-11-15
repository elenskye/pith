//! 重新解析：同一份附件再解析一次，就地换掉这份文本的单元和段落（uid、id、译本关联都不变），
//! 再给旧卡片按内容重新找锚点（规则在 `concept_engine::reanchor`）。
//!
//! 一次重新解析（`apply`）：先做「导入前」备份；然后在一个事务里
//!
//! 1. 记下旧的书（旧文本、和它关联的另一本、旧的对齐）：重找锚点要知道每个锚点原来在哪；
//! 2. 换掉文本，段落检索表跟着换；
//! 3. 重新对齐：它是原著，就和每一份译本重新对齐；它是译本，就和原著重新对齐；
//! 4. 受影响的研究逐个重找锚点、存回去、重新审计：这份文本上的研究；它是别的原著当前用的译本时，还有原著上的研究
//!    （中译句、挂在译本旁注 / 注释 / 导读上的句子）。
//!
//! 中途出错整个回滚，旧文本、对齐、检索、研究都原样。

use std::path::Path;

use concept_engine::align::{self, Alignment};
use concept_engine::audit::{self, Finding};
use concept_engine::model::Run;
use concept_engine::prompts::Books;
use concept_engine::reanchor::{Change, Reanchor};
use concept_engine::text::{Numbering, Text};
use rusqlite::Connection;
use serde::Serialize;

use super::store::{self, Meta};
use super::search;
use crate::common::Tx;
use crate::research::snapshot::Snapshots;

/// 这个附件已经解析过的文本，和重新解析会动到的研究（解析对话框里说明用）
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Existing {
    pub uid: String,
    pub runs: usize,
    pub cards: usize,
}

/// 一次研究重找锚点的结果
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub uid: String,
    /// 这次研究是哪份文本（原著）上的
    pub text_uid: String,
    pub skill_title: String,
    pub label: String,
    pub cards: usize,
    pub kept: usize,
    pub moved: usize,
    pub lost: usize,
    pub zh_lost: usize,
    pub scope_dropped: usize,
    /// 位置变了、没挂上、中译核不上了的句子
    pub changes: Vec<Change>,
    /// 做完重新审计发现的问题
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reparsed {
    pub uid: String,
    /// 「导入前」备份的文件名
    pub backup: String,
    pub paragraphs: usize,
    pub cards: usize,
    /// 原位跟过去的、位置变了的、没挂上的句子，中译核不上了的句子
    pub kept: usize,
    pub moved: usize,
    pub lost: usize,
    pub zh_lost: usize,
    pub findings: usize,
    pub runs: Vec<RunReport>,
}

/// 受影响的研究：（原著的 uid, 研究的摘要）。这份文本上的；它是别的原著当前用的译本时，还有原著上的
fn affected(c: &Connection, uid: &str) -> Result<Vec<(String, store::RunSummary)>, String> {
    let mut out: Vec<(String, store::RunSummary)> = store::runs_list(c, uid)?.into_iter().map(|r| (uid.to_string(), r)).collect();
    if let Some(original) = used_by(c, uid)? {
        out.extend(store::runs_list(c, &original)?.into_iter().map(|r| (original.clone(), r)));
    }
    Ok(out)
}

/// 把这份文本当译本用的原著（原著当前用的译本就是它）
fn used_by(c: &Connection, uid: &str) -> Result<Option<String>, String> {
    let Some((original, false)) = store::partner(c, uid)? else { return Ok(None) };
    Ok((store::partner(c, &original)? == Some((uid.to_string(), true))).then_some(original))
}

pub fn existing(c: &Connection, attachment_id: i64) -> Result<Option<Existing>, String> {
    let Some(uid) = store::text_of_attachment(c, attachment_id)? else { return Ok(None) };
    let runs = affected(c, &uid)?;
    Ok(Some(Existing { uid, runs: runs.len(), cards: runs.iter().map(|(_, r)| r.cards.len()).sum() }))
}

/// 同一个附件不存第二份文本（`concept_text_save` 之前查）：已经解析过的走重新解析
pub fn refuse_second(c: &Connection, attachment_id: i64) -> Result<(), String> {
    match store::text_of_attachment(c, attachment_id)? {
        Some(_) => Err("这个附件已经解析过了，请用「重新解析」。".into()),
        None => Ok(()),
    }
}

/// 已经解析过的文本用的段落号写法（重新解析默认沿用，免得段落号全变）
pub fn numbering_of(c: &Connection, attachment_id: i64) -> Result<Option<Numbering>, String> {
    let Some(uid) = store::text_of_attachment(c, attachment_id)? else { return Ok(None) };
    Ok(Numbering::from_key(&store::text_row_get(c, &uid)?.numbering))
}

/// 一组研究重新解析前后用的书（没变的那一本两边是同一份）
struct Shelf<'a> {
    text_uid: String,
    old: Books<'a>,
    new: Books<'a>,
}

fn sources_of(snapshots: &Snapshots, run: &Run) -> Vec<(String, String)> {
    run.sources.iter().filter_map(|s| s.sha256.as_deref().and_then(|sha| snapshots.text(sha).ok()).map(|t| (s.id.clone(), t))).collect()
}

/// 重新解析 `uid` 这份文本：`text` 是新解析出来的。返回每次研究重找锚点的结果
pub fn apply(c: &Connection, backups: &Path, snapshots: &Snapshots, uid: &str, text: &Text, meta: &Meta, source_sha256: &str) -> Result<Reparsed, String> {
    let now = crate::text::now_iso();
    let id = store::text_id(c, uid)?;
    let backup = crate::import::backup_before(c, backups)?;
    let tx = Tx::begin(c)?;

    // 1. 旧的书
    let old_text = store::text_load(&tx, uid)?;
    // 它是原著：当前用的译本和旧的对齐
    let translation: Option<(String, Text, Option<Alignment>)> = match store::partner(&tx, uid)? {
        Some((t, true)) => Some((t.clone(), store::text_load(&tx, &t)?, store::alignment_load(&tx, uid, &t)?)),
        _ => None,
    };
    // 它是译本：原著（有的话），和原著现在是不是用它
    let original: Option<(String, Text, Option<Alignment>, bool)> = match store::partner(&tx, uid)? {
        Some((o, false)) => Some((o.clone(), store::text_load(&tx, &o)?, store::alignment_load(&tx, &o, uid)?, used_by(&tx, uid)?.is_some())),
        _ => None,
    };
    let runs = affected(&tx, uid)?;

    // 2. 换掉文本；段落检索表里先拿掉旧段落（按旧段落的 id 找），再加新的
    search::remove_text(&tx, id)?;
    store::text_replace(&tx, uid, text, meta, source_sha256, &now)?;
    search::add_text(&tx, id)?;

    // 3. 重新对齐（原著 → 每一份译本；译本 → 原著）
    let mut as_original: Option<Alignment> = None;
    for t in store::translations_of(&tx, uid)? {
        let al = match &translation {
            Some((current, zh, _)) if *current == t => align::align(text, zh),
            _ => align::align(text, &store::text_load(&tx, &t)?),
        };
        store::alignment_save(&tx, uid, &t, &al, &now)?;
        if translation.as_ref().is_some_and(|(current, ..)| *current == t) {
            as_original = Some(al);
        }
    }
    let as_translation: Option<Alignment> = match &original {
        Some((o, en, ..)) => {
            let al = align::align(en, text);
            store::alignment_save(&tx, o, uid, &al, &now)?;
            Some(al)
        }
        None => None,
    };

    // 4. 受影响的研究
    let mut shelves: Vec<Shelf> = vec![Shelf {
        text_uid: uid.to_string(),
        old: Books { original: &old_text, translation: translation.as_ref().map(|(_, t, _)| t), alignment: translation.as_ref().and_then(|(.., al)| al.as_ref()) },
        new: Books { original: text, translation: translation.as_ref().map(|(_, t, _)| t), alignment: as_original.as_ref() },
    }];
    if let Some((o, en, al, true)) = &original {
        shelves.push(Shelf { text_uid: o.clone(), old: Books { original: en, translation: Some(&old_text), alignment: al.as_ref() }, new: Books { original: en, translation: Some(text), alignment: as_translation.as_ref() } });
    }
    let mut out = Reparsed { uid: uid.to_string(), backup, paragraphs: text.paragraphs.len(), cards: 0, kept: 0, moved: 0, lost: 0, zh_lost: 0, findings: 0, runs: Vec::new() };
    for shelf in &shelves {
        let again = Reanchor::new(shelf.old, shelf.new);
        for (_, summary) in runs.iter().filter(|(t, _)| *t == shelf.text_uid) {
            let mut run = store::run_load(&tx, &summary.uid)?;
            let texts = sources_of(snapshots, &run);
            let lookup = |id: &str| texts.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone());
            let report = again.run(&mut run, &lookup);
            store::run_save(&tx, &shelf.text_uid, &run)?;
            let findings = audit::audit(&run, &shelf.new, &lookup);
            out.cards += run.cards.len();
            out.kept += report.kept;
            out.moved += report.moved;
            out.lost += report.lost;
            out.zh_lost += report.zh_lost;
            out.findings += findings.len();
            out.runs.push(RunReport {
                uid: run.uid.clone(),
                text_uid: shelf.text_uid.clone(),
                skill_title: run.skill_title.clone(),
                label: summary.label.clone(),
                cards: run.cards.len(),
                kept: report.kept,
                moved: report.moved,
                lost: report.lost,
                zh_lost: report.zh_lost,
                scope_dropped: report.scope_dropped,
                changes: report.changes,
                findings,
            });
        }
    }
    // 研究的名字可能带段落号（「I.i.3 等 2 段」）：用重找之后的
    for (_, summary) in affected(&tx, uid)? {
        if let Some(r) = out.runs.iter_mut().find(|r| r.uid == summary.uid) {
            r.label = summary.label;
        }
    }
    tx.commit()?;
    Ok(out)
}
