//! 结论的核对（`decisions/0020`「结论」）：结论里每个数字都必须对得上一条已核实的事实或一个代码算出的值。
//!
//! 模型写结论时用占位符引用数：`{F3}` 由代码填成那条事实的值，`{C1}` 填成算出来的值，`[F3]` 只是出处标记。
//! 模型自己敲出来的数也逐个检查：
//!
//! - 带 %、元、人这类单位的数：必须等于这条结论引用的某条已核实事实，或某个计算结果（按写出来的小数位四舍五入后相等）；
//! - 年份、年龄这类描述用的数：出现在引用的事实的时间、人群、口径里，或者出现在问题里，也算对上；
//! - 其余的标成「对不上」，连同原因留在结论里，界面和报告把它标出来。

use std::collections::BTreeSet;

use crate::dec::Dec;
use crate::model::{Calc, Conclusion, Fact, Segment, Uncertainty};
use crate::numbers::{self, Quantity};
use crate::trust::Tier;

/// 模型给的一条结论（解析后的原样）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub text: String,
    pub facts: Vec<String>,
    pub calcs: Vec<String>,
    pub uncertainty: Vec<String>,
    pub note: String,
}

/// 核对时能参考的上下文
pub struct Context<'a> {
    pub facts: &'a [Fact],
    pub calcs: &'a [Calc],
    /// 问题和子问题的文字：里面出现过的年份、年龄等可以在结论里照写
    pub question_text: &'a str,
}

/// 可以拿来当「描述」的单位：年份、年龄、月份……它们不是统计出来的值
const DESCRIPTOR_UNITS: &[&str] = &["", "年", "月", "日", "季度", "岁", "个月", "周", "天"];

enum Piece {
    Text(String),
    Value { fact: bool, id: String },
    Marker { fact: bool, id: String },
}

/// 把模板切成文字、`{F1}` / `{C1}` 占位符、`[F1]` / `[C1]` 标记
fn pieces(template: &str) -> Vec<Piece> {
    let cs: Vec<char> = template.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < cs.len() {
        let close = match cs[i] {
            '{' => Some('}'),
            '[' => Some(']'),
            _ => None,
        };
        if let Some(close) = close
            && let Some(kind) = cs.get(i + 1).filter(|c| matches!(**c, 'F' | 'C'))
        {
            let digits: String = cs[i + 2..].iter().take_while(|c| c.is_ascii_digit()).collect();
            let end = i + 2 + digits.len();
            if !digits.is_empty() && cs.get(end) == Some(&close) {
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                let (fact, id) = (*kind == 'F', format!("{kind}{digits}"));
                out.push(if close == '}' { Piece::Value { fact, id } } else { Piece::Marker { fact, id } });
                i = end + 1;
                continue;
            }
        }
        text.push(cs[i]);
        i += 1;
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

fn push_text(segments: &mut Vec<Segment>, more: &str) {
    if more.is_empty() {
        return;
    }
    if let Some(Segment::Text { text }) = segments.last_mut() {
        text.push_str(more);
    } else {
        segments.push(Segment::Text { text: more.to_string() });
    }
}

/// 文字里出现的数（不管单位），描述用
fn numbers_in(text: &str, into: &mut Vec<Dec>) {
    for f in numbers::scan(text) {
        into.extend(f.quantities().iter().map(|q| q.number));
    }
}

fn abs(d: Dec) -> Dec {
    if d.is_negative() { Dec::ZERO.sub(d).unwrap_or(d) } else { d }
}

/// 结论里写的数 `q` 是不是事实里的数 `f`
fn same_as_fact(q: &Quantity, f: &Quantity) -> bool {
    match (q.base(), f.base()) {
        (Some(a), Some(b)) => q.unit == f.unit && (a == b || (!q.number.is_negative() && a == abs(b))),
        _ => false,
    }
}

/// 结论里写的数 `q` 是不是计算结果 `c`（按 `q` 写出来的小数位四舍五入；方向词由文字表达，所以不带符号时比绝对值）
fn same_as_calc(q: &Quantity, c: &Calc) -> bool {
    let (Some(result), Some(shown)) = (c.result, q.base()) else { return false };
    let unit_ok = q.unit == c.unit || (c.unit == "倍" && q.unit.is_empty()) || (c.unit.is_empty() && q.unit == "倍");
    // 写成「1.2 万」的按换算后的值比，位数按换算后还剩几位算
    let places = q.number.places().saturating_sub(q.scale);
    let rounded = result.round(places);
    unit_ok && (rounded == shown || (!q.number.is_negative() && abs(rounded) == shown))
}

/// 核对并定稿一条结论。`id` 是 K1、K2……
pub fn finish(id: &str, draft: &Draft, ctx: &Context) -> Conclusion {
    let mut issues: Vec<String> = Vec::new();
    let note = |issues: &mut Vec<String>, s: String| {
        if !issues.contains(&s) {
            issues.push(s);
        }
    };
    let parts = pieces(&draft.text);

    // 这条结论引用了哪些事实、计算：模型列的 + 占位符 + 标记
    let mut fact_ids: Vec<String> = Vec::new();
    let mut calc_ids: Vec<String> = Vec::new();
    let cite = |list: &mut Vec<String>, id: &str| {
        if !list.iter().any(|x| x == id) {
            list.push(id.to_string());
        }
    };
    for id in &draft.facts {
        cite(&mut fact_ids, id.trim());
    }
    for id in &draft.calcs {
        cite(&mut calc_ids, id.trim());
    }
    for p in &parts {
        if let Piece::Value { fact, id } | Piece::Marker { fact, id } = p {
            cite(if *fact { &mut fact_ids } else { &mut calc_ids }, id);
        }
    }
    // 只留存在并且核实过的
    calc_ids.retain(|id| match ctx.calcs.iter().find(|c| &c.id == id) {
        Some(c) if c.result.is_some() => true,
        Some(c) => {
            note(&mut issues, format!("{id} 算不了：{}", c.error.clone().unwrap_or_default()));
            false
        }
        None => {
            note(&mut issues, format!("引用了不存在的计算 {id}"));
            false
        }
    });
    // 计算用到的事实也算引用了
    for id in &calc_ids {
        if let Some(c) = ctx.calcs.iter().find(|c| &c.id == id) {
            for f in &c.inputs {
                cite(&mut fact_ids, f);
            }
        }
    }
    fact_ids.retain(|id| match ctx.facts.iter().find(|f| &f.id == id) {
        Some(f) if f.verified() => true,
        Some(_) => {
            note(&mut issues, format!("引用了没有核实的事实 {id}"));
            false
        }
        None => {
            note(&mut issues, format!("引用了不存在的事实 {id}"));
            false
        }
    });
    let cited_facts: Vec<&Fact> = fact_ids.iter().filter_map(|id| ctx.facts.iter().find(|f| &f.id == id)).collect();
    let cited_calcs: Vec<&Calc> = calc_ids.iter().filter_map(|id| ctx.calcs.iter().find(|c| &c.id == id)).collect();

    // 描述用的数：引用的事实的时间、人群、地区、指标、口径、生产者，和问题里的
    let mut descriptors: Vec<Dec> = Vec::new();
    numbers_in(ctx.question_text, &mut descriptors);
    for f in &cited_facts {
        for field in [&f.period, &f.population, &f.region, &f.metric, &f.definition, &f.producer] {
            numbers_in(field, &mut descriptors);
        }
    }

    let match_fact = |q: &Quantity| cited_facts.iter().find(|f| f.check.parsed.as_ref().is_some_and(|p| p.quantities().iter().any(|fq| same_as_fact(q, fq))));
    let match_calc = |q: &Quantity| cited_calcs.iter().find(|c| same_as_calc(q, c));

    let mut segments: Vec<Segment> = Vec::new();
    for p in parts {
        match p {
            Piece::Marker { fact, id } => {
                let known = if fact { fact_ids.contains(&id) } else { calc_ids.contains(&id) };
                if known {
                    push_text(&mut segments, &format!("[{id}]"));
                }
            }
            Piece::Value { fact: true, id } => match cited_facts.iter().find(|f| f.id == id) {
                Some(f) => segments.push(Segment::Fact { id, text: f.shown() }),
                None => segments.push(Segment::Unmatched { text: format!("{{{id}}}"), why: "这条事实不存在或没有核实".into() }),
            },
            Piece::Value { fact: false, id } => match cited_calcs.iter().find(|c| c.id == id) {
                Some(c) => segments.push(Segment::Calc { id, text: c.shown.clone() }),
                None => segments.push(Segment::Unmatched { text: format!("{{{id}}}"), why: "这个计算不存在或算不了".into() }),
            },
            Piece::Text(text) => {
                let cs: Vec<char> = text.chars().collect();
                let mut at = 0;
                for found in numbers::scan(&text) {
                    let span = found.span();
                    let raw: String = cs[span.start..span.end].iter().collect();
                    let qs = found.quantities();
                    // 整个（单个数，或区间的两端）对上同一条事实 / 同一个计算
                    let by_fact = match_fact(qs[0]).filter(|f| qs.iter().all(|q| f.check.parsed.as_ref().is_some_and(|p| p.quantities().iter().any(|fq| same_as_fact(q, fq)))));
                    let by_calc = if qs.len() == 1 { match_calc(qs[0]) } else { None };
                    let descriptor = qs.iter().all(|q| DESCRIPTOR_UNITS.contains(&q.unit.as_str()) && q.scale == 0 && descriptors.contains(&q.number));
                    push_text(&mut segments, &cs[at..span.start].iter().collect::<String>());
                    at = span.end;
                    if let Some(f) = by_fact {
                        segments.push(Segment::Fact { id: f.id.clone(), text: raw });
                    } else if let Some(c) = by_calc {
                        segments.push(Segment::Calc { id: c.id.clone(), text: raw });
                    } else if descriptor {
                        push_text(&mut segments, &raw);
                    } else {
                        note(&mut issues, format!("「{}」对不上这条结论引用的任何事实或计算", raw.trim()));
                        segments.push(Segment::Unmatched { text: raw, why: "对不上引用的任何事实或计算".into() });
                    }
                }
                push_text(&mut segments, &cs[at..].iter().collect::<String>());
                for word in numbers::chinese_quantities(&text) {
                    note(&mut issues, format!("「{word}」这样的写法核对不了，数量要写成阿拉伯数字"));
                }
            }
        }
    }
    for s in &segments {
        if let Segment::Unmatched { text, why } = s
            && text.starts_with('{')
        {
            note(&mut issues, format!("{text}：{why}"));
        }
    }

    // 不确定的类型：模型标的 + 代码按等级表判断的「只有低可信来源」
    let mut uncertainty: BTreeSet<Uncertainty> = BTreeSet::new();
    for key in &draft.uncertainty {
        match Uncertainty::ALL.into_iter().find(|u| u.key() == key.trim()) {
            // 这一项只由代码判断
            Some(Uncertainty::LowTrustOnly) => {}
            Some(u) => {
                uncertainty.insert(u);
            }
            None if key.trim().is_empty() || key.trim() == "none" => {}
            None => note(&mut issues, format!("不认识的不确定类型「{}」", key.trim())),
        }
    }
    if !cited_facts.is_empty() && cited_facts.iter().all(|f| f.tier == Tier::Low) {
        uncertainty.insert(Uncertainty::LowTrustOnly);
    }
    if cited_facts.is_empty() && !uncertainty.contains(&Uncertainty::NotFound) {
        note(&mut issues, "这条结论没有引用任何已核实的事实".to_string());
    }
    // 引用的事实按可信度从高到低排（同级按编号）
    let mut ordered: Vec<&Fact> = cited_facts.clone();
    ordered.sort_by(|a, b| b.tier.cmp(&a.tier).then_with(|| fact_number(&a.id).cmp(&fact_number(&b.id))));

    Conclusion {
        id: id.to_string(),
        template: draft.text.clone(),
        segments,
        facts: ordered.iter().map(|f| f.id.clone()).collect(),
        calcs: calc_ids,
        uncertainty: uncertainty.into_iter().collect(),
        note: draft.note.trim().to_string(),
        issues,
    }
}

/// F12 → 12（排序用）
pub fn fact_number(id: &str) -> u32 {
    id.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().unwrap_or(u32::MAX)
}

/// 打回重写时给模型看的问题清单；没有问题返回 `None`
pub fn feedback(conclusions: &[Conclusion]) -> Option<String> {
    let lines: Vec<String> = conclusions.iter().filter(|c| !c.issues.is_empty()).map(|c| format!("第 {} 条结论：{}", c.id.trim_start_matches('K'), c.issues.join("；"))).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::{self, tests::verified};
    use crate::model::{CheckStatus, Op};

    fn world() -> (Vec<Fact>, Vec<Calc>) {
        let mut f1 = verified("F1", "12.3", "%");
        f1.period = "2024 年".into();
        let mut f2 = verified("F2", "13.4", "%");
        f2.period = "2023 年".into();
        let mut f3 = verified("F3", "15.8", "%");
        f3.producer = "示例大学劳动经济研究所".into();
        f3.tier = Tier::High;
        let mut f4 = verified("F4", "9.9", "%");
        f4.check.status = CheckStatus::Unverified;
        let mut f5 = verified("F5", "1.2", "万人");
        f5.metric = "新增就业".into();
        let facts = vec![f1, f2, f3, f4, f5];
        let mut c1 = Calc { id: "C1".into(), op: Op::Diff, inputs: vec!["F1".into(), "F2".into()], label: "比上年".into(), decimals: 1, result: None, shown: String::new(), unit: String::new(), steps: String::new(), error: None };
        calc::run(&mut c1, &facts);
        let mut c2 = Calc { id: "C2".into(), op: Op::Diff, inputs: vec!["F1".into(), "F9".into()], ..c1.clone() };
        calc::run(&mut c2, &facts);
        (facts, vec![c1, c2])
    }

    fn check(text: &str, facts: &[&str], calcs: &[&str], uncertainty: &[&str]) -> Conclusion {
        let (f, c) = world();
        let draft = Draft { text: text.into(), facts: facts.iter().map(|s| s.to_string()).collect(), calcs: calcs.iter().map(|s| s.to_string()).collect(), uncertainty: uncertainty.iter().map(|s| s.to_string()).collect(), note: String::new() };
        finish("K1", &draft, &Context { facts: &f, calcs: &c, question_text: "示例市 2022–2026 年 16–24 岁青年的就业情况" })
    }

    #[test]
    fn placeholders_are_filled_by_code() {
        let k = check("示例市 2024 年青年失业率为 {F1}，比 2023 年低 {C1}。[F2]", &[], &[], &[]);
        assert_eq!(k.text(), "示例市 2024 年青年失业率为 12.3%，比 2023 年低 -1.1 个百分点。[F2]");
        assert!(k.issues.is_empty(), "{:?}", k.issues);
        assert_eq!(k.facts, ["F1", "F2"]);
        assert_eq!(k.calcs, ["C1"]);
        assert!(k.segments.contains(&Segment::Fact { id: "F1".into(), text: "12.3%".into() }));
        assert!(k.segments.contains(&Segment::Calc { id: "C1".into(), text: "-1.1 个百分点".into() }));
        // 全是低可信来源：代码标出来
        assert_eq!(k.uncertainty, [Uncertainty::LowTrustOnly]);
    }

    #[test]
    fn typed_numbers_are_matched_one_by_one() {
        let k = check("2024 年示例市 16–24 岁青年失业率为 12.3%，比上年下降 1.1 个百分点；独立估计为 15.8%。", &["F1", "F3"], &["C1"], &["conflicting"]);
        assert!(k.issues.is_empty(), "{:?}", k.issues);
        let linked: Vec<(&str, &str)> = k.segments.iter().filter_map(|s| match s {
            Segment::Fact { id, text } | Segment::Calc { id, text } => Some((id.as_str(), text.as_str())),
            _ => None,
        }).collect();
        assert_eq!(linked, [("F1", "12.3%"), ("C1", "1.1 个百分点"), ("F3", "15.8%")]);
        // 引用的事实按可信度从高到低
        assert_eq!(k.facts, ["F3", "F1", "F2"]);
        assert_eq!(k.uncertainty, [Uncertainty::Conflicting]);
        // 换一种量级写法也认：1.2 万人 = 12000 人
        let k = check("全年新增就业 12,000 人。", &["F5"], &[], &[]);
        assert!(k.issues.is_empty(), "{:?}", k.issues);
    }

    #[test]
    fn an_extra_number_that_matches_nothing_is_flagged() {
        let k = check("示例市 2024 年青年失业率为 12.3%，远低于全国的 17.6%。", &["F1"], &[], &[]);
        assert_eq!(k.issues, ["「17.6%」对不上这条结论引用的任何事实或计算"]);
        assert!(k.segments.contains(&Segment::Unmatched { text: "17.6%".into(), why: "对不上引用的任何事实或计算".into() }));
        // 数对、单位不对
        assert_eq!(check("失业率为 12.3‰。", &["F1"], &[], &[]).issues.len(), 1);
        // 数是别的事实的，但这条结论没引用它
        assert_eq!(check("失业率为 15.8%。", &["F1"], &[], &[]).issues.len(), 1);
        // 没引用事实的年份：问题里有的可以写，没有的不行
        assert!(check("2024 年失业率为 {F1}。", &[], &[], &[]).issues.is_empty());
        assert_eq!(check("2019 年失业率为 {F1}。", &[], &[], &[]).issues, ["「2019 年」对不上这条结论引用的任何事实或计算"]);
        // 中文数量词核对不了
        assert!(check("失业率为 {F1}，约三成青年在找工作。", &[], &[], &[]).issues[0].contains("三成"));
    }

    #[test]
    fn citing_missing_or_unverified_evidence_is_flagged() {
        let k = check("失业率为 {F4}，另见 {F9} 和 {C2}、{C7}。", &["F1"], &[], &[]);
        assert!(k.issues.iter().any(|i| i == "引用了没有核实的事实 F4"));
        assert!(k.issues.iter().any(|i| i == "引用了不存在的事实 F9"));
        assert!(k.issues.iter().any(|i| i.starts_with("C2 算不了")));
        assert!(k.issues.iter().any(|i| i == "引用了不存在的计算 C7"));
        assert_eq!(k.facts, ["F1"]);
        assert!(k.calcs.is_empty());
        assert_eq!(k.segments.iter().filter(|s| matches!(s, Segment::Unmatched { .. })).count(), 4);
        // 什么都没引用
        let k = check("总体上就业压力较大。", &[], &[], &[]);
        assert_eq!(k.issues, ["这条结论没有引用任何已核实的事实"]);
        // 「没找到」的结论可以不引用
        assert!(check("没有找到示例市灵活就业人数的可靠数据。", &[], &[], &["not_found"]).issues.is_empty());
        assert!(check("{F1}", &[], &[], &["made_up"]).issues[0].contains("不认识的不确定类型"));
    }

    #[test]
    fn feedback_lists_the_problems_per_conclusion() {
        let good = check("失业率为 {F1}。", &[], &[], &[]);
        let bad = Conclusion { id: "K2".into(), ..check("失业率为 17.6%。", &["F1"], &[], &[]) };
        assert_eq!(feedback(std::slice::from_ref(&good)), None);
        assert_eq!(feedback(&[good, bad]).unwrap(), "第 2 条结论：「17.6%」对不上这条结论引用的任何事实或计算");
    }
}
