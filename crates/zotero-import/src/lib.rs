//! Phase 0-A：Zotero 迁移原型。
//!
//! - `reader`：只读读取 zotero.sqlite + storage/，得到 `model::ZoteroLibrary`
//! - `audit`：用独立的 SQL 查询逐类核对「源数据 vs 读出的模型」，给出误差报告
//! - `fixture`：生成合成 Zotero 数据目录（测试用，不含任何真实数据）

pub mod audit;
pub mod fixture;
pub mod model;
pub mod reader;
pub mod util;

use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Io(std::io::Error, PathBuf),
    Json(serde_json::Error),
    Schema(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sqlite(e) => write!(f, "SQLite 错误：{e}"),
            Error::Io(e, p) => write!(f, "文件错误（{}）：{e}", p.display()),
            Error::Json(e) => write!(f, "JSON 错误：{e}"),
            Error::Schema(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
