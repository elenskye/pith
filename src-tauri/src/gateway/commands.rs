//! 设置 › 模型与费用、输入框下面的模型和用量圈用的命令（`model_*`、`usage_*`）。
//!
//! - 默认模型（`model.prefs`）：对话用哪个模型、读整章写长东西的子代理用哪个（空着：对话那一家最强的）。没填密钥的那一家不能选。
//!   1.1.0 第 1 期存的是「一家 + 一档」，读的时候换成那一档的模型（`decisions/0041`）。
//! - 每个模型的思考（`model.thinking`）：开不开、多深；没定的照清单的默认。对话、子代理、别处都照它。
//! - 模型清单：自带的 + 每家「刷新」从接口拿到的（`discover`）。
//! - 免费额度（`model.free`）：标了的几家（Google、Vertex）账本记 0、不占月上限。
//! - 密钥：Anthropic、OpenAI、DeepSeek、Google、Tavily（联网）、Mathpix（概念研究的 PDF）在这一页填；
//!   Vertex AI 填项目 ID、区域，认证看 gcloud 的 ADC。
//! - 用量：本月合计、各家、各用途、缓存命中、最近几笔（都从账本算，不存别处）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, State};

use super::catalog::{self, Choice, ModelSpec, Pick, Source, Thinking};
use super::discover::{self, Auth};
use super::vertex::{self, VertexConfig};
use super::{Provider, keys};
use crate::AppState;
use crate::agent::secret::{ApiKey, Place};
use crate::agent::usage::{Call, Ledger};

pub const PREFS_KEY: &str = "model.prefs";
pub const THINKING_KEY: &str = "model.thinking";
pub const FREE_KEY: &str = "model.free";

/// 默认模型：对话用的、子代理用的（None：对话那一家最强的）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub model: String,
    #[serde(default)]
    pub subagent: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { model: catalog::main_of(Provider::Anthropic).id.into(), subagent: None }
    }
}

/// 读设置里的默认模型。1.1.0 第 1 期的「一家 + 一档」换成那一档当时的模型
pub fn parse_prefs(json: &str) -> Option<Prefs> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v.get("model").is_some() {
        return serde_json::from_value(v).ok();
    }
    let provider: Provider = serde_json::from_value(v.get("provider")?.clone()).ok()?;
    let tier = v.get("tier").and_then(Value::as_str).unwrap_or("balanced");
    let model = match (provider, tier) {
        (Provider::Anthropic, "cheap") => "anthropic/claude-haiku-5-5",
        (Provider::Anthropic, "strong") => "anthropic/claude-opus-5-5",
        (Provider::Openai, "cheap") => "openai/gpt-6-luna",
        (Provider::Openai, "strong") => "openai/gpt-6.2-astra",
        (Provider::Deepseek, "strong") => "deepseek/deepseek-v4-pro",
        (Provider::Google, "cheap") => "google/gemini-3.5-flash-lite",
        (Provider::Google, "strong") => "google/gemini-4-carbon",
        (Provider::Vertex, "cheap") => "vertex/gemini-3.5-flash-lite",
        (Provider::Vertex, "strong") => "vertex/gemini-4-carbon",
        (p, _) => catalog::main_of(p).id,
    };
    Some(Prefs { model: model.into(), subagent: None })
}

pub fn prefs(state: &AppState) -> Prefs {
    crate::agent::setting(state, PREFS_KEY).and_then(|v| parse_prefs(&v)).unwrap_or_default()
}

pub type Choices = BTreeMap<String, Choice>;

pub fn choices(state: &AppState) -> Choices {
    crate::agent::setting(state, THINKING_KEY).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// 用免费额度的几家
pub fn free(state: &AppState) -> Vec<Provider> {
    crate::agent::setting(state, FREE_KEY).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// 对话、子代理这一回用哪个模型、思考多深（都照设置）。角色写了清单里的模型（`anthropic/claude-opus-5-5`）就用它
pub fn picks(state: &AppState, role_model: Option<&str>) -> (Pick, Pick) {
    discover::ensure_loaded(state);
    let p = prefs(state);
    let c = choices(state);
    let main = role_model.and_then(catalog::model).or_else(|| catalog::model(&p.model)).unwrap_or_else(|| catalog::main_of(Provider::Anthropic));
    // 子代理：设了而且和对话同一家就用它，不然用对话那一家最强的（换家要换密钥、断缓存）
    let sub = p.subagent.as_deref().and_then(catalog::model).filter(|s| s.provider == main.provider).unwrap_or_else(|| catalog::strongest(main.provider));
    (main.settle(c.get(main.id)), sub.settle(c.get(sub.id)))
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
pub struct ModelView {
    id: &'static str,
    label: &'static str,
    api_model: &'static str,
    thinking: Thinking,
    efforts: &'static [&'static str],
    source: Source,
    /// 刷新拿到的（不在自带清单里）
    found: bool,
    priced: bool,
    input: f64,
    output: f64,
    cache_read: f64,
    context: u64,
    /// 现在用的设置、清单的默认、是不是自定义过
    choice: Choice,
    default_choice: Choice,
    custom: bool,
}

fn model_view(m: &'static ModelSpec, c: &Choices) -> ModelView {
    let r = m.price.base;
    let p = m.settle(c.get(m.id));
    ModelView {
        id: m.id,
        label: m.label,
        api_model: m.api_model,
        thinking: m.thinking,
        efforts: m.efforts,
        source: m.source,
        found: m.source != Source::Builtin || !catalog::MODELS.iter().any(|x| x.id == m.id),
        priced: m.priced,
        input: r.input,
        output: r.output,
        cache_read: r.cache_read,
        context: m.context,
        choice: Choice { on: p.effort.is_some() || m.thinking == Thinking::Always, effort: p.effort.map(str::to_string) },
        default_choice: m.default_choice(),
        custom: c.contains_key(m.id),
    }
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
    models: Vec<ModelView>,
    /// 上一次刷新成功的时间；上一次没成的原因
    refreshed_at: Option<String>,
    refresh_note: Option<String>,
    /// 标成用免费额度（只有 Google、Vertex 能标）
    free: Option<bool>,
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
pub struct PrefsView {
    model: String,
    subagent: Option<String>,
    /// 子代理实际用的（「自动」时是对话那一家最强的）
    subagent_used: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// 开发版、测试：模拟模型，不联网、不花钱
    mock: bool,
    prefs: PrefsView,
    providers: Vec<ProviderView>,
    /// 不是对话模型、但在这一页填的密钥（Tavily、Mathpix）
    other_keys: Vec<KeyView>,
    vertex: VertexView,
    budget_usd: f64,
}

pub fn status(app: &AppHandle, state: &AppState) -> ModelStatus {
    discover::ensure_loaded(state);
    let c = choices(state);
    let saved = state.with_db(|db| Ok(discover::saved(db))).unwrap_or_default();
    let free = free(state);
    let providers = Provider::ALL
        .into_iter()
        .map(|p| {
            let l = saved.get(&p);
            ProviderView {
                id: p,
                label: p.label(),
                ready: ready(app, state, p),
                key: keys::key_of(p),
                key_place: keys::key_of(p).map(|k| keys::secret(app, k).place()),
                models: catalog::models_of(p).into_iter().map(|m| model_view(m, &c)).collect(),
                refreshed_at: l.and_then(|l| l.at.clone()),
                refresh_note: l.and_then(|l| l.note.clone()),
                free: matches!(p, Provider::Google | Provider::Vertex).then(|| free.contains(&p)),
            }
        })
        .collect();
    let other = |which: ApiKey, note: &'static str| KeyView { which, label: which.label(), has: keys::present(app, which), place: keys::secret(app, which).place(), note };
    let v = vertex_config(state);
    let p = prefs(state);
    let (_, sub) = picks(state, None);
    ModelStatus {
        mock: !crate::agent::use_real_model(),
        prefs: PrefsView { model: p.model, subagent: p.subagent, subagent_used: sub.model.id },
        providers,
        other_keys: vec![other(ApiKey::Tavily, "联网搜索：模型没有自带搜索时用它"), other(ApiKey::Mathpix, "概念研究：短的 PDF 转成文字")],
        vertex: VertexView { project: v.project, location: v.location, adc: vertex::adc_present(), gcloud: vertex::find_gcloud().is_some() },
        budget_usd: crate::agent::budget(state),
    }
}

fn put(state: &AppState, key: &str, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
    state.with_db(|c| crate::metadata::put_setting(c, key, Some(&json)))
}

#[tauri::command(async)]
pub fn model_status(app: AppHandle, state: State<AppState>) -> ModelStatus {
    status(&app, &state)
}

/// 换默认模型（对话的、子代理的）。没填密钥的那一家不能选
#[tauri::command(async)]
pub fn model_prefs_set(app: AppHandle, state: State<AppState>, prefs: Prefs) -> Result<ModelStatus, String> {
    discover::ensure_loaded(state.inner());
    let m = catalog::model(&prefs.model).ok_or_else(|| "清单里没有这个模型，先在设置里刷新一下这一家的清单。".to_string())?;
    if let Some(s) = prefs.subagent.as_deref() {
        catalog::model(s).ok_or_else(|| "清单里没有子代理要用的这个模型。".to_string())?;
    }
    if crate::agent::use_real_model() && !ready(&app, &state, m.provider) {
        return Err(match m.provider {
            Provider::Vertex => "先填 Vertex AI 的项目 ID，并在终端里跑一次 gcloud auth application-default login。".into(),
            p => format!("先填 {} 的密钥，再选它的模型。", p.label()),
        });
    }
    put(&state, PREFS_KEY, &prefs)?;
    Ok(status(&app, &state))
}

/// 定一个模型的思考：开不开、多深；`choice` 空着就回到默认
#[tauri::command(async)]
pub fn model_thinking_set(app: AppHandle, state: State<AppState>, model: String, choice: Option<Choice>) -> Result<ModelStatus, String> {
    discover::ensure_loaded(state.inner());
    let m = catalog::model(&model).ok_or_else(|| "清单里没有这个模型。".to_string())?;
    let mut c = choices(&state);
    match choice {
        Some(ch) => {
            if let Some(e) = ch.effort.as_deref()
                && !m.efforts.contains(&e)
            {
                return Err(format!("{} 不认「{e}」这一档。", m.label));
            }
            if !ch.on && m.thinking != Thinking::Switch {
                return Err(format!("{} 的思考关不掉。", m.label));
            }
            c.insert(model, ch);
        }
        None => {
            c.remove(&model);
        }
    }
    put(&state, THINKING_KEY, &c)?;
    Ok(status(&app, &state))
}

/// 刷新一家的模型清单（问接口，不花钱）。没成的原因留在这一家上，清单照旧
#[tauri::command(async)]
pub fn model_refresh(app: AppHandle, state: State<AppState>, provider: Provider) -> Result<ModelStatus, String> {
    discover::ensure_loaded(state.inner());
    if !crate::agent::use_real_model() {
        return Err("开发版用模拟模型，不问接口；正式版（或 PITH_REAL_MODEL=1）里才能刷新。".into());
    }
    let auth = match keys::key_of(provider) {
        Some(k) => Auth::Key(keys::load(&app, k)?),
        None => {
            let v = vertex_config(&state);
            if !v.is_set() {
                return Err("先填 Vertex AI 的项目 ID。".into());
            }
            Auth::Vertex(v)
        }
    };
    let ledger = Ledger::new(&state.paths.root);
    let result = discover::list(provider, &auth, Some(&ledger));
    state.with_db(|c| discover::store(c, provider, result, &crate::text::now_iso()))?;
    Ok(status(&app, &state))
}

/// 标一家用免费额度（Google、Vertex）
#[tauri::command(async)]
pub fn model_free_set(app: AppHandle, state: State<AppState>, provider: Provider, free: bool) -> Result<ModelStatus, String> {
    if !matches!(provider, Provider::Google | Provider::Vertex) {
        return Err("只有 Google 和 Vertex AI 能标免费额度。".into());
    }
    let mut list = self::free(&state);
    list.retain(|p| *p != provider);
    if free {
        list.push(provider);
    }
    put(&state, FREE_KEY, &list)?;
    Ok(status(&app, &state))
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
    if catalog::model(&p.model).is_some_and(|m| keys::key_of(m.provider) == Some(which) && m.provider != Provider::Anthropic) {
        put(&state, PREFS_KEY, &Prefs::default())?;
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

#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn old_tier_prefs_become_models() {
        assert_eq!(parse_prefs(r#"{"provider":"deepseek","tier":"balanced"}"#).unwrap().model, "deepseek/deepseek-flash");
        assert_eq!(parse_prefs(r#"{"provider":"anthropic","tier":"strong"}"#).unwrap().model, "anthropic/claude-opus-5-5");
        let p = parse_prefs(r#"{"model":"openai/gpt-6.1-terra","subagent":"openai/gpt-6.2-astra"}"#).unwrap();
        assert_eq!((p.model.as_str(), p.subagent.as_deref()), ("openai/gpt-6.1-terra", Some("openai/gpt-6.2-astra")));
        assert!(parse_prefs("oops").is_none());
    }
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
        "model_probe" => "刷新模型清单",
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
