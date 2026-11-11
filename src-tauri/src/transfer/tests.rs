//! 整库导出 / 恢复与升级流程的测试（合成数据）。
//!
//! 升级流程（`docs/testing.md`）：
//! 1. 每个旧版本：把合成的完整库「投影」到那个版本（只留那时有的表和列）→ 升到最新 → 旧数据逐表一致、App 的读取全部能用；
//! 2. 冻结的样本库 `src-tauri/fixtures/library-v*.json`（当时的代码生成的整库导出）→ 恢复 → 升到最新 → 同样的检查。

use super::*;
use crate::db::tests::{TempDir, temp_dir};
use crate::edit::{self, CreatorInput, ItemPatch};
use crate::library::{self, ListQuery, Scope};
use crate::paths::DataPaths;

pub(crate) struct Lib {
    pub _dir: TempDir,
    pub paths: DataPaths,
    pub c: Connection,
}

fn new_lib(dir: &Path) -> (DataPaths, Connection) {
    let paths = DataPaths::new(dir.to_path_buf());
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    crate::search::attach(&c, &paths.search_db).unwrap();
    (paths, c)
}

fn one<T: rusqlite::types::FromSql>(c: &Connection, sql: &str) -> T {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// 合成的「什么都有」的库：Zotero 合成夹具导入 + 手动新建的文献、App 里的标注、阅读位置、联网补全记录、「不是重复」、设置、没人用的标签。
pub(crate) fn rich_library(name: &str) -> Lib {
    let dir = temp_dir(name);
    let (paths, c) = new_lib(&dir.join("data"));
    let fx = zotero_import::fixture::build(&dir.join("fx")).unwrap();
    crate::import::zotero::run(&c, &paths, &fx.zotero_dir, &crate::import::zotero::Options { base_dir: Some(fx.base_dir.clone()) }).unwrap();

    let id = edit::item_create(&c, "book", None).unwrap();
    let mut p = ItemPatch { fields: [("title", "国富论（合成）"), ("date", "1776"), ("ISBN", "9780140449136")].map(|(k, v)| (k.to_string(), v.to_string())).into(), ..Default::default() };
    p.creators = Some(vec![CreatorInput { role: "author".into(), family: None, given: None, literal: Some("亚当·斯密".into()) }]);
    edit::item_update(&c, id, &p).unwrap();
    let pdf: i64 = one(&c, "SELECT id FROM attachments WHERE content_type = 'application/pdf' AND sha256 IS NOT NULL ORDER BY id LIMIT 1");
    let (a, b): (i64, i64) = c.query_row("SELECT MIN(id), MAX(id) FROM items", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    c.execute_batch(&format!(
        "INSERT INTO annotations (uid, attachment_id, kind, page_index, page_label, position, sort_key, text, comment, color, created_at, updated_at)
           VALUES ('01JAPPANNOTATION0000000000', {pdf}, 'highlight', 0, '1', '{{\"pageIndex\":0,\"rects\":[[72.5,700,300.25,712]]}}', '00000|000010|00080',
                   '合成的高亮', '评论 \"引号\"', '#ffd400', '2026-11-09T01:00:00.000Z', '2026-11-09T01:00:00.000Z');
         INSERT INTO reader_states (attachment_id, state, updated_at) VALUES ({pdf}, '{{\"pageIndex\":2,\"scale\":\"page-width\",\"top\":612.5}}', '2026-11-09T01:00:00.000Z');
         INSERT INTO metadata_lookups (item_id, identifier, trigger, status, source, record, changes, created_at, updated_at)
           VALUES ({id}, 'isbn:9780140449136', 'manual', 'found', 'openlibrary', '{{\"title\":\"The Wealth of Nations\"}}',
                   '[{{\"field\":\"title\",\"current\":\"国富论（合成）\",\"found\":\"The Wealth of Nations\",\"kind\":\"conflict\",\"state\":\"pending\"}}]',
                   '2026-11-09T01:00:00.000Z', '2026-11-09T01:00:00.000Z');
         INSERT INTO duplicate_dismissals (item_a, item_b, created_at) VALUES ({a}, {b}, '2026-11-09T01:00:00.000Z');
         INSERT INTO settings (key, value) VALUES ('metadata.enabled', '1'), ('metadata.email', 'someone@example.org');
         INSERT INTO tags (name) VALUES ('没人用的标签');"
    ))
    .unwrap();
    crate::search::rebuild(&c).unwrap();
    Lib { _dir: dir, paths, c }
}

fn digests(t: &BTreeMap<String, Table>) -> BTreeMap<String, Digest> {
    t.iter().map(|(k, v)| (k.clone(), v.digest())).collect()
}

fn export_of(lib: &Lib, to: &Path) -> ExportReport {
    std::fs::create_dir_all(to).unwrap();
    export(&snapshot(&lib.c).unwrap(), &lib.paths.attachments, to, "0.1.0-test").unwrap()
}

/// App 的读取在这个库上全部能用（升级、恢复之后跑）
fn exercise(c: &Connection) {
    assert_eq!(one::<String>(c, "PRAGMA integrity_check"), "ok");
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM pragma_foreign_key_check"), 0, "外键都对得上");
    library::overview(c).unwrap();
    for scope in [Scope::All, Scope::Unfiled, Scope::Trash, Scope::Missing, Scope::Duplicates, Scope::Review] {
        library::items_list(c, &ListQuery::of(scope)).unwrap();
    }
    let ids = |sql: &str| -> Vec<i64> { c.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect() };
    for id in ids("SELECT id FROM items") {
        library::item_get(c, id).unwrap();
        crate::metadata::latest(c, id).unwrap();
    }
    for id in ids("SELECT id FROM attachments") {
        library::attachment_get(c, id).unwrap();
    }
    for id in ids("SELECT DISTINCT attachment_id FROM annotations") {
        crate::reader::annotations_list(c, id).unwrap();
        crate::reader::reader_get(c, id).unwrap();
    }
    for id in ids("SELECT id FROM notes") {
        library::note_get(c, id).unwrap();
    }
    for id in ids("SELECT id FROM collections") {
        library::collection_get(c, id).unwrap();
    }
    library::collections_list(c).unwrap();
    library::tags_list(c).unwrap();
    for r in crate::import::runs_list(c).unwrap() {
        crate::import::runs_get(c, r.id).unwrap();
    }
    crate::metadata::settings_get(c).unwrap();
    crate::search::rebuild(c).unwrap();
}

// ---------------- 导出 ----------------

#[test]
fn export_writes_folder_and_passes_self_check() {
    let lib = rich_library("tr-export");
    let out = lib._dir.join("out");
    let r = export_of(&lib, &out);
    let dir = PathBuf::from(&r.path);
    assert!(dir.file_name().unwrap().to_string_lossy().starts_with("Pith 导出 "));
    assert!(dir.join(JSON_NAME).is_file() && dir.join("README.txt").is_file());
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 1, "没有留下半成品：{names:?}");

    // 自检：每张表都核对过，全部一致
    let tables = table_names(&lib.c).unwrap();
    assert_eq!(r.checks.len(), tables.len());
    assert!(r.checks.iter().all(|c| c.ok), "{:?}", r.checks);
    assert_eq!(r.checks[0].label, "文献");
    let items: usize = one::<i64>(&lib.c, "SELECT COUNT(*) FROM items") as usize;
    assert_eq!((r.counts.items, r.checks[0].source, r.checks[0].stored), (items, items, items));
    assert!(r.missing_files.is_empty());

    // 附件文件：与 files 表一一对应，内容一致
    let files = file_rows(&lib.c).unwrap();
    assert_eq!(r.counts.files, files.len());
    for (sha, ext, size) in &files {
        let p = dir.join(FILES_DIR).join(store::rel_path(sha, ext));
        assert_eq!(std::fs::metadata(&p).unwrap().len(), *size);
        assert_eq!(&zotero_import::util::hash_file(&p).unwrap().0, sha);
        assert!(!std::fs::metadata(&p).unwrap().permissions().readonly(), "导出里的文件可以随意删");
    }

    // library.json：一行一条记录，能被普通 JSON 工具读
    let text = std::fs::read_to_string(dir.join(JSON_NAME)).unwrap();
    let v: J = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], FORMAT);
    assert_eq!(v["formatVersion"], FORMAT_VERSION);
    assert_eq!(v["schemaVersion"], db::latest());
    assert_eq!(v["tables"]["items"].as_array().unwrap().len(), items);
    assert!(text.contains("\"title\":\"国富论（合成）\""), "中文不转义");
    assert!(text.lines().any(|l| l.starts_with("{\"id\":") && l.contains("\"uid\":")));
    let readme = std::fs::read_to_string(dir.join("README.txt")).unwrap();
    assert!(readme.contains(&format!("文献 {items} 条")), "{readme}");

    // 同一秒再导出一次：不覆盖
    let again = export_of(&lib, &out);
    assert_ne!(again.path, r.path);
}

#[test]
fn export_reports_files_missing_from_the_store() {
    let lib = rich_library("tr-export-missing");
    let (sha, ext, _) = file_rows(&lib.c).unwrap().into_iter().next().unwrap();
    let p = lib.paths.attachments.join(store::rel_path(&sha, &ext));
    store::remove_created(&lib.paths.attachments, std::slice::from_ref(&p));
    let r = export_of(&lib, &lib._dir.join("out"));
    assert_eq!(r.missing_files.len(), 1);
    assert!(r.checks.iter().all(|c| c.ok), "数据照常导出");
}

// ---------------- 从导出恢复 ----------------

#[test]
fn export_then_restore_reproduces_every_table() {
    let lib = rich_library("tr-roundtrip");
    let r = export_of(&lib, &lib._dir.join("out"));
    let before = dump(&lib.c).unwrap();

    // 恢复到另一个全新的库（换电脑 / 开发版 → 正式版）
    let other = temp_dir("tr-roundtrip-target");
    let (paths, mut live) = new_lib(&other.join("data"));
    edit::item_create(&live, "book", None).unwrap();
    let preview = inspect(Path::new(&r.path), &paths.attachments).unwrap();
    assert_eq!((preview.schema_version, preview.app_version.as_str(), preview.missing_files), (db::latest(), "0.1.0-test", 0));
    assert_eq!(preview.counts, r.counts);

    let p = prepare_export(Path::new(&r.path), Some(&paths.attachments), false).unwrap();
    let report = apply(&mut live, &paths.attachments, p, Some("x".into())).unwrap();
    assert!(report.checks.iter().all(|c| c.ok) && report.checks.len() == before.len());
    assert_eq!((report.from_schema, report.to_schema, report.missing_files), (db::latest(), db::latest(), 0));
    assert_eq!(report.files_copied, r.counts.files);
    assert_eq!(report.counts, r.counts);

    // 每张表逐行一模一样（含编号、时间、来源的原始数据）
    let after = dump(&live).unwrap();
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (t, rows) in &before {
        assert_eq!(&after[t].rows, &rows.rows, "{t}");
    }
    exercise(&live);
    // 附件：文件库里都有，只读
    for (sha, ext, _) in file_rows(&live).unwrap() {
        let p = paths.attachments.join(store::rel_path(&sha, &ext));
        assert!(std::fs::metadata(&p).unwrap().permissions().readonly());
    }

    // 再恢复一次同一份：文件已经在文件库里，不再复制
    let p = prepare_export(Path::new(&r.path), Some(&paths.attachments), false).unwrap();
    assert_eq!(apply(&mut live, &paths.attachments, p, None).unwrap().files_copied, 0);
}

#[test]
fn edited_or_foreign_exports_are_refused() {
    let lib = rich_library("tr-refuse");
    let r = export_of(&lib, &lib._dir.join("out"));
    let dir = PathBuf::from(&r.path);
    let json = dir.join(JSON_NAME);
    let original = std::fs::read_to_string(&json).unwrap();
    let store = &lib.paths.attachments;
    let refuse = |text: &str| -> String {
        std::fs::write(&json, text).unwrap();
        let e = inspect(&dir, store).unwrap_err();
        assert!(prepare_export(&dir, Some(store), false).is_err());
        e
    };

    let e = refuse(&original.replacen("国富论（合成）", "国富论（被改过）", 1));
    assert!(e.contains("文献") && e.contains("校验值对不上"), "{e}");
    let e = refuse(&original.replacen("\"formatVersion\": 1,", "\"formatVersion\": 2,", 1));
    assert!(e.contains("比这个 App 新"), "{e}");
    let e = refuse(&original.replacen(&format!("\"schemaVersion\": {},", db::latest()), &format!("\"schemaVersion\": {},", db::latest() + 1), 1));
    assert!(e.contains("比这个 App 新"), "{e}");
    let e = refuse(&original.replacen("\"research-lab-library\"", "\"pomelo-export\"", 1));
    assert!(e.contains("不是 Pith"), "{e}");
    let e = refuse(&original[..original.len() / 2]);
    assert!(e.contains("不完整或已损坏"), "{e}");
    // 去掉一张表（连同它的校验值）：缺表
    let mut v: J = serde_json::from_str(&original).unwrap();
    v["tables"].as_object_mut().unwrap().remove("reader_states");
    v["digests"].as_object_mut().unwrap().remove("reader_states");
    std::fs::write(&json, v.to_string()).unwrap();
    let e = prepare_export(&dir, Some(store), false).err().unwrap();
    assert!(e.contains("缺少「阅读位置」"), "{e}");

    std::fs::remove_file(&json).unwrap();
    assert!(inspect(&dir, store).unwrap_err().contains("不是 Pith 的整库导出"));
}

#[test]
fn missing_or_damaged_files_are_reported_not_fatal() {
    let lib = rich_library("tr-files");
    let r = export_of(&lib, &lib._dir.join("out"));
    let dir = PathBuf::from(&r.path);
    let files = file_rows(&lib.c).unwrap();
    // 删掉一个、改坏一个（大小不变）
    let (s0, e0, _) = &files[0];
    std::fs::remove_file(dir.join(FILES_DIR).join(store::rel_path(s0, e0))).unwrap();
    let (s1, e1, _) = &files[1];
    let p1 = dir.join(FILES_DIR).join(store::rel_path(s1, e1));
    let mut bytes = std::fs::read(&p1).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&p1, bytes).unwrap();

    let other = temp_dir("tr-files-target");
    let (paths, mut live) = new_lib(&other.join("data"));
    assert_eq!(inspect(&dir, &paths.attachments).unwrap().missing_files, 1, "预览只看大小");
    let p = prepare_export(&dir, Some(&paths.attachments), false).unwrap();
    let report = apply(&mut live, &paths.attachments, p, None).unwrap();
    assert_eq!((report.files_copied, report.missing_files), (files.len() - 2, 2), "坏文件不进文件库");
    assert!(!paths.attachments.join(store::rel_path(s1, e1)).exists());
    // 文件库里本来就有的（恢复到原来的库）：导出里缺也不算缺
    assert_eq!(inspect(&dir, &lib.paths.attachments).unwrap().missing_files, 0);
}

// ---------------- 升级 ----------------

fn maps(t: &BTreeMap<String, Table>) -> BTreeMap<String, Vec<serde_json::Map<String, J>>> {
    t.iter().map(|(k, v)| (k.clone(), v.rows.iter().map(|r| serde_json::from_str(r).unwrap()).collect())).collect()
}

/// 把完整的库投影到第 `v` 版：只留那时有的表和列（migration 只加不改，所以这就是当时的库会有的样子）
fn project(full: &BTreeMap<String, Table>, v: usize) -> (Connection, BTreeMap<String, Table>) {
    let mut c = Connection::open_in_memory().unwrap();
    db::configure(&c).unwrap();
    db::upgrade(&mut c, &db::MIGRATIONS[..v], None).unwrap();
    let written = load(&mut c, &maps(full), false).unwrap();
    (c, written)
}

#[test]
fn migrations_only_add() {
    // 投影的前提：migration 只建表、加列、建索引，不改、不删已有的数据
    for m in db::MIGRATIONS {
        let sql = m.sql.to_uppercase();
        for bad in ["UPDATE ", "DELETE ", "DROP ", "INSERT ", "RENAME "] {
            let hit = sql.lines().filter(|l| !l.trim_start().starts_with("--")).any(|l| l.contains(bad) && !l.contains("ON DELETE") && !l.contains("ON UPDATE"));
            assert!(!hit, "{:04}_{} 含有 {bad}：投影式的升级测试不再成立，需要为它单独写升级测试", m.version, m.name);
        }
    }
}

#[test]
fn every_old_version_upgrades_without_loss() {
    let lib = rich_library("tr-upgrade");
    let full = dump(&lib.c).unwrap();
    for v in 1..db::MIGRATIONS.len() {
        let (mut c, old) = project(&full, v);
        assert!(old.values().map(|t| t.rows.len()).sum::<usize>() > 0);
        assert_eq!(db::upgrade(&mut c, db::MIGRATIONS, None).unwrap(), db::latest());
        for (t, rows) in &old {
            let now = dump_table(&c, t, &rows.columns).unwrap();
            assert_eq!(now.digest(), rows.digest(), "第 {v} 版的「{}」升级后变了", label(t));
        }
        crate::search::attach_memory(&c).unwrap();
        exercise(&c);
    }
}

#[test]
fn old_version_export_restores_into_latest() {
    let lib = rich_library("tr-old-export");
    let (old, written) = project(&dump(&lib.c).unwrap(), 3);
    let out = lib._dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let r = export(&snapshot(&old).unwrap(), &lib.paths.attachments, &out, "0.1.0-old").unwrap();
    assert!(r.checks.iter().all(|c| c.ok));
    let other = temp_dir("tr-old-export-target");
    let (paths, mut live) = new_lib(&other.join("data"));
    let p = prepare_export(Path::new(&r.path), Some(&paths.attachments), false).unwrap();
    let report = apply(&mut live, &paths.attachments, p, None).unwrap();
    assert_eq!((report.from_schema, report.to_schema), (3, db::latest()));
    for (t, rows) in &written {
        assert_eq!(dump_table(&live, t, &rows.columns).unwrap().digest(), rows.digest(), "{t}");
    }
    exercise(&live);
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// 冻结的样本库：每个版本结束时用当时的代码生成（`freeze_fixture`），以后的版本都要能升级它。
#[test]
fn frozen_fixtures_upgrade_to_latest() {
    let mut seen = 0;
    for e in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = e.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(v) = name.strip_prefix("library-v").and_then(|n| n.strip_suffix(".json")) else { continue };
        let dir = temp_dir(&format!("tr-frozen-{v}"));
        std::fs::copy(&path, dir.join(JSON_NAME)).unwrap();
        let p = prepare_export(&dir, None, false).unwrap_or_else(|e| panic!("{name}：{e}"));
        assert_eq!(p.from_schema.to_string(), v);
        assert!(p.checks.iter().all(|c| c.ok));
        let (paths, mut live) = new_lib(&dir.join("data"));
        apply(&mut live, &paths.attachments, p, None).unwrap();
        assert_eq!(db::user_version(&live).unwrap(), db::latest());
        exercise(&live);
        seen += 1;
    }
    assert!(seen >= 1, "fixtures/ 里至少要有一份样本库");
}

/// 把当前版本的合成库存成样本库（新增 migration 之前跑一次，提交生成的文件）：
/// `cargo test -p pith freeze_fixture -- --ignored`
#[test]
#[ignore]
fn freeze_fixture() {
    let lib = rich_library("tr-freeze");
    let path = fixtures_dir().join(format!("library-v{}.json", db::latest()));
    assert!(!path.exists(), "{} 已存在：用过的样本库不改", path.display());
    let out = lib._dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let r = export(&snapshot(&lib.c).unwrap(), &lib.paths.attachments, &out, env!("CARGO_PKG_VERSION")).unwrap();
    // 临时目录的路径换成固定的占位（不把本机路径写进仓库），摘要随之重算
    let base = lib._dir.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(Path::new(&r.path).join(JSON_NAME)).unwrap().replace(&base, "/fixture");
    let dir = temp_dir("tr-freeze-check");
    std::fs::write(dir.join(JSON_NAME), &text).unwrap();
    let env = read_envelope(&dir).unwrap();
    let mut c = Connection::open_in_memory().unwrap();
    db::upgrade(&mut c, db::MIGRATIONS, None).unwrap();
    let written = load(&mut c, &env.tables, true).unwrap();
    // 保持一行一条记录的排版：只替换 digests 那一行
    let line = format!("\"digests\": {},", serde_json::to_string(&digests(&written)).unwrap());
    let fixed = text.lines().map(|l| if l.starts_with("\"digests\": ") { line.clone() } else { l.to_string() }).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::create_dir_all(fixtures_dir()).unwrap();
    std::fs::write(&path, fixed).unwrap();
    println!("已写出 {}", path.display());
}

// ---------------- 从备份恢复（只测 transfer 这一层；整个流程在 lib.rs 的测试里） ----------------

#[test]
fn backups_are_checked_before_restore() {
    let lib = rich_library("tr-backup-check");
    let b = &lib.paths.backups;
    // 第 2 版的旧备份：恢复时升到最新
    let (old, _) = project(&dump(&lib.c).unwrap(), 2);
    let name = "library-20260101-080000-manual.db";
    old.execute("VACUUM INTO ?1", [b.join(name).to_str().unwrap()]).unwrap();
    let p = prepare_backup(b, name).unwrap();
    assert_eq!(p.from_schema, 2);
    assert_eq!(db::user_version(&p.conn).unwrap(), db::latest());

    // 比 App 新、不是数据库、不是我们的库、名字不对 / 路径穿越：都拒绝
    let newer = "library-20260102-080000-manual.db";
    let c = Connection::open(b.join(newer)).unwrap();
    c.pragma_update(None, "user_version", db::latest() + 1).unwrap();
    c.execute_batch("CREATE TABLE items (x); CREATE TABLE attachments (x); CREATE TABLE files (x);").unwrap();
    drop(c);
    assert!(prepare_backup(b, newer).err().unwrap().contains("比这个 App 新"));
    std::fs::write(b.join("library-20260103-080000-manual.db"), b"not a database").unwrap();
    assert!(prepare_backup(b, "library-20260103-080000-manual.db").err().unwrap().contains("不能用"));
    let foreign = "library-20260104-080000-manual.db";
    let c = Connection::open(b.join(foreign)).unwrap();
    c.execute_batch("PRAGMA user_version = 3; CREATE TABLE notes (x);").unwrap();
    drop(c);
    assert!(prepare_backup(b, foreign).err().unwrap().contains("不是 Pith 的文献库"));
    for bad in ["../library.db", "library.db", "library-20260101-080000-manual.db/../x"] {
        assert!(prepare_backup(b, bad).is_err(), "{bad}");
    }
    assert!(prepare_backup(b, "library-20991231-080000-manual.db").err().unwrap().contains("找不到"));
}

/// 真实副本（只读）：导入到临时目录 → 整库导出（含自检）→ 恢复到另一个临时库 → 每张表逐行一致、文件都在。只打印汇总数字。
/// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith real_copy_export -- --ignored --nocapture`
#[test]
#[ignore]
fn real_copy_export() {
    let src = PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"));
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!src.starts_with(Path::new(&home).join("Zotero")), "不能直接读 ~/Zotero，请用副本");
    let dir = temp_dir("tr-real");
    let (paths, c) = new_lib(&dir.join("data"));
    crate::import::zotero::run(&c, &paths, &src, &crate::import::zotero::Options::default()).unwrap();
    let before = dump(&c).unwrap();
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let t = std::time::Instant::now();
    let r = export(&snapshot(&c).unwrap(), &paths.attachments, &out, "real").unwrap();
    let export_secs = t.elapsed().as_secs_f64();
    let json_kb = std::fs::metadata(Path::new(&r.path).join(JSON_NAME)).unwrap().len() / 1024;

    let (paths2, mut live) = new_lib(&dir.join("data2"));
    let t = std::time::Instant::now();
    let p = prepare_export(Path::new(&r.path), Some(&paths2.attachments), false).unwrap();
    let report = apply(&mut live, &paths2.attachments, p, None).unwrap();
    let restore_secs = t.elapsed().as_secs_f64();
    let after = dump(&live).unwrap();
    let same = before.iter().filter(|(t, rows)| after.get(*t).is_some_and(|a| a.rows == rows.rows)).count();
    exercise(&live);
    let rows: usize = before.values().map(|t| t.rows.len()).sum();
    println!(
        "文献 {} / 附件 {} / 笔记 {} / 标注 {} / 集合 {} / 文件 {} 个（{:.1} MB）；共 {} 张表 {rows} 行",
        r.counts.items,
        r.counts.attachments,
        r.counts.notes,
        r.counts.annotations,
        r.counts.collections,
        r.counts.files,
        r.counts.bytes as f64 / 1_048_576.0,
        before.len()
    );
    println!("导出 {export_secs:.1} 秒（library.json {json_kb} KB，自检 {} 类全部一致，缺文件 {}）", r.checks.len(), r.missing_files.len());
    println!("恢复 {restore_secs:.1} 秒：核对 {} 类{}，逐行一致的表 {same} / {}，复制文件 {}，缺文件 {}", report.checks.len(), if report.checks.iter().all(|c| c.ok) { "全部一致" } else { "有不一致" }, before.len(), report.files_copied, report.missing_files);
    assert_eq!(same, before.len());
    assert_eq!(report.missing_files, 0);
}
