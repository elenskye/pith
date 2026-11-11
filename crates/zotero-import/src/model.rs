//! 中间模型：尽量贴近 Zotero 原始数据，按名称而不是数据库内部 ID 组织。
//! 目标是「无损」：Zotero 数据库里每一条会被迁移的信息都能在这里找到对应位置，
//! 由 `audit` 模块逐类核对。v0.1 的 `library.db` 再从这里映射。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 模型格式版本；字段语义变化时加一。
pub const MODEL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoteroLibrary {
    pub model_version: u32,
    pub source: SourceInfo,
    pub libraries: Vec<LibraryInfo>,
    pub collections: Vec<Collection>,
    /// 普通条目（期刊文章、书、章节……），不含附件 / 笔记 / 标注。
    pub items: Vec<ItemBase>,
    pub attachments: Vec<Attachment>,
    pub notes: Vec<Note>,
    pub annotations: Vec<Annotation>,
    /// 读取时发现的不规范数据，不会中断读取。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub db_file: String,
    pub storage_dir: String,
    /// `settings` 表里的 client.lastVersion，例如 "9.0.3"。
    pub client_version: Option<String>,
    /// `version` 表：schema 名 → 版本号（userdata、system、globalSchema……）。
    pub schema_versions: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub library_id: i64,
    /// "user" 或 "group"（feed 库不读取）。
    pub kind: String,
    pub group_name: Option<String>,
    pub editable: bool,
}

/// 所有类型条目（普通条目、附件、笔记、标注）共有的部分。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemBase {
    pub item_id: i64,
    pub key: String,
    pub library_id: i64,
    pub item_type: String,
    pub date_added: String,
    pub date_modified: String,
    pub client_date_modified: String,
    pub version: i64,
    pub synced: bool,
    /// 本条目自己在回收站（`deletedItems`）里时的删除时间。
    pub deleted_at: Option<String>,
    /// 本条目或任一上级（父条目 / 父附件）在回收站里。
    pub in_trash: bool,
    /// 字段名 → 原始值（日期保持 Zotero 的 "YYYY-MM-DD 原文" 格式）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creators: Vec<Creator>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collection_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Creator {
    pub creator_type: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// 0 = 姓 / 名两栏，1 = 单栏（机构名或不拆分的姓名，常见于中文）。
    pub field_mode: i64,
    pub order_index: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    /// 0 = 手动，1 = 自动（导入时由元数据带来）。
    pub tag_type: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// 例如 "dc:relation"（相关条目）、"dc:replaces"（合并重复项）、"owl:sameAs"。
    pub predicate: String,
    /// 通常是 Zotero URI，例如 http://zotero.org/users/local/abcd/items/KEY。
    pub object: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkMode {
    ImportedFile,
    ImportedUrl,
    LinkedFile,
    LinkedUrl,
    EmbeddedImage,
    Unknown,
}

impl LinkMode {
    pub fn from_raw(v: i64) -> Self {
        match v {
            0 => Self::ImportedFile,
            1 => Self::ImportedUrl,
            2 => Self::LinkedFile,
            3 => Self::LinkedUrl,
            4 => Self::EmbeddedImage,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(flatten)]
    pub base: ItemBase,
    pub parent_key: Option<String>,
    pub link_mode: Option<LinkMode>,
    pub link_mode_raw: Option<i64>,
    pub content_type: Option<String>,
    pub charset: Option<String>,
    /// 原始 path 列："storage:文件名"、绝对路径或 "attachments:相对路径"。
    pub path: Option<String>,
    pub sync_state: Option<i64>,
    pub storage_mod_time: Option<i64>,
    /// Zotero 记录的文件 MD5（同步用）。
    pub storage_hash: Option<String>,
    pub last_processed_modification_time: Option<i64>,
    pub last_read: Option<i64>,
    /// 旧版 Zotero 允许附件自带一段笔记（存在 itemNotes 里）。
    pub note: Option<String>,
    pub file: Option<FileInfo>,
    /// 附件目录里主文件以外的文件（网页快照的资源等），相对附件目录。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// 存在 Zotero storage/<KEY>/ 里。
    Stored,
    /// 链接到 Zotero 之外的绝对路径。
    Linked,
    /// 相对「链接附件根目录」（Zotero 首选项，不在数据库里）。
    BaseRelative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInfo {
    pub kind: FileKind,
    /// 解析出的绝对路径；BaseRelative 且未提供根目录时为空。
    pub resolved_path: Option<String>,
    /// Stored：相对 storage/ 的路径 "KEY/文件名"；BaseRelative：相对根目录的路径。
    pub relative_path: Option<String>,
    pub exists: bool,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub md5: Option<String>,
    /// 与 storage_hash 比较的结果；没有 storage_hash 或文件不存在时为空。
    pub md5_matches_storage_hash: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    #[serde(flatten)]
    pub base: ItemBase,
    pub parent_key: Option<String>,
    pub title: Option<String>,
    /// Zotero 笔记是 HTML。
    pub html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(flatten)]
    pub base: ItemBase,
    /// 所属附件（PDF / EPUB / 快照）的 key。
    pub parent_key: Option<String>,
    /// highlight / note / image / ink / underline / text / unknown。
    pub annotation_type: String,
    pub annotation_type_raw: i64,
    pub author_name: Option<String>,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub color: Option<String>,
    pub page_label: Option<String>,
    pub sort_index: Option<String>,
    /// 原始 JSON 字符串（pageIndex + rects / paths），保持不变以免精度损失。
    pub position: Option<String>,
    pub is_external: bool,
}

pub fn annotation_type_name(v: i64) -> &'static str {
    match v {
        1 => "highlight",
        2 => "note",
        3 => "image",
        4 => "ink",
        5 => "underline",
        6 => "text",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub collection_id: i64,
    pub key: String,
    pub library_id: i64,
    pub name: String,
    pub parent_key: Option<String>,
    pub client_date_modified: String,
    pub version: i64,
    pub synced: bool,
    pub deleted_at: Option<String>,
    /// 本集合或任一上级集合在回收站里。
    pub in_trash: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
}

impl ZoteroLibrary {
    /// 依次遍历所有类型条目的公共部分。
    pub fn all_bases(&self) -> impl Iterator<Item = &ItemBase> {
        self.items
            .iter()
            .chain(self.attachments.iter().map(|a| &a.base))
            .chain(self.notes.iter().map(|n| &n.base))
            .chain(self.annotations.iter().map(|a| &a.base))
    }
}
