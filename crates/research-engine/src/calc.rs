//! 计算由代码做（`decisions/0020`「结论」）：模型只说「算什么、用哪几条事实」，这里执行并写出算式。
//! 只用已核实的事实；单位不一致、除数为 0、引用了区间或不存在的事实，都算不了并写明原因。

use crate::dec::Dec;
use crate::model::{Calc, Fact, Op};
use crate::numbers::{Found, Quantity};

pub const MAX_DECIMALS: u32 = 4;

/// 规范单位写给人看
pub fn unit_label(unit: &str) -> &str {
    if unit == "pp" { " 个百分点" } else { unit }
}

fn input<'a>(facts: &'a [Fact], id: &str) -> Result<(&'a Fact, &'a Quantity), String> {
    let fact = facts.iter().find(|f| f.id == id).ok_or_else(|| format!("没有 {id} 这条事实"))?;
    if !fact.verified() {
        return Err(format!("{id} 没有核实，不能用来计算"));
    }
    match &fact.check.parsed {
        Some(Found::One(q)) => Ok((fact, q)),
        Some(Found::Range(..)) => Err(format!("{id} 是一个区间，不能用来计算")),
        None => Err(format!("{id} 的数值读不懂")),
    }
}

/// 执行一个计算：填上 `result`、`shown`、`unit`、`steps`，算不了就填 `error`。
pub fn run(calc: &mut Calc, facts: &[Fact]) {
    calc.decimals = calc.decimals.min(MAX_DECIMALS);
    (calc.result, calc.shown, calc.unit, calc.steps, calc.error) = (None, String::new(), String::new(), String::new(), None);
    match compute(calc, facts) {
        Ok((value, unit, steps)) => {
            calc.shown = format!("{}{}", value.fixed(calc.decimals), unit_label(&unit));
            calc.steps = format!("{steps} = {}", calc.shown);
            calc.result = Some(value);
            calc.unit = unit;
        }
        Err(e) => calc.error = Some(e),
    }
}

fn compute(calc: &Calc, facts: &[Fact]) -> Result<(Dec, String, String), String> {
    let inputs: Vec<(&Fact, &Quantity)> = calc.inputs.iter().map(|id| input(facts, id)).collect::<Result<_, _>>()?;
    let need = |n: usize| if inputs.len() == n { Ok(()) } else { Err(format!("「{}」要正好 {n} 条事实，给了 {} 条", calc.op.label(), inputs.len())) };
    let overflow = || "数太大，算不了".to_string();
    let base = |q: &Quantity| q.base().ok_or_else(overflow);
    let same_unit = || {
        let first = &inputs[0].1.unit;
        match inputs.iter().find(|(_, q)| &q.unit != first) {
            Some((f, q)) => Err(format!("单位不一致：{} 是「{}」，{} 是「{}」", inputs[0].0.id, unit_label(first).trim(), f.id, unit_label(&q.unit).trim())),
            None => Ok(first.clone()),
        }
    };
    let name = |i: usize| format!("{}（{}）", inputs[i].0.shown(), inputs[i].0.id);
    match calc.op {
        Op::Diff => {
            need(2)?;
            let unit = same_unit()?;
            let value = base(inputs[0].1)?.sub(base(inputs[1].1)?).ok_or_else(overflow)?;
            // 两个百分比相减是百分点
            let unit = if unit == "%" { "pp".to_string() } else { unit };
            Ok((value, unit, format!("{} − {}", name(0), name(1))))
        }
        Op::Ratio => {
            need(2)?;
            let value = base(inputs[0].1)?.div(base(inputs[1].1)?).ok_or_else(|| format!("{} 是 0，不能做除数", inputs[1].0.id))?;
            let unit = if inputs[0].1.unit == inputs[1].1.unit { "倍".to_string() } else { String::new() };
            Ok((value, unit, format!("{} ÷ {}", name(0), name(1))))
        }
        Op::PctChange => {
            need(2)?;
            same_unit()?;
            let (new, old) = (base(inputs[0].1)?, base(inputs[1].1)?);
            let value = new.sub(old).and_then(|d| d.mul(Dec::from_int(100))).and_then(|d| d.div(old)).ok_or_else(|| format!("{} 是 0，算不了变化率", inputs[1].0.id))?;
            Ok((value, "%".to_string(), format!("({} − {}) ÷ {} × 100%", name(0), name(1), name(1))))
        }
        Op::Mean | Op::Sum => {
            if inputs.len() < 2 {
                return Err(format!("「{}」至少要 2 条事实", calc.op.label()));
            }
            let unit = same_unit()?;
            let mut total = Dec::ZERO;
            for (_, q) in &inputs {
                total = total.add(base(q)?).ok_or_else(overflow)?;
            }
            let listed = (0..inputs.len()).map(name).collect::<Vec<_>>().join(" + ");
            if calc.op == Op::Sum {
                return Ok((total, unit, listed));
            }
            let value = total.div(Dec::from_int(inputs.len() as i64)).ok_or_else(overflow)?;
            Ok((value, unit, format!("({listed}) ÷ {}", inputs.len())))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{Check, CheckStatus};
    use crate::numbers::parse_value;
    use crate::trust::{ProducerKind, Tier};

    /// 一条已经核实的事实（测试用，不经过正文）
    pub(crate) fn verified(id: &str, value: &str, unit: &str) -> Fact {
        Fact {
            id: id.into(),
            source: "S1".into(),
            sub_question: "Q1".into(),
            metric: "青年失业率".into(),
            value: value.into(),
            unit: unit.into(),
            region: "示例市".into(),
            population: "16–24 岁".into(),
            period: "2024 年".into(),
            definition: String::new(),
            producer: "示例市统计局".into(),
            producer_kind: ProducerKind::CnGovStats,
            relayed: false,
            quote: String::new(),
            unit_quote: String::new(),
            check: Check { status: CheckStatus::Verified, parsed: parse_value(value, unit).ok(), ..Default::default() },
            tier: Tier::Low,
        }
    }

    fn calc(op: Op, inputs: &[&str], decimals: u32, facts: &[Fact]) -> Calc {
        let mut c = Calc { id: "C1".into(), op, inputs: inputs.iter().map(|s| s.to_string()).collect(), label: String::new(), decimals, result: None, shown: String::new(), unit: String::new(), steps: String::new(), error: None };
        run(&mut c, facts);
        c
    }

    #[test]
    fn difference_ratio_change_mean_sum() {
        let facts = [verified("F1", "12.3", "%"), verified("F2", "13.4", "%"), verified("F3", "98,560", "元"), verified("F4", "8.2", "万元"), verified("F5", "10.1", "%")];
        let c = calc(Op::Diff, &["F1", "F2"], 1, &facts);
        assert_eq!((c.shown.as_str(), c.unit.as_str()), ("-1.1 个百分点", "pp"));
        assert_eq!(c.steps, "12.3%（F1） − 13.4%（F2） = -1.1 个百分点");
        // 元和万元换算后再算
        let c = calc(Op::Ratio, &["F3", "F4"], 2, &facts);
        assert_eq!(c.shown, "1.20倍");
        let c = calc(Op::Diff, &["F3", "F4"], 0, &facts);
        assert_eq!(c.shown, "16560元");
        let c = calc(Op::PctChange, &["F1", "F2"], 1, &facts);
        assert_eq!(c.shown, "-8.2%");
        assert!(c.steps.starts_with("(12.3%（F1） − 13.4%（F2）) ÷ 13.4%（F2） × 100%"));
        let c = calc(Op::Mean, &["F1", "F2", "F5"], 2, &facts);
        assert_eq!(c.shown, "11.93%");
        let c = calc(Op::Sum, &["F3", "F4"], 0, &facts);
        assert_eq!(c.shown, "180560元");
        // 0.1 + 0.2 这样的数不出浮点误差
        let exact = [verified("F1", "0.1", ""), verified("F2", "0.2", "")];
        assert_eq!(calc(Op::Sum, &["F1", "F2"], 1, &exact).result.unwrap().to_string(), "0.3");
    }

    #[test]
    fn what_cannot_be_computed_says_why() {
        let mut unverified = verified("F9", "5", "%");
        unverified.check.status = CheckStatus::Unverified;
        let facts = [verified("F1", "12.3", "%"), verified("F2", "98,560", "元"), verified("F3", "0", "%"), verified("F4", "12–15", "%"), unverified];
        let err = |op, inputs: &[&str]| calc(op, inputs, 1, &facts).error.unwrap();
        assert!(err(Op::Diff, &["F1", "F2"]).contains("单位不一致"));
        assert!(err(Op::Diff, &["F1"]).contains("要正好 2 条"));
        assert!(err(Op::Ratio, &["F1", "F3"]).contains("不能做除数"));
        assert!(err(Op::PctChange, &["F1", "F3"]).contains("算不了变化率"));
        assert!(err(Op::Diff, &["F1", "F4"]).contains("区间"));
        assert!(err(Op::Diff, &["F1", "F9"]).contains("没有核实"));
        assert!(err(Op::Diff, &["F1", "F8"]).contains("没有 F8"));
        assert!(err(Op::Mean, &["F1"]).contains("至少要 2 条"));
        let c = calc(Op::Diff, &["F1", "F2"], 9, &facts);
        assert!(c.result.is_none() && c.shown.is_empty() && c.decimals == MAX_DECIMALS);
    }
}
