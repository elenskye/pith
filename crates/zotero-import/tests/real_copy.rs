//! 真实 Zotero 库副本上的测试。默认忽略；需要设置环境变量：
//!   ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p zotero-import -- --ignored
//! 只读：测试前后核对数据库文件哈希不变。

mod common;

use std::path::PathBuf;
use zotero_import::reader::{self, ReadOptions};
use zotero_import::util::hash_file;
use zotero_import::{audit, fixture};

fn copy_dir() -> PathBuf {
    PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"))
}

fn columns(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut st = conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid").unwrap();
    st.query_map([table], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
}

/// 合成夹具的表结构与真实库逐表逐列一致（列名与顺序）。
#[test]
#[ignore]
fn fixture_columns_match_real_schema() {
    let real = reader::open_readonly(&reader::db_path(&copy_dir())).unwrap();
    let p = common::fixture("parity");
    let fx = reader::open_readonly(&reader::db_path(&p.zotero_dir)).unwrap();
    let mut checked = 0;
    for t in fixture::fixture_tables() {
        let rc = columns(&real, &t);
        assert!(!rc.is_empty(), "真实库没有表 {t}");
        assert_eq!(columns(&fx, &t), rc, "表 {t} 的列与真实库不一致");
        checked += 1;
    }
    assert!(checked >= 30);
}

#[test]
#[ignore]
fn real_copy_reads_without_loss_and_stays_unchanged() {
    let dir = copy_dir();
    let db = reader::db_path(&dir);
    let before = hash_file(&db).unwrap();
    let lib = reader::read_library(&dir, &ReadOptions::default()).unwrap();
    let conn = reader::open_readonly(&db).unwrap();
    let r = audit::audit(&conn, &lib, &reader::storage_path(&dir)).unwrap();
    for c in &r.checks {
        assert!(c.ok, "类别 {} 不一致：源 {} 模型 {}", c.category, c.source_count, c.parsed_count);
    }
    drop(conn);
    assert_eq!(hash_file(&db).unwrap(), before, "副本数据库被改动");
}
