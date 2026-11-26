//! 模型清单和价格表（`decisions/0036`）。
//!
//! - 每家三档：便宜、均衡、强（问卷：默认均衡，难的交给强一档的子代理）。一档 = 一个模型 + 思考强度。
//! - 价格（美元 / 百万 token）：Anthropic 取官方价目（2026-11）；OpenAI、Google、DeepSeek 取 Peck 2026-11-25 联网查的价格表
//!   （`../personal-cli-studio/peck/src-tauri/defaults/prices.json`）。DeepSeek 是人民币价，按 1 美元 = 6.7 元、一律按高峰（×2）折成美元、
//!   凑整到分（和 v0.3 账本里的 0.30 / 1.20 一样），宁可多记。
//! - 缓存：读、写各有单价；超过 `above` 个 token 的请求整条换一档（Haiku 5.5 的 10 万、OpenAI 的 27.2 万）。
//! - 设置里覆盖的单价（`usage::Prices`）只改输入、输出价，缓存的照这里的比例跟着变。

use serde::Serialize;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Thinking {
    /// 一直开着（effort 只管深浅）
    Always,
    /// 能开能关（这一档 effort 为空就关）
    Switch,
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
    pub efforts: &'static [&'static str],
    pub price: PriceRow,
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

const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const GPT_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
const DEEPSEEK_EFFORTS: &[&str] = &["low", "high", "max"];
const GEMINI_EFFORTS: &[&str] = &["low", "medium", "high"];

pub static MODELS: &[ModelSpec] = &[
    // Anthropic（官方价目；缓存写按 5 分钟的 1.25 倍）
    ModelSpec { id: "anthropic/claude-haiku-5-5", provider: Provider::Anthropic, api_model: "claude-haiku-5-5", label: "Claude Haiku 5.5", context: 1_000_000, max_output: 128_000, thinking: Thinking::Always, efforts: CLAUDE_EFFORTS, price: PriceRow { base: rate(0.10, 0.50, 0.01, 0.125), above: Some((100_000, rate(0.50, 2.50, 0.05, 0.625))) }, fallback: false },
    ModelSpec { id: "anthropic/claude-sonnet-5-5", provider: Provider::Anthropic, api_model: "claude-sonnet-5-5", label: "Claude Sonnet 5.5", context: 1_000_000, max_output: 128_000, thinking: Thinking::Always, efforts: CLAUDE_EFFORTS, price: flat(rate(2.0, 10.0, 0.10, 2.5)), fallback: true },
    ModelSpec { id: "anthropic/claude-opus-5-5", provider: Provider::Anthropic, api_model: "claude-opus-5-5", label: "Claude Opus 5.5", context: 1_000_000, max_output: 128_000, thinking: Thinking::Always, efforts: CLAUDE_EFFORTS, price: flat(rate(4.0, 20.0, 0.20, 5.0)), fallback: true },
    // OpenAI（Responses；超过 27.2 万 token 的请求整条换高一档）
    ModelSpec { id: "openai/gpt-6-luna", provider: Provider::Openai, api_model: "gpt-6-luna", label: "GPT-6 Luna", context: 1_050_000, max_output: 128_000, thinking: Thinking::Switch, efforts: GPT_EFFORTS, price: PriceRow { base: rate(0.10, 0.50, 0.01, 0.125), above: Some((272_000, rate(0.20, 0.75, 0.02, 0.25))) }, fallback: false },
    ModelSpec { id: "openai/gpt-6.2-sol", provider: Provider::Openai, api_model: "gpt-6.2-sol", label: "GPT-6.2 Sol", context: 1_050_000, max_output: 128_000, thinking: Thinking::Always, efforts: GPT_EFFORTS, price: PriceRow { base: rate(2.0, 10.0, 0.10, 2.5), above: Some((272_000, rate(4.0, 15.0, 0.20, 5.0))) }, fallback: false },
    ModelSpec { id: "openai/gpt-6.2-astra", provider: Provider::Openai, api_model: "gpt-6.2-astra", label: "GPT-6.2 Astra", context: 1_050_000, max_output: 128_000, thinking: Thinking::Always, efforts: GPT_EFFORTS, price: PriceRow { base: rate(8.0, 40.0, 0.40, 10.0), above: Some((272_000, rate(16.0, 60.0, 0.80, 20.0))) }, fallback: false },
    // DeepSeek（人民币价折美元，按高峰算）
    ModelSpec { id: "deepseek/deepseek-flash", provider: Provider::Deepseek, api_model: "deepseek-flash", label: "DeepSeek V4.1 Flash", context: 1_048_576, max_output: 393_216, thinking: Thinking::Switch, efforts: DEEPSEEK_EFFORTS, price: flat(rate(0.30, 1.20, 0.006, 0.30)), fallback: false },
    ModelSpec { id: "deepseek/deepseek-v4-pro", provider: Provider::Deepseek, api_model: "deepseek-v4-pro", label: "DeepSeek V4 Pro", context: 1_048_576, max_output: 393_216, thinking: Thinking::Switch, efforts: DEEPSEEK_EFFORTS, price: flat(rate(1.35, 4.05, 0.045, 1.35)), fallback: false },
    // Google Gemini（API Key）
    ModelSpec { id: "google/gemini-3.5-flash-lite", provider: Provider::Google, api_model: "gemini-3.5-flash-lite", label: "Gemini 3.5 Flash Lite", context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(0.30, 2.50, 0.03, 0.30)), fallback: false },
    ModelSpec { id: "google/gemini-3.8-flash", provider: Provider::Google, api_model: "gemini-3.8-flash", label: "Gemini 3.8 Flash", context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(0.75, 3.75, 0.075, 0.75)), fallback: false },
    ModelSpec { id: "google/gemini-4-carbon", provider: Provider::Google, api_model: "gemini-4-carbon", label: "Gemini 4 Carbon", context: 1_048_576, max_output: 32_768, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(2.0, 10.0, 0.10, 2.0)), fallback: false },
    // Vertex AI 上的 Gemini（同价）
    ModelSpec { id: "vertex/gemini-3.5-flash-lite", provider: Provider::Vertex, api_model: "gemini-3.5-flash-lite", label: "Gemini 3.5 Flash Lite", context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(0.30, 2.50, 0.03, 0.30)), fallback: false },
    ModelSpec { id: "vertex/gemini-3.8-flash", provider: Provider::Vertex, api_model: "gemini-3.8-flash", label: "Gemini 3.8 Flash", context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(0.75, 3.75, 0.075, 0.75)), fallback: false },
    ModelSpec { id: "vertex/gemini-4-carbon", provider: Provider::Vertex, api_model: "gemini-4-carbon", label: "Gemini 4 Carbon", context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, efforts: GEMINI_EFFORTS, price: flat(rate(2.0, 10.0, 0.10, 2.0)), fallback: false },
];

pub fn model(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

/// 按发给供应商的名字找（账本里记的是供应商回报的名字；带日期、`@default` 的去掉再找）
pub fn by_api_name(name: &str) -> Option<&'static ModelSpec> {
    let base = name.split('@').next().unwrap_or(name).trim();
    MODELS.iter().find(|m| m.api_model == base).or_else(|| {
        let undated = base.rsplit_once('-').filter(|(_, d)| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())).map(|(h, _)| h)?;
        MODELS.iter().find(|m| m.api_model == undated)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Cheap,
    Balanced,
    Strong,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Cheap, Tier::Balanced, Tier::Strong];

    pub fn label(self) -> &'static str {
        match self {
            Tier::Cheap => "便宜",
            Tier::Balanced => "均衡",
            Tier::Strong => "强",
        }
    }

    /// 高一档（读整章、写长东西交给它）
    pub fn up(self) -> Tier {
        match self {
            Tier::Cheap => Tier::Balanced,
            _ => Tier::Strong,
        }
    }
}

/// 一档：用哪个模型、思考多深（None：能关的关掉思考）
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    #[serde(serialize_with = "spec_id")]
    pub model: &'static ModelSpec,
    pub effort: Option<&'static str>,
    pub tier: Tier,
}

fn spec_id<S: serde::Serializer>(m: &&'static ModelSpec, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(m.id)
}

/// 每家三档（`decisions/0036`）。Opus 5.5 的 effort 默认是 medium，这里都写明
pub fn tier(provider: Provider, tier: Tier) -> Pick {
    let (id, effort) = match (provider, tier) {
        (Provider::Anthropic, Tier::Cheap) => ("anthropic/claude-haiku-5-5", Some("low")),
        (Provider::Anthropic, Tier::Balanced) => ("anthropic/claude-sonnet-5-5", Some("medium")),
        (Provider::Anthropic, Tier::Strong) => ("anthropic/claude-opus-5-5", Some("high")),
        (Provider::Openai, Tier::Cheap) => ("openai/gpt-6-luna", Some("low")),
        (Provider::Openai, Tier::Balanced) => ("openai/gpt-6.2-sol", Some("medium")),
        (Provider::Openai, Tier::Strong) => ("openai/gpt-6.2-astra", Some("high")),
        (Provider::Deepseek, Tier::Cheap) => ("deepseek/deepseek-flash", None),
        (Provider::Deepseek, Tier::Balanced) => ("deepseek/deepseek-flash", Some("high")),
        (Provider::Deepseek, Tier::Strong) => ("deepseek/deepseek-v4-pro", Some("high")),
        (Provider::Google, Tier::Cheap) => ("google/gemini-3.5-flash-lite", Some("low")),
        (Provider::Google, Tier::Balanced) => ("google/gemini-3.8-flash", Some("medium")),
        (Provider::Google, Tier::Strong) => ("google/gemini-4-carbon", Some("high")),
        (Provider::Vertex, Tier::Cheap) => ("vertex/gemini-3.5-flash-lite", Some("low")),
        (Provider::Vertex, Tier::Balanced) => ("vertex/gemini-3.8-flash", Some("medium")),
        (Provider::Vertex, Tier::Strong) => ("vertex/gemini-4-carbon", Some("high")),
    };
    Pick { model: model(id).expect("清单里的模型"), effort, tier }
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
    fn every_provider_has_three_tiers_in_the_catalog() {
        for p in Provider::ALL {
            for t in Tier::ALL {
                let pick = tier(p, t);
                assert_eq!(pick.model.provider, p, "{p:?} {t:?}");
                if let Some(e) = pick.effort {
                    assert!(pick.model.efforts.contains(&e), "{} 不认 {e}", pick.model.id);
                } else {
                    assert_eq!(pick.model.thinking, Thinking::Switch, "一直开着思考的模型不能关");
                }
            }
        }
        // 默认均衡：Anthropic 是 Sonnet 5.5
        assert_eq!(tier(Provider::Anthropic, Tier::Balanced).model.api_model, "claude-sonnet-5-5");
        assert_eq!(Tier::Balanced.up(), Tier::Strong);
        // id 唯一
        let ids: std::collections::HashSet<_> = MODELS.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), MODELS.len());
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
