//! 整库导出与恢复（Phase 6，`decisions/0013`）。
//!
//! 导出 = 一个文件夹：`library.json`（全部表、逐行，格式版本 1）+ `attachments/`（文件库里的文件，布局相同）+ `README.txt`。
//! `library.json` 是通用的表转储：所有表、所有列，以后 migration 新加的列自动包括；每张表记下行数与摘要（SHA-256）。
//! 导出写完马上自检：按导出重建一个临时库，逐表核对，全部文件重算哈希，都对才算导出成功。
//!
//! 恢复（备份或导出）都先在临时库里准备好：检查 → （导出：按导出时的版本建表、写入、逐表核对）→ 升到最新版本；
//! 然后在主库的锁里：「恢复前」备份 → 整库复制进主库（一步完成）→ 重建搜索索引。主库打不开时也能恢复。

use crate::backup;
use crate::common::{err, new_uid};
use crate::db;
use crate::import::Check;
use crate::store;
use rusqlite::Connection;
use rusqlite::types::{Value, ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// 导出格式的标识：定下就不改，不跟着 App 改名（decisions/0014）
pub const FORMAT: &str = "research-lab-library";
pub const FORMAT_VERSION: i64 = 1;
pub const JSON_NAME: &str = "library.json";
pub const FILES_DIR: &str = "attachments";

/// 界面上的中文名，也是核对表的顺序；不认识的表（以后的 migration）排在后面，用表名
const LABELS: &[(&str, &str)] = &[
    ("items", "文献"),
    ("item_fields", "文献字段"),
    ("creators", "作者"),
    ("item_relations", "文献关联"),
    ("collections", "集合"),
    ("collection_items", "集合里的文献"),
    ("collection_attachments", "集合里的附件"),
    ("collection_notes", "集合里的笔记"),
    ("tags", "标签"),
    ("item_tags", "文献的标签"),
    ("attachment_tags", "附件的标签"),
    ("note_tags", "笔记的标签"),
    ("annotation_tags", "标注的标签"),
    ("attachments", "附件"),
    ("files", "附件文件记录"),
    ("notes", "笔记"),
    ("annotations", "标注"),
    ("reader_states", "阅读位置"),
    ("metadata_lookups", "联网补全记录"),
    ("duplicate_dismissals", "「不是重复」"),
    ("import_runs", "导入记录"),
    ("settings", "设置"),
];

fn label(table: &str) -> String {
    LABELS.iter().find(|(t, _)| *t == table).map_or_else(|| table.to_string(), |(_, l)| l.to_string())
}

fn order(table: &str) -> usize {
    LABELS.iter().position(|(t, _)| *t == table).unwrap_or(LABELS.len())
}

// ---------------- 通用的表转储 ----------------

/// 一张表：列（按表结构的顺序）与每行的规范化 JSON（`{"列":值,…}`，列序固定）。
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<String>,
}

impl Table {
    /// 摘要：行排序后逐行 SHA-256，与行的先后无关
    pub fn digest(&self) -> Digest {
        let mut sorted: Vec<&String> = self.rows.iter().collect();
        sorted.sort();
        let mut h = Sha256::new();
        for r in sorted {
            h.update(r.as_bytes());
            h.update(b"\n");
        }
        Digest { rows: self.rows.len(), sha256: zotero_import::util::hex(&h.finalize()) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub rows: usize,
    pub sha256: String,
}

pub fn table_names(c: &Connection) -> Result<Vec<String>, String> {
    let mut st = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn columns(c: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut st = c.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid").map_err(err)?;
    st.query_map([table], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn cell(table: &str, col: &str, v: ValueRef) -> Result<String, String> {
    Ok(match v {
        ValueRef::Null => "null".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) if f.is_finite() => serde_json::to_string(&f).map_err(err)?,
        ValueRef::Real(_) => return Err(format!("{table}.{col} 里有不是有限数的值，导不出来")),
        ValueRef::Text(t) => serde_json::to_string(std::str::from_utf8(t).map_err(|_| format!("{table}.{col} 里有不是 UTF-8 的文字"))?).map_err(err)?,
        ValueRef::Blob(_) => return Err(format!("{table}.{col} 是二进制数据，格式版本 1 不支持")),
    })
}

fn line(columns: &[String], cells: &[String]) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in columns.iter().zip(cells).enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&serde_json::to_string(k).unwrap_or_default());
        s.push(':');
        s.push_str(v);
    }
    s.push('}');
    s
}

/// 读出一张表的全部行（只取 `columns` 这几列；按 rowid，与写入的先后一致）。
pub fn dump_table(c: &Connection, table: &str, columns: &[String]) -> Result<Table, String> {
    let cols = columns.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
    let mut st = c.prepare(&format!("SELECT {cols} FROM {} ORDER BY rowid", quote(table))).map_err(err)?;
    let mut rows = Vec::new();
    let mut q = st.query([]).map_err(err)?;
    while let Some(r) = q.next().map_err(err)? {
        let cells = (0..columns.len()).map(|i| cell(table, &columns[i], r.get_ref(i).map_err(err)?)).collect::<Result<Vec<_>, _>>()?;
        rows.push(line(columns, &cells));
    }
    Ok(Table { columns: columns.to_vec(), rows })
}

/// 整个库：表名 → 表
pub fn dump(c: &Connection) -> Result<BTreeMap<String, Table>, String> {
    let mut out = BTreeMap::new();
    for t in table_names(c)? {
        let cols = columns(c, &t)?;
        out.insert(t.clone(), dump_table(c, &t, &cols)?);
    }
    Ok(out)
}

fn json_cell(table: &str, col: &str, v: &J) -> Result<(String, Value), String> {
    Ok(match v {
        J::Null => ("null".into(), Value::Null),
        J::Number(n) if n.is_i64() => {
            let i = n.as_i64().unwrap_or_default();
            (i.to_string(), Value::Integer(i))
        }
        J::Number(n) => {
            let f = n.as_f64().filter(|f| f.is_finite()).ok_or_else(|| format!("{table}.{col} 的数字超出范围"))?;
            (serde_json::to_string(&f).map_err(err)?, Value::Real(f))
        }
        J::String(s) => (serde_json::to_string(s).map_err(err)?, Value::Text(s.clone())),
        _ => return Err(format!("{table}.{col} 的值不是文字、数字或空")),
    })
}

/// 导出里的一行 → 规范化 JSON 行 + 要写入的值。`strict`：行里不能有表里没有的列、也不能缺列。
fn parse_row(table: &str, columns: &[String], row: &serde_json::Map<String, J>, strict: bool) -> Result<(String, Vec<Value>), String> {
    if strict && let Some(k) = row.keys().find(|k| !columns.contains(k)) {
        return Err(format!("{table} 里有这个版本没有的列 {k}"));
    }
    let mut cells = Vec::with_capacity(columns.len());
    let mut values = Vec::with_capacity(columns.len());
    for col in columns {
        let v = match row.get(col) {
            Some(v) => v,
            None if strict => return Err(format!("{table} 的一行缺少列 {col}")),
            None => &J::Null,
        };
        let (c, v) = json_cell(table, col, v)?;
        cells.push(c);
        values.push(v);
    }
    Ok((line(columns, &cells), values))
}

/// 把行写进（空的）库：一个事务，期间关掉外键检查，写完再整体查一遍外键。返回各表的规范化行（用于核对）。
pub fn load(c: &mut Connection, tables: &BTreeMap<String, Vec<serde_json::Map<String, J>>>, strict: bool) -> Result<BTreeMap<String, Table>, String> {
    let have: HashSet<String> = table_names(c)?.into_iter().collect();
    if strict && let Some(t) = tables.keys().find(|t| !have.contains(*t)) {
        return Err(format!("导出里有这个版本没有的表 {t}"));
    }
    if strict && let Some(t) = have.iter().find(|t| !tables.contains_key(*t)) {
        return Err(format!("导出不完整：缺少「{}」", label(t)));
    }
    c.execute_batch("PRAGMA foreign_keys = OFF").map_err(err)?;
    let result = (|| {
        let tx = c.transaction().map_err(err)?;
        let mut out = BTreeMap::new();
        for (t, rows) in tables.iter().filter(|(t, _)| have.contains(*t)) {
            let cols = columns(&tx, t)?;
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                quote(t),
                cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
                (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ")
            );
            let mut st = tx.prepare(&sql).map_err(err)?;
            let mut lines = Vec::with_capacity(rows.len());
            for (i, row) in rows.iter().enumerate() {
                let (l, values) = parse_row(t, &cols, row, strict)?;
                st.execute(rusqlite::params_from_iter(values)).map_err(|e| format!("写入{}第 {} 行失败：{e}", label(t), i + 1))?;
                lines.push(l);
            }
            out.insert(t.clone(), Table { columns: cols, rows: lines });
        }
        let broken: i64 = tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).map_err(err)?;
        if broken > 0 {
            return Err(format!("有 {broken} 处引用对不上（比如附件指向不存在的文献）"));
        }
        tx.commit().map_err(err)?;
        Ok(out)
    })();
    c.execute_batch("PRAGMA foreign_keys = ON").map_err(err)?;
    result
}

/// 逐表核对：`expected`（来源）↔ `actual`（库里）。不一致时列出最多 5 行的开头。
pub fn compare(expected: &BTreeMap<String, Digest>, actual: &BTreeMap<String, Table>) -> Vec<Check> {
    let mut names: Vec<&String> = expected.keys().chain(actual.keys()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    names.sort_by_key(|n| (order(n), n.to_string()));
    names
        .into_iter()
        .map(|t| {
            let want = expected.get(t);
            let got = actual.get(t).map(Table::digest);
            let ok = want.is_some() && want == got.as_ref();
            Check {
                category: t.clone(),
                label: label(t),
                source: want.map_or(0, |d| d.rows),
                stored: got.as_ref().map_or(0, |d| d.rows),
                ok,
                diff: if ok { vec![] } else { vec![format!("{}：行数或内容对不上", label(t))] },
            }
        })
        .collect()
}

// ---------------- 数量 ----------------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub items: usize,
    pub attachments: usize,
    pub notes: usize,
    pub annotations: usize,
    pub collections: usize,
    pub tags: usize,
    /// 附件文件（文件库里的）个数与总大小
    pub files: usize,
    pub bytes: u64,
}

fn counts(rows: impl Fn(&str) -> usize, bytes: u64) -> Counts {
    Counts {
        items: rows("items"),
        attachments: rows("attachments"),
        notes: rows("notes"),
        annotations: rows("annotations"),
        collections: rows("collections"),
        tags: rows("tags"),
        files: rows("files"),
        bytes,
    }
}

fn library_counts(c: &Connection) -> Result<Counts, String> {
    let n = |t: &str| c.query_row(&format!("SELECT COUNT(*) FROM {}", quote(t)), [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
    let bytes: i64 = c.query_row("SELECT COALESCE(SUM(size), 0) FROM files", [], |r| r.get(0)).map_err(err)?;
    Ok(counts(n, bytes as u64))
}

/// 文件库里的文件：sha256、扩展名、大小（`files` 表）
fn file_rows(c: &Connection) -> Result<Vec<(String, String, u64)>, String> {
    let mut st = c.prepare("SELECT sha256, ext, size FROM files ORDER BY rowid").map_err(err)?;
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

// ---------------- 导出 ----------------

/// 在主库的锁里读出的全部内容（之后的写文件、自检不占锁）
pub struct Snapshot {
    schema: i64,
    tables: BTreeMap<String, Table>,
    files: Vec<(String, String, u64)>,
    /// sha256 → 原文件名（报告缺失文件用）
    names: std::collections::HashMap<String, String>,
    counts: Counts,
    /// 文件夹名用的本地时间
    stamp: String,
}

pub fn snapshot(c: &Connection) -> Result<Snapshot, String> {
    let mut st = c.prepare("SELECT sha256, COALESCE(original_name, title, sha256) FROM attachments WHERE sha256 IS NOT NULL").map_err(err)?;
    let names = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    Ok(Snapshot { schema: db::user_version(c)?, tables: dump(c)?, files: file_rows(c)?, names, counts: library_counts(c)?, stamp: backup::local_stamp()? })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// 导出文件夹
    pub path: String,
    pub counts: Counts,
    /// 自检：库里 ↔ 按导出重建的临时库，逐表
    pub checks: Vec<Check>,
    /// 文件库里本来就找不到、没能导出的文件（原文件名或 sha256 开头）
    pub missing_files: Vec<String>,
}

fn write_json(path: &Path, s: &Snapshot, app_version: &str) -> Result<(), String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    let digests: BTreeMap<&String, Digest> = s.tables.iter().map(|(k, t)| (k, t.digest())).collect();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "\"format\": {},", J::from(FORMAT));
    let _ = writeln!(out, "\"formatVersion\": {FORMAT_VERSION},");
    let _ = writeln!(out, "\"schemaVersion\": {},", s.schema);
    let _ = writeln!(out, "\"appVersion\": {},", J::from(app_version));
    let _ = writeln!(out, "\"exportedAt\": {},", J::from(crate::text::now_iso()));
    let _ = writeln!(out, "\"digests\": {},", serde_json::to_string(&digests).map_err(err)?);
    let _ = writeln!(out, "\"tables\": {{");
    for (i, (name, t)) in s.tables.iter().enumerate() {
        let _ = writeln!(out, "{}: [", J::from(name.as_str()));
        for (j, r) in t.rows.iter().enumerate() {
            out.push_str(r);
            out.push_str(if j + 1 < t.rows.len() { ",\n" } else { "\n" });
        }
        let _ = writeln!(out, "]{}", if i + 1 < s.tables.len() { "," } else { "" });
    }
    out.push_str("}\n}\n");
    std::fs::write(path, out).map_err(|e| format!("写不了 {JSON_NAME}：{e}"))
}

fn readme(s: &Snapshot, app_version: &str) -> String {
    let c = &s.counts;
    format!(
        "Pith 整库导出\n\n\
         导出时间：{}（App {app_version}，数据库第 {} 版，导出格式版本 {FORMAT_VERSION}）\n\
         内容：文献 {} 条、附件 {} 个、笔记 {} 条、标注 {} 条、集合 {} 个、附件文件 {} 个（{:.1} MB）。\n\n\
         - library.json：整个文献库。tables 下每张表一个列表、一行一条记录，列的含义见项目文档 docs/schema.md；\n  \
         digests 是每张表的行数与摘要，恢复时用来核对。\n\
         - attachments/：附件文件，按内容的 SHA-256 存放（attachments/<前两位>/<sha256>.<扩展名>），\n  \
         与 library.json 里的 files 表一一对应；原文件名在 attachments 表的 original_name 列。\n\n\
         恢复：Pith → 设置 → 整库导出 →「从导出恢复…」，选这个文件夹。\n\
         恢复会替换当时的整个库，替换前自动备份。\n",
        s.stamp,
        s.schema,
        c.items,
        c.attachments,
        c.notes,
        c.annotations,
        c.collections,
        c.files,
        c.bytes as f64 / 1_048_576.0
    )
}

/// 写出导出文件夹（在 `parent` 下新建 `Pith 导出 <时间>`），写完自检。自检不过就删掉，报错。
pub fn export(s: &Snapshot, attachments: &Path, parent: &Path, app_version: &str) -> Result<ExportReport, String> {
    if !parent.is_dir() {
        return Err("选的位置不是文件夹".into());
    }
    let name = format!("Pith 导出 {}", s.stamp);
    let mut target = parent.join(&name);
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = parent.join(format!("{name}-{n}"));
    }
    let partial = parent.join(format!(".{name}.partial-{}", new_uid()?));
    let result = (|| {
        std::fs::create_dir_all(partial.join(FILES_DIR)).map_err(|e| format!("无法创建导出文件夹：{e}"))?;
        let mut missing = Vec::new();
        for (sha, ext, _) in &s.files {
            let rel = store::rel_path(sha, ext);
            let src = attachments.join(&rel);
            if !src.is_file() {
                missing.push(sha.clone());
                continue;
            }
            let dst = partial.join(FILES_DIR).join(&rel);
            std::fs::create_dir_all(dst.parent().ok_or("附件路径不对")?).map_err(|e| format!("无法创建导出文件夹：{e}"))?;
            let (got, _, _) = store::copy_hashing(&src, &dst)?;
            if &got != sha {
                let _ = std::fs::remove_file(&dst);
                missing.push(sha.clone());
            }
        }
        write_json(&partial.join(JSON_NAME), s, app_version)?;
        std::fs::write(partial.join("README.txt"), readme(s, app_version)).map_err(|e| format!("写不了说明文件：{e}"))?;
        // 自检：按导出完整地准备一次恢复（读、核对摘要、重建临时库、逐表核对、重算全部文件的哈希），
        // 重建出来的每张表都要与库里的一模一样
        let p = prepare_export(&partial, None, true)?;
        let live: BTreeMap<String, Digest> = s.tables.iter().map(|(k, t)| (k.clone(), t.digest())).collect();
        let checks = compare(&live, &p.rebuilt);
        if let Some(bad) = checks.iter().find(|c| !c.ok) {
            return Err(format!("导出自检没通过（{}），已删除这次导出", bad.label));
        }
        let missing_after: HashSet<&String> = p.missing.iter().collect();
        if missing_after.len() != missing.len() || missing.iter().any(|m| !missing_after.contains(m)) {
            return Err("导出自检没通过：附件文件对不上，已删除这次导出".into());
        }
        Ok((checks, missing))
    })();
    match result {
        Ok((checks, missing)) => {
            std::fs::rename(&partial, &target).map_err(|e| format!("导出写完了，但改不了文件夹名：{e}（在 {}）", partial.display()))?;
            let missing_files = missing.iter().map(|m| s.names.get(m).cloned().unwrap_or_else(|| m[..12].to_string())).collect();
            Ok(ExportReport { path: target.display().to_string(), counts: s.counts.clone(), checks, missing_files })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&partial);
            Err(e)
        }
    }
}

// ---------------- 读导出 ----------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    schema_version: i64,
    app_version: String,
    exported_at: String,
    digests: BTreeMap<String, Digest>,
    tables: BTreeMap<String, Vec<serde_json::Map<String, J>>>,
}

fn read_envelope(dir: &Path) -> Result<Envelope, String> {
    let path = dir.join(JSON_NAME);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { format!("这个文件夹里没有 {JSON_NAME}，不是 Pith 的整库导出") } else { format!("读不了 {JSON_NAME}：{e}") }
    })?;
    let v: J = serde_json::from_str(&text).map_err(|e| format!("{JSON_NAME} 不完整或已损坏：{e}"))?;
    if v.get("format").and_then(J::as_str) != Some(FORMAT) {
        return Err(format!("{JSON_NAME} 不是 Pith 的整库导出"));
    }
    match v.get("formatVersion").and_then(J::as_i64) {
        Some(FORMAT_VERSION) => {}
        Some(n) if n > FORMAT_VERSION => return Err(format!("这份导出的格式版本（{n}）比这个 App 新，请用新版 App 恢复")),
        _ => return Err(format!("{JSON_NAME} 的格式版本不对")),
    }
    let env: Envelope = serde_json::from_value(v).map_err(|e| format!("{JSON_NAME} 的内容不对：{e}"))?;
    if env.schema_version < 1 || env.schema_version > db::latest() {
        return Err(format!("这份导出的数据库版本（第 {} 版）比这个 App 新（支持到第 {} 版），请用新版 App 恢复", env.schema_version, db::latest()));
    }
    if env.digests.keys().ne(env.tables.keys()) {
        return Err(format!("{JSON_NAME} 不完整：表与校验值对不上"));
    }
    Ok(env)
}

fn file_size(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len())
}

/// 一个文件在导出里（大小对）、在文件库里已有，还是都没有
enum FileAt {
    Export(PathBuf),
    Store,
    Missing,
}

fn locate(dir: &Path, store_root: Option<&Path>, sha: &str, ext: &str, size: u64) -> FileAt {
    let rel = store::rel_path(sha, ext);
    let p = dir.join(FILES_DIR).join(&rel);
    if file_size(&p) == Some(size) {
        return FileAt::Export(p);
    }
    if store_root.is_some_and(|r| file_size(&r.join(&rel)) == Some(size)) {
        return FileAt::Store;
    }
    FileAt::Missing
}

fn envelope_files(env: &Envelope) -> Result<Vec<(String, String, u64)>, String> {
    let mut out = Vec::new();
    for row in env.tables.get("files").into_iter().flatten() {
        let s = |k: &str| row.get(k).and_then(J::as_str).map(str::to_string);
        let (Some(sha), Some(ext), Some(size)) = (s("sha256"), s("ext"), row.get("size").and_then(J::as_u64)) else {
            return Err(format!("{JSON_NAME} 里的附件文件记录不对"));
        };
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) || ext.contains(['/', '\\']) || ext.starts_with('.') {
            return Err(format!("{JSON_NAME} 里的附件文件记录不对"));
        }
        out.push((sha, ext, size));
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    pub path: String,
    pub exported_at: String,
    pub app_version: String,
    pub schema_version: i64,
    pub counts: Counts,
    /// 导出里没有（或大小不对）、文件库里也没有的附件文件个数
    pub missing_files: usize,
}

/// 恢复前的预览：读 library.json、核对每张表的摘要、看附件文件在不在（只看大小，恢复时再算哈希）。
pub fn inspect(dir: &Path, store_root: &Path) -> Result<ExportPreview, String> {
    let env = read_envelope(dir)?;
    verify_envelope(&env)?;
    let files = envelope_files(&env)?;
    let missing = files.iter().filter(|(sha, ext, size)| matches!(locate(dir, Some(store_root), sha, ext, *size), FileAt::Missing)).count();
    let bytes = files.iter().map(|f| f.2).sum();
    Ok(ExportPreview {
        path: dir.display().to_string(),
        exported_at: env.exported_at.clone(),
        app_version: env.app_version.clone(),
        schema_version: env.schema_version,
        counts: counts(|t| env.tables.get(t).map_or(0, Vec::len), bytes),
        missing_files: missing,
    })
}

/// library.json 的内容与它记下的摘要一致（没有被改过、没有截断）
fn verify_envelope(env: &Envelope) -> Result<(), String> {
    let scratch = Connection::open_in_memory().map_err(err)?;
    let mut c = scratch;
    db::upgrade(&mut c, &db::MIGRATIONS[..env.schema_version as usize], None)?;
    for (t, rows) in &env.tables {
        let cols = columns(&c, t)?;
        if cols.is_empty() {
            return Err(format!("导出里有这个版本没有的表 {t}"));
        }
        let lines = rows.iter().map(|r| parse_row(t, &cols, r, true).map(|(l, _)| l)).collect::<Result<Vec<_>, _>>()?;
        if Some(&Table { columns: cols, rows: lines }.digest()) != env.digests.get(t) {
            return Err(format!("{JSON_NAME} 里的「{}」与导出时记下的校验值对不上：文件可能被改过或损坏了", label(t)));
        }
    }
    Ok(())
}

// ---------------- 恢复 ----------------

/// 临时文件：离开作用域时删除（连同 SQLite 的附属文件）
struct TmpFile(PathBuf);

impl Drop for TmpFile {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

/// 准备好的恢复来源：一个已升到最新版本的临时库（离开作用域时删除），以及要放进文件库的附件。
pub struct Prepared {
    // 字段按声明顺序释放：先关连接，再删临时文件
    conn: Connection,
    _tmp: TmpFile,
    /// 备份的文件名 / 导出文件夹
    pub source: String,
    pub from_schema: i64,
    /// 从导出恢复：逐表核对（导出 ↔ 重建的库）
    pub checks: Vec<Check>,
    /// 从导出恢复：按导出时的版本重建出来的各表（升级之前）
    rebuilt: BTreeMap<String, Table>,
    /// 导出里要放进文件库的文件：(导出里的路径, sha256, 扩展名)
    files: Vec<(PathBuf, String, String)>,
    /// 导出里没有、文件库里也没有的文件（sha256）
    missing: Vec<String>,
}

fn open_temp() -> Result<(Connection, TmpFile), String> {
    let tmp = TmpFile(std::env::temp_dir().join(format!("researchlab-restore-{}.db", new_uid()?)));
    let c = Connection::open(&tmp.0).map_err(|e| format!("无法创建临时数据库：{e}"))?;
    c.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA foreign_keys = ON;").map_err(err)?;
    Ok((c, tmp))
}

/// 准备从备份恢复：复制到临时库 → 检查 → 升到最新版本。
pub fn prepare_backup(backups: &Path, file_name: &str) -> Result<Prepared, String> {
    let path = backup::resolve(backups, file_name)?;
    let (src, from) = backup::open_checked(&path, db::latest())?;
    let (mut conn, tmp) = open_temp()?;
    backup::copy_into(&src, &mut conn)?;
    drop(src);
    db::upgrade(&mut conn, db::MIGRATIONS, None).map_err(|e| format!("这份备份升级到新版本时出错：{e}"))?;
    Ok(Prepared { conn, _tmp: tmp, source: file_name.to_string(), from_schema: from, checks: vec![], rebuilt: BTreeMap::new(), files: vec![], missing: vec![] })
}

/// 准备从导出恢复：读 → 核对摘要 → 按导出时的版本建临时库并写入 → 逐表核对 → 升到最新版本。
/// `store_root`：文件库（导出里缺的文件，文件库里有也行）。`hash_all`：现在就重算导出里全部文件的哈希（导出自检用；恢复时放进文件库那一步会算）。
pub fn prepare_export(dir: &Path, store_root: Option<&Path>, hash_all: bool) -> Result<Prepared, String> {
    let env = read_envelope(dir)?;
    let (conn, tmp) = open_temp()?;
    let mut p = Prepared {
        conn,
        _tmp: tmp,
        source: dir.display().to_string(),
        from_schema: env.schema_version,
        checks: vec![],
        rebuilt: BTreeMap::new(),
        files: vec![],
        missing: vec![],
    };
    db::upgrade(&mut p.conn, &db::MIGRATIONS[..env.schema_version as usize], None)?;
    let written = load(&mut p.conn, &env.tables, true)?;
    // 1. 导出里的行 ↔ 导出时记下的摘要（文件没被改过）；2. 读回来 ↔ 摘要（写入的值与读出的一致，例如类型没变）
    let written_ok = compare(&env.digests, &written);
    p.rebuilt = dump(&p.conn)?;
    p.checks = compare(&env.digests, &p.rebuilt);
    for (c, w) in p.checks.iter_mut().zip(&written_ok) {
        c.ok = c.ok && w.ok;
    }
    if let Some(bad) = p.checks.iter().find(|c| !c.ok) {
        return Err(format!("按导出重建的库与导出对不上（{}），没有恢复", bad.label));
    }
    for (sha, ext, size) in envelope_files(&env)? {
        match locate(dir, store_root, &sha, &ext, size) {
            FileAt::Export(path) => {
                if hash_all {
                    let (got, _) = zotero_import::util::hash_file(&path).map_err(|e| format!("读不了 {}：{e}", path.display()))?;
                    if got != sha {
                        p.missing.push(sha);
                        continue;
                    }
                }
                p.files.push((path, sha, ext));
            }
            FileAt::Store => {}
            FileAt::Missing => p.missing.push(sha),
        }
    }
    db::upgrade(&mut p.conn, db::MIGRATIONS, None).map_err(|e| format!("这份导出升级到新版本时出错：{e}"))?;
    Ok(p)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// 备份的文件名 / 导出文件夹
    pub source: String,
    /// 恢复前自动做的备份（文件名）
    pub safety_backup: Option<String>,
    pub from_schema: i64,
    pub to_schema: i64,
    pub counts: Counts,
    /// 从导出恢复：逐表核对（导出 ↔ 库里）
    pub checks: Vec<Check>,
    pub files_copied: usize,
    /// 恢复后库里记着、文件库里没有的附件文件
    pub missing_files: usize,
}

/// 在主库的锁里执行：附件放进文件库 → 整库复制进主库。调用前已做好「恢复前」备份。
pub fn apply(live: &mut Connection, attachments: &Path, p: Prepared, safety_backup: Option<String>) -> Result<RestoreReport, String> {
    let mut copied = 0;
    for (src, sha, ext) in &p.files {
        // 哈希不符（导出里的文件坏了）：不放进文件库，算作缺失
        if let Ok(put) = store::put(attachments, src, sha, ext) {
            copied += usize::from(put.created);
        }
    }
    backup::copy_into(&p.conn, live)?;
    let missing = file_rows(live)?.iter().filter(|(sha, ext, size)| file_size(&attachments.join(store::rel_path(sha, ext))) != Some(*size)).count();
    Ok(RestoreReport {
        source: p.source.clone(),
        safety_backup,
        from_schema: p.from_schema,
        to_schema: db::user_version(live)?,
        counts: library_counts(live)?,
        checks: p.checks.clone(),
        files_copied: copied,
        missing_files: missing,
    })
}

#[cfg(test)]
pub(crate) mod tests;
