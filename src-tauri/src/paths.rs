//! 数据目录（decisions/0003）。正式版用 ~/Pith/；调试构建（tauri dev、cargo test）用 ~/Pith Dev/，
//! 开发期间绝不碰正式目录。环境变量 PITH_DATA_DIR 可覆盖位置（临时运行、测量）。

use std::path::{Path, PathBuf};

pub const DATA_DIR_ENV: &str = "PITH_DATA_DIR";

pub fn is_dev() -> bool {
    cfg!(debug_assertions)
}

pub fn data_dir_name() -> &'static str {
    if is_dev() { "Pith Dev" } else { "Pith" }
}

/// `home` = 用户主目录。
pub fn resolve_root(home: &Path, env_override: Option<PathBuf>) -> PathBuf {
    env_override.filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| home.join(data_dir_name()))
}

#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub library_db: PathBuf,
    /// 搜索索引：派生数据，可删了重建，不进备份
    pub search_db: PathBuf,
    /// 附件按 SHA-256 存放：attachments/<前 2 位>/<sha256>.<ext>
    pub attachments: PathBuf,
    pub backups: PathBuf,
    pub exports: PathBuf,
}

impl DataPaths {
    pub fn new(root: PathBuf) -> Self {
        Self {
            library_db: root.join("library.db"),
            search_db: root.join("search.db"),
            attachments: root.join("attachments"),
            backups: root.join("backups"),
            exports: root.join("exports"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<(), String> {
        for d in [&self.root, &self.attachments, &self.backups, &self.exports] {
            std::fs::create_dir_all(d).map_err(|e| format!("无法创建数据目录 {}：{e}", d.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_builds_use_dev_dir() {
        assert_eq!(data_dir_name(), "Pith Dev");
        let home = Path::new("/Users/x");
        assert_eq!(resolve_root(home, None), PathBuf::from("/Users/x/Pith Dev"));
        assert_eq!(resolve_root(home, Some("/tmp/rl".into())), PathBuf::from("/tmp/rl"));
        assert_eq!(resolve_root(home, Some("".into())), PathBuf::from("/Users/x/Pith Dev"));
    }

    #[test]
    fn layout() {
        let p = DataPaths::new("/r".into());
        assert_eq!(p.library_db, PathBuf::from("/r/library.db"));
        assert_eq!(p.search_db, PathBuf::from("/r/search.db"));
        assert_eq!(p.attachments, PathBuf::from("/r/attachments"));
        assert_eq!(p.backups, PathBuf::from("/r/backups"));
        assert_eq!(p.exports, PathBuf::from("/r/exports"));
    }
}
