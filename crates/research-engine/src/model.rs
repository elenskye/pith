//! 一次调查里的数据。整份 `Run` 可以存成 JSON、读回来接着做（`src-tauri` 把它拆进主库的 research_* 表）。
//! 字段名在 JSON 里是 camelCase，界面直接用。

use serde::{Deserialize, Serialize};

use crate::dec::Dec;
use crate::fold::Span;
use crate::numbers::Found;
use crate::trust::{ProducerKind, Tier, TrustTable};

// ---------- 计划 ----------

/// 子问题从哪个角度量：官方统计，还是官方统计之外的维度（招聘平台、独立调查、社保、人口流动……）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Angle {
    #[default]
    Official,
    Alternative,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubQuestion {
    /// Q1、Q2……
    pub id: String,
    pub text: String,
    pub metric: String,
    /// 口径
    pub definition: String,
    pub population: String,
    pub region: String,
    pub period: String,
    pub angle: Angle,
    /// 候选来源类型
    pub source_types: Vec<String>,
    /// 搜索词
    pub queries: Vec<String>,
    /// 用户在确认计划时可以关掉
    pub enabled: bool,
}

/// 官方统计之外的衡量维度
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dimension {
    pub name: String,
    pub why: String,
    pub sources: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Plan {
    /// 模型对问题的理解
    pub summary: String,
    pub sub_questions: Vec<SubQuestion>,
    pub dimensions: Vec<Dimension>,
    /// 口径上要注意的地方（官方数字能不能直接比……）
    pub caveats: Vec<String>,
    /// 读回模型输出时改正、丢掉的地方
    pub problems: Vec<String>,
}

// ---------- 检索与来源 ----------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hit {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchRecord {
    /// 为哪个子问题搜的；计划时摸底搜到的是空串
    pub sub_question: String,
    pub query: String,
    pub engine: String,
    pub hits: Vec<Hit>,
    pub error: Option<String>,
    pub at: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Search,
    /// 用户手动加的网址或文件
    Manual,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Source {
    /// S1、S2……
    pub id: String,
    /// 网址；手动加的文件是它的路径
    pub url: String,
    pub origin: Origin,
    /// 哪些子问题搜到了它
    pub sub_questions: Vec<String>,
    pub title: String,
    // 下载（本机）之后
    pub final_url: Option<String>,
    pub fetched_at: Option<String>,
    pub sha256: Option<String>,
    /// html / pdf / xlsx / csv / text
    pub kind: Option<String>,
    pub content_type: Option<String>,
    /// 原件的扩展名（快照文件名用）
    pub ext: Option<String>,
    pub bytes: u64,
    pub text_chars: usize,
    /// 下载失败的原因
    pub fetch_error: Option<String>,
    /// 下载到了但取不出文字的原因（「取文字失败」，不静默）
    pub text_error: Option<String>,
    // 摘证据之后
    pub extracted: bool,
    /// 发给模型的字数；正文太长没读完时 `truncated`
    pub read_chars: usize,
    pub truncated: bool,
    pub extract_error: Option<String>,
    /// 模型从正文里读到的发布者、发布时间
    pub publisher: String,
    pub published: String,
    /// 由事实推出：这个来源里已核实事实的最高等级
    pub tier: Option<Tier>,
    /// 加入文献库后对应的条目
    pub item_id: Option<i64>,
    /// 正文。不进 JSON：存在快照的 .txt 里，用到时再读
    #[serde(skip)]
    pub text: Option<String>,
}

impl Source {
    pub fn usable(&self) -> bool {
        self.fetch_error.is_none() && self.text_error.is_none() && self.sha256.is_some()
    }
}

// ---------- 事实 ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// 还没核对
    #[default]
    Pending,
    Verified,
    Unverified,
}

/// 核对的结果（代码填，不由模型给）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Check {
    pub status: CheckStatus,
    /// 没核上的原因
    pub reason: Option<String>,
    /// 摘录在正文里的位置（按字符）
    pub span: Option<Span>,
    /// 摘录在正文里出现了几处
    pub occurrences: usize,
    /// 数值解析的结果
    pub parsed: Option<Found>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    /// F1、F2……
    pub id: String,
    /// 来源编号
    pub source: String,
    #[serde(default)]
    pub sub_question: String,
    pub metric: String,
    /// 数值原文（「12.3」「1.2万」「12.3–15.6」）
    pub value: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub population: String,
    #[serde(default)]
    pub period: String,
    /// 口径
    #[serde(default)]
    pub definition: String,
    /// 数据生产者：谁统计的，不是谁转述的
    pub producer: String,
    pub producer_kind: ProducerKind,
    /// 这个来源是在转述别人的数据
    #[serde(default)]
    pub relayed: bool,
    /// 原文摘录（逐字）
    pub quote: String,
    /// 单位不在摘录里时（表格）：写明单位的那一处原文
    #[serde(default)]
    pub unit_quote: String,
    #[serde(default)]
    pub check: Check,
    pub tier: Tier,
}

impl Fact {
    pub fn verified(&self) -> bool {
        self.check.status == CheckStatus::Verified
    }

    /// 给人看的值：数值原文 + 单位（数值原文里已经带单位就不重复）
    pub fn shown(&self) -> String {
        let (v, u) = (self.value.trim(), self.unit.trim());
        if u.is_empty() || v.ends_with(u) { v.to_string() } else { format!("{v}{}{u}", if u.starts_with(|c: char| c.is_ascii_alphabetic()) { " " } else { "" }) }
    }

    /// 重查时对齐用的键：（指标、地区、人群、时间），折叠后比较
    pub fn key(&self) -> String {
        [&self.metric, &self.region, &self.population, &self.period].iter().map(|s| crate::fold::fold(s).to_lowercase()).collect::<Vec<_>>().join("|")
    }
}

// ---------- 计算与结论 ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// 差：a − b
    Diff,
    /// 比：a ÷ b（倍）
    Ratio,
    /// 变化率（同比、环比）：(新 − 旧) ÷ 旧 × 100%
    PctChange,
    /// 平均
    Mean,
    /// 合计
    Sum,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Diff => "差",
            Op::Ratio => "比",
            Op::PctChange => "变化率",
            Op::Mean => "平均",
            Op::Sum => "合计",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calc {
    /// C1、C2……
    pub id: String,
    pub op: Op,
    /// 事实编号
    pub inputs: Vec<String>,
    #[serde(default)]
    pub label: String,
    /// 显示几位小数
    pub decimals: u32,
    /// 算出来的值（除法保留 8 位）；算不了是 `None`，原因在 `error`
    #[serde(default)]
    pub result: Option<Dec>,
    /// 按 `decimals` 四舍五入后的写法，带单位
    #[serde(default)]
    pub shown: String,
    #[serde(default)]
    pub unit: String,
    /// 算式，给人看
    #[serde(default)]
    pub steps: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// 结论为什么不确定
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Uncertainty {
    /// 找不到
    NotFound,
    /// 来源互相矛盾
    Conflicting,
    /// 已过时
    Outdated,
    /// 口径没说清
    UnclearDefinition,
    /// 只有低可信来源（由代码按等级表判断）
    LowTrustOnly,
}

impl Uncertainty {
    pub const ALL: [Uncertainty; 5] = [Uncertainty::NotFound, Uncertainty::Conflicting, Uncertainty::Outdated, Uncertainty::UnclearDefinition, Uncertainty::LowTrustOnly];

    pub fn key(self) -> &'static str {
        match self {
            Uncertainty::NotFound => "not_found",
            Uncertainty::Conflicting => "conflicting",
            Uncertainty::Outdated => "outdated",
            Uncertainty::UnclearDefinition => "unclear_definition",
            Uncertainty::LowTrustOnly => "low_trust_only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Uncertainty::NotFound => "找不到",
            Uncertainty::Conflicting => "互相矛盾",
            Uncertainty::Outdated => "已过时",
            Uncertainty::UnclearDefinition => "口径没说清",
            Uncertainty::LowTrustOnly => "只有低可信来源",
        }
    }
}

/// 结论的一小段：普通文字，或一个可以点回原文的数
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Text { text: String },
    /// 对上了一条已核实的事实
    Fact { id: String, text: String },
    /// 对上了一个算出来的值
    Calc { id: String, text: String },
    /// 对不上：在结论旁标出
    Unmatched { text: String, why: String },
}

impl Segment {
    pub fn text(&self) -> &str {
        match self {
            Segment::Text { text } | Segment::Fact { text, .. } | Segment::Calc { text, .. } | Segment::Unmatched { text, .. } => text,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Conclusion {
    /// K1、K2……
    pub id: String,
    /// 模型写的原样（数用 {F1}、{C1} 占位）
    pub template: String,
    /// 代码填好数、逐个核对之后的样子
    pub segments: Vec<Segment>,
    pub facts: Vec<String>,
    pub calcs: Vec<String>,
    pub uncertainty: Vec<Uncertainty>,
    pub note: String,
    /// 核对发现的问题（对不上的数、引用了不存在或没核实的事实……）
    pub issues: Vec<String>,
}

impl Conclusion {
    pub fn text(&self) -> String {
        self.segments.iter().map(Segment::text).collect()
    }
}

// ---------- 流程 ----------

/// 做到哪一步（`decisions/0020`「流程」）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Plan,
    Search,
    Download,
    Extract,
    Verify,
    Conclude,
    Report,
}

impl Stage {
    pub const ALL: [Stage; 7] = [Stage::Plan, Stage::Search, Stage::Download, Stage::Extract, Stage::Verify, Stage::Conclude, Stage::Report];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Plan => "计划",
            Stage::Search => "检索",
            Stage::Download => "下载",
            Stage::Extract => "摘证据",
            Stage::Verify => "核对",
            Stage::Conclude => "结论",
            Stage::Report => "报告",
        }
    }

    pub fn next(self) -> Option<Stage> {
        let i = Stage::ALL.iter().position(|s| *s == self)?;
        Stage::ALL.get(i + 1).copied()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// 计划出来了，等用户确认
    #[default]
    AwaitingPlan,
    Running,
    /// 用户取消了；可以从 `stage` 接着做
    Cancelled,
    /// 出错停下了（原因在 `error`）；可以从 `stage` 接着做
    Failed,
    Done,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelRef {
    /// anthropic / openai_compatible / mock
    pub provider: String,
    pub model: String,
}

/// 一次调查的量的上限（防止一个问题搜几百次、读几百页）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    pub sub_questions: usize,
    pub queries_per_sub_question: usize,
    pub results_per_query: usize,
    pub sources: usize,
    /// 一个来源一次发给模型多少字
    pub chunk_chars: usize,
    /// 一个来源最多发几段
    pub chunks_per_source: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { sub_questions: 12, queries_per_sub_question: 3, results_per_query: 5, sources: 40, chunk_chars: 20_000, chunks_per_source: 3 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 定计划、下结论的模型
    pub planner: ModelRef,
    /// 读网页、摘证据的模型
    pub worker: ModelRef,
    /// 检索用的引擎（执行模型绑定的专用搜索，没有就是 tavily）
    pub search_engine: String,
    pub trust: TrustTable,
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Plan,
    Search,
    Download,
    Extract,
    Verify,
    Conclude,
    Report,
}

/// 做过的一步和它的费用
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub kind: StepKind,
    pub label: String,
    pub at: String,
    /// 调了哪个接口（没联网的步骤是空串）
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// 搜索次数（Tavily 是点数）
    #[serde(default)]
    pub searches: u64,
    #[serde(default)]
    pub cost_usd: f64,
    /// 模拟的接口：不联网、不花钱
    #[serde(default)]
    pub mock: bool,
    pub ok: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Run {
    /// 对外的编号（调用方给）
    pub uid: String,
    pub question: String,
    /// 同一问题的第几次调查（从 1 起）
    pub seq: u32,
    /// 重查时：上一次调查的编号
    pub previous: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    /// 接下来（或正在）做哪一步
    pub stage: Stage,
    pub status: Status,
    pub error: Option<String>,
    /// 用的是模拟接口
    pub mock: bool,
    pub config: Config,
    pub plan: Plan,
    pub searches: Vec<SearchRecord>,
    pub sources: Vec<Source>,
    pub facts: Vec<Fact>,
    pub calcs: Vec<Calc>,
    pub conclusions: Vec<Conclusion>,
    /// 模型说没找到、回答不了的部分
    pub gaps: Vec<String>,
    pub steps: Vec<Step>,
}

impl Run {
    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn fact(&self, id: &str) -> Option<&Fact> {
        self.facts.iter().find(|f| f.id == id)
    }

    pub fn calc(&self, id: &str) -> Option<&Calc> {
        self.calcs.iter().find(|c| c.id == id)
    }

    pub fn cost_usd(&self) -> f64 {
        self.steps.iter().filter(|s| !s.mock).map(|s| s.cost_usd).sum()
    }

    pub fn verified_facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts.iter().filter(|f| f.verified())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_run_in_the_planned_order() {
        let labels: Vec<&str> = Stage::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["计划", "检索", "下载", "摘证据", "核对", "结论", "报告"]);
        assert_eq!(Stage::Plan.next(), Some(Stage::Search));
        assert_eq!(Stage::Report.next(), None);
        assert!(Stage::Verify < Stage::Conclude);
    }

    #[test]
    fn a_run_survives_json() {
        let mut run = Run { uid: "U".into(), question: "示例市的青年失业率是多少？".into(), seq: 1, ..Default::default() };
        run.sources.push(Source { id: "S1".into(), url: "https://stats.example.invalid/a".into(), text: Some("正文不进 JSON".into()), ..Default::default() });
        run.conclusions.push(Conclusion { id: "K1".into(), segments: vec![Segment::Text { text: "失业率为 ".into() }, Segment::Fact { id: "F1".into(), text: "12.3%".into() }], ..Default::default() });
        let json = serde_json::to_string(&run).unwrap();
        assert!(!json.contains("正文不进 JSON") && json.contains("\"subQuestions\"") && json.contains("\"kind\":\"fact\""));
        let mut back: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sources[0].text, None);
        back.sources[0].text = run.sources[0].text.clone();
        assert_eq!(back, run);
        assert_eq!(back.conclusions[0].text(), "失业率为 12.3%");
        // 旧数据缺字段也读得回
        let sparse: Run = serde_json::from_str(r#"{"uid":"X","question":"q"}"#).unwrap();
        assert_eq!((sparse.stage, sparse.status), (Stage::Plan, Status::AwaitingPlan));
    }

    #[test]
    fn fact_display_and_key() {
        let f = |value: &str, unit: &str| Fact {
            id: "F1".into(),
            source: "S1".into(),
            sub_question: String::new(),
            metric: "青年 失业率".into(),
            value: value.into(),
            unit: unit.into(),
            region: "示例市".into(),
            population: "16–24 岁".into(),
            period: "２０２４年".into(),
            definition: String::new(),
            producer: "示例统计局".into(),
            producer_kind: ProducerKind::CnGovStats,
            relayed: false,
            quote: String::new(),
            unit_quote: String::new(),
            check: Check::default(),
            tier: Tier::Low,
        };
        assert_eq!(f("12.3", "%").shown(), "12.3%");
        assert_eq!(f("12.3%", "%").shown(), "12.3%");
        assert_eq!(f("1.2", "万人").shown(), "1.2万人");
        assert_eq!(f("1.25", "").shown(), "1.25");
        assert_eq!(f("12.3", "%").key(), "青年失业率|示例市|16–24岁|2024年");
    }
}
