//! Zotero 导入测试：合成夹具（`zotero_import::fixture`，不含真实数据）覆盖的每种边界情况都逐条断言。

use super::*;
use crate::db::{self, tests::TempDir, tests::temp_dir};
use crate::import::runs_list;
use rusqlite::Connection;
use zotero_import::fixture::{self, Expected, FixturePaths};

struct Env {
    _dir: TempDir,
    paths: DataPaths,
    c: Connection,
    fx: FixturePaths,
}

fn env(name: &str) -> Env {
    let dir = temp_dir(name);
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    let fx = fixture::build(&dir.join("fixture")).unwrap();
    Env { _dir: dir, paths, c, fx }
}

fn opts(e: &Env) -> Options {
    Options {
        base_dir: Some(e.fx.base_dir.clone()),
    }
}

fn one<T: rusqlite::types::FromSql>(c: &Connection, sql: &str) -> T {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.extend(files_in(&e.path()));
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

#[test]
fn preview_counts_and_relative_paths() {
    let e = env("zimp-preview");
    let p = preview(&e.c, &e.fx.zotero_dir, &Options::default()).unwrap();
    assert_eq!(
        (p.items, p.attachments, p.notes, p.annotations, p.collections),
        (
            Expected::REGULAR_ITEMS,
            Expected::ATTACHMENTS,
            Expected::NOTES,
            Expected::ANNOTATIONS,
            Expected::COLLECTIONS
        )
    );
    assert_eq!(p.client_version.as_deref(), Some("9.0.3"));
    assert!(p.version_tested);
    assert_eq!(p.group_libraries, 1);
    assert_eq!(p.trash_items, 3, "JRNL0005、ATT00013、NOTE0004");
    assert_eq!(p.trash_collections, 1);
    assert_eq!(p.base_relative, 1);
    let keys: Vec<&str> = p.missing_files.iter().filter_map(|i| i.key.as_deref()).collect();
    assert!(
        keys.contains(&Expected::STORED_MISSING) && keys.contains(&Expected::LINKED_MISSING) && keys.contains(&Expected::BASE_RELATIVE),
        "{keys:?}"
    );
    let rel = p.missing_files.iter().find(|i| i.key.as_deref() == Some(Expected::BASE_RELATIVE)).unwrap();
    assert!(rel.reason.contains("链接附件根目录"), "{}", rel.reason);
    let stored = p.missing_files.iter().find(|i| i.key.as_deref() == Some(Expected::STORED_MISSING)).unwrap();
    assert_eq!(stored.parent_title.as_deref(), Some("Measuring External Efficacy"));
    assert!(p.blocked.is_none() && p.already_imported == 0 && !p.zotero_running);

    let with_base = preview(&e.c, &e.fx.zotero_dir, &opts(&e)).unwrap();
    assert_eq!(with_base.missing_files.len(), 2, "提供根目录后相对路径能找到");

    let e2 = preview(&e.c, &e.fx.root, &Options::default()).unwrap_err();
    assert!(e2.contains("没有 zotero.sqlite"), "{e2}");
}

#[test]
fn imports_fixture_without_loss() {
    let e = env("zimp-full");
    let db_file = reader::db_path(&e.fx.zotero_dir);
    let before = util::hash_file(&db_file).unwrap();
    let r = run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).unwrap();
    assert_eq!(util::hash_file(&db_file).unwrap(), before, "Zotero 副本一个字节都没改");

    // 核对全部一致
    assert!(r.report.checks.len() >= 14);
    for ch in r.report.checks.iter().chain(&r.report.source_checks) {
        assert!(ch.ok, "{} 不一致：{:?}", ch.label, ch.diff);
    }
    assert!(r.report.checks.iter().all(|ch| ch.source == ch.stored));
    let st = &r.summary.stats;
    assert_eq!((st.items, st.attachments, st.notes, st.annotations, st.collections), (21, 13, 4, 4, 8));
    assert_eq!(st.missing_files, 2);
    assert_eq!(st.trash, 4, "直接删除的 3 条记录 + 1 个集合");
    assert!(st.files_reused >= 1, "同样的 PNG（内嵌图片与快照资源）只存一份");
    assert!(r.summary.backup_file.as_deref().is_some_and(|f| f.ends_with("-before-import.db")));
    assert!(r.report.file_warnings.iter().any(|i| i.key.as_deref() == Some(Expected::MD5_MISMATCH)));
    assert!(r.report.not_migrated.iter().any(|n| n.label.contains("全文索引")));
    assert!(
        r.report.not_migrated.iter().any(|n| n.label.contains("没有对应附件记录的目录") && n.count == 1),
        "孤儿目录 {}",
        Expected::ORPHAN_DIR
    );
    assert_eq!(runs_list(&e.c).unwrap().len(), 1);
    assert_eq!(runs_get(&e.c, r.summary.id).unwrap(), r);

    let c = &e.c;
    // 条目与字段
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM items WHERE source = 'zotero'"), 21);
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM items WHERE source_key = 'FEED0001'"), 0, "feed 库不导入");
    assert_eq!(one::<i64>(c, "SELECT source_library FROM items WHERE source_key = 'GRUP0001'"), 3);
    let (title, date_raw, date_sort, year, container, doi, lang): (String, String, String, i64, String, String, String) = c
        .query_row(
            "SELECT title, date_raw, date_sort, year, container_title, doi, detected_lang FROM items WHERE source_key = 'JRNL0001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(
        (title.as_str(), date_raw.as_str(), date_sort.as_str(), year),
        ("Synthetic Panel Study of Efficacy and Voting", "March 2019", "2019-03-00", 2019)
    );
    assert_eq!(
        (container.as_str(), doi.as_str(), lang.as_str()),
        ("Journal of Synthetic Politics", "10.0000/synthetic.2019.001", "en")
    );
    assert_eq!(
        one::<String>(c, "SELECT created_at FROM items WHERE source_key = 'JRNL0001'"),
        "2026-01-01T10:00:00.000Z",
        "保留 Zotero 的添加时间"
    );
    assert_eq!(one::<String>(c, "SELECT detected_lang FROM items WHERE source_key = 'JRNL0002'"), "zh");
    assert_eq!(one::<Option<String>>(c, "SELECT language FROM items WHERE source_key = 'BOOK0002'"), None);
    assert_eq!(
        one::<String>(c, "SELECT container_title FROM items WHERE source_key = 'BSEC0001'"),
        "Research Design in Political Science",
        "章节的 bookTitle"
    );
    assert_eq!(
        one::<String>(
            c,
            "SELECT value FROM item_fields f JOIN items i ON i.id = f.item_id WHERE i.source_key = 'BOOK0002' AND f.field = 'numPages'"
        ),
        "352"
    );
    assert_eq!(one::<String>(c, "SELECT isbn FROM items WHERE source_key = 'BOOK0001'"), "9780000000001");
    // 作者：中文单栏不拆、30 位作者、译者
    assert_eq!(
        one::<String>(
            c,
            "SELECT literal FROM creators cr JOIN items i ON i.id = cr.item_id WHERE i.source_key = 'JRNL0002' AND cr.position = 0"
        ),
        "王小明"
    );
    assert_eq!(
        one::<i64>(
            c,
            &format!(
                "SELECT COUNT(*) FROM creators cr JOIN items i ON i.id = cr.item_id WHERE i.source_key = '{}'",
                Expected::MANY_AUTHORS_ITEM
            )
        ),
        Expected::MANY_AUTHORS as i64
    );
    assert_eq!(
        one::<i64>(
            c,
            "SELECT COUNT(*) FROM creators cr JOIN items i ON i.id = cr.item_id WHERE i.source_key = 'BOOK0001' AND cr.role = 'translator'"
        ),
        2
    );
    // 标签：普通条目、附件、笔记、标注都有
    assert_eq!(
        one::<String>(
            c,
            "SELECT kind FROM item_tags it JOIN tags t ON t.id = it.tag_id JOIN items i ON i.id = it.item_id WHERE i.source_key = 'JRNL0001' AND t.name = 'machine learning'"
        ),
        "automatic"
    );
    assert_eq!(
        one::<i64>(c, "SELECT COUNT(*) FROM attachment_tags x JOIN tags t ON t.id = x.tag_id WHERE t.name = 'pdf'"),
        1
    );
    assert_eq!(
        one::<i64>(c, "SELECT COUNT(*) FROM note_tags x JOIN tags t ON t.id = x.tag_id WHERE t.name = '笔记'"),
        1
    );
    assert_eq!(
        one::<i64>(c, "SELECT COUNT(*) FROM annotation_tags x JOIN tags t ON t.id = x.tag_id WHERE t.name = '重要'"),
        1
    );
    // 集合树、独立附件 / 笔记进集合
    assert_eq!(
        one::<String>(
            c,
            "SELECT p.name FROM collections c JOIN collections p ON p.id = c.parent_id WHERE c.source_key = 'COLL0003'"
        ),
        "文献综述"
    );
    assert_eq!(
        one::<i64>(
            c,
            "SELECT COUNT(*) FROM collection_items ci JOIN items i ON i.id = ci.item_id WHERE i.source_key = 'JRNL0008'"
        ),
        3
    );
    assert_eq!(
        one::<String>(
            c,
            "SELECT c.name FROM collection_attachments x JOIN collections c ON c.id = x.collection_id JOIN attachments a ON a.id = x.attachment_id WHERE a.source_key = 'ATT00006'"
        ),
        "方法"
    );
    assert_eq!(
        one::<String>(
            c,
            "SELECT c.name FROM collection_notes x JOIN collections c ON c.id = x.collection_id JOIN notes n ON n.id = x.note_id WHERE n.source_key = 'NOTE0002'"
        ),
        "读书：国富论"
    );
    // 回收站：只在直接删除的记录上有 deleted_at（S4）
    assert!(one::<Option<String>>(c, "SELECT deleted_at FROM items WHERE source_key = 'JRNL0005'").is_some());
    assert_eq!(
        one::<Option<String>>(c, "SELECT deleted_at FROM attachments WHERE source_key = 'ATT00012'"),
        None,
        "随父条目进回收站"
    );
    assert_eq!(
        one::<String>(
            c,
            "SELECT i.source_key FROM notes n JOIN items i ON i.id = n.item_id WHERE n.source_key = 'NOTE0003'"
        ),
        "JRNL0005"
    );
    assert!(one::<Option<String>>(c, "SELECT deleted_at FROM attachments WHERE source_key = 'ATT00013'").is_some());
    assert!(one::<Option<String>>(c, "SELECT deleted_at FROM notes WHERE source_key = 'NOTE0004'").is_some());
    assert!(one::<Option<String>>(c, "SELECT deleted_at FROM collections WHERE source_key = 'COLL0006'").is_some());
    assert_eq!(
        one::<Option<String>>(c, "SELECT deleted_at FROM collections WHERE source_key = 'COLL0007'"),
        None
    );

    // 附件：文件按哈希入库、只读；缺失的标记 missing；链接文件也复制进来并记下原路径
    let (sha, status, name): (String, String, String) = c
        .query_row("SELECT sha256, status, original_name FROM attachments WHERE source_key = 'ATT00001'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((status.as_str(), name.as_str()), ("ok", "Smith and Jones - 2019 - Synthetic Panel Study.pdf"));
    let stored = e.paths.attachments.join(&sha[..2]).join(format!("{sha}.pdf"));
    assert_eq!(util::hash_file(&stored).unwrap().0, sha);
    assert!(std::fs::metadata(&stored).unwrap().permissions().readonly());
    for k in [Expected::STORED_MISSING, Expected::LINKED_MISSING] {
        let (status, sha): (String, Option<String>) = c
            .query_row("SELECT status, sha256 FROM attachments WHERE source_key = ?1", [k], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((status.as_str(), sha), ("missing", None), "{k}");
    }
    assert_eq!(
        one::<String>(c, "SELECT status FROM attachments WHERE source_key = 'ATT00005'"),
        "ok",
        "网址链接没有文件，不算缺失"
    );
    let (kind, path): (String, String) = c
        .query_row("SELECT kind, original_path FROM attachments WHERE source_key = 'ATT00003'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(kind, "linked_file");
    assert!(path.ends_with("国富论-扫描版.pdf"));
    assert_eq!(
        one::<String>(c, "SELECT note FROM attachments WHERE source_key = 'ATT00003'"),
        "<p>旧版附件笔记</p>"
    );
    assert_eq!(
        one::<String>(c, "SELECT status FROM attachments WHERE source_key = 'ATT00007'"),
        "ok",
        "相对路径用根目录找到"
    );
    // 快照的资源文件也进了文件库
    let meta: String = one(c, "SELECT source_meta FROM attachments WHERE source_key = 'ATT00002'");
    let meta: J = serde_json::from_str(&meta).unwrap();
    assert_eq!(meta["extraFiles"][0][0], "images/fig1.png");
    assert_eq!(meta["linkMode"], 1);
    assert_eq!(meta["fields"]["accessDate"], "2026-01-02 10:00:00");
    // 笔记内嵌图片：上级是笔记，HTML 里的 key 换成了附件 uid
    let (img_uid, img_note): (String, i64) = c
        .query_row("SELECT uid, note_id FROM attachments WHERE source_key = 'ATT00011'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let (note_id, html): (i64, String) = c
        .query_row("SELECT id, html FROM notes WHERE source_key = 'NOTE0002'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(img_note, note_id);
    assert!(
        html.contains(&format!("data-attachment-key=\"{img_uid}\"")) && !html.contains("ATT00011"),
        "{html}"
    );
    assert_eq!(one::<Option<i64>>(c, "SELECT item_id FROM attachments WHERE source_key = 'ATT00011'"), None);
    // 文件库：同样的内容只有一份
    let rows: i64 = one(c, "SELECT COUNT(*) FROM files");
    let on_disk = files_in(&e.paths.attachments).len() as i64;
    assert_eq!(rows, on_disk);
    assert_eq!(
        one::<i64>(c, "SELECT COUNT(DISTINCT sha256) FROM attachments WHERE sha256 IS NOT NULL"),
        rows,
        "快照资源 fig1.png 与内嵌图片 image.png 同内容，只存一份"
    );
    // 标注：位置原样、页码
    let (pos, page, sort): (String, i64, String) = c
        .query_row(
            "SELECT position, page_index, sort_key FROM annotations WHERE source_key = 'ANNO0001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(pos, r#"{"pageIndex":0,"rects":[[72.125,700.5,300.25,712.75]]}"#);
    assert_eq!((page, sort.as_str()), (0, "00000|000123|00079"));
    assert_eq!(
        one::<String>(c, "SELECT author_name FROM annotations WHERE source_key = 'ANNO0004'"),
        "合成审稿人"
    );
    // 导入前备份
    assert_eq!(backup_list(&e.paths).iter().filter(|b| b.label == "before-import").count(), 1);
}

fn backup_list(p: &DataPaths) -> Vec<crate::backup::BackupInfo> {
    crate::backup::list(&p.backups).unwrap()
}

#[test]
fn second_import_is_refused() {
    let e = env("zimp-twice");
    run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).unwrap();
    let p = preview(&e.c, &e.fx.zotero_dir, &opts(&e)).unwrap();
    assert!(p.already_imported > 0 && p.blocked.as_deref().is_some_and(|b| b.contains("已经导入过")));
    let err = run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).unwrap_err();
    assert!(err.contains("已经导入过"), "{err}");
    assert_eq!(one::<i64>(&e.c, "SELECT COUNT(*) FROM items"), 21, "没有重复");
    assert_eq!(runs_list(&e.c).unwrap().len(), 1);
}

#[test]
fn failure_rolls_back_everything_including_copied_files() {
    let e = env("zimp-rollback");
    // 写到标注时出错（模拟中途失败）
    e.c.execute_batch("CREATE TEMP TRIGGER boom BEFORE INSERT ON annotations BEGIN SELECT RAISE(ABORT, '模拟失败'); END;")
        .unwrap();
    let err = run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).unwrap_err();
    assert!(err.contains("模拟失败"), "{err}");
    for t in ["items", "attachments", "notes", "collections", "tags", "files", "import_runs"] {
        assert_eq!(one::<i64>(&e.c, &format!("SELECT COUNT(*) FROM {t}")), 0, "{t} 已回滚");
    }
    assert!(
        files_in(&e.paths.attachments).is_empty(),
        "这次复制的附件已删除：{:?}",
        files_in(&e.paths.attachments)
    );
    assert_eq!(backup_list(&e.paths).len(), 1, "导入前备份还在");
    // 修好之后可以正常导入
    e.c.execute_batch("DROP TRIGGER boom").unwrap();
    assert!(run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).is_ok());
}

#[test]
fn files_already_in_store_are_reused() {
    let e = env("zimp-reuse");
    // 先手动放一份同内容的文件进文件库（模拟另一次导入带来的）
    let pdf = fixture::tiny_pdf("Synthetic panel study");
    let sha = util::sha256_hex(&pdf);
    let src = e.fx.root.join("same.pdf");
    std::fs::write(&src, &pdf).unwrap();
    store::put(&e.paths.attachments, &src, &sha, "pdf").unwrap();
    let r = run(&e.c, &e.paths, &e.fx.zotero_dir, &opts(&e)).unwrap();
    assert!(r.summary.stats.files_reused >= 2);
    assert_eq!(files_in(&e.paths.attachments).len() as i64, one::<i64>(&e.c, "SELECT COUNT(*) FROM files"));
}

#[test]
fn rewrite_attachment_keys() {
    let f = |k: &str| (k == "AAA").then(|| "U1".to_string());
    assert_eq!(
        rewrite_keys(r#"<img data-attachment-key="AAA"><img data-attachment-key='AAA'>"#, f),
        r#"<img data-attachment-key="U1"><img data-attachment-key='U1'>"#
    );
    assert_eq!(
        rewrite_keys(r#"<img data-attachment-key="BBB">"#, f),
        r#"<img data-attachment-key="BBB">"#,
        "认不出的保持原样"
    );
    assert_eq!(rewrite_keys("no images", f), "no images");
    assert_eq!(rewrite_keys(r#"broken data-attachment-key="AAA"#, f), r#"broken data-attachment-key="AAA"#);
}

/// 真实 Zotero 库副本导入到临时数据目录（只读副本，前后哈希不变）。默认忽略：
/// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith real_copy -- --ignored --nocapture`
/// 只打印聚合数字，不打印标题、作者、文件名。
#[test]
#[ignore]
fn real_copy_imports_without_loss() {
    let src = PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"));
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!src.starts_with(Path::new(&home).join("Zotero")), "不能直接读 ~/Zotero，请用副本");
    let dir = temp_dir("zimp-real");
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    let before = util::hash_file(&reader::db_path(&src)).unwrap();
    let t = std::time::Instant::now();
    let p = preview(&c, &src, &Options::default()).unwrap();
    let r = run(&c, &paths, &src, &Options::default()).unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(util::hash_file(&reader::db_path(&src)).unwrap(), before, "副本被改动");
    for ch in r.report.checks.iter().chain(&r.report.source_checks) {
        assert!(ch.ok, "{} 不一致：{:?}", ch.label, ch.diff);
    }
    let s = &r.summary.stats;
    assert_eq!(
        (s.items, s.attachments, s.notes, s.annotations, s.collections),
        (p.items, p.attachments, p.notes, p.annotations, p.collections)
    );
    println!(
        "预览：条目 {} 附件 {} 笔记 {} 标注 {} 集合 {} 标签 {} 回收站 {}+{} 缺失文件 {}",
        p.items,
        p.attachments,
        p.notes,
        p.annotations,
        p.collections,
        p.tags,
        p.trash_items,
        p.trash_collections,
        p.missing_files.len()
    );
    println!("导入：{s:?}");
    println!(
        "核对：写入 {} 类、读取 {} 类全部一致；用时 {secs:.2} s",
        r.report.checks.len(),
        r.report.source_checks.len()
    );
    for ch in &r.report.checks {
        println!("  {}：{} / {}", ch.label, ch.source, ch.stored);
    }
    println!(
        "不迁移：{:?}",
        r.report.not_migrated.iter().map(|n| format!("{} {}", n.label, n.count)).collect::<Vec<_>>()
    );
    println!(
        "提示 {} 条、文件警告 {} 条、跳过 {} 条",
        r.report.warnings.len(),
        r.report.file_warnings.len(),
        r.report.skipped.len()
    );
    let files: i64 = c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    println!("文件库：{files} 个文件，{:.1} MB", s.bytes_copied as f64 / 1e6);
}
