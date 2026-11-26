//! 设置 › 模型与费用、输入框下面的用量圈用的命令（`model_*`、`usage_*`）。
//!
//! - 默认模型：一家 + 一档（`model.prefs`，默认 Anthropic · 均衡）。没填密钥的那一家不能选成默认。
//! - 密钥：Anthropic、OpenAI、DeepSeek、Google、Tavily（联网，1.1.0 第 4 期用）、Mathpix（概念研究的 PDF）在这一页填；
//!   Vertex AI 填项目 ID、区域，认证看 gcloud 的 ADC。
//! - 用量：本月合计、各家、各用途、缓存命中、最近几笔（都从账本算，不存别处）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use super::catalog::{self, Tier};
use super::vertex::{self, VertexConfig};
use super::{Provider, keys};
use crate::AppState;
use crate::agent::secret::{ApiKey, Place};
use crate::agent::usage::{Call, Ledger};

pub const PREFS_KEY: &str = "model.prefs";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub provider: Provider,
    pub tier: Tier,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { provider: Provider::Anthropic, tier: Tier::Balanced }
    }
}

pub fn prefs(state: &AppState) -> Prefs {
    crate::agent::setting(state, PREFS_KEY).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

pub fn vertex_config(state: &AppState) -> VertexConfig {
    VertexConfig { project: crate::agent::setting(state, vertex::PROJECT_KEY).unwrap_or_default(), location: crate::agent::setting(state, vertex::LOCATION_KEY).unwrap_or_else(|| vertex::DEFAULT_LOCATION.into()) }
}

/// 这一家能不能用：填了密钥；Vertex 是填了项目、有 ADC
pub fn ready(app: &AppHandle, state: &AppState, p: Provider) -> bool {
    match keys::key_of(p) {
        Some(k) => keys::present(app, k),
        None => vertex_config(state).is_set() && vertex::adc_present(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierView {
    tier: Tier,
    label: &'static str,
    model: &'static str,
    model_label: &'static str,
    effort: Option<&'static str>,
    input: f64,
    output: f64,
    cache_read: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    id: Provider,
    label: &'static str,
    ready: bool,
    /// 用哪把密钥（Vertex 没有）
    key: Option<ApiKey>,
    key_place: Option<Place>,
    tiers: Vec<TierView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyView {
    which: ApiKey,
    label: &'static str,
    has: bool,
    place: Place,
    /// 拿它做什么（设置页的一句话）
    note: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexView {
    project: String,
    location: String,
    adc: bool,
    gcloud: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// 开发版、测试：模拟模型，不联网、不花钱
    mock: bool,
    prefs: Prefs,
    providers: Vec<ProviderView>,
    /// 不是对话模型、但在这一页填的密钥（Tavily、Mathpix）
    other_keys: Vec<KeyView>,
    vertex: VertexView,
    budget_usd: f64,
}

pub fn status(app: &AppHandle, state: &AppState) -> ModelStatus {
    let providers = Provider::ALL
        .into_iter()
        .map(|p| ProviderView {
            id: p,
            label: p.label(),
            ready: ready(app, state, p),
            key: keys::key_of(p),
            key_place: keys::key_of(p).map(|k| keys::secret(app, k).place()),
            tiers: Tier::ALL
                .into_iter()
                .map(|t| {
                    let pick = catalog::tier(p, t);
                    let r = pick.model.price.base;
                    TierView { tier: t, label: t.label(), model: pick.model.id, model_label: pick.model.label, effort: pick.effort, input: r.input, output: r.output, cache_read: r.cache_read }
                })
                .collect(),
        })
        .collect();
    let other = |which: ApiKey, note: &'static str| KeyView { which, label: which.label(), has: keys::present(app, which), place: keys::secret(app, which).place(), note };
    let v = vertex_config(state);
    ModelStatus {
        mock: !crate::agent::use_real_model(),
        prefs: prefs(state),
        providers,
        other_keys: vec![other(ApiKey::Tavily, "联网搜索：模型没有自带搜索时用它"), other(ApiKey::Mathpix, "概念研究：短的 PDF 转成文字")],
        vertex: VertexView { project: v.project, location: v.location, adc: vertex::adc_present(), gcloud: vertex::find_gcloud().is_some() },
        budget_usd: crate::agent::budget(state),
    }
}

#[tauri::command(async)]
pub fn model_status(app: AppHandle, state: State<AppState>) -> ModelStatus {
    status(&app, &state)
}

#[tauri::command(async)]
pub fn model_prefs_set(app: AppHandle, state: State<AppState>, prefs: Prefs) -> Result<ModelStatus, String> {
    if !crate::agent::use_real_model() || ready(&app, &state, prefs.provider) {
        let json = serde_json::to_string(&prefs).map_err(|e| e.to_string())?;
        state.with_db(|c| crate::metadata::put_setting(c, PREFS_KEY, Some(&json)))?;
        return Ok(status(&app, &state));
    }
    Err(match prefs.provider {
        Provider::Vertex => "先填 Vertex AI 的项目 ID，并在终端里跑一次 gcloud auth application-default login。".into(),
        p => format!("先填 {} 的密钥，再把它设成默认。", p.label()),
    })
}

/// 填一把密钥。Mathpix 要两项：`key` 是 app_id，`app_key` 是 app_key
#[tauri::command(async)]
pub fn model_key_set(app: AppHandle, state: State<AppState>, which: ApiKey, key: String, app_key: Option<String>) -> Result<ModelStatus, String> {
    let value = if which == ApiKey::Mathpix { crate::concept::keys::mathpix_pair(&key, app_key.as_deref().unwrap_or("")) } else { key };
    keys::save(&app, which, &value)?;
    Ok(status(&app, &state))
}

#[tauri::command(async)]
pub fn model_key_clear(app: AppHandle, state: State<AppState>, which: ApiKey) -> Result<ModelStatus, String> {
    keys::clear(&app, which)?;
    // 默认的那一家没有密钥了：默认换回 Anthropic（它也没有就照旧，发的时候再说）
    let p = prefs(&state);
    if keys::key_of(p.provider) == Some(which) && p.provider != Provider::Anthropic {
        let json = serde_json::to_string(&Prefs { provider: Provider::Anthropic, tier: p.tier }).map_err(|e| e.to_string())?;
        state.with_db(|c| crate::metadata::put_setting(c, PREFS_KEY, Some(&json)))?;
    }
    Ok(status(&app, &state))
}

#[tauri::command(async)]
pub fn model_vertex_set(app: AppHandle, state: State<AppState>, config: VertexConfig) -> Result<ModelStatus, String> {
    config.check()?;
    state.with_db(|c| {
        let tx = crate::common::Tx::begin(c)?;
        crate::metadata::put_setting(c, vertex::PROJECT_KEY, Some(config.project.trim()).filter(|s| !s.is_empty()))?;
        crate::metadata::put_setting(c, vertex::LOCATION_KEY, Some(config.location.trim()).filter(|s| !s.is_empty()))?;
        tx.commit()
    })?;
    vertex::token().forget();
    Ok(status(&app, &state))
}

// ---------- 用量 ----------

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub key: String,
    pub label: String,
    pub usd: f64,
    pub calls: u64,
}

#[derive(Serialize, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheView {
    /// 没命中缓存的输入
    pub input: u64,
    pub read: u64,
    pub write: u64,
    /// 命中的占全部输入的比例（0–1）
    pub hit_rate: f64,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub month: String,
    pub spent_usd: f64,
    pub budget_usd: f64,
    pub mock: bool,
    pub ledger_error: Option<String>,
    pub by_provider: Vec<Slice>,
    pub by_purpose: Vec<Slice>,
    /// 本月每天花了多少（1 号起）
    pub days: Vec<f64>,
    pub cache: CacheView,
    /// 最近的调用，新的在前（最多 20 笔，含模拟的）
    pub recent: Vec<Call>,
    /// 只算这段对话（给了对话 uid 时）
    pub conversation_usd: Option<f64>,
}

pub fn provider_label(p: &str) -> String {
    match p {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "openai_compatible" | "deepseek" => "DeepSeek",
        "google" => "Google Gemini",
        "vertex" => "Vertex AI",
        "tavily" => "Tavily",
        "mathpix" => "Mathpix",
        other => other,
    }
    .to_string()
}

pub fn purpose_label(p: &str) -> &'static str {
    match p {
        "chat" => "对话",
        "chat_subagent" => "子代理",
        "chat_web" => "联网",
        "extract" | "plan" => "排版",
        p if p.starts_with("research") => "事实调查",
        p if p.starts_with("concept") || p.starts_with("mathpix") => "概念研究",
        _ => "其他",
    }
}

fn slices(calls: &[&Call], key: impl Fn(&Call) -> (String, String)) -> Vec<Slice> {
    let mut out: Vec<Slice> = Vec::new();
    for c in calls {
        let (k, label) = key(c);
        match out.iter_mut().find(|s| s.key == k) {
            Some(s) => {
                s.usd += c.cost_usd;
                s.calls += 1;
            }
            None => out.push(Slice { key: k, label, usd: c.cost_usd, calls: 1 }),
        }
    }
    out.sort_by(|a, b| b.usd.total_cmp(&a.usd));
    out
}

/// 从账本算本月的用量（`month` 是 "2026-11"）
pub fn summarize(ledger: &Ledger, month: &str, budget_usd: f64, mock: bool, conversation: Option<&str>) -> UsageSummary {
    let (all, ledger_error) = match ledger.all() {
        Ok(a) => (a, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let this: Vec<&Call> = all.iter().filter(|c| !c.mock && c.at.starts_with(month)).collect();
    let mut days = vec![0.0; 31];
    for c in &this {
        if let Some(d) = c.at.get(8..10).and_then(|d| d.parse::<usize>().ok()).filter(|d| (1..=31).contains(d)) {
            days[d - 1] += c.cost_usd;
        }
    }
    // 这个月：到今天；以前的月：到最后一天有花销的
    let now = crate::text::now_iso();
    let today = now.get(8..10).and_then(|d| d.parse::<usize>().ok()).filter(|_| now.starts_with(month));
    let last_spent = days.iter().rposition(|v| *v > 0.0).map_or(0, |i| i + 1);
    days.truncate(today.unwrap_or(last_spent).max(last_spent));
    let models: Vec<&&Call> = this.iter().filter(|c| c.kind == crate::agent::usage::KIND_MODEL).collect();
    let (input, read, write) = models.iter().fold((0, 0, 0), |(i, r, w), c| (i + c.tokens.input, r + c.tokens.cache_read, w + c.tokens.cache_write));
    let total = input + read + write;
    let mut recent: Vec<Call> = all.iter().rev().take(20).cloned().collect();
    recent.iter_mut().for_each(|c| c.request_id = None);
    UsageSummary {
        month: month.to_string(),
        spent_usd: this.iter().map(|c| c.cost_usd).sum(),
        budget_usd,
        mock,
        ledger_error,
        by_provider: slices(&this, |c| (c.provider.clone(), provider_label(&c.provider))),
        by_purpose: slices(&this, |c| (purpose_label(&c.purpose).to_string(), purpose_label(&c.purpose).to_string())),
        days,
        cache: CacheView { input, read, write, hit_rate: if total > 0 { read as f64 / total as f64 } else { 0.0 } },
        recent,
        conversation_usd: conversation.map(|u| all.iter().filter(|c| !c.mock && c.conversation.as_deref() == Some(u)).map(|c| c.cost_usd).sum()),
    }
}

#[tauri::command(async)]
pub fn usage_summary(state: State<AppState>, conversation: Option<String>) -> UsageSummary {
    let now = crate::text::now_iso();
    summarize(&Ledger::new(&state.paths.root), now.get(..7).unwrap_or(""), crate::agent::budget(&state), !crate::agent::use_real_model(), conversation.as_deref())
}
