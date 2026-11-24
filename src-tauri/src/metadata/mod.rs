//! 联网补全（v0.1 Phase 5，`decisions/0012`）：按 DOI / ISBN 联网查元数据，补到文献上。
//! 规则：
//! - 只有在设置里打开了才联网（第一次用之前界面先说明会发什么、发给谁，用户同意后才打开）；
//! - 查到的只**自动填空着的**字段（作者整组为空时才填作者）；与现有不同的列为「待确认」，用户逐条采用或忽略；
//! - 每次查询记一行 `metadata_lookups`（查到的记录、改了什么、每处改动的状态）；自动填的可以撤销（只撤销之后没再被改过的）；
//! - 联网时不拿数据库锁：`prepare`（读库）→ `fetch`（联网）→ `record`（写库）；
//! - 导入后、填了 DOI / ISBN 后在后台排队查（`Queue` + `work`），一次一条、不超速。

pub mod net;

use crate::common::{Tx, err};
use crate::edit::{self, CreatorInput, ItemPatch};
use crate::fields;
use crate::search::DocRef;
use crate::text;
use net::{Http, NetError, Outcome, Pacer};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------- 标识与查到的记录 ----------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Isbn(String),
}

impl Identifier {
    /// 存库用：`doi:10.1/x`、`isbn:9780140449136`
    pub fn key(&self) -> String {
        match self {
            Identifier::Doi(d) => format!("doi:{}", d.to_lowercase()),
            Identifier::Isbn(i) => format!("isbn:{i}"),
        }
    }
    /// 给人看：`DOI 10.1/x`
    pub fn label(&self) -> String {
        match self {
            Identifier::Doi(d) => format!("DOI {d}"),
            Identifier::Isbn(i) => format!("ISBN {i}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundCreator {
    pub role: String,
    pub family: Option<String>,
    pub given: Option<String>,
    pub literal: Option<String>,
}

/// 查到的记录：字段已换成 Zotero 字段名（`date` 是原文，如 2015-05-27）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// crossref / datacite / … / openlibrary
    pub source: String,
    pub item_type: String,
    pub fields: BTreeMap<String, String>,
    pub creators: Vec<FoundCreator>,
    /// 其他日期（印刷版、网络版），比较年份时也算
    #[serde(default)]
    pub date_alternatives: Vec<String>,
}

// ---------------- 设置 ----------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// None = 还没问过用户（第一次用前界面要先说明范围）
    pub enabled: Option<bool>,
    /// 导入后、填了 DOI / ISBN 后自动查
    pub auto: bool,
    /// 联系邮箱（只放在发给 doi.org / Crossref 的请求里）
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,
    /// 空字符串 = 不带邮箱
    pub email: Option<String>,
}

pub(crate) fn setting(c: &Connection, key: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(err)
}

pub(crate) fn put_setting(c: &Connection, key: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(v) => c.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value", params![key, v]),
        None => c.execute("DELETE FROM settings WHERE key = ?1", [key]),
    }
    .map_err(err)?;
    Ok(())
}

pub fn settings_get(c: &Connection) -> Result<Settings, String> {
    Ok(Settings {
        enabled: setting(c, "metadata.enabled")?.map(|v| v == "1"),
        auto: setting(c, "metadata.auto")?.is_none_or(|v| v == "1"),
        email: setting(c, "metadata.email")?.filter(|e| !e.is_empty()),
    })
}

/// 邮箱只做最基本的检查：一个 @、两边不空、没有空白、不太长
fn check_email(e: &str) -> Result<(), String> {
    let ok = e.len() <= 200 && !e.chars().any(char::is_whitespace) && e.split('@').count() == 2 && e.split('@').all(|p| !p.is_empty()) && e.rsplit('@').next().is_some_and(|d| d.contains('.'));
    if ok { Ok(()) } else { Err(format!("邮箱格式不对：{e}")) }
}

pub fn settings_set(c: &Connection, p: &SettingsPatch) -> Result<Settings, String> {
    let tx = Tx::begin(c)?;
    if let Some(e) = &p.enabled {
        put_setting(&tx, "metadata.enabled", Some(if *e { "1" } else { "0" }))?;
    }
    if let Some(a) = &p.auto {
        put_setting(&tx, "metadata.auto", Some(if *a { "1" } else { "0" }))?;
    }
    if let Some(e) = &p.email {
        let e = e.trim();
        if !e.is_empty() {
            check_email(e)?;
        }
        put_setting(&tx, "metadata.email", Some(e).filter(|e| !e.is_empty()))?;
    }
    tx.commit()?;
    settings_get(c)
}

/// 自动查（导入后、改了 DOI / ISBN 后）：打开了联网、也打开了自动
pub fn auto_allowed(c: &Connection) -> bool {
    settings_get(c).is_ok_and(|s| s.enabled == Some(true) && s.auto)
}

/// 某次导入带来的、有 DOI 或 ISBN 的文献
pub fn run_items(c: &Connection, run_id: i64) -> Result<Vec<i64>, String> {
    let mut st = c
        .prepare("SELECT id FROM items WHERE import_run_id = ?1 AND deleted_at IS NULL AND (doi IS NOT NULL OR isbn IS NOT NULL) ORDER BY id")
        .map_err(err)?;
    st.query_map([run_id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 库里全部有 DOI 或 ISBN 的文献（设置页「补全全部文献」）
pub fn all_items(c: &Connection) -> Result<Vec<i64>, String> {
    let mut st = c.prepare("SELECT id FROM items WHERE deleted_at IS NULL AND (doi IS NOT NULL OR isbn IS NOT NULL) ORDER BY id").map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn require_enabled(c: &Connection) -> Result<Settings, String> {
    let s = settings_get(c)?;
    match s.enabled {
        Some(true) => Ok(s),
        Some(false) => Err("联网补全已关闭，可以在「设置」里打开".into()),
        None => Err("还没有打开联网补全：第一次使用前请先确认联网范围".into()),
    }
}

/// 这条文献可以用来查的标识：DOI 在前（字段或 extra 里的「DOI: …」），ISBN（统一成 13 位）在后。回收站里的不查。
pub fn identifiers_of(c: &Connection, item_id: i64) -> Result<Vec<Identifier>, String> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = c
        .query_row("SELECT doi, isbn, deleted_at FROM items WHERE id = ?1", [item_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(err)?;
    let Some((doi, isbn, None)) = row else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    if let Some(d) = doi.as_deref().and_then(text::normalize_doi) {
        out.push(Identifier::Doi(d));
    }
    if let Some(i) = isbn.as_deref().and_then(text::isbn13) {
        out.push(Identifier::Isbn(i));
    }
    Ok(out)
}

/// 用户粘贴的一段文字 → 标识（按空白、逗号、分号分开；DOI 可带 https://doi.org/、doi: 前缀；ISBN 可带连字符）。
/// 返回认出的（去重）和认不出的片段。
pub fn parse_identifiers(input: &str) -> (Vec<Identifier>, Vec<String>) {
    let mut ids: Vec<Identifier> = Vec::new();
    let mut bad = Vec::new();
    for tok in input.split(|c: char| c.is_whitespace() || c == ',' || c == '，' || c == ';' || c == '；').map(|t| t.trim_matches(['.', '。', '"', '“', '”'])) {
        if tok.is_empty() {
            continue;
        }
        let tok_l = tok.to_ascii_lowercase();
        let isbn_tok = tok_l.strip_prefix("isbn:").or_else(|| tok_l.strip_prefix("isbn")).unwrap_or(&tok_l).to_string();
        let id = if let Some(d) = text::normalize_doi(tok).filter(|d| d.contains('/') && d.len() > 7) {
            Some(Identifier::Doi(d))
        } else {
            isbn_checked(&isbn_tok).map(Identifier::Isbn)
        };
        match id {
            Some(id) if !ids.iter().any(|x| x.key() == id.key()) => ids.push(id),
            Some(_) => {}
            None if tok.eq_ignore_ascii_case("isbn") || tok.eq_ignore_ascii_case("doi") => {}
            None => bad.push(tok.to_string()),
        }
    }
    (ids, bad)
}

/// 用户输入的 ISBN：只由数字、连字符、X 组成，校验位对，统一成 13 位
fn isbn_checked(tok: &str) -> Option<String> {
    if !tok.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 'x' || c == 'X') {
        return None;
    }
    let d = text::normalize_isbn(tok)?;
    let digit = |b: u8| if b == b'X' { 10 } else { u32::from(b - b'0') };
    let ok = match d.len() {
        10 => d.bytes().enumerate().map(|(i, b)| digit(b) * (10 - i as u32)).sum::<u32>() % 11 == 0,
        13 => d.bytes().enumerate().map(|(i, b)| digit(b) * if i % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0,
        _ => false,
    };
    if ok { text::isbn13(&d) } else { None }
}

// ---------------- 比较：哪些补、哪些待确认 ----------------

/// 一处改动。kind：fill（原来空着）/ conflict（与现有不同）。
/// state：applied（已自动填）/ pending（待确认）/ accepted（用户采用）/ ignored（用户忽略）/ undone（已撤销）/ kept（要撤销时已被改过，保留）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// Zotero 字段名；另有 `creators`（作者整组）、`itemType`（文献类型）
    pub field: String,
    pub current: Option<String>,
    pub found: String,
    pub kind: String,
    pub state: String,
}

/// 文献现在的样子（比较用）
#[derive(Debug, Clone)]
pub struct Current {
    pub item_type: String,
    pub fields: BTreeMap<String, String>,
    pub creators: Vec<FoundCreator>,
}

fn current(c: &Connection, id: i64) -> Result<Current, String> {
    let item_type: String = c.query_row("SELECT item_type FROM items WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?.ok_or("找不到这条文献，可能已被删除")?;
    let mut st = c.prepare_cached("SELECT role, family, given, literal FROM creators WHERE item_id = ?1 ORDER BY position").map_err(err)?;
    let creators = st
        .query_map([id], |r| Ok(FoundCreator { role: r.get(0)?, family: r.get(1)?, given: r.get(2)?, literal: r.get(3)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(Current { item_type, fields: fields::load(c, id)?, creators })
}

/// 与现有不同时要用户确认的字段；其余字段（摘要、语言、ISSN、页数……）只在空着时补，不提示不同
const CONFLICT_FIELDS: &[&str] = &[
    "title", "date", "publicationTitle", "bookTitle", "proceedingsTitle", "volume", "issue", "pages", "publisher", "university", "institution",
];
/// 文献类型与查到的不同时，也照样补的字段
const UNIVERSAL: &[&str] = &["title", "date", "abstractNote", "language"];

/// 这种类型在 Zotero 里有没有这个字段（只管会补的这些；标题、日期、期刊 / 书名、摘要、语言各类型都有）。
/// 例如期刊文章没有「出版者」，Crossref 给了也不补。
pub fn allowed(item_type: &str, field: &str) -> bool {
    if UNIVERSAL.contains(&field) || fields::TITLE_FIELDS.contains(&field) || fields::DATE_FIELDS.contains(&field) || fields::CONTAINER_FIELDS.contains(&field) {
        return true;
    }
    if field == "DOI" || field == "ISBN" {
        return true; // 标识本身（新建时写入；补全时 plan 会跳过）
    }
    let list: &[&str] = match item_type {
        "journalArticle" => &["volume", "issue", "pages", "ISSN", "journalAbbreviation", "series"],
        "magazineArticle" => &["volume", "issue", "pages", "ISSN"],
        "newspaperArticle" => &["pages", "ISSN", "edition", "place"],
        "book" => &["publisher", "place", "edition", "numPages", "series", "seriesNumber", "volume"],
        "bookSection" => &["publisher", "place", "edition", "pages", "series", "seriesNumber", "volume"],
        "conferencePaper" => &["conferenceName", "publisher", "place", "volume", "pages", "series"],
        "thesis" => &["university", "place", "thesisType", "numPages"],
        "report" => &["institution", "place", "reportNumber", "reportType", "pages"],
        "encyclopediaArticle" | "dictionaryEntry" => &["publisher", "place", "edition", "pages", "volume"],
        _ => &[],
    };
    list.contains(&field)
}

/// 日期算不算一样：现有的年份等于查到的任何一个日期（出版、印刷、网络版）的年份
fn date_matches(current: &str, found: &Found, main: &str) -> bool {
    let year = |s: &str| text::parse_date(s).year;
    let Some(y) = year(current) else { return same("date", current, main) };
    std::iter::once(main).chain(found.date_alternatives.iter().map(String::as_str)).any(|d| year(d) == Some(y))
}

/// 字段的显示值（日期去掉排序值部分）
fn shown(field: &str, v: &str) -> String {
    if fields::DATE_FIELDS.contains(&field) { text::split_multipart(v).1 } else { v.trim().to_string() }
}

fn norm_pages(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).map(|c| if "‐‑‒–—−".contains(c) { '-' } else { c }).collect::<String>().to_lowercase()
}

fn norm_num(s: &str) -> String {
    let t = s.trim().to_lowercase();
    t.trim_start_matches('0').to_string()
}

/// 两个值算不算一样（只用在会提示不同的字段上）
pub fn same(field: &str, a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    if fields::DATE_FIELDS.contains(&field) {
        // 线上 / 印刷日期常差几天、精度也不同：年份一样就算一样
        let (ya, yb) = (text::parse_date(a).year, text::parse_date(b).year);
        return ya.is_some() && ya == yb;
    }
    match field {
        "pages" => norm_pages(a) == norm_pages(b),
        "volume" | "issue" | "numPages" | "edition" => norm_num(a) == norm_num(b),
        _ => {
            let (na, nb) = (text::title_norm(a), text::title_norm(b));
            !na.is_empty() && na == nb
        }
    }
}

fn creator_name(c: &FoundCreator) -> String {
    match (&c.literal, &c.family, &c.given) {
        (Some(l), _, _) => l.clone(),
        (None, Some(f), Some(g)) => format!("{f}, {g}"),
        (None, Some(f), None) => f.clone(),
        (None, None, Some(g)) => g.clone(),
        _ => String::new(),
    }
}

/// 作者整组的显示：「LeCun, Yann; Bengio, Yoshua」（编者等标注角色）
pub fn creators_text(list: &[FoundCreator]) -> String {
    list.iter()
        .map(|c| if c.role == "author" { creator_name(c) } else { format!("{}（{}）", creator_name(c), c.role) })
        .collect::<Vec<_>>()
        .join("; ")
}

/// 两组作者是否一样：只比作者（author）的姓（单栏的比整名），不分大小写与标点
fn same_creators(a: &[FoundCreator], b: &[FoundCreator]) -> bool {
    let key = |list: &[FoundCreator]| -> Vec<String> {
        list.iter()
            .filter(|c| c.role == "author")
            .map(|c| text::title_norm(c.literal.as_deref().or(c.family.as_deref()).or(c.given.as_deref()).unwrap_or("")))
            .collect()
    };
    let (ka, kb) = (key(a), key(b));
    ka.is_empty() || kb.is_empty() || ka == kb
}

/// 比较现有与查到的：空的补（fill），不同的待确认（conflict）。DOI / ISBN 本身不动。
pub fn plan(cur: &Current, found: &Found) -> Vec<Change> {
    let same_type = cur.item_type == found.item_type;
    let found_container = fields::natural_container(&found.item_type);
    let mut out = Vec::new();
    for (k, v) in &found.fields {
        if k == "DOI" || k == "ISBN" || v.trim().is_empty() {
            continue;
        }
        // 落到这条文献自己用的字段上：标题（案例是 caseName……）、日期、「期刊 / 书名」
        let target: &str = if k == "title" {
            fields::title_field(&cur.fields)
        } else if k == "date" {
            fields::date_field(&cur.fields)
        } else if Some(k.as_str()) == found_container || fields::CONTAINER_FIELDS.contains(&k.as_str()) {
            match fields::container_field(&cur.item_type, &cur.fields) {
                Some(f) => f,
                None => continue,
            }
        } else if same_type || UNIVERSAL.contains(&k.as_str()) {
            k
        } else {
            continue;
        };
        if !allowed(&cur.item_type, target) {
            continue;
        }
        let now = cur.fields.get(target).map(|x| shown(target, x)).filter(|x| !x.is_empty());
        let check_as = if fields::CONTAINER_FIELDS.contains(&target) {
            "publicationTitle"
        } else if fields::DATE_FIELDS.contains(&target) {
            "date"
        } else if fields::TITLE_FIELDS.contains(&target) {
            "title"
        } else {
            target
        };
        match now {
            None => out.push(Change { field: target.into(), current: None, found: v.clone(), kind: "fill".into(), state: "applied".into() }),
            Some(n) if CONFLICT_FIELDS.contains(&check_as) && !(if check_as == "date" { date_matches(&n, found, v) } else { same(check_as, &n, v) }) => {
                out.push(Change { field: target.into(), current: Some(n), found: v.clone(), kind: "conflict".into(), state: "pending".into() })
            }
            _ => {}
        }
    }
    if !found.creators.is_empty() {
        if cur.creators.is_empty() {
            out.push(Change { field: "creators".into(), current: None, found: creators_text(&found.creators), kind: "fill".into(), state: "applied".into() });
        } else if !same_creators(&cur.creators, &found.creators) {
            out.push(Change {
                field: "creators".into(),
                current: Some(creators_text(&cur.creators)),
                found: creators_text(&found.creators),
                kind: "conflict".into(),
                state: "pending".into(),
            });
        }
    }
    // 只有「文档」这种笼统类型才建议改类型（改类型会换掉能用的字段，所以要确认）
    if cur.item_type == "document" && found.item_type != "document" {
        out.push(Change { field: "itemType".into(), current: Some(cur.item_type.clone()), found: found.item_type.clone(), kind: "conflict".into(), state: "pending".into() });
    }
    out.sort_by_key(|c| (rank(&c.field), c.field.clone()));
    out
}

/// 显示顺序：标题、类型、作者、日期、期刊 / 书名、卷期页、出版者……，其余按字段名
fn rank(field: &str) -> usize {
    const ORDER: &[&str] = &["itemType", "creators", "date", "volume", "issue", "pages", "publisher", "place", "edition", "numPages", "ISSN", "language", "abstractNote"];
    if fields::TITLE_FIELDS.contains(&field) {
        0
    } else if fields::CONTAINER_FIELDS.contains(&field) {
        4
    } else {
        ORDER.iter().position(|f| *f == field).map_or(99, |i| if i >= 3 { i + 2 } else { i + 1 })
    }
}

fn creator_inputs(list: &[FoundCreator]) -> Vec<CreatorInput> {
    list.iter().map(|c| CreatorInput { role: c.role.clone(), family: c.family.clone(), given: c.given.clone(), literal: c.literal.clone() }).collect()
}

/// 把这些改动写进文献（在调用方的事务里）
fn write_changes(c: &Connection, item_id: i64, changes: &[&Change], found: &Found) -> Result<(), String> {
    let mut patch = ItemPatch::default();
    for ch in changes {
        match ch.field.as_str() {
            "creators" => patch.creators = Some(creator_inputs(&found.creators)),
            "itemType" => patch.item_type = Some(ch.found.clone()),
            f => {
                patch.fields.insert(f.to_string(), ch.found.clone());
            }
        }
    }
    if patch.fields.is_empty() && patch.creators.is_none() && patch.item_type.is_none() {
        return Ok(());
    }
    edit::item_update(c, item_id, &patch)?;
    Ok(())
}

// ---------------- 查询记录 ----------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupView {
    pub id: i64,
    pub item_id: i64,
    pub identifier: String,
    /// import / edit / manual / add
    pub trigger: String,
    /// found / not_found / failed
    pub status: String,
    pub source: Option<String>,
    pub message: Option<String>,
    pub changes: Vec<Change>,
    pub created_at: String,
    pub updated_at: String,
}

fn lookup_view(c: &Connection, id: i64) -> Result<LookupView, String> {
    c.query_row(
        "SELECT id, item_id, identifier, trigger, status, source, message, changes, created_at, updated_at FROM metadata_lookups WHERE id = ?1",
        [id],
        |r| {
            Ok(LookupView {
                id: r.get(0)?,
                item_id: r.get(1)?,
                identifier: r.get(2)?,
                trigger: r.get(3)?,
                status: r.get(4)?,
                source: r.get(5)?,
                message: r.get(6)?,
                changes: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这次联网补全的记录".to_string())
}

/// 这条文献最近一次联网补全
pub fn latest(c: &Connection, item_id: i64) -> Result<Option<LookupView>, String> {
    let id: Option<i64> = c.query_row("SELECT MAX(id) FROM metadata_lookups WHERE item_id = ?1", [item_id], |r| r.get(0)).map_err(err)?;
    id.map(|id| lookup_view(c, id)).transpose()
}

/// 联网查的结果
pub enum Fetched {
    Found { id: Identifier, found: Found },
    /// 每个标识都查过，都没有（最后一个的原因）
    NotFound { id: Identifier, message: String },
}

/// 依次按 DOI、ISBN 查，查到一个就停。
pub fn fetch(http: &dyn Http, pacer: &mut Pacer, ids: &[Identifier]) -> Result<Fetched, NetError> {
    let mut last = None;
    for id in ids {
        match net::lookup(http, pacer, id)? {
            Outcome::Found(found) => return Ok(Fetched::Found { id: id.clone(), found }),
            Outcome::NotFound(m) => last = Some((id.clone(), m)),
        }
    }
    let (id, message) = last.ok_or_else(|| NetError::Other("这条文献没有 DOI 或 ISBN".into()))?;
    Ok(Fetched::NotFound { id, message })
}

/// 一条文献这次补全的结果（汇总进度用）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ItemResult {
    pub found: bool,
    pub filled: usize,
    pub conflicts: usize,
}

/// 记下查询结果：查到时按**现在**的库重新比较（联网期间可能被改过），空的直接补上，不同的列为待确认。
pub fn record(c: &Connection, item_id: i64, trigger: &str, fetched: Result<Fetched, String>) -> Result<(ItemResult, Vec<DocRef>), String> {
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    let insert = |identifier: &str, status: &str, source: Option<&str>, message: Option<&str>, record: Option<String>, changes: &[Change]| -> Result<(), String> {
        tx.execute(
            "INSERT INTO metadata_lookups (item_id, identifier, trigger, status, source, message, record, changes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![item_id, identifier, trigger, status, source, message, record, serde_json::to_string(changes).map_err(err)?, now],
        )
        .map_err(err)?;
        Ok(())
    };
    let (result, refs) = match fetched {
        Err(message) => {
            let id = identifiers_of(&tx, item_id)?.first().map(Identifier::key).unwrap_or_default();
            insert(&id, "failed", None, Some(&message), None, &[])?;
            (ItemResult::default(), Vec::new())
        }
        Ok(Fetched::NotFound { id, message }) => {
            insert(&id.key(), "not_found", None, Some(&message), None, &[])?;
            (ItemResult::default(), Vec::new())
        }
        Ok(Fetched::Found { id, found }) => {
            let changes = plan(&current(&tx, item_id)?, &found);
            let fills: Vec<&Change> = changes.iter().filter(|ch| ch.state == "applied").collect();
            write_changes(&tx, item_id, &fills, &found)?;
            let res = ItemResult { found: true, filled: fills.len(), conflicts: changes.len() - fills.len() };
            insert(&id.key(), "found", Some(&found.source), None, Some(serde_json::to_string(&found).map_err(err)?), &changes)?;
            (res, if fills.is_empty() { Vec::new() } else { vec![DocRef::item(item_id)] })
        }
    };
    tx.commit()?;
    Ok((result, refs))
}

fn stored_found(c: &Connection, lookup_id: i64) -> Result<Found, String> {
    let rec: Option<String> = c.query_row("SELECT record FROM metadata_lookups WHERE id = ?1", [lookup_id], |r| r.get(0)).optional().map_err(err)?.flatten();
    rec.and_then(|r| serde_json::from_str(&r).ok()).ok_or_else(|| "这次补全没有查到记录".to_string())
}

fn save_changes(c: &Connection, lookup_id: i64, changes: &[Change]) -> Result<(), String> {
    c.execute(
        "UPDATE metadata_lookups SET changes = ?1, updated_at = ?2 WHERE id = ?3",
        params![serde_json::to_string(changes).map_err(err)?, text::now_iso(), lookup_id],
    )
    .map_err(err)?;
    Ok(())
}

/// 处理一处待确认：采用（写入查到的值）或忽略
pub fn resolve(c: &Connection, lookup_id: i64, field: &str, accept: bool) -> Result<(LookupView, Vec<DocRef>), String> {
    let mut view = lookup_view(c, lookup_id)?;
    let i = view.changes.iter().position(|ch| ch.field == field && ch.state == "pending").ok_or("这一处已经处理过了，请刷新")?;
    let tx = Tx::begin(c)?;
    let deleted: Option<String> = tx.query_row("SELECT deleted_at FROM items WHERE id = ?1", [view.item_id], |r| r.get(0)).optional().map_err(err)?.flatten();
    if deleted.is_some() {
        return Err("这条文献在回收站里，恢复后才能修改".into());
    }
    let mut refs = Vec::new();
    if accept {
        let found = stored_found(&tx, lookup_id)?;
        write_changes(&tx, view.item_id, &[&view.changes[i]], &found)?;
        refs.push(DocRef::item(view.item_id));
    }
    view.changes[i].state = (if accept { "accepted" } else { "ignored" }).into();
    save_changes(&tx, lookup_id, &view.changes)?;
    tx.commit()?;
    Ok((lookup_view(c, lookup_id)?, refs))
}

/// 撤销这次自动填的字段。之后被改过的（值已不是当时填的）保留不动。返回撤销了几处。
pub fn undo(c: &Connection, lookup_id: i64) -> Result<(LookupView, usize, Vec<DocRef>), String> {
    let mut view = lookup_view(c, lookup_id)?;
    let tx = Tx::begin(c)?;
    let cur = current(&tx, view.item_id)?;
    let found = stored_found(&tx, lookup_id).ok();
    let mut patch = ItemPatch::default();
    let mut n = 0;
    for ch in view.changes.iter_mut().filter(|ch| ch.state == "applied") {
        let unchanged = match ch.field.as_str() {
            "creators" => found.as_ref().is_some_and(|f| creators_text(&cur.creators) == creators_text(&f.creators)),
            f => cur.fields.get(f).is_some_and(|v| shown(f, v) == ch.found.trim()),
        };
        if !unchanged {
            ch.state = "kept".into();
            continue;
        }
        if ch.field == "creators" {
            patch.creators = Some(Vec::new());
        } else {
            patch.fields.insert(ch.field.clone(), String::new());
        }
        ch.state = "undone".into();
        n += 1;
    }
    if n == 0 && !view.changes.iter().any(|ch| ch.state == "kept") {
        return Err("这次补全没有可以撤销的改动".into());
    }
    if n > 0 {
        edit::item_update(&tx, view.item_id, &patch)?;
    }
    save_changes(&tx, lookup_id, &view.changes)?;
    tx.commit()?;
    Ok((lookup_view(c, lookup_id)?, n, if n > 0 { vec![DocRef::item(view.item_id)] } else { Vec::new() }))
}

/// 最近一次补全里还有待确认改动的文献（「待确认」视图）
pub const REVIEW_IDS: &str = "SELECT m.item_id FROM metadata_lookups m
     WHERE m.id = (SELECT MAX(x.id) FROM metadata_lookups x WHERE x.item_id = m.item_id)
       AND EXISTS (SELECT 1 FROM json_each(m.changes) j WHERE json_extract(j.value, '$.state') = 'pending')";

// ---------------- 排队：导入后、改了 DOI / ISBN 后在后台查 ----------------

/// 要不要查这条：回收站里的、没有 DOI / ISBN 的不查；导入触发的，这个标识查过（查到或确认没有）就不再查；
/// 改了标识触发的，标识和上次一样就不查；手动的总是查。
pub fn prepare(c: &Connection, item_id: i64, trigger: &str) -> Result<Option<Vec<Identifier>>, String> {
    let ids = identifiers_of(c, item_id)?;
    if ids.is_empty() {
        return Ok(None);
    }
    let keys: Vec<String> = ids.iter().map(Identifier::key).collect();
    let skip = match trigger {
        "import" => {
            let mut st = c.prepare_cached("SELECT identifier FROM metadata_lookups WHERE item_id = ?1 AND status IN ('found', 'not_found')").map_err(err)?;
            let done: Vec<String> = st.query_map([item_id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
            keys.iter().any(|k| done.contains(k))
        }
        "edit" => latest(c, item_id)?.is_some_and(|l| keys.contains(&l.identifier) && l.status != "failed"),
        _ => false,
    };
    Ok((!skip).then_some(ids))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub running: bool,
    /// 第几轮（界面靠它判断「这一轮结束了」）
    pub run: u64,
    pub total: usize,
    pub done: usize,
    /// 补上了字段的文献 / 补上的字段数
    pub filled_items: usize,
    pub filled_fields: usize,
    /// 有待确认改动的文献
    pub conflict_items: usize,
    pub not_found: usize,
    pub failed: usize,
    /// 没有 DOI / ISBN、已经查过而跳过的
    pub skipped: usize,
    /// 停下来的原因（用户停止、连不上网络）
    pub stopped: Option<String>,
    pub finished_at: Option<String>,
}

/// 待查的队列与进度。`push` 返回 true 时调用方要起一个线程跑 `work`。
#[derive(Default)]
pub struct Queue {
    jobs: Mutex<VecDeque<(i64, String)>>,
    progress: Mutex<Progress>,
    cancel: AtomicBool,
    /// 有线程在跑（在 jobs 锁里读写，避免漏掉刚放进来的）
    running: AtomicBool,
}

impl Queue {
    pub fn push(&self, items: &[i64], trigger: &str) -> bool {
        if items.is_empty() {
            return false;
        }
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let new: Vec<(i64, String)> = items.iter().filter(|i| !jobs.iter().any(|(j, _)| j == *i)).map(|&i| (i, trigger.to_string())).collect();
        jobs.extend(new);
        let start = !self.running.load(Ordering::SeqCst);
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if start {
            self.running.store(true, Ordering::SeqCst);
            self.cancel.store(false, Ordering::SeqCst);
            *p = Progress { running: true, run: p.run + 1, total: jobs.len(), ..Default::default() };
        } else {
            p.total = p.done + jobs.len();
        }
        start
    }

    /// 取下一条；没有了就在同一把锁里标记结束（不会和刚放进来的新一轮打架）
    fn pop(&self) -> Option<(i64, String)> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.pop_front();
        if job.is_none() {
            self.running.store(false, Ordering::SeqCst);
            self.update(finish);
        }
        job
    }

    fn update(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.progress.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// 停下：清空队列，正在查的这一条查完就停
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn status(&self) -> Progress {
        self.progress.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn stop(&self, reason: &str) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let dropped = jobs.len();
        jobs.clear();
        self.running.store(false, Ordering::SeqCst);
        self.update(|p| {
            p.stopped = Some(reason.to_string());
            p.total -= dropped.min(p.total);
            p.total = p.total.max(p.done);
            finish(p);
        });
    }
}

/// 后台线程借用数据库的方式（App 里是 AppState，测试里是一个连接）
pub trait Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String>;
}

/// 跑完队列。开始前做一份 `before-metadata` 备份（`backups` 为 None 时不做）；连续 3 次连不上网络就停。
pub fn work<D: Db>(db: &D, http: &dyn Http, pacer: &mut Pacer, queue: &Queue, backups: Option<&std::path::Path>) {
    if let Some(dir) = backups {
        let made = db.with(|c| {
            crate::backup::snapshot(c, dir, "before-metadata")?;
            crate::backup::prune_safety(dir, crate::backup::KEEP_SAFETY)
        });
        if let Err(e) = made {
            queue.stop(&format!("补全前备份失败，没有联网：{e}"));
            return;
        }
    }
    let mut offline = 0;
    loop {
        if queue.cancel.load(Ordering::SeqCst) {
            queue.stop("已停止");
            break;
        }
        // 用户中途关掉了联网补全：剩下的不查
        if !db.with(|c| Ok(settings_get(c)?.enabled == Some(true))).unwrap_or(false) {
            queue.stop("联网补全已关闭，剩下的没有查");
            break;
        }
        let Some((item, trigger)) = queue.pop() else { break };
        let ids = match db.with(|c| prepare(c, item, &trigger)) {
            Ok(Some(ids)) => ids,
            Ok(None) => {
                queue.update(|p| {
                    p.done += 1;
                    p.skipped += 1;
                });
                continue;
            }
            Err(_) => {
                queue.update(|p| {
                    p.done += 1;
                    p.failed += 1;
                });
                continue;
            }
        };
        let fetched = match fetch(http, pacer, &ids) {
            Err(NetError::Offline(m)) => {
                offline += 1;
                queue.update(|p| {
                    p.done += 1;
                    p.failed += 1;
                });
                if offline >= 3 {
                    queue.stop(&format!("连不上网络，已停止（{m}）"));
                    break;
                }
                continue;
            }
            Err(e) => Err(e.message()),
            Ok(f) => Ok(f),
        };
        offline = 0;
        let not_found = matches!(fetched, Ok(Fetched::NotFound { .. }));
        let failed = fetched.is_err();
        let res = db.with(|c| {
            let (res, refs) = record(c, item, &trigger, fetched)?;
            if let Err(e) = crate::search::update(c, &refs) {
                eprintln!("更新搜索索引失败：{e}");
            }
            Ok(res)
        });
        queue.update(|p| {
            p.done += 1;
            match res {
                Ok(r) => {
                    p.filled_items += usize::from(r.filled > 0);
                    p.filled_fields += r.filled;
                    p.conflict_items += usize::from(r.conflicts > 0);
                    p.not_found += usize::from(not_found);
                    p.failed += usize::from(failed);
                }
                Err(_) => p.failed += 1,
            }
        });
    }
}

fn finish(p: &mut Progress) {
    p.running = false;
    p.finished_at = Some(text::now_iso());
}

// ---------------- 按 DOI / ISBN 新建文献 ----------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Existing {
    pub identifier: String,
    pub item_id: i64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missed {
    pub identifier: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReport {
    pub created: Vec<i64>,
    /// 库里已经有（没有重复添加）
    pub existing: Vec<Existing>,
    pub not_found: Vec<Missed>,
    /// 认不出是 DOI 还是 ISBN 的片段
    pub invalid: Vec<String>,
    /// 联网失败（后面的没有再查）
    pub failed: Option<String>,
}

/// 一次最多添加多少个
pub const MAX_ADD: usize = 50;

/// 第一步（读库）：认出标识，库里已有的挑出来。返回要联网查的和报告的开头。
pub fn add_prepare(c: &Connection, input: &str) -> Result<(Vec<Identifier>, AddReport), String> {
    require_enabled(c)?;
    let (ids, invalid) = parse_identifiers(input);
    if ids.is_empty() {
        return Err(if invalid.is_empty() { "没有输入 DOI 或 ISBN".into() } else { format!("认不出 DOI 或 ISBN：{}", invalid.join("、")) });
    }
    if ids.len() > MAX_ADD {
        return Err(format!("一次最多添加 {MAX_ADD} 个"));
    }
    let mut report = AddReport { invalid, ..Default::default() };
    let mut todo = Vec::new();
    for id in ids {
        let hit: Option<(i64, Option<String>)> = match &id {
            Identifier::Doi(d) => c.query_row("SELECT id, title FROM items WHERE deleted_at IS NULL AND lower(doi) = ?1", [d.to_lowercase()], |r| Ok((r.get(0)?, r.get(1)?))),
            Identifier::Isbn(i) => c.query_row("SELECT id, title FROM items WHERE deleted_at IS NULL AND isbn = ?1", [i], |r| Ok((r.get(0)?, r.get(1)?))),
        }
        .optional()
        .map_err(err)?;
        match hit {
            Some((item_id, title)) => report.existing.push(Existing { identifier: id.label(), item_id, title }),
            None => todo.push(id),
        }
    }
    Ok((todo, report))
}

/// 第三步（写库）：按查到的记录新建一条文献（来源记为 lookup），记一行补全记录。
pub fn add_create(c: &Connection, collection: Option<i64>, id: &Identifier, found: &Found) -> Result<(i64, Vec<DocRef>), String> {
    let tx = Tx::begin(c)?;
    let item_id = edit::item_create(&tx, &found.item_type, collection)?;
    let mut patch = ItemPatch { creators: Some(creator_inputs(&found.creators)), ..Default::default() };
    for (k, v) in found.fields.iter().filter(|(k, _)| allowed(&found.item_type, k)) {
        patch.fields.insert(k.clone(), v.clone());
    }
    // 标识本身：DOI 字段（没有 DOI 字段的类型 Zotero 也照样存）；ISBN
    match id {
        Identifier::Doi(d) => {
            patch.fields.insert("DOI".into(), d.clone());
        }
        Identifier::Isbn(i) => {
            patch.fields.entry("ISBN".into()).or_insert_with(|| i.clone());
        }
    }
    edit::item_update(&tx, item_id, &patch)?;
    let meta = serde_json::json!({ "identifier": id.key(), "source": found.source });
    tx.execute("UPDATE items SET source = 'lookup', source_key = ?1, source_meta = ?2 WHERE id = ?3", params![id.key(), meta.to_string(), item_id])
        .map_err(err)?;
    let changes: Vec<Change> = patch
        .fields
        .iter()
        .map(|(k, v)| Change { field: k.clone(), current: None, found: v.clone(), kind: "fill".into(), state: "applied".into() })
        .collect();
    tx.execute(
        "INSERT INTO metadata_lookups (item_id, identifier, trigger, status, source, record, changes, created_at, updated_at)
         VALUES (?1, ?2, 'add', 'found', ?3, ?4, ?5, ?6, ?6)",
        params![item_id, id.key(), found.source, serde_json::to_string(found).map_err(err)?, serde_json::to_string(&changes).map_err(err)?, text::now_iso()],
    )
    .map_err(err)?;
    tx.commit()?;
    Ok((item_id, vec![DocRef::item(item_id)]))
}

#[cfg(test)]
mod tests;
