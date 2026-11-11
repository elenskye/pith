//! 评估：docx-rs / docx-rust 读入再写出（不做任何修改）后，内容还剩多少。
//! 用法：cargo run -p docx-engine --example eval_rust_libs -- <输出目录> <输入.docx>...
//! 每个结果写成 <输出目录>/<库>-<文件名>，并用本 crate 的校验器与原文件比较。

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use docx_engine::package::Package;
use docx_engine::verify;

fn via_docx_rs(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let doc = docx_rs::read_docx(bytes).map_err(|e| format!("读取失败：{e:?}"))?;
    let mut out = Cursor::new(Vec::new());
    doc.build().pack(&mut out).map_err(|e| format!("写出失败：{e}"))?;
    Ok(out.into_inner())
}

fn via_docx_rust(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let file = docx_rust::DocxFile::from_reader(Cursor::new(bytes.to_vec())).map_err(|e| format!("读取失败：{e:?}"))?;
    let mut doc = file.parse().map_err(|e| format!("解析失败：{e:?}"))?;
    let out = doc.write(Cursor::new(Vec::new())).map_err(|e| format!("写出失败：{e:?}"))?;
    Ok(out.into_inner())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out_dir, inputs) = args.split_first().expect("用法：<输出目录> <输入.docx>...");
    std::fs::create_dir_all(out_dir).unwrap();
    for input in inputs {
        let bytes = std::fs::read(input).unwrap();
        let original = Package::read(&bytes).unwrap();
        let before = verify::snapshot(&original).unwrap();
        let fname = Path::new(input).file_name().unwrap().to_string_lossy().to_string();
        for (lib, f) in [("docx-rs", via_docx_rs as fn(&[u8]) -> Result<Vec<u8>, String>), ("docx-rust", via_docx_rust)] {
            print!("{lib:10} {fname:36} ");
            let out = match std::panic::catch_unwind(|| f(&bytes)) {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => {
                    println!("失败：{e}");
                    continue;
                }
                Err(_) => {
                    println!("失败：库内部 panic");
                    continue;
                }
            };
            std::fs::write(Path::new(out_dir).join(format!("{lib}-{fname}")), &out).unwrap();
            let after = match Package::read(&out).and_then(|p| verify::snapshot(&p)) {
                Ok(s) => s,
                Err(e) => {
                    println!("输出无法解析：{e}");
                    continue;
                }
            };
            let problems = verify::compare(&before, &after);
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for p in &problems {
                *kinds.entry(p.kind.clone()).or_default() += 1;
            }
            let doc_b = &before.stories["word/document.xml"];
            let doc_a = after.stories.get("word/document.xml");
            let chars = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
            println!(
                "问题 {} 处 {:?}；正文非空白字符 {} → {}",
                problems.len(),
                kinds,
                chars(&doc_b.text),
                doc_a.map(|d| chars(&d.text)).unwrap_or(0)
            );
            for p in problems.iter().filter(|p| p.kind != "部件被改动").take(8) {
                println!("    · {} {}：{}", p.part, p.kind, p.detail);
            }
        }
    }
}
