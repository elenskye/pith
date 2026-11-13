//! 报告的内容（`decisions/0020`「报告」）：短答在前，展开看子问题、搜索、来源、事实表、计算过程和费用。
//!
//! `build` 把一次调查整理成给人看的样子：来源和事实按可信度从高到低排，写明用的是哪张等级表。
//! 界面、Markdown、HTML、Word 都从这一份 `Report` 出，不各排各的。

use serde::Serialize;

use crate::conclude::fact_number;
use crate::diff::{self, Diff};
use crate::model::{Calc, Conclusion, Dimension, Fact, Run, SearchRecord, Source, Status, Step, SubQuestion};
use crate::trust::{Tier, first_out_of_order};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    pub tier: Tier,
    pub label: &'static str,
    /// 这一等级的数据生产者类型（中文名）
    pub kinds: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactRow {
    #[serde(flatten)]
    pub fact: Fact,
    pub producer_kind_label: &'static str,
    pub tier_label: &'static str,
    pub source_title: String,
    pub source_url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    #[serde(flatten)]
    pub source: Source,
    pub tier_label: Option<&'static str>,
    /// 这个来源摘到几条事实、其中几条核实了
    pub facts: usize,
    pub verified: usize,
    /// 数据生产者（已核实的事实里出现过的）
    pub producers: Vec<String>,
    /// ok 可读并摘过 / failed 下载失败 / no_text 取不出文字 / pending 还没做到
    pub state: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub sub_questions: usize,
    pub searches: usize,
    pub sources: usize,
    pub readable: usize,
    pub download_failed: usize,
    /// 本机下载失败、改用 Claude 网页抓取拿到的
    pub fetched_by_claude: usize,
    pub no_text: usize,
    pub facts: usize,
    pub verified: usize,
    pub unverified: usize,
    /// 结论里对不上的地方
    pub conclusion_issues: usize,
    pub cost_usd: f64,
    pub calls: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub uid: String,
    pub question: String,
    pub seq: u32,
    pub status: Status,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// 模拟接口给的示例，不是真的调查结果
    pub mock: bool,
    pub planner_model: String,
    pub worker_model: String,
    pub search_engine: String,
    pub conclusions: Vec<Conclusion>,
    pub gaps: Vec<String>,
    /// 用的是哪张可信度等级表
    pub table_name: &'static str,
    pub table: Vec<TierRow>,
    /// 和默认表不同的地方（一句一条）
    pub table_changes: Vec<String>,
    pub summary: String,
    pub sub_questions: Vec<SubQuestion>,
    pub dimensions: Vec<Dimension>,
    pub caveats: Vec<String>,
    pub plan_problems: Vec<String>,
    pub searches: Vec<SearchRecord>,
    /// 按可信度从高到低；没有已核实事实的、读不了的排在后面
    pub sources: Vec<SourceRow>,
    /// 已核实的在前，按可信度从高到低；没核实的在后
    pub facts: Vec<FactRow>,
    pub calcs: Vec<Calc>,
    pub steps: Vec<Step>,
    pub totals: Totals,
    /// 要让读的人知道的情况（下载失败、正文没读完……）
    pub notes: Vec<String>,
    /// 和上一次调查的对比（重查时）
    pub diff: Option<Diff>,
}

fn tier_rank(t: Option<Tier>) -> u8 {
    match t {
        Some(Tier::High) => 0,
        Some(Tier::Medium) => 1,
        Some(Tier::Low) => 2,
        None => 3,
    }
}

pub fn build(run: &Run, previous: Option<&Run>) -> Report {
    let trust = &run.config.trust;
    let table = [Tier::High, Tier::Medium, Tier::Low].into_iter().map(|t| TierRow { tier: t, label: t.label(), kinds: trust.kinds_at(t).into_iter().map(|k| k.label()).collect() }).collect();
    let table_changes = trust.changes().into_iter().map(|(k, was, now)| format!("{}：默认「{}」，现在「{}」", k.label(), was.label(), now.label())).collect();

    let mut facts: Vec<FactRow> = run
        .facts
        .iter()
        .map(|f| {
            let s = run.source(&f.source);
            FactRow {
                producer_kind_label: f.producer_kind.label(),
                tier_label: f.tier.label(),
                source_title: s.map(|s| s.title.clone()).unwrap_or_default(),
                source_url: s.map(|s| s.final_url.clone().unwrap_or_else(|| s.url.clone())).unwrap_or_default(),
                fact: f.clone(),
            }
        })
        .collect();
    facts.sort_by_key(|a| (!a.fact.verified(), tier_rank(Some(a.fact.tier)), fact_number(&a.fact.id)));

    let mut sources: Vec<SourceRow> = run
        .sources
        .iter()
        .map(|s| {
            let mine: Vec<&Fact> = run.facts.iter().filter(|f| f.source == s.id).collect();
            let mut producers: Vec<String> = Vec::new();
            for f in mine.iter().filter(|f| f.verified() && !f.producer.is_empty()) {
                if !producers.contains(&f.producer) {
                    producers.push(f.producer.clone());
                }
            }
            let state = if s.fetch_error.is_some() {
                "failed"
            } else if s.text_error.is_some() {
                "no_text"
            } else if s.sha256.is_some() {
                "ok"
            } else {
                "pending"
            };
            SourceRow { tier_label: s.tier.map(Tier::label), facts: mine.len(), verified: mine.iter().filter(|f| f.verified()).count(), producers, state, source: s.clone() }
        })
        .collect();
    let state_rank = |s: &SourceRow| match s.state {
        "ok" => 0,
        "pending" => 1,
        "no_text" => 2,
        _ => 3,
    };
    sources.sort_by_key(|a| (tier_rank(a.source.tier), state_rank(a), fact_number(&a.source.id)));

    let totals = Totals {
        sub_questions: run.plan.sub_questions.iter().filter(|s| s.enabled).count(),
        searches: run.searches.iter().filter(|s| !s.sub_question.is_empty()).count(),
        sources: run.sources.len(),
        readable: run.sources.iter().filter(|s| s.usable()).count(),
        download_failed: run.sources.iter().filter(|s| s.fetch_error.is_some()).count(),
        fetched_by_claude: run.sources.iter().filter(|s| s.fetched_via.is_some() && s.fetch_error.is_none()).count(),
        no_text: run.sources.iter().filter(|s| s.text_error.is_some()).count(),
        facts: run.facts.len(),
        verified: run.verified_facts().count(),
        unverified: run.facts.iter().filter(|f| f.check.status == crate::model::CheckStatus::Unverified).count(),
        conclusion_issues: run.conclusions.iter().map(|c| c.issues.len()).sum(),
        cost_usd: run.cost_usd(),
        calls: run.steps.iter().filter(|s| !s.provider.is_empty()).count(),
    };

    let mut notes = Vec::new();
    if run.mock {
        notes.push("这是模拟接口给的示例：不联网、不花钱，数字是虚构的。".to_string());
    }
    notes.push(format!("可信度按「{}」排：{}。官方数字照样列出，只是按表降权并标明。", trust.name(), if trust.is_default() { "中国政府官方统计和官方媒体的数字列为低".to_string() } else { format!("和默认表不同的有 {} 项", trust.changes().len()) }));
    if totals.fetched_by_claude > 0 {
        notes.push(format!("{} 个来源本机下载失败，改用 Claude 网页抓取：读的是 Claude 抓到的正文（网页转成的文字或原 PDF），费用记在「过程与费用」里。", totals.fetched_by_claude));
    }
    if totals.download_failed > 0 {
        notes.push(format!("{} 个来源下载失败，没有读到。", totals.download_failed));
    }
    if totals.no_text > 0 {
        notes.push(format!("{} 个来源下载到了但取不出文字（比如扫描版 PDF），没有读。", totals.no_text));
    }
    let truncated: Vec<&str> = run.sources.iter().filter(|s| s.truncated).map(|s| s.id.as_str()).collect();
    if !truncated.is_empty() {
        notes.push(format!("{} 的正文太长，只读了前面一部分。", truncated.join("、")));
    }
    let failed_extract = run.sources.iter().filter(|s| s.extract_error.is_some()).count();
    if failed_extract > 0 {
        notes.push(format!("{failed_extract} 个来源摘证据时出错，可能漏了事实。"));
    }
    if totals.unverified > 0 {
        notes.push(format!("{} 条事实没有核实（摘录或数值在原文里对不上），没有用在结论里。", totals.unverified));
    }
    if totals.conclusion_issues > 0 {
        notes.push(format!("结论里有 {} 处没有对上已核实的事实，已在结论旁标出。", totals.conclusion_issues));
    }
    match run.status {
        Status::Cancelled => notes.push("这次调查被中途停止，结果不完整。".into()),
        Status::Failed => notes.push(format!("这次调查没有做完：{}", run.error.clone().unwrap_or_default())),
        _ => {}
    }

    Report {
        uid: run.uid.clone(),
        question: run.question.clone(),
        seq: run.seq,
        status: run.status,
        created_at: run.created_at.clone(),
        finished_at: run.finished_at.clone(),
        mock: run.mock,
        planner_model: run.config.planner.model.clone(),
        worker_model: run.config.worker.model.clone(),
        search_engine: run.config.search_engine.clone(),
        conclusions: run.conclusions.clone(),
        gaps: run.gaps.clone(),
        table_name: trust.name(),
        table,
        table_changes,
        summary: run.plan.summary.clone(),
        sub_questions: run.plan.sub_questions.clone(),
        dimensions: run.plan.dimensions.clone(),
        caveats: run.plan.caveats.clone(),
        plan_problems: run.plan.problems.clone(),
        searches: run.searches.clone(),
        sources,
        facts,
        calcs: run.calcs.clone(),
        steps: run.steps.clone(),
        totals,
        notes,
        diff: previous.map(|p| diff::compare(p, run)),
    }
}

/// 报告里的顺序对不对：来源、已核实的事实、每条结论引用的事实，都应当按可信度从高到低。返回发现的问题。
pub fn order_problems(report: &Report) -> Vec<String> {
    let mut out = Vec::new();
    let source_tiers: Vec<Tier> = report.sources.iter().filter_map(|s| s.source.tier).collect();
    if let Some(i) = first_out_of_order(&source_tiers) {
        let rated: Vec<&SourceRow> = report.sources.iter().filter(|s| s.source.tier.is_some()).collect();
        out.push(format!("来源没有按可信度排：{}（{}）排在了更可信的 {} 前面", rated[i - 1].source.id, rated[i - 1].tier_label.unwrap_or(""), rated[i].source.id));
    }
    // 没有可信度的（没有已核实事实）不能排在有的前面
    if let Some(first_unrated) = report.sources.iter().position(|s| s.source.tier.is_none())
        && report.sources[first_unrated..].iter().any(|s| s.source.tier.is_some())
    {
        out.push(format!("来源没有按可信度排：没有已核实事实的 {} 排在了前面", report.sources[first_unrated].source.id));
    }
    let verified: Vec<&FactRow> = report.facts.iter().filter(|f| f.fact.verified()).collect();
    let fact_tiers: Vec<Tier> = verified.iter().map(|f| f.fact.tier).collect();
    if let Some(i) = first_out_of_order(&fact_tiers) {
        out.push(format!("事实没有按可信度排：{}（{}）排在了更可信的 {} 前面", verified[i - 1].fact.id, verified[i - 1].tier_label, verified[i].fact.id));
    }
    if let Some(first_unverified) = report.facts.iter().position(|f| !f.fact.verified())
        && report.facts[first_unverified..].iter().any(|f| f.fact.verified())
    {
        out.push(format!("事实表里没核实的 {} 排在了已核实的前面", report.facts[first_unverified].fact.id));
    }
    for k in &report.conclusions {
        let tiers: Vec<Tier> = k.facts.iter().filter_map(|id| report.facts.iter().find(|f| &f.fact.id == id)).map(|f| f.fact.tier).collect();
        if first_out_of_order(&tiers).is_some() {
            out.push(format!("结论 {} 引用的事实没有按可信度排", k.id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::tests::verified;
    use crate::model::CheckStatus;
    use crate::trust::ProducerKind;

    fn run() -> Run {
        let mut official = verified("F1", "12.3", "%");
        official.source = "S1".into();
        let mut academic = verified("F2", "15.8", "%");
        academic.source = "S2".into();
        academic.producer = "示例大学".into();
        academic.producer_kind = ProducerKind::Academic;
        academic.tier = Tier::High;
        let mut platform = verified("F3", "4820", "个");
        platform.source = "S3".into();
        platform.producer_kind = ProducerKind::PlatformData;
        platform.tier = Tier::Medium;
        let mut unverified = verified("F4", "9.9", "%");
        unverified.source = "S2".into();
        unverified.tier = Tier::High;
        unverified.check.status = CheckStatus::Unverified;
        let source = |id: &str, tier: Option<Tier>| Source { id: id.into(), url: format!("https://{id}.example.invalid/"), title: format!("来源 {id}"), sha256: Some(id.into()), tier, ..Default::default() };
        Run {
            uid: "R".into(),
            question: "示例市青年失业率？".into(),
            seq: 1,
            status: Status::Done,
            facts: vec![official, academic, platform, unverified],
            sources: vec![
                source("S1", Some(Tier::Low)),
                source("S2", Some(Tier::High)),
                source("S3", Some(Tier::Medium)),
                Source { id: "S4".into(), url: "https://gone.example.invalid/".into(), fetch_error: Some("404".into()), ..Default::default() },
                Source { truncated: true, ..source("S5", None) },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn sources_and_facts_are_ranked_by_credibility() {
        let r = build(&run(), None);
        assert_eq!(r.sources.iter().map(|s| s.source.id.as_str()).collect::<Vec<_>>(), ["S2", "S3", "S1", "S5", "S4"]);
        assert_eq!(r.facts.iter().map(|f| f.fact.id.as_str()).collect::<Vec<_>>(), ["F2", "F3", "F1", "F4"]);
        assert_eq!(r.sources[0].tier_label, Some("高"));
        assert_eq!((r.sources[0].facts, r.sources[0].verified, r.sources[0].producers.clone()), (2, 1, vec!["示例大学".to_string()]));
        assert_eq!(r.sources[4].state, "failed");
        assert!(order_problems(&r).is_empty());
        assert_eq!((r.totals.sources, r.totals.readable, r.totals.download_failed, r.totals.verified, r.totals.unverified), (5, 4, 1, 3, 1));
        // 写明用的是哪张表
        assert_eq!(r.table_name, "默认等级表");
        assert!(r.table[2].kinds.contains(&"中国政府官方统计"));
        assert!(r.notes.iter().any(|n| n.contains("默认等级表") && n.contains("降权")));
        assert!(r.notes.iter().any(|n| n.contains("S5 的正文太长")));
        assert!(r.notes.iter().any(|n| n.contains("1 条事实没有核实")));
        assert!(r.diff.is_none());
        assert!(build(&run(), Some(&run())).diff.unwrap().nothing_changed());
    }

    #[test]
    fn a_low_credibility_source_ranked_first_is_caught() {
        let mut r = build(&run(), None);
        // 把低可信的 S1 挪到最前面
        let low = r.sources.iter().position(|s| s.source.id == "S1").unwrap();
        let s = r.sources.remove(low);
        r.sources.insert(0, s);
        let problems = order_problems(&r);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("S1（低）排在了更可信的 S2 前面"), "{problems:?}");
        // 事实表也一样
        let mut r = build(&run(), None);
        r.facts.swap(0, 2);
        assert!(order_problems(&r)[0].contains("事实没有按可信度排"));
        // 没核实的排到前面
        let mut r = build(&run(), None);
        r.facts.swap(0, 3);
        assert!(order_problems(&r).iter().any(|p| p.contains("没核实的 F4 排在了已核实的前面")));
        // 结论引用的事实
        let mut r = build(&run(), None);
        r.conclusions.push(Conclusion { id: "K1".into(), facts: vec!["F1".into(), "F2".into()], ..Default::default() });
        assert_eq!(order_problems(&r), ["结论 K1 引用的事实没有按可信度排"]);
    }

    #[test]
    fn a_custom_table_is_named_in_the_report() {
        let mut run = run();
        run.config.trust.tiers.insert(ProducerKind::CnGovStats, Tier::High);
        let r = build(&run, None);
        assert_eq!(r.table_name, "自定义等级表");
        assert_eq!(r.table_changes, ["中国政府官方统计：默认「低」，现在「高」"]);
        assert!(r.table[0].kinds.contains(&"中国政府官方统计"));
    }
}
