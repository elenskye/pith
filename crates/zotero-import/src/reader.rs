//! 只读读取 Zotero 数据目录（zotero.sqlite + storage/）。
//!
//! 安全约定：数据库一律以 `mode=ro&immutable=1` 打开，并设 `query_only`；
//! 不创建 -journal / -wal / -shm，不加锁，不改任何字节。

use crate::model::*;
use crate::util::{hash_file, value_to_string};
use crate::{Error, Result};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, Row};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    /// Zotero 首选项里的「链接附件根目录」，用于解析 "attachments:" 路径。
    pub base_attachment_dir: Option<PathBuf>,
    /// 关闭后不读取文件内容（只检查是否存在），用于快速预览。
    pub skip_hashing: bool,
}

/// Zotero 存放附件缓存的点文件，不算作附件内容。
pub const ZOTERO_CACHE_FILES: &[&str] = &[".zotero-ft-cache", ".zotero-ft-info", ".zotero-reader-state", ".DS_Store"];

pub fn db_path(zotero_dir: &Path) -> PathBuf {
    zotero_dir.join("zotero.sqlite")
}

pub fn storage_path(zotero_dir: &Path) -> PathBuf {
    zotero_dir.join("storage")
}

/// 以只读、不可变方式打开数据库。
pub fn open_readonly(db: &Path) -> Result<Connection> {
    if !db.is_file() {
        return Err(Error::Schema(format!("找不到数据库文件：{}", db.display())));
    }
    let uri = format!("file:{}?mode=ro&immutable=1", uri_escape(&db.to_string_lossy()));
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

fn uri_escape(p: &str) -> String {
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        match c {
            '%' => s.push_str("%25"),
            '?' => s.push_str("%3f"),
            '#' => s.push_str("%23"),
            ' ' => s.push_str("%20"),
            _ => s.push(c),
        }
    }
    s
}

pub fn has_table(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
        [name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn has_column(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, col],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 有该列就选该列，否则选 NULL；兼容不同 Zotero 版本。
fn col(conn: &Connection, table: &str, alias: &str, name: &str) -> Result<String> {
    Ok(if has_column(conn, table, name)? { format!("{alias}.{name}") } else { "NULL".to_string() })
}

/// 优先使用含自定义类型 / 字段的 *Combined 表。
pub fn pick_table<'a>(conn: &Connection, combined: &'a str, plain: &'a str) -> Result<&'a str> {
    Ok(if has_table(conn, combined)? { combined } else { plain })
}

pub const REQUIRED_TABLES: &[&str] = &[
    "items", "itemData", "itemDataValues", "creators", "itemCreators", "creatorTypes", "collections",
    "collectionItems", "tags", "itemTags", "itemAttachments", "itemNotes", "deletedItems", "libraries", "version",
];

pub fn check_schema(conn: &Connection) -> Result<()> {
    let missing: Vec<&str> =
        REQUIRED_TABLES.iter().copied().filter(|t| !has_table(conn, t).unwrap_or(false)).collect();
    if !missing.is_empty() {
        return Err(Error::Schema(format!("不像是 Zotero 数据库，缺少表：{}", missing.join(", "))));
    }
    Ok(())
}

/// 只读取 user / group 库，跳过 feed（RSS 订阅）。
pub const LIBRARY_FILTER: &str = "SELECT libraryID FROM libraries WHERE type IN ('user','group')";

fn opt_text(row: &Row, i: usize) -> rusqlite::Result<Option<String>> {
    Ok(value_to_string(row.get::<_, Value>(i)?))
}

fn text(row: &Row, i: usize) -> rusqlite::Result<String> {
    Ok(opt_text(row, i)?.unwrap_or_default())
}

fn opt_int(row: &Row, i: usize) -> rusqlite::Result<Option<i64>> {
    Ok(match row.get::<_, Value>(i)? {
        Value::Integer(v) => Some(v),
        Value::Real(v) => Some(v as i64),
        Value::Text(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn int(row: &Row, i: usize) -> rusqlite::Result<i64> {
    Ok(opt_int(row, i)?.unwrap_or(0))
}

pub fn read_library(zotero_dir: &Path, opts: &ReadOptions) -> Result<ZoteroLibrary> {
    let db = db_path(zotero_dir);
    let conn = open_readonly(&db)?;
    read_from_connection(&conn, &db, &storage_path(zotero_dir), opts)
}

pub fn read_from_connection(conn: &Connection, db: &Path, storage: &Path, opts: &ReadOptions) -> Result<ZoteroLibrary> {
    check_schema(conn)?;
    let mut warnings = Vec::new();

    // ---- 版本与库 ----
    let mut schema_versions = BTreeMap::new();
    {
        let mut st = conn.prepare("SELECT schema, version FROM version")?;
        let rows = st.query_map([], |r| Ok((text(r, 0)?, int(r, 1)?)))?;
        for r in rows {
            let (k, v) = r?;
            schema_versions.insert(k, v);
        }
    }
    let client_version: Option<String> = if has_table(conn, "settings")? {
        conn.query_row("SELECT value FROM settings WHERE setting='client' AND key='lastVersion'", [], |r| opt_text(r, 0))
            .ok()
            .flatten()
    } else {
        None
    };

    let group_names: HashMap<i64, String> = if has_table(conn, "groups")? {
        let mut st = conn.prepare("SELECT libraryID, name FROM groups")?;
        st.query_map([], |r| Ok((int(r, 0)?, text(r, 1)?)))?.collect::<rusqlite::Result<_>>()?
    } else {
        HashMap::new()
    };
    let libraries: Vec<LibraryInfo> = {
        let mut st = conn.prepare(&format!(
            "SELECT libraryID, type, editable FROM libraries WHERE libraryID IN ({LIBRARY_FILTER}) ORDER BY libraryID"
        ))?;
        st.query_map([], |r| {
            let id = int(r, 0)?;
            Ok(LibraryInfo { library_id: id, kind: text(r, 1)?, group_name: None, editable: int(r, 2)? != 0 })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|mut l| {
            l.group_name = group_names.get(&l.library_id).cloned();
            l
        })
        .collect()
    };

    // ---- 查找表 ----
    let type_table = pick_table(conn, "itemTypesCombined", "itemTypes")?;
    let field_table = pick_table(conn, "fieldsCombined", "fields")?;
    let item_types = id_name(conn, &format!("SELECT itemTypeID, typeName FROM {type_table}"))?;
    let charsets = if has_table(conn, "charsets")? { id_name(conn, "SELECT charsetID, charset FROM charsets")? } else { HashMap::new() };

    // ---- 条目 ----
    let deleted: HashMap<i64, Option<String>> = {
        let mut st = conn.prepare("SELECT itemID, dateDeleted FROM deletedItems")?;
        st.query_map([], |r| Ok((int(r, 0)?, opt_text(r, 1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut bases: BTreeMap<i64, ItemBase> = BTreeMap::new();
    {
        let mut st = conn.prepare(&format!(
            "SELECT itemID, itemTypeID, dateAdded, dateModified, clientDateModified, libraryID, key, version, synced
             FROM items WHERE libraryID IN ({LIBRARY_FILTER})"
        ))?;
        let rows = st.query_map([], |r| {
            Ok((int(r, 0)?, int(r, 1)?, text(r, 2)?, text(r, 3)?, text(r, 4)?, int(r, 5)?, text(r, 6)?, int(r, 7)?, int(r, 8)?))
        })?;
        for r in rows {
            let (id, type_id, added, modified, client_mod, lib, key, version, synced) = r?;
            let item_type = match item_types.get(&type_id) {
                Some(t) => t.clone(),
                None => {
                    warnings.push(format!("条目 {key} 的类型 ID {type_id} 不在类型表中"));
                    format!("itemType#{type_id}")
                }
            };
            bases.insert(
                id,
                ItemBase {
                    item_id: id,
                    key,
                    library_id: lib,
                    item_type,
                    date_added: added,
                    date_modified: modified,
                    client_date_modified: client_mod,
                    version,
                    synced: synced != 0,
                    deleted_at: deleted.get(&id).cloned().flatten(),
                    in_trash: false,
                    fields: BTreeMap::new(),
                    creators: vec![],
                    tags: vec![],
                    collection_keys: vec![],
                    relations: vec![],
                },
            );
        }
    }
    // 字段
    {
        let mut st = conn.prepare(&format!(
            "SELECT d.itemID, d.fieldID, f.fieldName, v.value FROM itemData d
             LEFT JOIN {field_table} f ON f.fieldID = d.fieldID
             LEFT JOIN itemDataValues v ON v.valueID = d.valueID"
        ))?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, int(r, 1)?, opt_text(r, 2)?, r.get::<_, Value>(3)?)))?;
        let mut non_text = 0usize;
        for r in rows {
            let (id, fid, fname, value) = r?;
            let Some(b) = bases.get_mut(&id) else { continue };
            if !matches!(value, Value::Text(_)) {
                non_text += 1;
            }
            let name = fname.unwrap_or_else(|| format!("field#{fid}"));
            b.fields.insert(name, value_to_string(value).unwrap_or_default());
        }
        if non_text > 0 {
            warnings.push(format!("{non_text} 个字段值不是文本，已按原值转为字符串"));
        }
    }

    // 作者
    {
        let mut st = conn.prepare(
            "SELECT ic.itemID, ct.creatorType, ic.creatorTypeID, c.firstName, c.lastName, c.fieldMode, ic.orderIndex
             FROM itemCreators ic
             LEFT JOIN creators c ON c.creatorID = ic.creatorID
             LEFT JOIN creatorTypes ct ON ct.creatorTypeID = ic.creatorTypeID
             ORDER BY ic.itemID, ic.orderIndex",
        )?;
        let rows = st.query_map([], |r| {
            Ok((int(r, 0)?, opt_text(r, 1)?, int(r, 2)?, opt_text(r, 3)?, opt_text(r, 4)?, int(r, 5)?, int(r, 6)?))
        })?;
        for r in rows {
            let (id, ctype, ctype_id, first, last, mode, order) = r?;
            let Some(b) = bases.get_mut(&id) else { continue };
            b.creators.push(Creator {
                creator_type: ctype.unwrap_or_else(|| format!("creatorType#{ctype_id}")),
                first_name: first,
                last_name: last,
                field_mode: mode,
                order_index: order,
            });
        }
    }

    // 标签
    {
        let mut st = conn.prepare(
            "SELECT it.itemID, t.name, it.type FROM itemTags it LEFT JOIN tags t ON t.tagID = it.tagID",
        )?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, text(r, 1)?, int(r, 2)?)))?;
        for r in rows {
            let (id, name, ty) = r?;
            if let Some(b) = bases.get_mut(&id) {
                b.tags.push(Tag { name, tag_type: ty });
            }
        }
        for b in bases.values_mut() {
            b.tags.sort_by(|a, b| (&a.name, a.tag_type).cmp(&(&b.name, b.tag_type)));
        }
    }

    // 集合
    let mut coll_rows: Vec<CollRow> = Vec::new();
    {
        let mut st = conn.prepare(&format!(
            "SELECT collectionID, collectionName, parentCollectionID, clientDateModified, libraryID, key, version, synced
             FROM collections WHERE libraryID IN ({LIBRARY_FILTER}) ORDER BY collectionID"
        ))?;
        let rows = st.query_map([], |r| {
            Ok((int(r, 0)?, text(r, 1)?, opt_int(r, 2)?, text(r, 3)?, int(r, 4)?, text(r, 5)?, int(r, 6)?, int(r, 7)?))
        })?;
        for r in rows {
            coll_rows.push(r?);
        }
    }
    let coll_key: HashMap<i64, String> = coll_rows.iter().map(|c| (c.0, c.5.clone())).collect();
    let coll_parent: HashMap<i64, Option<i64>> = coll_rows.iter().map(|c| (c.0, c.2)).collect();
    let deleted_colls: HashMap<i64, Option<String>> = if has_table(conn, "deletedCollections")? {
        let mut st = conn.prepare("SELECT collectionID, dateDeleted FROM deletedCollections")?;
        st.query_map([], |r| Ok((int(r, 0)?, opt_text(r, 1)?)))?.collect::<rusqlite::Result<_>>()?
    } else {
        HashMap::new()
    };
    let mut coll_relations: HashMap<i64, Vec<Relation>> = HashMap::new();
    if has_table(conn, "collectionRelations")? {
        let mut st = conn.prepare(
            "SELECT r.collectionID, p.predicate, r.predicateID, r.object FROM collectionRelations r
             LEFT JOIN relationPredicates p ON p.predicateID = r.predicateID",
        )?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, opt_text(r, 1)?, int(r, 2)?, text(r, 3)?)))?;
        for r in rows {
            let (id, p, pid, obj) = r?;
            coll_relations
                .entry(id)
                .or_default()
                .push(Relation { predicate: p.unwrap_or_else(|| format!("predicate#{pid}")), object: obj });
        }
    }
    let collections: Vec<Collection> = coll_rows
        .iter()
        .map(|(id, name, parent, cdm, lib, key, version, synced)| {
            // 本集合或任一祖先在回收站即视为在回收站（带环保护）
            let mut in_trash = false;
            let mut cur = Some(*id);
            let mut seen = HashSet::new();
            while let Some(c) = cur {
                if !seen.insert(c) {
                    break;
                }
                if deleted_colls.contains_key(&c) {
                    in_trash = true;
                    break;
                }
                cur = coll_parent.get(&c).copied().flatten();
            }
            let mut relations = coll_relations.remove(id).unwrap_or_default();
            relations.sort_by(|a, b| (&a.predicate, &a.object).cmp(&(&b.predicate, &b.object)));
            Collection {
                collection_id: *id,
                key: key.clone(),
                library_id: *lib,
                name: name.clone(),
                parent_key: parent.and_then(|p| coll_key.get(&p).cloned()),
                client_date_modified: cdm.clone(),
                version: *version,
                synced: *synced != 0,
                deleted_at: deleted_colls.get(id).cloned().flatten(),
                in_trash,
                relations,
            }
        })
        .collect();
    for c in &coll_rows {
        if let Some(p) = c.2
            && !coll_key.contains_key(&p) {
                warnings.push(format!("集合 {} 的上级集合 ID {p} 不存在", c.5));
            }
    }

    // 集合成员
    {
        let mut st = conn.prepare("SELECT itemID, collectionID FROM collectionItems ORDER BY itemID, collectionID")?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, int(r, 1)?)))?;
        for r in rows {
            let (id, cid) = r?;
            let Some(b) = bases.get_mut(&id) else { continue };
            match coll_key.get(&cid) {
                Some(k) => b.collection_keys.push(k.clone()),
                None => b.collection_keys.push(format!("collection#{cid}")),
            }
        }
    }

    // 关联
    if has_table(conn, "itemRelations")? {
        let mut st = conn.prepare(
            "SELECT r.itemID, p.predicate, r.predicateID, r.object FROM itemRelations r
             LEFT JOIN relationPredicates p ON p.predicateID = r.predicateID",
        )?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, opt_text(r, 1)?, int(r, 2)?, text(r, 3)?)))?;
        for r in rows {
            let (id, p, pid, obj) = r?;
            if let Some(b) = bases.get_mut(&id) {
                b.relations.push(Relation { predicate: p.unwrap_or_else(|| format!("predicate#{pid}")), object: obj });
            }
        }
        for b in bases.values_mut() {
            b.relations.sort_by(|a, b| (&a.predicate, &a.object).cmp(&(&b.predicate, &b.object)));
        }
    }

    // ---- 附件、笔记、标注的原始行 ----
    struct AttRow {
        parent: Option<i64>,
        link_mode: Option<i64>,
        content_type: Option<String>,
        charset_id: Option<i64>,
        path: Option<String>,
        sync_state: Option<i64>,
        storage_mod_time: Option<i64>,
        storage_hash: Option<String>,
        lpmt: Option<i64>,
        last_read: Option<i64>,
    }
    let mut att_rows: HashMap<i64, AttRow> = HashMap::new();
    {
        let t = "itemAttachments";
        let sql = format!(
            "SELECT a.itemID, a.parentItemID, a.linkMode, a.contentType, a.charsetID, a.path, {}, {}, {}, {}, {} FROM itemAttachments a",
            col(conn, t, "a", "syncState")?,
            col(conn, t, "a", "storageModTime")?,
            col(conn, t, "a", "storageHash")?,
            col(conn, t, "a", "lastProcessedModificationTime")?,
            col(conn, t, "a", "lastRead")?,
        );
        let mut st = conn.prepare(&sql)?;
        let rows = st.query_map([], |r| {
            Ok((
                int(r, 0)?,
                AttRow {
                    parent: opt_int(r, 1)?,
                    link_mode: opt_int(r, 2)?,
                    content_type: opt_text(r, 3)?,
                    charset_id: opt_int(r, 4)?,
                    path: opt_text(r, 5)?,
                    sync_state: opt_int(r, 6)?,
                    storage_mod_time: opt_int(r, 7)?,
                    storage_hash: opt_text(r, 8)?,
                    lpmt: opt_int(r, 9)?,
                    last_read: opt_int(r, 10)?,
                },
            ))
        })?;
        for r in rows {
            let (id, row) = r?;
            att_rows.insert(id, row);
        }
    }
    let mut note_rows: HashMap<i64, NoteRow> = HashMap::new();
    {
        let mut st = conn.prepare("SELECT itemID, parentItemID, note, title FROM itemNotes")?;
        let rows = st.query_map([], |r| Ok((int(r, 0)?, (opt_int(r, 1)?, opt_text(r, 2)?, opt_text(r, 3)?))))?;
        for r in rows {
            let (id, row) = r?;
            note_rows.insert(id, row);
        }
    }
    #[allow(clippy::type_complexity)]
    let mut ann_rows: HashMap<i64, (Option<i64>, i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64)> =
        HashMap::new();
    if has_table(conn, "itemAnnotations")? {
        let t = "itemAnnotations";
        let sql = format!(
            "SELECT a.itemID, a.parentItemID, a.type, {}, a.text, a.comment, a.color, a.pageLabel, a.sortIndex, a.position, {} FROM itemAnnotations a",
            col(conn, t, "a", "authorName")?,
            col(conn, t, "a", "isExternal")?,
        );
        let mut st = conn.prepare(&sql)?;
        let rows = st.query_map([], |r| {
            Ok((
                int(r, 0)?,
                (
                    opt_int(r, 1)?,
                    int(r, 2)?,
                    opt_text(r, 3)?,
                    opt_text(r, 4)?,
                    opt_text(r, 5)?,
                    opt_text(r, 6)?,
                    opt_text(r, 7)?,
                    opt_text(r, 8)?,
                    opt_text(r, 9)?,
                    int(r, 10)?,
                ),
            ))
        })?;
        for r in rows {
            let (id, row) = r?;
            ann_rows.insert(id, row);
        }
    }

    // ---- 回收站：自己或任一上级在 deletedItems 里 ----
    let mut parent_of: HashMap<i64, i64> = HashMap::new();
    for (id, a) in &att_rows {
        if let Some(p) = a.parent {
            parent_of.insert(*id, p);
        }
    }
    for (id, n) in &note_rows {
        if let Some(p) = n.0 {
            parent_of.insert(*id, p);
        }
    }
    for (id, a) in &ann_rows {
        if let Some(p) = a.0 {
            parent_of.insert(*id, p);
        }
    }
    let ids: Vec<i64> = bases.keys().copied().collect();
    for id in ids {
        let mut cur = Some(id);
        let mut seen = HashSet::new();
        let mut trash = false;
        while let Some(c) = cur {
            if !seen.insert(c) {
                warnings.push(format!("条目 {} 的父子关系成环", bases[&id].key));
                break;
            }
            if deleted.contains_key(&c) {
                trash = true;
                break;
            }
            cur = parent_of.get(&c).copied();
        }
        bases.get_mut(&id).unwrap().in_trash = trash;
    }
    for (id, p) in &parent_of {
        if bases.contains_key(id) && !bases.contains_key(p) {
            warnings.push(format!("条目 {} 的父条目 ID {p} 不存在", bases[id].key));
        }
    }

    // ---- 分类 ----
    let key_of: HashMap<i64, String> = bases.iter().map(|(id, b)| (*id, b.key.clone())).collect();
    let parent_keys: HashMap<i64, Option<String>> =
        bases.keys().map(|id| (*id, parent_of.get(id).and_then(|p| key_of.get(p).cloned()))).collect();
    let mut items = Vec::new();
    let mut attachments = Vec::new();
    let mut notes = Vec::new();
    let mut annotations = Vec::new();
    for (id, base) in bases {
        let pk = parent_keys.get(&id).cloned().flatten();
        match base.item_type.as_str() {
            "attachment" => {
                let row = att_rows.remove(&id);
                if row.is_none() {
                    warnings.push(format!("附件条目 {} 在 itemAttachments 中没有记录", base.key));
                }
                let note = note_rows.remove(&id).and_then(|n| n.1);
                let mut att = Attachment {
                    parent_key: pk,
                    link_mode: row.as_ref().and_then(|r| r.link_mode).map(LinkMode::from_raw),
                    link_mode_raw: row.as_ref().and_then(|r| r.link_mode),
                    content_type: row.as_ref().and_then(|r| r.content_type.clone()),
                    charset: row.as_ref().and_then(|r| r.charset_id).map(|c| {
                        charsets.get(&c).cloned().unwrap_or_else(|| format!("charset#{c}"))
                    }),
                    path: row.as_ref().and_then(|r| r.path.clone()),
                    sync_state: row.as_ref().and_then(|r| r.sync_state),
                    storage_mod_time: row.as_ref().and_then(|r| r.storage_mod_time),
                    storage_hash: row.as_ref().and_then(|r| r.storage_hash.clone()),
                    last_processed_modification_time: row.as_ref().and_then(|r| r.lpmt),
                    last_read: row.as_ref().and_then(|r| r.last_read),
                    note,
                    file: None,
                    extra_files: vec![],
                    base,
                };
                resolve_file(&mut att, storage, opts, &mut warnings)?;
                attachments.push(att);
            }
            "note" => {
                let row = note_rows.remove(&id);
                if row.is_none() {
                    warnings.push(format!("笔记条目 {} 在 itemNotes 中没有记录", base.key));
                }
                let (_, html, title) = row.unwrap_or((None, None, None));
                notes.push(Note { parent_key: pk, title, html, base });
            }
            "annotation" => {
                let Some(r) = ann_rows.remove(&id) else {
                    warnings.push(format!("标注条目 {} 在 itemAnnotations 中没有记录", base.key));
                    annotations.push(Annotation {
                        parent_key: pk,
                        annotation_type: "unknown".into(),
                        annotation_type_raw: 0,
                        author_name: None,
                        text: None,
                        comment: None,
                        color: None,
                        page_label: None,
                        sort_index: None,
                        position: None,
                        is_external: false,
                        base,
                    });
                    continue;
                };
                annotations.push(Annotation {
                    parent_key: pk,
                    annotation_type: annotation_type_name(r.1).into(),
                    annotation_type_raw: r.1,
                    author_name: r.2,
                    text: r.3,
                    comment: r.4,
                    color: r.5,
                    page_label: r.6,
                    sort_index: r.7,
                    position: r.8,
                    is_external: r.9 != 0,
                    base,
                });
            }
            _ => {
                if note_rows.remove(&id).is_some() {
                    warnings.push(format!("普通条目 {} 在 itemNotes 中有记录（旧版数据），笔记内容未迁移", base.key));
                }
                items.push(base);
            }
        }
    }
    for id in att_rows.keys() {
        warnings.push(format!("itemAttachments 中的条目 ID {id} 不是附件类型或不在可读库中"));
    }
    for id in ann_rows.keys() {
        warnings.push(format!("itemAnnotations 中的条目 ID {id} 不是标注类型或不在可读库中"));
    }

    items.sort_by(|a, b| a.key.cmp(&b.key));
    attachments.sort_by(|a, b| a.base.key.cmp(&b.base.key));
    notes.sort_by(|a, b| a.base.key.cmp(&b.base.key));
    annotations.sort_by(|a, b| a.base.key.cmp(&b.base.key));
    warnings.sort();
    warnings.dedup();

    Ok(ZoteroLibrary {
        model_version: MODEL_VERSION,
        source: SourceInfo {
            db_file: db.display().to_string(),
            storage_dir: storage.display().to_string(),
            client_version,
            schema_versions,
        },
        libraries,
        collections,
        items,
        attachments,
        notes,
        annotations,
        warnings,
    })
}

/// (collectionID, 名称, 上级 ID, clientDateModified, libraryID, key, version, synced)
type CollRow = (i64, String, Option<i64>, String, i64, String, i64, i64);
/// (parentItemID, note, title)
type NoteRow = (Option<i64>, Option<String>, Option<String>);

fn id_name(conn: &Connection, sql: &str) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare(sql)?;
    let m = st.query_map([], |r| Ok((int(r, 0)?, text(r, 1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(m)
}

/// 解析附件文件位置并计算哈希。只读文件，从不写入。
fn resolve_file(att: &mut Attachment, storage: &Path, opts: &ReadOptions, warnings: &mut Vec<String>) -> Result<()> {
    let Some(path) = att.path.clone() else { return Ok(()) };
    if att.link_mode == Some(LinkMode::LinkedUrl) {
        return Ok(());
    }
    let key = att.base.key.clone();
    let (kind, resolved, relative) = if let Some(name) = path.strip_prefix("storage:") {
        (FileKind::Stored, Some(storage.join(&key).join(name)), Some(format!("{key}/{name}")))
    } else if let Some(rel) = path.strip_prefix("attachments:") {
        (FileKind::BaseRelative, opts.base_attachment_dir.as_ref().map(|b| b.join(rel)), Some(rel.to_string()))
    } else if Path::new(&path).is_absolute() {
        (FileKind::Linked, Some(PathBuf::from(&path)), None)
    } else {
        warnings.push(format!("附件 {key} 的路径格式无法识别"));
        return Ok(());
    };

    let mut info = FileInfo {
        kind,
        resolved_path: resolved.as_ref().map(|p| p.display().to_string()),
        relative_path: relative,
        exists: false,
        size: None,
        sha256: None,
        md5: None,
        md5_matches_storage_hash: None,
    };
    if let Some(p) = &resolved
        && p.is_file() {
            info.exists = true;
            info.size = std::fs::metadata(p).map(|m| m.len()).ok();
            if !opts.skip_hashing {
                let (sha, md5) = hash_file(p).map_err(|e| Error::Io(e, p.clone()))?;
                if kind == FileKind::Stored {
                    info.md5_matches_storage_hash = att.storage_hash.as_ref().map(|h| h.eq_ignore_ascii_case(&md5));
                }
                info.sha256 = Some(sha);
                info.md5 = Some(md5);
            }
        }

    // 附件目录里的其他文件（快照资源等）
    if kind == FileKind::Stored {
        let dir = storage.join(&key);
        let main = path.strip_prefix("storage:").unwrap_or_default().to_string();
        if dir.is_dir() {
            let mut extra = Vec::new();
            list_files(&dir, &dir, &mut extra).map_err(|e| Error::Io(e, dir.clone()))?;
            extra.retain(|f| f != &main && !ZOTERO_CACHE_FILES.contains(&f.as_str()));
            extra.sort();
            att.extra_files = extra;
        }
    }
    att.file = Some(info);
    Ok(())
}

pub(crate) fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        let ft = e.file_type()?;
        if ft.is_dir() {
            list_files(root, &p, out)?;
        } else {
            out.push(p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
