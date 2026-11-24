//! zotero-probe：Phase 0-A 命令行工具。
//!
//!   zotero-probe read <zotero_dir> --out <dir> [--base-dir <dir>] [--no-hash]
//!   zotero-probe fixture <empty_dir>
//!
//! read 输出 library.json（完整中间模型，可能含个人数据）、report.json、report.md（只含统计与 key）。

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use zotero_import::{audit, fixture, reader};

fn usage() -> ExitCode {
    eprintln!("用法：\n  zotero-probe read <zotero_dir> --out <dir> [--base-dir <dir>] [--no-hash]\n  zotero-probe fixture <empty_dir>");
    ExitCode::from(64)
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// 拒绝直接读取正在使用的 Zotero 数据目录（~/Zotero），只接受副本。
fn guard_live_library(dir: &Path) -> Result<(), String> {
    let Some(home) = std::env::var_os("HOME") else { return Ok(()) };
    let live = canon(&PathBuf::from(home).join("Zotero"));
    if canon(dir) == live {
        return Err(format!("拒绝读取正在使用的 Zotero 数据目录 {}。请先按 docs/zotero-copy-guide.md 导出副本。", live.display()));
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fixture") => {
            let Some(dir) = args.get(1) else { return usage() };
            match fixture::build(Path::new(dir)) {
                Ok(p) => {
                    println!("已生成合成 Zotero 数据目录：{}", p.zotero_dir.display());
                    println!("链接附件根目录（--base-dir）：{}", p.base_dir.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("生成失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("read") => {
            let Some(dir) = args.get(1).map(PathBuf::from) else { return usage() };
            let mut out: Option<PathBuf> = None;
            let mut opts = reader::ReadOptions::default();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--out" => {
                        out = args.get(i + 1).map(PathBuf::from);
                        i += 1;
                    }
                    "--base-dir" => {
                        opts.base_attachment_dir = args.get(i + 1).map(PathBuf::from);
                        i += 1;
                    }
                    "--no-hash" => opts.skip_hashing = true,
                    _ => return usage(),
                }
                i += 1;
            }
            let Some(out) = out else { return usage() };
            if let Err(e) = guard_live_library(&dir) {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
            if canon(&out).starts_with(canon(&dir)) {
                eprintln!("输出目录不能放在 Zotero 数据目录里面。");
                return ExitCode::FAILURE;
            }
            match run_read(&dir, &out, &opts) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("读取失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => usage(),
    }
}

fn run_read(dir: &Path, out: &Path, opts: &reader::ReadOptions) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let started = std::time::Instant::now();
    let lib = reader::read_library(dir, opts)?;
    let conn = reader::open_readonly(&reader::db_path(dir))?;
    let report = audit::audit(&conn, &lib, &reader::storage_path(dir))?;
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("library.json"), serde_json::to_string_pretty(&lib)?)?;
    std::fs::write(out.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    std::fs::write(out.join("report.md"), audit::render_markdown(&report))?;
    let ms = started.elapsed().as_millis();
    println!(
        "结论：{}｜普通条目 {}、附件 {}、笔记 {}、标注 {}、集合 {}｜文件问题 {}｜耗时 {ms} ms",
        report.verdict, lib.items.len(), lib.attachments.len(), lib.notes.len(), lib.annotations.len(), lib.collections.len(), report.file_issues.len()
    );
    for c in report.checks.iter().filter(|c| !c.ok) {
        println!("  ✗ {}：源 {}，模型 {}", c.label, c.source_count, c.parsed_count);
    }
    println!("输出：{}", out.display());
    Ok(if report.verdict == "fail" { ExitCode::from(2) } else { ExitCode::SUCCESS })
}
