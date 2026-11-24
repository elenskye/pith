//! 快照备份：`backups/library-YYYYMMDD-HHMMSS-<标签>[-n].db`，用 `VACUUM INTO` 写出（WAL 下也是一致的单文件）。
//! 文件名就是全部元数据。标签：`daily`（每天第一次打开自动做，保留最近 30 份 + 每月最新一份）、
//! `manual`（手动，永不自动删）、`before-vN`（升级前）、`before-import`（导入前）、`before-restore`（恢复前）、`before-delete`（永久删除前）、`before-metadata`（联网补全前）；
//! 各类安全副本各保留最近 3 份。
//! 备份只有数据库，附件文件在文件库里共用：还有备份用到的文件不删（`sweep_files`），恢复任何一份备份附件都在。

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const PREFIX: &str = "library";
pub const DAILY: &str = "daily";
pub const MANUAL: &str = "manual";
pub const KEEP_DAILY: usize = 30;
pub const KEEP_SAFETY: usize = 3;
pub const BEFORE_RESTORE: &str = "before-restore";

/// 本地时间 `YYYYMMDD-HHMMSS`（SQLite 算，不引入时间库）
pub fn local_stamp() -> Result<String, String> {
    Connection::open_in_memory()
        .and_then(|c| c.query_row("SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')", [], |r| r.get(0)))
        .map_err(|e| e.to_string())
}

/// `library-<时间>-<label>[-n].db` 里还没被占用的那个
fn free_name(dir: &Path, label: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建备份目录：{e}"))?;
    let stamp = local_stamp()?;
    let mut path = dir.join(format!("{PREFIX}-{stamp}-{label}.db"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{PREFIX}-{stamp}-{label}-{n}.db"));
    }
    Ok(path)
}

/// 写出 `library-<时间>-<label>.db`，从不覆盖已有文件。
pub fn snapshot(conn: &Connection, dir: &Path, label: &str) -> Result<PathBuf, String> {
    let path = free_name(dir, label)?;
    let target = path.to_str().ok_or("备份路径包含无法识别的字符")?;
    conn.execute("VACUUM INTO ?1", [target]).map_err(|e| format!("备份失败：{e}"))?;
    Ok(path)
}

/// 主库打不开（损坏 / 版本比 App 新）时：把 library.db（连同 -wal / -shm）原样挪进备份目录，记为「恢复前」。
pub fn set_aside(library_db: &Path, dir: &Path) -> Result<Option<PathBuf>, String> {
    if !library_db.exists() {
        return Ok(None);
    }
    let path = free_name(dir, BEFORE_RESTORE)?;
    std::fs::rename(library_db, &path).map_err(|e| format!("无法挪开打不开的数据库：{e}"))?;
    for suffix in ["-wal", "-shm"] {
        let side = sidecar(library_db, suffix);
        if side.exists() {
            std::fs::rename(&side, sidecar(&path, suffix)).map_err(|e| format!("无法挪开打不开的数据库：{e}"))?;
        }
    }
    Ok(Some(path))
}

/// `set_aside` 的反向：恢复失败时把原来的库放回去。
pub fn put_back(aside: &Path, library_db: &Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(sidecar(library_db, suffix));
    }
    for suffix in ["", "-wal", "-shm"] {
        let from = sidecar(aside, suffix);
        if from.exists() {
            std::fs::rename(&from, sidecar(library_db, suffix)).map_err(|e| format!("恢复失败，原来的数据库在 {}：{e}", aside.display()))?;
        }
    }
    Ok(())
}

fn sidecar(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_with_sidecars(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("清理旧备份失败：{e}"))?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(sidecar(path, suffix));
    }
    Ok(())
}

/// 按文件名找备份：只认备份目录里、名字合规的文件（防路径穿越）。
pub fn resolve(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    if file_name.contains(['/', '\\']) || parse_name(file_name).is_none() {
        return Err("这不是 Pith 的备份文件".into());
    }
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("找不到这份备份，可能已经被清理了".into());
    }
    Ok(path)
}

/// 只读打开一个数据库文件并检查：完整、是本 App 的库、版本不比 App 新。返回连接与版本。
pub fn open_checked(path: &Path, latest: i64) -> Result<(Connection, i64), String> {
    let bad = |e: String| format!("这份数据库不能用：{e}");
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| bad(e.to_string()))?;
    let check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| bad(e.to_string()))?;
    if check != "ok" {
        return Err(bad("文件已损坏".into()));
    }
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| bad(e.to_string()))?;
    let ours: bool = c
        .query_row("SELECT COUNT(*) = 3 FROM sqlite_master WHERE type = 'table' AND name IN ('items', 'attachments', 'files')", [], |r| r.get(0))
        .map_err(|e| bad(e.to_string()))?;
    if v < 1 || !ours {
        return Err(bad("里面不是 Pith 的文献库".into()));
    }
    if v > latest {
        return Err(bad(format!("它的数据库版本（第 {v} 版）比这个 App 新（支持到第 {latest} 版），请用新版 App 恢复")));
    }
    Ok((c, v))
}

/// 整库复制：`dst` 的主库内容整体换成 `src`（SQLite backup API，一步完成；失败时 `dst` 不变）。
pub fn copy_into(src: &Connection, dst: &mut Connection) -> Result<(), String> {
    let b = rusqlite::backup::Backup::new(src, dst).map_err(|e| format!("恢复失败：{e}"))?;
    match b.step(-1).map_err(|e| format!("恢复失败：{e}"))? {
        rusqlite::backup::StepResult::Done => Ok(()),
        _ => Err("恢复失败：数据库正忙，请稍后再试".into()),
    }
}

/// 还要留着的附件文件：现在的库与每一份备份的 `files` 表。有备份读不了就报错（宁可不删）。
pub fn files_in_use(conn: &Connection, dir: &Path) -> Result<HashSet<String>, String> {
    let read = |c: &Connection| -> rusqlite::Result<Vec<String>> { c.prepare("SELECT sha256 FROM files")?.query_map([], |r| r.get(0))?.collect() };
    let mut keep: HashSet<String> = read(conn).map_err(|e| e.to_string())?.into_iter().collect();
    for (_, path) in scan(dir)? {
        let c = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| format!("读不了备份 {}：{e}", path.display()))?;
        keep.extend(read(&c).map_err(|e| format!("读不了备份 {}：{e}", path.display()))?);
    }
    Ok(keep)
}

/// 清理文件库里谁都不用的文件（现在的库和所有备份都没有记录）。返回删掉的个数。
pub fn sweep_files(conn: &Connection, dir: &Path, attachments: &Path) -> Result<usize, String> {
    let keep = files_in_use(conn, dir)?;
    Ok(crate::store::sweep(attachments, &keep))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    /// `YYYYMMDD-HHMMSS`（本地时间）
    stamp: String,
    label: String,
    /// 同一秒内的序号（1 = 没有重名）
    n: u32,
}

fn parse_name(file: &str) -> Option<Name> {
    let rest = file.strip_prefix(PREFIX)?.strip_prefix('-')?.strip_suffix(".db")?;
    let (stamp, label) = (rest.get(..15)?, rest.get(15..)?.strip_prefix('-')?);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !(digits(&stamp[..8]) && stamp.as_bytes()[8] == b'-' && digits(&stamp[9..])) || label.is_empty() {
        return None;
    }
    // 标签本身不以「-数字」结尾，结尾的数字就是重名序号
    let (label, n) = match label.rsplit_once('-') {
        Some((l, n)) if digits(n) && !l.is_empty() => (l, n.parse().ok()?),
        _ => (label, 1),
    };
    Some(Name { stamp: stamp.into(), label: label.into(), n })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file_name: String,
    /// daily / manual / before-import / before-restore / before-vN；界面再翻成中文
    pub label: String,
    /// `YYYY-MM-DD HH:MM:SS`，本地时间
    pub created_at: String,
    pub size: u64,
}

/// 新的在前
fn scan(dir: &Path) -> Result<Vec<(Name, PathBuf)>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("读不了备份目录：{e}")),
    };
    let mut out: Vec<(Name, PathBuf)> = entries.filter_map(Result::ok).filter_map(|e| Some((parse_name(e.file_name().to_str()?)?, e.path()))).collect();
    out.sort_by(|a, b| (&b.0.stamp, b.0.n).cmp(&(&a.0.stamp, a.0.n)));
    Ok(out)
}

fn info(name: &Name, path: &Path) -> BackupInfo {
    let s = &name.stamp;
    BackupInfo {
        file_name: path.file_name().and_then(|f| f.to_str()).unwrap_or_default().into(),
        label: name.label.clone(),
        created_at: format!("{}-{}-{} {}:{}:{}", &s[..4], &s[4..6], &s[6..8], &s[9..11], &s[11..13], &s[13..15]),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    }
}

pub fn list(dir: &Path) -> Result<Vec<BackupInfo>, String> {
    Ok(scan(dir)?.iter().map(|(n, p)| info(n, p)).collect())
}

pub fn info_of(path: &Path) -> Result<BackupInfo, String> {
    let file = path.file_name().and_then(|f| f.to_str()).unwrap_or_default();
    let name = parse_name(file).ok_or_else(|| format!("备份文件名不对：{file}"))?;
    Ok(info(&name, path))
}

/// daily：保留最新 `keep` 份，另外每个月最新的一份永久保留。返回删除的文件。
pub fn prune_daily(dir: &Path, keep: usize) -> Result<Vec<PathBuf>, String> {
    let mut months = HashSet::new();
    let mut deleted = Vec::new();
    for (i, (name, path)) in scan(dir)?.into_iter().filter(|(n, _)| n.label == DAILY).enumerate() {
        let newest_of_month = months.insert(name.stamp[..6].to_string());
        if i < keep || newest_of_month {
            continue;
        }
        remove_with_sidecars(&path)?;
        deleted.push(path);
    }
    Ok(deleted)
}

fn safety_kind(label: &str) -> Option<&'static str> {
    match label {
        "before-import" => Some("import"),
        BEFORE_RESTORE => Some("restore"),
        "before-delete" => Some("delete"),
        "before-metadata" => Some("metadata"),
        _ => label.strip_prefix("before-v").filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).map(|_| "upgrade"),
    }
}

/// 安全副本（升级前 / 导入前 / 恢复前）每类保留最新 `keep` 份。
pub fn prune_safety(dir: &Path, keep: usize) -> Result<Vec<PathBuf>, String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut deleted = Vec::new();
    for (name, path) in scan(dir)? {
        let Some(kind) = safety_kind(&name.label) else { continue };
        let n = seen.entry(kind).or_default();
        *n += 1;
        if *n > keep {
            remove_with_sidecars(&path)?;
            deleted.push(path);
        }
    }
    Ok(deleted)
}

/// 今天还没有 daily 快照就做一份，然后清理旧备份。返回新快照。
pub fn daily(conn: &Connection, dir: &Path) -> Result<Option<PathBuf>, String> {
    let today: String = conn.query_row("SELECT strftime('%Y%m%d', 'now', 'localtime')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if scan(dir)?.iter().any(|(n, _)| n.label == DAILY && n.stamp.starts_with(&today)) {
        return Ok(None);
    }
    let path = snapshot(conn, dir, DAILY)?;
    prune_daily(dir, KEEP_DAILY)?;
    prune_safety(dir, KEEP_SAFETY)?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, tests::temp_dir};

    fn file_db(dir: &Path) -> Connection {
        let mut c = db::open(&dir.join("library.db")).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &dir.join("backups")).unwrap();
        c
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(parse_name("library-20261108-093000-daily.db"), Some(Name { stamp: "20261108-093000".into(), label: "daily".into(), n: 1 }));
        assert_eq!(parse_name("library-20261108-093000-manual-2.db").unwrap().n, 2);
        assert_eq!(parse_name("library-20261108-093000-before-v2.db").unwrap().label, "before-v2");
        assert_eq!(parse_name("library-20261108-093000-before-import.db").unwrap().label, "before-import");
        for bad in ["library-2026-daily.db", "pomelo-20261108-093000-daily.db", "library-20261108-093000-.db", "library-20261108-093000-daily.txt"] {
            assert!(parse_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn snapshot_is_a_complete_database() {
        let dir = temp_dir("backup-snap");
        let c = file_db(&dir);
        c.execute("INSERT INTO settings (key, value) VALUES ('k', 'v')", []).unwrap();
        let p = snapshot(&c, &dir.join("backups"), MANUAL).unwrap();
        let p2 = snapshot(&c, &dir.join("backups"), MANUAL).unwrap();
        assert_ne!(p, p2, "同一秒内不覆盖");
        let snap = Connection::open(&p).unwrap();
        assert_eq!(db::user_version(&snap).unwrap(), db::latest());
        let v: String = snap.query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "v");
        let all = list(&dir.join("backups")).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|b| b.label == MANUAL && b.size > 0));
    }

    #[test]
    fn daily_once_per_day() {
        let dir = temp_dir("backup-daily");
        let c = file_db(&dir);
        let b = dir.join("backups");
        assert!(daily(&c, &b).unwrap().is_some());
        assert!(daily(&c, &b).unwrap().is_none());
        snapshot(&c, &b, MANUAL).unwrap();
        assert!(daily(&c, &b).unwrap().is_none());
        assert_eq!(list(&b).unwrap().len(), 2);
    }

    #[test]
    fn pruning_keeps_recent_monthly_and_manual() {
        let dir = temp_dir("backup-prune");
        let b = dir.join("b");
        // 两个月里各有若干 daily
        for d in 1..=20 {
            touch(&b, &format!("library-202609{d:02}-080000-daily.db"));
            touch(&b, &format!("library-202610{d:02}-080000-daily.db"));
        }
        touch(&b, "library-20260801-080000-manual.db");
        let deleted = prune_daily(&b, 5).unwrap();
        let left: Vec<String> = list(&b).unwrap().into_iter().map(|i| i.file_name).collect();
        assert_eq!(deleted.len(), 40 - 5 - 1, "保留最近 5 份 + 9 月最新一份");
        assert!(left.contains(&"library-20260920-080000-daily.db".to_string()));
        assert!(left.contains(&"library-20261020-080000-daily.db".to_string()));
        assert!(left.contains(&"library-20260801-080000-manual.db".to_string()));

        for d in 1..=5 {
            touch(&b, &format!("library-202611{d:02}-080000-before-import.db"));
            touch(&b, &format!("library-202611{d:02}-090000-before-v{d}.db"));
        }
        assert_eq!(prune_safety(&b, 3).unwrap().len(), 4);
        let left = list(&b).unwrap();
        assert_eq!(left.iter().filter(|i| i.label == "before-import").count(), 3);
        assert_eq!(left.iter().filter(|i| i.label.starts_with("before-v")).count(), 3);
        assert_eq!(left[0].created_at, "2026-11-05 09:00:00", "新的在前");
    }
}
