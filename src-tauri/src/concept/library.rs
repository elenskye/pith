//! 概念研究和文献库之间（`decisions/0024`「书与文献」「反方与后世评论」）：
//! - `DbShelf`：反方那一步「先查文献库」——用检索词搜标题、摘要、笔记、标注（沿用文献库的搜索索引），有 PDF 附件的取正文；
//!   来源正文存一份快照（`数据目录/concept/sources/`），核对、审计都从快照读；
//! - `Copying`：网页检索下载到的快照（v0.3 的下载器存在 `research/sources/`）复制一份到概念研究的快照目录，免得被事实调查的清理删掉；
//! - `add_sources`：网上的来源一键加入文献库（网页条目 + 原件附件，同一网址已有的连上不重建）。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use concept_engine::flow::{Shelf, ShelfHit};
use concept_engine::model::{CommentSource, Run, SourceOrigin};
use research_engine::model::Source;
use research_engine::ports::{Fetch, Fetched};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use zotero_import::util::sha256_hex;

use crate::common::{Tx, err};
use crate::edit::{self, ItemPatch, Target};
use crate::research::snapshot::Snapshots;
use crate::search::{DocKind, DocRef};
use crate::{AppState, library, text};

pub const TAG: &str = "概念研究";
/// 文献库里一条文献最多取多少字的正文
pub const ITEM_TEXT_CHARS: usize = 200_000;

pub fn snapshots(data_dir: &Path) -> Snapshots {
    Snapshots::at(data_dir.join("concept").join("sources"))
}

/// 一条文献给模型看的正文：标题、作者、摘要、笔记、标注，有 PDF 附件的加上 PDF 的文字
pub fn item_text(c: &Connection, attachments_dir: &Path, item_id: i64) -> Result<Option<(String, String, Option<i64>)>, String> {
    let Some((title, abstract_)): Option<(Option<String>, Option<String>)> = c.query_row("SELECT title, abstract FROM items WHERE id = ?1 AND deleted_at IS NULL", [item_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)? else { return Ok(None) };
    let title = title.unwrap_or_default();
    let mut parts = vec![title.clone()];
    let mut st = c.prepare("SELECT COALESCE(literal, TRIM(COALESCE(given, '') || ' ' || COALESCE(family, ''))) FROM creators WHERE item_id = ?1 ORDER BY position").map_err(err)?;
    let creators: Vec<String> = st.query_map([item_id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    if !creators.is_empty() {
        parts.push(creators.join("; "));
    }
    if let Some(a) = abstract_.filter(|a| !a.trim().is_empty()) {
        parts.push(format!("摘要：{a}"));
    }
    let mut st = c.prepare("SELECT html FROM notes WHERE item_id = ?1 AND deleted_at IS NULL ORDER BY id").map_err(err)?;
    for html in st.query_map([item_id], |r| r.get::<_, String>(0)).map_err(err)?.flatten() {
        let t = text::html_to_text(&html);
        if !t.trim().is_empty() {
            parts.push(format!("笔记：{t}"));
        }
    }
    let mut st = c
        .prepare("SELECT n.text, n.comment FROM annotations n JOIN attachments a ON a.id = n.attachment_id WHERE a.item_id = ?1 AND n.deleted_at IS NULL AND a.deleted_at IS NULL ORDER BY a.id, n.sort_key")
        .map_err(err)?;
    for (t, comment) in st.query_map([item_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))).map_err(err)?.flatten() {
        let line = [t.unwrap_or_default(), comment.unwrap_or_default()].into_iter().filter(|x| !x.trim().is_empty()).collect::<Vec<_>>().join(" —— ");
        if !line.is_empty() {
            parts.push(format!("标注：{line}"));
        }
    }
    // 第一个 PDF 附件的正文（沿用 v0.3 的取文字）
    let pdf: Option<i64> = c
        .query_row("SELECT a.id FROM attachments a JOIN files f ON f.sha256 = a.sha256 WHERE a.item_id = ?1 AND a.deleted_at IS NULL AND f.ext = 'pdf' ORDER BY a.id LIMIT 1", [item_id], |r| r.get(0))
        .optional()
        .map_err(err)?;
    if let Some(aid) = pdf
        && let Ok(library::Openable::File(path)) = library::attachment_target(c, attachments_dir, aid)
        && let Ok(bytes) = std::fs::read(&path)
        && let Ok(ex) = crate::research::content::extract(&bytes, Some("application/pdf"), &path.to_string_lossy())
        && let Ok(t) = ex.text
    {
        parts.push(format!("正文：{}", t.chars().take(ITEM_TEXT_CHARS).collect::<String>()));
    }
    Ok(Some((title, parts.join("\n\n"), pdf)))
}

pub struct DbShelf<'a> {
    pub state: &'a AppState,
    pub snapshots: Snapshots,
    /// 不算来源的文献（书本身）
    pub exclude: Vec<i64>,
}

impl Shelf for DbShelf<'_> {
    fn find(&self, terms: &[String], max: usize, _: &AtomicBool) -> Result<Vec<ShelfHit>, String> {
        let attachments = self.state.paths.attachments.clone();
        self.state.with_db(|c| {
            // 每个检索词各搜一遍，按最好的相关度排（bm25 越小越相关）
            let mut best: Vec<(i64, f64)> = Vec::new();
            for t in terms.iter().filter(|t| !t.trim().is_empty()) {
                let Some(hits) = crate::search::query(c, t)? else { continue };
                for (d, score) in hits {
                    if d.kind != DocKind::Item || self.exclude.contains(&d.id) {
                        continue;
                    }
                    match best.iter_mut().find(|(id, _)| *id == d.id) {
                        Some(x) => x.1 = x.1.min(score),
                        None => best.push((d.id, score)),
                    }
                }
            }
            best.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
            let mut out = Vec::new();
            for (id, _) in best {
                if out.len() >= max {
                    break;
                }
                if let Some((title, body, pdf)) = item_text(c, &attachments, id)? {
                    out.push(ShelfHit { item_id: id, attachment_id: pdf, title, text: body });
                }
            }
            Ok(out)
        })
    }

    fn keep(&self, text: &str) -> Result<String, String> {
        let sha = sha256_hex(text.as_bytes());
        self.snapshots.put_text(&sha, text)?;
        Ok(sha)
    }

    fn text(&self, source: &CommentSource) -> Result<String, String> {
        let sha = source.sha256.as_deref().ok_or("这个来源没有正文")?;
        self.snapshots.text(sha)
    }
}

/// 下载器存的快照复制一份到概念研究的快照目录
pub struct Copying<F: Fetch> {
    pub inner: F,
    pub from: Snapshots,
    pub to: Snapshots,
}

impl<F: Fetch> Fetch for Copying<F> {
    fn fetch(&self, url: &str, cancel: &AtomicBool) -> Result<Fetched, String> {
        let f = self.inner.fetch(url, cancel)?;
        if let Ok(path) = self.from.original(&f.sha256, &f.ext)
            && let Ok(bytes) = std::fs::read(&path)
        {
            self.to.put(&bytes, &f.ext)?;
        }
        if let Ok(t) = &f.text {
            self.to.put_text(&f.sha256, t)?;
        }
        Ok(f)
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        self.to.text(source.sha256.as_deref().ok_or("没有快照")?)
    }
}

/// 模拟的下载（开发版、测试）：给合成的评论网页，存进概念研究的快照目录
pub struct MockWeb {
    pub to: Snapshots,
}

impl Fetch for MockWeb {
    fn fetch(&self, url: &str, _: &AtomicBool) -> Result<Fetched, String> {
        if url != concept_engine::mock::WEB_URL {
            return Err("404：找不到这个网页（模拟）".into());
        }
        let page = concept_engine::mock::WEB_PAGE;
        let html = format!("<html><head><title>{}</title></head><body><p>{page}</p></body></html>", concept_engine::mock::WEB_TITLE);
        let sha = self.to.put(html.as_bytes(), "html")?;
        self.to.put_text(&sha, page)?;
        Ok(Fetched { final_url: url.into(), fetched_at: text::now_iso(), sha256: sha, kind: "html".into(), content_type: Some("text/html".into()), ext: "html".into(), bytes: html.len() as u64, title: Some(concept_engine::mock::WEB_TITLE.into()), text: Ok(page.into()), via: None })
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        self.to.text(source.sha256.as_deref().ok_or("没有快照")?)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub key: String,
    pub item_id: i64,
    pub created: bool,
    pub attachment_error: Option<String>,
}

fn file_name(title: &str, ext: &str) -> String {
    let base: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).take(80).collect();
    let base = base.trim().trim_matches('.');
    format!("{}.{ext}", if base.is_empty() { "来源" } else { base })
}

/// 网上的评论来源加入文献库（文献库里的来源本来就在库里，跳过）。返回对应的文献和要更新索引的文档
pub fn add_sources(c: &Connection, attachments_dir: &Path, snaps: &Snapshots, run: &Run, keys: &[String]) -> Result<(Vec<Added>, Vec<DocRef>), String> {
    if keys.is_empty() {
        return Err("没有选来源。".into());
    }
    let tx = Tx::begin(c)?;
    let (mut out, mut refs) = (Vec::new(), Vec::new());
    for key in keys {
        let s = run.sources.iter().find(|s| &s.id == key).ok_or_else(|| format!("这次研究里没有来源 {key}"))?;
        if s.origin == SourceOrigin::Library {
            if let Some(id) = s.item_id {
                out.push(Added { key: key.clone(), item_id: id, created: false, attachment_error: None });
            }
            continue;
        }
        if s.error.is_some() || s.sha256.is_none() {
            return Err(format!("{key} 没有下载到，加不了。"));
        }
        let url = s.final_url.clone().or_else(|| s.url.clone()).unwrap_or_default();
        let urls = [url.as_str(), s.url.as_deref().unwrap_or("")];
        if let Some(id) = s.added_item.filter(|id| tx.query_row("SELECT 1 FROM items WHERE id = ?1 AND deleted_at IS NULL", [id], |_| Ok(())).optional().ok().flatten().is_some()).or(crate::research::library::item_with_url(&tx, &urls)?) {
            super::store::source_set_added(&tx, &run.uid, key, id)?;
            out.push(Added { key: key.clone(), item_id: id, created: false, attachment_error: None });
            continue;
        }
        let id = edit::item_create(&tx, "webpage", None)?;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("title".to_string(), if s.title.trim().is_empty() { url.clone() } else { s.title.trim().to_string() });
        fields.insert("url".to_string(), url.clone());
        if let Some(at) = &s.fetched_at {
            fields.insert("accessDate".to_string(), at.get(..19).unwrap_or(at).replacen('T', " ", 1));
        }
        fields.insert("extra".to_string(), format!("Pith 概念研究：{}（来源 {key}）\n快照 SHA-256：{}", run.skill_title, s.sha256.clone().unwrap_or_default()));
        edit::item_update(&tx, id, &ItemPatch { fields, ..Default::default() })?;
        edit::tags_add(&tx, &[Target::item(id)], TAG)?;
        refs.push(DocRef::item(id));
        let ext = s.ext.clone().unwrap_or_else(|| "html".into());
        let mut attachment_error = None;
        match s.sha256.as_deref().map(|sha| snaps.original(sha, &ext)) {
            Some(Ok(path)) if path.is_file() => match edit::attachment_add(&tx, attachments_dir, Some(id), &path) {
                Ok((aid, more)) => {
                    edit::attachment_rename(&tx, aid, &file_name(&s.title, &ext))?;
                    tx.execute("UPDATE attachments SET original_name = ?1 WHERE id = ?2", params![file_name(&s.title, &ext), aid]).map_err(err)?;
                    refs.extend(more);
                }
                Err(e) => attachment_error = Some(e),
            },
            _ => attachment_error = Some("快照的原件不见了".into()),
        }
        super::store::source_set_added(&tx, &run.uid, key, id)?;
        out.push(Added { key: key.clone(), item_id: id, created: true, attachment_error });
    }
    tx.commit()?;
    Ok((out, refs))
}

pub struct AttachmentFile {
    pub path: PathBuf,
    /// epub / pdf
    pub ext: String,
    pub item_id: Option<i64>,
    pub sha256: String,
    pub name: String,
}

/// 附件文件在哪、是什么格式（解析成文本用）
pub fn attachment_file(c: &Connection, attachments_dir: &Path, attachment_id: i64) -> Result<AttachmentFile, String> {
    let (item_id, sha, ext, name): (Option<i64>, Option<String>, Option<String>, Option<String>) = c
        .query_row("SELECT a.item_id, a.sha256, f.ext, COALESCE(a.title, a.original_name) FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 WHERE a.id = ?1 AND a.deleted_at IS NULL", [attachment_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional()
        .map_err(err)?
        .ok_or("找不到这个附件（可能在回收站里）")?;
    let library::Openable::File(path) = library::attachment_target(c, attachments_dir, attachment_id)? else { return Err("这个附件是网址，不是文件，解析不了。".into()) };
    let ext = ext.unwrap_or_default().to_ascii_lowercase();
    if ext != "epub" && ext != "pdf" {
        return Err("只能解析 EPUB 和 PDF 附件。".into());
    }
    Ok(AttachmentFile { path, ext, item_id, sha256: sha.unwrap_or_default(), name: name.unwrap_or_default() })
}
