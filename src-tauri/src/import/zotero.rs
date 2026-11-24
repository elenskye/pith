//! Zotero 数据目录（副本）→ library.db。读取用 `crates/zotero-import`（只读、不加锁、不改任何字节），映射见 decisions/0009。
//!
//! 流程：读取 → 读取核对（Zotero 数据库 ↔ 读出的数据，独立 SQL）→ 是否导入过 → before-import 备份 → 附件复制进文件库
//! → 一个事务写入 → 写入核对（读出的数据 ↔ 库，逐类逐条）→ 提交。任何一步失败：回滚，删掉这次新复制的文件。

use super::{Check, Count, FileIssue, NewItem, Report, RunDetail, Stats, Writer, backup_before, compare, runs_get};
use crate::common::{Tx, err, new_uid};
use crate::paths::DataPaths;
use crate::store;
use crate::text::{iso_to_zotero_time as back, zotero_time_to_iso as iso};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value as J, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use zotero_import::model::{Attachment, FileKind, ItemBase, LinkMode, ZoteroLibrary};
use zotero_import::reader::{self, ReadOptions};
use zotero_import::{audit, util};

pub const SOURCE: &str = "zotero";

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Zotero 首选项里的「链接附件根目录」，解析 `attachments:` 相对路径用
    pub base_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub dir: String,
    pub client_version: Option<String>,
    pub userdata_version: Option<i64>,
    /// 这个 Zotero 数据库版本是否实测过
    pub version_tested: bool,
    pub items: usize,
    pub attachments: usize,
    pub notes: usize,
    pub annotations: usize,
    pub collections: usize,
    pub tags: usize,
    /// 回收站里直接删除的条目、附件、笔记
    pub trash_items: usize,
    pub trash_collections: usize,
    pub group_libraries: usize,
    /// 要复制进文件库的文件
    pub files: usize,
    pub bytes: u64,
    pub missing_files: Vec<FileIssue>,
    /// 用相对路径（attachments:）的链接附件个数；有的话需要提供根目录
    pub base_relative: usize,
    /// 库里已经有的（同一 Zotero 库、同一 key）
    pub already_imported: usize,
    /// 数据目录旁有 -journal / -wal 文件：Zotero 可能正开着
    pub zotero_running: bool,
    pub warnings: Vec<String>,
    /// 不能导入的原因
    pub blocked: Option<String>,
}

fn read(dir: &Path, opts: &Options, hashing: bool) -> Result<ZoteroLibrary, String> {
    if !reader::db_path(dir).is_file() {
        return Err("这个文件夹里没有 zotero.sqlite。请选择 Zotero 数据目录的副本（见 docs/zotero-copy-guide.md）。".into());
    }
    let ro = ReadOptions {
        base_attachment_dir: opts.base_dir.clone(),
        skip_hashing: !hashing,
    };
    reader::read_library(dir, &ro).map_err(|e| format!("读不了 Zotero 数据：{e}"))
}

pub fn preview(c: &Connection, dir: &Path, opts: &Options) -> Result<Preview, String> {
    let lib = read(dir, opts, false)?;
    let titles = Titles::of(&lib);
    let mut p = Preview {
        dir: dir.display().to_string(),
        client_version: lib.source.client_version.clone(),
        userdata_version: lib.source.schema_versions.get("userdata").copied(),
        version_tested: lib
            .source
            .schema_versions
            .get("userdata")
            .is_some_and(|v| audit::TESTED_USERDATA_VERSIONS.contains(v)),
        items: lib.items.len(),
        attachments: lib.attachments.len(),
        notes: lib.notes.len(),
        annotations: lib.annotations.len(),
        collections: lib.collections.len(),
        tags: lib
            .all_bases()
            .flat_map(|b| b.tags.iter().map(|t| t.name.as_str()))
            .collect::<HashSet<_>>()
            .len(),
        trash_items: lib.all_bases().filter(|b| b.deleted_at.is_some() && b.item_type != "annotation").count(),
        trash_collections: lib.collections.iter().filter(|c| c.deleted_at.is_some()).count(),
        group_libraries: lib.libraries.iter().filter(|l| l.kind == "group").count(),
        files: 0,
        bytes: 0,
        missing_files: vec![],
        base_relative: 0,
        already_imported: already_imported(c, &lib)?,
        zotero_running: ["zotero.sqlite-journal", "zotero.sqlite-wal"].iter().any(|f| dir.join(f).exists()),
        warnings: lib.warnings.clone(),
        blocked: None,
    };
    for a in &lib.attachments {
        if let Some(f) = a.file.as_ref().filter(|f| f.exists) {
            p.files += 1;
            p.bytes += f.size.unwrap_or(0);
        }
        if a.file.as_ref().is_some_and(|f| f.kind == FileKind::BaseRelative) {
            p.base_relative += 1;
        }
        if let Some(i) = file_issue(a, &titles) {
            p.missing_files.push(i);
        }
    }
    if !p.version_tested {
        p.warnings.push(format!(
            "这个 Zotero 数据库版本（userdata {}）没有实测过；导入时会逐类核对，不一致就不会写入。",
            p.userdata_version.unwrap_or(0)
        ));
    }
    if p.group_libraries > 0 {
        p.warnings
            .push(format!("有 {} 个群组库：其中的条目会并入本库（Pith 没有群组）。", p.group_libraries));
    }
    if p.already_imported > 0 {
        p.blocked = Some(format!(
            "这个 Zotero 库已经导入过（{} 条记录已在库里）。重复导入与增量更新在 v0.1.x 做；要重新导入，请先恢复导入前的备份。",
            p.already_imported
        ));
    }
    Ok(p)
}

/// 库里已有的同一 Zotero 库、同一 key 的记录数。
fn already_imported(c: &Connection, lib: &ZoteroLibrary) -> Result<usize, String> {
    let mut items = c
        .prepare_cached("SELECT 1 FROM items WHERE source = 'zotero' AND source_library = ?1 AND source_key = ?2")
        .map_err(err)?;
    let mut n = 0;
    for it in &lib.items {
        if items.query_row(params![it.library_id, it.key], |_| Ok(())).optional().map_err(err)?.is_some() {
            n += 1;
        }
    }
    for (table, keys) in [
        ("attachments", lib.attachments.iter().map(|a| &a.base.key).collect::<Vec<_>>()),
        ("notes", lib.notes.iter().map(|x| &x.base.key).collect()),
        ("collections", lib.collections.iter().map(|x| &x.key).collect()),
    ] {
        let mut st = c
            .prepare(&format!(
                "SELECT 1 FROM {table} t JOIN import_runs r ON r.id = t.import_run_id WHERE r.source = 'zotero' AND t.source_key = ?1"
            ))
            .map_err(err)?;
        for k in keys {
            if st.query_row([k], |_| Ok(())).optional().map_err(err)?.is_some() {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 显示用的标题（缺失文件列表里给出附件名与所属文献）。
struct Titles(HashMap<(i64, String), String>);

impl Titles {
    fn of(lib: &ZoteroLibrary) -> Self {
        let mut m = HashMap::new();
        for b in lib.all_bases() {
            if let Some(t) = b.fields.get("title").or_else(|| b.fields.get("caseName")) {
                m.insert((b.library_id, b.key.clone()), t.clone());
            }
        }
        for n in &lib.notes {
            if let Some(t) = &n.title {
                m.insert((n.base.library_id, n.base.key.clone()), t.clone());
            }
        }
        Titles(m)
    }
    fn get(&self, lib: i64, key: Option<&String>) -> Option<String> {
        key.and_then(|k| self.0.get(&(lib, k.clone())).cloned())
    }
}

/// 这个附件本该有文件吗（网址链接没有文件）
fn file_expected(a: &Attachment) -> bool {
    match a.link_mode {
        Some(LinkMode::LinkedUrl) => false,
        Some(LinkMode::ImportedFile | LinkMode::ImportedUrl | LinkMode::EmbeddedImage | LinkMode::LinkedFile) => true,
        _ => a.path.is_some(),
    }
}

fn file_issue(a: &Attachment, titles: &Titles) -> Option<FileIssue> {
    if !file_expected(a) {
        return None;
    }
    let reason = match &a.file {
        Some(f) if f.exists => return None,
        None if a.path.is_none() => "Zotero 里没有记录文件路径".to_string(),
        None => "文件路径的格式认不出".to_string(),
        Some(f) => match f.kind {
            FileKind::Stored => "Zotero 的 storage 里没有这个文件（可能只在 Zotero 云端，需要先在 Zotero 里下载）".to_string(),
            FileKind::Linked => format!("链接的文件不存在：{}", a.path.as_deref().unwrap_or("")),
            FileKind::BaseRelative if f.resolved_path.is_none() => "使用相对路径，需要提供 Zotero 的「链接附件根目录」".to_string(),
            FileKind::BaseRelative => format!("相对路径的文件不存在：{}", f.resolved_path.as_deref().unwrap_or("")),
        },
    };
    Some(FileIssue {
        key: Some(a.base.key.clone()),
        title: titles.get(a.base.library_id, Some(&a.base.key)),
        parent_title: titles.get(a.base.library_id, a.parent_key.as_ref()),
        reason,
        in_trash: a.base.in_trash,
    })
}

/// 复制进文件库的结果
struct Copied {
    sha256: String,
    md5: String,
    size: u64,
    ext: String,
    created: bool,
}

type Key = (i64, String);

fn key_of(b: &ItemBase) -> Key {
    (b.library_id, b.key.clone())
}

fn known_ext(c: &Connection, sha: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT ext FROM files WHERE sha256 = ?1", [sha], |r| r.get(0))
        .optional()
        .map_err(err)
}

fn original_name(path: Option<&str>) -> Option<String> {
    let p = path?;
    let name = p.strip_prefix("storage:").map(String::from).unwrap_or_else(|| {
        let rel = p.strip_prefix("attachments:").unwrap_or(p);
        Path::new(rel)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel.to_string())
    });
    Some(name).filter(|n| !n.is_empty())
}

/// 主文件与快照资源文件复制进文件库。返回 (主文件, 资源文件)。
#[allow(clippy::type_complexity)]
fn copy_files(
    c: &Connection,
    root: &Path,
    storage: &Path,
    lib: &ZoteroLibrary,
    created: &mut Vec<PathBuf>,
) -> Result<(HashMap<Key, Copied>, HashMap<Key, Vec<(String, Copied)>>), String> {
    let mut main = HashMap::new();
    let mut extras: HashMap<Key, Vec<(String, Copied)>> = HashMap::new();
    // 同样的内容只存一份：扩展名以库里已有的、或这次第一次见到的为准
    let mut seen_ext: HashMap<String, String> = HashMap::new();
    let mut put = |src: &Path, sha: &str, name: Option<&str>, ctype: Option<&str>| -> Result<Copied, String> {
        let ext = match seen_ext.get(sha).cloned().map(Some).map_or_else(|| known_ext(c, sha), Ok)? {
            Some(e) => e,
            None => store::ext_for(name, ctype),
        };
        seen_ext.insert(sha.to_string(), ext.clone());
        let p = store::put(root, src, sha, &ext)?;
        if p.created {
            created.push(p.path.clone());
        }
        Ok(Copied {
            sha256: p.sha256,
            md5: p.md5,
            size: p.size,
            ext,
            created: p.created,
        })
    };
    for a in &lib.attachments {
        let Some(f) = a.file.as_ref().filter(|f| f.exists) else { continue };
        let (Some(src), Some(sha)) = (f.resolved_path.as_deref(), f.sha256.as_deref()) else {
            continue;
        };
        let name = original_name(a.path.as_deref());
        main.insert(key_of(&a.base), put(Path::new(src), sha, name.as_deref(), a.content_type.as_deref())?);
        for rel in &a.extra_files {
            let src = storage.join(&a.base.key).join(rel);
            let (sha, _) = util::hash_file(&src).map_err(|e| format!("读不了附件资源文件 {}：{e}", src.display()))?;
            let copied = put(&src, &sha, Some(rel), None)?;
            extras.entry(key_of(&a.base)).or_default().push((rel.clone(), copied));
        }
    }
    Ok((main, extras))
}

pub fn run(c: &Connection, paths: &DataPaths, dir: &Path, opts: &Options) -> Result<RunDetail, String> {
    let lib = read(dir, opts, true)?;
    // 1. 读取核对：Zotero 数据库 ↔ 读出的数据
    let storage = reader::storage_path(dir);
    let zconn = reader::open_readonly(&reader::db_path(dir)).map_err(|e| format!("读不了 Zotero 数据：{e}"))?;
    let audited = audit::audit(&zconn, &lib, &storage).map_err(|e| format!("核对 Zotero 数据时出错：{e}"))?;
    drop(zconn);
    let bad: Vec<&str> = audited.checks.iter().filter(|x| !x.ok).map(|x| x.label.as_str()).collect();
    if !bad.is_empty() {
        return Err(format!(
            "读取 Zotero 数据时核对不一致（{}），没有导入。请把这个情况告诉开发者。",
            bad.join("、")
        ));
    }
    // 2. 导入过就不再导入
    let dup = already_imported(c, &lib)?;
    if dup > 0 {
        return Err(format!("这个 Zotero 库已经导入过（{dup} 条记录已在库里），没有重复导入。"));
    }
    // 3. 备份 → 4. 复制文件 → 5. 写入与核对
    let backup_file = backup_before(c, &paths.backups)?;
    let mut created = Vec::new();
    let result = copy_files(c, &paths.attachments, &storage, &lib, &mut created).and_then(|(main, extras)| {
        let tx = Tx::begin(c)?;
        let detail = write(&tx, &paths.attachments, dir, &backup_file, &lib, &audited, &main, &extras)?;
        tx.commit()?;
        Ok(detail)
    });
    if result.is_err() {
        store::remove_created(&paths.attachments, &created);
    }
    result
}

fn rels(r: &[zotero_import::model::Relation]) -> J {
    J::Array(r.iter().map(|x| json!([x.predicate, x.object])).collect())
}

/// 所有类型共有的 Zotero 同步信息；附件 / 笔记 / 标注没有对应表的字段、作者、关联也放这里。
fn base_meta(b: &ItemBase, regular: bool) -> Map<String, J> {
    let mut m = Map::new();
    m.insert("libraryID".into(), json!(b.library_id));
    m.insert("version".into(), json!(b.version));
    m.insert("synced".into(), json!(b.synced));
    m.insert("clientDateModified".into(), json!(b.client_date_modified));
    if !regular {
        if !b.fields.is_empty() {
            m.insert("fields".into(), json!(b.fields));
        }
        if !b.relations.is_empty() {
            m.insert("relations".into(), rels(&b.relations));
        }
        if !b.creators.is_empty() {
            let cr: Vec<J> = b
                .creators
                .iter()
                .map(|c| json!([c.creator_type, c.first_name, c.last_name, c.field_mode, c.order_index]))
                .collect();
            m.insert("creators".into(), J::Array(cr));
        }
    }
    m
}

fn tag_kind(t: i64) -> &'static str {
    if t == 1 { "automatic" } else { "manual" }
}

/// 笔记 HTML 里 `data-attachment-key="KEY"` 的 KEY 换成 `f(KEY)`（附件 uid）；f 返回 None 的保持原样。
pub(crate) fn rewrite_keys(html: &str, f: impl Fn(&str) -> Option<String>) -> String {
    const ATTR: &str = "data-attachment-key=";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find(ATTR) {
        let after = &rest[i + ATTR.len()..];
        let Some(q) = after.chars().next().filter(|q| *q == '"' || *q == '\'') else {
            out.push_str(&rest[..i + ATTR.len()]);
            rest = after;
            continue;
        };
        let Some(end) = after[1..].find(q) else {
            break;
        };
        let key = &after[1..1 + end];
        out.push_str(&rest[..i + ATTR.len()]);
        out.push(q);
        out.push_str(&f(key).unwrap_or_else(|| key.to_string()));
        out.push(q);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[allow(clippy::too_many_arguments)]
fn write(
    c: &Connection,
    root: &Path,
    dir: &Path,
    backup_file: &str,
    lib: &ZoteroLibrary,
    audited: &audit::Report,
    files: &HashMap<Key, Copied>,
    extras: &HashMap<Key, Vec<(String, Copied)>>,
) -> Result<RunDetail, String> {
    let mut w = Writer::begin(c, SOURCE, dir, backup_file)?;
    let mut stats = Stats::default();
    let mut report = Report {
        source_checks: audited
            .checks
            .iter()
            .map(|x| Check {
                category: x.category.clone(),
                label: x.label.clone(),
                source: x.source_count,
                stored: x.parsed_count,
                ok: x.ok,
                diff: x.diff_sample.clone(),
            })
            .collect(),
        warnings: audited.warnings.clone(),
        ..Default::default()
    };
    let titles = Titles::of(lib);
    let mut skip = Skip::default();

    // ---- 集合：先上级后下级 ----
    let present: HashSet<Key> = lib.collections.iter().map(|c| (c.library_id, c.key.clone())).collect();
    let mut coll_ids: HashMap<Key, i64> = HashMap::new();
    let mut pending: Vec<usize> = (0..lib.collections.len()).collect();
    while !pending.is_empty() {
        let parent_of = |i: usize| lib.collections[i].parent_key.as_ref().map(|p| (lib.collections[i].library_id, p.clone()));
        let ready: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|&i| parent_of(i).is_none_or(|p| !present.contains(&p) || coll_ids.contains_key(&p)))
            .collect();
        // 一个都放不下 = 成环：剩下的都作顶层导入
        let batch = if ready.is_empty() { pending.clone() } else { ready };
        for &i in &batch {
            let col = &lib.collections[i];
            let parent = parent_of(i);
            let parent_id = parent.as_ref().and_then(|p| coll_ids.get(p)).copied();
            let mut meta = Map::new();
            meta.insert("libraryID".into(), json!(col.library_id));
            meta.insert("version".into(), json!(col.version));
            meta.insert("synced".into(), json!(col.synced));
            if !col.relations.is_empty() {
                meta.insert("relations".into(), rels(&col.relations));
            }
            if parent.is_some() && parent_id.is_none() {
                // 上级不存在或成环：作为顶层导入，原上级 key 记在 meta 里
                meta.insert("orphanParentKey".into(), json!(col.parent_key));
                report.warnings.push(format!("集合「{}」的上级集合找不到，作为顶层集合导入", col.name));
            }
            c.execute(
                "INSERT INTO collections (uid, parent_id, name, source_key, created_at, updated_at, deleted_at, source_meta, import_run_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8)",
                params![
                    new_uid()?,
                    parent_id,
                    col.name,
                    col.key,
                    iso(&col.client_date_modified),
                    col.deleted_at.as_deref().map(iso),
                    J::Object(meta).to_string(),
                    w.run_id
                ],
            )
            .map_err(err)?;
            coll_ids.insert((col.library_id, col.key.clone()), c.last_insert_rowid());
            stats.collections += 1;
            stats.trash += col.deleted_at.is_some() as usize;
        }
        pending.retain(|i| !batch.contains(i));
    }

    let member = |b: &ItemBase, table: &str, col: &str, id: i64, skip: &mut Skip, report: &mut Report| -> Result<(), String> {
        for k in &b.collection_keys {
            match coll_ids.get(&(b.library_id, k.clone())) {
                Some(cid) => {
                    c.execute(
                        &format!("INSERT OR IGNORE INTO {table} (collection_id, {col}) VALUES (?1, ?2)"),
                        params![cid, id],
                    )
                    .map_err(err)?;
                }
                None => {
                    skip.memberships.insert((b.key.clone(), k.clone()));
                    report.warnings.push(format!(
                        "「{}」所在的集合 {k} 不存在，这条集合关系没有导入",
                        titles.get(b.library_id, Some(&b.key)).unwrap_or_else(|| b.key.clone())
                    ));
                }
            }
        }
        Ok(())
    };
    let tag_all = |w: &mut Writer, b: &ItemBase, table: &str, col: &str, id: i64, report: &mut Report| -> Result<(), String> {
        for t in &b.tags {
            if t.tag_type != 0 && t.tag_type != 1 {
                report.warnings.push(format!("标签「{}」的类型 {} 认不出，按手动标签导入", t.name, t.tag_type));
            }
            let tid = w.tag_id(&t.name)?;
            c.execute(
                &format!("INSERT OR IGNORE INTO {table} ({col}, tag_id, kind) VALUES (?1, ?2, ?3)"),
                params![id, tid, tag_kind(t.tag_type)],
            )
            .map_err(err)?;
        }
        Ok(())
    };

    // ---- 文献条目 ----
    let mut item_ids: HashMap<Key, i64> = HashMap::new();
    for it in &lib.items {
        let meta = J::Object(base_meta(it, true)).to_string();
        let id = w.insert_item(
            &NewItem {
                item_type: &it.item_type,
                source: SOURCE,
                source_library: Some(it.library_id),
                source_key: Some(&it.key),
                created_at: &iso(&it.date_added),
                updated_at: &iso(&it.date_modified),
                deleted_at: it.deleted_at.as_deref().map(iso),
                source_meta: Some(meta),
            },
            &it.fields,
        )?;
        item_ids.insert(key_of(it), id);
        for cr in &it.creators {
            // 单栏（fieldMode 1）：姓名整体放 literal，不拆
            let (family, literal) = if cr.field_mode == 1 {
                (None, Some(cr.last_name.clone().unwrap_or_default()))
            } else {
                (cr.last_name.clone(), None)
            };
            c.execute(
                "INSERT INTO creators (item_id, role, family, given, literal, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, cr.creator_type, family, cr.first_name, literal, cr.order_index],
            )
            .map_err(err)?;
        }
        for r in &it.relations {
            c.execute(
                "INSERT OR IGNORE INTO item_relations (item_id, predicate, object) VALUES (?1, ?2, ?3)",
                params![id, r.predicate, r.object],
            )
            .map_err(err)?;
        }
        tag_all(&mut w, it, "item_tags", "item_id", id, &mut report)?;
        member(it, "collection_items", "item_id", id, &mut skip, &mut report)?;
        stats.items += 1;
        stats.trash += it.deleted_at.is_some() as usize;
    }

    // ---- 笔记（附件 uid 先定好：笔记里的内嵌图片引用要改写成 uid）----
    let att_uid: HashMap<Key, String> = lib
        .attachments
        .iter()
        .map(|a| Ok((key_of(&a.base), new_uid()?)))
        .collect::<Result<_, String>>()?;
    let mut note_ids: HashMap<Key, i64> = HashMap::new();
    for n in &lib.notes {
        let lid = n.base.library_id;
        let mut meta = base_meta(&n.base, false);
        let item_id = match &n.parent_key {
            Some(pk) => {
                let id = item_ids.get(&(lid, pk.clone())).copied();
                if id.is_none() {
                    meta.insert("orphanParentKey".into(), json!(pk));
                    report.warnings.push(format!(
                        "笔记「{}」的上级条目找不到，作为独立笔记导入",
                        n.title.as_deref().unwrap_or(&n.base.key)
                    ));
                }
                id
            }
            None => None,
        };
        if n.html.is_none() {
            meta.insert("htmlMissing".into(), json!(true));
        }
        let html = n
            .html
            .as_deref()
            .map(|h| rewrite_keys(h, |k| att_uid.get(&(lid, k.to_string())).cloned()))
            .unwrap_or_default();
        c.execute(
            "INSERT INTO notes (uid, item_id, title, html, source_key, created_at, updated_at, deleted_at, source_meta, import_run_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                new_uid()?,
                item_id,
                n.title,
                html,
                n.base.key,
                iso(&n.base.date_added),
                iso(&n.base.date_modified),
                n.base.deleted_at.as_deref().map(iso),
                J::Object(meta).to_string(),
                w.run_id
            ],
        )
        .map_err(err)?;
        let id = c.last_insert_rowid();
        note_ids.insert(key_of(&n.base), id);
        tag_all(&mut w, &n.base, "note_tags", "note_id", id, &mut report)?;
        member(&n.base, "collection_notes", "note_id", id, &mut skip, &mut report)?;
        stats.notes += 1;
        stats.trash += n.base.deleted_at.is_some() as usize;
    }

    // ---- 附件 ----
    let mut att_ids: HashMap<Key, i64> = HashMap::new();
    for a in &lib.attachments {
        let k = key_of(&a.base);
        let lid = a.base.library_id;
        let mut meta = base_meta(&a.base, false);
        let (item_id, note_id) = match &a.parent_key {
            Some(pk) => {
                let pk = (lid, pk.clone());
                match (item_ids.get(&pk), note_ids.get(&pk)) {
                    (Some(i), _) => (Some(*i), None),
                    (None, Some(n)) => (None, Some(*n)),
                    _ => {
                        meta.insert("orphanParentKey".into(), json!(pk.1));
                        report.warnings.push(format!(
                            "附件「{}」的上级找不到，作为独立附件导入",
                            titles.get(lid, Some(&a.base.key)).unwrap_or_else(|| a.base.key.clone())
                        ));
                        (None, None)
                    }
                }
            }
            None => (None, None),
        };
        let kind = match a.link_mode {
            Some(LinkMode::ImportedFile | LinkMode::ImportedUrl) => "stored",
            Some(LinkMode::LinkedFile) => "linked_file",
            Some(LinkMode::LinkedUrl) => "linked_url",
            Some(LinkMode::EmbeddedImage) => "embedded_image",
            _ => match a.path.as_deref() {
                None => "linked_url",
                Some(p) if p.starts_with("storage:") => "stored",
                Some(_) => "linked_file",
            },
        };
        for (key, v) in [
            ("linkMode", json!(a.link_mode_raw)),
            ("charset", json!(a.charset)),
            ("path", json!(a.path)),
            ("syncState", json!(a.sync_state)),
            ("storageModTime", json!(a.storage_mod_time)),
            ("storageHash", json!(a.storage_hash)),
            ("lastProcessedModificationTime", json!(a.last_processed_modification_time)),
            ("lastRead", json!(a.last_read)),
        ] {
            meta.insert(key.into(), v);
        }
        if let Some(ex) = extras.get(&k) {
            meta.insert("extraFiles".into(), J::Array(ex.iter().map(|(rel, f)| json!([rel, f.sha256])).collect()));
        }
        let copied = files.get(&k);
        let record_file = |f: &Copied| -> Result<(), String> {
            c.execute(
                "INSERT OR IGNORE INTO files (sha256, size, ext, md5, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![f.sha256, f.size as i64, f.ext, f.md5, w.now],
            )
            .map_err(err)?;
            Ok(())
        };
        if let Some(f) = copied {
            record_file(f)?;
            if f.created {
                stats.files_copied += 1;
                stats.bytes_copied += f.size;
            } else {
                stats.files_reused += 1;
            }
        }
        for (_, f) in extras.get(&k).into_iter().flatten() {
            record_file(f)?;
            if f.created {
                stats.files_copied += 1;
                stats.bytes_copied += f.size;
            } else {
                stats.files_reused += 1;
            }
        }
        let issue = file_issue(a, &titles);
        let status = if copied.is_some() || issue.is_none() { "ok" } else { "missing" };
        if let Some(i) = issue {
            stats.missing_files += 1;
            report.missing_files.push(i);
        }
        if a.file.as_ref().and_then(|f| f.md5_matches_storage_hash) == Some(false) {
            report.file_warnings.push(FileIssue {
                key: Some(a.base.key.clone()),
                title: titles.get(lid, Some(&a.base.key)),
                parent_title: titles.get(lid, a.parent_key.as_ref()),
                reason: "文件与 Zotero 记录的校验值不同（可能在本机改过，例如写入了批注），已按现在的内容导入".into(),
                in_trash: a.base.in_trash,
            });
        }
        let original_path = match a.file.as_ref().map(|f| f.kind) {
            Some(FileKind::Linked | FileKind::BaseRelative) => a.path.clone(),
            _ => None,
        };
        c.execute(
            "INSERT INTO attachments (uid, item_id, note_id, kind, sha256, status, title, original_name, original_path, url, content_type, note, source_key,
                                      created_at, updated_at, deleted_at, source_meta, import_run_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                att_uid[&k],
                item_id,
                note_id,
                kind,
                copied.map(|f| &f.sha256),
                status,
                a.base.fields.get("title"),
                original_name(a.path.as_deref()),
                original_path,
                a.base.fields.get("url"),
                a.content_type,
                a.note,
                a.base.key,
                iso(&a.base.date_added),
                iso(&a.base.date_modified),
                a.base.deleted_at.as_deref().map(iso),
                J::Object(meta).to_string(),
                w.run_id
            ],
        )
        .map_err(err)?;
        let id = c.last_insert_rowid();
        att_ids.insert(k, id);
        tag_all(&mut w, &a.base, "attachment_tags", "attachment_id", id, &mut report)?;
        member(&a.base, "collection_attachments", "attachment_id", id, &mut skip, &mut report)?;
        stats.attachments += 1;
        stats.trash += a.base.deleted_at.is_some() as usize;
    }

    // ---- 标注 ----
    for an in &lib.annotations {
        let lid = an.base.library_id;
        let Some(att) = an.parent_key.as_ref().and_then(|pk| att_ids.get(&(lid, pk.clone()))) else {
            skip.annotations.insert(an.base.key.clone());
            stats.skipped += 1;
            report.skipped.push(format!("标注 {} 找不到所属附件，没有导入", an.base.key));
            continue;
        };
        let mut meta = base_meta(&an.base, false);
        meta.insert("annotationType".into(), json!(an.annotation_type_raw));
        if an.sort_index.is_none() {
            meta.insert("sortIndexMissing".into(), json!(true));
        }
        if an.position.is_none() {
            meta.insert("positionMissing".into(), json!(true));
        }
        let position = an.position.clone().unwrap_or_else(|| "{}".into());
        let page_index = serde_json::from_str::<J>(&position)
            .ok()
            .and_then(|p| p.get("pageIndex").and_then(J::as_i64))
            .or_else(|| an.sort_index.as_deref().and_then(|s| s.split('|').next()).and_then(|p| p.parse().ok()))
            .unwrap_or(0);
        c.execute(
            "INSERT INTO annotations (uid, attachment_id, kind, page_index, page_label, position, sort_key, text, comment, color, source_key,
                                      created_at, updated_at, deleted_at, author_name, is_external, source_meta, import_run_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                new_uid()?,
                att,
                an.annotation_type,
                page_index,
                an.page_label,
                position,
                an.sort_index.clone().unwrap_or_default(),
                an.text,
                an.comment,
                an.color,
                an.base.key,
                iso(&an.base.date_added),
                iso(&an.base.date_modified),
                an.base.deleted_at.as_deref().map(iso),
                an.author_name,
                an.is_external as i64,
                J::Object(meta).to_string(),
                w.run_id
            ],
        )
        .map_err(err)?;
        let id = c.last_insert_rowid();
        tag_all(&mut w, &an.base, "annotation_tags", "annotation_id", id, &mut report)?;
        stats.annotations += 1;
    }
    stats.new_tags = w.new_tags;

    // ---- 写入核对 ----
    let mut source = model_lines(lib, files, extras, &skip);
    let mut stored = stored_lines(c, root, w.run_id)?;
    for (cat, label) in CATEGORIES {
        report.checks.push(compare(
            cat,
            label,
            source.remove(cat).unwrap_or_default(),
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

    report.not_migrated = not_migrated(audited);
    report.warnings.sort();
    report.warnings.dedup();
    w.finish(&stats, &report)?;
    runs_get(c, w.run_id)
}

/// 核对时有意排除的（报告里单独列出）
#[derive(Default)]
struct Skip {
    annotations: HashSet<String>,
    /// (记录 key, 集合 key)
    memberships: HashSet<(String, String)>,
}

const CATEGORIES: &[(&str, &str)] = &[
    ("records", "条目（全部类型）"),
    ("fields", "字段值"),
    ("creators", "作者 / 编者"),
    ("tags", "标签关联"),
    ("collections", "集合"),
    ("collection_members", "集合成员"),
    ("item_relations", "条目关联"),
    ("collection_relations", "集合关联"),
    ("attachments", "附件记录"),
    ("files", "附件文件"),
    ("notes", "笔记"),
    ("annotations", "PDF 标注"),
    ("deleted_items", "回收站条目"),
    ("deleted_collections", "回收站集合"),
];

fn s(v: impl ToString) -> J {
    J::String(v.to_string())
}

fn os(v: Option<&str>) -> J {
    v.map(s).unwrap_or(J::Null)
}

fn oi(v: Option<i64>) -> J {
    v.map(s).unwrap_or(J::Null)
}

fn b01(v: bool) -> J {
    s(if v { 1 } else { 0 })
}

/// meta 里的值 → 与模型侧同样的写法（字符串原样、数字转字符串、布尔转 0 / 1）
fn mv(v: Option<&J>) -> J {
    match v {
        None | Some(J::Null) => J::Null,
        Some(J::String(x)) => J::String(x.clone()),
        Some(J::Bool(x)) => b01(*x),
        Some(other) => s(other),
    }
}

fn line(v: Vec<J>) -> String {
    J::Array(v).to_string()
}

type Lines = HashMap<&'static str, Vec<String>>;

/// 读出的 Zotero 数据 → 规范化记录行
fn model_lines(lib: &ZoteroLibrary, files: &HashMap<Key, Copied>, extras: &HashMap<Key, Vec<(String, Copied)>>, skip: &Skip) -> Lines {
    let mut m: Lines = HashMap::new();
    let mut push = |cat: &'static str, v: Vec<J>| m.entry(cat).or_default().push(line(v));
    for b in lib.all_bases().filter(|b| !(b.item_type == "annotation" && skip.annotations.contains(&b.key))) {
        let k = s(&b.key);
        push(
            "records",
            vec![
                k.clone(),
                s(&b.item_type),
                s(b.library_id),
                s(&b.date_added),
                s(&b.date_modified),
                s(&b.client_date_modified),
                s(b.version),
                b01(b.synced),
            ],
        );
        for (f, v) in &b.fields {
            push("fields", vec![k.clone(), s(f), s(v)]);
        }
        for c in &b.creators {
            let mode = if c.field_mode == 1 { 1 } else { 0 };
            push(
                "creators",
                vec![
                    k.clone(),
                    s(&c.creator_type),
                    s(c.first_name.as_deref().unwrap_or("")),
                    s(c.last_name.as_deref().unwrap_or("")),
                    s(mode),
                    s(c.order_index),
                ],
            );
        }
        for t in &b.tags {
            push("tags", vec![k.clone(), s(&t.name), s(if t.tag_type == 1 { 1 } else { 0 })]);
        }
        for c in b.collection_keys.iter().filter(|c| !skip.memberships.contains(&(b.key.clone(), (*c).clone()))) {
            push("collection_members", vec![k.clone(), s(c)]);
        }
        for r in &b.relations {
            push("item_relations", vec![k.clone(), s(&r.predicate), s(&r.object)]);
        }
        if let Some(d) = &b.deleted_at {
            push("deleted_items", vec![k.clone(), s(d)]);
        }
    }
    for c in &lib.collections {
        push(
            "collections",
            vec![
                s(&c.key),
                s(c.library_id),
                s(&c.name),
                os(c.parent_key.as_deref()),
                s(&c.client_date_modified),
                s(c.version),
                b01(c.synced),
            ],
        );
        for r in &c.relations {
            push("collection_relations", vec![s(&c.key), s(&r.predicate), s(&r.object)]);
        }
        if let Some(d) = &c.deleted_at {
            push("deleted_collections", vec![s(&c.key), s(d)]);
        }
    }
    for a in &lib.attachments {
        let k = key_of(&a.base);
        let sha = a.file.as_ref().filter(|f| f.exists).and_then(|f| f.sha256.as_deref());
        push(
            "attachments",
            vec![
                s(&a.base.key),
                os(a.parent_key.as_deref()),
                oi(a.link_mode_raw),
                os(a.content_type.as_deref()),
                os(a.charset.as_deref()),
                os(a.path.as_deref()),
                oi(a.sync_state),
                oi(a.storage_mod_time),
                os(a.storage_hash.as_deref()),
                oi(a.last_processed_modification_time),
                oi(a.last_read),
                os(a.note.as_deref()),
                os(sha),
            ],
        );
        if let Some(f) = files.get(&k) {
            push("files", vec![s(&a.base.key), s(""), s(&f.sha256), s(f.size)]);
        }
        for (rel, f) in extras.get(&k).into_iter().flatten() {
            push("files", vec![s(&a.base.key), s(rel), s(&f.sha256), s(f.size)]);
        }
    }
    for n in &lib.notes {
        push(
            "notes",
            vec![s(&n.base.key), os(n.parent_key.as_deref()), os(n.title.as_deref()), os(n.html.as_deref())],
        );
    }
    for a in lib.annotations.iter().filter(|a| !skip.annotations.contains(&a.base.key)) {
        push(
            "annotations",
            vec![
                s(&a.base.key),
                os(a.parent_key.as_deref()),
                s(a.annotation_type_raw),
                os(a.author_name.as_deref()),
                os(a.text.as_deref()),
                os(a.comment.as_deref()),
                os(a.color.as_deref()),
                os(a.page_label.as_deref()),
                os(a.sort_index.as_deref()),
                os(a.position.as_deref()),
                b01(a.is_external),
            ],
        );
    }
    m
}

fn meta_of(raw: Option<String>) -> Map<String, J> {
    raw.and_then(|r| serde_json::from_str::<J>(&r).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// 库里这次导入的记录 → 与 `model_lines` 同样格式的记录行（只读列与 source_meta，不看导入时的中间变量）
fn stored_lines(c: &Connection, root: &Path, run: i64) -> Result<Lines, String> {
    let mut m: Lines = HashMap::new();
    let push = |m: &mut Lines, cat: &'static str, v: Vec<J>| m.entry(cat).or_default().push(line(v));
    let rows = |sql: &str, n: usize| -> Result<Vec<Vec<Option<String>>>, String> {
        let mut st = c.prepare(sql).map_err(err)?;
        st.query_map([run], |r| {
            (0..n)
                .map(|i| Ok(util::value_to_string(r.get::<_, rusqlite::types::Value>(i)?)))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)
    };
    let txt = |v: &Option<String>| os(v.as_deref());

    // id → key（父子关系换回 Zotero key）
    let mut item_key: HashMap<String, String> = HashMap::new();
    let mut note_key: HashMap<String, String> = HashMap::new();
    let mut att_key: HashMap<String, String> = HashMap::new();
    let mut coll_key: HashMap<String, String> = HashMap::new();

    let items = rows(
        "SELECT id, source_key, item_type, source_library, created_at, updated_at, deleted_at, source_meta FROM items WHERE import_run_id = ?1",
        8,
    )?;
    for r in &items {
        item_key.insert(r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default());
    }
    let collections = rows(
        "SELECT id, source_key, name, parent_id, created_at, deleted_at, source_meta FROM collections WHERE import_run_id = ?1",
        7,
    )?;
    for r in &collections {
        coll_key.insert(r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default());
    }
    let notes = rows(
        "SELECT id, source_key, item_id, title, html, created_at, updated_at, deleted_at, source_meta FROM notes WHERE import_run_id = ?1",
        9,
    )?;
    for r in &notes {
        note_key.insert(r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default());
    }
    let atts = rows(
        "SELECT a.id, a.source_key, a.item_id, a.note_id, a.content_type, a.note, a.sha256, a.created_at, a.updated_at, a.deleted_at, a.source_meta, a.uid,
                f.size, f.ext
           FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 WHERE a.import_run_id = ?1",
        14,
    )?;
    let mut uid_key: HashMap<String, String> = HashMap::new();
    for r in &atts {
        att_key.insert(r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default());
        uid_key.insert(r[11].clone().unwrap_or_default(), r[1].clone().unwrap_or_default());
    }
    let anns = rows(
        "SELECT id, source_key, attachment_id, page_label, position, sort_key, text, comment, color, author_name, is_external, created_at, updated_at, deleted_at, source_meta
           FROM annotations WHERE import_run_id = ?1",
        15,
    )?;
    let lookup = |map: &HashMap<String, String>, id: &Option<String>| id.as_ref().and_then(|i| map.get(i)).cloned();

    // 所有类型共有的部分
    #[allow(clippy::too_many_arguments)]
    let common = |m: &mut Lines,
                  key: &Option<String>,
                  ty: &str,
                  lib: J,
                  created: &Option<String>,
                  updated: &Option<String>,
                  deleted: &Option<String>,
                  meta: &Map<String, J>,
                  regular: bool| {
        let k = txt(key);
        push(
            m,
            "records",
            vec![
                k.clone(),
                s(ty),
                lib,
                s(back(created.as_deref().unwrap_or(""))),
                s(back(updated.as_deref().unwrap_or(""))),
                mv(meta.get("clientDateModified")),
                mv(meta.get("version")),
                mv(meta.get("synced")),
            ],
        );
        if let Some(d) = deleted {
            push(m, "deleted_items", vec![k.clone(), s(back(d))]);
        }
        if !regular {
            for (f, v) in meta.get("fields").and_then(J::as_object).into_iter().flatten() {
                push(m, "fields", vec![k.clone(), s(f), mv(Some(v))]);
            }
            for r in meta.get("relations").and_then(J::as_array).into_iter().flatten() {
                push(m, "item_relations", vec![k.clone(), mv(r.get(0)), mv(r.get(1))]);
            }
            for cr in meta.get("creators").and_then(J::as_array).into_iter().flatten() {
                let name = |i: usize| s(cr.get(i).and_then(J::as_str).unwrap_or(""));
                let mode = if cr.get(3).and_then(J::as_i64) == Some(1) { 1 } else { 0 };
                push(m, "creators", vec![k.clone(), mv(cr.get(0)), name(1), name(2), s(mode), mv(cr.get(4))]);
            }
        }
    };

    for r in &items {
        let meta = meta_of(r[7].clone());
        common(&mut m, &r[1], r[2].as_deref().unwrap_or(""), txt(&r[3]), &r[4], &r[5], &r[6], &meta, true);
    }
    for r in &notes {
        let meta = meta_of(r[8].clone());
        common(&mut m, &r[1], "note", mv(meta.get("libraryID")), &r[5], &r[6], &r[7], &meta, false);
        let parent = lookup(&item_key, &r[2]).or_else(|| meta.get("orphanParentKey").and_then(J::as_str).map(String::from));
        let html = if meta.get("htmlMissing") == Some(&J::Bool(true)) {
            J::Null
        } else {
            s(rewrite_keys(r[4].as_deref().unwrap_or(""), |u| uid_key.get(u).cloned()))
        };
        push(&mut m, "notes", vec![txt(&r[1]), os(parent.as_deref()), txt(&r[3]), html]);
    }
    for r in &atts {
        let meta = meta_of(r[10].clone());
        common(&mut m, &r[1], "attachment", mv(meta.get("libraryID")), &r[7], &r[8], &r[9], &meta, false);
        let parent = lookup(&item_key, &r[2])
            .or_else(|| lookup(&note_key, &r[3]))
            .or_else(|| meta.get("orphanParentKey").and_then(J::as_str).map(String::from));
        push(
            &mut m,
            "attachments",
            vec![
                txt(&r[1]),
                os(parent.as_deref()),
                mv(meta.get("linkMode")),
                txt(&r[4]),
                mv(meta.get("charset")),
                mv(meta.get("path")),
                mv(meta.get("syncState")),
                mv(meta.get("storageModTime")),
                mv(meta.get("storageHash")),
                mv(meta.get("lastProcessedModificationTime")),
                mv(meta.get("lastRead")),
                txt(&r[5]),
                txt(&r[6]),
            ],
        );
        // 文件：看文件库里实际的文件大小
        let on_disk = |sha: &str, ext: &str| -> J {
            match std::fs::metadata(root.join(store::rel_path(sha, ext))) {
                Ok(md) => s(md.len()),
                Err(_) => s("文件库里没有"),
            }
        };
        let ext_of = |sha: &str| -> Option<String> { c.query_row("SELECT ext FROM files WHERE sha256 = ?1", [sha], |x| x.get(0)).ok() };
        if let (Some(sha), Some(ext)) = (&r[6], &r[13]) {
            push(&mut m, "files", vec![txt(&r[1]), s(""), s(sha), on_disk(sha, ext)]);
        }
        for ex in meta.get("extraFiles").and_then(J::as_array).into_iter().flatten() {
            let sha = ex.get(1).and_then(J::as_str).unwrap_or("");
            let size = ext_of(sha).map(|e| on_disk(sha, &e)).unwrap_or_else(|| s("没有文件记录"));
            push(&mut m, "files", vec![txt(&r[1]), mv(ex.get(0)), s(sha), size]);
        }
    }
    for r in &anns {
        let meta = meta_of(r[14].clone());
        common(&mut m, &r[1], "annotation", mv(meta.get("libraryID")), &r[11], &r[12], &r[13], &meta, false);
        let flag = |k: &str| meta.get(k) == Some(&J::Bool(true));
        push(
            &mut m,
            "annotations",
            vec![
                txt(&r[1]),
                os(lookup(&att_key, &r[2]).as_deref()),
                mv(meta.get("annotationType")),
                txt(&r[9]),
                txt(&r[6]),
                txt(&r[7]),
                txt(&r[8]),
                txt(&r[3]),
                if flag("sortIndexMissing") { J::Null } else { txt(&r[5]) },
                if flag("positionMissing") { J::Null } else { txt(&r[4]) },
                s(r[10].as_deref().unwrap_or("0")),
            ],
        );
    }
    for r in &collections {
        let meta = meta_of(r[6].clone());
        let parent = lookup(&coll_key, &r[3]).or_else(|| meta.get("orphanParentKey").and_then(J::as_str).map(String::from));
        push(
            &mut m,
            "collections",
            vec![
                txt(&r[1]),
                mv(meta.get("libraryID")),
                txt(&r[2]),
                os(parent.as_deref()),
                s(back(r[4].as_deref().unwrap_or(""))),
                mv(meta.get("version")),
                mv(meta.get("synced")),
            ],
        );
        for rel in meta.get("relations").and_then(J::as_array).into_iter().flatten() {
            push(&mut m, "collection_relations", vec![txt(&r[1]), mv(rel.get(0)), mv(rel.get(1))]);
        }
        if let Some(d) = &r[5] {
            push(&mut m, "deleted_collections", vec![txt(&r[1]), s(back(d))]);
        }
    }

    // 关联表：字段、作者、标签、集合成员、条目关联
    for r in rows(
        "SELECT i.source_key, f.field, f.value FROM item_fields f JOIN items i ON i.id = f.item_id WHERE i.import_run_id = ?1",
        3,
    )? {
        push(&mut m, "fields", vec![txt(&r[0]), txt(&r[1]), txt(&r[2])]);
    }
    for r in rows(
        "SELECT i.source_key, c.role, c.given, c.family, c.literal, c.position FROM creators c JOIN items i ON i.id = c.item_id WHERE i.import_run_id = ?1",
        6,
    )? {
        let (mode, last) = match &r[4] {
            Some(lit) => (1, lit.clone()),
            None => (0, r[3].clone().unwrap_or_default()),
        };
        push(
            &mut m,
            "creators",
            vec![txt(&r[0]), txt(&r[1]), s(r[2].as_deref().unwrap_or("")), s(last), s(mode), txt(&r[5])],
        );
    }
    for r in rows(
        "SELECT i.source_key, i.predicate, i.object FROM (SELECT x.source_key, r.predicate, r.object, x.import_run_id FROM item_relations r JOIN items x ON x.id = r.item_id) i WHERE i.import_run_id = ?1",
        3,
    )? {
        push(&mut m, "item_relations", vec![txt(&r[0]), txt(&r[1]), txt(&r[2])]);
    }
    for (table, col, owner) in [
        ("item_tags", "item_id", "items"),
        ("note_tags", "note_id", "notes"),
        ("attachment_tags", "attachment_id", "attachments"),
        ("annotation_tags", "annotation_id", "annotations"),
    ] {
        let sql = format!(
            "SELECT o.source_key, t.name, x.kind FROM {table} x JOIN tags t ON t.id = x.tag_id JOIN {owner} o ON o.id = x.{col} WHERE o.import_run_id = ?1"
        );
        for r in rows(&sql, 3)? {
            push(
                &mut m,
                "tags",
                vec![txt(&r[0]), txt(&r[1]), s(if r[2].as_deref() == Some("automatic") { 1 } else { 0 })],
            );
        }
    }
    for (table, col, owner) in [
        ("collection_items", "item_id", "items"),
        ("collection_notes", "note_id", "notes"),
        ("collection_attachments", "attachment_id", "attachments"),
    ] {
        let sql = format!(
            "SELECT o.source_key, c.source_key FROM {table} x JOIN collections c ON c.id = x.collection_id JOIN {owner} o ON o.id = x.{col} WHERE o.import_run_id = ?1"
        );
        for r in rows(&sql, 2)? {
            push(&mut m, "collection_members", vec![txt(&r[0]), txt(&r[1])]);
        }
    }
    Ok(m)
}

/// 来源里有、按设计不迁移的数据（只列非零的）。
fn not_migrated(a: &audit::Report) -> Vec<Count> {
    let label = |t: &str| match t {
        "fulltextItems" => Some("Zotero 全文索引（可从 PDF 重建）"),
        "savedSearches" => Some("保存的搜索"),
        "feeds" => Some("RSS 订阅源"),
        "items(feed 库)" => Some("RSS 订阅里的条目"),
        "syncedSettings" => Some("同步设置（含阅读位置）"),
        "publicationsItems" => Some("「我的出版物」标记"),
        "retractedItems" => Some("撤稿提示"),
        "customItemTypes" => Some("自定义条目类型"),
        "customFields" => Some("自定义字段"),
        _ => None,
    };
    let mut out: Vec<Count> = a
        .not_migrated
        .iter()
        .filter_map(|(t, n)| {
            Some(Count {
                label: label(t)?.into(),
                count: (*n)?,
            })
        })
        .filter(|c| c.count > 0)
        .collect();
    let cache: usize = a.files.cache_files.values().sum();
    if cache > 0 {
        out.push(Count {
            label: "附件目录里的 Zotero 缓存文件（全文缓存、阅读状态）".into(),
            count: cache as i64,
        });
    }
    if !a.storage_orphan_dirs.is_empty() {
        out.push(Count {
            label: "storage 里没有对应附件记录的目录".into(),
            count: a.storage_orphan_dirs.len() as i64,
        });
    }
    out
}

#[cfg(test)]
mod tests;
