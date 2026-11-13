//! 整条流程在合成夹具上跑（模拟模型、模拟搜索、模拟下载，不联网）：
//! 计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告 → 重查对比；取消与接着做；各种失败；
//! 以及「改坏测试」：摘录改一个字、数字改一位、单位错、结论里多一个对不上的数、低可信来源被排到前面，每一种都要被拦下。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use research_engine::audit::{self, Kind};
use research_engine::fixture::{self, MockFetch, MockModel, MockSearch, QUESTION};
use research_engine::flow;
use research_engine::model::{CheckStatus, Config, ModelRef, Run, Segment, Source, Stage, Status, StepKind, Uncertainty};
use research_engine::ports::{ErrorKind, Fetch, Fetched, Ports, Progress, Purpose, Recorder};
use research_engine::trust::Tier;
use research_engine::{render, report};

#[derive(Default)]
struct Saves {
    seen: Vec<(Stage, Status)>,
    last: Option<Run>,
}

impl Recorder for Saves {
    fn save(&mut self, run: &Run) -> Result<(), String> {
        if self.seen.last() != Some(&(run.stage, run.status)) {
            self.seen.push((run.stage, run.status));
        }
        self.last = Some(run.clone());
        Ok(())
    }
}

struct Kit {
    planner: MockModel,
    worker: MockModel,
    search: MockSearch,
    fetch: Box<dyn Fetch>,
    cancel: Arc<AtomicBool>,
    saves: Saves,
}

impl Kit {
    fn new(round: u32) -> Kit {
        Kit { planner: MockModel::new("mock-planner", round), worker: MockModel::new("mock-worker", round), search: MockSearch::new(round), fetch: Box::new(MockFetch::new(round)), cancel: Arc::new(AtomicBool::new(false)), saves: Saves::default() }
    }

    fn config() -> Config {
        Config { planner: ModelRef { provider: "mock".into(), model: "mock-planner".into() }, worker: ModelRef { provider: "mock".into(), model: "mock-worker".into() }, search_engine: "mock".into(), ..Default::default() }
    }

    fn with<T>(&mut self, f: impl FnOnce(&mut Ports) -> T) -> T {
        let now = || "2026-11-12T09:00:00".to_string();
        let quiet = |_: Progress| {};
        let mut ports = Ports { planner: &self.planner, worker: &self.worker, search: &self.search, fetch: self.fetch.as_ref(), recorder: &mut self.saves, now: &now, progress: &quiet, cancel: &self.cancel };
        f(&mut ports)
    }

    fn plan(&mut self) -> Run {
        self.with(|p| flow::plan(QUESTION, "RUN1", 1, None, Kit::config(), 0, p)).unwrap()
    }

    fn execute(&mut self, run: &mut Run) {
        self.with(|p| flow::execute(run, p)).unwrap();
    }

    fn investigate(&mut self) -> Run {
        let mut run = self.plan();
        self.execute(&mut run);
        run
    }
}

fn texts(round: u32) -> impl FnMut(&Source) -> Result<String, String> {
    move |s: &Source| MockFetch::new(round).text(s)
}

fn fact<'a>(run: &'a Run, value: &str, source: &str) -> &'a research_engine::model::Fact {
    run.facts.iter().find(|f| f.value == value && f.source == source).unwrap_or_else(|| panic!("没有 {source} 的 {value}：{:?}", run.facts.iter().map(|f| (&f.id, &f.source, &f.value)).collect::<Vec<_>>()))
}

#[test]
fn whole_flow_on_the_synthetic_city() {
    let mut kit = Kit::new(1);

    // 1 计划：停在等用户确认；主动提出了官方统计之外的维度
    let mut run = kit.plan();
    assert_eq!((run.stage, run.status), (Stage::Search, Status::AwaitingPlan));
    assert_eq!(run.plan.sub_questions.len(), 6);
    assert!(run.plan.sub_questions.iter().filter(|s| s.angle == research_engine::model::Angle::Alternative).count() >= 2);
    assert!(run.plan.dimensions.iter().any(|d| d.name.contains("招聘平台")) && run.plan.problems.is_empty());
    assert!(run.mock && run.steps.len() == 1 && run.steps[0].kind == StepKind::Plan);

    // 2–7 执行到底
    kit.execute(&mut run);
    assert_eq!((run.stage, run.status, run.error.clone()), (Stage::Report, Status::Done, None));
    assert!(run.finished_at.is_some());
    // 每一步都存过
    let stages: Vec<Stage> = kit.saves.seen.iter().map(|(s, _)| *s).collect();
    for s in [Stage::Search, Stage::Download, Stage::Extract, Stage::Verify, Stage::Conclude, Stage::Report] {
        assert!(stages.contains(&s), "{s:?} 没存");
    }
    assert_eq!(kit.saves.last.as_ref().unwrap().status, Status::Done);

    // 检索：7 个搜索词；网址去重后 9 个来源
    assert_eq!(run.searches.len(), 7);
    assert_eq!(run.sources.len(), 9);
    let bulletin = &run.sources[0];
    assert!(bulletin.url.ends_with("/stats/bulletin-2024.html") && bulletin.sub_questions == ["Q1", "Q2"]);
    // 下载：一个 404，一个取不出文字（不静默）
    let missing = run.sources.iter().find(|s| s.url.ends_with(fixture::MISSING)).unwrap();
    assert!(missing.fetch_error.as_deref().unwrap().contains("404"));
    let scanned = run.sources.iter().find(|s| s.url.ends_with(fixture::SCANNED)).unwrap();
    assert!(scanned.text_error.as_deref().unwrap().starts_with("取文字失败") && scanned.sha256.is_some());
    assert_eq!(run.sources.iter().filter(|s| s.usable()).count(), 7);

    // 摘证据 + 核对：14 条里 13 条核实；网友博客里那条转述的摘录在原文里找不到
    assert_eq!((run.facts.len(), run.verified_facts().count()), (14, 13));
    let bad = run.facts.iter().find(|f| f.check.status == CheckStatus::Unverified).unwrap();
    assert_eq!((bad.value.as_str(), bad.check.reason.as_deref()), ("35", Some("摘录在保存的正文里找不到")));
    // 数据生产者与可信度：官方统计降权；转述的按原始生产者算
    let official = fact(&run, "12.3", "S1");
    assert_eq!((official.producer.as_str(), official.tier, official.relayed), ("示例市统计局", Tier::Low, false));
    let relayed = run.facts.iter().find(|f| f.relayed).unwrap();
    assert_eq!((relayed.producer.as_str(), relayed.tier), ("示例市统计局", Tier::Low));
    assert_eq!(run.facts.iter().find(|f| f.value == "15.8").unwrap().tier, Tier::High);
    assert_eq!(run.facts.iter().find(|f| f.value == "48200").unwrap().tier, Tier::Medium);
    assert_eq!(run.source("S3").unwrap().tier, Some(Tier::High));
    // 表格里的数靠表头给单位
    let table_cell = run.facts.iter().find(|f| f.value == "13.4").unwrap();
    assert!(table_cell.verified() && table_cell.unit_quote.contains("（%）"));

    // 结论：5 条，数字全部对上；计算由代码做
    assert_eq!(run.conclusions.len(), 5);
    assert!(run.conclusions.iter().all(|k| k.issues.is_empty()), "{:?}", run.conclusions.iter().map(|k| &k.issues).collect::<Vec<_>>());
    assert_eq!(run.conclusions[0].text(), "按官方口径，示例市 16–24 岁城镇青年调查失业率 2024 年为 12.3%，比 2023 年的 13.4% 低 1.1 个百分点。数字由示例市统计局发布。");
    assert_eq!(run.conclusions[0].uncertainty, [Uncertainty::LowTrustOnly]);
    assert_eq!(run.conclusions[1].uncertainty, [Uncertainty::Conflicting, Uncertainty::UnclearDefinition]);
    assert!(run.conclusions[1].text().contains("15.8%") && run.conclusions[1].text().contains("高 3.5 个百分点"));
    assert!(run.conclusions[2].text().contains("98,560元") && run.conclusions[2].text().contains("1.61倍"));
    assert!(run.conclusions[4].text().contains("高 2.2 个百分点（样本市为 10.1%）"));
    // 引用的事实里可信度高的在前
    assert_eq!(run.fact(&run.conclusions[1].facts[0]).unwrap().tier, Tier::High);
    assert_eq!(run.calcs.len(), 4);
    assert!(run.calcs.iter().all(|c| c.error.is_none() && c.steps.contains(" = ")));
    assert_eq!(run.gaps.len(), 2);

    // 步骤与调用次数：计划 1 + 摘录 7（可读的来源）+ 结论 1；搜索 7
    assert_eq!(kit.planner.calls(), ["research_plan:plan", "research_conclude:conclude#1"]);
    assert_eq!(kit.worker.calls().len(), 7);
    assert_eq!(run.steps.iter().filter(|s| s.kind == StepKind::Search).count(), 7);
    assert!(run.steps.iter().all(|s| s.ok) && run.cost_usd() == 0.0);

    // 审计：从头再核一遍，没有问题
    assert_eq!(audit::audit(&run, &mut texts(1)), []);

    // 报告：短答在前；来源按可信度排；写明等级表
    let r = report::build(&run, None);
    assert!(audit::audit_order(&r).is_empty());
    assert_eq!(r.sources[0].source.id, "S3");
    assert_eq!((r.totals.sources, r.totals.readable, r.totals.download_failed, r.totals.no_text, r.totals.facts, r.totals.verified), (9, 7, 1, 1, 14, 13));
    let doc = render::document(&r);
    let md = render::markdown(&doc);
    assert!(md.find("## 短答").unwrap() < md.find("## 子问题").unwrap());
    for part in ["**12.3%**〔F1〕", "默认等级表", "中国政府官方统计", "取文字失败", "下载失败", "未核实：摘录在保存的正文里找不到", "计算由程序执行", "模拟，没有花钱", "官方统计之外的衡量维度"] {
        assert!(md.contains(part), "Markdown 里缺「{part}」：\n{md}");
    }
    let html = render::html(&doc);
    assert!(html.contains("<a class=\"ref\" href=\"#F1\">12.3%</a>") && html.contains("<tr id=\"F1\">") && html.contains("<tr id=\"S1\">") && html.contains("<tr id=\"C1\">"));
    assert!(!html.contains("<link") && !html.contains("src="));
}

#[test]
fn rerun_marks_what_changed() {
    let first = Kit::new(1).investigate();
    let mut kit = Kit::new(2);
    let mut second = flow::rerun_from(&first, "RUN2", Kit::config(), "2026-12-01T09:00:00".into());
    assert_eq!((second.seq, second.previous.as_deref(), second.stage, second.status), (2, Some("RUN1"), Stage::Search, Status::AwaitingPlan));
    kit.execute(&mut second);
    assert_eq!(second.status, Status::Done);
    // 重查不再花钱拆问题
    assert_eq!(kit.planner.calls(), ["research_conclude:conclude#1"]);
    assert_eq!(audit::audit(&second, &mut texts(2)), []);

    let r = report::build(&second, Some(&first));
    let d = r.diff.clone().unwrap();
    // 统计局修订了 2024 年的数
    assert_eq!(d.changed.len(), 1);
    assert_eq!((d.changed[0].old.value.as_str(), d.changed[0].new.value.as_str(), d.changed[0].new.period.as_str()), ("12.3%", "12.6%", "2024 年"));
    assert_eq!(d.added.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["11.9%"]);
    assert_eq!(d.removed.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["30%"]);
    assert_eq!(d.unchanged, 11);
    assert_eq!(d.sources_added.len(), 1);
    assert!(d.sources_added[0].url.ends_with("/stats/h1-2025.html"));
    assert!(d.sources_removed.len() == 1 && d.sources_removed[0].url.ends_with("/blog/blog-rumor.html"));
    // 用到 12.3% 的三条结论变了，另外两条没变
    assert_eq!((d.conclusions_added.len(), d.conclusions_removed.len(), d.conclusions_same), (3, 3, 2));
    assert!(d.conclusions_added[0].contains("12.6%") && d.conclusions_removed[0].contains("12.3%"));
    let md = render::markdown(&render::document(&r));
    for part in ["## 和上一次调查的对比", "### 变了的数字", "12.3%", "### 这次新查到的", "### 消失的来源", "### 变了的结论"] {
        assert!(md.contains(part), "缺「{part}」");
    }
    // 没变化的重查
    let again = Kit::new(1).investigate();
    assert!(report::build(&again, Some(&first)).diff.unwrap().nothing_changed());
}

// ---------- 改坏测试：模型的输出被改坏，流程要拦下 ----------

/// 把统计公报那份来源的摘录回复改掉一处
fn corrupt_bulletin(from: &'static str, to: &'static str) -> Kit {
    let mut kit = Kit::new(1);
    kit.worker = MockModel::new("mock-worker", 1).tampering(move |req, text| {
        if req.tag == "S1#1" {
            assert!(text.contains(from), "夹具里没有「{from}」");
            text.replacen(from, to, 1)
        } else {
            text
        }
    });
    kit
}

fn assert_blocked(run: &Run, reason: &str) {
    assert_eq!(run.status, Status::Done);
    let f = run.facts.iter().find(|f| f.source == "S1" && f.metric.contains("调查失业率")).unwrap();
    assert_eq!(f.check.status, CheckStatus::Unverified);
    assert!(f.check.reason.as_deref().unwrap().contains(reason), "{:?}", f.check.reason);
    // 没核实的不进结论
    assert!(run.conclusions.iter().all(|k| !k.facts.contains(&f.id) && !k.template.contains(&format!("{{{}}}", f.id))));
    assert!(run.conclusions.iter().all(|k| k.issues.is_empty()));
    assert!(!run.conclusions.iter().any(|k| k.text().contains("按官方口径")));
    assert!(report::build(run, None).notes.iter().any(|n| n.contains("2 条事实没有核实")));
}

#[test]
fn a_quote_changed_by_one_character_is_blocked() {
    let run = corrupt_bulletin("城镇青年调查失业率为 12.3%，比上年", "城镇青年登记失业率为 12.3%，比上年").investigate();
    assert_blocked(&run, "摘录在保存的正文里找不到");
}

#[test]
fn a_number_changed_by_one_digit_is_blocked() {
    let run = corrupt_bulletin("\"value\": \"12.3\"", "\"value\": \"12.8\"").investigate();
    assert_blocked(&run, "数值不在摘录里");
    // 摘录里的数也一起改：摘录就找不到了
    let run = corrupt_bulletin("\"value\": \"12.3\", \"unit\": \"%\", \"region\": \"示例市\", \"population\": \"16–24 岁城镇青年\", \"period\": \"2024 年\", \"definition\": \"城镇调查失业率，不含在校学生\", \"producer\": \"示例市统计局\", \"producer_kind\": \"cn_gov_stats\", \"relayed\": false, \"quote\": \"2024 年，全市 16–24 岁城镇青年调查失业率为 12.3%", "\"value\": \"12.8\", \"unit\": \"%\", \"region\": \"示例市\", \"population\": \"16–24 岁城镇青年\", \"period\": \"2024 年\", \"definition\": \"城镇调查失业率，不含在校学生\", \"producer\": \"示例市统计局\", \"producer_kind\": \"cn_gov_stats\", \"relayed\": false, \"quote\": \"2024 年，全市 16–24 岁城镇青年调查失业率为 12.8%").investigate();
    assert_blocked(&run, "摘录在保存的正文里找不到");
}

#[test]
fn a_wrong_unit_is_blocked() {
    let run = corrupt_bulletin("\"value\": \"12.3\", \"unit\": \"%\"", "\"value\": \"12.3\", \"unit\": \"‰\"").investigate();
    assert_blocked(&run, "单位对不上：摘录里是「%」，事实写的是「‰」");
}

#[test]
fn an_extra_number_in_a_conclusion_is_sent_back_then_flagged() {
    // 第一稿多写了一个事实表里没有的数：打回重写，第二稿干净
    let mut kit = Kit::new(1);
    kit.planner = MockModel::new("mock-planner", 1).tampering(|req, text| if req.tag == "conclude#1" { text.replacen("数字由示例市统计局发布。", "远低于全国的 17.6%。", 1) } else { text });
    let run = kit.investigate();
    assert_eq!(kit.planner.calls(), ["research_plan:plan", "research_conclude:conclude#1", "research_conclude:conclude#2"]);
    assert!(run.conclusions.iter().all(|k| k.issues.is_empty()));
    assert!(!run.conclusions[0].text().contains("17.6"));
    let steps: Vec<_> = run.steps.iter().filter(|s| s.kind == StepKind::Conclude).collect();
    assert_eq!(steps.len(), 2);
    assert!(steps[0].note.as_deref().unwrap().contains("「17.6%」对不上"));

    // 重写了还是有：留在结论里，但标出来
    let mut kit = Kit::new(1);
    kit.planner = MockModel::new("mock-planner", 1).tampering(|req, text| if req.purpose == Purpose::Conclude { text.replacen("数字由示例市统计局发布。", "远低于全国的 17.6%。", 1) } else { text });
    let run = kit.investigate();
    assert_eq!(run.status, Status::Done);
    assert_eq!(run.conclusions[0].issues, ["「17.6%」对不上这条结论引用的任何事实或计算"]);
    assert!(run.conclusions[0].segments.iter().any(|s| matches!(s, Segment::Unmatched { text, .. } if text == "17.6%")));
    assert!(run.conclusions[1..].iter().all(|k| k.issues.is_empty()));
    let r = report::build(&run, None);
    assert!(r.notes.iter().any(|n| n.contains("结论里有 1 处没有对上")));
    let doc = render::document(&r);
    assert!(render::markdown(&doc).contains("**⚠ 17.6%**"));
    assert!(render::html(&doc).contains("<mark title=\"对不上引用的任何事实或计算\">17.6%</mark>"));
    // 如实标出来的不算审计问题
    assert_eq!(audit::audit(&run, &mut texts(1)), []);
}

#[test]
fn a_low_credibility_source_ranked_first_is_blocked() {
    let run = Kit::new(1).investigate();
    let mut r = report::build(&run, None);
    assert!(audit::audit_order(&r).is_empty());
    let low = r.sources.iter().position(|s| s.source.tier == Some(Tier::Low)).unwrap();
    let s = r.sources.remove(low);
    r.sources.insert(0, s);
    let found = audit::audit_order(&r);
    assert!(found.len() == 1 && found[0].kind == Kind::Order && found[0].message.contains("排在了更可信的 S3 前面"), "{found:?}");
    // 改了等级表：同一批事实重新定级、重新排
    let mut custom = run.clone();
    custom.config.trust.tiers.insert(research_engine::trust::ProducerKind::CnGovStats, Tier::High);
    flow::verify_all(&mut custom, &mut texts(1));
    let r = report::build(&custom, None);
    assert_eq!(r.table_name, "自定义等级表");
    assert_eq!(r.sources[0].source.id, "S1");
    assert!(audit::audit_order(&r).is_empty());
}

// ---------- 改坏测试：存下来的结果被改坏，审计要查出来 ----------

#[test]
fn audit_catches_every_kind_of_tampering_with_stored_results() {
    let good = Kit::new(1).investigate();
    let kinds = |run: &Run| -> Vec<(Kind, String)> { audit::audit(run, &mut texts(1)).into_iter().map(|f| (f.kind, f.target)).collect() };
    assert!(kinds(&good).is_empty());
    let f1 = good.facts.iter().position(|f| f.id == "F1").unwrap();

    // 摘录改一个字
    let mut run = good.clone();
    run.facts[f1].quote = run.facts[f1].quote.replace("调查失业率", "登记失业率");
    assert!(kinds(&run).contains(&(Kind::Quote, "F1".into())));

    // 数字改一位（连同存着的解析结果一起改也没用：摘录里没有这个数）
    let mut run = good.clone();
    run.facts[f1].value = "12.8".into();
    assert!(kinds(&run).contains(&(Kind::Value, "F1".into())));

    // 单位错
    let mut run = good.clone();
    run.facts[f1].unit = "‰".into();
    assert!(kinds(&run).contains(&(Kind::Unit, "F1".into())));

    // 没核实的被标成已核实
    let mut run = good.clone();
    let bad = run.facts.iter().position(|f| !f.verified()).unwrap();
    run.facts[bad].check.status = CheckStatus::Verified;
    assert!(kinds(&run).contains(&(Kind::Quote, run.facts[bad].id.clone())));

    // 结论里多一个对不上的数（模板和显示的文字一起改）
    let mut run = good.clone();
    run.conclusions[0].template.push_str("全国为 17.6%。");
    run.conclusions[0].segments.push(Segment::Text { text: "全国为 17.6%。".into() });
    let found = audit::audit(&run, &mut texts(1));
    assert!(found.iter().any(|f| f.kind == Kind::Conclusion && f.target == "K1" && f.message.contains("「17.6%」对不上")), "{found:?}");

    // 只改显示的数
    let mut run = good.clone();
    for s in &mut run.conclusions[0].segments {
        if let Segment::Fact { text, .. } = s {
            *text = text.replace("12.3%", "2.3%");
        }
    }
    assert!(kinds(&run).contains(&(Kind::Conclusion, "K1".into())));

    // 计算结果被改
    let mut run = good.clone();
    run.calcs[0].shown = "9.9 个百分点".into();
    assert!(kinds(&run).contains(&(Kind::Calc, "C1".into())));

    // 可信度被抬高
    let mut run = good.clone();
    run.facts[f1].tier = Tier::High;
    assert!(kinds(&run).contains(&(Kind::Tier, "F1".into())));

    // 保存的正文被换掉
    let mut swapped = |s: &Source| if s.id == "S1" { Ok("别的内容".to_string()) } else { MockFetch::new(1).text(s) };
    assert!(audit::audit(&good, &mut swapped).iter().any(|f| f.kind == Kind::Quote && f.target == "F1"));
}

// ---------- 取消、失败、接着做 ----------

#[test]
fn cancel_stops_at_the_current_step_and_can_resume() {
    let mut kit = Kit::new(1);
    let (flag, seen) = (kit.cancel.clone(), Arc::new(AtomicUsize::new(0)));
    let counter = seen.clone();
    kit.worker = MockModel::new("mock-worker", 1).tampering(move |_, text| {
        // 读到第 3 个来源时用户点了「停止」
        if counter.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
            flag.store(true, Ordering::SeqCst);
        }
        text
    });
    let mut run = kit.plan();
    kit.execute(&mut run);
    assert_eq!((run.stage, run.status, run.error.clone()), (Stage::Extract, Status::Cancelled, None));
    assert_eq!(run.sources.iter().filter(|s| s.extracted).count(), 3);
    assert!(run.conclusions.is_empty());
    assert_eq!(kit.saves.last.as_ref().unwrap().status, Status::Cancelled);
    assert!(report::build(&run, None).notes.iter().any(|n| n.contains("中途停止")));

    // 接着做：搜过的不再搜，读过的不再读
    kit.cancel.store(false, Ordering::SeqCst);
    kit.execute(&mut run);
    assert_eq!(run.status, Status::Done);
    assert_eq!(seen.load(Ordering::SeqCst), 7);
    assert_eq!(run.steps.iter().filter(|s| s.kind == StepKind::Search).count(), 7);
    assert_eq!((run.facts.len(), run.conclusions.len()), (14, 5));
    assert_eq!(audit::audit(&run, &mut texts(1)), []);

    // 一开始就取消：什么都不做
    let mut kit = Kit::new(1);
    let mut run = kit.plan();
    kit.cancel.store(true, Ordering::SeqCst);
    kit.execute(&mut run);
    assert_eq!((run.stage, run.status), (Stage::Search, Status::Cancelled));
    assert!(run.searches.is_empty());
}

#[test]
fn over_budget_stops_the_run_until_the_user_continues() {
    let mut kit = Kit::new(1);
    kit.worker.fail = Some((Purpose::Extract, fixture::failure(ErrorKind::Budget, "会超过月度上限，没有发送。")));
    let mut run = kit.plan();
    kit.execute(&mut run);
    assert_eq!((run.stage, run.status), (Stage::Extract, Status::Failed));
    assert!(run.error.as_deref().unwrap().contains("月度上限"));
    assert!(run.facts.is_empty() && run.sources.iter().all(|s| !s.extracted));
    assert_eq!(kit.worker.calls().len(), 1);
    // 调高上限之后接着做
    kit.worker.fail = None;
    kit.execute(&mut run);
    assert_eq!((run.status, run.facts.len()), (Status::Done, 14));
}

#[test]
fn one_failed_source_does_not_stop_the_others() {
    let mut kit = Kit::new(1);
    kit.worker = MockModel::new("mock-worker", 1).tampering(|req, text| if req.tag == "S2#1" { "这不是 JSON".into() } else { text });
    let run = kit.investigate();
    assert_eq!(run.status, Status::Done);
    assert!(run.source("S2").unwrap().extract_error.as_deref().unwrap().contains("读不懂"));
    assert_eq!(run.facts.len(), 11);
    assert!(report::build(&run, None).notes.iter().any(|n| n.contains("1 个来源摘证据时出错")));

    // 每个来源都失败（比如被限流）：做完，但没有结论，写明原因
    let mut kit = Kit::new(1);
    kit.worker.fail = Some((Purpose::Extract, fixture::failure(ErrorKind::Failed, "调用太频繁，被限流了。")));
    let run = kit.investigate();
    assert_eq!(run.status, Status::Done);
    assert!(run.conclusions.is_empty() && run.gaps[0].contains("没有核实到任何事实"));
    assert_eq!(kit.planner.calls(), ["research_plan:plan"]);
    assert!(run.steps.iter().filter(|s| s.kind == StepKind::Extract).all(|s| !s.ok));
}

struct Offline;

impl Fetch for Offline {
    fn fetch(&self, _: &str, _: &AtomicBool) -> Result<Fetched, String> {
        Err("连不上网络".into())
    }
    fn text(&self, _: &Source) -> Result<String, String> {
        Err("没有保存".into())
    }
}

#[test]
fn nothing_to_read_fails_with_a_reason() {
    // 下载全失败
    let mut kit = Kit::new(1);
    kit.fetch = Box::new(Offline);
    let run = kit.investigate();
    assert_eq!((run.stage, run.status), (Stage::Download, Status::Failed));
    assert!(run.error.as_deref().unwrap().contains("9 个来源都读不了（例如：连不上网络）"));

    // 搜索全失败、也没有手动加的来源
    let mut kit = Kit::new(1);
    kit.search.fail = Some(fixture::failure(ErrorKind::Failed, "搜索服务出错（500）"));
    let mut run = kit.plan();
    kit.execute(&mut run);
    assert_eq!((run.stage, run.status), (Stage::Search, Status::Failed));
    assert!(run.error.as_deref().unwrap().contains("搜索出错：搜索服务出错（500）"));
    // 手动加一个网址就能往下做
    assert_eq!(flow::add_manual_source(&mut run, &format!("{}/stats/bulletin-2024.html", fixture::BASE)).unwrap(), "S1");
    kit.execute(&mut run);
    assert_eq!(run.status, Status::Done);
    assert_eq!((run.sources.len(), run.facts.len()), (1, 3));
    assert_eq!(run.conclusions.len(), 2);

    // 密钥不对：马上停，不一个个试下去
    let mut kit = Kit::new(1);
    kit.search.fail = Some(fixture::failure(ErrorKind::Fatal, "搜索的密钥不对"));
    let mut run = kit.plan();
    kit.execute(&mut run);
    assert_eq!(run.status, Status::Failed);
    assert_eq!(run.steps.iter().filter(|s| s.kind == StepKind::Search).count(), 1);
}

#[test]
fn the_user_can_drop_sub_questions_before_running() {
    let mut kit = Kit::new(1);
    let mut run = kit.plan();
    // 只留样本市和招聘平台两个子问题
    for s in &mut run.plan.sub_questions {
        s.enabled = s.id == "Q5" || s.id == "Q6";
    }
    kit.execute(&mut run);
    assert_eq!(run.status, Status::Done);
    assert_eq!(run.searches.len(), 2);
    assert_eq!(run.sources.len(), 2);
    // 模拟模型按事实表里有的事实写结论，编号对得上
    assert_eq!(run.conclusions.len(), 1);
    assert!(run.conclusions[0].issues.is_empty() && run.conclusions[0].text().contains("48200个"));
    assert_eq!(audit::audit(&run, &mut texts(1)), []);
}

/// 本机下载失败、由别的办法（Claude 网页抓取）拿到的来源：来源上记着、费用记成一步、报告里写明（`decisions/0023`）
struct Rescue(MockFetch);

impl Fetch for Rescue {
    fn fetch(&self, url: &str, cancel: &AtomicBool) -> Result<Fetched, String> {
        match self.0.fetch(url, cancel) {
            Err(why) if url.ends_with(fixture::MISSING) => Ok(Fetched {
                final_url: url.into(),
                fetched_at: "2026-11-12T09:00:01".into(),
                sha256: "ab".repeat(32),
                kind: "text".into(),
                content_type: Some("text/plain".into()),
                ext: "txt".into(),
                bytes: 40,
                title: Some("抓到的旧公报".into()),
                text: Ok("2023 年示例市旧公报：这一页只有一些说明文字，没有数字。".into()),
                via: Some(research_engine::ports::Via { how: "claude_web_fetch".into(), download_error: why, provider: "anthropic".into(), model: "claude-haiku-5-5".into(), usage: research_engine::ports::Usage { input_tokens: 3000, output_tokens: 20, searches: 0, cost_usd: 0.0016 } }),
            }),
            other => other,
        }
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        if source.url.ends_with(fixture::MISSING) { Ok("2023 年示例市旧公报：这一页只有一些说明文字，没有数字。".into()) } else { self.0.text(source) }
    }
}

#[test]
fn a_source_rescued_by_claude_web_fetch_is_marked_and_charged() {
    let mut kit = Kit::new(1);
    kit.fetch = Box::new(Rescue(MockFetch::new(1)));
    let run = kit.investigate();
    let s = run.sources.iter().find(|s| s.url.ends_with(fixture::MISSING)).expect("夹具里有这个 404 的来源");
    assert_eq!((s.fetched_via.as_deref(), s.fetch_error.as_deref()), (Some("claude_web_fetch"), None));
    assert!(s.usable());
    let step = run.steps.iter().find(|x| x.label.contains("改用 Claude 网页抓取")).expect("费用记成一步");
    assert_eq!((step.kind, step.provider.as_str(), step.model.as_str()), (StepKind::Download, "anthropic", "claude-haiku-5-5"));
    assert!(step.label.contains("404") && (step.cost_usd - 0.0016).abs() < 1e-12);
    assert!(run.cost_usd() >= 0.0016);
    let r = report::build(&run, None);
    assert_eq!(r.totals.fetched_by_claude, 1);
    assert!(r.notes.iter().any(|n| n.contains("改用 Claude 网页抓取")), "{:?}", r.notes);
    assert!(render::markdown(&render::document(&r)).contains("经 Claude 网页抓取"));
}
