//! 排版 Agent 最后一步：全部是确定性的（`decisions/0008` 第 4 步）。
//! 按用户确认的规范和计划改一份副本 → 独立校验器核对保真与规范 → 只检查的项目由代码统计 → 写报告。
//! 原文件从不改写；保真校验没通过就不写出文件。

use std::path::{Path, PathBuf};

use docx_engine::package::Package;
use docx_engine::{checks, format, outline, report, verify};

use super::extract::{self, Certainty, Handling, Requirement};
use super::planner::Draft;
use super::store::{CheckLine, RunResult};

fn output_path(thesis: &Path) -> PathBuf {
    let dir = thesis.parent().unwrap_or(Path::new("."));
    let stem = thesis.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "文档".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}（Pith 排版）") } else { format!("{stem}（Pith 排版 {n}）") };
        let docx = dir.join(format!("{name}.docx"));
        if !docx.exists() && !dir.join(format!("{name} 报告.md")).exists() {
            return docx;
        }
        n += 1;
    }
}

fn certainty_label(c: Certainty) -> &'static str {
    match c {
        Certainty::Explicit => "明文",
        Certainty::Template => "模板",
        Certainty::Inferred => "推断",
        Certainty::Unspecified => "未规定",
    }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// 报告里「要求清单」的部分：哪些自动应用了、哪些没做、检查结果
fn requirements_md(reqs: &[Requirement], check_lines: &[CheckLine], draft: &Draft, assembled_problems: &[String]) -> String {
    let mut s = String::new();
    let auto: Vec<&Requirement> = reqs.iter().filter(|r| r.handling == Handling::Auto && r.enabled).collect();
    let off: Vec<&Requirement> = reqs.iter().filter(|r| r.handling == Handling::Auto && !r.enabled).collect();
    let structure: Vec<&Requirement> = reqs.iter().filter(|r| r.handling == Handling::Structure).collect();
    let keep: Vec<&Requirement> = reqs.iter().filter(|r| r.handling == Handling::Keep).collect();
    s.push_str("\n## 要求清单的处理\n\n");
    s.push_str(&format!("- 自动应用：{} 条（{}）\n", auto.len(), auto.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join("、")));
    let guessed: Vec<String> = auto.iter().filter(|r| r.certainty != Certainty::Explicit).map(|r| format!("{}（{}）", r.id, certainty_label(r.certainty))).collect();
    if !guessed.is_empty() {
        s.push_str(&format!("- 其中不是规范明文的：{}\n", guessed.join("、")));
    }
    if !off.is_empty() {
        s.push_str(&format!("- 你关掉、没有应用的：{}\n", off.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join("、")));
    }
    for p in assembled_problems {
        s.push_str(&format!("- ⚠ {p}\n"));
    }
    if !check_lines.is_empty() {
        s.push_str("\n### 只检查、不改的要求\n\n统计由程序完成，不是模型数的。「需人工核对」的程序没有检查。\n\n| 要求 | 结果 | 统计 | 说明 |\n| --- | --- | --- | --- |\n");
        for c in check_lines {
            let mark = match c.ok {
                Some(true) => "✓",
                Some(false) => "✗",
                None => "需人工核对",
            };
            s.push_str(&format!("| {} {} | {mark} | {} | {} |\n", c.requirement, cell(&c.text), cell(&c.value), cell(&c.detail)));
        }
    }
    if !structure.is_empty() {
        s.push_str("\n### 要改内容或结构、这一版没有做的要求\n\n");
        for r in &structure {
            s.push_str(&format!("- {} {}\n", r.id, r.text));
        }
    }
    if !keep.is_empty() {
        s.push_str("\n### 不动的\n\n");
        for r in &keep {
            s.push_str(&format!("- {} {}\n", r.id, r.text));
        }
    }
    if !draft.observations.is_empty() {
        s.push_str("\n## 文档里的其他情况\n\n分析论文时模型看到的、计划解决不了的情况，供参考（没有经过程序核对）。\n\n");
        for o in &draft.observations {
            let at = if o.paragraphs.is_empty() { String::new() } else { format!("（第 {} 段）", o.paragraphs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("、")) };
            s.push_str(&format!("- {}{at}\n", o.text));
        }
    }
    s
}

/// 「只检查」的要求：能对上程序检查项的由代码统计，对不上的标「需人工核对」
fn run_checks(reqs: &[Requirement], o: &outline::Outline) -> Vec<CheckLine> {
    reqs.iter()
        .filter(|r| r.handling == Handling::Check)
        .map(|r| match &r.check {
            Some(c) => {
                let out = checks::run(o, c.kind, c.min, c.max);
                CheckLine { requirement: r.id.clone(), text: r.text.clone(), ok: out.ok, value: out.value, detail: out.detail }
            }
            None => CheckLine { requirement: r.id.clone(), text: r.text.clone(), ok: None, value: "—".into(), detail: "程序不会检查这一项".into() },
        })
        .collect()
}

pub fn apply(ruleset_name: &str, reqs: &[Requirement], draft: &Draft, thesis: &Path, now: &str) -> Result<RunResult, String> {
    let name = thesis.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(thesis).map_err(|e| format!("读不了 {name}：{e}"))?;
    let original = Package::read(&bytes).map_err(|e| format!("{name} 不是有效的 .docx：{e}"))?;
    let assembled = extract::assemble(ruleset_name, reqs);
    assembled.spec.validate().map_err(|e| e.to_string())?;
    if assembled.lines.iter().all(|l| l.requirement.is_none()) {
        return Err("要求清单里没有一条能自动应用的设置（都关掉了，或值读不懂），没有可排的。".into());
    }
    let plan = draft.to_plan(&format!("{name} 的文档计划"))?;

    let mut pkg = original.clone();
    let rep = format::apply_with_plan(&mut pkg, &assembled.spec, &plan).map_err(|e| format!("排版失败：{e}"))?;
    let out_bytes = pkg.write().map_err(|e| format!("排版失败：{e}"))?;
    // 从写出的字节重新读，用另一套解析器核对
    let after = Package::read(&out_bytes).map_err(|e| format!("排版结果读不回来：{e}"))?;
    let (problems, spec_checks) = verify::verify_all(&original, &after, &assembled.spec, &plan).map_err(|e| e.to_string())?;
    let check_lines = run_checks(reqs, &outline::read(&after).map_err(|e| e.to_string())?);

    let mut md = report::markdown(Some(&rep), &problems, &spec_checks);
    md.push_str(&requirements_md(reqs, &check_lines, draft, &assembled.problems));

    let delivered = problems.is_empty();
    let (mut output, mut report_file) = (None, None);
    if delivered {
        let out = output_path(thesis);
        std::fs::write(&out, &out_bytes).map_err(|e| format!("写不出 {}：{e}", out.display()))?;
        let rp = out.with_file_name(format!("{} 报告.md", out.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
        // 报告写不出不算失败：App 里看得到
        if std::fs::write(&rp, format!("{md}\n---\n\n原文件：{name}（没有改动）\n排版后：{}\n", out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).is_ok() {
            report_file = Some(rp.display().to_string());
        }
        output = Some(out.display().to_string());
    } else {
        md = format!("> 保真校验没有通过，所以没有写出文件。原文件没有改动。\n\n{md}");
    }
    Ok(RunResult {
        applied_at: now.to_string(),
        delivered,
        output_path: output,
        report_path: report_file,
        report_md: md,
        fidelity_problems: problems.len(),
        spec_checks: spec_checks.len(),
        spec_failed: spec_checks.iter().filter(|c| !c.ok).count(),
        checks: check_lines,
    })
}
