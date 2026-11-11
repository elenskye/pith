//! 合成 Zotero 数据目录，**不含任何真实数据**。
//!
//! 表结构按 Zotero 9.0.3（userdata schema 125）实际列名手写（只含本原型读取或统计的表），
//! 列名与真实库的一致性由 `tests/real_copy.rs` 的忽略测试核对。
//! 内容故意覆盖边界情况，每一种都在 `Expected` 里写明，供测试断言。

use crate::util::md5_hex;
use crate::{Error, Result};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DDL: &str = r#"
CREATE TABLE version (schema TEXT PRIMARY KEY, version INT NOT NULL);
CREATE TABLE settings (setting TEXT, key TEXT, value, PRIMARY KEY (setting, key));
CREATE TABLE libraries (libraryID INTEGER PRIMARY KEY, type TEXT NOT NULL, editable INT NOT NULL, filesEditable INT NOT NULL, version INT NOT NULL DEFAULT 0, storageVersion INT NOT NULL DEFAULT 0, lastSync INT NOT NULL DEFAULT 0, archived INT NOT NULL DEFAULT 0, isAdmin INT NOT NULL DEFAULT 0);
CREATE TABLE groups (groupID INTEGER PRIMARY KEY, libraryID INT NOT NULL UNIQUE, name TEXT NOT NULL, description TEXT NOT NULL, version INT NOT NULL);
CREATE TABLE feeds (libraryID INTEGER PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL UNIQUE, lastUpdate TIMESTAMP, lastCheck TIMESTAMP, lastCheckError TEXT, cleanupReadAfter INT, cleanupUnreadAfter INT, refreshInterval INT);
CREATE TABLE feedItems (itemID INTEGER PRIMARY KEY, guid TEXT NOT NULL UNIQUE, readTime TIMESTAMP, translatedTime TIMESTAMP);
CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT, templateItemTypeID INT, display INT DEFAULT 1);
CREATE TABLE itemTypesCombined (itemTypeID INT NOT NULL, typeName TEXT NOT NULL, display INT DEFAULT 1 NOT NULL, custom INT NOT NULL, PRIMARY KEY (itemTypeID));
CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT, fieldFormatID INT);
CREATE TABLE fieldsCombined (fieldID INT NOT NULL, fieldName TEXT NOT NULL, label TEXT, fieldFormatID INT, custom INT NOT NULL, PRIMARY KEY (fieldID));
CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
CREATE TABLE charsets (charsetID INTEGER PRIMARY KEY, charset TEXT UNIQUE);
CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INT NOT NULL, dateAdded TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, dateModified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, clientDateModified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, libraryID INT NOT NULL, key TEXT NOT NULL, version INT NOT NULL DEFAULT 0, synced INT NOT NULL DEFAULT 0, UNIQUE (libraryID, key));
CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value UNIQUE);
CREATE TABLE itemData (itemID INT, fieldID INT, valueID, PRIMARY KEY (itemID, fieldID));
CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT, fieldMode INT, UNIQUE (lastName, firstName, fieldMode));
CREATE TABLE itemCreators (itemID INT NOT NULL, creatorID INT NOT NULL, creatorTypeID INT NOT NULL DEFAULT 1, orderIndex INT NOT NULL DEFAULT 0, PRIMARY KEY (itemID, creatorID, creatorTypeID, orderIndex), UNIQUE (itemID, orderIndex));
CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT NOT NULL, parentCollectionID INT DEFAULT NULL, clientDateModified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, libraryID INT NOT NULL, key TEXT NOT NULL, version INT NOT NULL DEFAULT 0, synced INT NOT NULL DEFAULT 0, UNIQUE (libraryID, key));
CREATE TABLE collectionItems (collectionID INT NOT NULL, itemID INT NOT NULL, orderIndex INT NOT NULL DEFAULT 0, PRIMARY KEY (collectionID, itemID));
CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE itemTags (itemID INT NOT NULL, tagID INT NOT NULL, type INT NOT NULL, PRIMARY KEY (itemID, tagID));
CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INT, linkMode INT, contentType TEXT, charsetID INT, path TEXT, syncState INT DEFAULT 0, storageModTime INT, storageHash TEXT, lastProcessedModificationTime INT, lastRead INT);
CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INT, note TEXT, title TEXT);
CREATE TABLE itemAnnotations (itemID INTEGER PRIMARY KEY, parentItemID INT NOT NULL, type INTEGER NOT NULL, authorName TEXT, text TEXT, comment TEXT, color TEXT, pageLabel TEXT, sortIndex TEXT NOT NULL, position TEXT NOT NULL, isExternal INT NOT NULL);
CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY, dateDeleted DEFAULT CURRENT_TIMESTAMP NOT NULL);
CREATE TABLE deletedCollections (collectionID INTEGER PRIMARY KEY, dateDeleted DEFAULT CURRENT_TIMESTAMP NOT NULL);
CREATE TABLE relationPredicates (predicateID INTEGER PRIMARY KEY, predicate TEXT UNIQUE);
CREATE TABLE itemRelations (itemID INT NOT NULL, predicateID INT NOT NULL, object TEXT NOT NULL, PRIMARY KEY (itemID, predicateID, object));
CREATE TABLE collectionRelations (collectionID INT NOT NULL, predicateID INT NOT NULL, object TEXT NOT NULL, PRIMARY KEY (collectionID, predicateID, object));
CREATE TABLE fulltextItems (itemID INTEGER PRIMARY KEY, indexedPages INT, totalPages INT, indexedChars INT, totalChars INT, version INT NOT NULL DEFAULT 0, synced INT NOT NULL DEFAULT 0);
CREATE TABLE savedSearches (savedSearchID INTEGER PRIMARY KEY, savedSearchName TEXT NOT NULL, clientDateModified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, libraryID INT NOT NULL, key TEXT NOT NULL, version INT NOT NULL DEFAULT 0, synced INT NOT NULL DEFAULT 0);
CREATE TABLE syncedSettings (setting TEXT NOT NULL, libraryID INT NOT NULL, value NOT NULL, version INT NOT NULL DEFAULT 0, synced INT NOT NULL DEFAULT 0, PRIMARY KEY (setting, libraryID));
"#;

/// 夹具中表的名称（列名一致性测试用）。
pub fn fixture_tables() -> Vec<String> {
    DDL.lines()
        .filter_map(|l| l.strip_prefix("CREATE TABLE "))
        .filter_map(|l| l.split_whitespace().next().map(String::from))
        .collect()
}

const ITEM_TYPES: &[&str] = &["annotation", "attachment", "book", "bookSection", "conferencePaper", "journalArticle", "note", "preprint", "report", "thesis", "webpage"];
const FIELDS: &[&str] = &[
    "title", "abstractNote", "date", "publicationTitle", "volume", "issue", "pages", "DOI", "ISSN", "ISBN", "url", "accessDate",
    "language", "publisher", "place", "bookTitle", "proceedingsTitle", "university", "thesisType", "reportNumber", "institution",
    "websiteTitle", "extra", "shortTitle", "edition", "numPages", "repository", "archiveID", "rights", "libraryCatalog", "series",
];
const CREATOR_TYPES: &[&str] = &["author", "contributor", "editor", "translator", "seriesEditor"];

/// 夹具里故意制造的情况，测试按这里断言。
pub struct Expected;
impl Expected {
    pub const REGULAR_ITEMS: usize = 21; // 用户库 20 + group 库 1；feed 库 1 条不读
    pub const ATTACHMENTS: usize = 13;
    pub const NOTES: usize = 4;
    pub const ANNOTATIONS: usize = 4;
    pub const COLLECTIONS: usize = 8;
    pub const STORED_MISSING: &'static str = "ATT00008";
    pub const MD5_MISMATCH: &'static str = "ATT00009";
    pub const LINKED_MISSING: &'static str = "ATT00004";
    pub const BASE_RELATIVE: &'static str = "ATT00007";
    pub const SNAPSHOT_WITH_EXTRA: &'static str = "ATT00002";
    pub const ATTACHMENT_WITH_NOTE: &'static str = "ATT00003";
    pub const ORPHAN_DIR: &'static str = "ORPHAN01";
    pub const MANY_AUTHORS_ITEM: &'static str = "JRNL0006";
    pub const MANY_AUTHORS: usize = 30;
    pub const ITEMS_DELETED_DIRECTLY: usize = 3; // JRNL0005, ATT00013, NOTE0004
    pub const ITEMS_TRASHED_VIA_PARENT: usize = 2; // ATT00012, NOTE0003
    pub const COLLECTIONS_DELETED_DIRECTLY: usize = 1; // COLL0006
    pub const COLLECTIONS_TRASHED_VIA_ANCESTOR: usize = 1; // COLL0007
}

pub struct FixturePaths {
    pub root: PathBuf,
    /// 相当于 ~/Zotero：内含 zotero.sqlite 与 storage/
    pub zotero_dir: PathBuf,
    /// 链接附件根目录（"attachments:" 路径相对于它）
    pub base_dir: PathBuf,
    /// 链接文件（绝对路径）所在目录
    pub linked_dir: PathBuf,
}

/// 最小但合法的单页 PDF（有正确的 xref 偏移），内容为一行 ASCII 文本。
pub fn tiny_pdf(text: &str) -> Vec<u8> {
    let esc = text.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let content = format!("BT /F1 12 Tf 72 720 Td ({esc}) Tj ET");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

pub const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63,
    0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

struct B<'a> {
    c: &'a Connection,
    next_item: i64,
    next_coll: i64,
    types: HashMap<&'static str, i64>,
    fields: HashMap<&'static str, i64>,
    ctypes: HashMap<&'static str, i64>,
    values: HashMap<String, i64>,
    creators: HashMap<(String, String, i64), i64>,
    tags: HashMap<String, i64>,
    preds: HashMap<String, i64>,
    storage: PathBuf,
}

impl B<'_> {
    fn item(&mut self, lib: i64, ty: &str, key: &str, day: u32) -> Result<i64> {
        self.next_item += 1;
        let id = self.next_item;
        let added = format!("2026-01-{day:02} 10:00:00");
        let modified = format!("2026-02-{day:02} 11:30:00");
        self.c.execute(
            "INSERT INTO items (itemID, itemTypeID, dateAdded, dateModified, clientDateModified, libraryID, key, version, synced) VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, 1)",
            params![id, self.types[ty], added, modified, lib, key, id * 3],
        )?;
        Ok(id)
    }

    fn field(&mut self, id: i64, name: &str, value: &str) -> Result<()> {
        let vid = match self.values.get(value) {
            Some(v) => *v,
            None => {
                let v = self.values.len() as i64 + 1;
                self.c.execute("INSERT INTO itemDataValues (valueID, value) VALUES (?1, ?2)", params![v, value])?;
                self.values.insert(value.to_string(), v);
                v
            }
        };
        self.c.execute("INSERT INTO itemData (itemID, fieldID, valueID) VALUES (?1, ?2, ?3)", params![id, self.fields[name], vid])?;
        Ok(())
    }

    fn fields(&mut self, id: i64, kv: &[(&str, &str)]) -> Result<()> {
        for (k, v) in kv {
            self.field(id, k, v)?;
        }
        Ok(())
    }

    fn creator(&mut self, id: i64, ctype: &str, first: &str, last: &str, mode: i64, order: i64) -> Result<()> {
        let k = (first.to_string(), last.to_string(), mode);
        let cid = match self.creators.get(&k) {
            Some(c) => *c,
            None => {
                let c = self.creators.len() as i64 + 1;
                self.c.execute("INSERT INTO creators (creatorID, firstName, lastName, fieldMode) VALUES (?1, ?2, ?3, ?4)", params![c, first, last, mode])?;
                self.creators.insert(k, c);
                c
            }
        };
        self.c.execute(
            "INSERT INTO itemCreators (itemID, creatorID, creatorTypeID, orderIndex) VALUES (?1, ?2, ?3, ?4)",
            params![id, cid, self.ctypes[ctype], order],
        )?;
        Ok(())
    }

    fn authors(&mut self, id: i64, names: &[(&str, &str)]) -> Result<()> {
        for (i, (f, l)) in names.iter().enumerate() {
            self.creator(id, "author", f, l, 0, i as i64)?;
        }
        Ok(())
    }

    fn tag(&mut self, id: i64, name: &str, ty: i64) -> Result<()> {
        let tid = match self.tags.get(name) {
            Some(t) => *t,
            None => {
                let t = self.tags.len() as i64 + 1;
                self.c.execute("INSERT INTO tags (tagID, name) VALUES (?1, ?2)", params![t, name])?;
                self.tags.insert(name.to_string(), t);
                t
            }
        };
        self.c.execute("INSERT INTO itemTags (itemID, tagID, type) VALUES (?1, ?2, ?3)", params![id, tid, ty])?;
        Ok(())
    }

    fn coll(&mut self, lib: i64, key: &str, name: &str, parent: Option<i64>) -> Result<i64> {
        self.next_coll += 1;
        let id = self.next_coll;
        self.c.execute(
            "INSERT INTO collections (collectionID, collectionName, parentCollectionID, clientDateModified, libraryID, key, version, synced) VALUES (?1, ?2, ?3, '2026-01-01 09:00:00', ?4, ?5, ?1, 1)",
            params![id, name, parent, lib, key],
        )?;
        Ok(id)
    }

    fn put(&self, cid: i64, id: i64) -> Result<()> {
        self.c.execute("INSERT INTO collectionItems (collectionID, itemID, orderIndex) VALUES (?1, ?2, 0)", params![cid, id])?;
        Ok(())
    }

    fn relate(&mut self, id: i64, pred: &str, object: &str, collection: bool) -> Result<()> {
        let pid = match self.preds.get(pred) {
            Some(p) => *p,
            None => {
                let p = self.preds.len() as i64 + 1;
                self.c.execute("INSERT INTO relationPredicates (predicateID, predicate) VALUES (?1, ?2)", params![p, pred])?;
                self.preds.insert(pred.to_string(), p);
                p
            }
        };
        let t = if collection { "collectionRelations (collectionID" } else { "itemRelations (itemID" };
        self.c.execute(&format!("INSERT INTO {t}, predicateID, object) VALUES (?1, ?2, ?3)"), params![id, pid, object])?;
        Ok(())
    }

    /// 存入 storage 的附件。`write` 为 None 表示不创建文件（模拟丢失）；`hash` 控制 storageHash。
    #[allow(clippy::too_many_arguments)]
    fn stored(&mut self, key: &str, parent: Option<i64>, mode: i64, ctype: &str, name: &str, write: Option<&[u8]>, hash: StoredHash, day: u32) -> Result<i64> {
        let id = self.item(1, "attachment", key, day)?;
        let storage_hash = match (&hash, write) {
            (StoredHash::Correct, Some(b)) => Some(md5_hex(b)),
            (StoredHash::Correct, None) => Some("0".repeat(32)),
            (StoredHash::Wrong, _) => Some("ffffffffffffffffffffffffffffffff".into()),
            (StoredHash::None, _) => None,
        };
        if let Some(bytes) = write {
            let dir = self.storage.join(key);
            std::fs::create_dir_all(&dir).map_err(|e| Error::Io(e, dir.clone()))?;
            std::fs::write(dir.join(name), bytes).map_err(|e| Error::Io(e, dir.join(name)))?;
            std::fs::write(dir.join(".zotero-ft-cache"), b"cached full text").map_err(|e| Error::Io(e, dir.clone()))?;
        }
        let charset = if ctype == "text/html" { Some(1) } else { None };
        self.c.execute(
            "INSERT INTO itemAttachments (itemID, parentItemID, linkMode, contentType, charsetID, path, syncState, storageModTime, storageHash, lastProcessedModificationTime, lastRead)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 2, 1767261600000, ?7, NULL, NULL)",
            params![id, parent, mode, ctype, charset, format!("storage:{name}"), storage_hash],
        )?;
        self.field(id, "title", if ctype == "text/html" { "Snapshot" } else { "Full Text PDF" })?;
        Ok(id)
    }

    fn linked(&mut self, key: &str, parent: Option<i64>, mode: i64, ctype: Option<&str>, path: Option<&str>, day: u32) -> Result<i64> {
        let id = self.item(1, "attachment", key, day)?;
        self.c.execute(
            "INSERT INTO itemAttachments (itemID, parentItemID, linkMode, contentType, charsetID, path, syncState) VALUES (?1, ?2, ?3, ?4, NULL, ?5, 0)",
            params![id, parent, mode, ctype, path],
        )?;
        Ok(id)
    }

    fn note(&mut self, key: &str, parent: Option<i64>, html: &str, title: &str, day: u32) -> Result<i64> {
        let id = self.item(1, "note", key, day)?;
        self.c.execute("INSERT INTO itemNotes (itemID, parentItemID, note, title) VALUES (?1, ?2, ?3, ?4)", params![id, parent, html, title])?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn annotation(&mut self, key: &str, parent: i64, ty: i64, author: Option<&str>, text: Option<&str>, comment: Option<&str>, color: &str, page: &str, sort: &str, position: &str) -> Result<i64> {
        let id = self.item(1, "annotation", key, 20)?;
        self.c.execute(
            "INSERT INTO itemAnnotations (itemID, parentItemID, type, authorName, text, comment, color, pageLabel, sortIndex, position, isExternal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            params![id, parent, ty, author, text, comment, color, page, sort, position],
        )?;
        Ok(id)
    }

    fn trash_item(&self, id: i64) -> Result<()> {
        self.c.execute("INSERT INTO deletedItems (itemID, dateDeleted) VALUES (?1, '2026-03-01 08:00:00')", [id])?;
        Ok(())
    }
}

enum StoredHash {
    Correct,
    Wrong,
    None,
}

/// 在 `root`（必须不存在或为空）下生成合成数据目录。
pub fn build(root: &Path) -> Result<FixturePaths> {
    if root.exists() && std::fs::read_dir(root).map_err(|e| Error::Io(e, root.into()))?.next().is_some() {
        return Err(Error::Schema(format!("夹具目录必须为空：{}", root.display())));
    }
    let zotero_dir = root.join("zotero");
    let storage = zotero_dir.join("storage");
    let base_dir = root.join("linked-base");
    let linked_dir = root.join("linked-abs");
    for d in [&storage, &base_dir.join("papers"), &linked_dir] {
        std::fs::create_dir_all(d).map_err(|e| Error::Io(e, d.to_path_buf()))?;
    }
    let conn = Connection::open(zotero_dir.join("zotero.sqlite"))?;
    conn.execute_batch(DDL)?;
    conn.execute_batch(
        "INSERT INTO version VALUES ('userdata', 125), ('system', 32), ('globalSchema', 42), ('triggers', 18);
         INSERT INTO settings VALUES ('client', 'lastVersion', '9.0.3'), ('client', 'lastCompatibleVersion', '8.0.1');
         INSERT INTO libraries VALUES (1, 'user', 1, 1, 100, 100, 0, 0, 0), (2, 'feed', 0, 0, 0, 0, 0, 0, 0), (3, 'group', 1, 1, 5, 5, 0, 0, 0);
         INSERT INTO groups VALUES (999, 3, '合成课题组', '', 5);
         INSERT INTO feeds (libraryID, name, url) VALUES (2, 'Example Feed', 'https://example.org/feed.xml');
         INSERT INTO charsets VALUES (1, 'utf-8'), (2, 'gbk');",
    )?;
    let mut b = B {
        c: &conn,
        next_item: 0,
        next_coll: 0,
        types: HashMap::new(),
        fields: HashMap::new(),
        ctypes: HashMap::new(),
        values: HashMap::new(),
        creators: HashMap::new(),
        tags: HashMap::new(),
        preds: HashMap::new(),
        storage: storage.clone(),
    };
    for (i, t) in ITEM_TYPES.iter().enumerate() {
        let id = i as i64 + 1;
        conn.execute("INSERT INTO itemTypes VALUES (?1, ?2, NULL, 1)", params![id, t])?;
        conn.execute("INSERT INTO itemTypesCombined VALUES (?1, ?2, 1, 0)", params![id, t])?;
        b.types.insert(t, id);
    }
    for (i, f) in FIELDS.iter().enumerate() {
        let id = i as i64 + 1;
        conn.execute("INSERT INTO fields VALUES (?1, ?2, NULL)", params![id, f])?;
        conn.execute("INSERT INTO fieldsCombined VALUES (?1, ?2, NULL, NULL, 0)", params![id, f])?;
        b.fields.insert(f, id);
    }
    for (i, t) in CREATOR_TYPES.iter().enumerate() {
        let id = i as i64 + 1;
        conn.execute("INSERT INTO creatorTypes VALUES (?1, ?2)", params![id, t])?;
        b.ctypes.insert(t, id);
    }

    // ---------- 集合 ----------
    let c_thesis = b.coll(1, "COLL0001", "毕业论文", None)?;
    let c_review = b.coll(1, "COLL0002", "文献综述", Some(c_thesis))?;
    let c_theory = b.coll(1, "COLL0003", "理论", Some(c_review))?;
    let c_method = b.coll(1, "COLL0004", "方法", Some(c_thesis))?;
    let c_book = b.coll(1, "COLL0005", "读书：国富论", None)?;
    let c_del = b.coll(1, "COLL0006", "已删除的集合", None)?;
    let c_del_child = b.coll(1, "COLL0007", "子集合（随上级删除）", Some(c_del))?;
    let c_group = b.coll(3, "COLL0008", "课题组共享", None)?;
    conn.execute("INSERT INTO deletedCollections VALUES (?1, '2026-03-02 08:00:00')", [c_del])?;
    b.relate(c_review, "dc:relation", "http://zotero.org/users/local/synthlib/collections/COLL0003", true)?;

    // ---------- 普通条目（用户库 20 条） ----------
    let j1 = b.item(1, "journalArticle", "JRNL0001", 1)?;
    b.fields(j1, &[
        ("title", "Synthetic Panel Study of Efficacy and Voting"), ("date", "2019-03-00 March 2019"),
        ("publicationTitle", "Journal of Synthetic Politics"), ("volume", "12"), ("issue", "3"), ("pages", "101-130"),
        ("DOI", "10.0000/synthetic.2019.001"), ("ISSN", "0000-0001"), ("language", "en"), ("libraryCatalog", "Synthetic DB"),
        ("abstractNote", "We examine internal and external efficacy.\nSecond paragraph with \"quotes\" and 'apostrophes'."),
        ("url", "https://example.org/articles/1"), ("accessDate", "2026-01-01 10:00:00"), ("shortTitle", "Synthetic Panel Study"),
    ])?;
    b.authors(j1, &[("Alice", "Smith"), ("Bob", "Jones")])?;
    b.tag(j1, "machine learning", 1)?;
    b.tag(j1, "政治效能感", 0)?;
    b.put(c_review, j1)?;
    b.put(c_theory, j1)?;

    let j2 = b.item(1, "journalArticle", "JRNL0002", 2)?;
    b.fields(j2, &[("title", "互联网使用与政治效能感：基于全国调查数据"), ("date", "2021-00-00 2021"), ("publicationTitle", "合成社会学研究"), ("volume", "36"), ("issue", "2"), ("pages", "45-70"), ("language", "zh-CN")])?;
    b.creator(j2, "author", "", "王小明", 1, 0)?;
    b.creator(j2, "author", "", "李华", 1, 1)?;
    b.tag(j2, "政治效能感", 0)?;
    b.put(c_review, j2)?;
    b.relate(j1, "dc:relation", "http://zotero.org/users/local/synthlib/items/JRNL0002", false)?;
    b.relate(j2, "dc:relation", "http://zotero.org/users/local/synthlib/items/JRNL0001", false)?;

    let j3 = b.item(1, "journalArticle", "JRNL0003", 3)?;
    b.fields(j3, &[("title", "Measuring External Efficacy"), ("date", "2016-10-00 10/2016"), ("publicationTitle", "Synthetic Methods Quarterly"), ("DOI", "10.0000/synthetic.2016.010")])?;
    b.authors(j3, &[("Carla", "Müller-Lüdenscheidt")])?;
    b.put(c_method, j3)?;

    let j4 = b.item(1, "journalArticle", "JRNL0004", 4)?;
    b.fields(j4, &[("title", "Survey Weights in Practice"), ("date", "2013-05-01 2013-05-01"), ("extra", "original-date: 2012\ntype: article-journal\nPMID: 00000000")])?;
    b.authors(j4, &[("D.", "O'Connor")])?;
    b.tag(j4, "to read", 0)?;
    b.tag(j4, "🔥 hot", 0)?;

    let j5 = b.item(1, "journalArticle", "JRNL0005", 5)?; // 直接删除；其附件、笔记随之进入回收站
    b.fields(j5, &[("title", "A Trashed Article"), ("date", "2010-00-00 2010")])?;
    b.authors(j5, &[("Eve", "Trash")])?;
    b.put(c_del_child, j5)?;
    b.trash_item(j5)?;

    let j6 = b.item(1, "journalArticle", "JRNL0006", 6)?;
    b.fields(j6, &[("title", "A Large Collaboration Paper"), ("date", "2024-07-15 2024-07-15")])?;
    let many: Vec<(String, String)> = (0..Expected::MANY_AUTHORS).map(|i| (format!("First{i:02}"), format!("Last{i:02}"))).collect();
    for (i, (f, l)) in many.iter().enumerate() {
        b.creator(j6, "author", f, l, 0, i as i64)?;
    }
    b.tag(j6, "machine learning", 1)?;

    let j7 = b.item(1, "journalArticle", "JRNL0007", 7)?;
    b.fields(j7, &[("title", "Item In No Collection"), ("date", "2001-00-00 2001")])?;
    b.creator(j7, "author", "", "World Values Survey Association", 1, 0)?;

    let j8 = b.item(1, "journalArticle", "JRNL0008", 8)?;
    b.fields(j8, &[("title", "Item In Three Collections"), ("date", "2022-11-00 Nov 2022")])?;
    b.authors(j8, &[("Fay", "Lin")])?;
    b.put(c_review, j8)?;
    b.put(c_method, j8)?;
    b.put(c_book, j8)?;

    let bk1 = b.item(1, "book", "BOOK0001", 9)?;
    b.fields(bk1, &[("title", "国民财富的性质和原因的研究"), ("date", "1972-00-00 1972"), ("publisher", "合成出版社"), ("place", "北京"), ("ISBN", "978-0-00-000000-1"), ("edition", "2"), ("language", "zh-CN")])?;
    b.creator(bk1, "author", "", "亚当·斯密", 1, 0)?;
    b.creator(bk1, "translator", "", "郭大力", 1, 1)?;
    b.creator(bk1, "translator", "", "王亚南", 1, 2)?;
    b.put(c_book, bk1)?;
    b.relate(bk1, "dc:replaces", "http://zotero.org/users/local/synthlib/items/OLDKEY01", false)?;

    let bk2 = b.item(1, "book", "BOOK0002", 10)?;
    b.fields(bk2, &[("title", "An Inquiry into the Nature and Causes of the Wealth of Nations"), ("date", "1776-00-00 1776"), ("publisher", "Synthetic Press"), ("place", "London")])?;
    b.authors(bk2, &[("Adam", "Smith")])?;
    b.put(c_book, bk2)?;
    // numPages 以 INTEGER 存储：检验非文本值的处理
    {
        let v = b.values.len() as i64 + 1;
        conn.execute("INSERT INTO itemDataValues (valueID, value) VALUES (?1, 352)", [v])?;
        b.values.insert("<int 352>".into(), v);
        conn.execute("INSERT INTO itemData VALUES (?1, ?2, ?3)", params![bk2, b.fields["numPages"], v])?;
    }

    let bk3 = b.item(1, "book", "BOOK0003", 11)?;
    b.fields(bk3, &[("title", "Research Design in Political Science"), ("date", "2015-00-00 2015"), ("series", "Methods Series")])?;
    b.creator(bk3, "editor", "Gina", "Park", 0, 0)?;
    b.creator(bk3, "seriesEditor", "Hal", "Kim", 0, 1)?;

    let s1 = b.item(1, "bookSection", "BSEC0001", 12)?;
    b.fields(s1, &[("title", "Chapter on Efficacy"), ("bookTitle", "Research Design in Political Science"), ("pages", "33-58"), ("date", "2015-00-00 2015")])?;
    b.authors(s1, &[("Ivy", "Chen")])?;
    b.creator(s1, "editor", "Gina", "Park", 0, 1)?;
    b.put(c_theory, s1)?;

    let s2 = b.item(1, "bookSection", "BSEC0002", 13)?;
    b.fields(s2, &[("title", "论分工"), ("bookTitle", "国民财富的性质和原因的研究"), ("pages", "5-12")])?;
    b.creator(s2, "author", "", "亚当·斯密", 1, 0)?;
    b.put(c_book, s2)?;

    let cf1 = b.item(1, "conferencePaper", "CONF0001", 14)?;
    b.fields(cf1, &[("title", "Efficacy Online"), ("proceedingsTitle", "Proceedings of the Synthetic Conference"), ("date", "2020-06-00 June 2020"), ("place", "Online")])?;
    b.authors(cf1, &[("Jon", "Doe"), ("Kai", "Roe")])?;

    let cf2 = b.item(1, "conferencePaper", "CONF0002", 15)?;
    b.fields(cf2, &[("title", "Another Conference Paper"), ("DOI", "10.0000/synthetic.conf.2")])?;

    let th = b.item(1, "thesis", "THES0001", 16)?;
    b.fields(th, &[("title", "大学生政治效能感的影响因素研究"), ("university", "合成大学"), ("thesisType", "硕士学位论文"), ("date", "2025-06-00 2025年6月"), ("place", "上海")])?;
    b.creator(th, "author", "", "林小柚", 1, 0)?;
    b.creator(th, "contributor", "", "张导师", 1, 1)?;
    b.put(c_thesis, th)?;
    b.tag(th, "政治效能感", 0)?;

    let rp = b.item(1, "report", "REPT0001", 17)?;
    b.fields(rp, &[("title", "Synthetic Employment Statistics Report"), ("institution", "Synthetic Bureau of Statistics"), ("reportNumber", "SBS-2025-07"), ("date", "2025-07-00 2025-07")])?;

    let w1 = b.item(1, "webpage", "WEBP0001", 18)?;
    b.fields(w1, &[("title", "Synthetic Policy Page"), ("websiteTitle", "Example Gov"), ("url", "https://example.org/policy"), ("accessDate", "2026-01-18 12:00:00")])?;

    let w2 = b.item(1, "webpage", "WEBP0002", 19)?;
    b.fields(w2, &[("title", "Webpage Without Date"), ("url", "https://example.org/nodate")])?;

    let pp = b.item(1, "preprint", "PREP0001", 20)?;
    b.fields(pp, &[("title", "A Synthetic Preprint"), ("repository", "SynthArXiv"), ("archiveID", "synth:2501.00001"), ("DOI", "10.0000/synth.2501.00001")])?;
    b.authors(pp, &[("Lea", "Ng")])?;

    // group 库与 feed 库
    let g1 = b.item(3, "journalArticle", "GRUP0001", 21)?;
    b.fields(g1, &[("title", "Group Library Article")])?;
    b.put(c_group, g1)?;
    let f1 = b.item(2, "journalArticle", "FEED0001", 22)?;
    b.fields(f1, &[("title", "Feed Item (should be skipped)")])?;
    conn.execute("INSERT INTO feedItems VALUES (?1, 'guid-1', NULL, NULL)", [f1])?;

    // ---------- 附件 ----------
    let pdf1 = tiny_pdf("Synthetic panel study");
    let a1 = b.stored("ATT00001", Some(j1), 0, "application/pdf", "Smith and Jones - 2019 - Synthetic Panel Study.pdf", Some(&pdf1), StoredHash::Correct, 1)?;
    b.tag(a1, "pdf", 0)?;

    let html = "<html><head><meta charset=\"utf-8\"><title>Snapshot</title></head><body><p>互联网使用</p><img src=\"images/fig1.png\"></body></html>";
    let a2 = b.stored("ATT00002", Some(j2), 1, "text/html", "snapshot.html", Some(html.as_bytes()), StoredHash::Correct, 2)?;
    b.fields(a2, &[("url", "https://example.org/articles/2"), ("accessDate", "2026-01-02 10:00:00")])?;
    let img_dir = storage.join("ATT00002/images");
    std::fs::create_dir_all(&img_dir).map_err(|e| Error::Io(e, img_dir.clone()))?;
    std::fs::write(img_dir.join("fig1.png"), TINY_PNG).map_err(|e| Error::Io(e, img_dir.clone()))?;

    let linked_pdf = linked_dir.join("国富论-扫描版.pdf");
    std::fs::write(&linked_pdf, tiny_pdf("Wealth of Nations scan")).map_err(|e| Error::Io(e, linked_pdf.clone()))?;
    let a3 = b.linked("ATT00003", Some(bk1), 2, Some("application/pdf"), Some(&linked_pdf.display().to_string()), 3)?;
    b.field(a3, "title", "国富论-扫描版.pdf")?;
    conn.execute("INSERT INTO itemNotes (itemID, parentItemID, note, title) VALUES (?1, ?2, '<p>旧版附件笔记</p>', '旧版附件笔记')", params![a3, bk1])?;

    let missing_linked = linked_dir.join("does-not-exist.pdf");
    b.linked("ATT00004", Some(bk2), 2, Some("application/pdf"), Some(&missing_linked.display().to_string()), 4)?;

    let a5 = b.linked("ATT00005", Some(w1), 3, Some("text/html"), None, 5)?;
    b.fields(a5, &[("title", "Synthetic Policy Page (link)"), ("url", "https://example.org/policy"), ("accessDate", "2026-01-18 12:00:00")])?;

    let a6 = b.stored("ATT00006", None, 0, "application/pdf", "standalone-report.pdf", Some(&tiny_pdf("Standalone report")), StoredHash::Correct, 6)?;
    b.put(c_method, a6)?;

    let base_pdf = base_dir.join("papers/conf.pdf");
    std::fs::write(&base_pdf, tiny_pdf("Conference paper")).map_err(|e| Error::Io(e, base_pdf.clone()))?;
    b.linked("ATT00007", Some(cf1), 2, Some("application/pdf"), Some("attachments:papers/conf.pdf"), 7)?;

    b.stored("ATT00008", Some(j3), 0, "application/pdf", "missing.pdf", None, StoredHash::Correct, 8)?;
    b.stored("ATT00009", Some(j4), 0, "application/pdf", "weights.pdf", Some(&tiny_pdf("Survey weights")), StoredHash::Wrong, 9)?;
    b.stored("ATT00010", Some(th), 0, "application/pdf", "硕士学位论文.pdf", Some(&tiny_pdf("Thesis")), StoredHash::None, 10)?;

    // 笔记（NOTE0002 内嵌图片 ATT00011）
    let n1 = b.note("NOTE0001", Some(j1), "<div data-schema-version=\"9\"><h1>读书笔记</h1><p>内部效能感 vs 外部效能感，公式 $E = mc^2$。</p></div>", "读书笔记", 11)?;
    b.tag(n1, "笔记", 0)?;
    let n2 = b.note("NOTE0002", None, "<div><p>独立笔记，含图片</p><img data-attachment-key=\"ATT00011\"></div>", "独立笔记，含图片", 12)?;
    b.put(c_book, n2)?;
    b.stored("ATT00011", Some(n2), 4, "image/png", "image.png", Some(TINY_PNG), StoredHash::Correct, 12)?;

    let a12 = b.stored("ATT00012", Some(j5), 0, "application/pdf", "trashed.pdf", Some(&tiny_pdf("Trashed")), StoredHash::Correct, 13)?;
    let _ = a12;
    b.note("NOTE0003", Some(j5), "<p>父条目已删除</p>", "父条目已删除", 13)?;
    let a13 = b.stored("ATT00013", Some(j1), 0, "application/pdf", "old-version.pdf", Some(&tiny_pdf("Old version")), StoredHash::Correct, 14)?;
    b.trash_item(a13)?;
    let n4 = b.note("NOTE0004", None, "<p>直接删除的笔记</p>", "直接删除的笔记", 14)?;
    b.trash_item(n4)?;

    // 标注（在 ATT00001 与 ATT00006 上）
    let an1 = b.annotation("ANNO0001", a1, 1, None, Some("internal and external efficacy"), Some("核心概念"), "#ffd400", "1", "00000|000123|00079", r#"{"pageIndex":0,"rects":[[72.125,700.5,300.25,712.75]]}"#)?;
    b.tag(an1, "重要", 0)?;
    b.annotation("ANNO0002", a1, 2, None, None, Some("这里需要回头再读"), "#5fb236", "1", "00000|000200|00100", r#"{"pageIndex":0,"rects":[[400,600,422,622]]}"#)?;
    b.annotation("ANNO0003", a1, 4, None, None, None, "#2ea8e5", "1", "00000|000000|00000", r#"{"pageIndex":0,"width":2,"paths":[[100.5,100.5,120.25,130.75,140,150]]}"#)?;
    b.annotation("ANNO0004", a6, 5, Some("合成审稿人"), Some("standalone"), None, "#ff6666", "i", "00000|000010|00720", r#"{"pageIndex":0,"rects":[[72,720,150,732]]}"#)?;

    // 孤儿目录、不迁移的数据
    let orphan = storage.join(Expected::ORPHAN_DIR);
    std::fs::create_dir_all(&orphan).map_err(|e| Error::Io(e, orphan.clone()))?;
    std::fs::write(orphan.join("leftover.pdf"), tiny_pdf("Leftover")).map_err(|e| Error::Io(e, orphan.clone()))?;
    conn.execute_batch(&format!(
        "INSERT INTO fulltextItems (itemID, indexedPages, totalPages) VALUES ({a1}, 1, 1), ({a6}, 1, 1);
         INSERT INTO savedSearches (savedSearchID, savedSearchName, libraryID, key) VALUES (1, '未读', 1, 'SRCH0001');
         INSERT INTO syncedSettings (setting, libraryID, value) VALUES ('lastPageIndex_u_ATT00001', 1, '0');"
    ))?;
    drop(b);
    conn.close().map_err(|(_, e)| Error::Sqlite(e))?;

    Ok(FixturePaths { root: root.to_path_buf(), zotero_dir, base_dir, linked_dir })
}
