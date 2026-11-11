//! BibTeX / RIS / CSL JSON 文件导入（写入层）。解析在 `formats`；这里负责查重、写库、写入核对。
//! 每条记录的原文存进 items.source_meta（`raw`），映射不到的字段以「格式:原名」进 item_fields，保证不丢。

use super::formats::{self, Format, Parsed, Record};
use super::{NewItem, Report, RunDetail, Stats, Writer, backup_before, compare, runs_get};
use crate::common::{Tx, err, new_uid};
use crate::paths::DataPaths;
use crate::text::{self, DateParts};
use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// 大于这个的文件不当作文献数据（防止误选了 PDF、数据库之类的大文件）
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Options {
    /// 跳过与库里（或本文件前面）可能重复的记录
    pub skip_duplicates: bool,
    /// 把这次导入的文献放进新集合「导入：文件名」
    pub into_collection: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCount {
    pub item_type: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Duplicate {
    /// 第几条（从 1 起）
    pub index: usize,
    pub title: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub path: String,
    pub file_name: String,
    /// bibtex / ris / csl-json
    pub format: String,
    pub records: usize,
    pub by_type: Vec<TypeCount>,
    pub duplicates: Vec<Duplicate>,
    /// 前几条的标题
    pub samples: Vec<String>,
    pub notes: usize,
    pub warnings: Vec<String>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn load(path: &Path) -> Result<(Format, Parsed), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读不了这个文件：{e}"))?;
    if meta.len() > MAX_BYTES {
        return Err(format!("文件太大（{} MB），不像是文献数据文件", meta.len() / 1024 / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读不了这个文件：{e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 编码。请在导出时选择 UTF-8（Unicode），或先用文本编辑器另存为 UTF-8。".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let format = formats::detect_format(&file_name(path), text).ok_or("认不出文件格式。支持 BibTeX（.bib）、RIS（.ris）、CSL JSON（.json）。")?;
    let parsed = formats::parse(format, text)?;
    if parsed.records.is_empty() {
        return Err(if parsed.warnings.is_empty() {
            "文件里没有找到文献记录".to_string()
        } else {
            format!("文件里没有可导入的记录：{}", parsed.warnings.join("；"))
        });
    }
    Ok((format, parsed))
}

/// 记录的完整字段：解析出的字段 + 日期（Zotero 两段式「排序值 原文」，与 Zotero 导入的条目一致）。
fn full_fields(r: &Record) -> BTreeMap<String, String> {
    let mut f = r.fields.clone();
    if let Some(d) = &r.date {
        let parts = if d.year.is_some() {
            DateParts {
                year: d.year,
                month: d.month,
                day: d.day,
            }
        } else {
            text::parse_date(&d.raw)
        };
        f.insert("date".into(), text::multipart(parts, &d.raw));
    }
    f
}

/// 每条记录可能重复的原因（与库里未删除的文献比 DOI、规范化标题 + 年份；也与本文件前面的记录比）。
fn duplicates(c: &Connection, records: &[Record]) -> Result<Vec<Option<String>>, String> {
    let mut dois: HashSet<String> = HashSet::new();
    let mut st = c
        .prepare("SELECT lower(doi) FROM items WHERE deleted_at IS NULL AND doi IS NOT NULL")
        .map_err(err)?;
    for d in st.query_map([], |r| r.get::<_, String>(0)).map_err(err)? {
        dois.insert(d.map_err(err)?);
    }
    let mut titles: HashSet<(String, Option<i64>)> = HashSet::new();
    let mut st = c
        .prepare("SELECT title_norm, year FROM items WHERE deleted_at IS NULL AND title_norm IS NOT NULL")
        .map_err(err)?;
    for t in st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)? {
        titles.insert(t.map_err(err)?);
    }
    let mut out = Vec::with_capacity(records.len());
    for r in records {
        let cols = crate::fields::derive_columns(&full_fields(r));
        let doi = cols.doi.as_ref().map(|d| d.to_lowercase());
        let title = cols.title_norm.clone().map(|t| (t, cols.year));
        let reason = if doi.as_ref().is_some_and(|d| dois.contains(d)) {
            Some(format!("DOI 相同（{}）", cols.doi.as_deref().unwrap_or("")))
        } else if title.as_ref().is_some_and(|t| titles.contains(t)) {
            Some("标题与年份相同".to_string())
        } else {
            None
        };
        dois.extend(doi);
        titles.extend(title);
        out.push(reason);
    }
    Ok(out)
}

fn record_title(r: &Record) -> Option<String> {
    r.fields.get("title").cloned()
}

pub fn preview(c: &Connection, path: &Path) -> Result<Preview, String> {
    let (format, parsed) = load(path)?;
    let dups = duplicates(c, &parsed.records)?;
    let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &parsed.records {
        *by_type.entry(&r.item_type).or_default() += 1;
    }
    let mut by_type: Vec<TypeCount> = by_type.into_iter().map(|(t, n)| TypeCount { item_type: t.into(), count: n }).collect();
    by_type.sort_by(|a, b| b.count.cmp(&a.count).then(a.item_type.cmp(&b.item_type)));
    let mut warnings = parsed.warnings.clone();
    for (i, r) in parsed.records.iter().enumerate() {
        for w in &r.warnings {
            warnings.push(format!("第 {} 条「{}」：{w}", i + 1, record_title(r).unwrap_or_default()));
        }
    }
    Ok(Preview {
        path: path.display().to_string(),
        file_name: file_name(path),
        format: format.source_name().into(),
        records: parsed.records.len(),
        by_type,
        duplicates: dups
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                d.as_ref().map(|reason| Duplicate {
                    index: i + 1,
                    title: record_title(&parsed.records[i]),
                    reason: reason.clone(),
                })
            })
            .collect(),
        samples: parsed.records.iter().filter_map(record_title).take(5).collect(),
        notes: parsed.records.iter().map(|r| r.notes.len()).sum(),
        warnings,
    })
}

pub fn run(c: &Connection, paths: &DataPaths, path: &Path, opts: &Options) -> Result<RunDetail, String> {
    let (format, parsed) = load(path)?;
    import_parsed(c, paths, path, format, &parsed, opts)
}

pub(crate) fn import_parsed(c: &Connection, paths: &DataPaths, path: &Path, format: Format, parsed: &Parsed, opts: &Options) -> Result<RunDetail, String> {
    let dups = duplicates(c, &parsed.records)?;
    let backup_file = backup_before(c, &paths.backups)?;
    let tx = Tx::begin(c)?;
    let source = format.source_name();
    let mut w = Writer::begin(&tx, source, path, &backup_file)?;
    let mut stats = Stats::default();
    let mut report = Report {
        warnings: parsed.warnings.clone(),
        ..Default::default()
    };

    let collection = if opts.into_collection {
        tx.execute(
            "INSERT INTO collections (uid, name, created_at, updated_at, import_run_id) VALUES (?1, ?2, ?3, ?3, ?4)",
            params![new_uid()?, format!("导入：{}", file_name(path)), w.now, w.run_id],
        )
        .map_err(err)?;
        stats.collections += 1;
        Some(tx.last_insert_rowid())
    } else {
        None
    };

    let mut written: Vec<usize> = Vec::new();
    for (i, r) in parsed.records.iter().enumerate() {
        let title = record_title(r);
        if let (true, Some(reason)) = (opts.skip_duplicates, &dups[i]) {
            stats.skipped += 1;
            report.skipped.push(format!(
                "第 {} 条「{}」可能重复（{reason}），没有导入",
                i + 1,
                title.clone().unwrap_or_default()
            ));
            continue;
        }
        for warning in &r.warnings {
            report
                .warnings
                .push(format!("第 {} 条「{}」：{warning}", i + 1, title.clone().unwrap_or_default()));
        }
        let mut meta = json!({ "index": i, "raw": r.raw });
        if !r.warnings.is_empty() {
            meta["warnings"] = json!(r.warnings);
        }
        let id = w.insert_item(
            &NewItem {
                item_type: &r.item_type,
                source,
                source_library: None,
                source_key: r.source_key.as_deref(),
                created_at: &w.now,
                updated_at: &w.now,
                deleted_at: None,
                source_meta: Some(meta.to_string()),
            },
            &full_fields(r),
        )?;
        for (pos, cr) in r.creators.iter().enumerate() {
            tx.execute(
                "INSERT INTO creators (item_id, role, family, given, literal, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, cr.role, cr.family, cr.given, cr.literal, pos as i64],
            )
            .map_err(err)?;
        }
        for t in &r.tags {
            let tid = w.tag_id(t)?;
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag_id, kind) VALUES (?1, ?2, 'manual')",
                params![id, tid],
            )
            .map_err(err)?;
        }
        for n in &r.notes {
            let first = n.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(80).collect::<String>());
            tx.execute(
                "INSERT INTO notes (uid, item_id, title, html, created_at, updated_at, import_run_id) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                params![new_uid()?, id, first, text::text_to_html(n), w.now, w.run_id],
            )
            .map_err(err)?;
            stats.notes += 1;
        }
        if let Some(cid) = collection {
            tx.execute("INSERT INTO collection_items (collection_id, item_id) VALUES (?1, ?2)", params![cid, id])
                .map_err(err)?;
        }
        stats.items += 1;
        written.push(i);
    }
    stats.new_tags = w.new_tags;

    // 写入核对：解析出的记录 ↔ 库（按记录序号对齐）
    let mut source_lines = record_lines(parsed, &written);
    let mut stored = stored_lines(&tx, w.run_id)?;
    for (cat, label) in CATEGORIES {
        report.checks.push(compare(
            cat,
            label,
            source_lines.remove(cat).unwrap_or_default(),
            stored.remove(cat).unwrap_or_default(),
        ));
    }
    let bad: Vec<&str> = report.checks.iter().filter(|x| !x.ok).map(|x| x.label.as_str()).collect();
    if !bad.is_empty() {
        return Err(format!(
            "写入后核对不一致（{}），已整批撤销，库里没有任何改动。请把这个情况告诉开发者。",
            bad.join("、")
        ));
    }
    w.finish(&stats, &report)?;
    let detail = runs_get(&tx, w.run_id)?;
    tx.commit()?;
    Ok(detail)
}

const CATEGORIES: &[(&str, &str)] = &[
    ("records", "文献条目"),
    ("fields", "字段值"),
    ("creators", "作者 / 编者"),
    ("tags", "标签"),
    ("notes", "子笔记"),
];

type Lines = HashMap<&'static str, Vec<String>>;

fn opt(v: &Option<String>) -> J {
    v.as_ref().map(|s| J::String(s.clone())).unwrap_or(J::Null)
}

fn record_lines(parsed: &Parsed, written: &[usize]) -> Lines {
    let mut m: Lines = HashMap::new();
    let mut push = |cat: &'static str, v: J| m.entry(cat).or_default().push(v.to_string());
    for &i in written {
        let r = &parsed.records[i];
        push("records", json!([i, r.item_type, r.source_key, r.raw]));
        for (k, v) in full_fields(r) {
            push("fields", json!([i, k, v]));
        }
        for (pos, cr) in r.creators.iter().enumerate() {
            push("creators", json!([i, pos, cr.role, opt(&cr.family), opt(&cr.given), opt(&cr.literal)]));
        }
        let mut seen = HashSet::new();
        for t in r.tags.iter().filter(|t| seen.insert(t.as_str())) {
            push("tags", json!([i, t]));
        }
        for n in &r.notes {
            push("notes", json!([i, text::text_to_html(n)]));
        }
    }
    m
}

fn stored_lines(c: &Connection, run: i64) -> Result<Lines, String> {
    let mut m: Lines = HashMap::new();
    let mut index: HashMap<i64, J> = HashMap::new();
    let mut st = c
        .prepare("SELECT id, item_type, source_key, source_meta FROM items WHERE import_run_id = ?1")
        .map_err(err)?;
    let rows = st
        .query_map([run], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(err)?;
    for row in rows {
        let (id, ty, key, meta) = row.map_err(err)?;
        let meta: J = meta.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or(J::Null);
        let i = meta.get("index").cloned().unwrap_or(J::Null);
        m.entry("records")
            .or_default()
            .push(json!([i, ty, key, meta.get("raw").cloned().unwrap_or(J::Null)]).to_string());
        index.insert(id, i);
    }
    let mut each = |cat: &'static str, sql: &str, n: usize| -> Result<(), String> {
        let mut st = c.prepare(sql).map_err(err)?;
        let rows = st
            .query_map([run], |r| {
                let id: i64 = r.get(0)?;
                let rest = (1..=n).map(|k| r.get::<_, rusqlite::types::Value>(k)).collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((id, rest))
            })
            .map_err(err)?;
        for row in rows {
            let (id, rest) = row.map_err(err)?;
            let mut v = vec![index.get(&id).cloned().unwrap_or(J::Null)];
            v.extend(rest.into_iter().map(|x| match x {
                rusqlite::types::Value::Null => J::Null,
                rusqlite::types::Value::Integer(n) => json!(n),
                other => zotero_import::util::value_to_string(other).map(J::String).unwrap_or(J::Null),
            }));
            m.entry(cat).or_default().push(J::Array(v).to_string());
        }
        Ok(())
    };
    each(
        "fields",
        "SELECT f.item_id, f.field, f.value FROM item_fields f JOIN items i ON i.id = f.item_id WHERE i.import_run_id = ?1",
        2,
    )?;
    each(
        "creators",
        "SELECT c.item_id, c.position, c.role, c.family, c.given, c.literal FROM creators c JOIN items i ON i.id = c.item_id WHERE i.import_run_id = ?1",
        5,
    )?;
    each(
        "tags",
        "SELECT x.item_id, t.name FROM item_tags x JOIN tags t ON t.id = x.tag_id JOIN items i ON i.id = x.item_id WHERE i.import_run_id = ?1",
        1,
    )?;
    each(
        "notes",
        "SELECT n.item_id, n.html FROM notes n JOIN items i ON i.id = n.item_id WHERE i.import_run_id = ?1",
        1,
    )?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, tests::temp_dir};
    use crate::import::formats::{RecordCreator, RecordDate};

    fn rec(ty: &str, title: &str, extra: &[(&str, &str)]) -> Record {
        let mut fields: BTreeMap<String, String> = extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        fields.insert("title".into(), title.into());
        Record {
            item_type: ty.into(),
            fields,
            date: None,
            creators: vec![],
            tags: vec![],
            notes: vec![],
            source_key: None,
            raw: format!("@raw{{{title}}}"),
            warnings: vec![],
        }
    }

    fn setup(name: &str) -> (crate::db::tests::TempDir, DataPaths, Connection) {
        let dir = temp_dir(name);
        let paths = DataPaths::new(dir.join("data"));
        paths.ensure().unwrap();
        let mut c = db::open(&paths.library_db).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
        (dir, paths, c)
    }

    fn sample() -> Parsed {
        let mut a = rec(
            "journalArticle",
            "Synthetic Panel Study",
            &[
                ("publicationTitle", "Journal of Synthetic Politics"),
                ("DOI", "10.0000/SYN.1"),
                ("bibtex:howpublished", "\\url{x}"),
            ],
        );
        a.date = Some(RecordDate {
            raw: "March 2019".into(),
            year: Some(2019),
            month: Some(3),
            day: None,
        });
        a.creators = vec![
            RecordCreator {
                role: "author".into(),
                family: Some("Smith".into()),
                given: Some("Alice".into()),
                literal: None,
            },
            RecordCreator {
                role: "author".into(),
                family: None,
                given: None,
                literal: Some("王小明".into()),
            },
        ];
        a.tags = vec!["政治效能感".into(), "survey".into()];
        a.notes = vec!["第一段\n\n第二段".into()];
        a.source_key = Some("smith2019".into());
        let mut b = rec("book", "国民财富的性质和原因的研究", &[("ISBN", "978-0-00-000000-1")]);
        b.date = Some(RecordDate {
            raw: "1972年".into(),
            year: None,
            month: None,
            day: None,
        });
        b.warnings = vec!["作者列表里的 others 已忽略".into()];
        let dup = rec("journalArticle", "Synthetic panel study!", &[("DOI", "10.0000/syn.1")]);
        Parsed {
            records: vec![a, b, dup],
            warnings: vec!["第 4 条无法解析，已跳过".into()],
        }
    }

    #[test]
    fn writes_records_and_verifies() {
        let (_d, paths, c) = setup("file-write");
        let p = Path::new("/tmp/refs.bib");
        let r = import_parsed(
            &c,
            &paths,
            p,
            Format::Bibtex,
            &sample(),
            &Options {
                skip_duplicates: true,
                into_collection: true,
            },
        )
        .unwrap();
        assert!(r.report.checks.iter().all(|x| x.ok), "{:?}", r.report.checks);
        assert_eq!(
            (
                r.summary.stats.items,
                r.summary.stats.skipped,
                r.summary.stats.notes,
                r.summary.stats.collections
            ),
            (2, 1, 1, 1)
        );
        assert!(r.report.skipped[0].contains("DOI 相同"), "本文件内的重复也能查出：{:?}", r.report.skipped);
        assert!(r.report.warnings.iter().any(|w| w.contains("others")) && r.report.warnings.iter().any(|w| w.contains("第 4 条")));
        assert_eq!(r.summary.source, "bibtex");
        assert!(r.summary.backup_file.is_some());

        let (title, date_raw, sort, year, doi, key, lang): (String, String, String, i64, String, String, String) = c
            .query_row(
                "SELECT title, date_raw, date_sort, year, doi, source_key, detected_lang FROM items WHERE source = 'bibtex' AND item_type = 'journalArticle'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap();
        assert_eq!(
            (title.as_str(), date_raw.as_str(), sort.as_str(), year),
            ("Synthetic Panel Study", "March 2019", "2019-03-00", 2019)
        );
        assert_eq!((doi.as_str(), key.as_str(), lang.as_str()), ("10.0000/SYN.1", "smith2019", "en"));
        let date_field: String = c
            .query_row("SELECT value FROM item_fields WHERE field = 'date' AND value LIKE '2019%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(date_field, "2019-03-00 March 2019", "与 Zotero 导入的条目同一种日期格式");
        let raw_field: String = c
            .query_row("SELECT value FROM item_fields WHERE field = 'bibtex:howpublished'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw_field, "\\url{x}");
        let (sort, year, lang): (String, i64, String) = c
            .query_row("SELECT date_sort, year, detected_lang FROM items WHERE item_type = 'book'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((sort.as_str(), year, lang.as_str()), ("1972-00-00", 1972, "zh"), "只有原文的日期由写入层解析");
        let html: String = c.query_row("SELECT html FROM notes", [], |r| r.get(0)).unwrap();
        assert_eq!(html, "<p>第一段</p><p>第二段</p>");
        let coll: String = c
            .query_row(
                "SELECT c.name FROM collections c JOIN collection_items x ON x.collection_id = c.id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(coll, "导入：refs.bib");
        let meta: String = c.query_row("SELECT source_meta FROM items WHERE item_type = 'book'", [], |r| r.get(0)).unwrap();
        assert!(meta.contains("@raw{国民财富的性质和原因的研究}") && meta.contains("others"));

        // 再导入一次：全部被查出可能重复
        let r2 = import_parsed(
            &c,
            &paths,
            p,
            Format::Bibtex,
            &sample(),
            &Options {
                skip_duplicates: true,
                into_collection: false,
            },
        )
        .unwrap();
        assert_eq!((r2.summary.stats.items, r2.summary.stats.skipped), (0, 3));
        // 不跳过时照样导入
        let r3 = import_parsed(
            &c,
            &paths,
            p,
            Format::Bibtex,
            &sample(),
            &Options {
                skip_duplicates: false,
                into_collection: false,
            },
        )
        .unwrap();
        assert_eq!(r3.summary.stats.items, 3);
        assert_eq!(crate::import::runs_list(&c).unwrap().len(), 3);
    }

    /// 三种格式各一份合成文件，从文件到库走完整流程（解析 → 查重 → 写入 → 核对）。
    #[test]
    fn three_formats_end_to_end() {
        let (d, paths, c) = setup("file-e2e");
        let bib = d.join("refs.bib");
        std::fs::write(
            &bib,
            "@string{jsp = {Journal of Synthetic Politics}}\n\
             @article{smith2019, author = {M{\\\"u}ller, J{\\\"o}rg and 王小明}, title = {Synthetic \\emph{Panel} Study}, journal = jsp,\n\
               year = 2019, month = mar, pages = {101--130}, doi = {10.0000/syn.2019}, keywords = {efficacy; survey}, note = {In press}}\n\
             @book{smith1776, author = {Smith, Adam}, title = {Wealth of Nations}, publisher = {Synthetic Press}, year = 1776}\n\
             @misc{broken, title = {Unclosed\n",
        )
        .unwrap();
        let p = preview(&c, &bib).unwrap();
        assert_eq!((p.format.as_str(), p.records), ("bibtex", 2));
        assert!(p.warnings.iter().any(|w| w.contains("broken") || w.contains("跳过")), "{:?}", p.warnings);
        let r = run(&c, &paths, &bib, &Options { skip_duplicates: true, into_collection: true }).unwrap();
        assert_eq!(r.summary.stats.items, 2);
        let (title, journal, pages, lang): (String, String, String, String) = c
            .query_row(
                "SELECT i.title, i.container_title, f.value, i.detected_lang FROM items i JOIN item_fields f ON f.item_id = i.id AND f.field = 'pages' WHERE i.source_key = 'smith2019'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((title.as_str(), journal.as_str(), pages.as_str(), lang.as_str()), ("Synthetic Panel Study", "Journal of Synthetic Politics", "101-130", "en"));
        let first: String = c.query_row("SELECT family FROM creators cr JOIN items i ON i.id = cr.item_id WHERE i.source_key = 'smith2019' AND cr.position = 0", [], |r| r.get(0)).unwrap();
        assert_eq!(first, "Müller");
        let literal: String = c.query_row("SELECT literal FROM creators cr JOIN items i ON i.id = cr.item_id WHERE i.source_key = 'smith2019' AND cr.position = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(literal, "王小明");

        let ris = d.join("export.ris");
        std::fs::write(&ris, "\u{feff}TY  - JOUR\r\nTI  - Synthetic Panel Study\r\nAU  - Müller, Jörg\r\nPY  - 2019///\r\nDO  - 10.0000/SYN.2019\r\nER  - \r\nTY  - CHAP\r\nTI  - 论分工\r\nAU  - 亚当·斯密\r\nT2  - 国民财富的性质和原因的研究\r\nPY  - 1972\r\nN1  - 合成的子笔记\r\nER  - \r\n").unwrap();
        let p = preview(&c, &ris).unwrap();
        assert_eq!((p.format.as_str(), p.records, p.duplicates.len()), ("ris", 2, 1), "DOI 大小写不同也算重复");
        let r = run(&c, &paths, &ris, &Options { skip_duplicates: true, into_collection: false }).unwrap();
        assert_eq!((r.summary.stats.items, r.summary.stats.skipped, r.summary.stats.notes), (1, 1, 1));
        let (container, lang): (String, String) = c.query_row("SELECT container_title, detected_lang FROM items WHERE title = '论分工'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((container.as_str(), lang.as_str()), ("国民财富的性质和原因的研究", "zh"));

        let json = d.join("items.json");
        std::fs::write(
            &json,
            r#"[{"id": "a1", "type": "paper-conference", "title": "Efficacy Online", "container-title": "Proceedings of the Synthetic Conference",
                 "author": [{"family": "Doe", "given": "Jon"}], "issued": {"date-parts": [[2020, 6]]}, "keyword": "online, efficacy"}]"#,
        )
        .unwrap();
        let r = run(&c, &paths, &json, &Options { skip_duplicates: true, into_collection: false }).unwrap();
        assert!(r.report.checks.iter().all(|x| x.ok));
        let (ty, sort, container): (String, String, String) =
            c.query_row("SELECT item_type, date_sort, container_title FROM items WHERE source = 'csl-json'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((ty.as_str(), sort.as_str(), container.as_str()), ("conferencePaper", "2020-06-00", "Proceedings of the Synthetic Conference"));
        assert_eq!(crate::import::runs_list(&c).unwrap().len(), 3);
    }

    #[test]
    fn unreadable_files_are_reported() {
        let (d, _paths, c) = setup("file-bad");
        let bin = d.join("x.bib");
        std::fs::write(&bin, [0xff, 0xfe, 0x00, 0x41]).unwrap();
        assert!(preview(&c, &bin).unwrap_err().contains("UTF-8"));
        let unknown = d.join("x.txt");
        std::fs::write(&unknown, "just some text").unwrap();
        assert!(preview(&c, &unknown).unwrap_err().contains("认不出文件格式"));
        assert!(preview(&c, &d.join("missing.bib")).unwrap_err().contains("读不了"));
    }
}
