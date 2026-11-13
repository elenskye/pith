//! 概念研究的 Tauri 命令（`concept_*`）和它们用到的设置、密钥、后台执行（`decisions/0024`）。
//!
//! - 解析成文本：`concept_text_prepare`（EPUB 直接解析出结构预览；PDF 先算页数和费用）→（PDF：`concept_pdf_convert` 走 Mathpix 或本机取文字）
//!   →（可选 `concept_text_renumber` 换段落号写法）→ `concept_text_save`（先做「导入前」备份；译本连上原著并对齐；段落进检索）。
//! - 一次研究：`concept_run_prepare` 算出每步要发什么、最坏多少钱（不联网）→ 用户确认 → `concept_run_execute` 在后台线程里跑，
//!   过程中发 `concept-progress` 事件，做完发 `concept-done`；`concept_cancel` 停下，`concept_run_prepare` 带 `resume` 接着做。
//! - 和排版 Agent、事实调查一样：只有「应用程序」里用正式数据目录的 Pith 用真接口和钥匙串；开发版、测试、指定了数据目录的打包版
//!   用模拟接口（录制的回复，不联网、不花钱），`PITH_REAL_MODEL=1` 可以显式改用真接口。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use concept_engine::align::{self, Alignment};
use concept_engine::audit::{self, Finding};
use concept_engine::flow::{self, Ports, Progress, Recorder, Shelf};
use concept_engine::model::{Card, Run, RunStatus, Scope};
use concept_engine::prompts::Books;
use concept_engine::skill::{self, Broken, Entry, Skill, StepKind};
use concept_engine::text::{self, Numbering, Preview, Role, Text, UnitKind};
use concept_engine::{graph, mathpix as text_mathpix, mock, render};
use research_engine::model::ModelRef;
use research_engine::ports::{Fetch, Model, Search};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::library::{self as shelf, Copying, DbShelf, MockWeb};
use super::mathpix::{self, Mathpix};
use super::responses::{self, OpenAiSearch, Responses};
use super::{keys, search, store};
use crate::agent::secret::{ApiKey, Place};
use crate::agent::usage::{self, Ledger, Prices};
use crate::agent::{self as format_agent, AgentState};
use crate::common::new_uid;
use crate::research::anthropic::{self, Anthropic};
use crate::research::commands::ResearchState;
use crate::research::download::{Downloader, WithFallback};
use crate::research::meter::{self, Logged, Meter};
use crate::research::snapshot::Snapshots;
use crate::research::tavily::{Depth, Tavily};
use crate::{AppState, text as apptext};

const SETTINGS_KEY: &str = "concept.settings";
const PROGRESS_EVENT: &str = "concept-progress";
const DONE_EVENT: &str = "concept-done";
pub const OPUS: &str = "claude-opus-5-5";
pub const OPENAI_SEARCH: &str = "openai_web_search";
pub const CLAUDE_SEARCH: &str = "claude_web_search";
pub const TAVILY: &str = "tavily";

// ---------- 设置 ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// gpt-6.1-sol（默认）或 claude-opus-5-5
    pub model: String,
    /// 推理深浅（OpenAI 的 reasoning.effort）：low / medium / high / xhigh / max
    pub effort: String,
    /// 开着就用模型的专用搜索（OpenAI → Responses 的 web_search；Opus → Claude 网页搜索），关了用 Tavily
    pub dedicated_search: bool,
    /// PDF 超过这么多页提示改用 EPUB
    pub pdf_page_limit: u32,
    pub openai_base: Option<String>,
    pub mathpix_base: Option<String>,
    /// 单价覆盖（和事实调查一样）
    pub prices: Prices,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: responses::DEFAULT_MODEL.into(), effort: "medium".into(), dedicated_search: true, pdf_page_limit: 60, openai_base: None, mathpix_base: None, prices: Prices::default() }
    }
}

impl Settings {
    pub fn check(&self) -> Result<(), String> {
        if self.model != responses::DEFAULT_MODEL && self.model != OPUS {
            return Err(format!("模型只能是 {} 或 {OPUS}。", responses::DEFAULT_MODEL));
        }
        if !responses::EFFORTS.contains(&self.effort.as_str()) {
            return Err("推理深浅只能是 low / medium / high / xhigh / max。".into());
        }
        if !(1..=2000).contains(&self.pdf_page_limit) {
            return Err("PDF 页数上限要在 1 到 2000 之间。".into());
        }
        for b in [&self.openai_base, &self.mathpix_base].into_iter().flatten() {
            let b = b.trim();
            if !b.is_empty() && (!b.starts_with("https://") || b.len() > 200 || b.chars().any(char::is_whitespace)) {
                return Err("接口地址要以 https:// 开头。".into());
            }
        }
        self.prices.check()
    }

    pub fn provider(&self) -> &'static str {
        if self.model.starts_with("claude") { anthropic::PROVIDER } else { responses::PROVIDER }
    }

    pub fn search_engine(&self) -> &'static str {
        match (self.dedicated_search, self.provider()) {
            (true, p) if p == responses::PROVIDER => OPENAI_SEARCH,
            (true, _) => CLAUDE_SEARCH,
            _ => TAVILY,
        }
    }

    /// 要用到哪几把密钥（`with_web`：要不要上网找）
    pub fn keys_needed(&self, with_web: bool) -> Vec<ApiKey> {
        let mut out = vec![if self.provider() == anthropic::PROVIDER { ApiKey::Anthropic } else { ApiKey::Openai }];
        if with_web && self.search_engine() == TAVILY {
            out.push(ApiKey::Tavily);
        }
        out
    }
}

fn settings(state: &AppState) -> Settings {
    format_agent::setting(state, SETTINGS_KEY).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn mock_mode() -> bool {
    !format_agent::use_real_model()
}

// ---------- 状态 ----------

enum Ticket {
    /// 解析好的文本，等用户确认后存
    Text { text: Box<Text>, attachment_id: i64, item_id: Option<i64>, sha256: String, meta: store::Meta },
    /// PDF：算好了页数，等用户确认转换
    Pdf { attachment_id: i64, pages: u32 },
    Run { text_uid: String, skill: String, scope: Box<Scope>, targets: Vec<Card>, previous: Option<String>, resume: Option<String> },
}

pub struct ConceptState {
    cancel: Arc<AtomicBool>,
    busy: Mutex<Option<String>>,
    next_ticket: AtomicU64,
    tickets: Mutex<HashMap<u64, Ticket>>,
}

impl Default for ConceptState {
    fn default() -> Self {
        ConceptState { cancel: Arc::new(AtomicBool::new(false)), busy: Mutex::new(None), next_ticket: AtomicU64::new(1), tickets: Mutex::new(HashMap::new()) }
    }
}

impl ConceptState {
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

    fn put_back(&self, id: u64, t: Ticket) {
        if let Ok(mut m) = self.tickets.lock() {
            m.insert(id, t);
        }
    }

    fn running(&self) -> Option<String> {
        self.busy.lock().ok().and_then(|b| b.clone())
    }

    fn claim(&self, what: &str) -> Result<(), String> {
        let mut b = self.busy.lock().map_err(|_| "内部错误".to_string())?;
        if b.is_some() {
            return Err("上一次概念研究还在进行。等它做完，或者先停止。".into());
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

struct Busy<'a>(&'a ConceptState);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub fn manage(app: &tauri::App) {
    app.manage(ConceptState::default());
}

/// 启动时：进行中的研究标成已停止；段落检索的表对上主库
pub fn on_startup(c: &rusqlite::Connection) {
    match store::mark_interrupted(c) {
        Ok(n) if n > 0 => eprintln!("{n} 次概念研究上次没做完，标成已停止"),
        Ok(_) => {}
        Err(e) => eprintln!("检查概念研究失败：{e}"),
    }
    if let Err(e) = search::ensure(c) {
        eprintln!("概念研究的段落检索不可用：{e}");
    }
}

// ---------- 密钥 ----------
// OpenAI、Mathpix 在 `concept::keys`；Anthropic、Tavily 和排版、调查共用（都经 `research::commands` 找）

fn has_key(state: &AppState, agent: &AgentState, research: &ResearchState, which: ApiKey) -> bool {
    crate::research::commands::key_known(state, agent, research, which)
}

fn load_key(agent: &AgentState, research: &ResearchState, which: ApiKey) -> Result<String, String> {
    crate::research::commands::key_load(which, agent, research)
}

fn missing_keys(state: &AppState, agent: &AgentState, research: &ResearchState, s: &Settings, with_web: bool) -> Vec<String> {
    if mock_mode() {
        return Vec::new();
    }
    s.keys_needed(with_web).into_iter().filter(|k| !has_key(state, agent, research, *k)).map(|k| k.label().to_string()).collect()
}

// ---------- 接口的组装 ----------

struct Kit {
    model: Box<dyn Model>,
    search: Box<dyn Search>,
    fetch: Box<dyn Fetch>,
}

fn kit(state: &AppState, agent: &AgentState, research: &ResearchState, s: &Settings) -> Result<Kit, String> {
    let root = &state.paths.root;
    let concept_snaps = shelf::snapshots(root);
    if mock_mode() {
        let mut m = mock::MockModel::default();
        if !cfg!(test) {
            m.delay = Duration::from_millis(700);
        }
        let now = apptext::now_iso;
        return Ok(Kit { model: Box::new(Logged { inner: m, ledger: Ledger::new(root), now }), search: Box::new(Logged { inner: mock::MockSearch, ledger: Ledger::new(root), now }), fetch: Box::new(MockWeb { to: concept_snaps }) });
    }
    let mut prices = s.prices.clone();
    // 设置里没写 GPT-6.1 Sol 的单价：用内置的
    if prices.models.is_empty() {
        prices = Prices::default();
    }
    let meter = Arc::new(Meter::new(root, format_agent::budget(state), prices));
    let model: Box<dyn Model> = if s.provider() == anthropic::PROVIDER {
        Box::new(Anthropic::new(None, load_key(agent, research, ApiKey::Anthropic)?, &s.model, meter.clone()))
    } else {
        Box::new(Responses::new(s.openai_base.as_deref(), load_key(agent, research, ApiKey::Openai)?, &s.model, &s.effort, meter.clone()))
    };
    // 搜索的密钥在真的要上网时才取（没开「也上网找」不要求 Tavily）
    let search: Box<dyn Search> = match s.search_engine() {
        OPENAI_SEARCH => Box::new(OpenAiSearch { model: Responses::new(s.openai_base.as_deref(), load_key(agent, research, ApiKey::Openai)?, &s.model, "low", meter.clone()) }),
        CLAUDE_SEARCH => Box::new(anthropic::ClaudeSearch { model: Anthropic::new(None, load_key(agent, research, ApiKey::Anthropic)?, &s.model, meter.clone()) }),
        _ => match load_key(agent, research, ApiKey::Tavily) {
            Ok(k) => Box::new(Tavily::new(None, k, Depth::Basic, meter.clone())),
            Err(_) => Box::new(NoSearch),
        },
    };
    let fetch = Copying { inner: WithFallback { inner: Downloader::real(root), claude: None }, from: Snapshots::new(root), to: concept_snaps };
    Ok(Kit { model, search, fetch: Box::new(fetch) })
}

/// 没填 Tavily 的密钥时的搜索：报错（只有用户选了「也上网找」才会走到）
struct NoSearch;

impl Search for NoSearch {
    fn engine(&self) -> String {
        TAVILY.into()
    }
    fn search(&self, _: &str, _: usize, _: &AtomicBool) -> Result<research_engine::ports::SearchReply, research_engine::ports::PortError> {
        Err(research_engine::ports::PortError::new(research_engine::ports::ErrorKind::Fatal, "还没有填 Tavily 的密钥，上不了网。到「设置 › 密钥与费用」填写，或者在「设置 › 概念研究」打开专用搜索。"))
    }
}

// ---------- 技能 ----------

pub fn skills_dir(state: &AppState) -> PathBuf {
    state.paths.root.join("skills")
}

/// 用户的技能：`数据目录/skills/<名字>/SKILL.md`
fn user_skills(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    let mut entries: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let folder = e.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') {
            continue;
        }
        match std::fs::read_to_string(e.path().join("SKILL.md")) {
            Ok(src) => out.push((folder, src)),
            Err(_) => out.push((folder, String::new())),
        }
    }
    out
}

fn catalog(state: &AppState) -> (Vec<Skill>, Vec<Broken>) {
    skill::catalog(&user_skills(&skills_dir(state)))
}

fn find_skill(state: &AppState, name: &str) -> Result<Skill, String> {
    catalog(state).0.into_iter().find(|s| s.name == name).ok_or_else(|| format!("没有「{name}」这个技能（用户技能的文件夹可能被删了或格式不对）。"))
}

// ---------- 界面用的数据 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    which: ApiKey,
    label: &'static str,
    has: bool,
    place: Place,
    needed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptStatus {
    mock: bool,
    settings: Settings,
    keys: Vec<KeyStatus>,
    provider: &'static str,
    search_engine: &'static str,
    skills: Vec<Skill>,
    broken_skills: Vec<Broken>,
    skills_dir: String,
    /// 模型单价：输入、缓存输入、输出（美元 / 百万 token）
    price_input: f64,
    price_cached: f64,
    price_output: f64,
    long_context_tokens: u64,
    web_search_usd: f64,
    mathpix_page_usd: f64,
    budget_usd: f64,
    month_spent_usd: f64,
    /// 概念研究最近的调用（账本里的），新的在前
    calls: Vec<usage::Call>,
    running: Option<String>,
}

fn status(state: &AppState, agent: &AgentState, research: &ResearchState, concept: &ConceptState) -> ConceptStatus {
    let s = settings(state);
    let (skills, broken_skills) = catalog(state);
    let ledger = Ledger::new(&state.paths.root);
    let month = apptext::now_iso().get(..7).unwrap_or("").to_string();
    let calls = ledger.all().map(|all| all.into_iter().rev().filter(|c| c.purpose.starts_with("concept")).take(40).collect()).unwrap_or_default();
    let needed = s.keys_needed(true);
    let keys = [ApiKey::Openai, ApiKey::Mathpix].into_iter().map(|k| KeyStatus { which: k, label: k.label(), has: has_key(state, agent, research, k), place: keys::secret(k).place(), needed: needed.contains(&k) || k == ApiKey::Mathpix }).collect();
    let p = s.prices.model(&s.model);
    ConceptStatus {
        mock: mock_mode(),
        keys,
        provider: s.provider(),
        search_engine: s.search_engine(),
        skills,
        broken_skills,
        skills_dir: skills_dir(state).display().to_string(),
        price_input: p.input,
        price_cached: if s.model == responses::DEFAULT_MODEL { responses::SOL_CACHED_INPUT } else { p.input },
        price_output: p.output,
        long_context_tokens: responses::LONG_CONTEXT,
        web_search_usd: responses::WEB_SEARCH_USD,
        mathpix_page_usd: mathpix::PAGE_USD,
        budget_usd: format_agent::budget(state),
        month_spent_usd: ledger.month_total(&month).unwrap_or(0.0),
        calls,
        running: concept.running(),
        settings: s,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSummary {
    uid: String,
    title: String,
    language: String,
    format: String,
    item_id: Option<i64>,
    item_title: Option<String>,
    attachment_id: Option<i64>,
    numbering: String,
    page_source: String,
    has_pages: bool,
    original_uid: Option<String>,
    translation_uid: Option<String>,
    created_at: String,
    converter: Option<String>,
    warnings: Vec<String>,
}

fn summaries(c: &rusqlite::Connection) -> Result<Vec<TextSummary>, String> {
    let rows = store::texts_list(c)?;
    let mut out = Vec::new();
    for r in &rows {
        let item_title: Option<String> = match r.item_id {
            Some(id) => c.query_row("SELECT title FROM items WHERE id = ?1", [id], |x| x.get(0)).ok().flatten(),
            None => None,
        };
        let translation_uid = rows.iter().rev().find(|x| x.original_uid.as_deref() == Some(r.uid.as_str())).map(|x| x.uid.clone());
        out.push(TextSummary {
            uid: r.uid.clone(),
            title: r.title.clone(),
            language: r.language.clone(),
            format: r.format.clone(),
            item_id: r.item_id,
            item_title,
            attachment_id: r.attachment_id,
            numbering: r.numbering.clone(),
            page_source: r.page_source.clone(),
            has_pages: r.page_source != "none",
            original_uid: r.original_uid.clone(),
            translation_uid,
            created_at: r.created_at.clone(),
            converter: r.meta.converter.clone(),
            warnings: r.meta.warnings.clone(),
        });
    }
    Ok(out)
}

fn summary(c: &rusqlite::Connection, uid: &str) -> Result<TextSummary, String> {
    summaries(c)?.into_iter().find(|t| t.uid == uid).ok_or_else(|| "找不到这份文本".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    ticket: u64,
    preview: Preview,
    /// 可以连上的原著（别的、语言不同的文本）
    candidates: Vec<TextSummary>,
    /// 同一条文献已经解析过的（再存就是第二份）
    existing: Option<String>,
    converter: Option<String>,
    mock: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Prepared {
    Epub(Box<TextPreview>),
    Pdf { ticket: u64, pages: u32, limit: u32, over_limit: bool, has_mathpix: bool, cost_usd: f64, budget_usd: f64, month_spent_usd: f64, mock: bool },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitInfo {
    index: usize,
    key: String,
    label: String,
    kind: UnitKind,
    depth: u32,
    body: usize,
    sidenotes: usize,
    notes: usize,
    guide: usize,
    pages: Option<(String, String)>,
    aligned: Option<bool>,
    other_count: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextView {
    summary: TextSummary,
    units: Vec<UnitInfo>,
    translation: Option<TextSummary>,
    original: Option<TextSummary>,
    aligned_units: usize,
    mismatched_units: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaView {
    seq: usize,
    key: String,
    role: Role,
    number: Option<u32>,
    text: String,
    page: Option<String>,
    breaks: Vec<text::PageBreak>,
    attached: Option<usize>,
    marker: Option<String>,
    /// 对齐的另一边（seq）
    partner: Option<usize>,
}

fn para_view(p: &text::Paragraph, partner: Option<usize>) -> ParaView {
    ParaView { seq: p.seq, key: p.key.clone(), role: p.role, number: p.number, text: p.text.clone(), page: p.page.clone(), breaks: p.breaks.clone(), attached: p.attached, marker: p.marker.clone(), partner }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitView {
    text_uid: String,
    unit: UnitInfo,
    paragraphs: Vec<ParaView>,
    translation_uid: Option<String>,
    /// 对齐的译本里这一章（含旁注、注释）和紧挨在前面的导读
    translation: Vec<ParaView>,
    translation_label: Option<String>,
    aligned: bool,
    language: String,
}

// ---------- 书（原著 + 译本 + 对齐） ----------

struct Loaded {
    original: Text,
    translation: Option<(String, Text)>,
    alignment: Option<Alignment>,
}

impl Loaded {
    fn books(&self) -> Books<'_> {
        Books { original: &self.original, translation: self.translation.as_ref().map(|(_, t)| t), alignment: self.alignment.as_ref() }
    }
}

fn load_books(c: &rusqlite::Connection, uid: &str) -> Result<Loaded, String> {
    let original = store::text_load(c, uid)?;
    let (translation, alignment) = match store::partner(c, uid)? {
        Some((t, true)) => {
            let text = store::text_load(c, &t)?;
            let al = store::alignment_load(c, uid, &t)?;
            (Some((t, text)), al)
        }
        _ => (None, None),
    };
    Ok(Loaded { original, translation, alignment })
}

fn unit_infos(t: &Text, alignment: Option<&Alignment>) -> Vec<UnitInfo> {
    let pv = text::preview(t);
    pv.units
        .into_iter()
        .map(|u| {
            let a = alignment.and_then(|a| a.units.iter().find(|x| x.unit == u.index));
            UnitInfo { index: u.index, key: u.key, label: u.label, kind: u.kind, depth: u.depth, body: u.counts.body, sidenotes: u.counts.sidenote, notes: u.counts.note, guide: u.counts.guide, pages: u.pages, aligned: a.map(|x| x.aligned), other_count: a.map(|x| x.other_count) }
        })
        .collect()
}

// ---------- 命令：设置与密钥 ----------

#[tauri::command(async)]
pub fn concept_status(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>) -> ConceptStatus {
    status(&state, &agent, &research, &concept)
}

#[tauri::command(async)]
pub fn concept_settings_set(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, settings: Settings) -> Result<ConceptStatus, String> {
    settings.check()?;
    let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    state.with_db(|c| crate::metadata::put_setting(c, SETTINGS_KEY, Some(&json)))?;
    Ok(status(&state, &agent, &research, &concept))
}

/// OpenAI 一个框；Mathpix 两个框（`key` 是 app_id，`app_key` 是 app_key）
#[tauri::command(async)]
pub fn concept_key_set(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, which: ApiKey, key: String, app_key: Option<String>) -> Result<ConceptStatus, String> {
    if !matches!(which, ApiKey::Openai | ApiKey::Mathpix) {
        return Err("这里只填 OpenAI 和 Mathpix 的密钥。".into());
    }
    let value = if which == ApiKey::Mathpix { keys::mathpix_pair(&key, app_key.as_deref().unwrap_or("")) } else { key };
    keys::secret(which).save(&value)?;
    state.with_db(|c| crate::metadata::put_setting(c, keys::saved_setting(which), Some("1")))?;
    Ok(status(&state, &agent, &research, &concept))
}

#[tauri::command(async)]
pub fn concept_key_clear(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, which: ApiKey) -> Result<ConceptStatus, String> {
    if !matches!(which, ApiKey::Openai | ApiKey::Mathpix) {
        return Err("这里只管 OpenAI 和 Mathpix 的密钥。".into());
    }
    keys::secret(which).clear()?;
    state.with_db(|c| crate::metadata::put_setting(c, keys::saved_setting(which), None))?;
    Ok(status(&state, &agent, &research, &concept))
}

/// 在访达中打开技能文件夹（没有就建）
#[tauri::command(async)]
pub fn concept_skills_reveal(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = skills_dir(&state);
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    let readme = dir.join("README.txt");
    if !readme.exists() {
        let _ = std::fs::write(&readme, "这里放自己的技能：每个技能一个文件夹，里面一个 SKILL.md（格式见 Pith 内置技能，或 concept-engine 的 README）。\n同名的文件夹覆盖内置技能。\n");
    }
    app.opener().reveal_item_in_dir(&readme).map_err(|e| format!("无法打开访达：{e}"))
}

// ---------- 命令：解析成文本 ----------

fn candidates(c: &rusqlite::Connection, language: &str, item_id: Option<i64>) -> Result<Vec<TextSummary>, String> {
    Ok(summaries(c)?.into_iter().filter(|t| t.language != language && t.original_uid.is_none() && (item_id.is_none() || t.item_id != item_id)).collect())
}

fn text_preview(c: &rusqlite::Connection, concept: &ConceptState, t: Ticket) -> Result<TextPreview, String> {
    let Ticket::Text { text, attachment_id, item_id, meta, .. } = &t else { return Err("内部错误：不是文本".into()) };
    let preview = text::preview(text);
    let existing = store::text_of_attachment(c, *attachment_id)?;
    let cands = candidates(c, &text.language, *item_id)?;
    let converter = meta.converter.clone();
    Ok(TextPreview { ticket: concept.ticket(t), preview, candidates: cands, existing, converter, mock: mock_mode() })
}

/// EPUB → 文本和存的时候附带的信息
pub fn parse_epub(bytes: &[u8]) -> Result<(Text, store::Meta), String> {
    let book = concept_engine::epub::read(bytes)?;
    let t = text::from_book(&book, None);
    if t.paragraphs.iter().all(|p| p.role != Role::Body) {
        return Err("这本 EPUB 里没认出正文段落。".into());
    }
    let meta = store::Meta { creators: t.creators.clone(), publisher: t.publisher.clone(), date: t.date.clone(), identifiers: t.identifiers.clone(), warnings: t.warnings.clone(), converter: None, pages: None };
    Ok((t, meta))
}

/// 存一份解析好的文本：先做「导入前」备份；译本连上原著并对齐；段落进检索。返回 uid
#[allow(clippy::too_many_arguments)]
pub fn save_text(c: &rusqlite::Connection, backups: &Path, text: &Text, meta: &store::Meta, attachment_id: Option<i64>, item_id: Option<i64>, sha256: &str, original_uid: Option<&str>) -> Result<String, String> {
    let uid = new_uid()?;
    let now = apptext::now_iso();
    crate::import::backup_before(c, backups)?;
    let id = store::text_save(c, &store::NewText { uid: &uid, item_id, attachment_id, source_sha256: sha256, text, meta: meta.clone(), original_uid, now: &now })?;
    if let Some(o) = original_uid {
        let original = store::text_load(c, o)?;
        store::alignment_save(c, o, &uid, &align::align(&original, text), &now)?;
    }
    search::add_text(c, id)?;
    Ok(uid)
}

/// 文献库里的一个 EPUB / PDF 附件：EPUB 直接解析出结构预览；PDF 先算页数和费用
#[tauri::command(async)]
pub fn concept_text_prepare(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, attachment_id: i64) -> Result<Prepared, String> {
    let file = state.with_db(|c| shelf::attachment_file(c, &state.paths.attachments, attachment_id))?;
    let bytes = std::fs::read(&file.path).map_err(|e| format!("读不了附件文件：{e}"))?;
    if file.ext == "pdf" {
        let s = settings(&state);
        let pages = mathpix::local_pages(&bytes).map(|p| p.len() as u32).unwrap_or(0);
        if pages == 0 {
            return Err("这份 PDF 读不了页数（可能损坏或加密）。".into());
        }
        let spent = Ledger::new(&state.paths.root).month_total(apptext::now_iso().get(..7).unwrap_or("")).unwrap_or(0.0);
        let ticket = concept.ticket(Ticket::Pdf { attachment_id, pages });
        return Ok(Prepared::Pdf { ticket, pages, limit: s.pdf_page_limit, over_limit: pages > s.pdf_page_limit, has_mathpix: mock_mode() || has_key(&state, &agent, &research, ApiKey::Mathpix), cost_usd: pages as f64 * mathpix::PAGE_USD, budget_usd: format_agent::budget(&state), month_spent_usd: spent, mock: mock_mode() });
    }
    let (t, meta) = parse_epub(&bytes)?;
    let ticket = Ticket::Text { text: Box::new(t), attachment_id, item_id: file.item_id, sha256: file.sha256, meta };
    state.with_db(|c| Ok(Prepared::Epub(Box::new(text_preview(c, &concept, ticket)?))))
}

/// PDF：用户确认后转文字。有 Mathpix 密钥（或模拟模式）走 Mathpix；`local` 或没有密钥时用本机取文字（中文可能乱码）
#[tauri::command(async)]
pub fn concept_pdf_convert(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, ticket: u64, local: bool) -> Result<TextPreview, String> {
    let Ticket::Pdf { attachment_id, pages } = concept.take(ticket)? else { return Err("这次确认已经过期，请重新确认。".into()) };
    let file = state.with_db(|c| shelf::attachment_file(c, &state.paths.attachments, attachment_id))?;
    let bytes = std::fs::read(&file.path).map_err(|e| format!("读不了附件文件：{e}"))?;
    let title = file.name.trim_end_matches(".pdf").to_string();
    let use_mathpix = !local && (mock_mode() || has_key(&state, &agent, &research, ApiKey::Mathpix));
    let (t, converter, mut warnings) = if use_mathpix && mock_mode() {
        // 模拟：用本机取的文字拼成 Mathpix 的行（不联网、不花钱）
        let local_pages = mathpix::local_pages(&bytes)?;
        let lines = super::fixtures::lines_from_pages(&local_pages);
        (text_mathpix::from_lines(&lines, None, &title, None)?, "mathpix（模拟）".to_string(), vec!["模拟模式：没有真的调用 Mathpix，用本机取的文字拼成行。".to_string()])
    } else if use_mathpix {
        concept.claim("pdf")?;
        let _busy = Busy(&concept);
        let s = settings(&state);
        let meter = Arc::new(Meter::new(&state.paths.root, format_agent::budget(&state), s.prices.clone()));
        let client = Mathpix::new(s.mathpix_base.as_deref(), &load_key(&agent, &research, ApiKey::Mathpix)?, meter)?;
        let done = client.convert(&bytes, &file.name, pages, &concept.cancel, &|_| {}).map_err(|e| e.message)?;
        let mut w = vec![format!("Mathpix 转了 {} 页，费用 ${:.3} 已记进用量（{}）。", done.pages, done.cost_usd, done.pdf_id)];
        if !done.deleted {
            w.push("Mathpix 上的副本没删掉，可以到 console.mathpix.com 手动删除。".to_string());
        }
        (text_mathpix::from_lines(&done.lines, done.mmd.as_deref(), &title, None)?, "mathpix".to_string(), w)
    } else {
        let local_pages = mathpix::local_pages(&bytes)?;
        let t = text_mathpix::from_pages(&local_pages, &title, None);
        (t, "local".to_string(), vec!["用的是本机取文字（没有 Mathpix 密钥或选了本机）：段落可能断得不对，中文 PDF 可能是乱码。".to_string()])
    };
    if t.paragraphs.iter().all(|p| p.role != Role::Body) {
        return Err("这份 PDF 里没取出正文。".into());
    }
    warnings.extend(t.warnings.clone());
    let meta = store::Meta { warnings, converter: Some(converter), pages: Some(pages), ..Default::default() };
    let ticket = Ticket::Text { text: Box::new(t), attachment_id, item_id: file.item_id, sha256: file.sha256, meta };
    state.with_db(|c| text_preview(c, &concept, ticket))
}

/// 换段落号的写法（存之前）
#[tauri::command(async)]
pub fn concept_text_renumber(state: State<AppState>, concept: State<ConceptState>, ticket: u64, numbering: Numbering) -> Result<TextPreview, String> {
    let mut t = concept.take(ticket)?;
    if let Ticket::Text { text, .. } = &mut t {
        text.renumber(numbering);
    } else {
        return Err("这次确认已经过期，请重新确认。".into());
    }
    state.with_db(|c| text_preview(c, &concept, t))
}

/// 确认后存：先做「导入前」备份；译本连上原著并对齐；段落进检索
#[tauri::command(async)]
pub fn concept_text_save(state: State<AppState>, concept: State<ConceptState>, ticket: u64, original_uid: Option<String>) -> Result<TextSummary, String> {
    let t = concept.take(ticket)?;
    let Ticket::Text { text, attachment_id, item_id, sha256, meta, .. } = &t else { return Err("这次确认已经过期，请重新确认。".into()) };
    let saved = state.with_db(|c| {
        let uid = save_text(c, &state.paths.backups, text, meta, Some(*attachment_id), *item_id, sha256, original_uid.as_deref())?;
        summary(c, &uid)
    });
    if saved.is_err() {
        concept.put_back(ticket, t);
    }
    saved
}

/// 改一份译本对应的原著（或取消关联），重新对齐
#[tauri::command(async)]
pub fn concept_text_link(state: State<AppState>, uid: String, original_uid: Option<String>) -> Result<TextSummary, String> {
    let now = apptext::now_iso();
    state.with_db(|c| {
        store::text_link(c, &uid, original_uid.as_deref(), &now)?;
        if let Some(o) = &original_uid {
            let (original, translation) = (store::text_load(c, o)?, store::text_load(c, &uid)?);
            store::alignment_save(c, o, &uid, &align::align(&original, &translation), &now)?;
        }
        summary(c, &uid)
    })
}

#[tauri::command(async)]
pub fn concept_text_delete(state: State<AppState>, concept: State<ConceptState>, uid: String) -> Result<(), String> {
    if concept.running().is_some() {
        return Err("有一次概念研究正在进行，先停止。".into());
    }
    state.with_db(|c| {
        let id = store::text_id(c, &uid)?;
        search::remove_text(c, id)?;
        store::text_delete(c, &uid)
    })?;
    sweep(&state);
    Ok(())
}

#[tauri::command(async)]
pub fn concept_texts_list(state: State<AppState>) -> Result<Vec<TextSummary>, String> {
    state.with_db(|c| summaries(c))
}

/// 文献库的附件上「解析成文本」按钮：这个附件解析过没有
#[tauri::command(async)]
pub fn concept_attachment_text(state: State<AppState>, attachment_id: i64) -> Result<Option<String>, String> {
    state.with_db(|c| store::text_of_attachment(c, attachment_id))
}

#[tauri::command(async)]
pub fn concept_text_get(state: State<AppState>, uid: String) -> Result<TextView, String> {
    state.with_db(|c| {
        let loaded = load_books(c, &uid)?;
        let s = summary(c, &uid)?;
        let original = match &s.original_uid {
            Some(o) => summary(c, o).ok(),
            None => None,
        };
        let translation = loaded.translation.as_ref().and_then(|(t, _)| summary(c, t).ok());
        let al = loaded.alignment.as_ref();
        Ok(TextView {
            units: unit_infos(&loaded.original, al),
            aligned_units: al.map(|a| a.units.iter().filter(|u| u.aligned).count()).unwrap_or(0),
            mismatched_units: al.map(|a| a.mismatched().iter().map(|u| u.key.clone()).collect()).unwrap_or_default(),
            summary: s,
            translation,
            original,
        })
    })
}

/// 一章的段落（原著）和对齐的译本那一章（含旁注、注释）、紧挨在前面的导读
#[tauri::command(async)]
pub fn concept_unit_get(state: State<AppState>, uid: String, unit: usize) -> Result<UnitView, String> {
    state.with_db(|c| {
        let l = load_books(c, &uid)?;
        let t = &l.original;
        let info = unit_infos(t, l.alignment.as_ref()).into_iter().find(|u| u.index == unit).ok_or("找不到这一章")?;
        let paragraphs: Vec<ParaView> = t.paragraphs.iter().filter(|p| p.unit == unit).map(|p| para_view(p, l.alignment.as_ref().and_then(|a| a.other_of(p.seq)))).collect();
        let mut translation = Vec::new();
        let mut translation_label = None;
        let mut aligned = false;
        if let (Some((_, zh)), Some(al)) = (&l.translation, &l.alignment)
            && let Some(ua) = al.units.iter().find(|u| u.unit == unit)
            && let Some(zu) = ua.other_unit
        {
            aligned = ua.aligned;
            translation_label = Some(ua.other_label.clone());
            // 紧挨在前面的导读
            let mut g = zu;
            let mut guides = Vec::new();
            while g > 0 && zh.units[g - 1].kind == UnitKind::Guide {
                g -= 1;
                guides.push(g);
            }
            guides.reverse();
            for gu in guides {
                translation.extend(zh.paragraphs.iter().filter(|p| p.unit == gu).map(|p| para_view(p, None)));
            }
            translation.extend(zh.paragraphs.iter().filter(|p| p.unit == zu).map(|p| para_view(p, al.original_of(p.seq))));
        }
        Ok(UnitView { text_uid: uid.clone(), unit: info, paragraphs, translation_uid: l.translation.as_ref().map(|(u, _)| u.clone()), translation, translation_label, aligned, language: t.language.clone() })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub seq: usize,
    pub key: String,
    role: Role,
    unit: usize,
    pub unit_key: String,
    unit_label: String,
    text: String,
    /// 在对齐的中译里搜到、映射回原著的
    pub via_translation: bool,
    translation_key: Option<String>,
    translation_text: Option<String>,
}

/// 在一本书里检索段落（问一个问题、追踪一个概念）。原著里搜不到的词（中文搜英文原著）先搜对齐的中译、再映射回原著。
/// `terms` 用空格或「；」分开，各搜一遍合起来
#[tauri::command(async)]
pub fn concept_search(state: State<AppState>, uid: String, query: String, limit: Option<usize>) -> Result<Vec<SearchHit>, String> {
    state.with_db(|c| search_hits(c, &uid, &query, limit.unwrap_or(40)))
}

pub fn search_hits(c: &rusqlite::Connection, uid: &str, query: &str, limit: usize) -> Result<Vec<SearchHit>, String> {
    let limit = limit.clamp(1, 200);
    let uid = uid.to_string();
    let query = query.to_string();
    {
        search::ensure(c)?;
        let l = load_books(c, &uid)?;
        let id = store::text_id(c, &uid)?;
        let mut hits: Vec<(usize, f64, bool)> = Vec::new();
        // 多取一些候选：中译的命中有不少在导读、注释或没对齐的章里，映射不回原著；先按相关度取前 limit 条，再按书里的顺序排
        let pool = (limit * 8).min(800);
        let terms: Vec<&str> = query.split(['；', ';', '\n', '，', ',']).map(str::trim).filter(|t| !t.is_empty()).collect();
        let terms = if terms.is_empty() { vec![query.trim()] } else { terms };
        for term in terms.iter().filter(|t| !t.is_empty()) {
            let direct = search::query(c, id, term, pool)?;
            let mapped: Vec<(usize, f64)> = match (&l.translation, &l.alignment) {
                (Some((tu, zh)), Some(al)) if direct.is_empty() || term.chars().any(text::is_cjk) => {
                    let zid = store::text_id(c, tu)?;
                    search::query(c, zid, term, pool)?
                        .into_iter()
                        .filter_map(|(zseq, score)| {
                            // 正文映射回原著；旁注、注释映射到它挂的正文段
                            let z = zh.paragraphs.get(zseq)?;
                            let body = if z.role == Role::Body { Some(zseq) } else { z.attached };
                            al.original_of(body?).map(|o| (o, score))
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            for (seq, score) in direct {
                if !hits.iter().any(|h| h.0 == seq) {
                    hits.push((seq, score, false));
                }
            }
            for (seq, score) in mapped {
                if !hits.iter().any(|h| h.0 == seq) {
                    hits.push((seq, score, true));
                }
            }
        }
        // bm25 越小越相关：先按相关度留前 limit 条，再按书里的顺序（追踪概念要按章看）
        hits.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        hits.truncate(limit);
        hits.sort_by_key(|h| h.0);
        let t = &l.original;
        Ok(hits
            .into_iter()
            .filter_map(|(seq, _, via)| {
                let p = t.paragraphs.get(seq)?;
                let u = &t.units[p.unit];
                let zh = l.alignment.as_ref().and_then(|a| a.other_of(seq)).and_then(|z| l.translation.as_ref().and_then(|(_, tt)| tt.paragraphs.get(z)));
                Some(SearchHit { seq, key: p.key.clone(), role: p.role, unit: p.unit, unit_key: u.key.clone(), unit_label: u.label.clone(), text: p.text.clone(), via_translation: via, translation_key: zh.map(|z| z.key.clone()), translation_text: zh.map(|z| z.text.clone()) })
            })
            .collect())
    }
}

// ---------- 命令：一次研究 ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    text_uid: String,
    skill: String,
    entry: Entry,
    #[serde(default)]
    paragraphs: Vec<String>,
    unit: Option<String>,
    question: Option<String>,
    #[serde(default)]
    terms: Vec<String>,
    /// （研究 uid, 卡片编号）
    #[serde(default)]
    cards: Vec<(String, String)>,
    instruction: Option<String>,
    #[serde(default)]
    skip: Vec<String>,
    #[serde(default)]
    with: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepLine {
    id: String,
    title: String,
    kind: StepKind,
    /// 这一步发什么（「11 段原文（英文，6,203 字）+ 对齐的中译」）
    sends: String,
    max_output_tokens: u32,
    input_tokens: u64,
    typical_usd: f64,
    max_usd: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPreview {
    ticket: u64,
    skill: String,
    skill_title: String,
    resume: Option<String>,
    paragraphs: usize,
    chars: usize,
    languages: Vec<String>,
    first: Option<String>,
    last: Option<String>,
    targets: Vec<String>,
    steps: Vec<StepLine>,
    provider: &'static str,
    model: String,
    effort: String,
    search_engine: &'static str,
    typical_usd: f64,
    max_usd: f64,
    budget_usd: f64,
    month_spent_usd: f64,
    within_budget: bool,
    missing_keys: Vec<String>,
    mock: bool,
}

fn targets_of(c: &rusqlite::Connection, cards: &[(String, String)]) -> Result<Vec<Card>, String> {
    let mut out = Vec::new();
    for (run_uid, key) in cards {
        let r = store::run_load(c, run_uid)?;
        let card = r.cards.into_iter().find(|x| &x.id == key).ok_or_else(|| format!("找不到卡片 {key}"))?;
        out.push(card);
    }
    Ok(out)
}

fn worst_model(s: &Settings, prices: &Prices, input: u64, max_output: u64) -> (f64, f64) {
    if s.provider() == responses::PROVIDER {
        (responses::typical_case(prices, &s.model, input, max_output / 3), responses::worst_case(prices, &s.model, input, max_output))
    } else {
        let p = prices.model(&s.model);
        ((input as f64 * p.input + (max_output / 3) as f64 * p.output) / 1e6, (input as f64 * 1.1 * p.input + max_output as f64 * p.output) / 1e6)
    }
}

fn preview_run(state: &AppState, agent: &AgentState, research: &ResearchState, concept: &ConceptState, skill: &Skill, run: &Run, ticket: Ticket) -> Result<RunPreview, String> {
    let s = settings(state);
    let prices = if s.prices.models.is_empty() { Prices::default() } else { s.prices.clone() };
    let scope = run.scope.clone().ok_or("内部错误：没有范围")?;
    let (lines, paragraphs, chars, languages, first, last) = state.with_db(|c| {
        let l = load_books(c, &scope.text)?;
        let books = l.books();
        let snaps = shelf::snapshots(&state.paths.root);
        let db = DbShelf { state, snapshots: snaps, exclude: Vec::new() };
        let _ = &db;
        let paras = concept_engine::prompts::scope_paragraphs(run, &books);
        let chars: usize = paras.iter().map(|p| p.text.chars().count()).sum();
        let mut languages = vec![if l.original.language == "zh" { "中文".to_string() } else { "英文".to_string() }];
        if l.translation.is_some() {
            languages.push("对齐的中译".into());
        }
        let mut lines = Vec::new();
        for p in flow::planned(skill, run, &books, &[]) {
            let step = skill.steps.iter().find(|x| x.id == p.step).ok_or("内部错误：步骤")?;
            match (&p.request, p.kind) {
                (Some(req), _) => {
                    let input = meter::input_estimate(req) + p.extra_input_tokens;
                    let (typ, max) = worst_model(&s, &prices, input, req.max_tokens as u64);
                    let mut parts = Vec::new();
                    for i in &step.input {
                        parts.push(match i {
                            skill::Input::Scope => format!("{} 段原文（{} 字）", paras.len(), chars),
                            skill::Input::Translation => "对齐的中译和译本的旁注、注释".to_string(),
                            skill::Input::Cards => "前面做出的卡片".to_string(),
                            skill::Input::Sources => format!("找到的来源（每条最多 {} 字）", concept_engine::prompts::SOURCE_CHARS),
                            skill::Input::Question => "问题".to_string(),
                            skill::Input::Instruction => "你写的要求".to_string(),
                            skill::Input::Terms => "检索词".to_string(),
                        });
                    }
                    lines.push(StepLine { id: p.step.clone(), title: p.title.clone(), kind: p.kind, sends: parts.join(" + "), max_output_tokens: req.max_tokens, input_tokens: input, typical_usd: typ, max_usd: max });
                }
                (None, StepKind::Library) => lines.push(StepLine { id: p.step.clone(), title: p.title.clone(), kind: p.kind, sends: format!("在文献库里搜检索词，最多 {} 条（不联网、不花钱）", p.max_results), max_output_tokens: 0, input_tokens: 0, typical_usd: 0.0, max_usd: 0.0 }),
                (None, _) => {
                    let per = match s.search_engine() {
                        OPENAI_SEARCH => responses::search_worst(&prices, &s.model),
                        CLAUDE_SEARCH => prices.claude_search(),
                        _ => prices.tavily_credit(),
                    };
                    lines.push(StepLine { id: p.step.clone(), title: p.title.clone(), kind: p.kind, sends: format!("网页检索 1 次（{}），下载最多 {} 个网页（本机下载不花钱）", s.search_engine(), p.max_results), max_output_tokens: 0, input_tokens: 0, typical_usd: per, max_usd: per });
                }
            }
        }
        Ok((lines, paras.len(), chars, languages, paras.first().map(|p| p.key.clone()), paras.last().map(|p| p.key.clone())))
    })?;
    let (typical, max) = lines.iter().fold((0.0, 0.0), |(a, b), l| (a + l.typical_usd, b + l.max_usd));
    let spent = Ledger::new(&state.paths.root).month_total(apptext::now_iso().get(..7).unwrap_or("")).unwrap_or(0.0);
    let budget = format_agent::budget(state);
    let with_web = flow::active_steps(skill, run).iter().any(|i| skill.steps[*i].kind == StepKind::Web);
    let resume = match &ticket {
        Ticket::Run { resume, .. } => resume.clone(),
        _ => None,
    };
    Ok(RunPreview {
        ticket: concept.ticket(ticket),
        skill: skill.name.clone(),
        skill_title: skill.title.clone(),
        resume,
        paragraphs,
        chars,
        languages,
        first,
        last,
        targets: run.targets.iter().map(|c| format!("{} {}", c.id, c.title)).collect(),
        steps: lines,
        provider: s.provider(),
        model: if mock_mode() { "模拟模型".into() } else { s.model.clone() },
        effort: s.effort.clone(),
        search_engine: s.search_engine(),
        typical_usd: if mock_mode() { 0.0 } else { typical },
        max_usd: if mock_mode() { 0.0 } else { max },
        budget_usd: budget,
        month_spent_usd: spent,
        within_budget: mock_mode() || spent + max <= budget,
        missing_keys: missing_keys(state, agent, research, &s, with_web),
        mock: mock_mode(),
    })
}

/// 新的一次研究（`resume` 为空）或接着做一次没做完的：算出每步发什么、最坏多少钱（不联网）
#[tauri::command(async)]
pub fn concept_run_prepare(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, request: Option<RunRequest>, resume: Option<String>) -> Result<RunPreview, String> {
    run_prepare(&state, &agent, &research, &concept, request, resume)
}

fn run_prepare(state: &AppState, agent: &AgentState, research: &ResearchState, concept: &ConceptState, request: Option<RunRequest>, resume: Option<String>) -> Result<RunPreview, String> {
    if let Some(uid) = resume {
        let (text_uid, run) = state.with_db(|c| Ok((store::run_text(c, &uid)?, store::run_load(c, &uid)?)))?;
        if run.status == RunStatus::Done {
            return Err("这次研究已经做完了。".into());
        }
        let sk = find_skill(state, &run.skill)?;
        let scope = run.scope.clone().ok_or("这次研究没有范围")?;
        let ticket = Ticket::Run { text_uid, skill: sk.name.clone(), scope: Box::new(scope), targets: run.targets.clone(), previous: run.previous.clone(), resume: Some(uid) };
        return preview_run(state, agent, research, concept, &sk, &run, ticket);
    }
    let r = request.ok_or("没有给要研究的范围")?;
    let sk = find_skill(state, &r.skill)?;
    let (translation, targets) = state.with_db(|c| Ok((store::partner(c, &r.text_uid)?.filter(|(_, is_translation)| *is_translation).map(|(u, _)| u), targets_of(c, &r.cards)?)))?;
    let scope = Scope { entry: r.entry, text: r.text_uid.clone(), translation, paragraphs: r.paragraphs, unit: r.unit, question: r.question.filter(|q| !q.trim().is_empty()), terms: r.terms.into_iter().filter(|t| !t.trim().is_empty()).collect(), cards: r.cards, instruction: r.instruction.filter(|q| !q.trim().is_empty()), skip: r.skip, with: r.with };
    let s = settings(state);
    let run = flow::new_run("preview", &sk, scope.clone(), targets.clone(), ModelRef { provider: s.provider().into(), model: s.model.clone() }, &s.effort, mock_mode(), apptext::now_iso())?;
    let ticket = Ticket::Run { text_uid: r.text_uid, skill: sk.name.clone(), scope: Box::new(scope), targets, previous: None, resume: None };
    preview_run(state, agent, research, concept, &sk, &run, ticket)
}

/// 用同一技能对同一范围重跑（旧结果保留）
#[tauri::command(async)]
pub fn concept_rerun_prepare(state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, uid: String) -> Result<RunPreview, String> {
    let (text_uid, old) = state.with_db(|c| Ok((store::run_text(c, &uid)?, store::run_load(c, &uid)?)))?;
    let sk = find_skill(&state, &old.skill)?;
    let scope = old.scope.clone().ok_or("这次研究没有范围")?;
    let s = settings(&state);
    let run = flow::new_run("preview", &sk, scope.clone(), old.targets.clone(), ModelRef { provider: s.provider().into(), model: s.model.clone() }, &s.effort, mock_mode(), apptext::now_iso())?;
    let ticket = Ticket::Run { text_uid, skill: sk.name.clone(), scope: Box::new(scope), targets: old.targets, previous: Some(uid), resume: None };
    preview_run(&state, &agent, &research, &concept, &sk, &run, ticket)
}

struct DbRecorder<'a> {
    state: &'a AppState,
    text_uid: String,
}

impl Recorder for DbRecorder<'_> {
    fn save(&mut self, run: &Run) -> Result<(), String> {
        self.state.with_db(|c| store::run_save(c, &self.text_uid, run))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    uid: String,
    #[serde(flatten)]
    progress: Progress,
}

/// 跑一次（后台线程里调；测试直接调）
pub fn execute_now(state: &AppState, agent: &AgentState, research: &ResearchState, concept: &ConceptState, uid: &str, progress: &dyn Fn(Progress)) -> Result<RunStatus, String> {
    let (text_uid, mut run) = state.with_db(|c| Ok((store::run_text(c, uid)?, store::run_load(c, uid)?)))?;
    let sk = find_skill(state, &run.skill)?;
    let s = settings(state);
    let k = kit(state, agent, research, &s)?;
    let l = state.with_db(|c| load_books(c, &text_uid))?;
    let exclude: Vec<i64> = state.with_db(|c| Ok(store::texts_list(c)?.into_iter().filter_map(|t| t.item_id).collect()))?;
    let db = DbShelf { state, snapshots: shelf::snapshots(&state.paths.root), exclude };
    let mut recorder = DbRecorder { state, text_uid };
    let now = apptext::now_iso;
    let mut ports = Ports { model: k.model.as_ref(), search: k.search.as_ref(), fetch: k.fetch.as_ref(), shelf: &db as &dyn Shelf, recorder: &mut recorder, now: &now, progress, cancel: &concept.cancel };
    flow::execute(&mut run, &sk, &l.books(), &mut ports)?;
    Ok(run.status)
}

/// 用户确认后：建这次研究（或接着做），在后台跑。马上返回 uid；进度看 `concept-progress`，做完看 `concept-done`
#[tauri::command(async)]
pub fn concept_run_execute(app: AppHandle, state: State<AppState>, agent: State<AgentState>, research: State<ResearchState>, concept: State<ConceptState>, ticket: u64) -> Result<String, String> {
    let uid = run_create(&state, &agent, &research, &concept, ticket)?;
    concept.claim(&uid)?;
    let app = app.clone();
    let id = uid.clone();
    std::thread::spawn(move || {
        let (state, agent, research, concept) = (app.state::<AppState>(), app.state::<AgentState>(), app.state::<ResearchState>(), app.state::<ConceptState>());
        let progress = |p: Progress| {
            let _ = app.emit(PROGRESS_EVENT, ProgressEvent { uid: id.clone(), progress: p });
        };
        let result = execute_now(&state, &agent, &research, &concept, &id, &progress);
        concept.release();
        let (status, error) = match result {
            Ok(s) => (Some(s), None),
            Err(e) => {
                eprintln!("概念研究出错：{e}");
                (None, Some(e))
            }
        };
        let _ = app.emit(DONE_EVENT, serde_json::json!({ "uid": id, "status": status, "error": error }));
    });
    Ok(uid)
}

/// 确认后建这次研究（或接着做的那次），返回 uid；查密钥
fn run_create(state: &AppState, agent: &AgentState, research: &ResearchState, concept: &ConceptState, ticket: u64) -> Result<String, String> {
    let Ticket::Run { text_uid, skill, scope, targets, previous, resume } = concept.take(ticket)? else { return Err("这次确认已经过期，请重新确认。".into()) };
    let sk = find_skill(state, &skill)?;
    let s = settings(state);
    let probe = flow::new_run("probe", &sk, (*scope).clone(), targets.clone(), ModelRef::default(), &s.effort, mock_mode(), String::new())?;
    let with_web = flow::active_steps(&sk, &probe).iter().any(|i| sk.steps[*i].kind == StepKind::Web);
    let missing = missing_keys(state, agent, research, &s, with_web);
    if !missing.is_empty() {
        return Err(format!("还没有填 {} 的密钥。到「设置 › 密钥与费用」填写后再试。", missing.join("、")));
    }
    let uid = match resume {
        Some(u) => u,
        None => {
            let uid = new_uid()?;
            let mut run = flow::new_run(&uid, &sk, *scope, targets, ModelRef { provider: if mock_mode() { "mock".into() } else { s.provider().into() }, model: if mock_mode() { "模拟模型".into() } else { s.model.clone() } }, &s.effort, mock_mode(), apptext::now_iso())?;
            run.previous = previous;
            state.with_db(|c| store::run_save(c, &text_uid, &run))?;
            uid
        }
    };
    Ok(uid)
}

#[tauri::command]
pub fn concept_cancel(concept: State<ConceptState>) {
    concept.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command(async)]
pub fn concept_runs_list(state: State<AppState>, text_uid: String) -> Result<Vec<store::RunSummary>, String> {
    state.with_db(|c| store::runs_list(c, &text_uid))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    text_uid: String,
    translation_uid: Option<String>,
    run: Run,
    /// 从存下的文本重新核对发现的问题
    findings: Vec<Finding>,
    /// 关系图（和 HTML 导出同一份）
    svg: String,
    running: bool,
    cost_usd: f64,
    /// 卡片引到的段落（点锚点时显示），key → 文字和页码
    paragraphs: Vec<AnchoredParagraph>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchoredParagraph {
    key: String,
    text: String,
    role: Role,
    pages: Option<String>,
    unit: usize,
    seq: usize,
    translation: bool,
}

fn source_lookup(state: &AppState, run: &Run) -> Vec<(String, String)> {
    let snaps = shelf::snapshots(&state.paths.root);
    run.sources.iter().filter_map(|s| s.sha256.as_deref().and_then(|sha| snaps.text(sha).ok()).map(|t| (s.id.clone(), t))).collect()
}

fn view(state: &AppState, concept: &ConceptState, uid: &str) -> Result<RunView, String> {
    let (text_uid, run) = state.with_db(|c| Ok((store::run_text(c, uid)?, store::run_load(c, uid)?)))?;
    let l = state.with_db(|c| load_books(c, &text_uid))?;
    let books = l.books();
    let texts = source_lookup(state, &run);
    let lookup = |id: &str| texts.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone());
    let findings = if run.status == RunStatus::Done { audit::audit(&run, &books, &lookup) } else { Vec::new() };
    let mut keys: Vec<String> = Vec::new();
    for s in run.cards.iter().flat_map(|c| &c.sentences).chain(run.relations.iter().map(|r| &r.note)) {
        if let Some(p) = &s.paragraph
            && !keys.contains(p)
        {
            keys.push(p.clone());
        }
        if let Some(z) = &s.check.zh_paragraph {
            let k = format!("{}{z}", concept_engine::prompts::TRANSLATION_PREFIX);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    let mut paragraphs = Vec::new();
    for k in keys {
        let (book, key, tr) = match k.strip_prefix(concept_engine::prompts::TRANSLATION_PREFIX) {
            Some(rest) => (books.translation, rest.to_string(), true),
            None => (Some(books.original), k.clone(), false),
        };
        if let Some(b) = book
            && let Some(p) = b.paragraph(&key)
        {
            paragraphs.push(AnchoredParagraph { key: k.clone(), text: p.text.clone(), role: p.role, pages: if tr { None } else { b.paragraph_pages(p.seq) }, unit: p.unit, seq: p.seq, translation: tr });
        }
    }
    Ok(RunView { svg: graph::svg(&graph::layout(&run)), running: concept.running().as_deref() == Some(uid), cost_usd: run.cost_usd(), translation_uid: l.translation.as_ref().map(|(u, _)| u.clone()), text_uid, findings, paragraphs, run })
}

#[tauri::command(async)]
pub fn concept_run_get(state: State<AppState>, concept: State<ConceptState>, uid: String) -> Result<RunView, String> {
    view(&state, &concept, &uid)
}

fn sweep(state: &AppState) {
    if let Ok(keep) = state.with_db(|c| store::snapshot_shas(c)) {
        shelf::snapshots(&state.paths.root).sweep(&keep);
    }
}

#[tauri::command(async)]
pub fn concept_run_delete(state: State<AppState>, concept: State<ConceptState>, uid: String) -> Result<(), String> {
    if concept.running().as_deref() == Some(uid.as_str()) {
        return Err("这次研究正在进行，先停止。".into());
    }
    state.with_db(|c| store::run_delete(c, &uid))?;
    sweep(&state);
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

pub fn export_bytes(run: &Run, books: &Books, findings: &[Finding], format: Format) -> Result<Vec<u8>, String> {
    Ok(match format {
        Format::Markdown => render::markdown(run, books, findings).into_bytes(),
        Format::Html => render::html(run, books, findings).into_bytes(),
        // Word：和事实调查一样，用文档引擎拼最小的 .docx（`research/docx.rs`，同一份 Doc）
        Format::Docx => crate::research::docx::write_with(&render::document(run, books, findings), "概要", None)?,
    })
}

fn export_path(dir: &Path, title: &str, ext: &str) -> PathBuf {
    let base: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '？' | '：') || c.is_control() { ' ' } else { c }).take(40).collect();
    let base = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = if base.is_empty() { "概念研究".to_string() } else { base };
    let name = |n: u32| if n == 1 { format!("{base}.{ext}") } else { format!("{base}（{n}）.{ext}") };
    let mut n = 1;
    while dir.join(name(n)).exists() {
        n += 1;
    }
    dir.join(name(n))
}

/// 导出到选的文件夹（同名的不覆盖）。返回文件路径；在访达中显示用 `research_reveal`
#[tauri::command(async)]
pub fn concept_export(state: State<AppState>, uid: String, format: Format, dir: String) -> Result<String, String> {
    let (text_uid, run) = state.with_db(|c| Ok((store::run_text(c, &uid)?, store::run_load(c, &uid)?)))?;
    let l = state.with_db(|c| load_books(c, &text_uid))?;
    let books = l.books();
    let texts = source_lookup(&state, &run);
    let lookup = |id: &str| texts.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone());
    let findings = audit::audit(&run, &books, &lookup);
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err("选的不是一个文件夹。".into());
    }
    let ext = match format {
        Format::Markdown => "md",
        Format::Html => "html",
        Format::Docx => "docx",
    };
    let path = export_path(&dir, &render::title(&run), ext);
    std::fs::write(&path, export_bytes(&run, &books, &findings, format)?).map_err(|e| format!("写不了 {}：{e}", path.display()))?;
    Ok(path.display().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResult {
    added: Vec<shelf::Added>,
    view: RunView,
}

/// 把网上的评论来源加入文献库（原件作为附件）。先做「导入前」备份
#[tauri::command(async)]
pub fn concept_add_to_library(state: State<AppState>, concept: State<ConceptState>, uid: String, keys: Vec<String>) -> Result<LibraryResult, String> {
    let run = state.with_db(|c| store::run_load(c, &uid))?;
    let snaps = shelf::snapshots(&state.paths.root);
    let added = crate::mutate(&state, |c| {
        crate::import::backup_before(c, &state.paths.backups)?;
        shelf::add_sources(c, &state.paths.attachments, &snaps, &run, &keys)
    })?;
    Ok(LibraryResult { added, view: view(&state, &concept, &uid)? })
}

// ---------- Word 搜索窗：卡片里的原文 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub run_uid: String,
    pub card: String,
    card_title: String,
    /// 原文句（英文原著的；中译本身是一份文本时是中文）
    pub text: String,
    pub language: String,
    pub paragraph: String,
    /// 有页码的版本才有
    pub pages: Option<String>,
    /// 原文句对应的中译句（核对通过的）
    zh: Option<String>,
    zh_paragraph: Option<String>,
}

/// 一条文献（文献库 uid）解析过的书里，卡片上核对通过的原文句（Word 搜索窗的「卡片里的原文」）
#[tauri::command(async)]
pub fn concept_quotes(state: State<AppState>, item_uid: String) -> Result<Vec<Quote>, String> {
    state.with_db(|c| quotes_for_item(c, &item_uid))
}

pub fn quotes_for_item(c: &rusqlite::Connection, item_uid: &str) -> Result<Vec<Quote>, String> {
    let item: Option<i64> = c.query_row("SELECT id FROM items WHERE uid = ?1", [item_uid], |r| r.get(0)).ok();
    let Some(item) = item else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for text_uid in store::texts_of_item(c, item)? {
        let language = store::text_row_get(c, &text_uid)?.language;
        for r in store::runs_list(c, &text_uid)? {
            let run = store::run_load(c, &r.uid)?;
            for card in &run.cards {
                for s in card.sentences.iter().filter(|s| s.category == concept_engine::model::Category::Original && s.check.status == concept_engine::model::Status::Verified) {
                    let (Some(paragraph), Some(q)) = (&s.paragraph, s.quote.as_ref().or(Some(&s.text))) else { continue };
                    if out.iter().any(|x: &Quote| &x.text == q && &x.paragraph == paragraph) {
                        continue;
                    }
                    let zh_ok = s.check.zh_status == Some(concept_engine::model::ZhStatus::Verified);
                    out.push(Quote { run_uid: run.uid.clone(), card: card.id.clone(), card_title: card.title.clone(), text: q.clone(), language: language.clone(), paragraph: paragraph.clone(), pages: s.check.pages.clone(), zh: s.zh.clone().filter(|_| zh_ok), zh_paragraph: s.check.zh_paragraph.clone() });
                }
            }
        }
        // 译本（中文）那份文本：卡片引的是原著，搜索窗选中文译本时给中译句
    }
    // 中文译本这条文献：用它对应的原著那边的卡片，给中译句（定位是段落号）
    let mut st = c.prepare("SELECT o.uid, t.uid FROM concept_texts t JOIN concept_texts o ON o.id = t.original_id WHERE t.item_id = ?1").map_err(crate::common::err)?;
    let pairs: Vec<(String, String)> = st.query_map([item], |r| Ok((r.get(0)?, r.get(1)?))).map_err(crate::common::err)?.collect::<Result<_, _>>().map_err(crate::common::err)?;
    for (original, translation) in pairs {
        let language = store::text_row_get(c, &translation)?.language;
        for r in store::runs_list(c, &original)? {
            let run = store::run_load(c, &r.uid)?;
            for card in &run.cards {
                for s in card.sentences.iter().filter(|s| s.category == concept_engine::model::Category::Original && s.check.zh_status == Some(concept_engine::model::ZhStatus::Verified)) {
                    let (Some(zh), Some(zp)) = (&s.zh, &s.check.zh_paragraph) else { continue };
                    if out.iter().any(|x: &Quote| &x.text == zh) {
                        continue;
                    }
                    out.push(Quote { run_uid: run.uid.clone(), card: card.id.clone(), card_title: card.title.clone(), text: zh.clone(), language: language.clone(), paragraph: zp.clone(), pages: None, zh: None, zh_paragraph: None });
                }
            }
        }
    }
    Ok(out)
}

// ---------- C4 真实验收（默认忽略；会联网花钱，事先问用户上限） ----------

#[cfg(test)]
mod real_acceptance {
    //! 和 App 的命令走同一条路：`run_prepare`（确认框里的内容）→ `run_create` → `execute_now`，结果存进 `PITH_ACCEPT_DIR`，
    //! 打包版用 `PITH_DATA_DIR` 指过去就能在 App 里看。命令见 `docs/testing.md`「概念研究」。
    use super::*;
    use crate::edit::{self, CreatorInput, ItemPatch};
    use crate::paths::DataPaths;
    use concept_engine::model::{Category, Status};
    use std::collections::BTreeMap;

    fn env(k: &str) -> Option<String> {
        std::env::var(k).ok().filter(|v| !v.trim().is_empty())
    }

    fn person(role: &str, family: &str, given: &str) -> CreatorInput {
        CreatorInput { role: role.into(), family: Some(family.into()), given: Some(given.into()), literal: None }
    }

    fn literal(role: &str, name: &str) -> CreatorInput {
        CreatorInput { role: role.into(), family: None, given: None, literal: Some(name.into()) }
    }

    /// 文献库里建一条书目、附上文件，返回（条目 id, 附件 id）
    fn item_with(state: &AppState, c: &rusqlite::Connection, kind: &str, fields: &[(&str, &str)], creators: Vec<CreatorInput>, file: &Path) -> Result<(i64, i64), String> {
        let id = edit::item_create(c, kind, None)?;
        let fields: BTreeMap<String, String> = fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        edit::item_update(c, id, &ItemPatch { fields, creators: Some(creators), ..Default::default() })?;
        let (aid, mut docs) = edit::attachment_add(c, &state.paths.attachments, Some(id), file)?;
        docs.push(crate::search::DocRef::item(id));
        crate::search::update(c, &docs)?;
        Ok((id, aid))
    }

    fn print_text(t: &Text) {
        let body = t.paragraphs.iter().filter(|p| p.role == Role::Body).count();
        println!("  段落 {}（正文 {body}），单元 {}，页码来源 {:?}，段落号 {:?}", t.paragraphs.len(), t.units.len(), t.page_source, t.numbering);
        for u in t.chapters().into_iter().take(3) {
            let n = t.body_of(u.index).count();
            let pages: Vec<String> = t.body_of(u.index).filter_map(|p| t.paragraph_pages(p.seq)).collect();
            println!("  {} {}：正文 {n} 段，页码 {}–{}", u.key, u.label, pages.first().map(String::as_str).unwrap_or("-"), pages.last().map(String::as_str).unwrap_or("-"));
        }
    }

    fn summarize(state: &AppState, concept: &ConceptState, uid: &str) {
        let v = view(state, concept, uid).unwrap();
        let run = &v.run;
        println!("\n研究 {uid}：{:?}，错误 {:?}，费用 ${:.4}", run.status, run.error, v.cost_usd);
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut cats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut unanchored = 0;
        for card in &run.cards {
            *kinds.entry(format!("{:?}", card.kind)).or_default() += 1;
            for s in &card.sentences {
                let e = cats.entry(format!("{:?}", s.category)).or_default();
                e.0 += 1;
                if s.check.status == Status::Verified {
                    e.1 += 1;
                }
                if s.category == Category::Explanation && s.paragraph.is_none() {
                    unanchored += 1;
                }
            }
        }
        println!("卡片 {kinds:?}；关系 {}；句子（共, 核对通过）{cats:?}；没挂上原文的解释 {unanchored}", run.relations.len());
        let zh_ok = run.cards.iter().flat_map(|c| &c.sentences).filter(|s| s.check.zh_status == Some(concept_engine::model::ZhStatus::Verified)).count();
        println!("中译句核对通过 {zh_ok}；来源 {}；拦下的句子 {}；审计问题 {}", run.sources.len(), run.rejected.len(), v.findings.len());
        for r in &run.rejected {
            println!("  拦下：{} {} — {}", r.card, r.sentence.paragraph.as_deref().unwrap_or("-"), r.reason);
        }
        for f in &v.findings {
            println!("  审计：{f:?}");
        }
        for card in &run.cards {
            println!("  {} [{:?}] {}（{} 句）", card.id, card.kind, card.title, card.sentences.len());
        }
        if let Some(dir) = env("PITH_EXPORT_DIR") {
            let l = state.with_db(|c| load_books(c, &v.text_uid)).unwrap();
            for (format, ext) in [(Format::Markdown, "md"), (Format::Html, "html"), (Format::Docx, "docx")] {
                let path = export_path(Path::new(&dir), &render::title(run), ext);
                std::fs::write(&path, export_bytes(run, &l.books(), &v.findings, format).unwrap()).unwrap();
                println!("导出 {}", path.display());
            }
        }
    }

    #[test]
    #[ignore]
    fn real_concept_run() {
        let root = PathBuf::from(env("PITH_ACCEPT_DIR").expect("要设 PITH_ACCEPT_DIR（数据目录）"));
        let state = AppState::init(DataPaths::new(root.clone()));
        let (agent, research, concept) = (AgentState::default(), ResearchState::default(), ConceptState::default());
        if let Some(b) = env("PITH_BUDGET") {
            let b: f64 = b.parse().expect("PITH_BUDGET 要是数");
            state.with_db(|c| crate::metadata::put_setting(c, format_agent::BUDGET_KEY, Some(&b.to_string()))).unwrap();
        }
        if let Some(e) = env("PITH_EFFORT") {
            let s = Settings { effort: e, ..settings(&state) };
            s.check().unwrap();
            state.with_db(|c| crate::metadata::put_setting(c, SETTINGS_KEY, Some(&serde_json::to_string(&s).unwrap()))).unwrap();
        }
        let ledger = Ledger::new(&root);
        let started = apptext::now_iso();
        let spent = |at: &str| ledger.all().unwrap().iter().filter(|c| !c.mock && c.at.as_str() >= at).map(|c| c.cost_usd).sum::<f64>();
        println!("模拟模式 {}；月上限 ${:.2}，本月已花 ${:.4}", mock_mode(), format_agent::budget(&state), ledger.month_total(&started[..7]).unwrap());
        let texts = state.with_db(|c| store::texts_list(c)).unwrap();
        let find = |lang: &str| texts.iter().find(|t| t.language == lang).map(|t| t.uid.clone());
        match env("PITH_STAGE").as_deref().unwrap_or("books") {
            // 两本书进文献库、解析、存、对齐（不联网）
            "books" => {
                if !texts.is_empty() {
                    println!("已经有 {} 份文本，跳过", texts.len());
                    return;
                }
                let en = PathBuf::from(env("PITH_BOOK_EN").expect("要设 PITH_BOOK_EN"));
                let zh = PathBuf::from(env("PITH_BOOK_ZH").expect("要设 PITH_BOOK_ZH"));
                let saved = state
                    .with_db(|c| {
                        crate::import::backup_before(c, &state.paths.backups)?;
                        let (ei, ea) = item_with(&state, c, "book", &[("title", "The Wealth of Nations"), ("publisher", "Penguin Books"), ("place", "London"), ("date", "1999"), ("series", "Penguin Classics"), ("language", "en")], vec![person("author", "Smith", "Adam"), person("editor", "Skinner", "Andrew")], &en)?;
                        let f = shelf::attachment_file(c, &state.paths.attachments, ea)?;
                        let (t, meta) = parse_epub(&std::fs::read(&f.path).map_err(|e| e.to_string())?)?;
                        println!("英文：");
                        print_text(&t);
                        let en_uid = save_text(c, &state.paths.backups, &t, &meta, Some(ea), Some(ei), &f.sha256, None)?;
                        let (zi, za) = item_with(&state, c, "book", &[("title", "国富论"), ("publisher", "陕西人民出版社"), ("place", "西安"), ("date", "2011"), ("ISBN", "9787224094282"), ("language", "zh")], vec![literal("author", "亚当·斯密"), literal("translator", "杨敬年")], &zh)?;
                        let f = shelf::attachment_file(c, &state.paths.attachments, za)?;
                        let (t, meta) = parse_epub(&std::fs::read(&f.path).map_err(|e| e.to_string())?)?;
                        println!("中文：");
                        print_text(&t);
                        let zh_uid = save_text(c, &state.paths.backups, &t, &meta, Some(za), Some(zi), &f.sha256, Some(&en_uid))?;
                        Ok((en_uid, zh_uid))
                    })
                    .unwrap();
                let al = state.with_db(|c| store::alignment_load(c, &saved.0, &saved.1)).unwrap().unwrap();
                println!("存好：英文 {}，中文 {}；对齐的章 {:?}", saved.0, saved.1, al.units.iter().filter(|u| u.aligned).map(|u| u.key.clone()).take(5).collect::<Vec<_>>());
            }
            // 一份短 PDF 经 Mathpix 转文字（花钱：每页 $0.005）
            "pdf" => {
                let path = PathBuf::from(env("PITH_PDF").expect("要设 PITH_PDF"));
                let bytes = std::fs::read(&path).unwrap();
                let pages = mathpix::local_pages(&bytes).unwrap().len() as u32;
                let s = settings(&state);
                println!("{pages} 页，预计 ${:.3}", pages as f64 * mathpix::PAGE_USD);
                let meter = Arc::new(Meter::new(&root, format_agent::budget(&state), s.prices.clone()));
                let client = Mathpix::new(s.mathpix_base.as_deref(), &load_key(&agent, &research, ApiKey::Mathpix).unwrap(), meter).unwrap();
                let done = client.convert(&bytes, &path.file_name().unwrap().to_string_lossy(), pages, &concept.cancel, &|p| println!("  … {p}")).map_err(|e| e.message).unwrap();
                println!("Mathpix {}：{} 页，${:.4}，删掉副本 {}", done.pdf_id, done.pages, done.cost_usd, done.deleted);
                if let Some(dir) = env("PITH_EXPORT_DIR") {
                    std::fs::write(Path::new(&dir).join("mathpix-lines.json"), serde_json::to_string_pretty(&done.lines).unwrap()).unwrap();
                    std::fs::write(Path::new(&dir).join("mathpix.mmd"), done.mmd.clone().unwrap_or_default()).unwrap();
                }
                let title = env("PITH_PDF_TITLE").unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into_owned());
                let t = text_mathpix::from_lines(&done.lines, done.mmd.as_deref(), &title, None).unwrap();
                for p in &t.paragraphs {
                    println!("  {:?} {} 页{:?}：{}", p.role, p.key, t.paragraph_pages(p.seq), p.text.chars().take(30).collect::<String>());
                }
                let uid = state
                    .with_db(|c| {
                        crate::import::backup_before(c, &state.paths.backups)?;
                        let (i, a) = item_with(&state, c, "magazineArticle", &[("title", &title), ("publicationTitle", "示例周刊"), ("date", "2026"), ("language", "zh")], vec![literal("author", "林舟")], &path)?;
                        let f = shelf::attachment_file(c, &state.paths.attachments, a)?;
                        let meta = store::Meta { converter: Some("mathpix".into()), pages: Some(pages), warnings: t.warnings.clone(), ..Default::default() };
                        save_text(c, &state.paths.backups, &t, &meta, Some(a), Some(i), &f.sha256, None)
                    })
                    .unwrap();
                println!("存好：{uid}");
            }
            // 一次研究：PITH_TEXT=en|zh|pdf|<uid>，PITH_REQUEST 是确认框之前的请求（不含 textUid），PITH_GO=1 才真的发
            "run" => {
                let text_uid = match env("PITH_TEXT").as_deref().unwrap_or("en") {
                    "en" => find("en").expect("先跑 books"),
                    "zh" => find("zh").expect("先跑 books"),
                    "pdf" => texts.iter().find(|t| t.format == "pdf").map(|t| t.uid.clone()).expect("先跑 pdf"),
                    other => other.to_string(),
                };
                let mut req: serde_json::Value = serde_json::from_str(&env("PITH_REQUEST").expect("要设 PITH_REQUEST")).unwrap();
                req["textUid"] = text_uid.clone().into();
                // 和页面一样：整章给出正文段；问题、概念先在本地检索，取前几条
                if req["paragraphs"].as_array().is_none_or(|a| a.is_empty()) {
                    let l = state.with_db(|c| load_books(c, &text_uid)).unwrap();
                    let t = &l.original;
                    let keys: Vec<String> = if let Some(unit) = req["unit"].as_str() {
                        let u = t.chapters().into_iter().find(|u| u.key == unit).expect("没有这一章").index;
                        t.body_of(u).map(|p| p.key.clone()).collect()
                    } else {
                        let limit: usize = env("PITH_LIMIT").and_then(|v| v.parse().ok()).unwrap_or(8);
                        let mut queries: Vec<String> = req["terms"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                        if let Some(q) = env("PITH_SEARCH").or_else(|| req["question"].as_str().map(String::from)) {
                            queries.insert(0, q);
                        }
                        let mut keys = Vec::new();
                        for q in &queries {
                            for h in state.with_db(|c| search_hits(c, &text_uid, q, limit)).unwrap() {
                                println!("  检索「{q}」：{} {}（经中译 {}）", h.key, h.unit_key, h.via_translation);
                                if h.role == Role::Body && !keys.contains(&h.key) {
                                    keys.push(h.key);
                                }
                            }
                        }
                        keys.truncate(limit);
                        keys
                    };
                    req["paragraphs"] = keys.into();
                }
                let request: RunRequest = serde_json::from_value(req).unwrap();
                let preview = run_prepare(&state, &agent, &research, &concept, Some(request), None).unwrap();
                let mut shown = serde_json::to_value(&preview).unwrap();
                shown.as_object_mut().unwrap().remove("ticket");
                println!("确认框：{}", serde_json::to_string_pretty(&shown).unwrap());
                if env("PITH_GO").as_deref() != Some("1") {
                    println!("（没有 PITH_GO=1，只看确认框，不发送）");
                    return;
                }
                assert!(preview.within_budget && preview.missing_keys.is_empty(), "超上限或缺密钥");
                let uid = run_create(&state, &agent, &research, &concept, preview.ticket).unwrap();
                let progress = |p: Progress| println!("  … {}", serde_json::to_string(&p).unwrap_or_default());
                let status = execute_now(&state, &agent, &research, &concept, &uid, &progress);
                println!("结束：{status:?}；这次花了 ${:.4}", spent(&started));
                summarize(&state, &concept, &uid);
            }
            // 本地检索（不联网）：PITH_TEXT、PITH_SEARCH、PITH_LIMIT
            "search" => {
                let text_uid = find(env("PITH_TEXT").as_deref().unwrap_or("en")).expect("先跑 books");
                let limit: usize = env("PITH_LIMIT").and_then(|v| v.parse().ok()).unwrap_or(8);
                for h in state.with_db(|c| search_hits(c, &text_uid, &env("PITH_SEARCH").expect("要设 PITH_SEARCH"), limit)).unwrap() {
                    println!("  {} {:?}（经中译 {}）", h.key, h.role, h.via_translation);
                }
            }
            // 只重新审计、导出（不联网）
            "export" => summarize(&state, &concept, &env("PITH_RUN").expect("要设 PITH_RUN")),
            // Word 搜索窗里「卡片里的原文」
            "quotes" => {
                let items: Vec<String> = state.with_db(|c| texts_list_items(c)).unwrap();
                for item in items {
                    let q = state.with_db(|c| quotes_for_item(c, &item)).unwrap();
                    println!("文献 {item}：{} 句", q.len());
                    for x in q.iter().take(4) {
                        println!("  {} {} 页 {:?}：{}", x.language, x.paragraph, x.pages, x.text.chars().take(50).collect::<String>());
                    }
                }
            }
            other => panic!("不认识的 PITH_STAGE：{other}"),
        }
    }

    /// 把一个数据目录里的概念研究（书、文本、研究、评论来源的快照、账本里的真实调用）导进另一个数据目录。
    /// 用户要求把 C4 的真实验收结果导进正式版（同 v0.3 的 `import_research_runs`，规则 4 的一次性例外）。
    /// 文本和研究的 uid 不变，已有的跳过；文献和附件在目标里新建（附件按 SHA-256 核对）；先做「导入前」备份，导完重新审计。
    /// 目标是正式数据目录时先退出正式版；`PITH_DRY_RUN=1` 只列出要导的东西，不打开目标（打开会把它升到最新版本）。
    #[test]
    #[ignore]
    fn import_concept_runs() {
        use std::collections::HashMap;
        let from = PathBuf::from(env("PITH_FROM").expect("要设 PITH_FROM"));
        let to = PathBuf::from(env("PITH_TO").expect("要设 PITH_TO"));
        assert_ne!(from, to);
        let dry = env("PITH_DRY_RUN").as_deref() == Some("1");
        let src = AppState::init(DataPaths::new(from.clone()));
        let mut texts = src.with_db(|c| store::texts_list(c)).unwrap();
        // 原著先导，译本要连上它
        texts.sort_by_key(|t| t.original_uid.is_some());
        let runs: Vec<(String, Vec<String>)> = texts.iter().map(|t| (t.uid.clone(), src.with_db(|c| store::runs_list(c, &t.uid)).unwrap().into_iter().map(|r| r.uid).collect())).collect();
        let src_ledger = Ledger::new(&from);
        let real: Vec<_> = src_ledger.all().unwrap().into_iter().filter(|c| !c.mock).collect();
        println!("源 {}：{} 份文本、{} 次研究、账本里 {} 笔真实调用（${:.4}）", from.display(), texts.len(), runs.iter().map(|r| r.1.len()).sum::<usize>(), real.len(), real.iter().map(|c| c.cost_usd).sum::<f64>());
        for t in &texts {
            println!("  {} {}（{}，{}）", t.uid, t.title, t.language, t.format);
        }
        if dry {
            println!("（PITH_DRY_RUN=1：只列出来，不打开目标）");
            return;
        }
        let dst = AppState::init(DataPaths::new(to.clone()));
        let b = dst.with_db(|c| crate::import::backup_before(c, &dst.paths.backups)).unwrap();
        println!("目标 {}：已做「导入前」备份 {b}", to.display());
        // 文献和附件
        let mut items: HashMap<i64, i64> = HashMap::new();
        let tmp = std::env::temp_dir().join(format!("pith-import-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let copy_item = |si: i64| -> i64 {
            let (kind, fields, creators) = src
                .with_db(|c| {
                    let kind: String = c.query_row("SELECT item_type FROM items WHERE id = ?1", [si], |r| r.get(0)).map_err(crate::common::err)?;
                    let mut st = c.prepare("SELECT field, value FROM item_fields WHERE item_id = ?1").map_err(crate::common::err)?;
                    let fields: Vec<(String, String)> = st.query_map([si], |r| Ok((r.get(0)?, r.get(1)?))).map_err(crate::common::err)?.collect::<Result<_, _>>().map_err(crate::common::err)?;
                    let mut st = c.prepare("SELECT role, family, given, literal FROM creators WHERE item_id = ?1 ORDER BY position").map_err(crate::common::err)?;
                    let creators: Vec<CreatorInput> = st.query_map([si], |r| Ok(CreatorInput { role: r.get(0)?, family: r.get(1)?, given: r.get(2)?, literal: r.get(3)? })).map_err(crate::common::err)?.collect::<Result<_, _>>().map_err(crate::common::err)?;
                    Ok((kind, fields, creators))
                })
                .unwrap();
            // 日期存的是 Zotero 两段式（「1999-00-00 1999」），编辑接口要的是后一段
            let fields: BTreeMap<String, String> = fields.into_iter().map(|(k, v)| if k == "date" { let d = v.split_once(' ').map(|(_, b)| b.to_string()).unwrap_or(v); (k, d) } else { (k, v) }).collect();
            dst.with_db(|c| {
                let id = edit::item_create(c, &kind, None)?;
                edit::item_update(c, id, &ItemPatch { fields, creators: Some(creators), ..Default::default() })?;
                crate::search::update(c, &[crate::search::DocRef::item(id)])?;
                Ok(id)
            })
            .unwrap()
        };
        let mut imported_texts = 0;
        for t in &texts {
            if dst.with_db(|c| store::text_id(c, &t.uid)).is_ok() {
                println!("  文本 {} 目标里已经有了，跳过", t.uid);
                continue;
            }
            let item = t.item_id.map(|si| *items.entry(si).or_insert_with(|| copy_item(si)));
            let att = t.attachment_id.map(|sa| {
                let f = src.with_db(|c| shelf::attachment_file(c, &src.paths.attachments, sa)).unwrap();
                let named = tmp.join(&f.name);
                std::fs::copy(&f.path, &named).unwrap();
                let aid = dst
                    .with_db(|c| {
                        let (aid, docs) = edit::attachment_add(c, &dst.paths.attachments, item, &named)?;
                        crate::search::update(c, &docs)?;
                        Ok(aid)
                    })
                    .unwrap();
                let got = dst.with_db(|c| shelf::attachment_file(c, &dst.paths.attachments, aid)).unwrap();
                assert_eq!(got.sha256, t.source_sha256, "{} 的附件哈希对不上", f.name);
                aid
            });
            let text = src.with_db(|c| store::text_load(c, &t.uid)).unwrap();
            dst.with_db(|c| {
                let now = apptext::now_iso();
                let id = store::text_save(c, &store::NewText { uid: &t.uid, item_id: item, attachment_id: att, source_sha256: &t.source_sha256, text: &text, meta: t.meta.clone(), original_uid: t.original_uid.as_deref(), now: &now })?;
                if let Some(o) = &t.original_uid {
                    let original = store::text_load(c, o)?;
                    store::alignment_save(c, o, &t.uid, &align::align(&original, &text), &now)?;
                }
                search::add_text(c, id)
            })
            .unwrap();
            // 读回来逐段一致
            assert_eq!(dst.with_db(|c| store::text_load(c, &t.uid)).unwrap().paragraphs, text.paragraphs);
            imported_texts += 1;
            println!("  文本 {}：{} 段，文献 {:?}、附件 {:?}", t.title, text.paragraphs.len(), item, att);
        }
        let _ = std::fs::remove_dir_all(&tmp);
        // 研究和评论来源的快照
        let (src_snap, dst_snap) = (shelf::snapshots(&from), shelf::snapshots(&to));
        let mut imported_runs = Vec::new();
        for (text_uid, uids) in &runs {
            for uid in uids {
                if dst.with_db(|c| store::run_load(c, uid)).is_ok() {
                    println!("  研究 {uid} 目标里已经有了，跳过");
                    continue;
                }
                let mut run = src.with_db(|c| store::run_load(c, uid)).unwrap();
                for s in &mut run.sources {
                    // 文献库里的编号在目标里对不上
                    (s.item_id, s.attachment_id, s.added_item) = (None, None, None);
                    let Some(sha) = s.sha256.clone() else { continue };
                    let original = src_snap.original(&sha, s.ext.as_deref().unwrap_or("")).unwrap();
                    if original.is_file() {
                        let got = dst_snap.put(&std::fs::read(&original).unwrap(), s.ext.as_deref().unwrap_or("")).unwrap();
                        assert_eq!(got, sha, "{} 的快照哈希对不上", s.id);
                    }
                    dst_snap.put_text(&sha, &src_snap.text(&sha).unwrap()).unwrap();
                }
                dst.with_db(|c| store::run_save(c, text_uid, &run)).unwrap();
                println!("  研究 {uid}（{}）：{} 张卡片、{} 个来源", run.skill, run.cards.len(), run.sources.len());
                imported_runs.push(uid.clone());
            }
        }
        // 账本：真实调用补进目标（按时间、用途、模型、费用认同一笔）
        let dst_ledger = Ledger::new(&to);
        let have = dst_ledger.all().unwrap();
        let add: Vec<_> = real.into_iter().filter(|c| !have.iter().any(|h| h.at == c.at && h.purpose == c.purpose && h.model == c.model && (h.cost_usd - c.cost_usd).abs() < 1e-12)).collect();
        for c in &add {
            dst_ledger.append(c).unwrap();
        }
        println!("账本：补进 {} 笔，${:.4}；目标本月已花 ${:.4}", add.len(), add.iter().map(|c| c.cost_usd).sum::<f64>(), dst_ledger.month_total(&apptext::now_iso()[..7]).unwrap());
        // 导完重新审计（和 App 打开研究时一样）
        let concept = ConceptState::default();
        for uid in &imported_runs {
            let v = view(&dst, &concept, uid).unwrap();
            println!("  审计 {uid}：{} 个问题", v.findings.len());
            assert!(v.findings.is_empty(), "{:?}", v.findings);
        }
        println!("导入：文本 {imported_texts} 份、研究 {} 次", imported_runs.len());
    }

    fn texts_list_items(c: &rusqlite::Connection) -> Result<Vec<String>, String> {
        let mut st = c.prepare("SELECT DISTINCT i.uid FROM concept_texts t JOIN items i ON i.id = t.item_id").map_err(crate::common::err)?;
        let rows = st.query_map([], |r| r.get(0)).map_err(crate::common::err)?.collect::<Result<Vec<String>, _>>().map_err(crate::common::err)?;
        Ok(rows)
    }
}
