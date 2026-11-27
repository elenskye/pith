//! 记账和月上限：每次调模型之前按最坏情况（输入估计留一成 + 输出写满）查一次，加上本月已花的超过月上限就不发；
//! 收完按接口回报的用量 × 价格表（`catalog`，带缓存读写价）记进账本 `agent/usage.jsonl`——和排版 Agent、事实调查、概念研究
//! 共用一本账、一个月上限（`decisions/0030`：只看月上限，不逐次确认）。中途断了、停了按估计记（宁多勿少）。

use std::sync::atomic::AtomicBool;

use super::catalog::{self, PriceRow, Rate};
use super::{Delta, ErrorKind, GateError, Model, Reply, Request, Tokens};
use crate::agent::usage::{self, Call, Ledger, Prices};

pub struct Meter {
    pub ledger: Ledger,
    pub budget_usd: f64,
    pub prices: Prices,
    pub now: fn() -> String,
    /// 用免费额度的几家（设置里标的，Google、Vertex）：账本记 0、不占月上限，备注里写按价目约多少
    pub free: Vec<super::Provider>,
}

/// 这一次调用是干什么的（账本里的 purpose）、哪段对话
#[derive(Clone, Copy, Debug)]
pub struct Use<'a> {
    pub purpose: &'a str,
    pub conversation: Option<&'a str>,
}

pub const UNKNOWN: PriceRow = PriceRow { base: Rate { input: usage::UNKNOWN_PRICE.input, output: usage::UNKNOWN_PRICE.output, cache_read: usage::UNKNOWN_PRICE.input, cache_write: usage::UNKNOWN_PRICE.input * 1.25 }, above: None };

impl Meter {
    pub fn new(data_dir: &std::path::Path, budget_usd: f64, prices: Prices) -> Meter {
        Meter { ledger: Ledger::new(data_dir), budget_usd, prices, now: crate::text::now_iso, free: Vec::new() }
    }

    /// 一个模型（供应商回报的名字）的价：清单里的，设置里改过输入、输出价的按比例换；都没有的按最贵的
    pub fn row(&self, api_model: &str) -> PriceRow {
        let base = catalog::price_of(api_model).unwrap_or(UNKNOWN);
        match self.prices.models.get(api_model) {
            Some(p) => base.scaled(p.input, p.output),
            None => base,
        }
    }

    pub fn month(&self) -> String {
        (self.now)().get(..7).unwrap_or("").to_string()
    }

    pub fn month_spent(&self) -> Result<f64, String> {
        self.ledger.month_total(&self.month())
    }

    pub fn check(&self, worst_usd: f64) -> Result<(), GateError> {
        let spent = self.month_spent().map_err(|e| GateError::new(ErrorKind::Budget, format!("{e}。为了不超支，现在不发请求。")))?;
        if spent + worst_usd > self.budget_usd {
            return Err(GateError::new(
                ErrorKind::Budget,
                format!("这一步最多可能花 ${worst_usd:.2}，加上本月已花的 ${spent:.2} 会超过月上限 ${:.2}，所以没有发送。可以在「设置 › 模型与费用」调高上限。", self.budget_usd),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn record(&self, model: &dyn Model, u: &Use, reported: &str, tokens: Tokens, cost: f64, estimated: bool, note: Option<String>, request_id: Option<String>) -> Result<(), GateError> {
        let call = Call {
            at: (self.now)(),
            purpose: u.purpose.into(),
            provider: model.spec().provider.id().into(),
            kind: usage::KIND_MODEL.into(),
            searches: 0,
            model: reported.into(),
            tokens,
            cost_usd: if model.is_mock() { 0.0 } else { cost },
            estimated,
            mock: model.is_mock(),
            note,
            request_id,
            conversation: u.conversation.map(str::to_string),
        };
        self.ledger.append(&call).map_err(|e| GateError::new(ErrorKind::Budget, e))
    }
}

/// 发一次：查上限 → 发 → 记账。返回回答和这一次花的美元数（模拟的是 0）
pub fn send(model: &dyn Model, meter: &Meter, u: &Use, req: &Request, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<(Reply, f64), GateError> {
    let spec = model.spec();
    let free = meter.free.contains(&spec.provider);
    if !model.is_mock() && !free {
        meter.check(catalog::worst_usd(&meter.row(spec.api_model), req.input_estimate(), req.max_output_tokens))?;
    }
    // 免费额度：记 0，备注里留按价目的数
    let charge = |cost: f64, note: Option<String>| -> (f64, Option<String>) {
        if free {
            let list = format!("免费额度（按价目约 ${cost:.4}）");
            (0.0, Some(note.map_or(list.clone(), |n| format!("{n}；{list}"))))
        } else {
            (cost, note)
        }
    };
    match model.send(req, on, cancel) {
        Ok(reply) => {
            let note = match &reply.stop {
                super::Stop::Refusal => Some("模型拒答".to_string()),
                super::Stop::MaxTokens => Some("写到上限被截断".to_string()),
                _ => (reply.model != spec.api_model).then(|| format!("由 {} 回答", reply.model)),
            };
            let (cost, note) = charge(meter.row(&reply.model).usd(&reply.tokens), note);
            meter.record(model, u, &reply.model, reply.tokens, cost, false, note, reply.request_id.clone())?;
            Ok((reply, if model.is_mock() { 0.0 } else { cost }))
        }
        Err(e) => {
            if let Some(t) = e.spent {
                let note = if e.kind == ErrorKind::Cancelled { "中途停止，按估计记" } else { "中途出错，按估计记" };
                let (cost, note) = charge(meter.row(spec.api_model).usd(&t), Some(note.into()));
                meter.record(model, u, spec.api_model, t, cost, true, note, None)?;
            }
            Err(e)
        }
    }
}
