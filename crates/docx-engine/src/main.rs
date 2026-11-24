//! 命令行。永不改写输入文件。
//!   docx-engine fixture <输出.docx>
//!   docx-engine apply <输入.docx> --spec <规范.json> [--plan <计划.json>] --out <输出.docx> [--report <报告.md>]
//!   docx-engine verify <排版前.docx> <排版后.docx> [--spec <规范.json>] [--plan <计划.json>] [--json]
//!   docx-engine inspect <文件.docx>
//!   docx-engine dotm <输出.dotm> --bas <模块.bas> --ribbon <customUI.xml>   （调试 Word 模板用）

use std::path::Path;
use std::process::ExitCode;

use docx_engine::package::Package;
use docx_engine::plan::Plan;
use docx_engine::spec::Spec;
use docx_engine::{fixture, format, report, template, vba, verify, Error};

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn read(path: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::Io(e, path.into()))
}

fn write_new(path: &str, bytes: &[u8]) -> Result<(), Error> {
    if Path::new(path).exists() {
        return Err(Error::Spec(format!("拒绝覆盖已存在的文件：{path}")));
    }
    std::fs::write(path, bytes).map_err(|e| Error::Io(e, path.into()))
}

fn load_spec(path: &str) -> Result<Spec, Error> {
    Spec::from_json(&String::from_utf8_lossy(&read(path)?))
}

fn load_plan(args: &[String]) -> Result<Plan, Error> {
    match arg(args, "--plan") {
        Some(p) => Plan::from_json(&String::from_utf8_lossy(&read(p)?)),
        None => Ok(Plan::default()),
    }
}

fn run(args: &[String]) -> Result<ExitCode, Error> {
    let usage = || Error::Spec("用法见 src/main.rs 文件头".into());
    match args.first().map(String::as_str) {
        Some("fixture") => {
            let out = args.get(1).ok_or_else(usage)?;
            write_new(out, &fixture::build().write()?)?;
            println!("已生成 {out}");
            Ok(ExitCode::SUCCESS)
        }
        Some("apply") => {
            let input = args.get(1).ok_or_else(usage)?;
            let out = arg(args, "--out").ok_or_else(usage)?;
            if Path::new(out).canonicalize().ok() == Path::new(input).canonicalize().ok() {
                return Err(Error::Spec("输出不能是输入文件本身".into()));
            }
            let spec = load_spec(arg(args, "--spec").ok_or_else(usage)?)?;
            let plan = load_plan(args)?;
            let original = Package::read(&read(input)?)?;
            let mut pkg = original.clone();
            let rep = format::apply_with_plan(&mut pkg, &spec, &plan)?;
            let bytes = pkg.write()?;
            let after = Package::read(&bytes)?;
            let (problems, checks) = verify::verify_all(&original, &after, &spec, &plan)?;
            write_new(out, &bytes)?;
            let md = report::markdown(Some(&rep), &problems, &checks);
            if let Some(r) = arg(args, "--report") {
                write_new(r, md.as_bytes())?;
            }
            println!("{}", md.lines().take(6).collect::<Vec<_>>().join("\n"));
            println!("已写出 {out}");
            Ok(if problems.is_empty() && checks.iter().all(|c| c.ok) { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Some("verify") => {
            let (a, b) = (args.get(1).ok_or_else(usage)?, args.get(2).ok_or_else(usage)?);
            let before = Package::read(&read(a)?)?;
            let after = Package::read(&read(b)?)?;
            let plan = load_plan(args)?;
            let mut problems = verify::compare(&verify::snapshot(&before)?, &verify::snapshot(&after)?);
            problems.extend(verify::check_protected(&before, &after, &plan)?);
            let checks = match arg(args, "--spec") {
                Some(p) => verify::check_spec_with_plan(&after, &load_spec(p)?, &plan)?,
                None => Vec::new(),
            };
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::json!({ "problems": problems, "checks": checks }));
            } else {
                print!("{}", report::markdown(None, &problems, &checks));
            }
            Ok(if problems.is_empty() && checks.iter().all(|c| c.ok) { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Some("dotm") => {
            let out = args.get(1).ok_or_else(usage)?;
            let text = |k: &str| -> Result<String, Error> { Ok(String::from_utf8_lossy(&read(arg(args, k).ok_or_else(usage)?)?).into_owned()) };
            let code: String = text("--bas")?.lines().filter(|l| !l.starts_with("Attribute VB_")).collect::<Vec<_>>().join("\n");
            let project = vba::Project::word("Pith", vec![vba::Module { name: "PithMain".into(), kind: vba::ModuleKind::Standard, code }]);
            let ribbon = text("--ribbon")?;
            // 选项卡图片：--ribbon 同目录下 images/*.png
            let img_dir = arg(args, "--ribbon").and_then(|r| std::path::Path::new(r).parent()).map(|d| d.join("images"));
            let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
            if let Some(Ok(rd)) = img_dir.as_ref().map(std::fs::read_dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "png") {
                        owned.push((p.file_stem().unwrap_or_default().to_string_lossy().to_string(), std::fs::read(&p).map_err(|e| Error::Io(e, p.clone()))?));
                    }
                }
            }
            owned.sort();
            let images: Vec<(&str, &[u8])> = owned.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
            let bytes = template::build_dotm(&template::Template { project: &project, ribbon: &ribbon, images: &images, note: "Pith" }).map_err(Error::Spec)?;
            write_new(out, &bytes)?;
            println!("已生成 {out}");
            Ok(ExitCode::SUCCESS)
        }
        Some("vba-dump") => {
            print!("{}", vba::dump(&read(args.get(1).ok_or_else(usage)?)?).map_err(Error::Spec)?);
            Ok(ExitCode::SUCCESS)
        }
        Some("inspect") => {
            let pkg = Package::read(&read(args.get(1).ok_or_else(usage)?)?)?;
            let snap = verify::snapshot(&pkg)?;
            for (part, st) in &snap.stories {
                println!("{part}：{} 字，删除 {} 字，域 {} 个，公式文字 {} 字", st.text.chars().count(), st.deleted.chars().count(), st.field_codes.len(), st.math.chars().count());
                let counts: Vec<String> = st.counts.iter().map(|(k, v)| format!("{k} {v}")).collect();
                println!("  {}", counts.join("，"));
            }
            println!("部件 {} 个", snap.parts.len());
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(usage()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            ExitCode::from(2)
        }
    }
}
