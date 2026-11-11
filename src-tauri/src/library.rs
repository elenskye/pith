//! 文献库的只读查询：列表（范围 + 搜索 + 标签 / 类型筛选 + 排序）、详情、集合树、标签。写入在 `import`（Phase 2）与 `edit`（Phase 3）。
//! 详情只给常用字段，其余原始字段收在「其他字段」里（用户要求：存全不等于显示全）。

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::common::err;
use crate::fields;
use crate::search::{self, DocKind, DocRef};
use crate::text::html_to_text;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub items: i64,
    /// 回收站里直接删除的条目、附件、笔记、集合
    pub trash: i64,
    pub collections: i64,
    pub tags: i64,
    pub attachments: i64,
    pub notes: i64,
    pub missing_files: i64,
    /// 不在任何集合里的文献
    pub unfiled: i64,
    /// 可能重复的组数
    pub duplicates: i64,
    /// 联网补全后有「待确认」改动的文献
    pub review: i64,
}

pub fn overview(c: &Connection) -> Result<Overview, String> {
    let n = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).map_err(err);
    Ok(Overview {
        items: n("SELECT COUNT(*) FROM items WHERE deleted_at IS NULL")?,
        trash: n("SELECT (SELECT COUNT(*) FROM items WHERE deleted_at IS NOT NULL) + (SELECT COUNT(*) FROM attachments WHERE deleted_at IS NOT NULL)
                       + (SELECT COUNT(*) FROM notes WHERE deleted_at IS NOT NULL) + (SELECT COUNT(*) FROM collections WHERE deleted_at IS NOT NULL)")?,
        collections: n(&format!("{LIVE_COLLECTIONS} SELECT COUNT(*) FROM live"))?,
        tags: n("SELECT COUNT(*) FROM tags")?,
        attachments: n("SELECT COUNT(*) FROM attachments WHERE deleted_at IS NULL AND kind != 'embedded_image'")?,
        notes: n("SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL")?,
        missing_files: n("SELECT COUNT(*) FROM attachments WHERE deleted_at IS NULL AND status = 'missing'")?,
        unfiled: n(&format!(
            "{LIVE_COLLECTIONS} SELECT COUNT(*) FROM items i WHERE i.deleted_at IS NULL
                AND NOT EXISTS (SELECT 1 FROM collection_items x WHERE x.item_id = i.id AND x.collection_id IN live)"
        ))?,
        duplicates: crate::dedup::groups(c)?.len() as i64,
        review: n(&format!("SELECT COUNT(*) FROM items WHERE deleted_at IS NULL AND id IN ({})", crate::metadata::REVIEW_IDS))?,
    })
}

/// 不在回收站（自己和所有上级都没删）的集合
const LIVE_COLLECTIONS: &str = "WITH RECURSIVE live(id) AS (
    SELECT id FROM collections WHERE parent_id IS NULL AND deleted_at IS NULL
    UNION ALL SELECT c.id FROM collections c JOIN live l ON c.parent_id = l.id WHERE c.deleted_at IS NULL)";

/// 列表看哪一部分
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    All,
    /// 不在任何集合里的
    Unfiled,
    Collection {
        id: i64,
        /// 连同下级集合里的
        #[serde(default, rename = "includeChildren")]
        include_children: bool,
    },
    Trash,
    /// 有附件文件缺失的
    Missing,
    /// 可能重复的（按组排列）
    Duplicates,
    /// 联网补全后有待确认改动的
    Review,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub scope: Scope,
    /// 搜索词（标题、作者、标签、摘要、笔记……）；空 = 不搜索
    #[serde(default)]
    pub q: String,
    /// 同时带有这些标签的
    #[serde(default)]
    pub tags: Vec<String>,
    /// 文献类型（Zotero 类型名）；`attachment` / `note` 只看独立附件 / 笔记
    #[serde(default)]
    pub item_type: Option<String>,
    /// added（加入时间；回收站里是删除时间）/ title / creator / year / type / relevance。空：搜索时按相关度，否则按加入时间新的在前
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub desc: bool,
}

impl ListQuery {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn of(scope: Scope) -> Self {
        ListQuery { scope, q: String::new(), tags: vec![], item_type: None, sort: None, desc: false }
    }
}

/// 列表里的一行：文献条目，或独立附件 / 独立笔记（Zotero 里它们也是顶层条目），回收站里还有集合。
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
    /// item / attachment / note / collection
    pub kind: String,
    pub id: i64,
    pub uid: String,
    /// 条目类型（Zotero 类型名）；附件为 MIME 类型
    pub item_type: Option<String>,
    pub title: Option<String>,
    /// 第一作者（中文单栏姓名原样；西文「姓 名」）
    pub first_creator: Option<String>,
    pub creator_count: i64,
    pub year: Option<i64>,
    pub container_title: Option<String>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    /// 条目：子附件数；附件：0
    pub attachments: i64,
    pub notes: i64,
    /// 条目：子附件里缺文件的个数；附件：缺文件为 1
    pub missing_files: i64,
    /// 「可能重复」视图里的组号（从 0 起）与原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_reason: Option<String>,
}

const CREATOR_NAME: &str = "COALESCE(cr.literal, TRIM(COALESCE(cr.family, '') || ' ' || COALESCE(cr.given, '')))";

fn note_title(title: Option<String>, html: &str) -> Option<String> {
    title.filter(|t| !t.trim().is_empty()).or_else(|| html_to_text(html).lines().next().map(|l| l.chars().take(80).collect()))
}

fn ids_sql(ids: &[i64]) -> String {
    if ids.is_empty() { "NULL".into() } else { ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",") }
}

/// 集合本身，加上（需要时）它所有不在回收站的下级集合
fn collection_ids(c: &Connection, id: i64, include_children: bool) -> Result<Vec<i64>, String> {
    if !include_children {
        return Ok(vec![id]);
    }
    let mut st = c
        .prepare(
            "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT c.id FROM collections c JOIN sub s ON c.parent_id = s.id WHERE c.deleted_at IS NULL)
             SELECT id FROM sub",
        )
        .map_err(err)?;
    st.query_map([id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

fn row_base(kind: &str, r: &Row, title: Option<String>) -> rusqlite::Result<ListRow> {
    Ok(ListRow {
        kind: kind.into(),
        id: r.get(0)?,
        uid: r.get(1)?,
        item_type: None,
        title,
        first_creator: None,
        creator_count: 0,
        year: None,
        container_title: None,
        created_at: r.get("created_at")?,
        deleted_at: r.get("deleted_at")?,
        attachments: 0,
        notes: 0,
        missing_files: 0,
        group: None,
        group_reason: None,
    })
}

/// 同时带有全部这些标签的记录 (kind, id)
fn tagged(c: &Connection, tags: &[String]) -> Result<HashSet<(&'static str, i64)>, String> {
    let mut out = HashSet::new();
    let marks = vec!["?"; tags.len()].join(",");
    for (kind, table, col) in [("item", "item_tags", "item_id"), ("attachment", "attachment_tags", "attachment_id"), ("note", "note_tags", "note_id")] {
        let sql = format!(
            "SELECT x.{col} FROM {table} x JOIN tags t ON t.id = x.tag_id WHERE t.name IN ({marks}) GROUP BY x.{col} HAVING COUNT(DISTINCT t.id) = {}",
            tags.len()
        );
        let mut st = c.prepare(&sql).map_err(err)?;
        for id in st.query_map(params_from_iter(tags), |r| r.get::<_, i64>(0)).map_err(err)? {
            out.insert((kind, id.map_err(err)?));
        }
    }
    Ok(out)
}

fn doc_ref(kind: &str, id: i64) -> Option<DocRef> {
    Some(DocRef {
        kind: match kind {
            "item" => DocKind::Item,
            "attachment" => DocKind::Attachment,
            "note" => DocKind::Note,
            _ => return None,
        },
        id,
    })
}

/// None 永远排在最后（不论升序降序），与 Zotero 一致
fn cmp_opt<T: Ord>(a: &Option<T>, b: &Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            if desc {
                y.cmp(x)
            } else {
                x.cmp(y)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn items_list(c: &Connection, query: &ListQuery) -> Result<Vec<ListRow>, String> {
    let trash = query.scope == Scope::Trash;
    let mut groups: HashMap<i64, (usize, String)> = HashMap::new();
    let coll_ids = match &query.scope {
        Scope::Collection { id, include_children } => ids_sql(&collection_ids(c, *id, *include_children)?),
        _ => String::new(),
    };
    if query.scope == Scope::Duplicates {
        for (n, g) in crate::dedup::groups(c)?.into_iter().enumerate() {
            for id in g.items {
                groups.insert(id, (n, g.reasons.join("、")));
            }
        }
    }
    let dup_ids = ids_sql(&groups.keys().copied().collect::<Vec<_>>());
    let not_in_live = |table: &str, col: &str, alias: &str| format!("NOT EXISTS (SELECT 1 FROM {table} x WHERE x.{col} = {alias}.id AND x.collection_id IN live)");
    let in_colls = |table: &str, col: &str, alias: &str| format!("{alias}.id IN (SELECT {col} FROM {table} WHERE collection_id IN ({coll_ids}))");

    let item_cond = match &query.scope {
        Scope::All => "i.deleted_at IS NULL".to_string(),
        Scope::Unfiled => format!("i.deleted_at IS NULL AND {}", not_in_live("collection_items", "item_id", "i")),
        Scope::Collection { .. } => format!("i.deleted_at IS NULL AND {}", in_colls("collection_items", "item_id", "i")),
        Scope::Trash => "i.deleted_at IS NOT NULL".to_string(),
        Scope::Missing => "i.deleted_at IS NULL AND EXISTS (SELECT 1 FROM attachments a WHERE a.item_id = i.id AND a.deleted_at IS NULL AND a.status = 'missing')".into(),
        Scope::Duplicates => format!("i.deleted_at IS NULL AND i.id IN ({dup_ids})"),
        Scope::Review => format!("i.deleted_at IS NULL AND i.id IN ({})", crate::metadata::REVIEW_IDS),
    };
    let standalone_att = "a.deleted_at IS NULL AND a.item_id IS NULL AND a.note_id IS NULL";
    let att_cond = match &query.scope {
        Scope::All => Some(standalone_att.to_string()),
        Scope::Unfiled => Some(format!("{standalone_att} AND {}", not_in_live("collection_attachments", "attachment_id", "a"))),
        Scope::Collection { .. } => Some(format!("{standalone_att} AND {}", in_colls("collection_attachments", "attachment_id", "a"))),
        // 回收站里列出所有直接删除的附件（含子附件）
        Scope::Trash => Some("a.deleted_at IS NOT NULL".into()),
        Scope::Missing => Some(format!("{standalone_att} AND a.status = 'missing'")),
        Scope::Duplicates | Scope::Review => None,
    };
    let standalone_note = "n.deleted_at IS NULL AND n.item_id IS NULL";
    let note_cond = match &query.scope {
        Scope::All => Some(standalone_note.to_string()),
        Scope::Unfiled => Some(format!("{standalone_note} AND {}", not_in_live("collection_notes", "note_id", "n"))),
        Scope::Collection { .. } => Some(format!("{standalone_note} AND {}", in_colls("collection_notes", "note_id", "n"))),
        Scope::Trash => Some("n.deleted_at IS NOT NULL".into()),
        Scope::Missing | Scope::Duplicates | Scope::Review => None,
    };

    let mut rows: Vec<ListRow> = Vec::new();
    let sql = format!(
        "{LIVE_COLLECTIONS}
         SELECT i.id, i.uid, i.item_type, i.title,
                (SELECT {CREATOR_NAME} FROM creators cr WHERE cr.item_id = i.id ORDER BY cr.position LIMIT 1),
                (SELECT COUNT(*) FROM creators cr WHERE cr.item_id = i.id),
                i.year, i.container_title, i.created_at AS created_at, i.deleted_at AS deleted_at,
                (SELECT COUNT(*) FROM attachments a WHERE a.item_id = i.id AND a.deleted_at IS NULL),
                (SELECT COUNT(*) FROM notes n WHERE n.item_id = i.id AND n.deleted_at IS NULL),
                (SELECT COUNT(*) FROM attachments a WHERE a.item_id = i.id AND a.deleted_at IS NULL AND a.status = 'missing')
           FROM items i WHERE {item_cond}"
    );
    let mut st = c.prepare(&sql).map_err(err)?;
    let mapped = st
        .query_map([], |r| {
            let mut row = row_base("item", r, r.get(3)?)?;
            row.item_type = r.get(2)?;
            row.first_creator = r.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty());
            row.creator_count = r.get(5)?;
            row.year = r.get(6)?;
            row.container_title = r.get(7)?;
            row.attachments = r.get(10)?;
            row.notes = r.get(11)?;
            row.missing_files = r.get(12)?;
            Ok(row)
        })
        .map_err(err)?;
    rows.extend(mapped.collect::<Result<Vec<_>, _>>().map_err(err)?);

    if let Some(cond) = att_cond {
        let sql = format!(
            "{LIVE_COLLECTIONS}
             SELECT a.id, a.uid, a.content_type, COALESCE(a.title, a.original_name), a.created_at AS created_at, a.deleted_at AS deleted_at, a.status = 'missing'
               FROM attachments a WHERE {cond} AND a.kind != 'embedded_image'"
        );
        let mut st = c.prepare(&sql).map_err(err)?;
        let mapped = st
            .query_map([], |r| {
                let mut row = row_base("attachment", r, r.get(3)?)?;
                row.item_type = r.get(2)?;
                row.missing_files = r.get::<_, bool>(6)? as i64;
                Ok(row)
            })
            .map_err(err)?;
        rows.extend(mapped.collect::<Result<Vec<_>, _>>().map_err(err)?);
    }
    if let Some(cond) = note_cond {
        let sql = format!("{LIVE_COLLECTIONS} SELECT n.id, n.uid, n.title, n.html, n.created_at AS created_at, n.deleted_at AS deleted_at FROM notes n WHERE {cond}");
        let mut st = c.prepare(&sql).map_err(err)?;
        let mapped = st.query_map([], |r| row_base("note", r, note_title(r.get(2)?, &r.get::<_, String>(3)?))).map_err(err)?;
        rows.extend(mapped.collect::<Result<Vec<_>, _>>().map_err(err)?);
    }
    if trash {
        let mut st = c.prepare("SELECT id, uid, name, created_at, deleted_at FROM collections WHERE deleted_at IS NOT NULL").map_err(err)?;
        let mapped = st.query_map([], |r| row_base("collection", r, r.get(2)?)).map_err(err)?;
        rows.extend(mapped.collect::<Result<Vec<_>, _>>().map_err(err)?);
    }

    // ---- 筛选 ----
    if let Some(t) = query.item_type.as_deref().filter(|t| !t.is_empty()) {
        rows.retain(|r| match t {
            "attachment" | "note" => r.kind == t,
            ty => r.kind == "item" && r.item_type.as_deref() == Some(ty),
        });
    }
    let tags: Vec<String> = query.tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    if !tags.is_empty() {
        let set = tagged(c, &tags)?;
        rows.retain(|r| set.iter().any(|(k, id)| *k == r.kind && *id == r.id));
    }
    let hits = search::query(c, &query.q)?;
    if let Some(h) = &hits {
        let needle = query.q.trim().to_lowercase();
        rows.retain(|r| match doc_ref(&r.kind, r.id) {
            Some(d) => h.contains_key(&d),
            // 回收站里的集合按名字找
            None => r.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&needle)),
        });
    }

    // ---- 排序 ----
    for r in rows.iter_mut() {
        if let Some((g, reason)) = groups.get(&r.id).filter(|_| r.kind == "item") {
            r.group = Some(*g);
            r.group_reason = Some(reason.clone());
        }
    }
    let sort = query.sort.as_deref().unwrap_or(if hits.is_some() { "relevance" } else { "added" });
    let desc = if query.sort.is_none() { sort == "added" } else { query.desc };
    let rank = |r: &ListRow| doc_ref(&r.kind, r.id).and_then(|d| hits.as_ref()?.get(&d).copied());
    let lower = |s: &Option<String>| s.as_ref().map(|t| t.to_lowercase());
    rows.sort_by(|a, b| {
        let primary = if query.scope == Scope::Duplicates {
            a.group.cmp(&b.group).then(b.created_at.cmp(&a.created_at))
        } else {
            match sort {
                "title" => cmp_opt(&lower(&a.title), &lower(&b.title), desc),
                "creator" => cmp_opt(&lower(&a.first_creator), &lower(&b.first_creator), desc),
                "year" => cmp_opt(&a.year, &b.year, desc),
                "type" => cmp_opt(&a.item_type, &b.item_type, desc),
                "relevance" => rank(a).partial_cmp(&rank(b)).unwrap_or(Ordering::Equal).then(b.created_at.cmp(&a.created_at)),
                _ => {
                    let key = |r: &ListRow| if trash { r.deleted_at.clone() } else { Some(r.created_at.clone()) };
                    cmp_opt(&key(a), &key(b), desc)
                }
            }
        };
        primary.then(b.id.cmp(&a.id))
    });
    Ok(rows)
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub id: i64,
    pub name: String,
    /// 带这个标签、不在回收站的文献 / 独立附件 / 笔记数
    pub count: i64,
}

/// 用着的标签（只挂在标注上的不列），按名字排序。
pub fn tags_list(c: &Connection) -> Result<Vec<TagCount>, String> {
    let mut st = c
        .prepare(
            "SELECT * FROM (
               SELECT t.id, t.name,
                      (SELECT COUNT(*) FROM item_tags x JOIN items i ON i.id = x.item_id AND i.deleted_at IS NULL WHERE x.tag_id = t.id)
                    + (SELECT COUNT(*) FROM attachment_tags x JOIN attachments a ON a.id = x.attachment_id AND a.deleted_at IS NULL WHERE x.tag_id = t.id)
                    + (SELECT COUNT(*) FROM note_tags x JOIN notes n ON n.id = x.note_id AND n.deleted_at IS NULL WHERE x.tag_id = t.id) AS n
                 FROM tags t)
              WHERE n > 0 ORDER BY name COLLATE NOCASE",
        )
        .map_err(err)?;
    st.query_map([], |r| Ok(TagCount { id: r.get(0)?, name: r.get(1)?, count: r.get(2)? })).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Creator {
    pub role: String,
    /// 显示用的全名
    pub name: String,
    pub family: Option<String>,
    pub given: Option<String>,
    pub literal: Option<String>,
}

/// 集合的引用（详情里显示、移出集合用）
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NamedRef {
    pub id: i64,
    pub name: String,
}

/// 「其他字段」里的一项（显示值：日期字段去掉了排序值）
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldValue {
    pub name: String,
    pub value: String,
}

/// 条目详情里的子附件
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChildAttachment {
    pub id: i64,
    pub title: Option<String>,
    /// stored / linked_file / linked_url / embedded_image
    pub kind: String,
    pub content_type: Option<String>,
    /// ok / missing
    pub status: String,
    pub size: Option<i64>,
    pub url: Option<String>,
    /// 能在 App 的阅读器里打开（PDF 且文件在）
    pub readable: bool,
    pub annotation_count: i64,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChildNote {
    pub id: i64,
    pub title: Option<String>,
    /// 纯文本开头一段
    pub preview: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub id: i64,
    pub uid: String,
    pub item_type: String,
    pub title: Option<String>,
    pub creators: Vec<Creator>,
    /// 日期原文
    pub date: Option<String>,
    pub year: Option<i64>,
    pub container_title: Option<String>,
    /// 「期刊 / 书名」用的字段（publicationTitle / bookTitle / …）；这种类型没有就是空，界面不显示这一栏
    pub container_field: Option<String>,
    /// 下面几个是字段里的原值（列里的 DOI / ISBN 是规范化过的）
    pub doi: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub extra: Option<String>,
    /// 常用字段以外的全部字段，按名字排序
    pub other_fields: Vec<FieldValue>,
    pub tags: Vec<String>,
    pub collections: Vec<NamedRef>,
    pub attachments: Vec<ChildAttachment>,
    pub notes: Vec<ChildNote>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

fn strings(c: &Connection, sql: &str, id: i64) -> Result<Vec<String>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    st.query_map(params![id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 所在的集合（不含回收站里的集合）
fn collection_refs(c: &Connection, table: &str, col: &str, id: i64) -> Result<Vec<NamedRef>, String> {
    let mut st = c
        .prepare(&format!(
            "{LIVE_COLLECTIONS} SELECT c.id, c.name FROM {table} x JOIN collections c ON c.id = x.collection_id WHERE x.{col} = ?1 AND c.id IN live ORDER BY c.name"
        ))
        .map_err(err)?;
    st.query_map(params![id], |r| Ok(NamedRef { id: r.get(0)?, name: r.get(1)? })).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn item_get(c: &Connection, id: i64) -> Result<ItemDetail, String> {
    let mut st = c
        .prepare(&format!(
            "SELECT cr.role, {CREATOR_NAME}, cr.family, cr.given, cr.literal FROM creators cr WHERE cr.item_id = ?1 ORDER BY cr.position"
        ))
        .map_err(err)?;
    let creators = st
        .query_map(params![id], |r| {
            Ok(Creator {
                role: r.get(0)?,
                name: r.get(1)?,
                family: r.get(2)?,
                given: r.get(3)?,
                literal: r.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let tags = strings(
        c,
        "SELECT t.name FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = ?1 ORDER BY t.name",
        id,
    )?;
    let collections = collection_refs(c, "collection_items", "item_id", id)?;
    let mut st = c
        .prepare(&format!(
            "SELECT a.id, COALESCE(a.title, a.original_name), a.kind, a.content_type, a.status, f.size, a.url, {},
                    (SELECT COUNT(*) FROM annotations x WHERE x.attachment_id = a.id AND x.deleted_at IS NULL)
               FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256
              WHERE a.item_id = ?1 AND a.deleted_at IS NULL ORDER BY a.created_at, a.id",
            crate::reader::READABLE
        ))
        .map_err(err)?;
    let attachments = st
        .query_map(params![id], |r| {
            Ok(ChildAttachment {
                id: r.get(0)?,
                title: r.get(1)?,
                kind: r.get(2)?,
                content_type: r.get(3)?,
                status: r.get(4)?,
                size: r.get(5)?,
                url: r.get(6)?,
                readable: r.get(7)?,
                annotation_count: r.get(8)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = c
        .prepare("SELECT id, title, html FROM notes WHERE item_id = ?1 AND deleted_at IS NULL ORDER BY created_at, id")
        .map_err(err)?;
    let notes = st
        .query_map(params![id], |r| {
            let html: String = r.get(2)?;
            let title = note_title(r.get(1)?, &html);
            // 预览不重复标题：Zotero 的笔记标题就是正文第一行
            let text = html_to_text(&html);
            let body = match (text.split_once('\n'), &title) {
                (Some((first, rest)), Some(t)) if first.trim() == t.trim() => rest.to_string(),
                (None, Some(t)) if text.trim() == t.trim() => String::new(),
                _ => text,
            };
            Ok(ChildNote {
                id: r.get(0)?,
                title,
                preview: body.chars().take(160).collect(),
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let f = fields::load(c, id)?;
    let item_type: String =
        c.query_row("SELECT item_type FROM items WHERE id = ?1", params![id], |r| r.get(0)).optional().map_err(err)?.ok_or("找不到这条文献，可能已被删除")?;
    let (title_f, date_f) = (fields::title_field(&f), fields::date_field(&f));
    let container_f = fields::container_field(&item_type, &f);
    let shown: Vec<&str> = [Some(title_f), Some(date_f), container_f, Some("DOI"), Some("ISBN"), Some("language"), Some("abstractNote"), Some("extra")].into_iter().flatten().collect();
    let get = |k: &str| f.get(k).map(|v| fields::display_value(k, v)).filter(|v| !v.trim().is_empty());
    let other_fields = f.iter().filter(|(k, _)| !shown.contains(&k.as_str())).map(|(k, v)| FieldValue { name: k.clone(), value: fields::display_value(k, v) }).collect();
    // 字段里的原值优先；没有字段时（不经导入、直接写列的数据）退回到列
    c.query_row(
        "SELECT id, uid, item_type, year, container_title, source, created_at, updated_at, deleted_at, title, date_raw, doi, isbn, language, abstract, extra
           FROM items WHERE id = ?1",
        params![id],
        |r| {
            Ok(ItemDetail {
                id: r.get(0)?,
                uid: r.get(1)?,
                item_type: r.get(2)?,
                title: get(title_f).or(r.get(9)?),
                creators,
                date: get(date_f).or(r.get(10)?),
                year: r.get(3)?,
                container_title: r.get(4)?,
                container_field: container_f.map(String::from),
                doi: get("DOI").or(r.get(11)?),
                isbn: get("ISBN").or(r.get(12)?),
                language: get("language").or(r.get(13)?),
                abstract_text: get("abstractNote").or(r.get(14)?),
                extra: get("extra").or(r.get(15)?),
                other_fields,
                tags,
                collections,
                attachments,
                notes,
                source: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                deleted_at: r.get(8)?,
            })
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这条文献，可能已被删除".to_string())
}

/// 上级（附件、笔记的所属条目或笔记）
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Parent {
    /// item / note
    pub kind: String,
    pub id: i64,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDetail {
    pub id: i64,
    pub uid: String,
    pub kind: String,
    pub title: Option<String>,
    pub original_name: Option<String>,
    /// 链接文件的原始位置
    pub original_path: Option<String>,
    pub url: Option<String>,
    pub content_type: Option<String>,
    pub status: String,
    pub size: Option<i64>,
    pub sha256: Option<String>,
    pub parent: Option<Parent>,
    pub tags: Vec<String>,
    pub collections: Vec<NamedRef>,
    /// 旧版 Zotero 附件自带的笔记（纯文本）
    pub note: Option<String>,
    pub annotation_count: i64,
    /// 能在 App 的阅读器里打开（PDF 且文件在）
    pub readable: bool,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

pub fn attachment_get(c: &Connection, id: i64) -> Result<AttachmentDetail, String> {
    let tags = strings(
        c,
        "SELECT t.name FROM attachment_tags x JOIN tags t ON t.id = x.tag_id WHERE x.attachment_id = ?1 ORDER BY t.name",
        id,
    )?;
    let collections = collection_refs(c, "collection_attachments", "attachment_id", id)?;
    c.query_row(
        &format!(
            "SELECT a.id, a.uid, a.kind, a.title, a.original_name, a.original_path, a.url, a.content_type, a.status, f.size, a.sha256,
                    a.item_id, i.title, a.note_id, n.title, n.html, a.note,
                    (SELECT COUNT(*) FROM annotations x WHERE x.attachment_id = a.id AND x.deleted_at IS NULL), a.created_at, a.deleted_at, {}
               FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 LEFT JOIN items i ON i.id = a.item_id LEFT JOIN notes n ON n.id = a.note_id
              WHERE a.id = ?1",
            crate::reader::READABLE
        ),
        params![id],
        |r| {
            let parent = match (r.get::<_, Option<i64>>(11)?, r.get::<_, Option<i64>>(13)?) {
                (Some(i), _) => Some(Parent {
                    kind: "item".into(),
                    id: i,
                    title: r.get(12)?,
                }),
                (None, Some(n)) => Some(Parent {
                    kind: "note".into(),
                    id: n,
                    title: note_title(r.get(14)?, &r.get::<_, Option<String>>(15)?.unwrap_or_default()),
                }),
                _ => None,
            };
            Ok(AttachmentDetail {
                id: r.get(0)?,
                uid: r.get(1)?,
                kind: r.get(2)?,
                title: r.get(3)?,
                original_name: r.get(4)?,
                original_path: r.get(5)?,
                url: r.get(6)?,
                content_type: r.get(7)?,
                status: r.get(8)?,
                size: r.get(9)?,
                sha256: r.get(10)?,
                parent,
                tags,
                collections,
                note: r.get::<_, Option<String>>(16)?.map(|h| html_to_text(&h)).filter(|t| !t.is_empty()),
                annotation_count: r.get(17)?,
                readable: r.get(20)?,
                created_at: r.get(18)?,
                deleted_at: r.get(19)?,
            })
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这个附件，可能已被删除".to_string())
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: i64,
    pub uid: String,
    pub title: Option<String>,
    /// 纯文本（v0.1 只读显示；HTML 原样保存在库里）
    pub text: String,
    pub parent: Option<Parent>,
    pub tags: Vec<String>,
    pub collections: Vec<NamedRef>,
    pub image_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

pub fn note_get(c: &Connection, id: i64) -> Result<NoteDetail, String> {
    let tags = strings(
        c,
        "SELECT t.name FROM note_tags x JOIN tags t ON t.id = x.tag_id WHERE x.note_id = ?1 ORDER BY t.name",
        id,
    )?;
    let collections = collection_refs(c, "collection_notes", "note_id", id)?;
    c.query_row(
        "SELECT n.id, n.uid, n.title, n.html, n.item_id, i.title, (SELECT COUNT(*) FROM attachments a WHERE a.note_id = n.id),
                n.created_at, n.updated_at, n.deleted_at
           FROM notes n LEFT JOIN items i ON i.id = n.item_id WHERE n.id = ?1",
        params![id],
        |r| {
            let html: String = r.get(3)?;
            Ok(NoteDetail {
                id: r.get(0)?,
                uid: r.get(1)?,
                title: note_title(r.get(2)?, &html),
                text: html_to_text(&html),
                parent: r
                    .get::<_, Option<i64>>(4)?
                    .map(|i| {
                        Ok::<_, rusqlite::Error>(Parent {
                            kind: "item".into(),
                            id: i,
                            title: r.get(5)?,
                        })
                    })
                    .transpose()?,
                tags,
                collections,
                image_count: r.get(6)?,
                created_at: r.get(7)?,
                updated_at: r.get(8)?,
                deleted_at: r.get(9)?,
            })
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这条笔记，可能已被删除".to_string())
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetail {
    pub id: i64,
    pub name: String,
    pub parent_name: Option<String>,
    /// 下级集合个数（含随它进回收站的）
    pub children: i64,
    /// 直接成员（文献 + 独立附件 / 笔记）
    pub members: i64,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

pub fn collection_get(c: &Connection, id: i64) -> Result<CollectionDetail, String> {
    c.query_row(
        "SELECT c.id, c.name, p.name, (SELECT COUNT(*) FROM collections k WHERE k.parent_id = c.id),
                (SELECT COUNT(*) FROM collection_items WHERE collection_id = c.id) + (SELECT COUNT(*) FROM collection_attachments WHERE collection_id = c.id)
                  + (SELECT COUNT(*) FROM collection_notes WHERE collection_id = c.id),
                c.created_at, c.deleted_at
           FROM collections c LEFT JOIN collections p ON p.id = c.parent_id WHERE c.id = ?1",
        params![id],
        |r| {
            Ok(CollectionDetail {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_name: r.get(2)?,
                children: r.get(3)?,
                members: r.get(4)?,
                created_at: r.get(5)?,
                deleted_at: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这个集合".to_string())
}

/// 打开附件要用的位置：文件库里的文件，或网址。
#[derive(Debug, PartialEq)]
pub enum Openable {
    File(std::path::PathBuf),
    Url(String),
}

pub fn attachment_target(c: &Connection, attachments_dir: &std::path::Path, id: i64) -> Result<Openable, String> {
    let (status, sha, ext, url): (String, Option<String>, Option<String>, Option<String>) = c
        .query_row(
            "SELECT a.status, a.sha256, f.ext, a.url FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 WHERE a.id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(err)?
        .ok_or("找不到这个附件")?;
    if let (Some(sha), Some(ext)) = (sha, ext) {
        let p = attachments_dir.join(crate::store::rel_path(&sha, &ext));
        return if p.is_file() {
            Ok(Openable::File(p))
        } else {
            Err("文件库里找不到这个文件（可能被手动删除了）".into())
        };
    }
    match url.filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
        Some(u) => Ok(Openable::Url(u)),
        None if status == "missing" => Err("这个附件的文件缺失，导入时就没有找到".into()),
        None => Err("这个附件没有可以打开的文件或网址".into()),
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionNode {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    /// 直接成员（未删除的文献 + 独立附件 / 笔记）
    pub item_count: i64,
}

/// 平铺返回不在回收站的集合（上级在回收站的也不返回），界面按 parent_id 组树。按名字排序。
pub fn collections_list(c: &Connection) -> Result<Vec<CollectionNode>, String> {
    let mut st = c
        .prepare(&format!(
            "{LIVE_COLLECTIONS}
             SELECT c.id, c.parent_id, c.name,
                    (SELECT COUNT(*) FROM collection_items x JOIN items i ON i.id = x.item_id AND i.deleted_at IS NULL WHERE x.collection_id = c.id)
                  + (SELECT COUNT(*) FROM collection_attachments x JOIN attachments a ON a.id = x.attachment_id AND a.deleted_at IS NULL WHERE x.collection_id = c.id)
                  + (SELECT COUNT(*) FROM collection_notes x JOIN notes n ON n.id = x.note_id AND n.deleted_at IS NULL WHERE x.collection_id = c.id)
               FROM collections c WHERE c.id IN live ORDER BY c.name COLLATE NOCASE, c.id"
        ))
        .map_err(err)?;
    st.query_map([], |r| {
        Ok(CollectionNode {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            name: r.get(2)?,
            item_count: r.get(3)?,
        })
    })
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{Tx, new_uid};
    use crate::db::tests::memory_db;

    fn names(v: &[NamedRef]) -> Vec<&str> {
        v.iter().map(|n| n.name.as_str()).collect()
    }

    /// 合成数据：两条文献（一条中文单栏作者）、一条在回收站、一个集合树。
    fn seed(c: &Connection) {
        let tx = Tx::begin(c).unwrap();
        let t = "2026-11-08T00:00:00.000Z";
        let item = |title: &str, created: &str, deleted: Option<&str>| {
            tx.execute(
                "INSERT INTO items (uid, item_type, title, year, container_title, source, created_at, updated_at, deleted_at) VALUES (?1, 'journalArticle', ?2, 2020, '某期刊', 'manual', ?3, ?3, ?4)",
                params![new_uid().unwrap(), title, created, deleted],
            )
            .unwrap();
            tx.last_insert_rowid()
        };
        let a = item("政治效能感研究", "2026-11-01T00:00:00.000Z", None);
        let b = item("Political Efficacy", "2026-11-02T00:00:00.000Z", None);
        item("已删除的文献", "2026-11-03T00:00:00.000Z", Some(t));
        tx.execute(
            "INSERT INTO creators (item_id, role, literal, position) VALUES (?1, 'author', '张三', 0)",
            params![a],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO creators (item_id, role, family, given, position) VALUES (?1, 'author', 'Campbell', 'Angus', 0)",
            params![b],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO creators (item_id, role, family, given, position) VALUES (?1, 'author', 'Gurin', 'Gerald', 1)",
            params![b],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO collections (uid, name, created_at, updated_at) VALUES (?1, '毕业论文', ?2, ?2)",
            params![new_uid().unwrap(), t],
        )
        .unwrap();
        let root = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO collections (uid, parent_id, name, created_at, updated_at) VALUES (?1, ?2, '文献综述', ?3, ?3)",
            params![new_uid().unwrap(), root, t],
        )
        .unwrap();
        tx.execute("INSERT INTO collection_items (collection_id, item_id) VALUES (?1, ?2)", params![root, b])
            .unwrap();
        tx.execute("INSERT INTO tags (name) VALUES ('政治心理')", []).unwrap();
        tx.execute("INSERT INTO item_tags (item_id, tag_id) VALUES (?1, 1)", params![b]).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn empty_library() {
        let c = memory_db();
        assert_eq!(
            overview(&c).unwrap(),
            Overview {
                items: 0,
                trash: 0,
                collections: 0,
                tags: 0,
                attachments: 0,
                notes: 0,
                missing_files: 0,
                unfiled: 0,
                duplicates: 0,
                review: 0
            }
        );
        assert!(items_list(&c, &ListQuery::of(Scope::All)).unwrap().is_empty());
        assert!(collections_list(&c).unwrap().is_empty());
    }

    #[test]
    fn lists_items_collections_and_trash() {
        let c = memory_db();
        seed(&c);
        let o = overview(&c).unwrap();
        assert_eq!((o.items, o.trash, o.collections, o.tags), (2, 1, 2, 1));
        let all = items_list(&c, &ListQuery::of(Scope::All)).unwrap();
        assert_eq!(
            all.iter().map(|i| i.title.as_deref().unwrap()).collect::<Vec<_>>(),
            ["Political Efficacy", "政治效能感研究"],
            "新加入的在前"
        );
        assert_eq!(all[0].first_creator.as_deref(), Some("Campbell Angus"));
        assert_eq!(all[0].creator_count, 2);
        assert_eq!(all[1].first_creator.as_deref(), Some("张三"));
        assert_eq!(all[0].uid.len(), 26);
        let trash = items_list(&c, &ListQuery::of(Scope::Trash)).unwrap();
        assert_eq!(trash.len(), 1);
        let cols = collections_list(&c).unwrap();
        let root = cols.iter().find(|n| n.name == "毕业论文").unwrap();
        assert_eq!(root.item_count, 1);
        assert_eq!(cols.iter().find(|n| n.name == "文献综述").unwrap().parent_id, Some(root.id));
        assert_eq!(items_list(&c, &ListQuery::of(Scope::Collection { id: root.id, include_children: false })).unwrap().len(), 1);
    }

    #[test]
    fn item_detail() {
        let c = memory_db();
        seed(&c);
        let id = items_list(&c, &ListQuery::of(Scope::All)).unwrap()[0].id;
        let d = item_get(&c, id).unwrap();
        assert_eq!(d.title.as_deref(), Some("Political Efficacy"));
        let creators: Vec<_> = d.creators.iter().map(|c| (c.name.as_str(), c.family.as_deref())).collect();
        assert_eq!(creators, [("Campbell Angus", Some("Campbell")), ("Gurin Gerald", Some("Gurin"))]);
        assert_eq!(d.tags, ["政治心理"]);
        assert_eq!(names(&d.collections), ["毕业论文"]);
        assert!(item_get(&c, 999).is_err());
        let json = serde_json::to_value(&d).unwrap();
        assert!(json.get("abstract").is_some() && json.get("itemType").is_some(), "字段名与前端约定一致");
    }

    /// 导入合成 Zotero 夹具后，列表、回收站、详情、集合树的表现与 Zotero 一致。
    #[test]
    fn views_after_zotero_import() {
        use crate::db::{self, tests::temp_dir};
        use crate::import::zotero;
        use crate::paths::DataPaths;
        let dir = temp_dir("lib-views");
        let paths = DataPaths::new(dir.join("data"));
        paths.ensure().unwrap();
        let mut c = db::open(&paths.library_db).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
        let fx = zotero_import::fixture::build(&dir.join("fx")).unwrap();
        zotero::run(
            &c,
            &paths,
            &fx.zotero_dir,
            &zotero::Options {
                base_dir: Some(fx.base_dir.clone()),
            },
        )
        .unwrap();
        let id_of = |table: &str, key: &str| -> i64 {
            c.query_row(&format!("SELECT id FROM {table} WHERE source_key = ?1"), [key], |r| r.get(0))
                .unwrap()
        };

        let o = overview(&c).unwrap();
        assert_eq!(o.items, 20, "21 条减去回收站里的 1 条");
        assert_eq!(o.trash, 4, "JRNL0005、ATT00013、NOTE0004、COLL0006");
        assert_eq!(o.collections, 6, "8 个减去在回收站的 COLL0006 与随它进回收站的 COLL0007");
        assert_eq!(o.missing_files, 2);

        // 全部文献：文献 + 独立附件 ATT00006 + 独立笔记 NOTE0002；子附件、内嵌图片、子笔记不单列
        let all = items_list(&c, &ListQuery::of(Scope::All)).unwrap();
        let kinds = |rows: &[ListRow], k: &str| rows.iter().filter(|r| r.kind == k).count();
        assert_eq!((kinds(&all, "item"), kinds(&all, "attachment"), kinds(&all, "note")), (20, 1, 1));
        let j3 = all.iter().find(|r| r.id == id_of("items", "JRNL0003") && r.kind == "item").unwrap();
        assert_eq!((j3.attachments, j3.missing_files), (1, 1), "JRNL0003 的 PDF 缺失");
        let j1 = all.iter().find(|r| r.id == id_of("items", "JRNL0001") && r.kind == "item").unwrap();
        assert_eq!((j1.attachments, j1.notes), (1, 1), "ATT00013 直接删除，不算在内");

        // 回收站：直接删除的条目、附件、笔记、集合；随父条目进回收站的不单列
        let trash = items_list(&c, &ListQuery::of(Scope::Trash)).unwrap();
        let mut got: Vec<(String, String)> = trash.iter().map(|r| (r.kind.clone(), r.title.clone().unwrap_or_default())).collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("attachment".into(), "Full Text PDF".into()),
                ("collection".into(), "已删除的集合".into()),
                ("item".into(), "A Trashed Article".into()),
                ("note".into(), "直接删除的笔记".into())
            ]
        );
        let trashed = item_get(&c, id_of("items", "JRNL0005")).unwrap();
        assert_eq!((trashed.attachments.len(), trashed.notes.len()), (1, 1), "回收站里的条目带着它的附件和笔记");

        // 集合：方法里有 JRNL0003、JRNL0008 与独立附件 ATT00006
        let cols = collections_list(&c).unwrap();
        assert!(cols.iter().all(|n| n.name != "子集合（随上级删除）"));
        let method = cols.iter().find(|n| n.name == "方法").unwrap();
        assert_eq!(method.item_count, 3);
        let rows = items_list(&c, &ListQuery::of(Scope::Collection { id: method.id, include_children: false })).unwrap();
        assert_eq!(kinds(&rows, "attachment"), 1);
        let deleted_coll = collection_get(&c, id_of("collections", "COLL0006")).unwrap();
        assert_eq!((deleted_coll.children, deleted_coll.members), (1, 0));

        // 详情
        let d = item_get(&c, id_of("items", "JRNL0001")).unwrap();
        assert_eq!(d.attachments[0].status, "ok");
        assert!(d.attachments[0].size.unwrap() > 0);
        assert_eq!(d.notes[0].title.as_deref(), Some("读书笔记"));
        assert!(d.notes[0].preview.starts_with("内部效能感"), "预览不重复标题：{}", d.notes[0].preview);
        assert_eq!(names(&d.collections), ["文献综述", "理论"]);
        assert_eq!(d.container_field.as_deref(), Some("publicationTitle"));
        assert_eq!(d.date.as_deref(), Some("March 2019"), "日期显示原文");
        assert!(d.other_fields.iter().any(|f| f.name == "volume" && f.value == "12"));
        assert!(d.other_fields.iter().all(|f| f.name != "title" && f.name != "date" && f.name != "DOI"));
        let missing = attachment_get(&c, id_of("attachments", "ATT00008")).unwrap();
        assert_eq!(
            (missing.status.as_str(), missing.parent.as_ref().map(|p| p.kind.as_str())),
            ("missing", Some("item"))
        );
        assert!(attachment_target(&c, &paths.attachments, missing.id).unwrap_err().contains("缺失"));
        let img = attachment_get(&c, id_of("attachments", "ATT00011")).unwrap();
        assert_eq!(img.parent.map(|p| p.kind), Some("note".into()));
        let old = attachment_get(&c, id_of("attachments", "ATT00003")).unwrap();
        assert_eq!(old.note.as_deref(), Some("旧版附件笔记"));
        assert!(old.original_path.unwrap().ends_with("国富论-扫描版.pdf"));
        let pdf = attachment_get(&c, id_of("attachments", "ATT00001")).unwrap();
        assert_eq!((pdf.tags.clone(), pdf.annotation_count), (vec!["pdf".to_string()], 3));
        assert!(pdf.readable && !missing.readable && !img.readable, "只有文件在的 PDF 能在阅读器里打开（图片、缺失的不行）");
        // 从 Zotero 导入的标注：阅读器直接按原格式读出（位置 JSON、页内顺序）
        let anns = crate::reader::annotations_list(&c, pdf.id).unwrap();
        assert_eq!(anns.len(), 3);
        assert!(anns.iter().all(|a| a.position.is_object() && a.position["pageIndex"] == a.page_index));
        assert!(anns.windows(2).all(|w| w[0].sort_key <= w[1].sort_key));
        match attachment_target(&c, &paths.attachments, pdf.id).unwrap() {
            Openable::File(p) => assert!(p.starts_with(&paths.attachments) && p.extension().unwrap() == "pdf"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            attachment_target(&c, &paths.attachments, id_of("attachments", "ATT00005")).unwrap(),
            Openable::Url("https://example.org/policy".into())
        );
        // ---- 范围、筛选、排序 ----
        let q = |scope: Scope| ListQuery::of(scope);
        let run = |query: ListQuery| items_list(&c, &query).unwrap();
        assert_eq!(o.unfiled, 10, "20 条在用的文献里 10 条在集合中");
        assert_eq!(run(q(Scope::Unfiled)).iter().filter(|r| r.kind == "item").count(), 10);
        let missing = run(q(Scope::Missing));
        assert_eq!(missing.len(), 2, "JRNL0003 与 BOOK0002 有缺失的附件");
        assert!(missing.iter().all(|r| r.missing_files > 0));
        let mut by_tag = q(Scope::All);
        by_tag.tags = vec!["政治效能感".into()];
        assert_eq!(run(by_tag.clone()).len(), 3);
        by_tag.tags.push("machine learning".into());
        assert_eq!(run(by_tag).len(), 1, "多个标签是「同时带有」");
        let mut books = q(Scope::All);
        books.item_type = Some("book".into());
        assert_eq!(run(books).len(), 3);
        let mut notes_only = q(Scope::All);
        notes_only.item_type = Some("note".into());
        assert_eq!(run(notes_only).len(), 1);
        let mut by_year = q(Scope::All);
        by_year.sort = Some("year".into());
        let years: Vec<Option<i64>> = run(by_year.clone()).into_iter().filter(|r| r.kind == "item").map(|r| r.year).collect();
        assert_eq!(years.first(), Some(&Some(1776)));
        assert_eq!(years.last(), Some(&None), "没有年份的排在最后");
        by_year.desc = true;
        let years: Vec<Option<i64>> = run(by_year).into_iter().filter(|r| r.kind == "item").map(|r| r.year).collect();
        assert_eq!((years.first(), years.last()), (Some(&Some(2025)), Some(&None)), "降序时空值也在最后");
        let mut by_title = q(Scope::All);
        by_title.sort = Some("title".into());
        let titles: Vec<String> = run(by_title).into_iter().filter_map(|r| r.title).collect();
        assert_eq!(titles[0], "A Large Collaboration Paper");
        // 可能重复：造一条同 DOI 的
        c.execute(
            "INSERT INTO items (uid, item_type, title, doi, source, created_at, updated_at) VALUES ('DUP1', 'journalArticle', 'Dup', '10.0000/SYNTHETIC.2016.010', 'manual', 't', 't')",
            [],
        )
        .unwrap();
        let dups = run(q(Scope::Duplicates));
        assert_eq!(dups.len(), 2);
        assert!(dups.iter().all(|r| r.group == Some(0) && r.group_reason.as_deref() == Some("DOI 相同")));
        assert_eq!(overview(&c).unwrap().duplicates, 1);
        let tags = tags_list(&c).unwrap();
        assert!(tags.iter().any(|t| t.name == "政治效能感" && t.count == 3));
        assert!(tags.iter().all(|t| t.name != "重要"), "只挂在标注上的标签不列");

        // 搜索：标签、标题、子笔记内容都能命中；中文二元组；多词是「同时包含」
        crate::search::attach_memory(&c).unwrap();
        assert!(crate::search::rebuild(&c).unwrap() > 0);
        let search = |text: &str| {
            let mut query = q(Scope::All);
            query.q = text.into();
            run(query)
        };
        let hits = search("政治效能感");
        assert_eq!(hits.len(), 3, "JRNL0001（标签）、JRNL0002、THES0001");
        assert!(search("效能").len() >= 3);
        assert!(search("内部效能感").iter().any(|r| r.id == id_of("items", "JRNL0001")), "子笔记里的词命中所属文献");
        let effic = search("effic");
        assert!(effic.iter().any(|r| r.id == id_of("items", "JRNL0001")), "西文前缀");
        assert!(effic[0].title.as_deref().is_some_and(|t| t.contains("Efficacy")), "标题命中排在前面");
        assert!(search("efficacy 政治").len() <= search("efficacy").len());
        assert!(search("\"(*NEAR").is_empty());
        let mut in_trash = q(Scope::Trash);
        in_trash.q = "Trashed".into();
        assert_eq!(run(in_trash).len(), 1, "回收站里也能搜");

        let note = note_get(&c, id_of("notes", "NOTE0002")).unwrap();
        assert_eq!((note.image_count, names(&note.collections)), (1, vec!["读书：国富论"]));
        assert_eq!(note.text, "独立笔记，含图片");
    }
}
