//! 审计：对一份存下来的调查从头再核一遍，不相信里面已经写着的「已核实」。
//!
//! 重做的事：每条事实的摘录、数值、单位对着保存的正文再核；可信度按等级表再定；每个计算再算；
//! 每条结论按它的模板和引用再定稿，和存着的比。任何一处对不上都列出来。
//! 用途：导出报告前的自检、数据被手改或恢复后的复查，以及「改坏测试」（把结果逐项改坏，这里都要查得出）。

use serde::Serialize;

use crate::conclude::{self, Context, Draft};
use crate::flow;
use crate::model::{Run, Source, Uncertainty};
use crate::{calc, report};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 摘录在保存的正文里找不到
    Quote,
    /// 数值不在摘录里、读不懂
    Value,
    /// 单位对不上
    Unit,
    /// 读不到保存的正文
    Text,
    /// 可信度和等级表不一致
    Tier,
    /// 计算结果和重算的不一样
    Calc,
    /// 结论和重新核对的结果不一样（多了对不上的数、引用变了……）
    Conclusion,
    /// 没有按可信度排序
    Order,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub kind: Kind,
    /// 哪一条：F3、C1、K2、S4
    pub target: String,
    pub message: String,
}

fn kind_of(reason: &str) -> Kind {
    if reason.contains("读不到保存的正文") {
        Kind::Text
    } else if reason.contains("单位") {
        Kind::Unit
    } else if reason.contains("摘录") && !reason.contains("数值不在摘录里") {
        Kind::Quote
    } else {
        Kind::Value
    }
}

/// `text_of` 给出某个来源保存下来的正文。没有问题时返回空表。
pub fn audit(run: &Run, text_of: &mut dyn FnMut(&Source) -> Result<String, String>) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut fresh = run.clone();
    // 不用内存里带着的正文，一律重新读保存的那份
    for s in &mut fresh.sources {
        s.text = None;
    }
    flow::verify_all(&mut fresh, text_of);

    for (stored, now) in run.facts.iter().zip(&fresh.facts) {
        if stored.verified() && !now.verified() {
            let reason = now.check.reason.clone().unwrap_or_default();
            out.push(Finding { kind: kind_of(&reason), target: stored.id.clone(), message: format!("{} 标着「已核实」，重新核对没过：{reason}", stored.id) });
        } else if stored.verified() && stored.check.parsed != now.check.parsed {
            out.push(Finding { kind: Kind::Value, target: stored.id.clone(), message: format!("{} 存着的数值解析结果和重新解析的不一样", stored.id) });
        }
        if stored.tier != now.tier {
            out.push(Finding { kind: Kind::Tier, target: stored.id.clone(), message: format!("{} 的可信度写的是「{}」，按等级表应当是「{}」", stored.id, stored.tier.label(), now.tier.label()) });
        }
    }
    for (stored, now) in run.sources.iter().zip(&fresh.sources) {
        if stored.tier != now.tier {
            out.push(Finding { kind: Kind::Tier, target: stored.id.clone(), message: format!("{} 的可信度和它的已核实事实对不上", stored.id) });
        }
    }

    // 计算：用重新核对过的事实再算
    for c in &mut fresh.calcs {
        calc::run(c, &fresh.facts);
    }
    for (stored, now) in run.calcs.iter().zip(&fresh.calcs) {
        if stored.result != now.result || stored.shown != now.shown || stored.unit != now.unit {
            let now_shown = now.error.clone().map(|e| format!("算不了（{e}）")).unwrap_or_else(|| now.shown.clone());
            out.push(Finding { kind: Kind::Calc, target: stored.id.clone(), message: format!("{} 存着的结果是「{}」，重算是「{now_shown}」", stored.id, stored.shown) });
        }
    }

    // 结论：按模板和引用重新定稿
    let mut question_text = run.question.clone();
    for s in run.plan.sub_questions.iter().filter(|s| s.enabled) {
        for part in [&s.text, &s.metric, &s.definition, &s.population, &s.region, &s.period] {
            question_text.push('\n');
            question_text.push_str(part);
        }
    }
    let ctx = Context { facts: &fresh.facts, calcs: &fresh.calcs, question_text: &question_text };
    for k in &run.conclusions {
        let draft = Draft {
            text: k.template.clone(),
            facts: k.facts.clone(),
            calcs: k.calcs.clone(),
            uncertainty: k.uncertainty.iter().filter(|u| **u != Uncertainty::LowTrustOnly).map(|u| u.key().to_string()).collect(),
            note: k.note.clone(),
        };
        let now = conclude::finish(&k.id, &draft, &ctx);
        if now.segments != k.segments {
            let extra = now.issues.iter().find(|i| !k.issues.contains(i)).cloned().unwrap_or_else(|| "显示的文字和按模板核对出来的不一样".into());
            out.push(Finding { kind: Kind::Conclusion, target: k.id.clone(), message: format!("结论 {}：{extra}", k.id) });
        } else if now.issues != k.issues || now.facts != k.facts || now.calcs != k.calcs || now.uncertainty != k.uncertainty {
            let extra = now.issues.iter().find(|i| !k.issues.contains(i)).cloned().unwrap_or_else(|| "引用的事实、计算或不确定类型和重新核对的不一样".into());
            out.push(Finding { kind: Kind::Conclusion, target: k.id.clone(), message: format!("结论 {}：{extra}", k.id) });
        }
    }
    out
}

/// 报告里的顺序（来源、事实、结论引用的事实按可信度从高到低）
pub fn audit_order(r: &report::Report) -> Vec<Finding> {
    report::order_problems(r).into_iter().map(|message| Finding { kind: Kind::Order, target: String::new(), message }).collect()
}
