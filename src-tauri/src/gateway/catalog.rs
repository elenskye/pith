//! 模型清单和价格表（`decisions/0036`，1.1.0 验收改：`decisions/0041`）。
//!
//! - 每家一份模型清单：Pith 自带的（下面的 `MODELS`，2026-11-27 对着各家接口核过）+ 在设置里「刷新」从接口拿到的（`discover`）。
//!   不再分便宜、均衡、强三档：对话用哪个模型、子代理用哪个，在设置里选；每个模型想不想、想多深，在设置里各定一次，别处都照它。
//! - 思考：`Always` 一直开着（只能调深浅）；`Switch` 能关（Anthropic 关的写法每个模型不同，记在 `off`；OpenAI 写 effort `none`；
//!   DeepSeek 写 `thinking: disabled`）；`Never` 不思考。`efforts` 是这个模型认的深浅，从浅到深。
//! - 价格（美元 / 百万 token）：Anthropic 取官方价目（2026-11）；OpenAI、Google、DeepSeek 取 Peck 2026-11-25 联网查的价格表
//!   （`../personal-cli-studio/peck/src-tauri/defaults/prices.json`）。DeepSeek 是人民币价，按 1 美元 = 6.7 元、一律按高峰（×2）折成美元、
//!   凑整到分（和 v0.3 账本里的 0.30 / 1.20 一样），宁可多记。刷新拿到的、清单里没有价的按最贵的估（`priced: false`）。
//! - 缓存：读、写各有单价；超过 `above` 个 token 的请求整条换一档（Haiku 5.5 的 10 万、OpenAI 的 27.2 万）。
//! - 设置里覆盖的单价（`usage::Prices`）只改输入、输出价，缓存的照这里的比例跟着变。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use super::{Provider, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    pub base: Rate,
    /// 一次请求的提示（输入 + 缓存读写）超过这么多 token，整条按 `high` 算
    pub above: Option<(u64, Rate)>,
}

impl PriceRow {
    pub fn rate_for(&self, prompt_tokens: u64) -> Rate {
        match self.above {
            Some((n, r)) if prompt_tokens > n => r,
            _ => self.base,
        }
    }

    /// 一次调用的美元数
    pub fn usd(&self, t: &Tokens) -> f64 {
        let r = self.rate_for(t.input + t.cache_read + t.cache_write);
        (t.input as f64 * r.input + t.output as f64 * r.output + t.cache_read as f64 * r.cache_read + t.cache_write as f64 * r.cache_write) / 1e6
    }

    /// 设置里改了输入、输出价：缓存价照原来的比例跟着变
    pub fn scaled(&self, input: f64, output: f64) -> PriceRow {
        let s = |r: Rate| {
            let k = if r.input > 0.0 { input / r.input } else { 1.0 };
            Rate { input, output, cache_read: r.cache_read * k, cache_write: r.cache_write * k }
        };
        PriceRow { base: s(self.base), above: self.above.map(|(n, r)| (n, s(r))) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thinking {
    /// 不思考（没有深浅可调）
    Never,
    /// 能开能关
    Switch,
    /// 一直开着（effort 只管深浅）
    Always,
}

/// 这个模型的能力是哪来的（设置里写给站主看）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Pith 自带的清单（对着接口核过）
    Builtin,
    /// 接口直接报的（Anthropic 的 capabilities、OpenAI 拒收时列出的档位、Gemini 的 thinking）
    Api,
    /// 接口没说，照同一系列推断
    Inferred,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// `供应商/模型`，全清单唯一
    pub id: &'static str,
    pub provider: Provider,
    /// 发给供应商的名字
    pub api_model: &'static str,
    pub label: &'static str,
    pub context: u64,
    pub max_output: u32,
    pub thinking: Thinking,
    /// Anthropic：关掉思考时 `thinking.type` 写什么（`disabled`，或 Sonnet 5.5 的 `between_tools`）
    #[serde(skip)]
    pub off: Option<&'static str>,
    /// 认的深浅，从浅到深
    pub efforts: &'static [&'static str],
    /// 没自定义时用哪一档
    pub default_effort: Option<&'static str>,
    pub price: PriceRow,
    /// false：价格表里没有，按最贵的估
    pub priced: bool,
    pub source: Source,
    /// 这一家默认的对话模型
    #[serde(skip)]
    pub main: bool,
    /// 这一家最强的（子代理「自动」时用它）
    #[serde(skip)]
    pub strong: bool,
    /// Anthropic：开服务端的拒答回退（`fallbacks: "default"`）
    #[serde(skip)]
    pub fallback: bool,
}

const fn rate(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Rate {
    Rate { input, output, cache_read, cache_write }
}

const fn flat(r: Rate) -> PriceRow {
    PriceRow { base: r, above: None }
}

/// 自带清单的一行（大多数字段一样，省得每行写全）
#[allow(clippy::too_many_arguments)]
const fn builtin(id: &'static str, provider: Provider, api_model: &'static str, label: &'static str, max_output: u32, thinking: Thinking, efforts: &'static [&'static str], default_effort: &'static str, price: PriceRow) -> ModelSpec {
    ModelSpec {
        id,
        provider,
        api_model,
        label,
        context: 1_000_000,
        max_output,
        thinking,
        off: None,
        efforts,
        default_effort: Some(default_effort),
        price,
        priced: true,
        source: Source::Builtin,
        main: false,
        strong: false,
        fallback: false,
    }
}

const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const GPT_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const DEEPSEEK_EFFORTS: &[&str] = &["low", "high", "max"];
const GEMINI_EFFORTS: &[&str] = &["low", "medium", "high"];
const GEMINI_LITE_EFFORTS: &[&str] = &["minimal", "low", "medium", "high"];

const FABLE: PriceRow = flat(rate(8.0, 40.0, 0.20, 10.0));
const OPUS: PriceRow = flat(rate(4.0, 20.0, 0.20, 5.0));
const SONNET: PriceRow = flat(rate(2.0, 10.0, 0.10, 2.5));
const HAIKU: PriceRow = PriceRow { base: rate(0.10, 0.50, 0.01, 0.125), above: Some((100_000, rate(0.50, 2.50, 0.05, 0.625))) };
const ASTRA: PriceRow = PriceRow { base: rate(8.0, 40.0, 0.40, 10.0), above: Some((272_000, rate(16.0, 60.0, 0.80, 20.0))) };
const SOL: PriceRow = PriceRow { base: rate(2.0, 10.0, 0.10, 2.5), above: Some((272_000, rate(4.0, 15.0, 0.20, 5.0))) };
const TERRA: PriceRow = PriceRow { base: rate(1.0, 5.0, 0.05, 1.25), above: Some((272_000, rate(2.0, 7.5, 0.10, 2.5))) };
const LUNA: PriceRow = PriceRow { base: rate(0.10, 0.50, 0.01, 0.125), above: Some((272_000, rate(0.20, 0.75, 0.02, 0.25))) };
const DS_FLASH: PriceRow = flat(rate(0.30, 1.20, 0.006, 0.30));
const DS_PRO: PriceRow = flat(rate(1.35, 4.05, 0.045, 1.35));
const G_CARBON: PriceRow = flat(rate(2.0, 10.0, 0.10, 2.0));
const G_FLASH: PriceRow = flat(rate(0.75, 3.75, 0.075, 0.75));
const G_LITE: PriceRow = flat(rate(0.30, 2.50, 0.03, 0.30));

use Provider::{Anthropic as A, Deepseek as D, Google as G, Openai as O, Vertex as V};
use Thinking::{Always, Switch};

/// 自带的清单：每家从强到弱。2026-11-27 核过：Anthropic 的思考和深浅照 Models API 的 capabilities；
/// OpenAI 的照接口拒收 `minimal` 时列出的档位（`none` 在里面就是能关）；Gemini 3.5 Flash Lite 认 `minimal`。
pub static MODELS: &[ModelSpec] = &[
    // Anthropic（官方价目；缓存写按 5 分钟的 1.25 倍）。Opus、Fable 关不掉思考；Sonnet 5.5 关是 between_tools，Haiku 5.5 是 disabled（都只在 high 以下）
    ModelSpec { fallback: true, ..builtin("anthropic/claude-fable-5-2", A, "claude-fable-5-2", "Claude Fable 5.2", 128_000, Always, CLAUDE_EFFORTS, "high", FABLE) },
    ModelSpec { fallback: true, strong: true, ..builtin("anthropic/claude-opus-5-5", A, "claude-opus-5-5", "Claude Opus 5.5", 128_000, Always, CLAUDE_EFFORTS, "high", OPUS) },
    ModelSpec { fallback: true, main: true, off: Some("between_tools"), ..builtin("anthropic/claude-sonnet-5-5", A, "claude-sonnet-5-5", "Claude Sonnet 5.5", 128_000, Switch, CLAUDE_EFFORTS, "medium", SONNET) },
    ModelSpec { off: Some("disabled"), ..builtin("anthropic/claude-haiku-5-5", A, "claude-haiku-5-5", "Claude Haiku 5.5", 128_000, Switch, CLAUDE_EFFORTS, "low", HAIKU) },
    // OpenAI（Responses；超过 27.2 万 token 的请求整条换高一档）
    ModelSpec { strong: true, context: 1_050_000, ..builtin("openai/gpt-6.2-astra", O, "gpt-6.2-astra", "GPT-6.2 Astra", 128_000, Always, GPT_EFFORTS, "high", ASTRA) },
    ModelSpec { main: true, context: 1_050_000, ..builtin("openai/gpt-6.2-sol", O, "gpt-6.2-sol", "GPT-6.2 Sol", 128_000, Always, GPT_EFFORTS, "medium", SOL) },
    ModelSpec { context: 1_050_000, ..builtin("openai/gpt-6.1-terra", O, "gpt-6.1-terra", "GPT-6.1 Terra", 128_000, Switch, GPT_EFFORTS, "medium", TERRA) },
    ModelSpec { context: 1_050_000, ..builtin("openai/gpt-6-luna", O, "gpt-6-luna", "GPT-6 Luna", 128_000, Switch, GPT_EFFORTS, "low", LUNA) },
    // DeepSeek（人民币价折美元，按高峰算）
    ModelSpec { strong: true, context: 1_048_576, ..builtin("deepseek/deepseek-v4-pro", D, "deepseek-v4-pro", "DeepSeek V4 Pro", 393_216, Switch, DEEPSEEK_EFFORTS, "high", DS_PRO) },
    ModelSpec { main: true, context: 1_048_576, ..builtin("deepseek/deepseek-flash", D, "deepseek-flash", "DeepSeek V4.1 Flash", 393_216, Switch, DEEPSEEK_EFFORTS, "high", DS_FLASH) },
    // Google Gemini（API Key）。3.x、4 的思考关不掉，最浅是 minimal / low
    ModelSpec { strong: true, context: 1_048_576, ..builtin("google/gemini-4-carbon", G, "gemini-4-carbon", "Gemini 4 Carbon", 32_768, Always, GEMINI_EFFORTS, "high", G_CARBON) },
    ModelSpec { context: 1_048_576, ..builtin("google/gemini-4-argon", G, "gemini-4-argon", "Gemini 4 Argon", 32_768, Always, GEMINI_EFFORTS, "high", G_CARBON) },
    ModelSpec { main: true, context: 1_048_576, ..builtin("google/gemini-3.8-flash", G, "gemini-3.8-flash", "Gemini 3.8 Flash", 65_536, Always, GEMINI_EFFORTS, "medium", G_FLASH) },
    ModelSpec { context: 1_048_576, ..builtin("google/gemini-3.5-flash-lite", G, "gemini-3.5-flash-lite", "Gemini 3.5 Flash Lite", 65_536, Always, GEMINI_LITE_EFFORTS, "low", G_LITE) },
    // Vertex AI 上的 Gemini（同价）
    ModelSpec { strong: true, context: 1_048_576, ..builtin("vertex/gemini-4-carbon", V, "gemini-4-carbon", "Gemini 4 Carbon", 32_768, Always, GEMINI_EFFORTS, "high", G_CARBON) },
    ModelSpec { main: true, context: 1_048_576, ..builtin("vertex/gemini-3.8-flash", V, "gemini-3.8-flash", "Gemini 3.8 Flash", 65_536, Always, GEMINI_EFFORTS, "medium", G_FLASH) },
    ModelSpec { context: 1_048_576, ..builtin("vertex/gemini-3.5-flash-lite", V, "gemini-3.5-flash-lite", "Gemini 3.5 Flash Lite", 65_536, Always, GEMINI_LITE_EFFORTS, "low", G_LITE) },
];

// ---------- 刷新拿到的模型（清单里没有的） ----------

/// 接口报来的一个模型（存在设置里，启动时登记）
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub api_model: String,
    pub label: String,
    pub context: u64,
    pub max_output: u32,
    pub thinking: Thinking,
    #[serde(default)]
    pub off: Option<String>,
    pub efforts: Vec<String>,
    pub source: Source,
}

#[derive(Default)]
struct Registry {
    /// 每家刷新拿到的（只有清单里没有的），按接口给的顺序
    found: HashMap<Provider, Vec<&'static ModelSpec>>,
    /// 同样的内容不再造一份（登记的模型活到 App 退出）
    made: HashMap<(Provider, Found), &'static ModelSpec>,
}

fn registry() -> &'static RwLock<Registry> {
    static R: OnceLock<RwLock<Registry>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn make(p: Provider, f: &Found) -> ModelSpec {
    let efforts: Vec<&'static str> = f.efforts.iter().map(|e| known_effort(e).unwrap_or_else(|| leak(e.clone()))).collect();
    let efforts: &'static [&'static str] = Box::leak(efforts.into_boxed_slice());
    // 价格：同名的自带模型（Vertex 和 Google 同价）有就用它的
    let twin = MODELS.iter().find(|m| m.api_model == f.api_model);
    ModelSpec {
        id: leak(format!("{}/{}", p.id(), f.api_model)),
        provider: p,
        api_model: leak(f.api_model.clone()),
        label: leak(f.label.clone()),
        context: f.context,
        max_output: f.max_output.max(1024),
        thinking: f.thinking,
        off: f.off.clone().map(|o| known_effort(&o).unwrap_or_else(|| leak(o))),
        efforts,
        default_effort: pick_default(efforts),
        price: twin.map(|m| m.price).unwrap_or(super::meter::UNKNOWN),
        priced: twin.is_some(),
        source: f.source,
        main: false,
        strong: false,
        // 拒答回退要模型支持，刷新拿到的不开
        fallback: false,
    }
}

fn known_effort(e: &str) -> Option<&'static str> {
    ["none", "minimal", "low", "medium", "high", "xhigh", "max", "disabled", "between_tools"].into_iter().find(|k| *k == e)
}

/// 没自定义时：medium，没有就 high，再没有就最浅的
fn pick_default(efforts: &'static [&'static str]) -> Option<&'static str> {
    ["medium", "high"].iter().find_map(|w| efforts.iter().find(|e| *e == w).copied()).or_else(|| efforts.first().copied())
}

/// 登记一家刷新拿到的模型（清单里已有的跳过）。同一份内容不重复造
pub fn register(p: Provider, found: &[Found]) {
    let Ok(mut r) = registry().write() else { return };
    let mut list = Vec::new();
    for f in found {
        if MODELS.iter().any(|m| m.provider == p && m.api_model == f.api_model) {
            continue;
        }
        let key = (p, f.clone());
        let spec = match r.made.get(&key) {
            Some(s) => *s,
            None => {
                let s: &'static ModelSpec = Box::leak(Box::new(make(p, f)));
                r.made.insert(key, s);
                s
            }
        };
        if !list.iter().any(|x: &&ModelSpec| x.id == spec.id) {
            list.push(spec);
        }
    }
    r.found.insert(p, list);
}

/// 这一家刷新拿到的（清单里没有的）
pub fn found_of(p: Provider) -> Vec<&'static ModelSpec> {
    registry().read().map(|r| r.found.get(&p).cloned().unwrap_or_default()).unwrap_or_default()
}

/// 一家的全部模型：自带的在前（从强到弱），刷新拿到的在后
pub fn models_of(p: Provider) -> Vec<&'static ModelSpec> {
    MODELS.iter().filter(|m| m.provider == p).chain(found_of(p)).collect()
}

pub fn model(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id).or_else(|| registry().read().ok()?.found.values().flatten().find(|m| m.id == id).copied())
}

/// 按发给供应商的名字找（账本里记的是供应商回报的名字；带日期、`@default` 的去掉再找）
pub fn by_api_name(name: &str) -> Option<&'static ModelSpec> {
    let base = name.split('@').next().unwrap_or(name).trim();
    let find = |n: &str| MODELS.iter().find(|m| m.api_model == n);
    find(base).or_else(|| {
        let undated = base.rsplit_once('-').filter(|(_, d)| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())).map(|(h, _)| h)?;
        find(undated)
    })
}

/// 这一家默认的对话模型
pub fn main_of(p: Provider) -> &'static ModelSpec {
    MODELS.iter().find(|m| m.provider == p && m.main).expect("每家都有默认的模型")
}

/// 这一家最强的（子代理「自动」）
pub fn strongest(p: Provider) -> &'static ModelSpec {
    MODELS.iter().find(|m| m.provider == p && m.strong).expect("每家都有最强的模型")
}

/// 联网搜索时借用的便宜模型（Claude 网页搜索、Gemini 的 Google 搜索、OpenAI 网页搜索）
pub fn helper(p: Provider) -> &'static ModelSpec {
    let api = match p {
        Provider::Anthropic => "claude-haiku-5-5",
        Provider::Openai => "gpt-6.2-sol",
        Provider::Deepseek => "deepseek-flash",
        Provider::Google | Provider::Vertex => "gemini-3.5-flash-lite",
    };
    MODELS.iter().find(|m| m.provider == p && m.api_model == api).expect("清单里的模型")
}

// ---------- 每个模型的思考（设置里定一次，别处照它） ----------

/// 一个模型的思考设置：开不开（能关的才看）、多深
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub on: bool,
    #[serde(default)]
    pub effort: Option<String>,
}

impl ModelSpec {
    /// 没自定义时的设置
    pub fn default_choice(&self) -> Choice {
        Choice { on: self.thinking != Thinking::Never, effort: self.default_effort.map(str::to_string) }
    }

    /// 把一份设置对到这个模型上：认不得的深浅换成默认，关不掉的照开
    pub fn settle(&'static self, c: Option<&Choice>) -> Pick {
        let model = self;
        let on = match self.thinking {
            Thinking::Never => false,
            Thinking::Always => true,
            Thinking::Switch => c.is_none_or(|c| c.on),
        };
        if !on {
            return Pick { model, effort: None };
        }
        let wanted = c.and_then(|c| c.effort.as_deref());
        let effort = wanted.and_then(|w| self.efforts.iter().find(|e| **e == w).copied()).or(self.default_effort);
        Pick { model, effort }
    }
}

/// 这一回用哪个模型、思考多深（None：能关的关掉思考；一直开着的用接口默认）
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    #[serde(serialize_with = "spec_id")]
    pub model: &'static ModelSpec,
    pub effort: Option<&'static str>,
}

fn spec_id<S: serde::Serializer>(m: &&'static ModelSpec, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(m.id)
}

/// 照自带清单的默认设置挑（测试、真实验收用）
#[cfg(test)]
pub fn default_pick(id: &str) -> Pick {
    let m = model(id).unwrap_or_else(|| panic!("清单里没有 {id}"));
    m.settle(None)
}

/// 账本按供应商回报的模型名计价：清单里有的用这里的价；没有的 None（由 `usage::price` 的旧表或最贵的价接手）
pub fn price_of(api_name: &str) -> Option<PriceRow> {
    by_api_name(api_name).map(|m| m.price)
}

/// 发送前的最坏情况：输入留一成余量（按没命中缓存算）+ 输出写满上限
pub fn worst_usd(row: &PriceRow, input_estimate: u64, max_output: u32) -> f64 {
    let input = input_estimate + input_estimate.div_ceil(10);
    let r = row.rate_for(input);
    (input as f64 * r.input.max(r.cache_write) + max_output as f64 * r.output) / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_has_a_main_and_a_strongest_model() {
        for p in Provider::ALL {
            assert_eq!(main_of(p).provider, p);
            assert_eq!(strongest(p).provider, p);
            assert_eq!(helper(p).provider, p);
            for m in models_of(p) {
                let d = m.settle(None);
                if let Some(e) = d.effort {
                    assert!(m.efforts.contains(&e), "{} 不认 {e}", m.id);
                }
                if m.provider == Provider::Anthropic && m.thinking == Thinking::Switch {
                    assert!(m.off.is_some(), "{} 关思考要写法", m.id);
                }
            }
        }
        assert_eq!(main_of(Provider::Anthropic).api_model, "claude-sonnet-5-5");
        assert_eq!(strongest(Provider::Vertex).api_model, "gemini-4-carbon");
        // id 唯一
        let ids: std::collections::HashSet<_> = MODELS.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), MODELS.len());
    }

    #[test]
    fn choices_settle_against_what_the_model_supports() {
        let sonnet = model("anthropic/claude-sonnet-5-5").unwrap();
        assert_eq!(sonnet.settle(None).effort, Some("medium"));
        assert_eq!(sonnet.settle(Some(&Choice { on: true, effort: Some("max".into()) })).effort, Some("max"));
        // 能关的关掉
        assert_eq!(sonnet.settle(Some(&Choice { on: false, effort: Some("max".into()) })).effort, None);
        // 认不得的深浅回到默认
        assert_eq!(sonnet.settle(Some(&Choice { on: true, effort: Some("minimal".into()) })).effort, Some("medium"));
        // 关不掉的照开
        let opus = model("anthropic/claude-opus-5-5").unwrap();
        assert_eq!(opus.settle(Some(&Choice { on: false, effort: Some("low".into()) })).effort, Some("low"));
        let lite = model("google/gemini-3.5-flash-lite").unwrap();
        assert_eq!(lite.settle(Some(&Choice { on: true, effort: Some("minimal".into()) })).effort, Some("minimal"));
    }

    #[test]
    fn found_models_join_the_list_once() {
        let f = Found {
            api_model: "claude-opus-5".into(),
            label: "Claude Opus 5".into(),
            context: 1_000_000,
            max_output: 128_000,
            thinking: Thinking::Switch,
            off: Some("disabled".into()),
            efforts: vec!["low".into(), "high".into()],
            source: Source::Api,
        };
        let known = Found { api_model: "claude-opus-5-5".into(), label: "Claude Opus 5.5".into(), ..f.clone() };
        register(Provider::Anthropic, &[known.clone(), f.clone()]);
        let first = model("anthropic/claude-opus-5").unwrap();
        register(Provider::Anthropic, &[known, f]);
        let again = model("anthropic/claude-opus-5").unwrap();
        assert!(std::ptr::eq(first, again), "同样的内容不再造一份");
        assert_eq!(found_of(Provider::Anthropic).len(), 1, "清单里有的不算刷新拿到的");
        assert!(!first.priced && first.price == super::super::meter::UNKNOWN);
        assert_eq!((first.default_effort, first.off), (Some("high"), Some("disabled")));
        assert_eq!(models_of(Provider::Anthropic).last().unwrap().id, "anthropic/claude-opus-5");
    }

    #[test]
    fn cost_counts_cache_and_long_prompt_tiers() {
        let sonnet = model("anthropic/claude-sonnet-5-5").unwrap().price;
        let t = Tokens { input: 1_000_000, output: 100_000, cache_read: 2_000_000, cache_write: 400_000 };
        // 2 + 1 + 0.2 + 1.0
        assert!((sonnet.usd(&t) - 4.2).abs() < 1e-9);
        let haiku = model("anthropic/claude-haiku-5-5").unwrap().price;
        let small = Tokens { input: 50_000, output: 1_000, ..Default::default() };
        assert!((haiku.usd(&small) - (0.005 + 0.0005)).abs() < 1e-9);
        // 超过 10 万 token 整条换高一档
        let big = Tokens { input: 60_000, cache_read: 60_000, output: 1_000, ..Default::default() };
        assert!((haiku.usd(&big) - (0.03 + 0.003 + 0.0025)).abs() < 1e-9);
        // DeepSeek 按高峰折美元：和 v0.3 账本里的 0.30 / 1.20 一样
        let flash = model("deepseek/deepseek-flash").unwrap().price.base;
        assert_eq!((flash.input, flash.output), (0.30, 1.20));
        // 设置改了单价：缓存价按比例跟着
        let s = sonnet.scaled(4.0, 20.0);
        assert!((s.base.cache_read - 0.2).abs() < 1e-9 && (s.base.cache_write - 5.0).abs() < 1e-9);
    }

    #[test]
    fn names_from_the_ledger_find_their_price() {
        assert_eq!(by_api_name("claude-opus-5-5").unwrap().id, "anthropic/claude-opus-5-5");
        assert_eq!(by_api_name("claude-sonnet-5-5@default").unwrap().id, "anthropic/claude-sonnet-5-5");
        assert_eq!(by_api_name("gemini-3.8-flash-20261101").unwrap().api_model, "gemini-3.8-flash");
        assert!(price_of("something-else").is_none());
        let opus = model("anthropic/claude-opus-5-5").unwrap().price;
        // 最坏情况：10 万 token 输入（按缓存写价，留一成）+ 1.6 万输出
        assert!((worst_usd(&opus, 100_000, 16_000) - (110_000.0 * 5.0 + 16_000.0 * 20.0) / 1e6).abs() < 1e-9);
    }
}
