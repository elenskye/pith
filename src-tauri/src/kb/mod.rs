//! 知识库（1.0.2 建表，1.1.0 第 3 期能检索，`decisions/0031`、`0038`）。
//!
//! - `store`：知识库和材料（一份材料能进几个库）、能种进来的东西、1.1.0 的升级（同一来源并成一份、解析好的书迁成全文材料）。
//! - `passages`：材料切成段落块（书是一段一块，带段落号、页码、章）；读一章、上下文、目录。
//! - `index`：`search.db` 里的按词索引和本机向量（派生数据），后台补齐向量。
//! - `retrieve`：按意思 + 按词 → 重排，带上对照译文（`kb_search` 工具）。
//! - `read`：读原文（`kb_read` 工具、面板、子代理读整章）。
//! - `cite`：回答里的 `[P123]` → `[1]` 和页边注。
//! - `align`：原著和译本都有向量时，用相似度重新对齐。
//! - `commands`：`kb_*` 命令。

pub mod align;
pub mod cite;
pub mod commands;
pub mod index;
pub mod passages;
pub mod read;
pub mod retrieve;
pub mod store;

#[cfg(test)]
mod tests;

use rusqlite::Connection;

/// 拿一下数据库连接（App 里是 `AppState` 的锁；测试里是一个连接）。检索、补向量时联本机模型不拿锁
pub trait Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String>;
}

impl Db for crate::AppState {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_db(|c| f(c))
    }
}

impl Db for std::sync::Mutex<Connection> {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let guard = self.lock().map_err(|_| "数据库连接异常".to_string())?;
        f(&guard)
    }
}

/// 启动时（和整库恢复以后）：升级（家进 `kb_members`、并重复、迁书、补切段落块），再让按词的索引对上主库。
/// 尽力而为：失败只打印，不挡启动
pub fn on_startup(c: &Connection) {
    // 一个事务：第一次打开 1.1.0 要抄一万多段书，一次提交快得多；中途出错整个不算，下次启动再来
    let upgraded = crate::common::Tx::begin(c).and_then(|tx| {
        let n = store::upgrade(&tx, &crate::text::now_iso())?;
        tx.commit()?;
        Ok(n)
    });
    match upgraded {
        Ok(n) if n > 0 => eprintln!("解析好的书迁进知识库：{n} 本"),
        Ok(_) => {}
        Err(e) => eprintln!("知识库升级失败：{e}"),
    }
    if let Err(e) = index::ensure(c) {
        eprintln!("知识库的检索索引不可用：{e}");
    }
}
