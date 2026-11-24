//! 误差报告：用一套**独立于 reader** 的 SQL 查询，从源数据库逐类生成规范化记录行，
//! 与读出的模型生成的记录行比较（数量 + SHA-256 指纹）。任何一类不一致即判定为数据丢失。
//! 文件层面另做存在性、MD5（对比 Zotero 的 storageHash）与 storage 目录孤儿检查。

use crate::model::*;
use crate::reader::{LIBRARY_FILTER, ZOTERO_CACHE_FILES, has_table, list_files, pick_table};
use crate::util::{now_utc, sha256_hex, value_to_string};
use crate::{Error, Result};
use rusqlite::Connection;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// 本原型实际验证过的 Zotero userdata schema 版本。
pub const TESTED_USERDATA_VERSIONS: &[i64] = &[125];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub generated_at: String,
    pub client_version: Option<String>,
    pub schema_versions: BTreeMap<String, i64>,
    /// pass / pass_with_warnings / fail
    pub verdict: String,
    pub checks: Vec<CategoryCheck>,
    pub type_counts: BTreeMap<String, usize>,
    pub trash: TrashStats,
    pub files: FileStats,
    pub file_issues: Vec<FileIssue>,
    pub storage_orphan_dirs: Vec<String>,
    /// 原型不迁移的数据：表名 → 行数（表不存在为空）。
    pub not_migrated: BTreeMap<String, Option<i64>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryCheck {
    pub category: String,
    pub label: String,
    pub source_count: usize,
    pub parsed_count: usize,
    pub source_fingerprint: String,
    pub parsed_fingerprint: String,
    pub ok: bool,
    /// 不一致时列出最多 5 条差异样例（仅 key，不含正文）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff_sample: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrashStats {
    pub items_deleted_directly: usize,
    pub items_trashed_via_parent: usize,
    pub collections_deleted_directly: usize,
    pub collections_trashed_via_ancestor: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileStats {
    pub attachments: usize,
    pub by_link_mode: BTreeMap<String, usize>,
    pub stored: usize,
    pub stored_exists: usize,
    pub md5_match: usize,
    pub md5_mismatch: usize,
    pub md5_no_storage_hash: usize,
    pub linked: usize,
    pub linked_exists: usize,
    pub base_relative: usize,
    pub base_relative_exists: usize,
    pub no_file: usize,
    pub total_bytes: u64,
    pub extra_files: usize,
    pub duplicate_content_groups: usize,
    pub storage_dirs: usize,
    pub cache_files: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileIssue {
    pub attachment_key: String,
    /// stored_missing / md5_mismatch / linked_missing / base_relative_unresolved / base_relative_missing / path_unrecognized
    pub kind: String,
    pub in_trash: bool,
}

type Lines = Vec<String>;

fn line(v: J) -> String {
    v.to_string()
}

fn s(v: Value) -> J {
    match value_to_string(v) {
        Some(x) => J::String(x),
        None => J::Null,
    }
}

fn fingerprint(lines: &mut Lines) -> String {
    lines.sort();
    sha256_hex(lines.join("\n").as_bytes())
}

fn query_lines(conn: &Connection, sql: &str, ncols: usize) -> Result<Lines> {
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map([], |r| {
        let mut v = Vec::with_capacity(ncols);
        for i in 0..ncols {
            v.push(s(r.get::<_, Value>(i)?));
        }
        Ok(line(J::Array(v)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn count(conn: &Connection, table: &str) -> Result<Option<i64>> {
    if !has_table(conn, table)? {
        return Ok(None);
    }
    Ok(Some(conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| r.get(0))?))
}

/// 源数据库侧：每一类的规范化记录行。
fn source_lines(conn: &Connection) -> Result<Vec<(&'static str, &'static str, Lines)>> {
    let tt = pick_table(conn, "itemTypesCombined", "itemTypes")?;
    let ft = pick_table(conn, "fieldsCombined", "fields")?;
    let lib = format!("i.libraryID IN ({LIBRARY_FILTER})");
    let type_expr = "COALESCE(t.typeName, 'itemType#' || i.itemTypeID)";
    let mut out = Vec::new();

    out.push(("items", "条目（全部类型）", query_lines(conn, &format!(
        "SELECT i.key, {type_expr}, i.libraryID, i.dateAdded, i.dateModified, i.clientDateModified, i.version, i.synced != 0
         FROM items i LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID WHERE {lib}"), 8)?));

    out.push(("fields", "字段值", query_lines(conn, &format!(
        "SELECT i.key, COALESCE(f.fieldName, 'field#' || d.fieldID), COALESCE(v.value, '')
         FROM itemData d JOIN items i ON i.itemID = d.itemID
         LEFT JOIN {ft} f ON f.fieldID = d.fieldID LEFT JOIN itemDataValues v ON v.valueID = d.valueID WHERE {lib}"), 3)?));

    out.push(("creators", "作者 / 编者", query_lines(conn, &format!(
        "SELECT i.key, COALESCE(ct.creatorType, 'creatorType#' || ic.creatorTypeID), c.firstName, c.lastName, COALESCE(c.fieldMode, 0), ic.orderIndex
         FROM itemCreators ic JOIN items i ON i.itemID = ic.itemID
         LEFT JOIN creators c ON c.creatorID = ic.creatorID LEFT JOIN creatorTypes ct ON ct.creatorTypeID = ic.creatorTypeID WHERE {lib}"), 6)?));

    out.push(("tags", "标签关联", query_lines(conn, &format!(
        "SELECT i.key, COALESCE(t.name, ''), it.type FROM itemTags it JOIN items i ON i.itemID = it.itemID
         LEFT JOIN tags t ON t.tagID = it.tagID WHERE {lib}"), 3)?));

    out.push(("collections", "集合", query_lines(conn, &format!(
        "SELECT c.key, c.collectionName, p.key, c.libraryID, c.clientDateModified, c.version, c.synced != 0
         FROM collections c LEFT JOIN collections p ON p.collectionID = c.parentCollectionID
         WHERE c.libraryID IN ({LIBRARY_FILTER})"), 7)?));

    out.push(("collection_items", "集合成员", query_lines(conn, &format!(
        "SELECT i.key, COALESCE(c.key, 'collection#' || ci.collectionID) FROM collectionItems ci
         JOIN items i ON i.itemID = ci.itemID LEFT JOIN collections c ON c.collectionID = ci.collectionID WHERE {lib}"), 2)?));

    let rel = if has_table(conn, "itemRelations")? {
        query_lines(conn, &format!(
            "SELECT i.key, COALESCE(p.predicate, 'predicate#' || r.predicateID), r.object FROM itemRelations r
             JOIN items i ON i.itemID = r.itemID LEFT JOIN relationPredicates p ON p.predicateID = r.predicateID WHERE {lib}"), 3)?
    } else { vec![] };
    out.push(("item_relations", "条目关联", rel));

    let crel = if has_table(conn, "collectionRelations")? {
        query_lines(conn, &format!(
            "SELECT c.key, COALESCE(p.predicate, 'predicate#' || r.predicateID), r.object FROM collectionRelations r
             JOIN collections c ON c.collectionID = r.collectionID LEFT JOIN relationPredicates p ON p.predicateID = r.predicateID
             WHERE c.libraryID IN ({LIBRARY_FILTER})"), 3)?
    } else { vec![] };
    out.push(("collection_relations", "集合关联", crel));

    // 附件：列名在不同版本可能缺失，逐列判断
    let acol = |c: &str| -> Result<String> {
        Ok(if crate::reader::has_column(conn, "itemAttachments", c)? { format!("a.{c}") } else { "NULL".into() })
    };
    out.push(("attachments", "附件记录", query_lines(conn, &format!(
        "SELECT i.key, p.key, a.linkMode, a.contentType, CASE WHEN a.charsetID IS NULL THEN NULL ELSE COALESCE(cs.charset, 'charset#' || a.charsetID) END, a.path, {}, {}, {}, {}, {}
         FROM itemAttachments a JOIN items i ON i.itemID = a.itemID
         LEFT JOIN items p ON p.itemID = a.parentItemID LEFT JOIN charsets cs ON cs.charsetID = a.charsetID
         LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID WHERE {lib} AND t.typeName = 'attachment'",
        acol("syncState")?, acol("storageModTime")?, acol("storageHash")?, acol("lastProcessedModificationTime")?, acol("lastRead")?), 11)?));

    out.push(("notes", "笔记", query_lines(conn, &format!(
        "SELECT i.key, p.key, n.title, n.note FROM itemNotes n JOIN items i ON i.itemID = n.itemID
         LEFT JOIN items p ON p.itemID = n.parentItemID LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID
         WHERE {lib} AND t.typeName = 'note'"), 4)?));

    out.push(("attachment_notes", "附件自带笔记", query_lines(conn, &format!(
        "SELECT i.key, n.note FROM itemNotes n JOIN items i ON i.itemID = n.itemID
         LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID
         WHERE {lib} AND t.typeName = 'attachment' AND n.note IS NOT NULL"), 2)?));

    out.push(("other_item_notes", "普通条目上的旧式笔记", query_lines(conn, &format!(
        "SELECT i.key, n.note FROM itemNotes n JOIN items i ON i.itemID = n.itemID
         LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID
         WHERE {lib} AND COALESCE(t.typeName, '') NOT IN ('note', 'attachment')"), 2)?));

    let ann = if has_table(conn, "itemAnnotations")? {
        let ac = |c: &str| -> Result<String> {
            Ok(if crate::reader::has_column(conn, "itemAnnotations", c)? { format!("a.{c}") } else { "NULL".into() })
        };
        query_lines(conn, &format!(
            "SELECT i.key, p.key, a.type, {}, a.text, a.comment, a.color, a.pageLabel, a.sortIndex, a.position, COALESCE({}, 0) != 0
             FROM itemAnnotations a JOIN items i ON i.itemID = a.itemID LEFT JOIN items p ON p.itemID = a.parentItemID
             LEFT JOIN {tt} t ON t.itemTypeID = i.itemTypeID WHERE {lib} AND t.typeName = 'annotation'",
            ac("authorName")?, ac("isExternal")?), 11)?
    } else { vec![] };
    out.push(("annotations", "PDF 标注", ann));

    out.push(("deleted_items", "回收站条目", query_lines(conn, &format!(
        "SELECT i.key, d.dateDeleted FROM deletedItems d JOIN items i ON i.itemID = d.itemID WHERE {lib}"), 2)?));

    let dc = if has_table(conn, "deletedCollections")? {
        query_lines(conn, &format!(
            "SELECT c.key, d.dateDeleted FROM deletedCollections d JOIN collections c ON c.collectionID = d.collectionID
             WHERE c.libraryID IN ({LIBRARY_FILTER})"), 2)?
    } else { vec![] };
    out.push(("deleted_collections", "回收站集合", dc));
    Ok(out)
}

fn o<T: Serialize>(v: &T) -> J {
    serde_json::to_value(v).unwrap_or(J::Null)
}

fn os(v: &Option<String>) -> J {
    v.as_ref().map(|x| J::String(x.clone())).unwrap_or(J::Null)
}

fn oi(v: &Option<i64>) -> J {
    v.map(|x| J::String(x.to_string())).unwrap_or(J::Null)
}

/// 模型侧：与 source_lines 同样的格式。所有数字都按字符串比较（源侧经 value_to_string）。
fn parsed_lines(lib: &ZoteroLibrary) -> HashMap<&'static str, Lines> {
    let mut m: HashMap<&'static str, Lines> = HashMap::new();
    let n = |x: i64| J::String(x.to_string());
    let b = |x: bool| J::String(if x { "1".into() } else { "0".into() });
    for base in lib.all_bases() {
        let k = J::String(base.key.clone());
        m.entry("items").or_default().push(line(json!([k, base.item_type, n(base.library_id), base.date_added, base.date_modified, base.client_date_modified, n(base.version), b(base.synced)])));
        for (f, v) in &base.fields {
            m.entry("fields").or_default().push(line(json!([k, f, v])));
        }
        for c in &base.creators {
            m.entry("creators").or_default().push(line(json!([k, c.creator_type, os(&c.first_name), os(&c.last_name), n(c.field_mode), n(c.order_index)])));
        }
        for t in &base.tags {
            m.entry("tags").or_default().push(line(json!([k, t.name, n(t.tag_type)])));
        }
        for c in &base.collection_keys {
            m.entry("collection_items").or_default().push(line(json!([k, c])));
        }
        for r in &base.relations {
            m.entry("item_relations").or_default().push(line(json!([k, r.predicate, r.object])));
        }
        if let Some(d) = &base.deleted_at {
            m.entry("deleted_items").or_default().push(line(json!([k, d])));
        }
    }
    for c in &lib.collections {
        m.entry("collections").or_default().push(line(json!([c.key, c.name, os(&c.parent_key), n(c.library_id), c.client_date_modified, n(c.version), b(c.synced)])));
        for r in &c.relations {
            m.entry("collection_relations").or_default().push(line(json!([c.key, r.predicate, r.object])));
        }
        if let Some(d) = &c.deleted_at {
            m.entry("deleted_collections").or_default().push(line(json!([c.key, d])));
        }
    }
    for a in &lib.attachments {
        m.entry("attachments").or_default().push(line(json!([a.base.key, os(&a.parent_key), oi(&a.link_mode_raw), os(&a.content_type), os(&a.charset), os(&a.path), oi(&a.sync_state), oi(&a.storage_mod_time), os(&a.storage_hash), oi(&a.last_processed_modification_time), oi(&a.last_read)])));
        if let Some(nt) = &a.note {
            m.entry("attachment_notes").or_default().push(line(json!([a.base.key, nt])));
        }
    }
    for nt in &lib.notes {
        m.entry("notes").or_default().push(line(json!([nt.base.key, os(&nt.parent_key), os(&nt.title), os(&nt.html)])));
    }
    for a in &lib.annotations {
        m.entry("annotations").or_default().push(line(json!([a.base.key, os(&a.parent_key), n(a.annotation_type_raw), os(&a.author_name), os(&a.text), os(&a.comment), os(&a.color), os(&a.page_label), os(&a.sort_index), os(&a.position), b(a.is_external)])));
    }
    m
}

fn diff_sample(src: &Lines, parsed: &Lines) -> Vec<String> {
    let a: HashSet<&String> = src.iter().collect();
    let b: HashSet<&String> = parsed.iter().collect();
    let key = |l: &String| -> String {
        serde_json::from_str::<J>(l).ok().and_then(|v| v.get(0).and_then(|x| x.as_str().map(String::from))).unwrap_or_default()
    };
    let mut out: Vec<String> = a.difference(&b).map(|l| format!("源有、模型无：{}", key(l))).collect();
    out.extend(b.difference(&a).map(|l| format!("模型有、源无：{}", key(l))));
    out.sort();
    out.truncate(5);
    out
}

pub fn audit(conn: &Connection, lib: &ZoteroLibrary, storage: &Path) -> Result<Report> {
    let mut warnings = lib.warnings.clone();
    let mut parsed = parsed_lines(lib);
    let mut checks = Vec::new();
    for (cat, label, mut src) in source_lines(conn)? {
        let mut mine = parsed.remove(cat).unwrap_or_default();
        let sf = fingerprint(&mut src);
        let pf = fingerprint(&mut mine);
        let ok = sf == pf && src.len() == mine.len();
        checks.push(CategoryCheck {
            category: cat.into(),
            label: label.into(),
            source_count: src.len(),
            parsed_count: mine.len(),
            source_fingerprint: sf,
            parsed_fingerprint: pf,
            ok,
            diff_sample: if ok { vec![] } else { diff_sample(&src, &mine) },
        });
    }

    // 类型分布
    let mut type_counts = BTreeMap::new();
    for b in lib.all_bases() {
        *type_counts.entry(b.item_type.clone()).or_insert(0) += 1;
    }

    // 回收站
    let mut trash = TrashStats::default();
    for b in lib.all_bases() {
        if b.deleted_at.is_some() {
            trash.items_deleted_directly += 1;
        } else if b.in_trash {
            trash.items_trashed_via_parent += 1;
        }
    }
    for c in &lib.collections {
        if c.deleted_at.is_some() {
            trash.collections_deleted_directly += 1;
        } else if c.in_trash {
            trash.collections_trashed_via_ancestor += 1;
        }
    }

    // 文件
    let mut files = FileStats { attachments: lib.attachments.len(), ..Default::default() };
    let mut issues = Vec::new();
    let mut by_sha: HashMap<&str, usize> = HashMap::new();
    for a in &lib.attachments {
        let mode = a.link_mode.map(|m| o(&m).as_str().unwrap_or("unknown").to_string()).unwrap_or_else(|| "none".into());
        *files.by_link_mode.entry(mode).or_insert(0) += 1;
        files.extra_files += a.extra_files.len();
        let issue = |kind: &str| FileIssue { attachment_key: a.base.key.clone(), kind: kind.into(), in_trash: a.base.in_trash };
        let Some(f) = &a.file else {
            if a.path.is_some() && a.link_mode != Some(LinkMode::LinkedUrl) {
                issues.push(issue("path_unrecognized"));
            }
            files.no_file += 1;
            continue;
        };
        if f.exists {
            files.total_bytes += f.size.unwrap_or(0);
            if let Some(h) = &f.sha256 {
                *by_sha.entry(h.as_str()).or_insert(0) += 1;
            }
        }
        match f.kind {
            FileKind::Stored => {
                files.stored += 1;
                if f.exists {
                    files.stored_exists += 1;
                    match f.md5_matches_storage_hash {
                        Some(true) => files.md5_match += 1,
                        Some(false) => {
                            files.md5_mismatch += 1;
                            issues.push(issue("md5_mismatch"));
                        }
                        None => files.md5_no_storage_hash += 1,
                    }
                } else {
                    issues.push(issue("stored_missing"));
                }
            }
            FileKind::Linked => {
                files.linked += 1;
                if f.exists {
                    files.linked_exists += 1;
                } else {
                    issues.push(issue("linked_missing"));
                }
            }
            FileKind::BaseRelative => {
                files.base_relative += 1;
                if f.exists {
                    files.base_relative_exists += 1;
                } else if f.resolved_path.is_none() {
                    issues.push(issue("base_relative_unresolved"));
                } else {
                    issues.push(issue("base_relative_missing"));
                }
            }
        }
    }
    files.duplicate_content_groups = by_sha.values().filter(|n| **n > 1).count();
    issues.sort_by(|a, b| (&a.kind, &a.attachment_key).cmp(&(&b.kind, &b.attachment_key)));

    // storage 目录：孤儿目录与缓存文件
    let att_keys: HashSet<&str> = lib.attachments.iter().map(|a| a.base.key.as_str()).collect();
    let mut orphans = Vec::new();
    if storage.is_dir() {
        for e in std::fs::read_dir(storage).map_err(|e| Error::Io(e, storage.to_path_buf()))? {
            let e = e.map_err(|e| Error::Io(e, storage.to_path_buf()))?;
            let name = e.file_name().to_string_lossy().to_string();
            if !e.path().is_dir() {
                continue;
            }
            files.storage_dirs += 1;
            if !att_keys.contains(name.as_str()) {
                orphans.push(name.clone());
            }
            let mut all = Vec::new();
            list_files(&e.path(), &e.path(), &mut all).map_err(|er| Error::Io(er, e.path()))?;
            for f in all {
                if ZOTERO_CACHE_FILES.contains(&f.as_str()) {
                    *files.cache_files.entry(f).or_insert(0) += 1;
                }
            }
        }
    } else {
        warnings.push(format!("storage 目录不存在：{}", storage.display()));
    }
    orphans.sort();

    // 不迁移的数据
    let mut not_migrated = BTreeMap::new();
    for t in ["fulltextItems", "fulltextWords", "savedSearches", "feeds", "feedItems", "syncedSettings", "groups", "publicationsItems", "retractedItems", "customItemTypes", "customFields", "proxies", "settings"] {
        not_migrated.insert(t.to_string(), count(conn, t)?);
    }
    let excluded_items: i64 = conn.query_row(&format!("SELECT count(*) FROM items WHERE libraryID NOT IN ({LIBRARY_FILTER})"), [], |r| r.get(0))?;
    not_migrated.insert("items(feed 库)".to_string(), Some(excluded_items));

    if let Some(v) = lib.source.schema_versions.get("userdata")
        && !TESTED_USERDATA_VERSIONS.contains(v) {
            warnings.push(format!("userdata schema 版本 {v} 未经本原型验证（已验证：{TESTED_USERDATA_VERSIONS:?}）"));
        }

    let failed = checks.iter().any(|c| !c.ok);
    let verdict = if failed {
        "fail"
    } else if !issues.is_empty() || !orphans.is_empty() || !warnings.is_empty() {
        "pass_with_warnings"
    } else {
        "pass"
    };

    Ok(Report {
        generated_at: now_utc(),
        client_version: lib.source.client_version.clone(),
        schema_versions: lib.source.schema_versions.clone(),
        verdict: verdict.into(),
        checks,
        type_counts,
        trash,
        files,
        file_issues: issues,
        storage_orphan_dirs: orphans,
        not_migrated,
        warnings,
    })
}

fn issue_label(kind: &str) -> &str {
    match kind {
        "stored_missing" => "storage 中找不到文件",
        "md5_mismatch" => "MD5 与 Zotero 记录不符",
        "linked_missing" => "链接文件不存在",
        "base_relative_unresolved" => "相对路径无法解析（未提供链接附件根目录）",
        "base_relative_missing" => "相对路径文件不存在",
        "path_unrecognized" => "路径格式无法识别",
        _ => kind,
    }
}

/// 中文 Markdown 报告。只含数量、状态与 key，不含标题、正文、文件名。
pub fn render_markdown(r: &Report) -> String {
    let mut m = String::new();
    let verdict = match r.verdict.as_str() {
        "pass" => "通过：所有类别无丢失，无文件问题",
        "pass_with_warnings" => "通过（有提示）：所有类别无丢失，存在文件问题或提示",
        _ => "失败：存在数据丢失",
    };
    m.push_str("# Zotero 迁移误差报告\n\n");
    m.push_str(&format!("- 生成时间（UTC）：{}\n", r.generated_at));
    m.push_str(&format!("- Zotero 版本：{}\n", r.client_version.as_deref().unwrap_or("未知")));
    m.push_str(&format!("- userdata schema：{}\n", r.schema_versions.get("userdata").map(|v| v.to_string()).unwrap_or("未知".into())));
    m.push_str(&format!("- **结论：{verdict}**\n\n"));

    m.push_str("## 逐类核对（源数据库 vs 读出的模型）\n\n| 类别 | 源 | 模型 | 指纹一致 |\n| --- | ---: | ---: | :---: |\n");
    for c in &r.checks {
        m.push_str(&format!("| {} | {} | {} | {} |\n", c.label, c.source_count, c.parsed_count, if c.ok { "✓" } else { "✗" }));
    }
    for c in r.checks.iter().filter(|c| !c.ok) {
        m.push_str(&format!("\n**{} 差异样例**\n\n", c.label));
        for d in &c.diff_sample {
            m.push_str(&format!("- {d}\n"));
        }
    }

    m.push_str("\n## 条目类型\n\n| 类型 | 数量 |\n| --- | ---: |\n");
    let mut tc: Vec<_> = r.type_counts.iter().collect();
    tc.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (t, n) in tc {
        m.push_str(&format!("| {t} | {n} |\n"));
    }

    let t = &r.trash;
    m.push_str(&format!(
        "\n## 回收站\n\n| 项 | 数量 |\n| --- | ---: |\n| 条目：直接删除 | {} |\n| 条目：因父条目在回收站 | {} |\n| 集合：直接删除 | {} |\n| 集合：因上级集合在回收站 | {} |\n",
        t.items_deleted_directly, t.items_trashed_via_parent, t.collections_deleted_directly, t.collections_trashed_via_ancestor
    ));

    let f = &r.files;
    m.push_str("\n## 附件文件\n\n| 项 | 数量 |\n| --- | ---: |\n");
    m.push_str(&format!("| 附件记录 | {} |\n", f.attachments));
    for (k, v) in &f.by_link_mode {
        m.push_str(&format!("| 　链接方式 {k} | {v} |\n"));
    }
    m.push_str(&format!("| 存入 storage 的文件 | {} |\n| 　存在 | {} |\n| 　MD5 与 Zotero 记录一致 | {} |\n| 　MD5 不一致 | {} |\n| 　Zotero 无 MD5 记录 | {} |\n", f.stored, f.stored_exists, f.md5_match, f.md5_mismatch, f.md5_no_storage_hash));
    m.push_str(&format!("| 链接的外部文件 | {} |\n| 　存在 | {} |\n| 相对根目录的链接文件 | {} |\n| 　存在 | {} |\n| 无文件（网址链接等） | {} |\n", f.linked, f.linked_exists, f.base_relative, f.base_relative_exists, f.no_file));
    m.push_str(&format!("| 存在文件总大小 | {:.1} MB |\n| 附件目录中的附加文件 | {} |\n| 内容完全相同的文件组 | {} |\n| storage 子目录 | {} |\n| storage 孤儿目录（无对应附件） | {} |\n", f.total_bytes as f64 / 1048576.0, f.extra_files, f.duplicate_content_groups, f.storage_dirs, r.storage_orphan_dirs.len()));
    for (k, v) in &f.cache_files {
        m.push_str(&format!("| Zotero 缓存文件 {k} | {v} |\n"));
    }

    if !r.file_issues.is_empty() {
        m.push_str("\n### 文件问题\n\n| 附件 key | 问题 | 在回收站 |\n| --- | --- | :---: |\n");
        for i in &r.file_issues {
            m.push_str(&format!("| {} | {} | {} |\n", i.attachment_key, issue_label(&i.kind), if i.in_trash { "是" } else { "" }));
        }
    }
    if !r.storage_orphan_dirs.is_empty() {
        m.push_str(&format!("\n孤儿目录：{}\n", r.storage_orphan_dirs.join("、")));
    }

    m.push_str("\n## 本原型不迁移的数据\n\n| 表 | 行数 |\n| --- | ---: |\n");
    for (k, v) in &r.not_migrated {
        m.push_str(&format!("| {k} | {} |\n", v.map(|x| x.to_string()).unwrap_or("（无此表）".into())));
    }

    if !r.warnings.is_empty() {
        m.push_str("\n## 提示\n\n");
        for w in &r.warnings {
            m.push_str(&format!("- {w}\n"));
        }
    }
    m
}
