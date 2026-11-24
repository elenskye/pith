//! 整条流程（模拟模型，合成文档）：提炼 → 规范 → 计划 → 排版 → 报告；预算、记账、停止。
//! 真实模型的验收在文件末尾（默认忽略，要密钥文件，会花钱）。

use super::*;
use crate::db::tests::temp_dir;
use claude::{Failure, Mock};
use docx_engine::fixture;

fn quiet(_: Progress) {}

fn ctx<'a>(dir: &'a Path, model: &'a dyn Model, cancel: &'a AtomicBool, budget: f64) -> Ctx<'a> {
    Ctx { data_dir: dir, model, key: "", budget_usd: budget, now: "2026-11-10T08:00:00.000Z".into(), progress: &quiet, cancel }
}

fn write_thesis(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("论文.docx");
    std::fs::write(&p, fixture::build_real_like().write().unwrap()).unwrap();
    p
}

fn docs() -> Vec<SourceDoc> {
    extract::tests::sample_docs()
}

#[test]
fn whole_flow_with_the_mock_model() {
    let dir = temp_dir("agent-flow");
    let (mock, cancel) = (Mock::default(), AtomicBool::new(false));
    let c = ctx(&dir, &mock, &cancel, 20.0);

    // 1 提炼：存成一套规范，出处逐条核对过
    let ruleset = extract_run(&c, docs()).unwrap();
    assert!(ruleset.mock && ruleset.requirements.len() >= 10);
    assert!(ruleset.requirements.iter().flat_map(|r| &r.sources).all(|s| s.verified));
    let store = Store::new(&dir);
    assert_eq!(store.ruleset(&ruleset.id).unwrap(), ruleset);

    // 3 计划：按结构和片段
    let thesis = write_thesis(&dir);
    let before = std::fs::read(&thesis).unwrap();
    let mut run = plan_run(&c, &ruleset, &thesis, Mode::Brief, None).unwrap();
    assert!(run.draft.style_map.iter().any(|m| m.from == "Normal (Web)"));
    assert!(run.sent_chars > 0);
    // 用户加上保护封面（模拟模型不判断）
    run.draft.protect_sections.push(planner::Protect { section: 1, reason: "封面".into(), enabled: true });

    // 4 排版：写出副本和报告，原文件一个字节不变
    apply_run(&dir, &ruleset, &mut run, "2026-11-10T08:05:00.000Z").unwrap();
    let res = run.result.clone().unwrap();
    assert!(res.delivered, "{}", res.report_md);
    assert_eq!(res.fidelity_problems, 0);
    assert_eq!(res.spec_failed, 0, "{}", res.report_md);
    assert_eq!(std::fs::read(&thesis).unwrap(), before);
    let out = res.output_path.unwrap();
    assert!(out.ends_with("论文（Pith 排版）.docx") && Path::new(&out).is_file());
    assert!(Path::new(&res.report_path.unwrap()).is_file());
    // 只检查的要求由代码统计；对不上的标需人工核对
    let kw = res.checks.iter().find(|c| c.requirement == "C2").unwrap();
    assert_eq!((kw.ok, kw.value.as_str()), (Some(false), "2 个"));
    assert!(res.checks.iter().any(|c| c.requirement == "F1" && c.ok.is_none()));
    for part in ["**结论：通过**", "## 要求清单的处理", "只检查、不改的要求", "这一版没有做的要求", "D7 公式编号写在公式所在行的最右边", "受保护区域", "## 结构"] {
        assert!(res.report_md.contains(part), "报告缺「{part}」：\n{}", res.report_md);
    }
    assert_eq!(store.run(&ruleset.id, &run.id).unwrap(), run);

    // 再排一次不覆盖
    apply_run(&dir, &ruleset, &mut run, "2026-11-10T08:06:00.000Z").unwrap();
    assert!(run.result.unwrap().output_path.unwrap().ends_with("论文（Pith 排版 2）.docx"));

    // 模拟模型的调用记了账，但不算钱
    let ledger = Ledger::new(&dir);
    let calls = ledger.all().unwrap();
    assert_eq!(calls.iter().map(|c| c.purpose.as_str()).collect::<Vec<_>>(), ["extract", "plan"]);
    assert!(calls.iter().all(|c| c.mock && c.cost_usd == 0.0));
    assert_eq!(ledger.month_total("2026-11").unwrap(), 0.0);
}

#[test]
fn structure_flow_with_the_mock_model() {
    // 像完整论文的合成文档：模拟模型按标题认出摘要、引言、声明；规范里有页眉、页码、三线表等结构类要求
    let dir = temp_dir("agent-structure");
    let (mock, cancel) = (Mock::default(), AtomicBool::new(false));
    let c = ctx(&dir, &mock, &cancel, 20.0);
    let ruleset = extract_run(&c, docs()).unwrap();
    let a = extract::assemble(&ruleset.name, &ruleset.requirements);
    assert!(a.problems.is_empty(), "{:?}", a.problems);
    let st = &a.spec.structure;
    assert_eq!(st.header.as_ref().unwrap().single.center.as_deref(), Some("某某大学本科毕业论文"));
    assert_eq!(st.header.as_ref().unwrap().rule, Some(true));
    assert_eq!(st.page_numbers.as_ref().unwrap().front_format.as_deref(), Some("upper_roman"));
    assert_eq!((st.body_on_odd_page, st.three_line_tables, st.duplex_from_pages, st.tight_headings.as_slice()), (Some(true), Some(true), Some(30), &[2u8][..]));
    assert_eq!(st.footnote_numbering.as_ref().unwrap().restart.as_deref(), Some("each_page"));
    // 结构类的值也都记着来自哪条要求
    assert!(a.lines.iter().any(|l| l.target == "header" && l.prop == "single_center" && l.requirement.as_deref() == Some("E2")));
    assert!(a.lines.iter().any(|l| l.target == "page_number" && l.value.starts_with("大写罗马数字")));

    let thesis = dir.join("完整论文.docx");
    std::fs::write(&thesis, fixture::build_thesis_like().write().unwrap()).unwrap();
    let mut run = plan_run(&c, &ruleset, &thesis, Mode::Brief, None).unwrap();
    let l = &run.draft.layout;
    assert_eq!(l.front_start.as_ref().map(|a| a.text.as_str()), Some("摘  要"));
    assert_eq!(l.body_start.as_ref().map(|a| a.text.as_str()), Some("1 引言"));
    assert_eq!(l.back_start.as_ref().map(|a| a.text.as_str()), Some("原创性声明"));
    assert!(!l.duplex && l.duplex_note.contains("30 页"), "{}", l.duplex_note);
    apply_run(&dir, &ruleset, &mut run, "2026-11-11T08:00:00.000Z").unwrap();
    let res = run.result.unwrap();
    assert!(res.delivered && res.fidelity_problems == 0 && res.spec_failed == 0, "{}", res.report_md);
    for part in ["## 结构", "新加的分节符：在「摘 要」之前、在「1 引言」之前、在「原创性声明」之前", "排成三线表：第 1 个表格", "第 2 个表格没有动", "| 正文：页眉 | ✓ |", "| 前置部分：页码 | ✓ |", "| 正文从奇数页开始 | ✓ |", "| 脚注编号 | ✓ |"] {
        assert!(res.report_md.contains(part), "报告缺「{part}」：\n{}", res.report_md);
    }
}

/// 假装是真模型（要算钱），回答固定
struct Paid(String, &'static str);

impl Model for Paid {
    fn is_mock(&self) -> bool {
        false
    }
    fn complete(&self, _key: &str, _req: &Request, _p: &dyn Fn(Progress), _c: &AtomicBool) -> Result<claude::Reply, Failure> {
        Ok(claude::Reply { text: self.0.clone(), model: usage::MODEL.into(), tokens: usage::Tokens { input: 50_000, output: 10_000, cache_read: 0, cache_write: 0 }, stop_reason: self.1.into(), request_id: Some("req_1".into()) })
    }
}

#[test]
fn real_calls_are_charged_and_capped() {
    let dir = temp_dir("agent-budget");
    let cancel = AtomicBool::new(false);
    let paid = Paid(extract::mock_reply(), "end_turn");
    let ruleset = extract_run(&ctx(&dir, &paid, &cancel, 20.0), docs()).unwrap();
    let call = ruleset.call.unwrap();
    assert!((call.cost_usd - 0.4).abs() < 1e-9 && !call.mock && !call.estimated);
    let ledger = Ledger::new(&dir);
    assert!((ledger.month_total("2026-11").unwrap() - 0.4).abs() < 1e-9);
    // 上限 $1：已花 0.4，这次最坏情况约 0.65，超了 → 不发送、不记账
    let e = extract_run(&ctx(&dir, &paid, &cancel, 1.0), docs()).unwrap_err();
    assert!(e.contains("没有发送"), "{e}");
    assert_eq!(ledger.all().unwrap().len(), 1);
    // 上个月花的不算
    let mut next = ctx(&dir, &paid, &cancel, 1.0);
    next.now = "2026-12-01T00:00:00.000Z".into();
    extract_run(&next, docs()).unwrap();
}

#[test]
fn unusable_answers_are_still_charged() {
    let dir = temp_dir("agent-unusable");
    let cancel = AtomicBool::new(false);
    let ledger = Ledger::new(&dir);
    let e = extract_run(&ctx(&dir, &Paid("{".into(), "max_tokens"), &cancel, 20.0), docs()).unwrap_err();
    assert!(e.contains("截断") && e.contains("$0.40"), "{e}");
    let e = extract_run(&ctx(&dir, &Paid(String::new(), "refusal"), &cancel, 20.0), docs()).unwrap_err();
    assert!(e.contains("拒绝") && e.contains("重试"), "{e}");
    let e = extract_run(&ctx(&dir, &Paid("不是清单".into(), "end_turn"), &cancel, 20.0), docs()).unwrap_err();
    assert!(e.contains("格式不对") && e.contains("已记进用量"), "{e}");
    assert_eq!(ledger.all().unwrap().len(), 3);
    assert!(Store::new(&dir).rulesets().is_empty(), "不能用的结果不存");
}

struct Broken;

impl Model for Broken {
    fn is_mock(&self) -> bool {
        false
    }
    fn complete(&self, _key: &str, _req: &Request, _p: &dyn Fn(Progress), _c: &AtomicBool) -> Result<claude::Reply, Failure> {
        Err(Failure { message: "已停止。".into(), spent: Some(usage::Tokens { input: 1000, output: 500, cache_read: 0, cache_write: 0 }), cancelled: true })
    }
}

#[test]
fn stopped_calls_are_recorded_as_estimates() {
    let dir = temp_dir("agent-stopped");
    let cancel = AtomicBool::new(false);
    assert_eq!(extract_run(&ctx(&dir, &Broken, &cancel, 20.0), docs()).unwrap_err(), "已停止。");
    let calls = Ledger::new(&dir).all().unwrap();
    assert!(calls.len() == 1 && calls[0].estimated && calls[0].cost_usd > 0.0);
    // 模拟模型失败不记账
    let dir2 = temp_dir("agent-mock-fail");
    let mock = Mock { fail: Some(Failure { message: "坏了".into(), spent: None, cancelled: false }), delay: Default::default() };
    assert_eq!(extract_run(&ctx(&dir2, &mock, &cancel, 20.0), docs()).unwrap_err(), "坏了");
    assert!(Ledger::new(&dir2).all().unwrap().is_empty());
}

#[test]
fn thesis_changed_after_confirmation_is_refused() {
    let dir = temp_dir("agent-changed");
    let (mock, cancel) = (Mock::default(), AtomicBool::new(false));
    let c = ctx(&dir, &mock, &cancel, 20.0);
    let ruleset = extract_run(&c, docs()).unwrap();
    let thesis = write_thesis(&dir);
    let e = plan_run(&c, &ruleset, &thesis, Mode::Full, Some("别的指纹")).unwrap_err();
    assert!(e.contains("改过"), "{e}");
    assert!(read_thesis(&dir.join("没有.docx")).is_err());
    std::fs::write(dir.join("旧.doc"), b"x").unwrap();
    assert!(read_thesis(&dir.join("旧.doc")).unwrap_err().contains("另存为 .docx"));
}

#[test]
fn requirement_documents_are_loaded() {
    let dir = temp_dir("agent-docs");
    let txt = dir.join("规范.txt");
    std::fs::write(&txt, "第一条  \n\n\n\n第二条：正文用宋体小四号，行距固定值 20 磅。\n").unwrap();
    let docx = dir.join("模板.docx");
    std::fs::write(&docx, fixture::build().write().unwrap()).unwrap();
    let docs = extract::load_docs(&[txt.display().to_string(), docx.display().to_string()]).unwrap();
    assert_eq!(docs.iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), ["R1", "R2"]);
    assert_eq!(docs[0].text, "第一条\n\n第二条：正文用宋体小四号，行距固定值 20 磅。");
    assert!(docs[1].chars > 50);
    assert!(extract::load_docs(&[]).is_err());
    let pdf = dir.join("规范.pdf");
    std::fs::write(&pdf, b"%PDF").unwrap();
    assert!(extract::load_docs(&[pdf.display().to_string()]).unwrap_err().contains("不支持"));
    std::fs::write(dir.join("空.txt"), "  \n").unwrap();
    assert!(extract::load_docs(&[dir.join("空.txt").display().to_string()]).unwrap_err().contains("几乎没有文字"));
}

// ---------- 真实模型验收（默认忽略；联网、花钱，要用户同意） ----------
//
//   PITH_API_KEY_FILE=<密钥文件> PITH_ACCEPT_DIR=<放账本和结果的目录> \
//     cargo test -p pith real_agent_extract -- --ignored --nocapture
//   PITH_API_KEY_FILE=… PITH_ACCEPT_DIR=… PITH_RULESET=<规范 id> PITH_THESIS=<论文.docx> PITH_MODE=brief|full \
//     [PITH_HUMAN_PLAN=<人工计划.json>] cargo test -p pith real_agent_plan -- --ignored --nocapture
//
// 账本在 PITH_ACCEPT_DIR 里，上限 PITH_ACCEPT_BUDGET（默认 5 美元）：验收合计不会超过它。

fn accept_dir() -> std::path::PathBuf {
    let d = std::path::PathBuf::from(std::env::var("PITH_ACCEPT_DIR").expect("设 PITH_ACCEPT_DIR"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn accept_budget() -> f64 {
    std::env::var("PITH_ACCEPT_BUDGET").ok().and_then(|v| v.parse().ok()).unwrap_or(5.0)
}

fn show(p: Progress) {
    eprintln!("  … 思考 {} 字，回答 {} 字", p.thinking_chars, p.output_chars);
}

fn real_ctx<'a>(dir: &'a Path, model: &'a dyn Model, key: &'a str, cancel: &'a AtomicBool) -> Ctx<'a> {
    Ctx { data_dir: dir, model, key, budget_usd: accept_budget(), now: text::now_iso(), progress: &show, cancel }
}

fn real_key() -> String {
    secret::Secret::new(false).load().expect("读密钥文件").expect("设 PITH_API_KEY_FILE")
}

fn print_call(c: &Call, estimated_input: u64) {
    println!("用量：输入 {}（发送前估计 {estimated_input}，估计 / 实际 = {:.2}），输出 {}，费用 ${:.4}，模型 {}，请求 {:?}", c.tokens.input, estimated_input as f64 / c.tokens.input.max(1) as f64, c.tokens.output, c.cost_usd, c.model, c.request_id);
}

#[test]
#[ignore]
fn real_agent_extract() {
    let dir = accept_dir();
    let paths: Vec<String> = std::env::var("PITH_REQ_DOCS").expect("设 PITH_REQ_DOCS（用 : 隔开）").split(':').map(String::from).collect();
    let docs = extract::load_docs(&paths).unwrap();
    let estimated = extract::request(&docs).text_len_estimate();
    println!("文档：{}", docs.iter().map(|d| format!("{} {}（{} 字）", d.label, d.name, d.chars)).collect::<Vec<_>>().join("，"));
    let (model, key, cancel) = (claude::Anthropic::default(), real_key(), AtomicBool::new(false));
    let rs = extract_run(&real_ctx(&dir, &model, &key, &cancel), docs).unwrap();
    print_call(rs.call.as_ref().unwrap(), estimated);
    println!("规范 id：{}\n名称：{}\n要求 {} 条；读回时的问题：{:?}", rs.id, rs.name, rs.requirements.len(), rs.problems);
    let unverified: Vec<String> = rs.requirements.iter().flat_map(|r| r.sources.iter().filter(|s| !s.verified).map(move |s| format!("{} {} {}「{}」", r.id, s.doc, s.section, s.quote))).collect();
    println!("出处核对不上的摘录 {} 处：{unverified:#?}", unverified.len());
    for r in &rs.requirements {
        println!("\n{} [{:?} / {:?}] {}", r.id, r.certainty, r.handling, r.text);
        for s in &r.sources {
            println!("    出处 {} {} {}「{}」", s.doc, s.section, if s.verified { "✓" } else { "✗" }, s.quote);
        }
        if !r.note.is_empty() {
            println!("    注：{}", r.note);
        }
        for s in &r.settings {
            println!("    设置 {} · {} = {}{}", extract::label_of(extract::TARGETS, &s.target), extract::label_of(extract::PROPS, &s.prop), s.value, s.error.as_ref().map(|e| format!("  ✗ {e}")).unwrap_or_default());
        }
        if let Some(c) = &r.check {
            println!("    检查 {:?} min {:?} max {:?}", c.kind, c.min, c.max);
        }
    }
    let a = extract::assemble(&rs.name, &rs.requirements);
    println!("\n合成规范时的问题：{:#?}", a.problems);
    let spec_json = serde_json::to_string_pretty(&a.spec).unwrap();
    std::fs::write(dir.join(format!("{}.spec.json", rs.id)), &spec_json).unwrap();
    println!("规范：\n{spec_json}");
    if let Ok(h) = std::env::var("PITH_HUMAN_SPEC") {
        compare_specs(&a.spec, &Spec::from_json(&std::fs::read_to_string(h).unwrap()).unwrap());
    }
}

use docx_engine::plan::Anchor;
use docx_engine::spec::Spec;

fn flat(v: &serde_json::Value, prefix: &str, out: &mut std::collections::BTreeMap<String, String>) {
    match v {
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                flat(x, &if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, out);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                // 标题按级别对
                let key = x.get("level").map(|l| format!("{prefix}[{l}]")).unwrap_or_else(|| prefix.to_string());
                flat(x, &key, out);
            }
        }
        serde_json::Value::Null => {}
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// 逐项对比模型清单合成的规范与人工写的规范
fn compare_specs(got: &Spec, want: &Spec) {
    let (mut g, mut w) = (Default::default(), Default::default());
    flat(&serde_json::to_value(got).unwrap(), "", &mut g);
    flat(&serde_json::to_value(want).unwrap(), "", &mut w);
    let skip = |k: &str| k == "name" || k.starts_with("direct_formatting") || k.ends_with(".level");
    let (mut same, mut diff, mut missing, mut extra) = (0, Vec::new(), Vec::new(), Vec::new());
    for (k, v) in &w {
        if skip(k) {
            continue;
        }
        let num = |s: &str| s.parse::<f64>().ok();
        match g.get(k) {
            Some(x) if x == v || (num(x).is_some() && num(x) == num(v)) => same += 1,
            Some(x) => diff.push(format!("{k}：人工 {v}，Agent {x}")),
            None => missing.push(format!("{k} = {v}")),
        }
    }
    for (k, v) in &g {
        if !skip(k) && !w.contains_key(k) {
            extra.push(format!("{k} = {v}"));
        }
    }
    println!("\n与人工规范对比：一致 {same} 项\n不同 {}：{diff:#?}\n人工有、Agent 没有 {}：{missing:#?}\nAgent 有、人工没有 {}：{extra:#?}", diff.len(), missing.len(), extra.len());
}

#[test]
#[ignore]
fn real_agent_plan() {
    let dir = accept_dir();
    let store = Store::new(&dir);
    let ruleset = store.ruleset(&std::env::var("PITH_RULESET").expect("设 PITH_RULESET")).unwrap();
    let source = std::path::PathBuf::from(std::env::var("PITH_THESIS").expect("设 PITH_THESIS"));
    let mode = match std::env::var("PITH_MODE").as_deref() {
        Ok("full") => Mode::Full,
        _ => Mode::Brief,
    };
    // 在验收目录里的副本上做：输出写在副本旁边，不动原件所在的文件夹
    let work = dir.join(format!("thesis-{}", if mode == Mode::Full { "full" } else { "brief" }));
    std::fs::create_dir_all(&work).unwrap();
    let thesis = work.join(source.file_name().unwrap());
    std::fs::copy(&source, &thesis).unwrap();

    let t = read_thesis(&thesis).unwrap();
    let assembled = extract::assemble(&ruleset.name, &ruleset.requirements);
    let (req, sent) = planner::request(&assembled, &t.outline, mode);
    let total: usize = t.outline.paragraphs.iter().map(|p| p.chars()).sum();
    println!("论文 {}：段落 {}，正文 {total} 字；方式「{}」发出 {sent} 字", t.name, t.outline.paragraphs.len(), mode.label());
    let estimated = req.text_len_estimate();
    let (model, key, cancel) = (claude::Anthropic::default(), real_key(), AtomicBool::new(false));
    let mut run = plan_run(&real_ctx(&dir, &model, &key, &cancel), &ruleset, &thesis, mode, None).unwrap();
    print_call(run.call.as_ref().unwrap(), estimated);
    println!("计划 id：{}\n{}", run.id, serde_json::to_string_pretty(&run.draft).unwrap());
    for h in planner::preview(&t.outline, &run.draft).unwrap() {
        println!("命中 {}：{} 段，如 {:?}", h.label, h.count, h.samples);
    }
    apply_run(&dir, &ruleset, &mut run, &text::now_iso()).unwrap();
    let res = run.result.clone().unwrap();
    println!("排版：写出 {}，保真问题 {}，规范核对 {} 项中 {} 项不符\n检查：{:#?}", res.delivered, res.fidelity_problems, res.spec_checks, res.spec_failed, res.checks);
    for line in res.report_md.lines().filter(|l| l.contains("| ✗ |") || l.starts_with("- 分区") || l.starts_with("- 新加的分节符") || l.starts_with("- 排成三线表") || l.contains("个表格没有动") || l.starts_with("- 紧跟")) {
        println!("报告：{line}");
    }
    assert!(res.delivered, "{}", res.report_md);

    // 与人工计划对比：同一份规范，两份计划各排一遍，逐段比较最后用的样式
    if let Ok(h) = std::env::var("PITH_HUMAN_PLAN") {
        let human = docx_engine::plan::Plan::from_json(&std::fs::read_to_string(h).unwrap()).unwrap();
        let original = Package::read(&std::fs::read(&thesis).unwrap()).unwrap();
        let styles_after = |plan: &docx_engine::plan::Plan| {
            let mut pkg = original.clone();
            docx_engine::format::apply_with_plan(&mut pkg, &assembled.spec, plan).unwrap();
            let o = outline::read(&Package::read(&pkg.write().unwrap()).unwrap()).unwrap();
            o.paragraphs.into_iter().map(|p| (p.index, p.style, outline::snippet(&p.text, 24))).collect::<Vec<_>>()
        };
        let (a, b) = (styles_after(&run.draft.to_plan("agent").unwrap()), styles_after(&human));
        let diffs: Vec<String> = a.iter().zip(&b).filter(|(x, y)| x.1 != y.1).map(|(x, y)| format!("¶{} Agent〈{}〉人工〈{}〉「{}」", x.0, x.1, y.1, x.2)).collect();
        println!("与人工计划对比：{} 段里样式不同的 {} 段\n{}", a.len(), diffs.len(), diffs.iter().take(60).cloned().collect::<Vec<_>>().join("\n"));
        println!("受保护分节：Agent {:?}，人工 {:?}", run.draft.to_plan("x").unwrap().protect_sections, human.protect_sections);
        let mine = run.draft.to_plan("x").unwrap().layout;
        let show = |a: &Option<Anchor>| a.as_ref().map(|a| format!("¶{}「{}」", a.paragraph, a.text)).unwrap_or_else(|| "（无）".into());
        println!("分区：Agent 前置 {} 正文 {} 之后 {} 双面 {} 不排表格 {:?}", show(&mine.front_start), show(&mine.body_start), show(&mine.back_start), mine.duplex, mine.skip_tables);
        println!("分区：人工  前置 {} 正文 {} 之后 {} 双面 {}", show(&human.layout.front_start), show(&human.layout.body_start), show(&human.layout.back_start), human.layout.duplex);
        println!("分区起点与人工一致：{}", mine.front_start.as_ref().map(|a| a.paragraph) == human.layout.front_start.as_ref().map(|a| a.paragraph) && mine.body_start.as_ref().map(|a| a.paragraph) == human.layout.body_start.as_ref().map(|a| a.paragraph) && mine.back_start.as_ref().map(|a| a.paragraph) == human.layout.back_start.as_ref().map(|a| a.paragraph));
    }
}

/// 不联网、不花钱：用存下来的计划（`PITH_ACCEPT_DIR` 里的 `PITH_RULESET` / `PITH_RUN`）在论文副本上按 App 的同一条路径
/// 重新排一遍，结果写到 `PITH_OUT_DIR`。引擎改了以后复查用；存下来的计划和结果不动。
#[test]
#[ignore]
fn real_agent_reapply() {
    let dir = accept_dir();
    let store = Store::new(&dir);
    let ruleset = store.ruleset(&std::env::var("PITH_RULESET").expect("设 PITH_RULESET")).unwrap();
    let mut run = store.run(&ruleset.id, &std::env::var("PITH_RUN").expect("设 PITH_RUN")).unwrap();
    let source = std::path::PathBuf::from(std::env::var("PITH_THESIS").expect("设 PITH_THESIS"));
    let bytes = std::fs::read(&source).unwrap();
    let sha: String = <sha2::Sha256 as sha2::Digest>::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(sha, run.thesis_sha256, "论文和计划当时用的不是同一份");
    let out = std::path::PathBuf::from(std::env::var("PITH_OUT_DIR").expect("设 PITH_OUT_DIR")).join(if run.mode == Mode::Full { "full" } else { "brief" });
    std::fs::create_dir_all(&out).unwrap();
    let thesis = out.join(source.file_name().unwrap());
    std::fs::write(&thesis, &bytes).unwrap();
    run.thesis_path = thesis.display().to_string();
    let res = apply::apply(&ruleset.name, &ruleset.requirements, &run.draft, &thesis, &text::now_iso()).unwrap();
    println!("排版（{}）：写出 {}，保真问题 {}，规范核对 {} 项中 {} 项不符", run.mode.label(), res.delivered, res.fidelity_problems, res.spec_checks, res.spec_failed);
    for line in res.report_md.lines().filter(|l| l.contains("| ✗ |") || l.contains("段前分页") || l.contains("参考文献样式的段落") || l.starts_with("- 分区") || l.starts_with("- 排成三线表")) {
        println!("报告：{line}");
    }
    println!("输出：{:?}\n报告：{:?}", res.output_path, res.report_path);
    assert!(res.delivered, "{}", res.report_md);
}

/// 不联网：看真实论文按两种方式各要发多少（`PITH_THESIS=<论文.docx>`，可加 `PITH_SHOW=1` 打印发送的文字）
#[test]
#[ignore]
fn real_thesis_digest() {
    let t = read_thesis(Path::new(&std::env::var("PITH_THESIS").expect("设 PITH_THESIS"))).unwrap();
    let total: usize = t.outline.paragraphs.iter().map(|p| p.chars()).sum();
    println!("{}：段落 {}，分节 {}，表格 {}，正文 {total} 字", t.name, t.outline.paragraphs.len(), t.outline.sections.len(), t.outline.tables);
    for mode in [Mode::Brief, Mode::Full] {
        let (text, sent) = planner::render(&t.outline, mode);
        println!("{}：发出论文原文 {sent} 字，整段文字 {} 字，估计 {} token", mode.label(), text.chars().count(), usage::estimate_tokens(&text));
        if std::env::var("PITH_SHOW").is_ok() && mode == Mode::Brief {
            println!("{text}");
        }
    }
}

/// 不联网：把存下来的规范（`PITH_ACCEPT_DIR` + `PITH_RULESET`）重新合成一遍，写出 `<id>.spec.json`，可与人工规范对比
#[test]
#[ignore]
fn real_ruleset_spec() {
    let dir = accept_dir();
    let rs = Store::new(&dir).ruleset(&std::env::var("PITH_RULESET").expect("设 PITH_RULESET")).unwrap();
    let a = extract::assemble(&rs.name, &rs.requirements);
    println!("问题：{:#?}", a.problems);
    for l in &a.lines {
        println!("{} · {} = {}  ← {}", l.target_label, l.prop_label, l.value, l.requirement.clone().unwrap_or_else(|| format!("默认做法（{}）", l.note.clone().unwrap_or_default())));
    }
    std::fs::write(dir.join(format!("{}.spec.json", rs.id)), serde_json::to_string_pretty(&a.spec).unwrap()).unwrap();
    if let Ok(h) = std::env::var("PITH_HUMAN_SPEC") {
        compare_specs(&a.spec, &Spec::from_json(&std::fs::read_to_string(h).unwrap()).unwrap());
    }
}

/// 给端到端点测用：把仿真实论文的合成文档写到 `PITH_E2E_DIR/论文.docx`
#[test]
#[ignore]
fn prepare_e2e_thesis() {
    let dir = std::path::PathBuf::from(std::env::var("PITH_E2E_DIR").expect("设 PITH_E2E_DIR"));
    println!("{}", write_thesis(&dir).display());
}
