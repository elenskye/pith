//! 搜索索引（L5，Phase 3）：`search.db` 以 `search` 名 ATTACH 到主连接上，只存派生数据，可删了重建，不写 migration，
//! 不进备份（备份用 `VACUUM INTO`，只写 main）。
//!
//! **文档** = 列表里的一行：文献（含回收站里的）、独立附件、独立笔记。子附件、子笔记、标注的内容并进所属文献。
//! 回收站里的也索引，显示与否由列表按视图过滤，所以移进 / 移出回收站不用改索引；直接删除的子附件 / 子笔记 / 标注
//! 不并进上级（与详情一致，详情也不显示它们），所以子项移进 / 移出回收站要 `update`。
//!
//! **表结构**：只有一张 FTS5 表 `search.docs(title, creators, tags, abstract, notes, other)`，查询按 bm25 列权重排序。
//! rowid = id × 4 + 种类（1 文献、2 附件、3 笔记），不另建 (kind, id) ↔ rowid 映射表：少一张要同步的表、查询不用 JOIN，
//! 删文档直接按 rowid；id 是 SQLite 整数主键，乘 4 远不会溢出（溢出时报错而不是写错）。
//!
//! **中文分词（实测后定：方案 1）**：`unicode61 remove_diacritics 2`，建索引和查询时都把中日韩连续文字改写成
//! 「重叠二元组 + 末字单字」（`政治效能感` → `政治 治效 效能 能感 感`）。查询：多字用二元组短语（等于子串匹配），
//! 单字用前缀（每个字都是某个词的开头，等于「含这个字」）；西文词用前缀（`effic` 找到 efficacy），大小写、重音不敏感。
//! 理由（真实副本 92 个文档、255 个查询词从库里自动取，`tests::real_copy_tokenizers`；表见 decisions/0010）：
//! - 中文 1 字 / 2 字 / 3–4 字、中文作者名、中英混合：两种方案命中完全相同，都等于子串真值（0 漏 0 多）。
//!   方案 2 的 trigram 对一两个字的词必须另走 LIKE 回退，方案 1 不用；
//! - 西文：方案 1 与「词首前缀」真值完全一致（0 漏），方案 2 与「子串」真值完全一致。差别只在词中间的子串
//!   （词中子串 20 个查询：方案 1 找到 36 条、方案 2 找到 487 条；整词、前缀、姓、标签、笔记各类方案 2 多 1–24 条，都是嵌在长词里的）。
//!   文献检索输入的是词或词头，词中间命中多半是误中（`form` 中 reform），不追求；
//! - 方案 1 索引 416 KB、建 13 ms，方案 2 为 1048 KB、58 ms；查询两者都在 1 ms 以内（调试构建），速度不是区分点。
//!
//! 已知不足：繁简不通；西文只做前缀（不找词中间）；Müller 与 Mueller 不等价；只有符号的词（emoji、`#`）按原文子串找、相关度都是 0。

use crate::common::{Tx, err};
use crate::text::{html_to_text, split_multipart, year_of};
use rusqlite::types::Value;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, Row, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

/// 表结构或文档内容的规则变了就加 1：打开时版本不同，整个 search 库清空重建
const SEARCH_VERSION: i64 = 1;

const CREATE: &str = "CREATE VIRTUAL TABLE search.docs USING fts5(title, creators, tags, abstract, notes, other, tokenize = 'unicode61 remove_diacritics 2')";

/// bm25 列权重，顺序同建表：标题最重，作者、标签其次
const WEIGHTS: &str = "10.0, 5.0, 4.0, 2.0, 1.0, 1.0";

/// 一次查询最多用多少个词（再多的忽略：粘贴整段文字时不至于拼出巨大的表达式）
const MAX_TERMS: usize = 32;

/// 列表里的一行对应的文档种类：文献、独立附件、独立笔记（子附件 / 子笔记的内容并进所属文献）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DocKind {
    Item,
    Attachment,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocRef {
    pub kind: DocKind,
    pub id: i64,
}

impl DocRef {
    pub fn item(id: i64) -> Self {
        DocRef { kind: DocKind::Item, id }
    }
    pub fn attachment(id: i64) -> Self {
        DocRef { kind: DocKind::Attachment, id }
    }
    pub fn note(id: i64) -> Self {
        DocRef { kind: DocKind::Note, id }
    }
}

const KINDS: [DocKind; 3] = [DocKind::Item, DocKind::Attachment, DocKind::Note];

impl DocKind {
    fn code(self) -> i64 {
        match self {
            DocKind::Item => 1,
            DocKind::Attachment => 2,
            DocKind::Note => 3,
        }
    }
}

fn rowid(r: DocRef) -> Result<i64, String> {
    r.id.checked_mul(4).map(|x| x + r.kind.code()).ok_or_else(|| format!("编号 {} 超出搜索索引的范围", r.id))
}

fn from_rowid(rowid: i64) -> Option<DocRef> {
    let kind = match rowid.rem_euclid(4) {
        1 => DocKind::Item,
        2 => DocKind::Attachment,
        3 => DocKind::Note,
        _ => return None,
    };
    Some(DocRef { kind, id: rowid.div_euclid(4) })
}

// ---------- 打开 ----------

/// 把 `path` 以 `search` 名 ATTACH 到连接上；版本不对就清空重建表结构（内容要再 `rebuild`）。
/// 文件坏了（不是数据库、结构损坏）就删掉重来：里面只有派生数据。别的失败（被锁住、没有权限）照常报错，不删文件。
pub fn attach(c: &Connection, path: &Path) -> Result<(), String> {
    let p = path.to_str().ok_or("搜索索引的路径含有无法识别的字符")?;
    match open_at(c, p) {
        Ok(()) => Ok(()),
        Err(first) if is_broken(p) => {
            detach(c);
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let _ = std::fs::remove_file(format!("{p}{suffix}"));
            }
            open_at(c, p).map_err(|e| format!("无法打开搜索索引：{e}（删除重建前：{first}）"))
        }
        Err(e) => Err(format!("无法打开搜索索引：{e}")),
    }
}

/// 用一个单独的只读连接看文件是否损坏
fn is_broken(path: &str) -> bool {
    let Ok(probe) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    match probe.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)) {
        Ok(s) => s != "ok",
        Err(e) => matches!(e.sqlite_error_code(), Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)),
    }
}

/// 测试用：ATTACH 一个内存库。
#[cfg_attr(not(test), allow(dead_code))]
pub fn attach_memory(c: &Connection) -> Result<(), String> {
    open_at(c, ":memory:")
}

fn open_at(c: &Connection, target: &str) -> Result<(), String> {
    detach(c); // 重复调用：先断开旧的
    c.execute("ATTACH DATABASE ?1 AS search", [target]).map_err(err)?;
    c.execute_batch("PRAGMA search.journal_mode = WAL; PRAGMA search.synchronous = NORMAL;").map_err(err)?;
    prepare(c)
}

fn detach(c: &Connection) {
    let attached = c
        .query_row("SELECT 1 FROM pragma_database_list WHERE name = 'search'", [], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some();
    if attached {
        let _ = c.execute_batch("DETACH DATABASE search");
    }
}

/// 版本不对、或表不在：清空 search 库，按现在的结构建表、写版本号。
fn prepare(c: &Connection) -> Result<(), String> {
    let version: i64 = c.query_row("PRAGMA search.user_version", [], |r| r.get(0)).map_err(err)?;
    let has_docs = c
        .query_row("SELECT 1 FROM search.sqlite_master WHERE name = 'docs'", [], |_| Ok(()))
        .optional()
        .map_err(err)?
        .is_some();
    if version == SEARCH_VERSION && has_docs {
        return Ok(());
    }
    let tx = Tx::begin(c)?;
    // 先删虚表（连带它的影子表），再删剩下的
    let mut st = tx
        .prepare(
            "SELECT type, name FROM search.sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
              ORDER BY sql LIKE 'CREATE VIRTUAL TABLE%' DESC",
        )
        .map_err(err)?;
    let old: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    drop(st);
    for (ty, name) in old {
        let what = if ty == "view" { "VIEW" } else { "TABLE" };
        tx.execute_batch(&format!("DROP {what} IF EXISTS search.\"{}\"", name.replace('"', "\"\""))).map_err(err)?;
    }
    tx.execute_batch(&format!("{CREATE}; PRAGMA search.user_version = {SEARCH_VERSION};")).map_err(err)?;
    tx.commit()
}

// ---------- 写入 ----------

/// 启动时调用：索引是空的、或与库对不上（文档集合 ≠ 列表行集合）就整体重建。返回是否重建了。
/// 比的是 rowid 集合（比只比个数更严）；内容是否过时不检查，靠各处改动后调用 `update`。
pub fn ensure_fresh(c: &Connection) -> Result<bool, String> {
    prepare(c)?;
    let ids = |sql: &str| -> Result<Vec<i64>, String> {
        let mut st = c.prepare(sql).map_err(err)?;
        st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
    };
    let want = ids(
        "SELECT id * 4 + 1 FROM items
         UNION ALL SELECT id * 4 + 2 FROM attachments WHERE item_id IS NULL AND note_id IS NULL AND kind != 'embedded_image'
         UNION ALL SELECT id * 4 + 3 FROM notes WHERE item_id IS NULL
         ORDER BY 1",
    )?;
    // 索引读不出来（损坏）也重建
    if ids("SELECT rowid FROM search.docs ORDER BY rowid").is_ok_and(|have| have == want) {
        return Ok(false);
    }
    rebuild(c)?;
    Ok(true)
}

/// 整体重建，返回文档数。
pub fn rebuild(c: &Connection) -> Result<usize, String> {
    let tx = Tx::begin(c)?;
    // 删表重建比逐行 DELETE 快（FTS5 删一行要重新分词）
    tx.execute_batch(&format!("DROP TABLE IF EXISTS search.docs; {CREATE};")).map_err(err)?;
    let mut n = 0;
    for kind in KINDS {
        n += write(&tx, kind, &Scope::All)?;
    }
    tx.execute("INSERT INTO search.docs (docs) VALUES ('optimize')", []).map_err(err)?;
    tx.commit()?;
    Ok(n)
}

/// 按库里现在的样子重写这些文档：行不存在就删掉文档；传入子附件 / 子笔记时改为重写所属文献。
/// 注意：直接永久删除一个**没进过回收站**的子附件 / 子笔记时，行已经不在、找不到上级，调用方要把上级一并传进来。
pub fn update(c: &Connection, refs: &[DocRef]) -> Result<(), String> {
    if refs.is_empty() {
        return Ok(());
    }
    let tx = Tx::begin(c)?;
    let mut targets: BTreeMap<DocKind, BTreeSet<i64>> = BTreeMap::new();
    for r in refs {
        let owner = owner(&tx, *r)?;
        // 它自己现在不是文档（成了子项、或是内嵌图片）：删掉以前可能有的文档
        if owner != Some(*r) {
            delete(&tx, *r)?;
        }
        if let Some(o) = owner {
            targets.entry(o.kind).or_default().insert(o.id);
        }
    }
    for (kind, ids) in targets {
        let ids: Vec<i64> = ids.into_iter().collect();
        write(&tx, kind, &Scope::Ids(&ids))?;
    }
    tx.commit()
}

/// 一次导入带来的全部文档（含子附件 / 子笔记 / 标注由这次导入带来的上级文献）。
pub fn update_run(c: &Connection, run_id: i64) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    for kind in KINDS {
        write(&tx, kind, &Scope::Run(run_id))?;
    }
    tx.commit()
}

/// 这一行现在对应哪个文档：文献 → 自己；子附件 / 子笔记 → 所属文献；内嵌图片 → 所属笔记对应的文档；
/// 行不存在 → 自己（`write` 会删掉它）；不是文档（没有上级的内嵌图片）→ None。
fn owner(c: &Connection, r: DocRef) -> Result<Option<DocRef>, String> {
    let note_owner = |id: i64| -> Result<Option<DocRef>, String> {
        let item: Option<Option<i64>> = c.query_row("SELECT item_id FROM notes WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
        Ok(Some(match item {
            Some(Some(item)) => DocRef::item(item),
            _ => DocRef::note(id),
        }))
    };
    match r.kind {
        DocKind::Item => Ok(Some(r)),
        DocKind::Note => note_owner(r.id),
        DocKind::Attachment => {
            let row: Option<(Option<i64>, Option<i64>, String)> = c
                .query_row("SELECT item_id, note_id, kind FROM attachments WHERE id = ?1", [r.id], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))
                .optional()
                .map_err(err)?;
            match row {
                None => Ok(Some(r)),
                Some((Some(item), _, _)) => Ok(Some(DocRef::item(item))),
                Some((None, Some(note), _)) => note_owner(note),
                Some((None, None, kind)) if kind == "embedded_image" => Ok(None),
                Some(_) => Ok(Some(r)),
            }
        }
    }
}

/// 要写哪些文档
enum Scope<'a> {
    All,
    Ids(&'a [i64]),
    /// 某次导入：这次导入带来的行，以及子项由这次导入带来的上级
    Run(i64),
}

// 这几段会拼进别的查询的 IN (…) 里，别名起得与外层不同
const RUN_ITEMS: &str = "SELECT id FROM items WHERE import_run_id = ?1
    UNION SELECT item_id FROM attachments WHERE import_run_id = ?1 AND item_id IS NOT NULL
    UNION SELECT item_id FROM notes WHERE import_run_id = ?1 AND item_id IS NOT NULL
    UNION SELECT run_a.item_id FROM annotations run_an JOIN attachments run_a ON run_a.id = run_an.attachment_id
           WHERE run_an.import_run_id = ?1 AND run_a.item_id IS NOT NULL";
const RUN_ATTACHMENTS: &str = "SELECT id FROM attachments WHERE import_run_id = ?1 UNION SELECT attachment_id FROM annotations WHERE import_run_id = ?1";
const RUN_NOTES: &str = "SELECT id FROM notes WHERE import_run_id = ?1";

impl Scope<'_> {
    /// ` AND col IN (这类文档的 id)`；All 时为空串
    fn within(&self, kind: DocKind, col: &str) -> String {
        let set = match self {
            Scope::All => return String::new(),
            Scope::Ids(_) => "SELECT value FROM json_each(?1)",
            Scope::Run(_) => match kind {
                DocKind::Item => RUN_ITEMS,
                DocKind::Attachment => RUN_ATTACHMENTS,
                DocKind::Note => RUN_NOTES,
            },
        };
        format!(" AND {col} IN ({set})")
    }

    fn args(&self) -> Vec<Value> {
        match self {
            Scope::All => vec![],
            Scope::Ids(ids) => vec![Value::Text(format!("[{}]", ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")))],
            Scope::Run(id) => vec![Value::Integer(*id)],
        }
    }
}

/// 读出这些文档并写进索引（先删旧的），返回写入的文档数。All 时调用方已清空了表。
fn write(c: &Connection, kind: DocKind, scope: &Scope) -> Result<usize, String> {
    let docs = load(c, kind, scope)?;
    match scope {
        Scope::All => {}
        Scope::Ids(ids) => {
            for id in ids.iter() {
                delete(c, DocRef { kind, id: *id })?;
            }
        }
        Scope::Run(_) => {
            for id in docs.keys() {
                delete(c, DocRef { kind, id: *id })?;
            }
        }
    }
    let mut st = c
        .prepare_cached("INSERT INTO search.docs (rowid, title, creators, tags, abstract, notes, other) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
        .map_err(err)?;
    for (id, d) in &docs {
        let [a, b, t, ab, n, o] = d.texts().map(|s| index_text(&s));
        st.execute(params![rowid(DocRef { kind, id: *id })?, a, b, t, ab, n, o]).map_err(err)?;
    }
    Ok(docs.len())
}

fn delete(c: &Connection, r: DocRef) -> Result<(), String> {
    c.prepare_cached("DELETE FROM search.docs WHERE rowid = ?1").map_err(err)?.execute([rowid(r)?]).map_err(err)?;
    Ok(())
}

// ---------- 文档内容 ----------

const TITLE: usize = 0;
const CREATORS: usize = 1;
const TAGS: usize = 2;
const ABSTRACT: usize = 3;
const NOTES: usize = 4;
const OTHER: usize = 5;

/// 一个文档各列的原文（还没切二元组），每列若干个值。
#[derive(Debug, Default)]
struct Doc([Vec<String>; 6]);

impl Doc {
    fn add(&mut self, col: usize, s: Option<&str>) {
        if let Some(s) = s.map(str::trim).filter(|s| !s.is_empty()) {
            self.0[col].push(s.to_string());
        }
    }

    /// 各列原文，值之间换行。同一个值只留在最靠前（权重最高）的那一列：期刊名既是列又是字段、标签挂在子附件上又挂在文献上……
    fn texts(&self) -> [String; 6] {
        let mut seen: HashSet<&str> = HashSet::new();
        std::array::from_fn(|i| self.0[i].iter().map(String::as_str).filter(|v| seen.insert(v)).collect::<Vec<_>>().join("\n"))
    }
}

/// 不进「其他」列的字段：已在别的列里（标题、摘要、年份）或对搜索没用（访问时间、语言代码）。
/// DOI / ISBN 的原文保留（带连字符的 ISBN 要靠它），与规范化后的列相同时去重。
/// 另外 `bibtex:` / `ris:` / `csl:` 开头的原始字段也不进（来源原文）。
const SKIP_FIELDS: &[&str] = &["title", "abstractNote", "date", "accessDate", "language"];

fn each(c: &Connection, sql: &str, args: &[Value], mut f: impl FnMut(&Row) -> rusqlite::Result<()>) -> Result<(), String> {
    let mut st = c.prepare(sql).map_err(err)?;
    let mut rows = st.query(params_from_iter(args)).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        f(r).map_err(err)?;
    }
    Ok(())
}

fn text_of(html: Option<String>) -> Option<String> {
    html.map(|h| html_to_text(&h))
}

fn load(c: &Connection, kind: DocKind, scope: &Scope) -> Result<BTreeMap<i64, Doc>, String> {
    match kind {
        DocKind::Item => load_items(c, scope),
        DocKind::Attachment => load_attachments(c, scope),
        DocKind::Note => load_notes(c, scope),
    }
}

fn load_items(c: &Connection, scope: &Scope) -> Result<BTreeMap<i64, Doc>, String> {
    let args = scope.args();
    let w = |col: &str| scope.within(DocKind::Item, col);
    let mut docs: BTreeMap<i64, Doc> = BTreeMap::new();
    each(
        c,
        &format!("SELECT id, title, abstract, container_title, year, doi, isbn FROM items WHERE 1{}", w("id")),
        &args,
        |r| {
            let mut d = Doc::default();
            d.add(TITLE, r.get::<_, Option<String>>(1)?.as_deref());
            d.add(ABSTRACT, r.get::<_, Option<String>>(2)?.as_deref());
            d.add(OTHER, r.get::<_, Option<String>>(3)?.as_deref());
            d.add(OTHER, r.get::<_, Option<i64>>(4)?.map(|y| y.to_string()).as_deref());
            d.add(OTHER, r.get::<_, Option<String>>(5)?.as_deref());
            d.add(OTHER, r.get::<_, Option<String>>(6)?.as_deref());
            docs.insert(r.get(0)?, d);
            Ok(())
        },
    )?;
    if docs.is_empty() {
        return Ok(docs);
    }
    let mut on = |sql: String, f: &mut dyn FnMut(&mut Doc, &Row) -> rusqlite::Result<()>| {
        each(c, &sql, &args, |r| {
            if let Some(d) = docs.get_mut(&r.get::<_, i64>(0)?) {
                f(d, r)?;
            }
            Ok(())
        })
    };
    on(
        format!("SELECT item_id, literal, family, given FROM creators WHERE 1{} ORDER BY item_id, position", w("item_id")),
        &mut |d, r| {
            let literal: Option<String> = r.get(1)?;
            let name = match literal.filter(|s| !s.trim().is_empty()) {
                Some(l) => l,
                None => format!("{} {}", r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, Option<String>>(3)?.unwrap_or_default()),
            };
            d.add(CREATORS, Some(&name));
            Ok(())
        },
    )?;
    // 标签：文献自己的，加上未删除的子附件、子笔记、标注上的
    on(
        format!(
            "SELECT x.item_id, t.name FROM item_tags x JOIN tags t ON t.id = x.tag_id WHERE 1{}
             UNION ALL
             SELECT a.item_id, t.name FROM attachment_tags x JOIN attachments a ON a.id = x.attachment_id JOIN tags t ON t.id = x.tag_id
              WHERE a.item_id IS NOT NULL AND a.deleted_at IS NULL{}
             UNION ALL
             SELECT n.item_id, t.name FROM note_tags x JOIN notes n ON n.id = x.note_id JOIN tags t ON t.id = x.tag_id
              WHERE n.item_id IS NOT NULL AND n.deleted_at IS NULL{}
             UNION ALL
             SELECT a.item_id, t.name FROM annotation_tags x JOIN annotations an ON an.id = x.annotation_id
                    JOIN attachments a ON a.id = an.attachment_id JOIN tags t ON t.id = x.tag_id
              WHERE a.item_id IS NOT NULL AND a.deleted_at IS NULL AND an.deleted_at IS NULL{}",
            w("x.item_id"),
            w("a.item_id"),
            w("n.item_id"),
            w("a.item_id")
        ),
        &mut |d, r| {
            d.add(TAGS, r.get::<_, Option<String>>(1)?.as_deref());
            Ok(())
        },
    )?;
    on(
        format!("SELECT item_id, field, value FROM item_fields WHERE 1{} ORDER BY item_id, field", w("item_id")),
        &mut |d, r| {
            let field: String = r.get(1)?;
            if field.contains(':') || SKIP_FIELDS.contains(&field.as_str()) {
                return Ok(());
            }
            let value: String = r.get(2)?;
            // 日期类字段是 Zotero 两段式「排序值 原文」：只要原文（没有原文时取年份）
            let value = match split_multipart(&value) {
                (Some(sort), raw) if field.to_ascii_lowercase().contains("date") => {
                    if raw.trim().is_empty() {
                        year_of(&sort).map(|y| y.to_string()).unwrap_or_default()
                    } else {
                        raw
                    }
                }
                _ => value,
            };
            d.add(OTHER, Some(&value));
            Ok(())
        },
    )?;
    on(
        format!(
            "SELECT item_id, html FROM notes WHERE item_id IS NOT NULL AND deleted_at IS NULL{} ORDER BY item_id, created_at, id",
            w("item_id")
        ),
        &mut |d, r| {
            d.add(NOTES, text_of(r.get(1)?).as_deref());
            Ok(())
        },
    )?;
    on(
        format!(
            "SELECT item_id, title, original_name, note FROM attachments
              WHERE item_id IS NOT NULL AND deleted_at IS NULL AND kind != 'embedded_image'{} ORDER BY item_id, created_at, id",
            w("item_id")
        ),
        &mut |d, r| {
            d.add(OTHER, r.get::<_, Option<String>>(1)?.as_deref());
            d.add(OTHER, r.get::<_, Option<String>>(2)?.as_deref());
            d.add(NOTES, text_of(r.get(3)?).as_deref());
            Ok(())
        },
    )?;
    on(
        format!(
            "SELECT a.item_id, an.text, an.comment FROM annotations an JOIN attachments a ON a.id = an.attachment_id
              WHERE a.item_id IS NOT NULL AND a.deleted_at IS NULL AND an.deleted_at IS NULL{} ORDER BY a.item_id, an.sort_key, an.id",
            w("a.item_id")
        ),
        &mut |d, r| {
            d.add(NOTES, r.get::<_, Option<String>>(1)?.as_deref());
            d.add(NOTES, r.get::<_, Option<String>>(2)?.as_deref());
            Ok(())
        },
    )?;
    Ok(docs)
}

fn load_attachments(c: &Connection, scope: &Scope) -> Result<BTreeMap<i64, Doc>, String> {
    let args = scope.args();
    let w = |col: &str| scope.within(DocKind::Attachment, col);
    let mut docs: BTreeMap<i64, Doc> = BTreeMap::new();
    each(
        c,
        &format!(
            "SELECT id, title, original_name, url, note FROM attachments
              WHERE item_id IS NULL AND note_id IS NULL AND kind != 'embedded_image'{}",
            w("id")
        ),
        &args,
        |r| {
            let mut d = Doc::default();
            let title: Option<String> = r.get(1)?;
            let name: Option<String> = r.get(2)?;
            d.add(TITLE, title.as_deref().filter(|t| !t.trim().is_empty()).or(name.as_deref()));
            d.add(OTHER, name.as_deref());
            d.add(OTHER, r.get::<_, Option<String>>(3)?.as_deref());
            d.add(NOTES, text_of(r.get(4)?).as_deref());
            docs.insert(r.get(0)?, d);
            Ok(())
        },
    )?;
    if docs.is_empty() {
        return Ok(docs);
    }
    let mut on = |sql: String, f: &mut dyn FnMut(&mut Doc, &Row) -> rusqlite::Result<()>| {
        each(c, &sql, &args, |r| {
            if let Some(d) = docs.get_mut(&r.get::<_, i64>(0)?) {
                f(d, r)?;
            }
            Ok(())
        })
    };
    on(
        format!(
            "SELECT x.attachment_id, t.name FROM attachment_tags x JOIN tags t ON t.id = x.tag_id WHERE 1{}
             UNION ALL
             SELECT an.attachment_id, t.name FROM annotation_tags x JOIN annotations an ON an.id = x.annotation_id JOIN tags t ON t.id = x.tag_id
              WHERE an.deleted_at IS NULL{}",
            w("x.attachment_id"),
            w("an.attachment_id")
        ),
        &mut |d, r| {
            d.add(TAGS, r.get::<_, Option<String>>(1)?.as_deref());
            Ok(())
        },
    )?;
    on(
        format!(
            "SELECT attachment_id, text, comment FROM annotations WHERE deleted_at IS NULL{} ORDER BY attachment_id, sort_key, id",
            w("attachment_id")
        ),
        &mut |d, r| {
            d.add(NOTES, r.get::<_, Option<String>>(1)?.as_deref());
            d.add(NOTES, r.get::<_, Option<String>>(2)?.as_deref());
            Ok(())
        },
    )?;
    Ok(docs)
}

fn load_notes(c: &Connection, scope: &Scope) -> Result<BTreeMap<i64, Doc>, String> {
    let args = scope.args();
    let w = |col: &str| scope.within(DocKind::Note, col);
    let mut docs: BTreeMap<i64, Doc> = BTreeMap::new();
    each(c, &format!("SELECT id, title, html FROM notes WHERE item_id IS NULL{}", w("id")), &args, |r| {
        let mut d = Doc::default();
        let text = html_to_text(&r.get::<_, String>(2)?);
        let title: Option<String> = r.get(1)?;
        // 没有标题时用正文第一行（与列表显示一致）
        d.add(TITLE, title.as_deref().filter(|t| !t.trim().is_empty()).or_else(|| text.lines().next()));
        d.add(NOTES, Some(&text));
        docs.insert(r.get(0)?, d);
        Ok(())
    })?;
    if docs.is_empty() {
        return Ok(docs);
    }
    each(
        c,
        &format!("SELECT x.note_id, t.name FROM note_tags x JOIN tags t ON t.id = x.tag_id WHERE 1{}", w("x.note_id")),
        &args,
        |r| {
            if let Some(d) = docs.get_mut(&r.get::<_, i64>(0)?) {
                d.add(TAGS, r.get::<_, Option<String>>(1)?.as_deref());
            }
            Ok(())
        },
    )?;
    Ok(docs)
}

// ---------- 分词与查询 ----------

/// 中日韩文字（汉字、假名、谚文）：没有空格分词，按重叠二元组索引
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x3134F | 0xF900..=0xFAFF  // 汉字（含扩展区、兼容区）
        | 0x3005..=0x3007                                                          // 々 〆 〇
        | 0x3040..=0x309F | 0x30A0..=0x30FA | 0x30FC..=0x30FF | 0x31F0..=0x31FF    // 假名（不含中点 ・）
        | 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F) // 谚文
}

/// 全角 ASCII 转半角、全角空格转空格（索引与查询两边都做）
fn halfwidth(c: char) -> char {
    match c as u32 {
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        0x3000 => ' ',
        _ => c,
    }
}

/// 西文词里的字符：字母、数字，以及组合附加符号（分解形式的重音）
fn is_word_char(c: char) -> bool {
    (c.is_alphanumeric() && !is_cjk(c)) || matches!(c as u32, 0x300..=0x36F)
}

/// 建索引前的文本变换：全角转半角；中日韩连续文字改写成「重叠二元组 + 末字单字」，前后用空格与别的文字隔开。
/// 其余文字原样交给 unicode61（它负责西文分词、大小写与重音）。
fn index_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    let mut run: Vec<char> = Vec::new();
    let flush = |out: &mut String, run: &mut Vec<char>| {
        if run.is_empty() {
            return;
        }
        out.push(' ');
        for (i, ch) in run.iter().enumerate() {
            out.push(*ch);
            if let Some(next) = run.get(i + 1) {
                out.push(*next);
            }
            out.push(' ');
        }
        run.clear();
    };
    for ch in s.chars().map(halfwidth) {
        if is_cjk(ch) {
            run.push(ch);
        } else {
            flush(&mut out, &mut run);
            out.push(ch);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// 查询解析的结果：FTS5 表达式（各短语之间隐式 AND），和没有文字、只能按原文找的符号词
#[derive(Debug, PartialEq)]
struct Parsed {
    fts: Vec<String>,
    literals: Vec<String>,
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// 把用户输入变成安全的 FTS5 表达式：所有内容都放进双引号短语里，FTS5 的语法字符（引号、星号、括号、冒号、减号、
/// NEAR / AND / OR / NOT）一律当普通文字（它们不是字母数字，本来就只起分隔作用）。按空白分词，词与词之间 AND；
/// 一个词里：连着的西文（被标点隔开的部分，如 `10.1000/abc`、`Müller-Lüdenscheidt`）组成一个短语、末词前缀匹配；
/// 中日韩文字单字前缀、多字二元组短语。
fn parse_query(q: &str) -> Parsed {
    let mut fts: Vec<String> = Vec::new();
    let mut literals: Vec<String> = Vec::new();
    for word in q.split_whitespace() {
        let word: String = word.chars().map(halfwidth).collect();
        let before = fts.len();
        let mut latin: Vec<String> = Vec::new();
        let mut tok = String::new();
        let mut cjk: Vec<char> = Vec::new();
        let flush_latin = |fts: &mut Vec<String>, latin: &mut Vec<String>, tok: &mut String| {
            if !tok.is_empty() {
                latin.push(std::mem::take(tok));
            }
            if !latin.is_empty() {
                fts.push(format!("{}*", quote(&latin.join(" "))));
                latin.clear();
            }
        };
        let flush_cjk = |fts: &mut Vec<String>, cjk: &mut Vec<char>| {
            match cjk.len() {
                0 => return,
                1 => fts.push(format!("{}*", quote(&cjk[0].to_string()))),
                _ => fts.push(quote(&cjk.windows(2).map(|w| w.iter().collect::<String>()).collect::<Vec<_>>().join(" "))),
            }
            cjk.clear();
        };
        for ch in word.chars() {
            if is_cjk(ch) {
                flush_latin(&mut fts, &mut latin, &mut tok);
                cjk.push(ch);
                continue;
            }
            flush_cjk(&mut fts, &mut cjk);
            if is_word_char(ch) {
                tok.push(ch);
            } else if !tok.is_empty() {
                latin.push(std::mem::take(&mut tok));
            }
        }
        flush_latin(&mut fts, &mut latin, &mut tok);
        flush_cjk(&mut fts, &mut cjk);
        if fts.len() == before {
            literals.push(word);
        }
    }
    fts.truncate(MAX_TERMS);
    literals.truncate(MAX_TERMS);
    Parsed { fts, literals }
}

/// 搜索。查询去掉空白后为空返回 None（不过滤）；否则返回命中的文档 → 相关度（bm25，越小越相关）。
/// 只有符号的词（`#`、emoji）没法进全文索引，按原文子串在各列里找；查询里只有这种词时相关度都是 0。
pub fn query(c: &Connection, q: &str) -> Result<Option<HashMap<DocRef, f64>>, String> {
    if q.trim().is_empty() {
        return Ok(None);
    }
    let p = parse_query(q);
    let mut args: Vec<Value> = Vec::new();
    let mut sql = if p.fts.is_empty() {
        "SELECT rowid, 0.0 FROM search.docs WHERE 1".to_string()
    } else {
        args.push(Value::Text(p.fts.join(" ")));
        format!("SELECT rowid, bm25(docs, {WEIGHTS}) FROM search.docs WHERE docs MATCH ?1")
    };
    for lit in &p.literals {
        args.push(Value::Text(lit.clone()));
        let n = args.len();
        sql.push_str(&format!(
            " AND (instr(title, ?{n}) OR instr(creators, ?{n}) OR instr(tags, ?{n}) OR instr(abstract, ?{n}) OR instr(notes, ?{n}) OR instr(other, ?{n}))"
        ));
    }
    let mut hits = HashMap::new();
    each(c, &sql, &args, |r| {
        if let Some(d) = from_rowid(r.get(0)?) {
            hits.insert(d, r.get(1)?);
        }
        Ok(())
    })
    .map_err(|e| format!("搜索失败：{e}"))?;
    Ok(Some(hits))
}

#[cfg(test)]
mod tests;
