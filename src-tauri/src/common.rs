//! 各模块共用的小工具：嵌套事务、对外稳定 ID（ULID）、错误转换。
// Tx 与 new_uid 从 Phase 2（导入）开始在正式代码里使用，Phase 1 只有测试用到
#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::Connection;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// 可嵌套的事务（SAVEPOINT）：模块函数互相调用、在导入的大事务里运行时，普通 BEGIN 会失败。
/// 没有 commit 就在离开作用域时回滚。
pub struct Tx<'c> {
    conn: &'c Connection,
    open: bool,
}

impl<'c> Tx<'c> {
    pub fn begin(conn: &'c Connection) -> Result<Self, String> {
        conn.execute_batch("SAVEPOINT rl_tx").map_err(err)?;
        Ok(Tx { conn, open: true })
    }

    pub fn commit(mut self) -> Result<(), String> {
        self.open = false;
        self.conn.execute_batch("RELEASE rl_tx").map_err(err)
    }
}

impl std::ops::Deref for Tx<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ = self.conn.execute_batch("ROLLBACK TO rl_tx; RELEASE rl_tx");
        }
    }
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// ULID：48 位毫秒时间戳 + 80 位随机数，Crockford Base32 共 26 位。按时间可排序。
/// 用作 items / collections / attachments / notes / annotations 的 `uid`（Word 引文、整库导出只认它）。
pub fn new_uid() -> Result<String, String> {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(err)?.as_millis() as u64;
    let mut rand = [0u8; 10];
    getrandom::fill(&mut rand).map_err(|e| format!("无法生成随机 ID：{e}"))?;
    Ok(encode_ulid(ms, rand))
}

fn encode_ulid(ms: u64, rand: [u8; 10]) -> String {
    let mut v: u128 = (ms as u128 & ((1 << 48) - 1)) << 80;
    for (i, b) in rand.iter().enumerate() {
        v |= (*b as u128) << (8 * (9 - i));
    }
    (0..26).rev().map(|i| CROCKFORD[((v >> (5 * i)) & 31) as usize] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_shape_and_order() {
        assert_eq!(encode_ulid(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(encode_ulid((1 << 48) - 1, [255; 10]), "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
        let a = encode_ulid(1_794_000_000_000, [9; 10]);
        let b = encode_ulid(1_794_000_000_001, [0; 10]);
        assert!(a < b, "时间靠后的 ID 排序靠后");
        let u = new_uid().unwrap();
        assert_eq!(u.len(), 26);
        assert!(u.bytes().all(|c| CROCKFORD.contains(&c)));
        assert_ne!(new_uid().unwrap(), u);
    }

    #[test]
    fn tx_rolls_back_unless_committed() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE t (x INTEGER) STRICT").unwrap();
        {
            let tx = Tx::begin(&c).unwrap();
            tx.execute("INSERT INTO t VALUES (1)", []).unwrap();
        }
        {
            let outer = Tx::begin(&c).unwrap();
            outer.execute("INSERT INTO t VALUES (2)", []).unwrap();
            {
                let inner = Tx::begin(&outer).unwrap();
                inner.execute("INSERT INTO t VALUES (3)", []).unwrap();
                inner.commit().unwrap();
            }
            outer.commit().unwrap();
        }
        let n: i64 = c.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }
}
