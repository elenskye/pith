//! 打开数据库与升级（migration）。
//! 规则：migration 只追加、用过的永不修改；版本 1..=N 连续；版本号存在 `PRAGMA user_version`。
//! 升级前自动备份；数据库比 App 新就拒绝打开（防止旧版覆盖新数据）。

use crate::backup;
use rusqlite::Connection;
use std::path::Path;

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "library", sql: include_str!("../migrations/0001_library.sql") },
    Migration { version: 2, name: "import", sql: include_str!("../migrations/0002_import.sql") },
    Migration { version: 3, name: "manage", sql: include_str!("../migrations/0003_manage.sql") },
    Migration { version: 4, name: "reader", sql: include_str!("../migrations/0004_reader.sql") },
    Migration { version: 5, name: "metadata", sql: include_str!("../migrations/0005_metadata.sql") },
    Migration { version: 6, name: "research", sql: include_str!("../migrations/0006_research.sql") },
];

pub fn latest() -> i64 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("无法打开数据库：{e}"))?;
    configure(&conn)?;
    Ok(conn)
}

pub fn configure(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )
    .map_err(|e| e.to_string())
}

pub fn user_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())
}

/// 执行未应用的 migration，返回升级后的版本。已有数据（版本 > 0）时先备份到 `backups_dir`。
pub fn migrate(conn: &mut Connection, migrations: &[Migration], backups_dir: &Path) -> Result<i64, String> {
    upgrade(conn, migrations, Some(backups_dir))
}

/// 同 `migrate`；`backups_dir` 为 None 时不做升级前备份（恢复用的临时库）。
pub fn upgrade(conn: &mut Connection, migrations: &[Migration], backups_dir: Option<&Path>) -> Result<i64, String> {
    let current = user_version(conn)?;
    let latest = migrations.last().map_or(0, |m| m.version);
    if current > latest {
        return Err(format!("数据库版本（{current}）比这个 App 支持的版本（{latest}）新。请用新版 App 打开，不要用旧版覆盖数据。"));
    }
    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    let Some(first) = pending.first() else {
        return Ok(current);
    };
    if let Some(dir) = backups_dir.filter(|_| current > 0) {
        backup::snapshot(conn, dir, &format!("before-v{}", first.version))?;
    }
    for m in pending {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(m.sql).map_err(|e| format!("数据库升级 {:04}_{} 失败：{e}", m.version, m.name))?;
        tx.pragma_update(None, "user_version", m.version).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(latest)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    /// $TMPDIR 下的新目录，离开作用域（含 panic）时删除。
    pub struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn temp_dir(name: &str) -> TempDir {
        let d = std::env::temp_dir().join(format!("researchlab-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }

    pub fn memory_db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        configure(&c).unwrap();
        migrate(&mut c, MIGRATIONS, &std::env::temp_dir()).unwrap(); // 新库：不备份
        crate::search::attach_memory(&c).unwrap();
        c
    }

    #[test]
    fn migrations_are_sequential() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, i as i64 + 1, "{} 编号不连续", m.name);
            assert!(!m.sql.trim().is_empty());
        }
    }

    #[test]
    fn fresh_db_migrates_without_backup() {
        let dir = temp_dir("fresh");
        let mut c = open(&dir.join("library.db")).unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest());
        assert_eq!(user_version(&c).unwrap(), latest());
        assert!(!dir.join("backups").exists());
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest(), "重复执行不出错");
    }

    #[test]
    fn schema_has_all_tables_strict_with_foreign_keys() {
        let c = memory_db();
        let tables: Vec<(String, String)> = c
            .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let names: Vec<&str> = tables.iter().map(|(n, _)| n.as_str()).collect();
        for t in [
            "items", "item_fields", "creators", "item_relations", "collections", "collection_items", "tags", "item_tags", "files", "attachments", "notes",
            "annotations", "import_runs", "settings", "attachment_tags", "note_tags", "annotation_tags", "collection_attachments", "collection_notes",
            "duplicate_dismissals", "reader_states", "metadata_lookups", "research_questions", "research_runs", "research_searches", "research_sources", "research_facts",
            "research_calcs", "research_conclusions", "research_conclusion_facts", "research_steps",
        ] {
            assert!(names.contains(&t), "缺表 {t}");
        }
        assert_eq!(names.len(), 31);
        assert!(tables.iter().all(|(_, sql)| sql.trim_end().ends_with("STRICT")), "全部是 STRICT 表");
        // 外键生效：不存在的条目不能挂作者
        let bad = c.execute("INSERT INTO creators (item_id, role, position) VALUES (999, 'author', 0)", []);
        assert!(bad.is_err());
        // STRICT 生效：整数列不收文本
        let bad = c.execute("INSERT INTO items (uid, item_type, year, created_at, updated_at) VALUES ('x', 'book', 'abc', 't', 't')", []);
        assert!(bad.is_err());
    }

    #[test]
    fn upgrade_backs_up_first() {
        let dir = temp_dir("upgrade");
        let backups = dir.join("backups");
        let v1 = [Migration { version: 1, name: "a", sql: "CREATE TABLE a (x INTEGER) STRICT; INSERT INTO a VALUES (7);" }];
        let v2 = [Migration { version: 1, name: "a", sql: v1[0].sql }, Migration { version: 2, name: "b", sql: "CREATE TABLE b (y TEXT) STRICT;" }];
        let mut c = open(&dir.join("library.db")).unwrap();
        migrate(&mut c, &v1, &backups).unwrap();
        assert_eq!(migrate(&mut c, &v2, &backups).unwrap(), 2);
        let files: Vec<_> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().ends_with("-before-v2.db"));
        let snap = Connection::open(&files[0]).unwrap();
        assert_eq!(user_version(&snap).unwrap(), 1);
        let x: i64 = snap.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 7);
    }

    /// 真实的升级路径：第 1 版库里已有数据 → 升到最新，数据不丢、先做 before-v2 备份、新列为空。
    #[test]
    fn version_1_with_data_upgrades_to_latest() {
        let dir = temp_dir("upgrade-real");
        let backups = dir.join("backups");
        let mut c = open(&dir.join("library.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..1], &backups).unwrap();
        c.execute(
            "INSERT INTO items (uid, item_type, title, source, created_at, updated_at) VALUES ('U1', 'book', '旧数据', 'manual', 't', 't')",
            [],
        )
        .unwrap();
        c.execute("INSERT INTO attachments (uid, item_id, kind, created_at, updated_at) VALUES ('A1', 1, 'linked_url', 't', 't')", []).unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &backups).unwrap(), latest());
        let (title, meta): (String, Option<String>) = c.query_row("SELECT title, source_meta FROM items WHERE uid = 'U1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((title.as_str(), meta), ("旧数据", None));
        let note_id: Option<i64> = c.query_row("SELECT note_id FROM attachments WHERE uid = 'A1'", [], |r| r.get(0)).unwrap();
        assert_eq!(note_id, None);
        let files: Vec<String> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(files.len() == 1 && files[0].ends_with("-before-v2.db"), "{files:?}");
    }

    #[test]
    fn newer_database_is_refused() {
        let dir = temp_dir("newer");
        let mut c = open(&dir.join("library.db")).unwrap();
        c.pragma_update(None, "user_version", latest() + 1).unwrap();
        let e = migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap_err();
        assert!(e.contains("比这个 App 支持的版本"), "{e}");
    }

    #[test]
    fn failed_migration_leaves_version_unchanged() {
        let dir = temp_dir("failed");
        let bad = [Migration { version: 1, name: "ok", sql: "CREATE TABLE a (x INTEGER) STRICT;" }, Migration { version: 2, name: "bad", sql: "CREATE TABLE a (y TEXT);" }];
        let mut c = open(&dir.join("library.db")).unwrap();
        assert!(migrate(&mut c, &bad, &dir.join("backups")).is_err());
        assert_eq!(user_version(&c).unwrap(), 1, "失败的那一步整体回滚");
    }
}
