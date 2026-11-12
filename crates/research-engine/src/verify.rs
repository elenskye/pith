//! 事实核对（代码做，不靠模型，`decisions/0020`「核对」）：
//!
//! 1. 摘录必须在保存的正文里逐字找得到，只容忍空白和全半角差异；
//! 2. 数值必须出现在摘录里（同一个数：千分位、万 / 亿换算后相等）；
//! 3. 单位必须对得上：紧跟在摘录里那个数后面，或者（表格）在另一处写明单位的原文里。
//!
//! 核不上的标「未核实」并写明原因，不进结论。

use crate::fold::{Folded, fold};
use crate::model::{Check, CheckStatus, Fact};
use crate::numbers::{self, Found, Quantity};

/// 摘录折叠后至少这么长：太短的（比如只有一个数）在正文里到处都是，核对没有意义
pub const MIN_QUOTE_CHARS: usize = 6;

fn unverified(reason: impl Into<String>, parsed: Option<Found>) -> Check {
    Check { status: CheckStatus::Unverified, reason: Some(reason.into()), span: None, occurrences: 0, parsed }
}

/// 核对一条事实。`text` 是它的来源保存下来的正文。
pub fn check_fact(fact: &Fact, text: &Folded) -> Check {
    let parsed = match numbers::parse_value(&fact.value, &fact.unit) {
        Ok(p) => p,
        Err(e) => return unverified(format!("数值读不懂：{e}"), None),
    };
    let quote = fold(&fact.quote);
    if quote.chars().count() < MIN_QUOTE_CHARS {
        return unverified("摘录太短，核对不了", Some(parsed));
    }
    let Some((span, occurrences)) = text.find(&quote) else {
        return unverified("摘录在保存的正文里找不到", Some(parsed));
    };
    // 单位不在摘录里时要另给一处原文，这处原文也得找得到
    let unit_quote = fold(&fact.unit_quote);
    if !unit_quote.is_empty() && text.find(&unit_quote).is_none() {
        return unverified("写明单位的那处原文在正文里找不到", Some(parsed));
    }
    if let Err(reason) = value_in_quote(&parsed, &fact.quote, &fact.unit, &fact.unit_quote) {
        return unverified(reason, Some(parsed));
    }
    Check { status: CheckStatus::Verified, reason: None, span: Some(span), occurrences, parsed: Some(parsed) }
}

/// 摘录里有没有这个数（连同单位）。`unit_text` 是模型给的单位原样，`unit_quote` 是另一处写明单位的原文。
pub fn value_in_quote(value: &Found, quote: &str, unit_text: &str, unit_quote: &str) -> Result<(), String> {
    let in_quote = numbers::scan(quote);
    let quote_chars: Vec<char> = quote.chars().collect();
    let wanted = value.quantities();
    // 区间的两端要在摘录的同一个区间里；单个数可以是摘录里任何一个数（含区间的一端）
    let candidates: Vec<Vec<&Quantity>> = match value {
        Found::One(_) => in_quote.iter().flat_map(|f| f.quantities()).map(|q| vec![q]).collect(),
        Found::Range(..) => in_quote.iter().filter(|f| matches!(f, Found::Range(..))).map(|f| f.quantities()).collect(),
    };
    let mut near_miss: Option<String> = None;
    for cand in &candidates {
        let mut all = true;
        for (w, q) in wanted.iter().zip(cand) {
            match same_quantity(w, q, &quote_chars, unit_text, unit_quote) {
                Ok(()) => {}
                Err(why) => {
                    all = false;
                    if let Some(why) = why {
                        near_miss.get_or_insert(why);
                    }
                    break;
                }
            }
        }
        if all {
            return Ok(());
        }
    }
    Err(near_miss.unwrap_or_else(|| "数值不在摘录里".to_string()))
}

/// `want`（事实的数）和 `seen`（摘录里的数）是不是同一个。
/// `Err(Some(原因))`：数对上了但单位不对；`Err(None)`：不是同一个数。
fn same_quantity(want: &Quantity, seen: &Quantity, quote: &[char], unit_text: &str, unit_quote: &str) -> Result<(), Option<String>> {
    let (Some(want_base), Some(seen_base)) = (want.base(), seen.base()) else { return Err(None) };
    if !seen.unit.is_empty() {
        // 摘录里这个数自己带着单位：换算后的值和单位都要相同
        if want_base != seen_base {
            // 值写出来一样、只是量级或单位不同：是单位错
            return if want.number == seen.number { Err(Some(unit_mismatch(want, seen))) } else { Err(None) };
        }
        return if want.unit == seen.unit { Ok(()) } else { Err(Some(unit_mismatch(want, seen))) };
    }
    // 摘录里是个光秃秃的数（或者只带量级）
    if want.unit.is_empty() {
        return if want_base == seen_base { Ok(()) } else { Err(None) };
    }
    // 事实有单位：先看这个数后面是不是紧跟着模型写的单位（扫描器不认识的单位，比如「岗位」）
    let unit_folded = fold(unit_text);
    let after: String = quote[seen.span.end.min(quote.len())..].iter().filter(|c| !c.is_whitespace()).take(unit_folded.chars().count()).map(|c| crate::fold::half_width(*c)).collect();
    if !unit_folded.is_empty() && after == unit_folded && want.number == seen.number {
        return Ok(());
    }
    // 再看另一处写明单位的原文（表头「单位：万人」）：数按写出来的比，量级由单位带
    let written_same = if seen.scale == 0 { want.number == seen.number } else { want_base == seen_base };
    if !written_same {
        return Err(None);
    }
    if !unit_quote.trim().is_empty() && unit_evidence_matches(want, unit_text, unit_quote) {
        return Ok(());
    }
    Err(Some(format!("摘录里的 {} 后面没有单位，也没有给出写明单位「{}」的原文", seen.number, unit_text.trim())))
}

fn unit_mismatch(want: &Quantity, seen: &Quantity) -> String {
    format!("单位对不上：摘录里是「{}」，事实写的是「{}」", show_unit(seen), show_unit(want))
}

fn show_unit(q: &Quantity) -> String {
    let scale = match q.scale {
        0 => "",
        3 => "千",
        4 => "万",
        6 => "百万",
        7 => "千万",
        8 => "亿",
        11 => "千亿",
        12 => "万亿",
        _ => "?",
    };
    let unit = if q.unit == "pp" { "个百分点" } else { q.unit.as_str() };
    let s = format!("{scale}{unit}");
    if s.is_empty() { "没有单位".into() } else { s }
}

/// 另一处原文里有没有这个单位：原样出现，或者扫描出同样的量级和规范单位
fn unit_evidence_matches(want: &Quantity, unit_text: &str, unit_quote: &str) -> bool {
    let (evidence, unit) = (fold(unit_quote), fold(unit_text));
    if !unit.is_empty() && evidence.contains(&unit) {
        return true;
    }
    // 「单位：%」和模型写的「％」：折叠后相同，上面已经覆盖；这里处理「百分点」和 pp 这类规范后才相同的
    let cs: Vec<char> = evidence.chars().collect();
    (0..cs.len()).any(|i| {
        let tail: String = cs[i..].iter().collect();
        let (scale, canon) = numbers::parse_unit(&tail);
        !canon.is_empty() && canon == want.unit && scale == want.scale && tail.starts_with(|c: char| !c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Fact;
    use crate::trust::{ProducerKind, Tier};

    const TEXT: &str = "示例市统计局 2025 年 2 月发布\n\n2024 年，示例市 16–24 岁城镇青年调查失业率为 12.3％，比上年下降 1.1 个百分点。\n全市城镇非私营单位就业人员年平均工资为 98,560 元。\n年末城镇就业人员 123.4 万人。\n\n表 2 分年龄组失业率（单位：%）\n年龄组\t2023\t2024\n16–24 岁\t13.4\t12.3\n25–59 岁\t4.1\t3.9\n\n表 3 就业人员（单位：万人）\n年份\t人数\n2024\t123.4\n求人倍率\t1.25";

    fn fact(value: &str, unit: &str, quote: &str, unit_quote: &str) -> Fact {
        Fact {
            id: "F1".into(),
            source: "S1".into(),
            sub_question: "Q1".into(),
            metric: "青年失业率".into(),
            value: value.into(),
            unit: unit.into(),
            region: "示例市".into(),
            population: "16–24 岁".into(),
            period: "2024 年".into(),
            definition: "城镇调查失业率".into(),
            producer: "示例市统计局".into(),
            producer_kind: ProducerKind::CnGovStats,
            relayed: false,
            quote: quote.into(),
            unit_quote: unit_quote.into(),
            check: Check::default(),
            tier: Tier::Low,
        }
    }

    fn run(value: &str, unit: &str, quote: &str, unit_quote: &str) -> Check {
        check_fact(&fact(value, unit, quote, unit_quote), &Folded::new(TEXT))
    }

    fn reason(c: &Check) -> &str {
        assert_eq!(c.status, CheckStatus::Unverified);
        c.reason.as_deref().unwrap()
    }

    #[test]
    fn verbatim_quote_with_the_value_passes() {
        let c = run("12.3", "%", "示例市 16–24 岁城镇青年调查失业率为 12.3％", "");
        assert_eq!(c.status, CheckStatus::Verified, "{:?}", c.reason);
        let span = c.span.unwrap();
        assert_eq!(crate::fold::slice_chars(TEXT, span.start, span.end), "示例市 16–24 岁城镇青年调查失业率为 12.3％");
        assert_eq!(c.occurrences, 1);
        // 空白和全半角不同没关系
        assert_eq!(run("12.3", "％", "示例市16–24岁城镇青年调查失业率为12.3%", "").status, CheckStatus::Verified);
        // 千分位、百分点、万人
        assert_eq!(run("98560", "元", "年平均工资为 98,560 元", "").status, CheckStatus::Verified);
        assert_eq!(run("1.1", "个百分点", "比上年下降 1.1 个百分点", "").status, CheckStatus::Verified);
        assert_eq!(run("123.4", "万人", "年末城镇就业人员 123.4 万人", "").status, CheckStatus::Verified);
        assert_eq!(run("1234000", "人", "年末城镇就业人员 123.4 万人", "").status, CheckStatus::Verified);
        // 区间
        assert_eq!(run("16–24", "岁", "示例市 16–24 岁城镇青年调查失业率", "").status, CheckStatus::Verified);
    }

    #[test]
    fn a_quote_that_is_one_character_off_is_caught() {
        // 「调查」改成「登记」
        let c = run("12.3", "%", "示例市 16–24 岁城镇青年登记失业率为 12.3％", "");
        assert_eq!(reason(&c), "摘录在保存的正文里找不到");
        // 标点换成不是全半角关系的
        assert!(reason(&run("12.3", "%", "失业率为 12.3％、比上年下降", "")).contains("找不到"));
        assert!(reason(&run("12.3", "%", "12.3％", "")).contains("太短"));
    }

    #[test]
    fn a_number_that_is_one_digit_off_is_caught() {
        // 摘录是对的，数值改了一位
        let c = run("12.8", "%", "示例市 16–24 岁城镇青年调查失业率为 12.3％", "");
        assert_eq!(reason(&c), "数值不在摘录里");
        // 摘录里的数也跟着改：那摘录就找不到了
        assert!(reason(&run("12.8", "%", "示例市 16–24 岁城镇青年调查失业率为 12.8％", "")).contains("找不到"));
        assert!(reason(&run("98650", "元", "年平均工资为 98,560 元", "")).contains("数值不在摘录里"));
        assert!(reason(&run("十二", "%", "示例市 16–24 岁城镇青年调查失业率为 12.3％", "")).contains("读不懂"));
    }

    #[test]
    fn a_wrong_unit_is_caught() {
        let c = run("12.3", "‰", "示例市 16–24 岁城镇青年调查失业率为 12.3％", "");
        assert_eq!(reason(&c), "单位对不上：摘录里是「%」，事实写的是「‰」");
        assert!(reason(&run("98560", "万元", "年平均工资为 98,560 元", "")).contains("单位对不上"));
        assert!(reason(&run("123.4", "人", "年末城镇就业人员 123.4 万人", "")).contains("单位对不上"));
        assert!(reason(&run("1.1", "%", "比上年下降 1.1 个百分点", "")).contains("单位对不上"));
    }

    #[test]
    fn table_cells_need_the_unit_from_elsewhere_in_the_text() {
        // 表格里的数没有单位：要另给表头那处原文
        let c = run("12.3", "%", "16–24 岁\t13.4\t12.3", "");
        assert!(reason(&c).contains("没有单位"), "{:?}", c.reason);
        assert_eq!(run("12.3", "%", "16–24 岁\t13.4\t12.3", "分年龄组失业率（单位：%）").status, CheckStatus::Verified);
        assert_eq!(run("123.4", "万人", "2024\t123.4", "就业人员（单位：万人）").status, CheckStatus::Verified);
        // 给的单位原文和事实的单位不是一回事
        assert!(reason(&run("12.3", "‰", "16–24 岁\t13.4\t12.3", "分年龄组失业率（单位：%）")).contains("没有单位"));
        // 单位原文是编的
        assert!(reason(&run("12.3", "%", "16–24 岁\t13.4\t12.3", "单位：百分比")).contains("写明单位的那处原文"));
        // 没有单位的量（倍率）不用另给
        assert_eq!(run("1.25", "", "求人倍率\t1.25", "").status, CheckStatus::Verified);
    }

    #[test]
    fn units_the_scanner_does_not_know_still_work_when_adjacent() {
        let text = Folded::new("示例招聘平台 2024 年第四季度在示例市发布 4,820 个岗位，同比减少。");
        let f = fact("4820", "个岗位", "在示例市发布 4,820 个岗位", "");
        assert_eq!(check_fact(&f, &text).status, CheckStatus::Verified);
        let f = fact("4820", "岗位", "在示例市发布 4,820 个岗位", "");
        assert_eq!(check_fact(&f, &text).status, CheckStatus::Unverified);
        // 扫描器不认识的单位：紧跟在数后面就行
        let text = Folded::new("示例招聘平台净增 3,200 岗位，覆盖 12 个行业。");
        assert_eq!(check_fact(&fact("3200", "岗位", "示例招聘平台净增 3,200 岗位", ""), &text).status, CheckStatus::Verified);
        assert!(check_fact(&fact("3200", "职位", "示例招聘平台净增 3,200 岗位", ""), &text).reason.unwrap().contains("没有单位"));
    }
}
