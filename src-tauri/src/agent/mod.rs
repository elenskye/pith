//! 排版 Agent（v0.2 W3，`decisions/0008`、`0015`、`0018`）：模型只出要求清单和文档计划，其余都是确定性的。
//!
//! 1. 提炼要求（`extract`）：规范、模板、附件 → 要求清单（出处、确定度、处理方式）；摘录逐条回原文核对
//! 2. 用户在 App 里逐条确认 → 由清单合成规范（Spec），每个值记得来自哪条要求
//! 3. 文档计划（`planner`）：论文按「结构和片段」或「全文」发给模型 → 计划草稿 → 核对、预览会改到哪些段落
//! 4. 用户确认计划 → 排版器改副本、校验器核对、只检查项由代码统计、出报告（`apply`）
//!
//! 联网只有 1、3 两步，每次发送前界面列出要发的内容和预计费用（`*_prepare`），用户确认后才调用（`*_run`）。
//! 费用记进账本，超过月度上限拒绝调用（`usage`）。
//!
//! 运行方式（照 Pomelo）：只有「应用程序」里用正式数据目录的 Pith 用真模型和钥匙串；
//! 开发版、测试、指定了数据目录的打包版用模拟模型（不联网、不花钱、不碰钥匙串），
//! 要在这些场合用真模型得显式设 `PITH_REAL_MODEL=1`（密钥从 `PITH_API_KEY_FILE` 的文件或界面给）。

pub mod apply;
pub mod claude;
pub mod extract;
pub mod planner;
pub mod secret;
pub mod store;
pub mod usage;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use docx_engine::outline::{self, Hit, Outline};
use docx_engine::package::Package;
use docx_engine::plan::Anchor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::common::new_uid;
use crate::{AppState, paths, text};
use claude::{Model, Progress, Request};
use extract::{Assembled, Requirement, SourceDoc};
use planner::{Draft, Mode};
use store::{Ruleset, Run, Store};
use usage::{Call, Estimate, Ledger};

pub const REAL_MODEL_ENV: &str = "PITH_REAL_MODEL";
const PROGRESS_EVENT: &str = "agent-progress";

/// 正式版：发布构建，并且没有用 PITH_DATA_DIR 另指数据目录
pub fn official() -> bool {
    !paths::is_dev() && std::env::var_os(paths::DATA_DIR_ENV).is_none_or(|v| v.is_empty())
}

fn use_real_model() -> bool {
    official() || std::env::var(REAL_MODEL_ENV).is_ok_and(|v| v == "1")
}

// ---------- 核心流程（不依赖 Tauri，测试直接调） ----------

pub struct Ctx<'a> {
    pub data_dir: &'a Path,
    pub model: &'a dyn Model,
    pub key: &'a str,
    pub budget_usd: f64,
    pub now: String,
    pub progress: &'a dyn Fn(Progress),
    pub cancel: &'a AtomicBool,
}

impl Ctx<'_> {
    fn month(&self) -> &str {
        self.now.get(..7).unwrap_or("")
    }
}

pub fn estimate_for(req: &Request, typical_output: u64, budget_usd: f64, ledger: &Ledger, month: &str) -> Result<Estimate, String> {
    Ok(usage::estimate(req.text_len_estimate(), typical_output, req.max_tokens as u64, budget_usd, ledger.month_total(month)?))
}

/// 一次模型调用：预算检查 → 调用 → 记账 → 看停止原因。返回回答的文字和这次的账目。
pub fn call(ctx: &Ctx, req: &Request, typical_output: u64) -> Result<(String, Call), String> {
    let ledger = Ledger::new(ctx.data_dir);
    let mock = ctx.model.is_mock();
    if !mock {
        let est = estimate_for(req, typical_output, ctx.budget_usd, &ledger, ctx.month())?;
        if !est.within_budget {
            return Err(usage::over_budget_message(&est));
        }
    }
    let record = |model: &str, tokens: usage::Tokens, estimated: bool, note: Option<String>, request_id: Option<String>| -> Result<Call, String> {
        let c = Call { at: ctx.now.clone(), purpose: req.purpose.to_string(), provider: usage::PROVIDER_ANTHROPIC.into(), kind: usage::KIND_MODEL.into(), searches: 0, model: model.to_string(), tokens, cost_usd: if mock { 0.0 } else { usage::cost_usd(model, &tokens) }, estimated, mock, note, request_id };
        ledger.append(&c)?;
        Ok(c)
    };
    match ctx.model.complete(ctx.key, req, ctx.progress, ctx.cancel) {
        Ok(reply) => {
            let c = record(&reply.model, reply.tokens, false, None, reply.request_id.clone())?;
            match reply.stop_reason.as_str() {
                "refusal" => Err(format!("模型拒绝了这次请求（被它的安全分类器拦下，多半是误判），没有结果。这次产生的 ${:.2} 已记进用量。请重试一次。", c.cost_usd)),
                "max_tokens" => Err(format!("回答太长，写到上限被截断了，这次的结果不能用。已经产生的 ${:.2} 记进了用量。可以减少文档后重试。", c.cost_usd)),
                _ => Ok((reply.text, c)),
            }
        }
        Err(f) => {
            if let Some(spent) = f.spent.filter(|_| !mock) {
                record(usage::MODEL, spent, true, Some(if f.cancelled { "中途停止，按估计记".into() } else { "中途出错，按估计记".into() }), None)?;
            }
            Err(f.message)
        }
    }
}

pub fn extract_run(ctx: &Ctx, docs: Vec<SourceDoc>) -> Result<Ruleset, String> {
    let mut req = extract::request(&docs);
    if ctx.model.is_mock() {
        req.mock_reply = Some(extract::mock_reply());
    }
    let (reply, c) = call(ctx, &req, extract::TYPICAL_OUTPUT_TOKENS)?;
    let ex = extract::parse(&reply, &docs).map_err(|e| format!("{e}（这次调用的 ${:.2} 已记进用量）", c.cost_usd))?;
    let ruleset = Ruleset { id: new_uid()?, name: ex.name, created_at: ctx.now.clone(), mock: ctx.model.is_mock(), docs, requirements: ex.requirements, problems: ex.problems, call: Some(c) };
    Store::new(ctx.data_dir).save_ruleset(&ruleset)?;
    Ok(ruleset)
}

#[derive(Debug)]
pub struct Thesis {
    pub name: String,
    pub sha256: String,
    pub outline: Outline,
}

pub fn read_thesis(path: &Path) -> Result<Thesis, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")) {
        return Err(format!("{name} 不是 .docx。旧版 .doc 请先在 Word 里另存为 .docx。"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读不了 {name}：{e}"))?;
    let pkg = Package::read(&bytes).map_err(|e| format!("{name} 不是有效的 .docx：{e}"))?;
    let outline = outline::read(&pkg).map_err(|e| format!("读不了 {name}：{e}"))?;
    let chars: usize = outline.paragraphs.iter().map(|p| p.chars()).sum();
    if chars > planner::MAX_THESIS_CHARS {
        return Err(format!("{name} 有 {chars} 字，超过一次能处理的 {} 字。", planner::MAX_THESIS_CHARS));
    }
    Ok(Thesis { name, sha256: Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(), outline })
}

pub fn plan_run(ctx: &Ctx, ruleset: &Ruleset, path: &Path, mode: Mode, expect_sha: Option<&str>) -> Result<Run, String> {
    let thesis = read_thesis(path)?;
    if expect_sha.is_some_and(|s| s != thesis.sha256) {
        return Err("这份论文在你确认之后又改过了。请重新选择它，再确认一次。".into());
    }
    let assembled = extract::assemble(&ruleset.name, &ruleset.requirements);
    let (mut req, sent_chars) = planner::request(&assembled, &thesis.outline, mode);
    if ctx.model.is_mock() {
        req.mock_reply = Some(planner::mock_reply(&thesis.outline));
    }
    let (reply, c) = call(ctx, &req, planner::TYPICAL_OUTPUT_TOKENS)?;
    let draft = planner::parse(&reply, &thesis.outline, &assembled.spec.structure).map_err(|e| format!("{e}（这次调用的 ${:.2} 已记进用量）", c.cost_usd))?;
    let run = Run {
        id: new_uid()?,
        ruleset_id: ruleset.id.clone(),
        created_at: ctx.now.clone(),
        thesis_path: path.display().to_string(),
        thesis_name: thesis.name,
        thesis_sha256: thesis.sha256,
        mode,
        mock: ctx.model.is_mock(),
        sent_chars,
        draft,
        call: Some(c),
        result: None,
    };
    Store::new(ctx.data_dir).save_run(&run)?;
    Ok(run)
}

pub fn apply_run(data_dir: &Path, ruleset: &Ruleset, run: &mut Run, now: &str) -> Result<(), String> {
    let result = apply::apply(&ruleset.name, &ruleset.requirements, &run.draft, Path::new(&run.thesis_path), now)?;
    run.result = Some(result);
    Store::new(data_dir).save_run(run)
}

// ---------- 界面用的数据 ----------

pub struct AgentState {
    secret: secret::Secret,
    cancel: AtomicBool,
    busy: AtomicBool,
    next_ticket: AtomicU64,
    tickets: Mutex<HashMap<u64, Ticket>>,
}

enum Ticket {
    Extract(Vec<SourceDoc>),
    Plan { ruleset_id: String, path: String, mode: Mode, sha256: String },
}

impl Default for AgentState {
    fn default() -> Self {
        AgentState { secret: secret::Secret::new(official()), cancel: AtomicBool::new(false), busy: AtomicBool::new(false), next_ticket: AtomicU64::new(1), tickets: Mutex::new(HashMap::new()) }
    }
}

impl AgentState {
    fn ticket(&self, t: Ticket) -> u64 {
        let id = self.next_ticket.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut m) = self.tickets.lock() {
            // 只留最近几张：确认框关掉不发的不攒着
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
        self.tickets.lock().ok().and_then(|mut m| m.remove(&id)).ok_or_else(|| "这次确认已经过期，请重新开始。".to_string())
    }
}

const BUDGET_KEY: &str = "agent.budget_usd";
const KEY_SAVED: &str = "agent.key_saved";
const SEND_MODE: &str = "agent.send_mode";

fn setting(state: &AppState, key: &str) -> Option<String> {
    state.with_db(|c| crate::metadata::setting(c, key)).ok().flatten()
}

fn budget(state: &AppState) -> f64 {
    setting(state, BUDGET_KEY).and_then(|v| v.parse::<f64>().ok()).filter(|v| (usage::MIN_BUDGET_USD..=usage::MAX_BUDGET_USD).contains(v)).unwrap_or(usage::DEFAULT_BUDGET_USD)
}

/// 论文发给模型的方式：默认「结构和片段」，设置里可以换成「全文」（用户 2026-11-11 定，对比见 `decisions/0018`）
fn send_mode(state: &AppState) -> Mode {
    match setting(state, SEND_MODE).as_deref() {
        Some("full") => Mode::Full,
        _ => Mode::Brief,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// 模拟模型：不联网、不花钱
    mock: bool,
    has_key: bool,
    key_place: secret::Place,
    model: String,
    price_input: f64,
    price_output: f64,
    budget_usd: f64,
    /// 论文发给模型的方式（生成文档计划时用）
    send_mode: Mode,
    month: String,
    month_spent_usd: f64,
    /// 最近的调用，新的在前
    calls: Vec<Call>,
    ledger_error: Option<String>,
}

fn status(state: &AppState, agent: &AgentState) -> AgentStatus {
    let ledger = Ledger::new(&state.paths.root);
    let now = text::now_iso();
    let month = now.get(..7).unwrap_or("").to_string();
    let (calls, ledger_error) = match ledger.all() {
        Ok(mut c) => {
            c.reverse();
            c.truncate(30);
            (c, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    let p = usage::price(usage::MODEL);
    AgentStatus {
        mock: !use_real_model(),
        has_key: agent.secret.known_present(setting(state, KEY_SAVED).as_deref() == Some("1")),
        key_place: agent.secret.place(),
        model: usage::MODEL.into(),
        price_input: p.input,
        price_output: p.output,
        budget_usd: budget(state),
        send_mode: send_mode(state),
        month_spent_usd: ledger.month_total(&month).unwrap_or(0.0),
        month,
        calls,
        ledger_error,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    label: String,
    name: String,
    chars: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    id: String,
    thesis_name: String,
    thesis_path: String,
    created_at: String,
    mode: Mode,
    mock: bool,
    cost_usd: f64,
    /// none 还没排 / delivered 已写出 / blocked 保真校验没过
    state: &'static str,
}

fn run_summary(r: &Run) -> RunSummary {
    RunSummary {
        id: r.id.clone(),
        thesis_name: r.thesis_name.clone(),
        thesis_path: r.thesis_path.clone(),
        created_at: r.created_at.clone(),
        mode: r.mode,
        mock: r.mock,
        cost_usd: r.call.as_ref().map(|c| c.cost_usd).unwrap_or(0.0),
        state: match &r.result {
            None => "none",
            Some(x) if x.delivered => "delivered",
            Some(_) => "blocked",
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetSummary {
    id: String,
    name: String,
    created_at: String,
    mock: bool,
    docs: Vec<String>,
    requirements: usize,
    runs: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesetView {
    id: String,
    name: String,
    created_at: String,
    mock: bool,
    docs: Vec<DocInfo>,
    requirements: Vec<Requirement>,
    problems: Vec<String>,
    call: Option<Call>,
    /// 由清单合成的规范：每个值和它的来历
    assembled: Assembled,
    runs: Vec<RunSummary>,
}

fn ruleset_view(store: &Store, r: Ruleset) -> RulesetView {
    let assembled = extract::assemble(&r.name, &r.requirements);
    RulesetView {
        runs: store.runs(&r.id).iter().map(run_summary).collect(),
        docs: r.docs.iter().map(|d| DocInfo { label: d.label.clone(), name: d.name.clone(), chars: d.chars }).collect(),
        id: r.id,
        name: r.name,
        created_at: r.created_at,
        mock: r.mock,
        requirements: r.requirements,
        problems: r.problems,
        call: r.call,
        assembled,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionView {
    index: usize,
    paragraphs: usize,
    chars: usize,
    opening: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    #[serde(flatten)]
    run: Run,
    /// 计划会改到正文里的哪些段落（按现在勾选的项算）
    hits: Vec<Hit>,
    sections: Vec<SectionView>,
    /// 文档里的样式名（改计划时选）
    styles: Vec<String>,
    /// 规范里有没有要分区处理的要求（页眉、页码、奇数页起）、要不要三线表：界面据此显示「版式」那几块
    needs_zones: bool,
    three_line: bool,
    /// 可以当分界点的段落（一级标题，含计划里映射成一级标题的）
    anchors: Vec<Anchor>,
    /// 文档里的表格和各自会不会排成三线表
    tables: Vec<TableView>,
    /// 论文文件读不到 / 变了：预览算不出来的原因
    thesis_problem: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    index: usize,
    rows: usize,
    cols: usize,
    paragraph: usize,
    first_row: String,
    /// three_line 会排成三线表 / skipped 计划里说不排（可以改）/ auto 程序自己跳过（不能改）
    state: &'static str,
    why: Option<String>,
}

/// 可以当分界点的段落：一级标题（按计划里的样式映射算），不在表格里、不太长
fn anchor_candidates(o: &Outline, draft: &Draft) -> Vec<Anchor> {
    let role = |style: &str| draft.style_map.iter().find(|m| m.enabled && m.from.eq_ignore_ascii_case(style)).map(|m| m.to.clone()).unwrap_or_else(|| style.to_lowercase());
    let mut out: Vec<Anchor> = o
        .paragraphs
        .iter()
        .filter(|p| !p.in_table && !p.in_textbox && (1..=40).contains(&p.chars()) && (role(&p.style) == "heading 1" || (p.outline_level == Some(1) && role(&p.style) == p.style.to_lowercase())))
        .filter_map(|p| planner::anchor_at(o, p.index).ok())
        .collect();
    // 计划里已经用着的也列上（哪怕不是标题）
    for a in [&draft.layout.front_start, &draft.layout.body_start, &draft.layout.back_start].into_iter().flatten() {
        if !out.iter().any(|x| x.paragraph == a.paragraph) {
            out.push(a.clone());
        }
    }
    out.sort_by_key(|a| a.paragraph);
    out
}

fn run_view(store: &Store, run: Run) -> RunView {
    let (mut hits, mut sections, mut styles, mut anchors, mut tables, mut problem) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), None);
    let structure = store.ruleset(&run.ruleset_id).map(|r| extract::assemble(&r.name, &r.requirements).spec.structure).unwrap_or_default();
    let three_line = structure.three_line_tables == Some(true);
    match read_thesis(Path::new(&run.thesis_path)) {
        Ok(t) => {
            if t.sha256 != run.thesis_sha256 {
                problem = Some("论文文件在生成计划之后改过了：下面的预览按现在的文件算，建议重新生成计划。".to_string());
            }
            match run.draft.to_plan("预览") {
                Ok(plan) => {
                    hits = outline::preview(&t.outline, &plan);
                    let (_, skipped) = docx_engine::structure::three_line_targets(&t.outline, &plan);
                    tables = t
                        .outline
                        .table_list
                        .iter()
                        .map(|x| {
                            let why = skipped.iter().find(|(n, _)| *n == x.index).map(|(_, w)| w.clone());
                            let state = match &why {
                                None => "three_line",
                                Some(_) if plan.layout.skip_tables.contains(&x.index) => "skipped",
                                Some(_) => "auto",
                            };
                            TableView { index: x.index, rows: x.rows, cols: x.cols, paragraph: x.paragraph, first_row: x.first_row.clone(), state, why }
                        })
                        .collect();
                }
                Err(e) => problem = Some(e),
            }
            sections = t.outline.sections.iter().map(|s| SectionView { index: s.index, paragraphs: s.paragraphs, chars: s.chars, opening: s.opening.clone() }).collect();
            styles = t.outline.styles.iter().map(|s| s.name.clone()).collect();
            anchors = anchor_candidates(&t.outline, &run.draft);
        }
        Err(e) => problem = Some(e),
    }
    RunView { run, hits, sections, styles, needs_zones: structure.needs_zones(), three_line, anchors, tables, thesis_problem: problem }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractPreview {
    ticket: u64,
    docs: Vec<DocInfo>,
    estimate: Estimate,
    mock: bool,
    has_key: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreview {
    ticket: u64,
    thesis_name: String,
    mode: Mode,
    paragraphs: usize,
    /// 论文正文总字数、这种方式会发出去的字数
    total_chars: usize,
    sent_chars: usize,
    estimate: Estimate,
    mock: bool,
    has_key: bool,
}

// ---------- 命令 ----------

#[tauri::command(async)]
pub fn agent_status(state: State<AppState>, agent: State<AgentState>) -> AgentStatus {
    status(&state, &agent)
}

#[tauri::command(async)]
pub fn agent_key_set(state: State<AppState>, agent: State<AgentState>, key: String) -> Result<AgentStatus, String> {
    agent.secret.save(&key)?;
    state.with_db(|c| crate::metadata::put_setting(c, KEY_SAVED, Some("1")))?;
    Ok(status(&state, &agent))
}

#[tauri::command(async)]
pub fn agent_key_clear(state: State<AppState>, agent: State<AgentState>) -> Result<AgentStatus, String> {
    agent.secret.clear()?;
    state.with_db(|c| crate::metadata::put_setting(c, KEY_SAVED, None))?;
    Ok(status(&state, &agent))
}

#[tauri::command(async)]
pub fn agent_budget_set(state: State<AppState>, agent: State<AgentState>, usd: f64) -> Result<AgentStatus, String> {
    if !usd.is_finite() || !(usage::MIN_BUDGET_USD..=usage::MAX_BUDGET_USD).contains(&usd) {
        return Err(format!("月度上限要在 ${:.0} 到 ${:.0} 之间。", usage::MIN_BUDGET_USD, usage::MAX_BUDGET_USD));
    }
    let v = format!("{:.2}", usd);
    state.with_db(|c| crate::metadata::put_setting(c, BUDGET_KEY, Some(&v)))?;
    Ok(status(&state, &agent))
}

#[tauri::command(async)]
pub fn agent_send_mode_set(state: State<AppState>, agent: State<AgentState>, mode: Mode) -> Result<AgentStatus, String> {
    let v = if mode == Mode::Full { "full" } else { "brief" };
    state.with_db(|c| crate::metadata::put_setting(c, SEND_MODE, Some(v)))?;
    Ok(status(&state, &agent))
}

#[tauri::command(async)]
pub fn format_rulesets_list(state: State<AppState>) -> Vec<RulesetSummary> {
    let store = Store::new(&state.paths.root);
    store
        .rulesets()
        .into_iter()
        .map(|r| RulesetSummary { runs: store.runs(&r.id).len(), docs: r.docs.iter().map(|d| d.name.clone()).collect(), requirements: r.requirements.len(), id: r.id, name: r.name, created_at: r.created_at, mock: r.mock })
        .collect()
}

#[tauri::command(async)]
pub fn format_ruleset_get(state: State<AppState>, id: String) -> Result<RulesetView, String> {
    let store = Store::new(&state.paths.root);
    Ok(ruleset_view(&store, store.ruleset(&id)?))
}

#[tauri::command(async)]
pub fn format_ruleset_delete(state: State<AppState>, id: String) -> Result<(), String> {
    Store::new(&state.paths.root).delete_ruleset(&id)
}

#[tauri::command(async)]
pub fn format_ruleset_rename(state: State<AppState>, id: String, name: String) -> Result<RulesetView, String> {
    let store = Store::new(&state.paths.root);
    let mut r = store.ruleset(&id)?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("名称不能空着，也不要超过 80 个字。".into());
    }
    r.name = name.to_string();
    store.save_ruleset(&r)?;
    Ok(ruleset_view(&store, r))
}

/// 读入要求文档，算出要发什么、大概多少钱。不联网。
#[tauri::command(async)]
pub fn format_extract_prepare(state: State<AppState>, agent: State<AgentState>, paths: Vec<String>) -> Result<ExtractPreview, String> {
    let docs = extract::load_docs(&paths)?;
    let req = extract::request(&docs);
    let now = text::now_iso();
    let estimate = estimate_for(&req, extract::TYPICAL_OUTPUT_TOKENS, budget(&state), &Ledger::new(&state.paths.root), now.get(..7).unwrap_or(""))?;
    let st = status(&state, &agent);
    Ok(ExtractPreview { docs: docs.iter().map(|d| DocInfo { label: d.label.clone(), name: d.name.clone(), chars: d.chars }).collect(), ticket: agent.ticket(Ticket::Extract(docs)), estimate, mock: st.mock, has_key: st.has_key })
}

struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 一次只跑一个模型调用；取密钥、选模型
fn with_model<T>(app: &AppHandle, state: &AppState, agent: &AgentState, f: impl FnOnce(&Ctx) -> Result<T, String>) -> Result<T, String> {
    if agent.busy.swap(true, Ordering::SeqCst) {
        return Err("上一次调用还没有结束。".into());
    }
    let _busy = Busy(&agent.busy);
    agent.cancel.store(false, Ordering::SeqCst);
    let real = use_real_model();
    let key = if real { agent.secret.load()?.ok_or_else(|| "还没有填 Anthropic API 密钥。到「设置 › 排版 Agent」填写后再试。".to_string())? } else { String::new() };
    let model: Box<dyn Model> = if real { Box::new(claude::Anthropic::default()) } else { Box::new(claude::Mock { fail: None, delay: std::time::Duration::from_millis(1200) }) };
    let progress = |p: Progress| {
        let _ = app.emit(PROGRESS_EVENT, serde_json::json!({ "thinkingChars": p.thinking_chars, "outputChars": p.output_chars }));
    };
    let ctx = Ctx { data_dir: &state.paths.root, model: model.as_ref(), key: &key, budget_usd: budget(state), now: text::now_iso(), progress: &progress, cancel: &agent.cancel };
    f(&ctx)
}

/// 用户确认后：把要求文档发给模型，提炼要求清单
#[tauri::command(async)]
pub fn format_extract_run(app: AppHandle, state: State<AppState>, agent: State<AgentState>, ticket: u64) -> Result<RulesetView, String> {
    let Ticket::Extract(docs) = agent.take(ticket)? else { return Err("这次确认已经过期，请重新开始。".into()) };
    let ruleset = with_model(&app, &state, &agent, |ctx| extract_run(ctx, docs))?;
    Ok(ruleset_view(&Store::new(&state.paths.root), ruleset))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingPatch {
    target: String,
    prop: String,
    value: String,
}

/// 用户确认清单时的修改：开关一条要求、改它的设置值
#[tauri::command(async)]
pub fn format_requirement_set(state: State<AppState>, ruleset_id: String, requirement_id: String, enabled: Option<bool>, settings: Option<Vec<SettingPatch>>) -> Result<RulesetView, String> {
    let store = Store::new(&state.paths.root);
    let mut rs = store.ruleset(&ruleset_id)?;
    let r = rs.requirements.iter_mut().find(|r| r.id == requirement_id).ok_or_else(|| "这条要求不存在了".to_string())?;
    if let Some(e) = enabled {
        r.enabled = e;
    }
    if let Some(list) = settings {
        if r.handling != extract::Handling::Auto {
            return Err("只有「自动」的要求有设置。".into());
        }
        let mut next = Vec::new();
        for s in list {
            extract::parse_setting(&s.target, &s.prop, &s.value).map_err(|e| format!("「{} · {} = {}」：{e}", extract::label_of(extract::TARGETS, &s.target), extract::label_of(extract::PROPS, &s.prop), s.value))?;
            next.push(extract::Setting { target: s.target, prop: s.prop, value: s.value.trim().to_string(), error: None });
        }
        if next.is_empty() {
            return Err("至少留一个设置；不想应用这条要求就把它关掉。".into());
        }
        r.settings = next;
    }
    store.save_ruleset(&rs)?;
    Ok(ruleset_view(&store, rs))
}

/// 读入论文，算出要发什么、大概多少钱。不联网。发送方式用设置里的（`mode` 给了就用给的）。
#[tauri::command(async)]
pub fn format_plan_prepare(state: State<AppState>, agent: State<AgentState>, ruleset_id: String, path: String, mode: Option<Mode>) -> Result<PlanPreview, String> {
    let mode = mode.unwrap_or_else(|| send_mode(&state));
    let ruleset = Store::new(&state.paths.root).ruleset(&ruleset_id)?;
    let thesis = read_thesis(Path::new(&path))?;
    let assembled = extract::assemble(&ruleset.name, &ruleset.requirements);
    if assembled.lines.iter().all(|l| l.requirement.is_none()) {
        return Err("这套规范里没有能自动应用的设置，排不了。".into());
    }
    let (req, sent_chars) = planner::request(&assembled, &thesis.outline, mode);
    let now = text::now_iso();
    let estimate = estimate_for(&req, planner::TYPICAL_OUTPUT_TOKENS, budget(&state), &Ledger::new(&state.paths.root), now.get(..7).unwrap_or(""))?;
    let st = status(&state, &agent);
    Ok(PlanPreview {
        ticket: agent.ticket(Ticket::Plan { ruleset_id, path, mode, sha256: thesis.sha256 }),
        thesis_name: thesis.name,
        mode,
        paragraphs: thesis.outline.paragraphs.len(),
        total_chars: thesis.outline.paragraphs.iter().map(|p| p.chars()).sum(),
        sent_chars,
        estimate,
        mock: st.mock,
        has_key: st.has_key,
    })
}

/// 用户确认后：把论文（按选的方式）发给模型，生成文档计划
#[tauri::command(async)]
pub fn format_plan_run(app: AppHandle, state: State<AppState>, agent: State<AgentState>, ticket: u64) -> Result<RunView, String> {
    let Ticket::Plan { ruleset_id, path, mode, sha256 } = agent.take(ticket)? else { return Err("这次确认已经过期，请重新开始。".into()) };
    let ruleset = Store::new(&state.paths.root).ruleset(&ruleset_id)?;
    let run = with_model(&app, &state, &agent, |ctx| plan_run(ctx, &ruleset, Path::new(&path), mode, Some(&sha256)))?;
    Ok(run_view(&Store::new(&state.paths.root), run))
}

#[tauri::command(async)]
pub fn format_run_get(state: State<AppState>, ruleset_id: String, run_id: String) -> Result<RunView, String> {
    let store = Store::new(&state.paths.root);
    let run = store.run(&ruleset_id, &run_id)?;
    Ok(run_view(&store, run))
}

#[tauri::command(async)]
pub fn format_run_delete(state: State<AppState>, ruleset_id: String, run_id: String) -> Result<(), String> {
    Store::new(&state.paths.root).delete_run(&ruleset_id, &run_id)
}

/// 用户改计划（勾选、增删映射和规则、保护的分节）。改过以后之前的排版结果作废。
#[tauri::command(async)]
pub fn format_plan_update(state: State<AppState>, ruleset_id: String, run_id: String, draft: Draft) -> Result<RunView, String> {
    let store = Store::new(&state.paths.root);
    let mut run = store.run(&ruleset_id, &run_id)?;
    for r in &draft.paragraph_rules {
        planner::check_rule(&r.pattern, &r.style).map_err(|e| format!("文字规则「{}」：{e}", r.pattern))?;
    }
    for m in &draft.style_map {
        if !planner::ROLES.iter().any(|x| x.0 == m.to.to_lowercase()) || m.from.trim().is_empty() {
            return Err(format!("样式映射「{} → {}」不对。", m.from, m.to));
        }
    }
    if draft.protect_sections.iter().any(|p| p.section == 0) {
        return Err("分节从 1 开始数。".into());
    }
    // 分界点要对得上现在的论文（序号处的段落以那段文字开头、不在表格里）
    let thesis = read_thesis(Path::new(&run.thesis_path))?;
    for (a, what) in [(&draft.layout.front_start, "前置部分的起点"), (&draft.layout.body_start, "正文的起点"), (&draft.layout.back_start, "正文之后的起点")] {
        if let Some(a) = a {
            docx_engine::structure::check_anchor(&thesis.outline, a, what).map_err(|e| e.to_string())?;
        }
    }
    if let Some(t) = draft.skip_tables.iter().find(|t| t.table == 0 || t.table > thesis.outline.table_list.len()) {
        return Err(format!("文档里没有第 {} 个表格。", t.table));
    }
    draft.to_plan("检查")?;
    // 摘要、观察、丢掉的项是模型那次给的，不让界面改
    run.draft = Draft { summary: run.draft.summary.clone(), observations: run.draft.observations.clone(), problems: run.draft.problems.clone(), ..draft };
    run.result = None;
    store.save_run(&run)?;
    Ok(run_view(&store, run))
}

/// 按确认的规范和计划排一份副本（确定性的，不联网）
#[tauri::command(async)]
pub fn format_apply(state: State<AppState>, ruleset_id: String, run_id: String) -> Result<RunView, String> {
    let store = Store::new(&state.paths.root);
    let ruleset = store.ruleset(&ruleset_id)?;
    let mut run = store.run(&ruleset_id, &run_id)?;
    apply_run(&state.paths.root, &ruleset, &mut run, &text::now_iso())?;
    Ok(run_view(&store, run))
}

#[tauri::command]
pub fn format_cancel(agent: State<AgentState>) {
    agent.cancel.store(true, Ordering::SeqCst);
}

pub fn manage(app: &tauri::App) {
    app.manage(AgentState::default());
}

#[cfg(test)]
mod tests;
