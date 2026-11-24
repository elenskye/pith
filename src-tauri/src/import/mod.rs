//! 导入（Phase 2）：Zotero 数据目录副本、BibTeX / RIS / CSL JSON 文件 → library.db。
//! 规则：先 before-import 备份，再在一个事务里写完；写完用确定性核对比较「来源 ↔ 库」，不一致整批回滚，
//! 这次新复制进文件库的附件也删掉。每次导入的数量与报告写进 `import_runs`。

pub mod file;
pub mod formats;
pub mod zotero;

use crate::backup;
use crate::common::{err, new_uid};
use crate::fields::derive_columns;
use crate::text;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const BEFORE_IMPORT: &str = "before-import";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub items: usize,
    pub attachments: usize,
    pub notes: usize,
    pub annotations: usize,
    pub collections: usize,
    /// 这次新建的标签
    pub new_tags: usize,
    pub files_copied: usize,
    /// 文件库里已有同样内容、没有再复制的
    pub files_reused: usize,
    pub bytes_copied: u64,
    pub missing_files: usize,
    /// 进了回收站的（直接删除的条目、附件、笔记、集合）
    pub trash: usize,
    pub skipped: usize,
}

/// 一类数据的核对结果：来源有多少、库里有多少、逐条指纹是否一致。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub category: String,
    pub label: String,
    pub source: usize,
    pub stored: usize,
    pub ok: bool,
    /// 不一致时最多 5 条样例（只有 key / 序号）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diff: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub label: String,
    pub count: i64,
}

/// 有问题的附件文件（缺失、校验值不符），给用户看
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIssue {
    pub key: Option<String>,
    pub title: Option<String>,
    pub parent_title: Option<String>,
    pub reason: String,
    pub in_trash: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Report {
    /// 写入核对：来源（读出的数据）↔ 库
    pub checks: Vec<Check>,
    /// Zotero 读取核对：Zotero 数据库 ↔ 读出的数据（独立 SQL，Phase 0-A 的审计）
    pub source_checks: Vec<Check>,
    pub missing_files: Vec<FileIssue>,
    pub file_warnings: Vec<FileIssue>,
    /// 来源里有、按设计不迁移的数据
    pub not_migrated: Vec<Count>,
    /// 跳过的记录（可能重复、坏记录……），每条一句
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: i64,
    pub source: String,
    pub source_path: Option<String>,
    pub backup_file: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub stats: Stats,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    #[serde(flatten)]
    pub summary: RunSummary,
    pub report: Report,
}

fn summary_of(r: &rusqlite::Row) -> rusqlite::Result<RunSummary> {
    let stats: Option<String> = r.get(6)?;
    Ok(RunSummary {
        id: r.get(0)?,
        source: r.get(1)?,
        source_path: r.get(2)?,
        backup_file: r.get(3)?,
        started_at: r.get(4)?,
        finished_at: r.get(5)?,
        stats: stats.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    })
}

const RUN_COLUMNS: &str = "id, source, source_path, backup_file, started_at, finished_at, stats, report";

/// 导入记录，新的在前。
pub fn runs_list(c: &Connection) -> Result<Vec<RunSummary>, String> {
    let mut st = c.prepare(&format!("SELECT {RUN_COLUMNS} FROM import_runs WHERE finished_at IS NOT NULL ORDER BY id DESC")).map_err(err)?;
    st.query_map([], summary_of).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn runs_get(c: &Connection, id: i64) -> Result<RunDetail, String> {
    c.query_row(&format!("SELECT {RUN_COLUMNS} FROM import_runs WHERE id = ?1"), [id], |r| {
        let report: Option<String> = r.get(7)?;
        Ok(RunDetail { summary: summary_of(r)?, report: report.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() })
    })
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这次导入的记录".to_string())
}

/// 导入前备份（before-import），并清理旧的安全副本。返回备份文件名。
pub(crate) fn backup_before(c: &Connection, backups: &Path) -> Result<String, String> {
    let path = backup::snapshot(c, backups, BEFORE_IMPORT)?;
    backup::prune_safety(backups, backup::KEEP_SAFETY)?;
    Ok(path.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string())
}

/// 一次导入的写入上下文（在事务里）。
pub(crate) struct Writer<'a> {
    pub c: &'a Connection,
    pub run_id: i64,
    pub now: String,
    tags: HashMap<String, i64>,
    pub new_tags: usize,
}

impl<'a> Writer<'a> {
    pub fn begin(c: &'a Connection, source: &str, source_path: &Path, backup_file: &str) -> Result<Self, String> {
        let now = text::now_iso();
        c.execute(
            "INSERT INTO import_runs (source, source_path, backup_file, started_at) VALUES (?1, ?2, ?3, ?4)",
            params![source, source_path.display().to_string(), backup_file, now],
        )
        .map_err(err)?;
        let run_id = c.last_insert_rowid();
        let mut st = c.prepare("SELECT name, id FROM tags").map_err(err)?;
        let tags = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        Ok(Writer { c, run_id, now, tags, new_tags: 0 })
    }

    pub fn finish(&self, stats: &Stats, report: &Report) -> Result<(), String> {
        self.c
            .execute(
                "UPDATE import_runs SET finished_at = ?1, stats = ?2, report = ?3 WHERE id = ?4",
                params![text::now_iso(), serde_json::to_string(stats).map_err(err)?, serde_json::to_string(report).map_err(err)?, self.run_id],
            )
            .map_err(err)?;
        Ok(())
    }

    pub fn tag_id(&mut self, name: &str) -> Result<i64, String> {
        if let Some(id) = self.tags.get(name) {
            return Ok(*id);
        }
        self.c.execute("INSERT INTO tags (name) VALUES (?1)", [name]).map_err(err)?;
        let id = self.c.last_insert_rowid();
        self.tags.insert(name.to_string(), id);
        self.new_tags += 1;
        Ok(id)
    }

    /// 写一条文献：常用列由字段推出，全部字段进 item_fields。返回 id。
    pub fn insert_item(&self, item: &NewItem, fields: &BTreeMap<String, String>) -> Result<i64, String> {
        let cols = derive_columns(fields);
        self.c
            .execute(
                "INSERT INTO items (uid, item_type, title, title_norm, date_raw, date_sort, year, container_title, doi, isbn, language, detected_lang,
                                    abstract, extra, source, source_library, source_key, created_at, updated_at, deleted_at, source_meta, import_run_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    new_uid()?,
                    item.item_type,
                    cols.title,
                    cols.title_norm,
                    cols.date_raw,
                    cols.date_sort,
                    cols.year,
                    cols.container_title,
                    cols.doi,
                    cols.isbn,
                    cols.language,
                    cols.detected_lang,
                    cols.abstract_text,
                    cols.extra,
                    item.source,
                    item.source_library,
                    item.source_key,
                    item.created_at,
                    item.updated_at,
                    item.deleted_at,
                    item.source_meta,
                    self.run_id
                ],
            )
            .map_err(err)?;
        let id = self.c.last_insert_rowid();
        let mut st = self.c.prepare_cached("INSERT INTO item_fields (item_id, field, value) VALUES (?1, ?2, ?3)").map_err(err)?;
        for (k, v) in fields {
            st.execute(params![id, k, v]).map_err(err)?;
        }
        Ok(id)
    }
}

pub(crate) struct NewItem<'a> {
    pub item_type: &'a str,
    pub source: &'a str,
    pub source_library: Option<i64>,
    pub source_key: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<String>,
    pub source_meta: Option<String>,
}

/// 核对用：把两边各自生成的「规范化记录行」逐类比较（数量 + 内容）。
pub(crate) fn compare(category: &str, label: &str, mut source: Vec<String>, mut stored: Vec<String>) -> Check {
    source.sort();
    stored.sort();
    let ok = source == stored;
    let diff = if ok {
        vec![]
    } else {
        let a: std::collections::HashSet<&String> = source.iter().collect();
        let b: std::collections::HashSet<&String> = stored.iter().collect();
        let key = |l: &String| serde_json::from_str::<serde_json::Value>(l).ok().and_then(|v| v.get(0).map(|k| k.to_string())).unwrap_or_default();
        let mut d: Vec<String> = a.difference(&b).map(|l| format!("来源有、库里没有：{}", key(l))).collect();
        d.extend(b.difference(&a).map(|l| format!("库里有、来源没有：{}", key(l))));
        d.sort();
        d.truncate(5);
        d
    };
    Check { category: category.into(), label: label.into(), source: source.len(), stored: stored.len(), ok, diff }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_reports_differences() {
        let l = |s: &str| serde_json::json!([s, 1]).to_string();
        let ok = compare("x", "X", vec![l("A"), l("B")], vec![l("B"), l("A")]);
        assert!(ok.ok && ok.source == 2 && ok.diff.is_empty());
        let bad = compare("x", "X", vec![l("A"), l("B")], vec![l("A")]);
        assert!(!bad.ok);
        assert_eq!(bad.diff, ["来源有、库里没有：\"B\""]);
    }
}
