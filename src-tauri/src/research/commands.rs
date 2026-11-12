//! 事实调查的 Tauri 命令（`research_*`）和它们用到的设置、密钥、后台执行。
//!
//! 一次调查要用户确认两次（`decisions/0020`「流程」第 10 条）：
//! 1. `research_plan_prepare` 算出要发什么、大概多少钱（不联网）→ 用户确认 → `research_plan_run` 让模型拆问题；
//! 2. 用户在计划页删改子问题（`research_plan_update`）、加网址或文件（`research_source_add`）→
//!    `research_execute_prepare` 估这一轮的费用 → 用户确认 → `research_execute` 在后台线程里做完检索到报告，
//!    过程中发 `research-progress` 事件，做完发 `research-done`。可以 `research_cancel`，之后可以再 `research_execute` 接着做。
//!
//! 和排版 Agent 一样：只有「应用程序」里用正式数据目录的 Pith 用真接口和钥匙串；开发版、测试、指定了数据目录的打包版
//! 用模拟接口（合成的示例市数据，不联网、不花钱），`PITH_REAL_MODEL=1` 可以显式改用真接口。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use research_engine::audit::{self, Finding};
use research_engine::fixture::{MockModel, MockSearch};
use research_engine::model::{Config, Limits, ModelRef, Plan, Run, Source, Stage, Status};
use research_engine::ports::{Model, Ports, Progress, Recorder, Search};
use research_engine::report::{self, Report};
use research_engine::trust::{ProducerKind, Tier, TrustTable};
use research_engine::{flow, prompts, render};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::anthropic::{self, Anthropic, ClaudeSearch};
use super::download::Downloader;
use super::meter::{self, Logged, Meter};
use super::openai::{self, OpenAiCompatible};
use super::snapshot::Snapshots;
use super::tavily::{Depth, Tavily};
use super::{docx, library, store};
use crate::agent::secret::{ApiKey, Place, Secret};
use crate::agent::usage::{self, Call, Ledger, Prices};
use crate::agent::{self as format_agent, AgentState};
use crate::common::new_uid;
use crate::{AppState, text};

const SETTINGS_KEY: &str = "research.settings";
const PROGRESS_EVENT: &str = "research-progress";
const DONE_EVENT: &str = "research-done";
pub const CLAUDE_WEB_SEARCH: &str = "claude_web_search";
pub const NO_SEARCH: &str = "none";

// ---------- 设置 ----------

/// 一个模型接口：哪家、哪个模型、地址（空 = 默认）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Endpoint {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
}

impl Default for Endpoint {
    fn default() -> Self {
        Endpoint { provider: anthropic::PROVIDER.into(), model: usage::MODEL.into(), base_url: None }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 定计划、下结论（默认 Opus 5.5）
    pub planner: Endpoint,
    /// 读网页、摘证据（默认 DeepSeek）
    pub worker: Endpoint,
    /// 每个模型接口的专用搜索：anthropic → claude_web_search；openai_compatible → none（用 Tavily）
    pub dedicated_search: BTreeMap<String, String>,
    pub tavily_depth: Depth,
    /// 计划时让模型用网页搜索摸底几次（0 = 不搜；只有计划模型是 Anthropic、并且它的专用搜索开着时有效）
    pub plan_web_searches: u32,
    pub trust: TrustTable,
    /// 单价覆盖
    pub prices: Prices,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            planner: Endpoint::default(),
            worker: Endpoint { provider: openai::PROVIDER.into(), model: openai::DEFAULT_MODEL.into(), base_url: Some(openai::DEFAULT_BASE.into()) },
            dedicated_search: [(anthropic::PROVIDER.to_string(), CLAUDE_WEB_SEARCH.to_string()), (openai::PROVIDER.to_string(), NO_SEARCH.to_string())].into(),
            tavily_depth: Depth::Basic,
            plan_web_searches: 3,
            trust: TrustTable::default(),
            prices: Prices::default(),
        }
    }
}

const PROVIDERS: &[&str] = &[anthropic::PROVIDER, openai::PROVIDER];

impl Settings {
    pub fn check(&self) -> Result<(), String> {
        for (who, e) in [("计划和结论的模型", &self.planner), ("执行模型", &self.worker)] {
            if !PROVIDERS.contains(&e.provider.as_str()) {
                return Err(format!("{who}的接口不认识：{}", e.provider));
            }
            let m = e.model.trim();
            if m.is_empty() || m.len() > 80 || !m.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '@')) {
                return Err(format!("{who}的模型名不对：只能有英文字母、数字和 . - _ : / @，不超过 80 个字符。"));
            }
            if let Some(b) = e.base_url.as_deref().map(str::trim).filter(|b| !b.is_empty())
                && (!b.starts_with("https://") || b.len() > 200 || b.chars().any(char::is_whitespace))
            {
                return Err(format!("{who}的接口地址要以 https:// 开头。"));
            }
        }
        for (provider, search) in &self.dedicated_search {
            let ok = search == NO_SEARCH || (search == CLAUDE_WEB_SEARCH && provider == anthropic::PROVIDER);
            if !ok {
                return Err(format!("「{provider}」不能用「{search}」当专用搜索。"));
            }
        }
        if self.plan_web_searches > 10 {
            return Err("计划时的网页搜索最多 10 次。".into());
        }
        self.prices.check()
    }

    pub fn dedicated(&self, provider: &str) -> &str {
        self.dedicated_search.get(provider).map(String::as_str).unwrap_or(NO_SEARCH)
    }

    /// 检索用执行模型绑定的专用搜索；没有就用 Tavily
    pub fn search_engine(&self) -> String {
        match self.dedicated(&self.worker.provider) {
            NO_SEARCH => "tavily".into(),
            s => s.to_string(),
        }
    }

    pub fn plan_searches(&self) -> u32 {
        if self.planner.provider == anthropic::PROVIDER && self.dedicated(anthropic::PROVIDER) == CLAUDE_WEB_SEARCH { self.plan_web_searches } else { 0 }
    }

    /// 要用到哪几把密钥
    pub fn keys_needed(&self) -> Vec<ApiKey> {
        let mut out = Vec::new();
        for p in [&self.planner.provider, &self.worker.provider] {
            let k = if p == anthropic::PROVIDER { ApiKey::Anthropic } else { ApiKey::Deepseek };
            if !out.contains(&k) {
                out.push(k);
            }
        }
        if self.search_engine() == "tavily" {
            out.push(ApiKey::Tavily);
        }
        out
    }

    fn config(&self, mock: bool) -> Config {
        if mock {
            return Config { planner: ModelRef { provider: "mock".into(), model: "模拟计划模型".into() }, worker: ModelRef { provider: "mock".into(), model: "模拟执行模型".into() }, search_engine: "mock".into(), trust: self.trust.clone().complete(), limits: Limits::default() };
        }
        Config {
            planner: ModelRef { provider: self.planner.provider.clone(), model: self.planner.model.trim().into() },
            worker: ModelRef { provider: self.worker.provider.clone(), model: self.worker.model.trim().into() },
            search_engine: self.search_engine(),
            trust: self.trust.clone().complete(),
            limits: Limits::default(),
        }
    }
}

fn settings(state: &AppState) -> Settings {
    format_agent::setting(state, SETTINGS_KEY).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn mock_mode() -> bool {
    !format_agent::use_real_model()
}

// ---------- 状态（密钥、进行中的调用） ----------

pub struct ResearchState {
    deepseek: Secret,
    tavily: Secret,
    cancel: Arc<AtomicBool>,
    /// 正在做的：「plan」或一次调查的 uid
    busy: Mutex<Option<String>>,
    next_ticket: AtomicU64,
    tickets: Mutex<HashMap<u64, Ticket>>,
}

enum Ticket {
    Plan { question: String },
    Execute { uid: String },
}

impl Default for ResearchState {
    fn default() -> Self {
        let official = format_agent::official();
        ResearchState { deepseek: Secret::for_key(ApiKey::Deepseek, official), tavily: Secret::for_key(ApiKey::Tavily, official), cancel: Arc::new(AtomicBool::new(false)), busy: Mutex::new(None), next_ticket: AtomicU64::new(1), tickets: Mutex::new(HashMap::new()) }
    }
}

impl ResearchState {
    fn ticket(&self, t: Ticket) -> u64 {
        let id = self.next_ticket.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut m) = self.tickets.lock() {
            if m.len() >= 8
                && let Some(oldest) = m.keys().min().copied()
            {
                m.remove(&oldest);
            }
            m.insert(id, t);
        }
        id
    }

    fn take(&self, id: u64) -> Result<Ticket, String> {
        self.tickets.lock().ok().and_then(|mut m| m.remove(&id)).ok_or_else(|| "这次确认已经过期，请重新确认。".to_string())
    }

    fn running(&self) -> Option<String> {
        self.busy.lock().ok().and_then(|b| b.clone())
    }

    fn claim(&self, what: &str) -> Result<(), String> {
        let mut b = self.busy.lock().map_err(|_| "内部错误".to_string())?;
        if b.is_some() {
            return Err("上一次调查还在进行。等它做完，或者先停止。".into());
        }
        *b = Some(what.to_string());
        self.cancel.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn release(&self) {
        if let Ok(mut b) = self.busy.lock() {
            *b = None;
        }
    }
}

struct Busy<'a>(&'a ResearchState);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn key_saved_setting(which: ApiKey) -> &'static str {
    match which {
        ApiKey::Anthropic => format_agent::KEY_SAVED,
        ApiKey::Deepseek => "research.key_saved.deepseek",
        ApiKey::Tavily => "research.key_saved.tavily",
    }
}

fn secret_of<'a>(which: ApiKey, agent: &'a AgentState, research: &'a ResearchState) -> &'a Secret {
    match which {
        ApiKey::Anthropic => agent.secret(),
        ApiKey::Deepseek => &research.deepseek,
        ApiKey::Tavily => &research.tavily,
    }
}

fn has_key(state: &AppState, agent: &AgentState, research: &ResearchState, which: ApiKey) -> bool {
    secret_of(which, agent, research).known_present(format_agent::setting(state, key_saved_setting(which)).as_deref() == Some("1"))
}

fn missing_keys(state: &AppState, agent: &AgentState, research: &ResearchState, s: &Settings) -> Vec<String> {
    if mock_mode() {
        return Vec::new();
    }
    s.keys_needed().into_iter().filter(|k| !has_key(state, agent, research, *k)).map(|k| k.label().to_string()).collect()
}

fn load_key(which: ApiKey, agent: &AgentState, research: &ResearchState) -> Result<String, String> {
    secret_of(which, agent, research).load()?.ok_or_else(|| format!("还没有填 {} 的 API 密钥。到「设置 › 事实调查」填写后再试。", which.label()))
}

// ---------- 接口的组装 ----------

struct Kit {
    planner: Box<dyn Model>,
    worker: Box<dyn Model>,
    search: Box<dyn Search>,
    fetch: Downloader,
}

fn model_for(e: &Endpoint, key: String, meter: &Arc<Meter>) -> Box<dyn Model> {
    if e.provider == anthropic::PROVIDER {
        Box::new(Anthropic::new(e.base_url.as_deref(), key, e.model.trim(), meter.clone()))
    } else {
        Box::new(OpenAiCompatible::new(e.base_url.as_deref(), key, Some(e.model.trim()), meter.clone()))
    }
}

fn key_for(e: &Endpoint) -> ApiKey {
    if e.provider == anthropic::PROVIDER { ApiKey::Anthropic } else { ApiKey::Deepseek }
}

/// `round`：模拟接口给第几轮的合成数据（重查时有变化）
fn kit(state: &AppState, agent: &AgentState, research: &ResearchState, s: &Settings, round: u32) -> Result<Kit, String> {
    let root = &state.paths.root;
    if mock_mode() {
        let delay = |ms| if cfg!(test) { Duration::ZERO } else { Duration::from_millis(ms) };
        let mut planner = MockModel::new("模拟计划模型", round);
        planner.delay = delay(900);
        let mut worker = MockModel::new("模拟执行模型", round);
        worker.delay = delay(250);
        let mut search = MockSearch::new(round);
        search.delay = delay(150);
        let now = text::now_iso;
        return Ok(Kit {
            planner: Box::new(Logged { inner: planner, ledger: Ledger::new(root), now }),
            worker: Box::new(Logged { inner: worker, ledger: Ledger::new(root), now }),
            search: Box::new(Logged { inner: search, ledger: Ledger::new(root), now }),
            fetch: Downloader::mock(root, round),
        });
    }
    let meter = Arc::new(Meter::new(root, format_agent::budget(state), s.prices.clone()));
    let planner = model_for(&s.planner, load_key(key_for(&s.planner), agent, research)?, &meter);
    let worker_key = load_key(key_for(&s.worker), agent, research)?;
    let worker = model_for(&s.worker, worker_key.clone(), &meter);
    let search: Box<dyn Search> = if s.search_engine() == CLAUDE_WEB_SEARCH {
        Box::new(ClaudeSearch { model: Anthropic::new(s.worker.base_url.as_deref(), worker_key, s.worker.model.trim(), meter.clone()) })
    } else {
        Box::new(Tavily::new(None, load_key(ApiKey::Tavily, agent, research)?, s.tavily_depth, meter))
    };
    Ok(Kit { planner, worker, search, fetch: Downloader::real(root) })
}

struct DbRecorder<'a> {
    state: &'a AppState,
    question_id: i64,
}

impl Recorder for DbRecorder<'_> {
    fn save(&mut self, run: &Run) -> Result<(), String> {
        self.state.with_db(|c| store::run_save(c, self.question_id, run))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    uid: Option<String>,
    #[serde(flatten)]
    progress: Progress,
}

// ---------- 界面用的数据 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    which: ApiKey,
    label: &'static str,
    has: bool,
    place: Place,
    /// 现在的设置用得到它
    needed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    role: &'static str,
    model: String,
    input: f64,
    output: f64,
    /// 单价表里有它（没有的按最贵的记）
    known: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustRow {
    kind: ProducerKind,
    label: &'static str,
    hint: &'static str,
    tier: Tier,
    default_tier: Tier,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchStatus {
    mock: bool,
    settings: Settings,
    keys: Vec<KeyStatus>,
    search_engine: String,
    plan_web_searches: u32,
    prices: Vec<PriceRow>,
    tavily_credit_usd: f64,
    claude_search_usd: f64,
    trust: Vec<TrustRow>,
    trust_name: &'static str,
    budget_usd: f64,
    month: String,
    month_spent_usd: f64,
    /// 事实调查最近的调用（账本里的），新的在前
    calls: Vec<Call>,
    ledger_error: Option<String>,
    running: Option<String>,
}

fn status(state: &AppState, agent: &AgentState, research: &ResearchState) -> ResearchStatus {
    let s = settings(state);
    let ledger = Ledger::new(&state.paths.root);
    let month = text::now_iso().get(..7).unwrap_or("").to_string();
    let (calls, ledger_error) = match ledger.all() {
        Ok(all) => (all.into_iter().rev().filter(|c| c.purpose.starts_with("research_")).take(40).collect(), None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let needed = s.keys_needed();
    let keys = [ApiKey::Anthropic, ApiKey::Deepseek, ApiKey::Tavily]
        .into_iter()
        .map(|k| KeyStatus { which: k, label: k.label(), has: has_key(state, agent, research, k), place: secret_of(k, agent, research).place(), needed: needed.contains(&k) })
        .collect();
    let price = |role, e: &Endpoint| {
        let p = s.prices.model(e.model.trim());
        PriceRow { role, model: e.model.clone(), input: p.input, output: p.output, known: s.prices.knows(e.model.trim()) }
    };
    let trust = s.trust.clone().complete();
    let default = TrustTable::default();
    ResearchStatus {
        mock: mock_mode(),
        keys,
        search_engine: s.search_engine(),
        plan_web_searches: s.plan_searches(),
        prices: vec![price("planner", &s.planner), price("worker", &s.worker)],
        tavily_credit_usd: s.prices.tavily_credit(),
        claude_search_usd: s.prices.claude_search(),
        trust: ProducerKind::ALL.into_iter().map(|k| TrustRow { kind: k, label: k.label(), hint: k.hint(), tier: trust.tier(k), default_tier: default.tier(k) }).collect(),
        trust_name: trust.name(),
        budget_usd: format_agent::budget(state),
        month_spent_usd: ledger.month_total(&month).unwrap_or(0.0),
        month,
        calls,
        ledger_error,
        running: research.running(),
        settings: s,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    question_id: i64,
    run: Run,
    report: Report,
    /// 从头再核一遍发现的问题（做完的调查才核）
    findings: Vec<Finding>,
    /// 正在后台做的就是这一次
    running: bool,
    /// 计划还能改（还没开始检索）
    plan_editable: bool,
}

fn view(state: &AppState, research: &ResearchState, uid: &str) -> Result<RunView, String> {
    let (question_id, run, previous) = state.with_db(|c| {
        let run = store::run_load(c, uid)?;
        let previous = match &run.previous {
            Some(p) => store::run_load(c, p).ok(),
            None => None,
        };
        Ok((store::question_of(c, uid)?, run, previous))
    })?;
    let report = report::build(&run, previous.as_ref());
    let findings = if run.status == Status::Done {
        let snapshots = Snapshots::new(&state.paths.root);
        let mut f = audit::audit(&run, &mut |s: &Source| s.sha256.as_deref().ok_or_else(|| "没有快照".to_string()).and_then(|sha| snapshots.text(sha)));
        f.extend(audit::audit_order(&report));
        f
    } else {
        Vec::new()
    };
    let running = research.running().as_deref() == Some(uid);
    let plan_editable = plan_editable(&run) && !running;
    Ok(RunView { question_id, run, report, findings, running, plan_editable })
}

fn plan_editable(run: &Run) -> bool {
    run.stage <= Stage::Search && run.searches.iter().all(|s| s.sub_question.is_empty()) && run.status != Status::Running
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    typical_usd: f64,
    max_usd: f64,
    budget_usd: f64,
    month_spent_usd: f64,
    /// 最坏情况也不超过月度上限
    within_budget: bool,
    /// 分项：（这一项, 次数, 预计, 最多）
    lines: Vec<EstimateLine>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateLine {
    label: String,
    count: usize,
    typical_usd: f64,
    max_usd: f64,
}

fn estimate(state: &AppState, lines: Vec<EstimateLine>) -> Estimate {
    let budget = format_agent::budget(state);
    let spent = Ledger::new(&state.paths.root).month_total(text::now_iso().get(..7).unwrap_or("")).unwrap_or(f64::INFINITY);
    let (typical, max) = (lines.iter().map(|l| l.typical_usd).sum(), lines.iter().map(|l| l.max_usd).sum::<f64>());
    Estimate { typical_usd: typical, max_usd: max, budget_usd: budget, month_spent_usd: spent, within_budget: spent + max <= budget, lines }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreview {
    ticket: u64,
    question: String,
    chars: usize,
    provider: String,
    model: String,
    web_searches: u32,
    estimate: Estimate,
    mock: bool,
    missing_keys: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutePreview {
    ticket: u64,
    uid: String,
    /// 从哪一步接着做
    stage: Stage,
    sub_questions: usize,
    queries: usize,
    search_engine: String,
    max_sources: usize,
    manual_sources: usize,
    worker_model: String,
    planner_model: String,
    estimate: Estimate,
    mock: bool,
    missing_keys: Vec<String>,
    /// 最坏情况会超过月度上限：做到一半可能停下（每次调用前还会检查）
    may_stop: bool,
}

fn search_price(s: &Settings) -> f64 {
    if s.search_engine() == CLAUDE_WEB_SEARCH {
        let req = ClaudeSearch::request("示例搜索词");
        meter::worst_case(&s.prices, s.worker.model.trim(), &req, s.prices.claude_search())
    } else {
        s.tavily_depth.credits() as f64 * s.prices.tavily_credit()
    }
}

fn execute_estimate(run: &Run, s: &Settings) -> (Vec<EstimateLine>, usize, usize) {
    let limits = &run.config.limits;
    let pending: Vec<(&str, &str)> = run
        .plan
        .sub_questions
        .iter()
        .filter(|q| q.enabled)
        .flat_map(|q| q.queries.iter().take(limits.queries_per_sub_question).map(move |x| (q.id.as_str(), x.as_str())))
        .filter(|(sub, query)| !run.searches.iter().any(|r| r.sub_question == *sub && r.query == *query && r.error.is_none()))
        .collect();
    let mut lines = Vec::new();
    if run.stage <= Stage::Search && !pending.is_empty() {
        let each = search_price(s);
        lines.push(EstimateLine { label: format!("检索（{}）", s.search_engine()), count: pending.len(), typical_usd: each * pending.len() as f64, max_usd: each * pending.len() as f64 });
    }
    let unread = run.sources.iter().filter(|x| !x.extracted && x.fetch_error.is_none()).count();
    let max_sources = if run.stage <= Stage::Search { (unread + pending.len() * limits.results_per_query).min(limits.sources) } else { unread };
    let typical_sources = if run.stage <= Stage::Search { (unread + pending.len() * 2).min(limits.sources) } else { unread };
    if run.stage <= Stage::Extract && max_sources > 0 {
        let probe = |chars: usize| prompts::extract_request(&run.question, &run.plan, &Source::default(), &"字".repeat(chars), (1, 1));
        let model = s.worker.model.trim();
        let typical = meter::typical_case(&s.prices, model, &probe(6000), 0.0) * typical_sources as f64;
        let max = meter::worst_case(&s.prices, model, &probe(limits.chunk_chars), 0.0) * (max_sources * limits.chunks_per_source) as f64;
        lines.push(EstimateLine { label: format!("摘证据（{model}）"), count: max_sources, typical_usd: typical, max_usd: max });
    }
    if run.stage <= Stage::Conclude {
        let mut req = prompts::conclude_request(&run.question, &run.plan, &[], &[], &run.config.trust, None);
        req.user.push("字".repeat(40 * 120));
        let model = s.planner.model.trim();
        lines.push(EstimateLine { label: format!("写结论（{model}）"), count: 1, typical_usd: meter::typical_case(&s.prices, model, &req, 0.0), max_usd: meter::worst_case(&s.prices, model, &req, 0.0) * 2.0 });
    }
    (lines, pending.len(), max_sources)
}

// ---------- 命令：设置与密钥 ----------

#[tauri::command(async)]
pub fn research_status(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>) -> ResearchStatus {
    status(&state, &agent, &research)
}

#[tauri::command(async)]
pub fn research_settings_set(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, settings: Settings) -> Result<ResearchStatus, String> {
    settings.check()?;
    let mut s = settings;
    // 等级表只存和默认不同的项
    let default = TrustTable::default();
    s.trust.tiers.retain(|k, t| default.tier(*k) != *t);
    let json = serde_json::to_string(&s).map_err(|e| e.to_string())?;
    state.with_db(|c| crate::metadata::put_setting(c, SETTINGS_KEY, Some(&json)))?;
    Ok(status(&state, &agent, &research))
}

#[tauri::command(async)]
pub fn research_key_set(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, which: ApiKey, key: String) -> Result<ResearchStatus, String> {
    secret_of(which, &agent, &research).save(&key)?;
    state.with_db(|c| crate::metadata::put_setting(c, key_saved_setting(which), Some("1")))?;
    Ok(status(&state, &agent, &research))
}

#[tauri::command(async)]
pub fn research_key_clear(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, which: ApiKey) -> Result<ResearchStatus, String> {
    secret_of(which, &agent, &research).clear()?;
    state.with_db(|c| crate::metadata::put_setting(c, key_saved_setting(which), None))?;
    Ok(status(&state, &agent, &research))
}

// ---------- 命令：调查 ----------

#[tauri::command(async)]
pub fn research_questions_list(state: State<AppState>) -> Result<Vec<store::QuestionSummary>, String> {
    state.with_db(|c| store::questions_list(c))
}

#[tauri::command(async)]
pub fn research_question_get(state: State<AppState>, id: i64) -> Result<store::QuestionSummary, String> {
    state.with_db(|c| store::question_get(c, id))
}

/// 新问题：算出要发什么、大概多少钱。不联网。
#[tauri::command(async)]
pub fn research_plan_prepare(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, question: String) -> Result<PlanPreview, String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("先写下要调查的问题。".into());
    }
    if question.chars().count() > 2000 {
        return Err("问题太长了（超过 2000 字）。拆成几个问题分别调查。".into());
    }
    let s = settings(&state);
    let web_searches = s.plan_searches();
    let req = prompts::plan_request(&question, &Limits::default(), web_searches);
    let model = s.planner.model.trim().to_string();
    let line = EstimateLine {
        label: format!("拆成子问题（{model}{}）", if web_searches > 0 { format!("，最多搜 {web_searches} 次") } else { String::new() }),
        count: 1,
        typical_usd: meter::typical_case(&s.prices, &model, &req, s.prices.claude_search()),
        max_usd: meter::worst_case(&s.prices, &model, &req, s.prices.claude_search()),
    };
    Ok(PlanPreview {
        chars: question.chars().count(),
        provider: s.planner.provider.clone(),
        model,
        web_searches,
        estimate: estimate(&state, vec![line]),
        mock: mock_mode(),
        missing_keys: missing_keys(&state, &agent, &research, &s),
        ticket: research.ticket(Ticket::Plan { question: question.clone() }),
        question,
    })
}

/// 用户确认后：模型把问题拆成子问题（会联网），停在「等用户确认计划」
#[tauri::command(async)]
pub fn research_plan_run(app: AppHandle, state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, ticket: u64) -> Result<RunView, String> {
    let Ticket::Plan { question } = research.take(ticket)? else { return Err("这次确认已经过期，请重新确认。".into()) };
    research.claim("plan")?;
    let _busy = Busy(&research);
    let s = settings(&state);
    let mock = mock_mode();
    let k = kit(&state, &agent, &research, &s, 1)?;
    let uid = new_uid()?;
    let question_id = state.with_db(|c| store::question_create(c, &question, &text::now_iso()))?;
    let mut recorder = DbRecorder { state: &state, question_id };
    let now = text::now_iso;
    let progress = |p: Progress| {
        let _ = app.emit(PROGRESS_EVENT, ProgressEvent { uid: None, progress: p });
    };
    let mut ports = Ports { planner: k.planner.as_ref(), worker: k.worker.as_ref(), search: k.search.as_ref(), fetch: &k.fetch, recorder: &mut recorder, now: &now, progress: &progress, cancel: &research.cancel };
    match flow::plan(&question, &uid, 1, None, s.config(mock), s.plan_searches(), &mut ports) {
        Ok(_) => view(&state, &research, &uid),
        Err(e) => {
            // 没有计划的问题不留着
            let _ = state.with_db(|c| store::question_delete(c, question_id));
            Err(e.message)
        }
    }
}

#[tauri::command(async)]
pub fn research_run_get(state: State<AppState>, research: State<ResearchState>, uid: String) -> Result<RunView, String> {
    view(&state, &research, &uid)
}

fn editable_run(state: &AppState, research: &ResearchState, uid: &str) -> Result<(i64, Run), String> {
    if research.running().as_deref() == Some(uid) {
        return Err("这次调查正在进行，等它停下再改。".into());
    }
    state.with_db(|c| Ok((store::question_of(c, uid)?, store::run_load(c, uid)?)))
}

/// 用户改计划：删改子问题、改搜索词、开关子问题、加子问题
#[tauri::command(async)]
pub fn research_plan_update(state: State<AppState>, research: State<ResearchState>, uid: String, plan: Plan) -> Result<RunView, String> {
    let (question_id, mut run) = editable_run(&state, &research, &uid)?;
    if !plan_editable(&run) {
        return Err("已经开始检索了，计划不能再改。要换计划，点「重查」新开一次。".into());
    }
    let limits = run.config.limits.clone();
    let mut next = plan;
    for s in &mut next.sub_questions {
        s.text = s.text.trim().to_string();
        let mut queries: Vec<String> = s.queries.iter().map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).collect();
        queries.dedup();
        queries.truncate(limits.queries_per_sub_question);
        if queries.is_empty() && !s.text.is_empty() {
            queries.push(s.text.clone());
        }
        s.queries = queries;
    }
    next.sub_questions.retain(|s| !s.text.is_empty());
    if next.sub_questions.len() > limits.sub_questions {
        return Err(format!("子问题最多 {} 个。", limits.sub_questions));
    }
    if !next.sub_questions.iter().any(|s| s.enabled) {
        return Err("至少留一个子问题。".into());
    }
    prompts::renumber(&mut next);
    // 模型给的注意事项和读回时发现的问题不让界面改
    next.problems = run.plan.problems.clone();
    run.plan = next;
    run.updated_at = text::now_iso();
    state.with_db(|c| store::run_save(c, question_id, &run))?;
    view(&state, &research, &uid)
}

/// 手动加一个来源：网址或本机文件（PDF、Excel、CSV、HTML、Word、文本）
#[tauri::command(async)]
pub fn research_source_add(state: State<AppState>, research: State<ResearchState>, uid: String, url: String) -> Result<RunView, String> {
    let (question_id, mut run) = editable_run(&state, &research, &uid)?;
    if run.stage > Stage::Download {
        return Err("这次调查已经读完来源了。要加来源，点「重查」新开一次，在计划页加。".into());
    }
    let url = url.trim().to_string();
    let is_web = url.starts_with("https://") || url.starts_with("http://");
    if !is_web && !Path::new(&url).is_file() {
        return Err("要填 http(s) 开头的网址，或者选一个本机文件。".into());
    }
    flow::add_manual_source(&mut run, &url)?;
    run.updated_at = text::now_iso();
    state.with_db(|c| store::run_save(c, question_id, &run))?;
    view(&state, &research, &uid)
}

#[tauri::command(async)]
pub fn research_source_remove(state: State<AppState>, research: State<ResearchState>, uid: String, key: String) -> Result<RunView, String> {
    let (question_id, mut run) = editable_run(&state, &research, &uid)?;
    let i = run.sources.iter().position(|s| s.id == key).ok_or_else(|| format!("没有来源 {key}"))?;
    if run.sources[i].origin != research_engine::model::Origin::Manual || run.stage > Stage::Search {
        return Err("只有还没开始下载时手动加的来源可以去掉。".into());
    }
    run.sources.remove(i);
    state.with_db(|c| store::run_save(c, question_id, &run))?;
    view(&state, &research, &uid)
}

/// 确认计划之后：估这一轮要花多少（不联网）
#[tauri::command(async)]
pub fn research_execute_prepare(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, uid: String) -> Result<ExecutePreview, String> {
    let (_, run) = editable_run(&state, &research, &uid)?;
    if run.status == Status::Done {
        return Err("这次调查已经做完了。".into());
    }
    let s = settings(&state);
    let (lines, queries, max_sources) = execute_estimate(&run, &s);
    let estimate = estimate(&state, lines);
    let may_stop = !mock_mode() && !estimate.within_budget;
    Ok(ExecutePreview {
        ticket: research.ticket(Ticket::Execute { uid: uid.clone() }),
        uid,
        stage: run.stage,
        sub_questions: run.plan.sub_questions.iter().filter(|q| q.enabled).count(),
        queries,
        search_engine: if mock_mode() { "mock".into() } else { s.search_engine() },
        max_sources,
        manual_sources: run.sources.iter().filter(|x| x.origin == research_engine::model::Origin::Manual).count(),
        worker_model: s.worker.model.clone(),
        planner_model: s.planner.model.clone(),
        estimate,
        mock: mock_mode(),
        missing_keys: missing_keys(&state, &agent, &research, &s),
        may_stop,
    })
}

fn execute_now(app: &AppHandle, uid: &str) -> Result<Status, String> {
    let (state, agent, research) = (app.state::<AppState>(), app.state::<AgentState>(), app.state::<ResearchState>());
    let (question_id, mut run) = state.with_db(|c| Ok((store::question_of(c, uid)?, store::run_load(c, uid)?)))?;
    let s = settings(&state);
    // 接着做时用现在的设置（换了模型、等级表都跟着变）
    let mock = mock_mode();
    let fresh = s.config(mock);
    run.config = Config { limits: run.config.limits.clone(), ..fresh };
    let k = kit(&state, &agent, &research, &s, run.seq.clamp(1, 2))?;
    let mut recorder = DbRecorder { state: &state, question_id };
    let now = text::now_iso;
    let id = uid.to_string();
    let progress = |p: Progress| {
        let _ = app.emit(PROGRESS_EVENT, ProgressEvent { uid: Some(id.clone()), progress: p });
    };
    let mut ports = Ports { planner: k.planner.as_ref(), worker: k.worker.as_ref(), search: k.search.as_ref(), fetch: &k.fetch, recorder: &mut recorder, now: &now, progress: &progress, cancel: &research.cancel };
    flow::execute(&mut run, &mut ports)?;
    Ok(run.status)
}

/// 用户确认后：在后台做完检索、下载、摘证据、核对、结论、报告。马上返回；进度看 `research-progress`，做完看 `research-done`
#[tauri::command(async)]
pub fn research_execute(app: AppHandle, state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, uid: String, ticket: u64) -> Result<(), String> {
    let Ticket::Execute { uid: confirmed } = research.take(ticket)? else { return Err("这次确认已经过期，请重新确认。".into()) };
    if confirmed != uid {
        return Err("这次确认已经过期，请重新确认。".into());
    }
    // 先把密钥这类问题挡在前面（不在后台线程里才发现）
    let missing = missing_keys(&state, &agent, &research, &settings(&state));
    if !missing.is_empty() {
        return Err(format!("还没有填 {} 的 API 密钥。到「设置 › 事实调查」填写后再试。", missing.join("、")));
    }
    research.claim(&uid)?;
    let app = app.clone();
    std::thread::spawn(move || {
        let result = execute_now(&app, &uid);
        let research = app.state::<ResearchState>();
        research.release();
        let (status, error) = match result {
            Ok(s) => (Some(s), None),
            Err(e) => {
                eprintln!("事实调查出错：{e}");
                (None, Some(e))
            }
        };
        let _ = app.emit(DONE_EVENT, serde_json::json!({ "uid": uid, "status": status, "error": error }));
    });
    Ok(())
}

#[tauri::command]
pub fn research_cancel(research: State<ResearchState>) {
    research.cancel.store(true, Ordering::SeqCst);
}

/// 重查：同一个问题再查一次，沿用上一次的计划（可以再改），从检索开始
#[tauri::command(async)]
pub fn research_rerun(state: State<AppState>, research: State<ResearchState>, uid: String) -> Result<RunView, String> {
    let (question_id, previous) = state.with_db(|c| Ok((store::question_of(c, &uid)?, store::run_load(c, &uid)?)))?;
    if previous.status != Status::Done {
        return Err("上一次调查还没有做完。先把它做完（点「继续」），或者删掉它。".into());
    }
    let s = settings(&state);
    let new_uid = new_uid()?;
    let mut run = flow::rerun_from(&previous, &new_uid, s.config(mock_mode()), text::now_iso());
    run.seq = state.with_db(|c| store::next_seq(c, question_id))?;
    state.with_db(|c| store::run_save(c, question_id, &run))?;
    view(&state, &research, &new_uid)
}

fn sweep_snapshots(state: &AppState) {
    if let Ok(keep) = state.with_db(|c| store::snapshot_shas(c)) {
        Snapshots::new(&state.paths.root).sweep(&keep);
    }
}

#[tauri::command(async)]
pub fn research_run_delete(state: State<AppState>, research: State<ResearchState>, uid: String) -> Result<(), String> {
    if research.running().as_deref() == Some(uid.as_str()) {
        return Err("这次调查正在进行，先停止。".into());
    }
    state.with_db(|c| {
        let q = store::question_of(c, &uid)?;
        store::run_delete(c, &uid)?;
        // 问题下面一次调查都没有了：问题也删掉
        if store::question_get(c, q)?.runs.is_empty() {
            store::question_delete(c, q)?;
        }
        Ok(())
    })?;
    sweep_snapshots(&state);
    Ok(())
}

#[tauri::command(async)]
pub fn research_question_delete(state: State<AppState>, research: State<ResearchState>, id: i64) -> Result<(), String> {
    let running = research.running();
    let runs = state.with_db(|c| store::question_get(c, id))?.runs;
    if runs.iter().any(|r| Some(&r.uid) == running.as_ref()) {
        return Err("这个问题的调查正在进行，先停止。".into());
    }
    state.with_db(|c| store::question_delete(c, id))?;
    sweep_snapshots(&state);
    Ok(())
}

// ---------- 命令：导出、加入文献库 ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Markdown,
    Html,
    Docx,
}

/// 文件名：问题的前 40 个字 +（第 N 次调查），不能有的字符换掉；同名的不覆盖
pub fn export_path(dir: &Path, question: &str, seq: u32, ext: &str) -> PathBuf {
    let base: String = question.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '？') || c.is_control() { ' ' } else { c }).take(40).collect();
    let base = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = if base.is_empty() { "事实调查".to_string() } else { base };
    let name = |n: u32| if n == 1 { format!("{base}（第 {seq} 次调查）.{ext}") } else { format!("{base}（第 {seq} 次调查 {n}）.{ext}") };
    let mut n = 1;
    while dir.join(name(n)).exists() {
        n += 1;
    }
    dir.join(name(n))
}

pub fn export_bytes(run: &Run, previous: Option<&Run>, format: Format) -> Result<Vec<u8>, String> {
    let doc = render::document(&report::build(run, previous));
    Ok(match format {
        Format::Markdown => render::markdown(&doc).into_bytes(),
        Format::Html => render::html(&doc).into_bytes(),
        Format::Docx => docx::write(&doc)?,
    })
}

#[tauri::command(async)]
pub fn research_export(state: State<AppState>, uid: String, format: Format, dir: String) -> Result<String, String> {
    let (run, previous) = state.with_db(|c| {
        let run = store::run_load(c, &uid)?;
        let previous = run.previous.as_deref().and_then(|p| store::run_load(c, p).ok());
        Ok((run, previous))
    })?;
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err("选的不是一个文件夹。".into());
    }
    let ext = match format {
        Format::Markdown => "md",
        Format::Html => "html",
        Format::Docx => "docx",
    };
    let path = export_path(&dir, &run.question, run.seq, ext);
    std::fs::write(&path, export_bytes(&run, previous.as_ref(), format)?).map_err(|e| format!("写不了 {}：{e}", path.display()))?;
    Ok(path.display().to_string())
}

/// 在访达中显示导出的报告（只认这三种扩展名、存在的文件）
#[tauri::command(async)]
pub fn research_reveal(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = Path::new(&path);
    let ok = p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["md", "html", "docx"].contains(&e));
    if !ok {
        return Err("找不到这个报告文件。".into());
    }
    app.opener().reveal_item_in_dir(p).map_err(|e| format!("无法打开访达：{e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResult {
    added: Vec<library::Added>,
    view: RunView,
}

/// 把选中的来源加入文献库（原件作为附件）。先做「导入前」备份。
#[tauri::command(async)]
pub fn research_add_to_library(state: State<AppState>, research: State<ResearchState>, uid: String, keys: Vec<String>) -> Result<LibraryResult, String> {
    let run = state.with_db(|c| store::run_load(c, &uid))?;
    let snapshots = Snapshots::new(&state.paths.root);
    let original = |s: &Source| snapshots.original_of(s);
    let added = crate::mutate(&state, |c| {
        crate::import::backup_before(c, &state.paths.backups)?;
        library::add(c, &state.paths.attachments, &run, &keys, &original)
    })?;
    Ok(LibraryResult { added, view: view(&state, &research, &uid)? })
}

pub fn manage(app: &tauri::App) {
    app.manage(ResearchState::default());
}

/// 启动时：上次异常退出时停在「进行中」的调查标成「已停止」，可以接着做
pub fn on_startup(c: &rusqlite::Connection) {
    match store::mark_interrupted(c) {
        Ok(n) if n > 0 => eprintln!("{n} 次调查上次没有正常结束，标成了「已停止」"),
        Ok(_) => {}
        Err(e) => eprintln!("检查调查记录失败：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    #[test]
    fn default_settings_follow_the_plan() {
        let s = Settings::default();
        assert_eq!((s.planner.provider.as_str(), s.planner.model.as_str()), ("anthropic", "claude-opus-5-5"));
        assert_eq!((s.worker.provider.as_str(), s.worker.model.as_str(), s.worker.base_url.as_deref()), ("openai_compatible", "deepseek-flash", Some("https://api.deepseek.com")));
        // DeepSeek 没有专用搜索 → Tavily；Anthropic 的专用搜索是 Claude 网页搜索
        assert_eq!(s.search_engine(), "tavily");
        assert_eq!(s.dedicated("anthropic"), CLAUDE_WEB_SEARCH);
        assert_eq!(s.plan_searches(), 3);
        assert_eq!(s.keys_needed(), [ApiKey::Anthropic, ApiKey::Deepseek, ApiKey::Tavily]);
        assert!(s.check().is_ok());
        // 执行模型也用 Anthropic：检索换成 Claude 网页搜索，不要 Tavily 的密钥
        let mut all_claude = s.clone();
        all_claude.worker = Endpoint { provider: "anthropic".into(), model: "claude-haiku-5-5".into(), base_url: None };
        assert_eq!(all_claude.search_engine(), CLAUDE_WEB_SEARCH);
        assert_eq!(all_claude.keys_needed(), [ApiKey::Anthropic]);
        // 关掉 Anthropic 的专用搜索：计划时不摸底，检索用 Tavily
        all_claude.dedicated_search.insert("anthropic".into(), NO_SEARCH.into());
        assert_eq!((all_claude.search_engine().as_str(), all_claude.plan_searches()), ("tavily", 0));
        // 旧的或不完整的设置也读得回
        let partial: Settings = serde_json::from_str(r#"{"planWebSearches":0}"#).unwrap();
        assert_eq!((partial.plan_web_searches, partial.worker.model.as_str()), (0, "deepseek-flash"));
    }

    #[test]
    fn bad_settings_are_refused() {
        let bad = |f: fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            s.check().unwrap_err()
        };
        assert!(bad(|s| s.planner.provider = "openai".into()).contains("不认识"));
        assert!(bad(|s| s.worker.model = "有 空格".into()).contains("模型名"));
        assert!(bad(|s| s.worker.base_url = Some("http://api.example.invalid".into())).contains("https://"));
        assert!(bad(|s| {
            s.dedicated_search.insert("openai_compatible".into(), CLAUDE_WEB_SEARCH.into());
        })
        .contains("不能用"));
        assert!(bad(|s| s.plan_web_searches = 50).contains("最多 10 次"));
    }

    #[test]
    fn mock_run_config_and_estimates() {
        let s = Settings::default();
        let c = s.config(true);
        assert_eq!((c.planner.provider.as_str(), c.search_engine.as_str()), ("mock", "mock"));
        assert_eq!(c.trust.tiers.len(), ProducerKind::ALL.len());
        let real = s.config(false);
        assert_eq!((real.planner.model.as_str(), real.worker.model.as_str(), real.search_engine.as_str()), ("claude-opus-5-5", "deepseek-flash", "tavily"));
        // 估费用：计划确认后，检索 + 摘证据 + 结论三项
        let plan = prompts::parse_plan(research_engine::fixture::PLAN_REPLY, &Limits::default()).unwrap();
        let run = Run { question: "示例市？".into(), stage: Stage::Search, plan, config: real, ..Default::default() };
        let (lines, queries, max_sources) = execute_estimate(&run, &s);
        assert_eq!(queries, 7);
        assert_eq!(max_sources, 35);
        assert_eq!(lines.len(), 3);
        assert!((lines[0].max_usd - 7.0 * 0.008).abs() < 1e-9);
        assert!(lines.iter().all(|l| l.typical_usd <= l.max_usd && l.typical_usd > 0.0));
        // 做到结论那一步了：只剩结论
        let later = Run { stage: Stage::Conclude, ..run };
        assert_eq!(execute_estimate(&later, &s).0.len(), 1);
    }

    #[test]
    fn exports_never_overwrite() {
        let dir = temp_dir("research-export-names");
        let p = export_path(&dir, "2022–2024 年示例市/样本市的青年就业？", 2, "md");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "2022–2024 年示例市 样本市的青年就业（第 2 次调查）.md");
        std::fs::write(&p, "x").unwrap();
        assert!(export_path(&dir, "2022–2024 年示例市/样本市的青年就业？", 2, "md").to_string_lossy().ends_with("（第 2 次调查 2）.md"));
        assert!(export_path(&dir, "???", 1, "html").to_string_lossy().ends_with("事实调查（第 1 次调查）.html"));
    }
}
