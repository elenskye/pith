#![allow(dead_code)]
use std::path::PathBuf;
use zotero_import::fixture::{self, FixturePaths};

/// 测试临时目录：<tmp>/researchlab-test-<pid>-<name>，每次重建。
pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("researchlab-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

pub fn fixture(name: &str) -> FixturePaths {
    fixture::build(&temp_dir(name)).expect("生成夹具")
}
