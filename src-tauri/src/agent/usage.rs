//! 用量账本与月度预算（`decisions/0004`、`0015`、`0018`）。
//!
//! 每次调用模型记一行（`数据目录/agent/usage.jsonl`，只追加）：token 数取接口返回的 `usage`，费用按单价表算，
//! 不由模型自己报。发送前按「估计的输入 + 输出上限」算最坏情况，加上本月已花的超过上限就拒绝调用。
//! 账本不在 library.db 里：恢复旧备份不会把已经花掉的钱抹掉。

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 排版 Agent 用的模型（`decisions/0015`）
pub const MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_BUDGET_USD: f64 = 20.0;
pub const MIN_BUDGET_USD: f64 = 1.0;
pub const MAX_BUDGET_USD: f64 = 50.0;

/// 美元 / 百万 token
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

/// 单价表。按接口说的实际回答的模型计价（现在只会是 `MODEL`）；不认识的模型按最贵的算，宁可多记。
pub fn price(model: &str) -> Price {
    match model {
        "claude-opus-5-5" => Price { input: 4.0, output: 20.0 },
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" => Price { input: 5.0, output: 25.0 },
        _ => Price { input: 10.0, output: 50.0 },
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

pub fn cost_usd(model: &str, t: &Tokens) -> f64 {
    let p = price(model);
    // 缓存写入按输入价的 1.25 倍、读取按 5%（本功能目前不用缓存，接口返回了就照算）
    (t.input as f64 * p.input + t.cache_write as f64 * p.input * 1.25 + t.cache_read as f64 * p.input * 0.05 + t.output as f64 * p.output) / 1e6
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    /// 本地时间 2026-11-10T16:20:31
    pub at: String,
    /// extract 提炼要求 / plan 文档计划
    pub purpose: String,
    pub model: String,
    pub tokens: Tokens,
    pub cost_usd: f64,
    /// 中断或出错、拿不到接口的用量时按估计记（宁多勿少）
    #[serde(default)]
    pub estimated: bool,
    /// 模拟模型：不联网、不花钱，不计入预算
    #[serde(default)]
    pub mock: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
}

pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn new(data_dir: &Path) -> Self {
        Ledger { path: data_dir.join("agent").join("usage.jsonl") }
    }

    pub fn append(&self, call: &Call) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| format!("无法写用量账本 {}：{e}", self.path.display()))?;
        let line = serde_json::to_string(call).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| format!("无法写用量账本：{e}"))
    }

    /// 读不懂的行不丢：返回错误，预算检查据此拒绝调用（账本坏了不能当成没花过钱）
    pub fn all(&self) -> Result<Vec<Call>, String> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("无法读用量账本 {}：{e}", self.path.display())),
        };
        text.lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("用量账本第 {} 行读不懂（{}）：{e}", i + 1, self.path.display())))
            .collect()
    }

    /// 某个月（"2026-11"）真实调用的花费合计
    pub fn month_total(&self, month: &str) -> Result<f64, String> {
        Ok(self.all()?.iter().filter(|c| !c.mock && c.at.starts_with(month)).map(|c| c.cost_usd).sum())
    }
}

/// 发送前估计 token 数（不联网：用接口数 token 就得先把内容发出去）。
/// 系数由真实调用校准（`decisions/0018`）：汉字和全角标点每个约 1.3 个 token，其余字符约 2 个一个 token，取偏高的值。
pub fn estimate_tokens(text: &str) -> u64 {
    let (mut wide, mut narrow) = (0u64, 0u64);
    for c in text.chars() {
        if (c as u32) >= 0x2E80 {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    (wide as f64 * WIDE_TOKENS + narrow as f64 / NARROW_CHARS_PER_TOKEN).ceil() as u64
}

pub const WIDE_TOKENS: f64 = 1.3;
pub const NARROW_CHARS_PER_TOKEN: f64 = 2.0;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub model: String,
    pub input_tokens: u64,
    /// 一般情况：输入 + 通常的输出量
    pub typical_usd: f64,
    /// 最坏情况：输入（留一成余量）+ 输出写满上限
    pub max_usd: f64,
    pub budget_usd: f64,
    pub month_spent_usd: f64,
    /// 最坏情况也不超预算
    pub within_budget: bool,
}

pub fn estimate(input_tokens: u64, typical_output: u64, max_output: u64, budget_usd: f64, month_spent_usd: f64) -> Estimate {
    let p = price(MODEL);
    let typical_usd = (input_tokens as f64 * p.input + typical_output as f64 * p.output) / 1e6;
    let max_usd = (input_tokens as f64 * 1.1 * p.input + max_output as f64 * p.output) / 1e6;
    Estimate { model: MODEL.into(), input_tokens, typical_usd, max_usd, budget_usd, month_spent_usd, within_budget: month_spent_usd + max_usd <= budget_usd }
}

pub fn over_budget_message(e: &Estimate) -> String {
    format!(
        "这次调用最多可能花 ${:.2}，加上本月已花的 ${:.2} 会超过月度上限 ${:.2}，所以没有发送。可以在「设置 › 排版 Agent」里调高上限（最高 ${MAX_BUDGET_USD:.0}），或下个月再用。",
        e.max_usd, e.month_spent_usd, e.budget_usd
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    fn call(at: &str, cost: f64, mock: bool) -> Call {
        Call { at: at.into(), purpose: "extract".into(), model: MODEL.into(), tokens: Tokens::default(), cost_usd: cost, estimated: false, mock, note: None, request_id: None }
    }

    #[test]
    fn prices_and_cost() {
        let t = Tokens { input: 40_000, output: 9_000, cache_read: 0, cache_write: 0 };
        assert!((cost_usd(MODEL, &t) - (0.16 + 0.18)).abs() < 1e-9);
        // 别的模型按它自己的价，不认识的按最贵的
        assert!(cost_usd("claude-opus-5", &t) > cost_usd(MODEL, &t));
        assert_eq!(price("something-new"), Price { input: 10.0, output: 50.0 });
    }

    #[test]
    fn ledger_sums_real_calls_of_the_month() {
        let dir = temp_dir("agent-ledger");
        let l = Ledger::new(&dir);
        assert_eq!(l.month_total("2026-11").unwrap(), 0.0);
        l.append(&call("2026-10-31T23:59:59", 5.0, false)).unwrap();
        l.append(&call("2026-11-01T00:00:01", 0.4, false)).unwrap();
        l.append(&call("2026-11-10T12:00:00", 0.3, false)).unwrap();
        l.append(&call("2026-11-10T12:05:00", 9.0, true)).unwrap();
        assert!((l.month_total("2026-11").unwrap() - 0.7).abs() < 1e-9);
        assert_eq!(l.all().unwrap().len(), 4);
        // 账本坏了：报错，不当成 0
        std::fs::write(dir.join("agent/usage.jsonl"), "{oops\n").unwrap();
        assert!(l.month_total("2026-11").unwrap_err().contains("读不懂"));
    }

    #[test]
    fn budget_uses_the_worst_case() {
        let e = estimate(50_000, 8_000, 32_000, 20.0, 19.0);
        assert!((e.typical_usd - (0.2 + 0.16)).abs() < 1e-9);
        assert!((e.max_usd - (0.22 + 0.64)).abs() < 1e-9);
        assert!(e.within_budget);
        let over = estimate(50_000, 8_000, 32_000, 20.0, 19.2);
        assert!(!over.within_budget);
        assert!(over_budget_message(&over).contains("没有发送"));
    }

    #[test]
    fn token_estimate_leans_high() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("页边距"), 4);
        assert_eq!(estimate_tokens("Times New Roman"), 8);
    }
}
