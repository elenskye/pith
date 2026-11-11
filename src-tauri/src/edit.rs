//! 文献库的写操作（Phase 3）：新建与编辑文献、作者、标签、集合、批量改字段、添加附件、回收站（移入 / 恢复 / 永久删除）、合并重复。
//! 规则：
//! - 字段改动只写 `item_fields`，再由 `fields::refresh_columns` 重算常用列（列永远由字段推出）；
//! - 一个操作一个事务；永久删除前做 `before-delete` 备份，删完清理没有人引用的附件文件；
//! - 返回受影响的文档（`search::DocRef`），命令层据此更新搜索索引。

use crate::common::{Tx, err, new_uid};
use crate::fields;
use crate::search::DocRef;
use crate::store;
use crate::text;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// 界面传来的操作对象：列表里的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Item,
    Attachment,
    Note,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Target {
    pub kind: Kind,
    pub id: i64,
}

impl Target {
    pub fn item(id: i64) -> Self {
        Target { kind: Kind::Item, id }
    }
    fn table(self) -> &'static str {
        match self.kind {
            Kind::Item => "items",
            Kind::Attachment => "attachments",
            Kind::Note => "notes",
            Kind::Collection => "collections",
        }
    }
    fn doc(self) -> Option<DocRef> {
        match self.kind {
            Kind::Item => Some(DocRef::item(self.id)),
            Kind::Attachment => Some(DocRef::attachment(self.id)),
            Kind::Note => Some(DocRef::note(self.id)),
            Kind::Collection => None,
        }
    }
}

fn docs(targets: &[Target]) -> Vec<DocRef> {
    targets.iter().filter_map(|t| t.doc()).collect()
}

/// 名字类输入：去首尾空白，不能为空、不能有控制字符、不能太长。
fn clean_name(s: &str, what: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{what}不能为空"));
    }
    if t.chars().count() > 500 || t.chars().any(|c| c.is_control()) {
        return Err(format!("{what}太长或含有不能显示的字符"));
    }
    Ok(t.to_string())
}

fn check_item_type(t: &str) -> Result<(), String> {
    if t.is_empty() || t.len() > 40 || !t.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(format!("文献类型不对：{t}"));
    }
    Ok(())
}

fn check_field_name(n: &str) -> Result<(), String> {
    if n.is_empty() || n.len() > 64 || n.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("字段名不对：{n}"));
    }
    Ok(())
}

fn exists(c: &Connection, t: Target) -> Result<bool, String> {
    Ok(c.query_row(&format!("SELECT 1 FROM {} WHERE id = ?1", t.table()), [t.id], |_| Ok(())).optional().map_err(err)?.is_some())
}

fn must_exist(c: &Connection, t: Target) -> Result<(), String> {
    if exists(c, t)? { Ok(()) } else { Err("找不到要操作的记录，可能已被删除，请刷新".into()) }
}

fn touch(c: &Connection, t: Target, now: &str) -> Result<(), String> {
    c.execute(&format!("UPDATE {} SET updated_at = ?1 WHERE id = ?2", t.table()), params![now, t.id]).map_err(err)?;
    Ok(())
}

/// 集合存在、自己和上级都不在回收站
fn live_collection(c: &Connection, id: i64) -> Result<(), String> {
    let mut cur = Some(id);
    let mut guard = 0;
    while let Some(x) = cur {
        let row: Option<(Option<String>, Option<i64>)> =
            c.query_row("SELECT deleted_at, parent_id FROM collections WHERE id = ?1", [x], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?;
        match row {
            None if x == id => return Err("找不到这个集合".into()),
            None => return Ok(()),
            Some((Some(_), _)) => return Err("这个集合在回收站里".into()),
            Some((None, parent)) => cur = parent,
        }
        guard += 1;
        if guard > 10_000 {
            return Err("集合层级异常（成环）".into());
        }
    }
    Ok(())
}

// ---------------- 文献 ----------------

/// 新建空白文献（手动），可直接放进一个集合。返回 id。
pub fn item_create(c: &Connection, item_type: &str, collection: Option<i64>) -> Result<i64, String> {
    check_item_type(item_type)?;
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    tx.execute(
        "INSERT INTO items (uid, item_type, source, created_at, updated_at) VALUES (?1, ?2, 'manual', ?3, ?3)",
        params![new_uid()?, item_type, now],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    if let Some(cid) = collection {
        live_collection(&tx, cid)?;
        tx.execute("INSERT INTO collection_items (collection_id, item_id) VALUES (?1, ?2)", params![cid, id]).map_err(err)?;
    }
    tx.commit()?;
    Ok(id)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPatch {
    pub item_type: Option<String>,
    /// 字段名 → 新值，空字符串表示删除。三个虚拟名 `title` / `date` / `container` 落到这条文献实际用的字段上
    /// （案例的标题是 caseName、章节的书名是 bookTitle……）；日期写成 Zotero 两段式。
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    pub creators: Option<Vec<CreatorInput>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatorInput {
    /// Zotero 角色名：author / editor / translator / …
    pub role: String,
    pub family: Option<String>,
    pub given: Option<String>,
    /// 单栏姓名（机构、中文姓名）；有它时忽略 family / given
    pub literal: Option<String>,
}

fn opt_trim(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}

fn set_creators(c: &Connection, id: i64, creators: &[CreatorInput]) -> Result<(), String> {
    c.execute("DELETE FROM creators WHERE item_id = ?1", [id]).map_err(err)?;
    let mut pos = 0i64;
    for cr in creators {
        if cr.role.is_empty() || !cr.role.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("作者角色不对：{}", cr.role));
        }
        let (family, given, literal) = match opt_trim(&cr.literal) {
            Some(l) => (None, None, Some(l)),
            None => (opt_trim(&cr.family), opt_trim(&cr.given), None),
        };
        if family.is_none() && given.is_none() && literal.is_none() {
            continue; // 空行不存
        }
        c.execute(
            "INSERT INTO creators (item_id, role, family, given, literal, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, cr.role, family, given, literal, pos],
        )
        .map_err(err)?;
        pos += 1;
    }
    Ok(())
}

fn set_field(c: &Connection, id: i64, f: &mut BTreeMap<String, String>, name: &str, value: &str) -> Result<(), String> {
    let v = value.trim();
    if v.is_empty() {
        c.execute("DELETE FROM item_fields WHERE item_id = ?1 AND field = ?2", params![id, name]).map_err(err)?;
        f.remove(name);
        return Ok(());
    }
    let stored = if fields::DATE_FIELDS.contains(&name) { text::multipart(text::parse_date(v), v) } else { v.to_string() };
    c.execute(
        "INSERT INTO item_fields (item_id, field, value) VALUES (?1, ?2, ?3) ON CONFLICT (item_id, field) DO UPDATE SET value = excluded.value",
        params![id, name, stored],
    )
    .map_err(err)?;
    f.insert(name.to_string(), stored);
    Ok(())
}

/// 改一条文献（在调用方的事务里）。
fn apply_patch(c: &Connection, id: i64, patch: &ItemPatch, now: &str) -> Result<(), String> {
    let old_type: String =
        c.query_row("SELECT item_type FROM items WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?.ok_or("找不到这条文献，可能已被删除")?;
    let mut f = fields::load(c, id)?;
    let mut ty = old_type.clone();
    if let Some(new_type) = patch.item_type.as_deref().filter(|t| *t != old_type) {
        check_item_type(new_type)?;
        // 换类型时「期刊 / 书名」跟着换到新类型的字段（期刊文章的 publicationTitle → 章节的 bookTitle）
        if let (Some(old_cf), Some(new_cf)) = (fields::container_field(&old_type, &f), fields::natural_container(new_type))
            && old_cf != new_cf
            && !f.contains_key(new_cf)
            && let Some(v) = f.get(old_cf).cloned()
        {
            set_field(c, id, &mut f, old_cf, "")?;
            set_field(c, id, &mut f, new_cf, &v)?;
        }
        c.execute("UPDATE items SET item_type = ?1 WHERE id = ?2", params![new_type, id]).map_err(err)?;
        ty = new_type.to_string();
    }
    for (name, value) in &patch.fields {
        let real: &str = match name.as_str() {
            "title" => fields::title_field(&f),
            "date" => fields::date_field(&f),
            "container" => fields::container_field(&ty, &f).ok_or("这种类型没有「期刊 / 书名」")?,
            other => {
                check_field_name(other)?;
                other
            }
        };
        let real = real.to_string();
        set_field(c, id, &mut f, &real, value)?;
    }
    if let Some(cr) = &patch.creators {
        set_creators(c, id, cr)?;
    }
    fields::refresh_columns(c, id, now)
}

pub fn item_update(c: &Connection, id: i64, patch: &ItemPatch) -> Result<Vec<DocRef>, String> {
    let tx = Tx::begin(c)?;
    apply_patch(&tx, id, patch, &text::now_iso())?;
    tx.commit()?;
    Ok(vec![DocRef::item(id)])
}

/// 批量：多条文献的同一个字段改成同一个值（`itemType` 改类型；`title` / `date` / `container` 同上）。
pub fn items_set_field(c: &Connection, ids: &[i64], field: &str, value: &str) -> Result<Vec<DocRef>, String> {
    let patch = if field == "itemType" {
        ItemPatch { item_type: Some(value.trim().to_string()), ..Default::default() }
    } else {
        ItemPatch { fields: BTreeMap::from([(field.to_string(), value.to_string())]), ..Default::default() }
    };
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    for &id in ids {
        apply_patch(&tx, id, &patch, &now)?;
    }
    tx.commit()?;
    Ok(ids.iter().map(|&i| DocRef::item(i)).collect())
}

// ---------------- 附件 ----------------

fn content_type_of(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html",
        "epub" => "application/epub+zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => return None,
    })
}

/// 把本机的一个文件作为附件加进来（复制进文件库，原文件不动）。`item` 为空 = 独立附件。返回附件 id。
pub fn attachment_add(c: &Connection, attachments_dir: &Path, item: Option<i64>, path: &Path) -> Result<(i64, Vec<DocRef>), String> {
    if !path.is_file() {
        return Err(format!("找不到这个文件：{}", path.display()));
    }
    if let Some(i) = item {
        must_exist(c, Target::item(i))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (sha, _) = zotero_import::util::hash_file(path).map_err(|e| format!("读不了这个文件：{e}"))?;
    let known: Option<String> = c.query_row("SELECT ext FROM files WHERE sha256 = ?1", [&sha], |r| r.get(0)).optional().map_err(err)?;
    let ext = known.unwrap_or_else(|| store::ext_for(Some(&name), None));
    let put = store::put(attachments_dir, path, &sha, &ext)?;
    let result = (|| {
        let tx = Tx::begin(c)?;
        let now = text::now_iso();
        tx.execute(
            "INSERT OR IGNORE INTO files (sha256, size, ext, md5, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![put.sha256, put.size as i64, ext, put.md5, now],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO attachments (uid, item_id, kind, sha256, status, title, original_name, content_type, created_at, updated_at)
             VALUES (?1, ?2, 'stored', ?3, 'ok', ?4, ?4, ?5, ?6, ?6)",
            params![new_uid()?, item, put.sha256, name, content_type_of(&ext), now],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        if let Some(i) = item {
            touch(&tx, Target::item(i), &now)?;
        }
        tx.commit()?;
        Ok(id)
    })();
    match result {
        Ok(id) => Ok((id, vec![DocRef::attachment(id)])),
        Err(e) => {
            if put.created {
                store::remove_created(attachments_dir, &[put.path]);
            }
            Err(e)
        }
    }
}

pub fn attachment_rename(c: &Connection, id: i64, title: &str) -> Result<Vec<DocRef>, String> {
    let title = clean_name(title, "附件名")?;
    let n = c.execute("UPDATE attachments SET title = ?1, updated_at = ?2 WHERE id = ?3", params![title, text::now_iso(), id]).map_err(err)?;
    if n == 0 {
        return Err("找不到这个附件".into());
    }
    Ok(vec![DocRef::attachment(id)])
}

// ---------------- 标签 ----------------

fn tag_link(t: Target) -> Result<(&'static str, &'static str), String> {
    match t.kind {
        Kind::Item => Ok(("item_tags", "item_id")),
        Kind::Attachment => Ok(("attachment_tags", "attachment_id")),
        Kind::Note => Ok(("note_tags", "note_id")),
        Kind::Collection => Err("集合不能加标签".into()),
    }
}

const TAG_LINKS: [(&str, &str, Kind); 3] = [("item_tags", "item_id", Kind::Item), ("attachment_tags", "attachment_id", Kind::Attachment), ("note_tags", "note_id", Kind::Note)];

fn tag_id(c: &Connection, name: &str) -> Result<Option<i64>, String> {
    c.query_row("SELECT id FROM tags WHERE name = ?1", [name], |r| r.get(0)).optional().map_err(err)
}

/// 没有任何记录用的标签删掉（Zotero 也这样）
fn purge_tag_if_unused(c: &Connection, id: i64) -> Result<(), String> {
    c.execute(
        "DELETE FROM tags WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM item_tags WHERE tag_id = ?1) AND NOT EXISTS (SELECT 1 FROM attachment_tags WHERE tag_id = ?1)
           AND NOT EXISTS (SELECT 1 FROM note_tags WHERE tag_id = ?1) AND NOT EXISTS (SELECT 1 FROM annotation_tags WHERE tag_id = ?1)",
        [id],
    )
    .map_err(err)?;
    Ok(())
}

/// 标签的全部使用者（索引要重写的文档）
fn tag_users(c: &Connection, id: i64) -> Result<Vec<DocRef>, String> {
    let mut out = Vec::new();
    for (table, col, kind) in TAG_LINKS {
        let mut st = c.prepare(&format!("SELECT {col} FROM {table} WHERE tag_id = ?1")).map_err(err)?;
        for r in st.query_map([id], |r| r.get::<_, i64>(0)).map_err(err)? {
            out.extend(Target { kind, id: r.map_err(err)? }.doc());
        }
    }
    Ok(out)
}

pub fn tags_add(c: &Connection, targets: &[Target], name: &str) -> Result<Vec<DocRef>, String> {
    let name = clean_name(name, "标签")?;
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    let id = match tag_id(&tx, &name)? {
        Some(id) => id,
        None => {
            tx.execute("INSERT INTO tags (name) VALUES (?1)", [&name]).map_err(err)?;
            tx.last_insert_rowid()
        }
    };
    for &t in targets {
        let (table, col) = tag_link(t)?;
        must_exist(&tx, t)?;
        tx.execute(&format!("INSERT OR IGNORE INTO {table} ({col}, tag_id, kind) VALUES (?1, ?2, 'manual')"), params![t.id, id]).map_err(err)?;
        touch(&tx, t, &now)?;
    }
    tx.commit()?;
    Ok(docs(targets))
}

pub fn tags_remove(c: &Connection, targets: &[Target], name: &str) -> Result<Vec<DocRef>, String> {
    let tx = Tx::begin(c)?;
    let Some(id) = tag_id(&tx, name)? else { return Ok(vec![]) };
    let now = text::now_iso();
    for &t in targets {
        let (table, col) = tag_link(t)?;
        if tx.execute(&format!("DELETE FROM {table} WHERE {col} = ?1 AND tag_id = ?2"), params![t.id, id]).map_err(err)? > 0 {
            touch(&tx, t, &now)?;
        }
    }
    purge_tag_if_unused(&tx, id)?;
    tx.commit()?;
    Ok(docs(targets))
}

/// 改名；新名字已有就合并进那个标签。
pub fn tag_rename(c: &Connection, id: i64, name: &str) -> Result<Vec<DocRef>, String> {
    let name = clean_name(name, "标签")?;
    let tx = Tx::begin(c)?;
    let users = tag_users(&tx, id)?;
    match tag_id(&tx, &name)? {
        Some(same) if same == id => {}
        Some(other) => {
            for table in ["item_tags", "attachment_tags", "note_tags", "annotation_tags"] {
                let col = match table {
                    "item_tags" => "item_id",
                    "attachment_tags" => "attachment_id",
                    "note_tags" => "note_id",
                    _ => "annotation_id",
                };
                tx.execute(&format!("INSERT OR IGNORE INTO {table} ({col}, tag_id, kind) SELECT {col}, ?1, kind FROM {table} WHERE tag_id = ?2"), params![other, id])
                    .map_err(err)?;
            }
            tx.execute("DELETE FROM tags WHERE id = ?1", [id]).map_err(err)?;
        }
        None => {
            if tx.execute("UPDATE tags SET name = ?1 WHERE id = ?2", params![name, id]).map_err(err)? == 0 {
                return Err("找不到这个标签".into());
            }
        }
    }
    tx.commit()?;
    Ok(users)
}

/// 从所有记录上去掉这个标签。
pub fn tag_delete(c: &Connection, id: i64) -> Result<Vec<DocRef>, String> {
    let tx = Tx::begin(c)?;
    let users = tag_users(&tx, id)?;
    tx.execute("DELETE FROM tags WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()?;
    Ok(users)
}

// ---------------- 集合 ----------------

pub fn collection_create(c: &Connection, name: &str, parent: Option<i64>) -> Result<i64, String> {
    let name = clean_name(name, "集合名")?;
    if let Some(p) = parent {
        live_collection(c, p)?;
    }
    let now = text::now_iso();
    c.execute("INSERT INTO collections (uid, parent_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", params![new_uid()?, parent, name, now])
        .map_err(err)?;
    Ok(c.last_insert_rowid())
}

pub fn collection_rename(c: &Connection, id: i64, name: &str) -> Result<(), String> {
    let name = clean_name(name, "集合名")?;
    if c.execute("UPDATE collections SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, text::now_iso(), id]).map_err(err)? == 0 {
        return Err("找不到这个集合".into());
    }
    Ok(())
}

/// 移到另一个集合下面（None = 顶层）。不能移到自己或自己的下级里。
pub fn collection_move(c: &Connection, id: i64, parent: Option<i64>) -> Result<(), String> {
    must_exist(c, Target { kind: Kind::Collection, id })?;
    if let Some(p) = parent {
        live_collection(c, p)?;
        let mut cur = Some(p);
        let mut guard = 0;
        while let Some(x) = cur {
            if x == id {
                return Err("不能把集合移到它自己或它的下级集合里".into());
            }
            cur = c.query_row("SELECT parent_id FROM collections WHERE id = ?1", [x], |r| r.get(0)).map_err(err)?;
            guard += 1;
            if guard > 10_000 {
                return Err("集合层级异常（成环）".into());
            }
        }
    }
    c.execute("UPDATE collections SET parent_id = ?1, updated_at = ?2 WHERE id = ?3", params![parent, text::now_iso(), id]).map_err(err)?;
    Ok(())
}

/// 能直接放进集合的：文献、独立附件、独立笔记（与 Zotero 一致，子附件 / 子笔记跟着文献走）
fn membership(c: &Connection, t: Target) -> Result<Option<(&'static str, &'static str)>, String> {
    Ok(match t.kind {
        Kind::Item => Some(("collection_items", "item_id")),
        Kind::Attachment => {
            let standalone: bool = c
                .query_row("SELECT item_id IS NULL AND note_id IS NULL FROM attachments WHERE id = ?1", [t.id], |r| r.get(0))
                .optional()
                .map_err(err)?
                .unwrap_or(false);
            standalone.then_some(("collection_attachments", "attachment_id"))
        }
        Kind::Note => {
            let standalone: bool =
                c.query_row("SELECT item_id IS NULL FROM notes WHERE id = ?1", [t.id], |r| r.get(0)).optional().map_err(err)?.unwrap_or(false);
            standalone.then_some(("collection_notes", "note_id"))
        }
        Kind::Collection => None,
    })
}

/// 加进集合，返回实际加进去的个数（子附件、子笔记、集合会被跳过）。
pub fn collection_add(c: &Connection, collection: i64, targets: &[Target]) -> Result<usize, String> {
    let tx = Tx::begin(c)?;
    live_collection(&tx, collection)?;
    let mut n = 0;
    for &t in targets {
        if let Some((table, col)) = membership(&tx, t)? {
            n += tx.execute(&format!("INSERT OR IGNORE INTO {table} (collection_id, {col}) VALUES (?1, ?2)"), params![collection, t.id]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn collection_remove(c: &Connection, collection: i64, targets: &[Target]) -> Result<usize, String> {
    let tx = Tx::begin(c)?;
    let mut n = 0;
    for &t in targets {
        if let Some((table, col)) = membership(&tx, t)? {
            n += tx.execute(&format!("DELETE FROM {table} WHERE collection_id = ?1 AND {col} = ?2"), params![collection, t.id]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(n)
}

// ---------------- 回收站 ----------------

/// 移进回收站（集合：里面的文献不动，下级集合随它进回收站）。
pub fn trash(c: &Connection, targets: &[Target]) -> Result<Vec<DocRef>, String> {
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    for &t in targets {
        must_exist(&tx, t)?;
        tx.execute(&format!("UPDATE {} SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL", t.table()), params![now, t.id]).map_err(err)?;
    }
    tx.commit()?;
    Ok(docs(targets))
}

fn undelete(c: &Connection, table: &str, id: i64) -> Result<(), String> {
    c.execute(&format!("UPDATE {table} SET deleted_at = NULL WHERE id = ?1"), [id]).map_err(err)?;
    Ok(())
}

/// 从回收站恢复。上级也在回收站时一起恢复（子附件 → 文献；内嵌图片 → 笔记 → 文献；集合 → 上级集合）。
pub fn restore(c: &Connection, targets: &[Target]) -> Result<Vec<DocRef>, String> {
    let tx = Tx::begin(c)?;
    let mut refs = docs(targets);
    for &t in targets {
        must_exist(&tx, t)?;
        undelete(&tx, t.table(), t.id)?;
        match t.kind {
            Kind::Item => {}
            Kind::Attachment => {
                let (item, note): (Option<i64>, Option<i64>) =
                    tx.query_row("SELECT item_id, note_id FROM attachments WHERE id = ?1", [t.id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
                let item = match note {
                    Some(n) => {
                        undelete(&tx, "notes", n)?;
                        tx.query_row("SELECT item_id FROM notes WHERE id = ?1", [n], |r| r.get(0)).map_err(err)?
                    }
                    None => item,
                };
                if let Some(i) = item {
                    undelete(&tx, "items", i)?;
                    refs.push(DocRef::item(i));
                }
            }
            Kind::Note => {
                if let Some(i) = tx.query_row("SELECT item_id FROM notes WHERE id = ?1", [t.id], |r| r.get::<_, Option<i64>>(0)).map_err(err)? {
                    undelete(&tx, "items", i)?;
                    refs.push(DocRef::item(i));
                }
            }
            Kind::Collection => {
                let mut cur: Option<i64> = tx.query_row("SELECT parent_id FROM collections WHERE id = ?1", [t.id], |r| r.get(0)).map_err(err)?;
                let mut guard = 0;
                while let Some(p) = cur {
                    undelete(&tx, "collections", p)?;
                    cur = tx.query_row("SELECT parent_id FROM collections WHERE id = ?1", [p], |r| r.get(0)).map_err(err)?;
                    guard += 1;
                    if guard > 10_000 {
                        break;
                    }
                }
            }
        }
    }
    tx.commit()?;
    Ok(refs)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    /// 删掉的记录（不含随之删除的子附件、子笔记）
    pub records: usize,
    /// 从文件库删掉的文件（库里和所有备份里都没人用的）
    pub files_removed: usize,
    pub backup_file: Option<String>,
}

/// 回收站里直接删除的全部记录
pub fn trashed(c: &Connection) -> Result<Vec<Target>, String> {
    let mut out = Vec::new();
    for (table, kind) in [("items", Kind::Item), ("attachments", Kind::Attachment), ("notes", Kind::Note), ("collections", Kind::Collection)] {
        let mut st = c.prepare(&format!("SELECT id FROM {table} WHERE deleted_at IS NOT NULL")).map_err(err)?;
        for r in st.query_map([], |r| r.get::<_, i64>(0)).map_err(err)? {
            out.push(Target { kind, id: r.map_err(err)? });
        }
    }
    Ok(out)
}

/// 永久删除回收站里的记录（只接受直接删除的那些）。先备份；子附件、子笔记、标注、下级集合随之删除；
/// 删掉没人引用的文件记录；文件本身等到没有备份用到时再删（`backup::sweep_files`）。
pub fn delete_forever(c: &Connection, backups: &Path, attachments_dir: &Path, targets: &[Target]) -> Result<(DeleteReport, Vec<DocRef>), String> {
    if targets.is_empty() {
        return Ok((DeleteReport::default(), vec![]));
    }
    for &t in targets {
        let deleted: Option<Option<String>> =
            c.query_row(&format!("SELECT deleted_at FROM {} WHERE id = ?1", t.table()), [t.id], |r| r.get(0)).optional().map_err(err)?;
        if !matches!(deleted, Some(Some(_))) {
            return Err("只能永久删除回收站里的内容".into());
        }
    }
    let backup = crate::backup::snapshot(c, backups, "before-delete")?;
    crate::backup::prune_safety(backups, crate::backup::KEEP_SAFETY)?;
    let tx = Tx::begin(c)?;
    let mut refs = docs(targets);
    for &t in targets {
        // 子附件 / 子笔记被删：所属文献的索引内容要重写
        let parent: Option<i64> = match t.kind {
            Kind::Attachment | Kind::Note => tx.query_row(&format!("SELECT item_id FROM {} WHERE id = ?1", t.table()), [t.id], |r| r.get(0)).map_err(err)?,
            _ => None,
        };
        tx.execute(&format!("DELETE FROM {} WHERE id = ?1", t.table()), [t.id]).map_err(err)?;
        refs.extend(parent.map(DocRef::item));
    }
    let orphans = unreferenced_files(&tx)?;
    for (sha, _) in &orphans {
        tx.execute("DELETE FROM files WHERE sha256 = ?1", [sha]).map_err(err)?;
    }
    tx.commit()?;
    // 数据库提交之后再删文件：就算删文件失败，库也是一致的（多出的文件无害）。
    // 还有备份用到的文件留着（刚做的「永久删除前」备份就用到），恢复备份时附件还在；备份清理掉之后再删。
    let files_removed = match crate::backup::sweep_files(c, backups, attachments_dir) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("清理附件文件失败：{e}");
            0
        }
    };
    let report = DeleteReport {
        records: targets.len(),
        files_removed,
        backup_file: backup.file_name().map(|f| f.to_string_lossy().into_owned()),
    };
    Ok((report, refs))
}

/// 清空回收站。
pub fn empty_trash(c: &Connection, backups: &Path, attachments_dir: &Path) -> Result<(DeleteReport, Vec<DocRef>), String> {
    let all = trashed(c)?;
    delete_forever(c, backups, attachments_dir, &all)
}

/// 文件库里没有任何附件引用的文件（主文件看 attachments.sha256，快照资源看 source_meta.extraFiles）
fn unreferenced_files(c: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut st = c
        .prepare(
            "SELECT f.sha256, f.ext FROM files f
              WHERE NOT EXISTS (SELECT 1 FROM attachments a WHERE a.sha256 = f.sha256)
                AND f.sha256 NOT IN (SELECT json_extract(e.value, '$[1]')
                                       FROM (SELECT source_meta FROM attachments WHERE source_meta IS NOT NULL AND json_valid(source_meta)) a,
                                            json_each(a.source_meta, '$.extraFiles') e)",
        )
        .map_err(err)?;
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

// ---------------- 合并重复 ----------------

/// 把 `others` 合并进 `master`：子附件、子笔记、标签、集合、关联都并到 master；master 缺的字段用别的补上（不覆盖已有值），
/// master 没有作者时用第一条有作者的；其余条目移进回收站，并在 master 上记一条 `dc:replaces` 关联（可追溯）。
pub fn items_merge(c: &Connection, master: i64, others: &[i64]) -> Result<Vec<DocRef>, String> {
    if others.is_empty() || others.contains(&master) {
        return Err("请至少选两条不同的文献".into());
    }
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    let alive = |id: i64| -> Result<bool, String> {
        Ok(tx.query_row("SELECT deleted_at IS NULL FROM items WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?.unwrap_or(false))
    };
    for &id in std::iter::once(&master).chain(others) {
        if !alive(id)? {
            return Err("要合并的文献不存在或在回收站里".into());
        }
    }
    let mut master_fields = fields::load(&tx, master)?;
    for &o in others {
        tx.execute("UPDATE attachments SET item_id = ?1, updated_at = ?3 WHERE item_id = ?2", params![master, o, now]).map_err(err)?;
        tx.execute("UPDATE notes SET item_id = ?1, updated_at = ?3 WHERE item_id = ?2", params![master, o, now]).map_err(err)?;
        tx.execute("INSERT OR IGNORE INTO item_tags (item_id, tag_id, kind) SELECT ?1, tag_id, kind FROM item_tags WHERE item_id = ?2", params![master, o]).map_err(err)?;
        tx.execute("INSERT OR IGNORE INTO collection_items (collection_id, item_id) SELECT collection_id, ?1 FROM collection_items WHERE item_id = ?2", params![master, o])
            .map_err(err)?;
        tx.execute("INSERT OR IGNORE INTO item_relations (item_id, predicate, object) SELECT ?1, predicate, object FROM item_relations WHERE item_id = ?2", params![master, o])
            .map_err(err)?;
        for (k, v) in fields::load(&tx, o)? {
            if master_fields.get(&k).is_none_or(|m| m.trim().is_empty()) && !v.trim().is_empty() {
                tx.execute(
                    "INSERT INTO item_fields (item_id, field, value) VALUES (?1, ?2, ?3) ON CONFLICT (item_id, field) DO UPDATE SET value = excluded.value",
                    params![master, k, v],
                )
                .map_err(err)?;
                master_fields.insert(k, v);
            }
        }
        let master_has_creators: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM creators WHERE item_id = ?1)", [master], |r| r.get(0)).map_err(err)?;
        if !master_has_creators {
            tx.execute(
                "INSERT INTO creators (item_id, role, family, given, literal, position) SELECT ?1, role, family, given, literal, position FROM creators WHERE item_id = ?2",
                params![master, o],
            )
            .map_err(err)?;
        }
        let uid: String = tx.query_row("SELECT uid FROM items WHERE id = ?1", [o], |r| r.get(0)).map_err(err)?;
        tx.execute("INSERT OR IGNORE INTO item_relations (item_id, predicate, object) VALUES (?1, 'dc:replaces', ?2)", params![master, format!("researchlab:item:{uid}")])
            .map_err(err)?;
        tx.execute("UPDATE items SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2", params![now, o]).map_err(err)?;
    }
    fields::refresh_columns(&tx, master, &now)?;
    tx.commit()?;
    Ok(std::iter::once(master).chain(others.iter().copied()).map(DocRef::item).collect())
}

#[cfg(test)]
mod tests;
