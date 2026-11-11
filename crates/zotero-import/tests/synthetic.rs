mod common;

use common::{fixture, temp_dir};
use std::collections::BTreeSet;
use zotero_import::audit::{self, Report};
use zotero_import::fixture::Expected;
use zotero_import::model::*;
use zotero_import::reader::{self, ReadOptions};
use zotero_import::util::hash_file;

fn read_and_audit(p: &zotero_import::fixture::FixturePaths, opts: &ReadOptions) -> (ZoteroLibrary, Report) {
    let lib = reader::read_library(&p.zotero_dir, opts).expect("读取");
    let conn = reader::open_readonly(&reader::db_path(&p.zotero_dir)).unwrap();
    let report = audit::audit(&conn, &lib, &reader::storage_path(&p.zotero_dir)).expect("审计");
    (lib, report)
}

fn att<'a>(lib: &'a ZoteroLibrary, key: &str) -> &'a Attachment {
    lib.attachments.iter().find(|a| a.base.key == key).unwrap_or_else(|| panic!("找不到附件 {key}"))
}

fn issues(r: &Report) -> BTreeSet<(String, String)> {
    r.file_issues.iter().map(|i| (i.kind.clone(), i.attachment_key.clone())).collect()
}

#[test]
fn fixture_reads_without_loss() {
    let p = fixture("lossless");
    let (lib, r) = read_and_audit(&p, &ReadOptions::default());

    for c in &r.checks {
        assert!(c.ok, "类别 {} 不一致：源 {} 模型 {}，{:?}", c.category, c.source_count, c.parsed_count, c.diff_sample);
    }
    assert_eq!(r.verdict, "pass_with_warnings");
    assert_eq!(lib.items.len(), Expected::REGULAR_ITEMS);
    assert_eq!(lib.attachments.len(), Expected::ATTACHMENTS);
    assert_eq!(lib.notes.len(), Expected::NOTES);
    assert_eq!(lib.annotations.len(), Expected::ANNOTATIONS);
    assert_eq!(lib.collections.len(), Expected::COLLECTIONS);
    assert_eq!(lib.source.client_version.as_deref(), Some("9.0.3"));
    assert_eq!(lib.source.schema_versions.get("userdata"), Some(&125));

    // 库：user + group，feed 不读
    let kinds: Vec<&str> = lib.libraries.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(kinds, ["user", "group"]);
    assert_eq!(lib.libraries[1].group_name.as_deref(), Some("合成课题组"));
    assert!(lib.all_bases().all(|b| b.key != "FEED0001"));
    assert_eq!(r.not_migrated["items(feed 库)"], Some(1));
    assert_eq!(r.not_migrated["fulltextItems"], Some(2));
    assert_eq!(r.not_migrated["savedSearches"], Some(1));
}

#[test]
fn fixture_file_checks() {
    let p = fixture("files");
    let (lib, r) = read_and_audit(&p, &ReadOptions::default());
    let expected: BTreeSet<(String, String)> = [
        ("base_relative_unresolved", Expected::BASE_RELATIVE),
        ("linked_missing", Expected::LINKED_MISSING),
        ("md5_mismatch", Expected::MD5_MISMATCH),
        ("stored_missing", Expected::STORED_MISSING),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(issues(&r), expected);
    assert_eq!(r.storage_orphan_dirs, vec![Expected::ORPHAN_DIR.to_string()]);

    let f = &r.files;
    assert_eq!(f.stored, 9);
    assert_eq!(f.stored_exists, 8);
    assert_eq!(f.md5_match, 6);
    assert_eq!(f.md5_mismatch, 1);
    assert_eq!(f.md5_no_storage_hash, 1);
    assert_eq!((f.linked, f.linked_exists), (2, 1));
    assert_eq!(f.no_file, 1); // linked_url
    assert_eq!(f.cache_files.get(".zotero-ft-cache"), Some(&8));

    // 快照的附加资源、附件自带笔记、内嵌图片、中文文件名
    assert_eq!(att(&lib, Expected::SNAPSHOT_WITH_EXTRA).extra_files, vec!["images/fig1.png".to_string()]);
    assert_eq!(att(&lib, Expected::ATTACHMENT_WITH_NOTE).note.as_deref(), Some("<p>旧版附件笔记</p>"));
    let img = att(&lib, "ATT00011");
    assert_eq!(img.link_mode, Some(LinkMode::EmbeddedImage));
    assert_eq!(img.parent_key.as_deref(), Some("NOTE0002"));
    let thesis = att(&lib, "ATT00010").file.as_ref().unwrap();
    assert!(thesis.exists && thesis.relative_path.as_deref() == Some("ATT00010/硕士学位论文.pdf"));
    assert_eq!(att(&lib, "ATT00002").charset.as_deref(), Some("utf-8"));

    // 文件哈希与磁盘一致
    let a1 = att(&lib, "ATT00001").file.as_ref().unwrap();
    let (sha, md5) = hash_file(std::path::Path::new(a1.resolved_path.as_ref().unwrap())).unwrap();
    assert_eq!(a1.sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(a1.md5.as_deref(), Some(md5.as_str()));
}

#[test]
fn base_dir_resolves_relative_links() {
    let p = fixture("basedir");
    let opts = ReadOptions { base_attachment_dir: Some(p.base_dir.clone()), skip_hashing: false };
    let (lib, r) = read_and_audit(&p, &opts);
    let f = att(&lib, Expected::BASE_RELATIVE).file.as_ref().unwrap();
    assert_eq!(f.kind, FileKind::BaseRelative);
    assert!(f.exists);
    assert!(!issues(&r).iter().any(|(_, k)| k == Expected::BASE_RELATIVE));
    assert_eq!((r.files.base_relative, r.files.base_relative_exists), (1, 1));
}

#[test]
fn fixture_structure_details() {
    let p = fixture("structure");
    let (lib, r) = read_and_audit(&p, &ReadOptions::default());
    let item = |k: &str| lib.items.iter().find(|i| i.key == k).unwrap();

    // 作者顺序与单栏姓名
    let many = item(Expected::MANY_AUTHORS_ITEM);
    assert_eq!(many.creators.len(), Expected::MANY_AUTHORS);
    assert!(many.creators.iter().enumerate().all(|(i, c)| c.order_index == i as i64 && c.last_name.as_deref() == Some(&format!("Last{i:02}"))));
    let cn = item("JRNL0002");
    assert_eq!(cn.creators[0].field_mode, 1);
    assert_eq!(cn.creators[0].last_name.as_deref(), Some("王小明"));
    let book = item("BOOK0001");
    assert_eq!(book.creators.iter().filter(|c| c.creator_type == "translator").count(), 2);

    // 日期保持 Zotero 原始格式；多行摘要；非文本值
    assert_eq!(item("JRNL0001").fields["date"], "2019-03-00 March 2019");
    assert!(item("JRNL0001").fields["abstractNote"].contains('\n'));
    assert_eq!(item("BOOK0002").fields["numPages"], "352");
    assert!(lib.warnings.iter().any(|w| w.contains("不是文本")));

    // 集合树、多集合成员、回收站
    let coll = |k: &str| lib.collections.iter().find(|c| c.key == k).unwrap();
    assert_eq!(coll("COLL0003").parent_key.as_deref(), Some("COLL0002"));
    assert!(coll("COLL0006").in_trash && coll("COLL0006").deleted_at.is_some());
    assert!(coll("COLL0007").in_trash && coll("COLL0007").deleted_at.is_none());
    assert_eq!(item("JRNL0008").collection_keys.len(), 3);
    assert!(item("JRNL0007").collection_keys.is_empty());
    assert_eq!(r.trash.items_deleted_directly, Expected::ITEMS_DELETED_DIRECTLY);
    assert_eq!(r.trash.items_trashed_via_parent, Expected::ITEMS_TRASHED_VIA_PARENT);
    assert_eq!(r.trash.collections_deleted_directly, Expected::COLLECTIONS_DELETED_DIRECTLY);
    assert_eq!(r.trash.collections_trashed_via_ancestor, Expected::COLLECTIONS_TRASHED_VIA_ANCESTOR);
    assert!(att(&lib, "ATT00012").base.in_trash);
    assert!(lib.notes.iter().find(|n| n.base.key == "NOTE0003").unwrap().base.in_trash);

    // 标签（含 emoji、手动/自动）、关联
    let tags: Vec<(&str, i64)> = item("JRNL0004").tags.iter().map(|t| (t.name.as_str(), t.tag_type)).collect();
    assert_eq!(tags, [("to read", 0), ("🔥 hot", 0)]);
    assert_eq!(item("JRNL0001").relations[0].predicate, "dc:relation");
    assert_eq!(item("BOOK0001").relations[0].predicate, "dc:replaces");

    // 标注：位置 JSON 原样保留
    let an = lib.annotations.iter().find(|a| a.base.key == "ANNO0001").unwrap();
    assert_eq!(an.annotation_type, "highlight");
    assert_eq!(an.parent_key.as_deref(), Some("ATT00001"));
    assert_eq!(an.position.as_deref(), Some(r#"{"pageIndex":0,"rects":[[72.125,700.5,300.25,712.75]]}"#));
    assert_eq!(an.base.tags[0].name, "重要");
    let ink = lib.annotations.iter().find(|a| a.base.key == "ANNO0003").unwrap();
    assert_eq!(ink.annotation_type, "ink");
}

#[test]
fn json_roundtrip_is_lossless() {
    let p = fixture("json");
    let (lib, _) = read_and_audit(&p, &ReadOptions::default());
    let s = serde_json::to_string(&lib).unwrap();
    let back: ZoteroLibrary = serde_json::from_str(&s).unwrap();
    assert_eq!(lib, back);
}

/// 审计必须能发现读取环节的任何丢失：逐类篡改模型，对应类别应判失败。
#[test]
fn audit_detects_tampering() {
    let p = fixture("tamper");
    let lib = reader::read_library(&p.zotero_dir, &ReadOptions::default()).unwrap();
    let conn = reader::open_readonly(&reader::db_path(&p.zotero_dir)).unwrap();
    let storage = reader::storage_path(&p.zotero_dir);

    type Tamper = Box<dyn Fn(&mut ZoteroLibrary)>;
    let cases: Vec<(&str, Tamper)> = vec![
        ("fields", Box::new(|l| { l.items[0].fields.pop_first(); })),
        ("fields", Box::new(|l| { let v = l.items[0].fields.values_mut().next().unwrap(); v.push(' '); })),
        ("creators", Box::new(|l| {
            // 作者顺序错乱：交换前两位的 order_index（只交换 Vec 位置不算丢失，order_index 仍在）
            let i = l.items.iter_mut().find(|i| i.creators.len() > 1).unwrap();
            let (a, b) = (i.creators[0].order_index, i.creators[1].order_index);
            i.creators[0].order_index = b;
            i.creators[1].order_index = a;
        })),
        ("creators", Box::new(|l| { l.items.iter_mut().find(|i| !i.creators.is_empty()).unwrap().creators[0].field_mode ^= 1; })),
        ("tags", Box::new(|l| { l.items.iter_mut().find(|i| !i.tags.is_empty()).unwrap().tags.clear(); })),
        ("collection_items", Box::new(|l| { l.items.iter_mut().find(|i| !i.collection_keys.is_empty()).unwrap().collection_keys.pop(); })),
        ("collections", Box::new(|l| { l.collections[2].parent_key = None; })),
        ("items", Box::new(|l| { l.items.pop(); })),
        ("attachments", Box::new(|l| { l.attachments[0].storage_hash = None; })),
        ("attachment_notes", Box::new(|l| { l.attachments.iter_mut().for_each(|a| a.note = None); })),
        ("notes", Box::new(|l| { l.notes[0].html = Some("<p>改过</p>".into()); })),
        ("annotations", Box::new(|l| { l.annotations[0].position = None; })),
        ("item_relations", Box::new(|l| { l.items.iter_mut().for_each(|i| i.relations.clear()); })),
        ("deleted_items", Box::new(|l| { l.items.iter_mut().for_each(|i| i.deleted_at = None); })),
        ("deleted_collections", Box::new(|l| { l.collections.iter_mut().for_each(|c| c.deleted_at = None); })),
    ];
    for (cat, f) in cases {
        let mut t = lib.clone();
        f(&mut t);
        let r = audit::audit(&conn, &t, &storage).unwrap();
        let c = r.checks.iter().find(|c| c.category == cat).unwrap();
        assert!(!c.ok, "篡改 {cat} 未被发现");
        assert_eq!(r.verdict, "fail");
    }
}

#[test]
fn source_is_never_modified() {
    let p = fixture("readonly");
    let db = reader::db_path(&p.zotero_dir);
    let before = hash_file(&db).unwrap();
    let mtime = std::fs::metadata(&db).unwrap().modified().unwrap();
    let listing = |d: &std::path::Path| -> BTreeSet<String> {
        std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect()
    };
    let files_before = listing(&p.zotero_dir);

    let (_, _) = read_and_audit(&p, &ReadOptions::default());
    let conn = reader::open_readonly(&db).unwrap();
    assert!(conn.execute("DELETE FROM items", []).is_err(), "只读连接不应允许写入");
    drop(conn);

    assert_eq!(hash_file(&db).unwrap(), before);
    assert_eq!(std::fs::metadata(&db).unwrap().modified().unwrap(), mtime);
    assert_eq!(listing(&p.zotero_dir), files_before, "不应产生 -journal / -wal / -shm 等文件");
}

#[test]
fn rejects_non_zotero_database() {
    let d = temp_dir("notzotero");
    std::fs::create_dir_all(&d).unwrap();
    let c = rusqlite::Connection::open(d.join("zotero.sqlite")).unwrap();
    c.execute_batch("CREATE TABLE items (id INTEGER);").unwrap();
    drop(c);
    let err = reader::read_library(&d, &ReadOptions::default()).unwrap_err();
    assert!(err.to_string().contains("缺少表"));
}

#[test]
fn cli_refuses_live_zotero_dir_and_writes_reports() {
    let exe = env!("CARGO_BIN_EXE_zotero-probe");
    let p = fixture("cli");
    // 伪造 HOME，使夹具目录看起来就是 ~/Zotero
    let fake_home = temp_dir("cli-home");
    std::fs::create_dir_all(&fake_home).unwrap();
    std::os::unix::fs::symlink(&p.zotero_dir, fake_home.join("Zotero")).unwrap();
    let out = p.root.join("out");
    let o = std::process::Command::new(exe)
        .args(["read", fake_home.join("Zotero").to_str().unwrap(), "--out", out.to_str().unwrap()])
        .env("HOME", &fake_home)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("拒绝读取"));
    assert!(!out.exists());

    let o = std::process::Command::new(exe)
        .args(["read", p.zotero_dir.to_str().unwrap(), "--out", out.to_str().unwrap()])
        .env("HOME", fake_home.join("elsewhere"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for f in ["library.json", "report.json", "report.md"] {
        assert!(out.join(f).is_file(), "缺少输出 {f}");
    }
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    assert!(md.contains("逐类核对") && !md.contains("Synthetic Panel Study"), "report.md 不应包含标题等正文");
}
