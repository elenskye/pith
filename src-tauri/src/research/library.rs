//! 把调查用到的来源加入文献库（`decisions/0020`「加入文献库」）：来源变成一条文献（网页 / 报告），
//! 下载到的原件作为附件（沿用文件库的 SHA-256 去重）。同一个网址已经在库里（没进回收站）的不重复建，直接连上。

use std::path::Path;

use research_engine::model::{Run, Source};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::common::{Tx, err};
use crate::edit::{self, ItemPatch, Target};
use crate::search::DocRef;

pub const TAG: &str = "事实调查";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub key: String,
    pub item_id: i64,
    /// false = 库里已经有这个网址，连上了已有的那条
    pub created: bool,
    pub attachment_id: Option<i64>,
    /// 原件没能作为附件加进来的原因（文献照样建了）
    pub attachment_error: Option<String>,
}

fn live_item(c: &Connection, id: i64) -> Result<bool, String> {
    c.query_row("SELECT 1 FROM items WHERE id = ?1 AND deleted_at IS NULL", [id], |_| Ok(())).optional().map_err(err).map(|r| r.is_some())
}

/// 库里有没有这个网址的文献（不算回收站里的）
pub fn item_with_url(c: &Connection, urls: &[&str]) -> Result<Option<i64>, String> {
    for url in urls.iter().filter(|u| !u.trim().is_empty()) {
        let found = c
            .query_row("SELECT f.item_id FROM item_fields f JOIN items i ON i.id = f.item_id WHERE f.field = 'url' AND f.value = ?1 AND i.deleted_at IS NULL ORDER BY f.item_id LIMIT 1", [url.trim()], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// 文件名里不能有的字符换掉
fn file_name(title: &str, ext: &str) -> String {
    let base: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).take(80).collect();
    let base = base.trim().trim_matches('.');
    format!("{}.{ext}", if base.is_empty() { "来源" } else { base })
}

/// 「2026-11-12T09:00:00.123Z」→ Zotero 的「2026-11-12 09:00:00」（UTC）
fn zotero_time(iso: &str) -> String {
    match iso.get(..19) {
        Some(t) if t.as_bytes().get(10) == Some(&b'T') => t.replacen('T', " ", 1),
        _ => iso.to_string(),
    }
}

fn fields_of(run: &Run, s: &Source) -> Vec<(String, String)> {
    let url = s.final_url.clone().unwrap_or_else(|| s.url.clone());
    let title = if s.title.trim().is_empty() { url.clone() } else { s.title.trim().to_string() };
    let webpage = s.kind.as_deref() == Some("html");
    let mut f = vec![("title".to_string(), title)];
    if is_web(&url) {
        f.push(("url".into(), url));
    }
    if let Some(at) = &s.fetched_at {
        f.push(("accessDate".into(), zotero_time(at)));
    }
    if !s.published.trim().is_empty() {
        f.push(("date".into(), s.published.trim().to_string()));
    }
    if !s.publisher.trim().is_empty() {
        f.push((if webpage { "websiteTitle" } else { "institution" }.into(), s.publisher.trim().to_string()));
    }
    let mut extra = format!("Pith 事实调查：{}（第 {} 次调查的来源 {}）", run.question, run.seq, s.id);
    if let Some(sha) = &s.sha256 {
        extra.push_str(&format!("\n快照 SHA-256：{sha}"));
    }
    f.push(("extra".into(), extra));
    f
}

/// 把这次调查里的 `keys` 这些来源加入文献库。返回每个来源对应的文献，和要更新搜索索引的文档。
/// `original` 给出某个来源的原件快照在哪（没有就不加附件）。
pub fn add(c: &Connection, attachments_dir: &Path, run: &Run, keys: &[String], original: &dyn Fn(&Source) -> Result<std::path::PathBuf, String>) -> Result<(Vec<Added>, Vec<DocRef>), String> {
    if keys.is_empty() {
        return Err("没有选来源。".into());
    }
    let tx = Tx::begin(c)?;
    let (mut out, mut refs) = (Vec::new(), Vec::new());
    for key in keys {
        let s = run.source(key).ok_or_else(|| format!("这次调查里没有来源 {key}"))?;
        if !s.usable() && s.fetch_error.is_some() {
            return Err(format!("{key} 没有下载到，加不了。"));
        }
        // 已经连着一条还在的文献
        if let Some(id) = s.item_id
            && live_item(&tx, id)?
        {
            out.push(Added { key: key.clone(), item_id: id, created: false, attachment_id: None, attachment_error: None });
            continue;
        }
        let urls = [s.final_url.as_deref().unwrap_or(""), s.url.as_str()];
        if let Some(id) = item_with_url(&tx, &urls)? {
            super::store::source_set_item(&tx, &run.uid, key, id)?;
            out.push(Added { key: key.clone(), item_id: id, created: false, attachment_id: None, attachment_error: None });
            continue;
        }
        let item_type = if s.kind.as_deref() == Some("html") || s.kind.is_none() { "webpage" } else { "report" };
        let id = edit::item_create(&tx, item_type, None)?;
        let patch = ItemPatch { fields: fields_of(run, s).into_iter().collect(), ..Default::default() };
        edit::item_update(&tx, id, &patch)?;
        edit::tags_add(&tx, &[Target::item(id)], TAG)?;
        refs.push(DocRef::item(id));
        let (mut attachment_id, mut attachment_error) = (None, None);
        match original(s) {
            Ok(path) => match edit::attachment_add(&tx, attachments_dir, Some(id), &path) {
                Ok((aid, more)) => {
                    let ext = s.ext.as_deref().unwrap_or("bin");
                    edit::attachment_rename(&tx, aid, &file_name(&s.title, ext))?;
                    tx.execute("UPDATE attachments SET original_name = ?1 WHERE id = ?2", params![file_name(&s.title, ext), aid]).map_err(err)?;
                    attachment_id = Some(aid);
                    refs.extend(more);
                }
                Err(e) => attachment_error = Some(e),
            },
            Err(e) => attachment_error = Some(e),
        }
        super::store::source_set_item(&tx, &run.uid, key, id)?;
        out.push(Added { key: key.clone(), item_id: id, created: true, attachment_id, attachment_error });
    }
    tx.commit()?;
    Ok((out, refs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{memory_db, temp_dir};
    use crate::research::download::Downloader;
    use crate::research::store;
    use research_engine::fixture::{MockModel, MockSearch, QUESTION};
    use research_engine::ports::{Ports, Progress};
    use std::sync::atomic::AtomicBool;

    /// 走真实的快照和取正文（模拟的下载），每一步存进库
    fn investigate(c: &Connection, dir: &Path) -> (Run, Downloader) {
        let q = store::question_create(c, QUESTION, "t").unwrap();
        let (planner, worker, search) = (MockModel::new("mock-planner", 1), MockModel::new("mock-worker", 1), MockSearch::new(1));
        let fetch = Downloader::mock(dir, 1);
        let cancel = AtomicBool::new(false);
        let mut recorder = store::tests::DbRecorder { c, question_id: q, saves: 0 };
        let now = || "2026-11-12T09:00:00.000Z".to_string();
        let quiet = |_: Progress| {};
        let mut ports = Ports { planner: &planner, worker: &worker, search: &search, fetch: &fetch, recorder: &mut recorder, now: &now, progress: &quiet, cancel: &cancel };
        let mut run = research_engine::flow::plan(QUESTION, "01RUNLIBRARYAAAAAAAAAAAAAA", 1, None, store::tests::config(), 0, &mut ports).unwrap();
        research_engine::flow::execute(&mut run, &mut ports).unwrap();
        assert_eq!(run.status, research_engine::model::Status::Done);
        (run, fetch)
    }

    #[test]
    fn sources_become_items_with_their_originals_attached() {
        let (c, dir) = (memory_db(), temp_dir("research-library"));
        let attachments = dir.join("attachments");
        let (run, fetch) = investigate(&c, &dir);
        // 走真的快照和取正文，也一样核得上
        assert_eq!(run.verified_facts().count(), 13);
        let original = |s: &Source| fetch.original_path(s);
        let keys: Vec<String> = ["S1", "S3", "S4"].map(String::from).to_vec();
        let (added, refs) = add(&c, &attachments, &run, &keys, &original).unwrap();
        assert_eq!(added.iter().map(|a| (a.key.as_str(), a.created)).collect::<Vec<_>>(), [("S1", true), ("S3", true), ("S4", true)]);
        assert!(added.iter().all(|a| a.attachment_id.is_some() && a.attachment_error.is_none()));
        assert!(refs.len() >= 6);
        let item = crate::library::item_get(&c, added[0].item_id).unwrap();
        let json = serde_json::to_value(&item).unwrap().to_string();
        for part in ["示例市 2024 年国民经济和社会发展统计公报", "https://fixture.example.invalid/stats/bulletin-2024.html", "webpage", "事实调查", "\"name\":\"accessDate\",\"value\":\"20", "快照 SHA-256", "\"containerTitle\":\"示例市统计局\""] {
            assert!(json.contains(part), "缺「{part}」：{json}");
        }
        // 网页之外的是报告；附件是原件、名字可读
        let pdf_item: String = c.query_row("SELECT item_type FROM items WHERE id = ?1", [added[1].item_id], |r| r.get(0)).unwrap();
        assert_eq!(pdf_item, "report");
        let (title, sha): (String, String) = c.query_row("SELECT title, sha256 FROM attachments WHERE id = ?1", [added[1].attachment_id.unwrap()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(title, "示例市青年就业调查报告（2024）.pdf");
        assert_eq!(Some(sha), run.source("S3").unwrap().sha256.clone());
        assert!(attachments.join(&run.source("S3").unwrap().sha256.as_ref().unwrap()[..2]).is_dir());
        // 连上了：调查里记着对应的文献
        let reloaded = store::run_load(&c, &run.uid).unwrap();
        assert_eq!(reloaded.source("S1").unwrap().item_id, Some(added[0].item_id));

        // 再加一次：不重复建
        let (again, _) = add(&c, &attachments, &reloaded, &["S1".to_string()], &original).unwrap();
        assert_eq!((again[0].item_id, again[0].created), (added[0].item_id, false));
        let items: i64 = c.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(items, 3);
        // 库里已经有同一个网址（比如手动建的）：连上那一条
        let existing = edit::item_create(&c, "webpage", None).unwrap();
        edit::item_update(&c, existing, &ItemPatch { fields: [("url".to_string(), run.source("S5").unwrap().url.clone())].into(), ..Default::default() }).unwrap();
        let (linked, _) = add(&c, &attachments, &reloaded, &["S5".to_string()], &original).unwrap();
        assert_eq!((linked[0].item_id, linked[0].created), (existing, false));
        // 下载失败的加不了；不存在的编号报错；整批不改
        let failed = run.sources.iter().find(|s| s.fetch_error.is_some()).unwrap().id.clone();
        assert!(add(&c, &attachments, &reloaded, &["S2".to_string(), failed], &original).unwrap_err().contains("没有下载到"));
        assert!(add(&c, &attachments, &reloaded, &["S99".to_string()], &original).is_err());
        let after: i64 = c.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(after, 4);
        // 快照不见了：文献照样建，附件说明原因
        let (no_file, _) = add(&c, &attachments, &reloaded, &["S2".to_string()], &|_| Err("快照文件不见了".into())).unwrap();
        assert_eq!(no_file[0].attachment_error.as_deref(), Some("快照文件不见了"));
    }

    #[test]
    fn file_names_and_times() {
        assert_eq!(zotero_time("2026-11-12T09:00:00.123Z"), "2026-11-12 09:00:00");
        assert_eq!(zotero_time("昨天"), "昨天");
        assert_eq!(file_name("a/b:c?.pdf", "pdf"), "a_b_c_.pdf.pdf");
        assert_eq!(file_name("  ", "html"), "来源.html");
    }
}
