//! 从文字里认数：千分位、小数、负数、万 / 亿、% / ‰ / 百分点、常见单位、区间（`decisions/0020`「核对」）。
//!
//! 只认阿拉伯数字（全角的照认）。「三成」「一半」这类中文数量词认不了，由 `chinese_quantities` 找出来交给调用方标记。
//! 这里不判断对错，只把文字变成可以精确比较的 `Quantity`。

use crate::dec::Dec;
use crate::fold::{Span, half_width};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quantity {
    /// 写出来的数（去掉千分位），不含量级
    pub number: Dec,
    /// 量级：10 的几次方（万 = 4，亿 = 8）
    pub scale: u32,
    /// 规范后的单位：`%`、`‰`、`pp`（百分点）、`元`、`人`……没有单位是空串
    pub unit: String,
    /// 在被扫描的文字里的位置（按字符）
    pub span: Span,
}

impl Quantity {
    /// 换成基本单位的值：1.2 万 → 12000
    pub fn base(&self) -> Option<Dec> {
        self.number.shift(self.scale)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Found {
    One(Quantity),
    /// 区间：两端共用后一个数的单位和量级（「1.2–1.5 万人」）
    Range(Quantity, Quantity),
}

impl Found {
    pub fn span(&self) -> Span {
        match self {
            Found::One(q) => q.span,
            Found::Range(a, b) => Span { start: a.span.start, end: b.span.end },
        }
    }

    pub fn quantities(&self) -> Vec<&Quantity> {
        match self {
            Found::One(q) => vec![q],
            Found::Range(a, b) => vec![a, b],
        }
    }
}

/// 量级词：长的在前
const SCALES: &[(&str, u32)] = &[("万亿", 12), ("千万", 7), ("百万", 6), ("亿", 8), ("万", 4)];
/// 「千」只在后面跟着这些字时当量级（「5 千米」是长度）
const AFTER_QIAN: &[char] = &['元', '人', '名', '个', '家', '户', '套', '件', '万', '亿', '多', '余'];

/// 认得的单位 → 规范写法；长的在前
const UNITS: &[(&str, &str)] = &[
    ("个百分点", "pp"),
    ("百分点", "pp"),
    ("个月", "个月"),
    ("小时", "小时"),
    ("季度", "季度"),
    ("人次", "人次"),
    ("美元", "美元"),
    ("港元", "港元"),
    ("欧元", "欧元"),
    ("日元", "日元"),
    ("英镑", "英镑"),
    ("%", "%"),
    ("‰", "‰"),
    ("元", "元"),
    ("人", "人"),
    ("倍", "倍"),
    ("岁", "岁"),
    ("年", "年"),
    ("月", "月"),
    ("日", "日"),
    ("天", "天"),
    ("周", "周"),
    ("个", "个"),
    ("家", "家"),
    ("户", "户"),
    ("套", "套"),
    ("件", "件"),
    ("所", "所"),
    ("名", "名"),
    ("位", "位"),
    ("项", "项"),
    ("次", "次"),
    ("条", "条"),
    ("篇", "篇"),
    ("份", "份"),
];

fn norm(c: char) -> char {
    match half_width(c) {
        '\u{FE6A}' => '%',
        '\u{2212}' | '\u{FE63}' => '-',
        other => other,
    }
}

fn starts_with(cs: &[char], at: usize, word: &str) -> bool {
    let mut i = at;
    for w in word.chars() {
        if cs.get(i) != Some(&w) {
            return false;
        }
        i += 1;
    }
    true
}

fn skip_spaces(cs: &[char], mut i: usize, max: usize) -> usize {
    let start = i;
    while i < cs.len() && i - start < max && matches!(cs[i], ' ' | '\u{00A0}' | '\u{2009}' | '\u{202F}') {
        i += 1;
    }
    i
}

/// 从 `at` 起认量级和单位。返回（量级, 单位, 结束位置）；什么都没认到时结束位置就是 `at`。
fn scale_and_unit(cs: &[char], at: usize) -> (u32, String, usize) {
    let mut i = skip_spaces(cs, at, 2);
    let mut scale = 0;
    if let Some((word, s)) = SCALES.iter().find(|(w, _)| starts_with(cs, i, w)) {
        let after = i + word.chars().count();
        // 「万分之三」里的万不是量级
        if !starts_with(cs, after, "分之") {
            scale = *s;
            i = after;
        }
    } else if cs.get(i) == Some(&'千') && cs.get(i + 1).is_some_and(|c| AFTER_QIAN.contains(c)) {
        scale = 3;
        i += 1;
        if let Some((word, s)) = SCALES.iter().find(|(w, _)| starts_with(cs, i, w)) {
            // 「3 千万」已经在上面按「千万」认了；这里是「2 千亿」
            scale += *s;
            i += word.chars().count();
        }
    }
    if scale > 0 && matches!(cs.get(i), Some('多' | '余')) {
        i += 1;
    }
    if let Some((word, unit)) = UNITS.iter().find(|(w, _)| starts_with(cs, i, w)) {
        return (scale, unit.to_string(), i + word.chars().count());
    }
    if scale > 0 { (scale, String::new(), i) } else { (0, String::new(), at) }
}

/// 从 `at`（一个数字）起认一个不带符号的数。返回（数, 数字部分的结束位置）
fn digits_at(cs: &[char], at: usize) -> Option<(Dec, usize)> {
    let digit = |i: usize| cs.get(i).is_some_and(|c| c.is_ascii_digit());
    if !digit(at) {
        return None;
    }
    let mut i = at;
    let mut text = String::new();
    while digit(i) {
        text.push(cs[i]);
        i += 1;
    }
    // 千分位：前一组 1–3 位，后面每组正好 3 位
    if text.len() <= 3 {
        while cs.get(i) == Some(&',') && digit(i + 1) && digit(i + 2) && digit(i + 3) && !digit(i + 4) {
            text.extend(&cs[i + 1..i + 4]);
            i += 4;
        }
    }
    if cs.get(i) == Some(&'.') && digit(i + 1) {
        text.push('.');
        i += 1;
        while digit(i) {
            text.push(cs[i]);
            i += 1;
        }
    }
    Dec::parse(&text).map(|d| (d, i))
}

fn quantity_at(cs: &[char], at: usize, negative: bool, start: usize) -> Option<(Quantity, usize)> {
    let (number, after_digits) = digits_at(cs, at)?;
    let number = if negative { Dec::ZERO.sub(number)? } else { number };
    let (scale, unit, end) = scale_and_unit(cs, after_digits);
    Some((Quantity { number, scale, unit, span: Span { start, end } }, end))
}

/// `2024-05-01`、`2024/5/1`、`2024.05.01`、`2024-05`：日期整个跳过，不当成数或区间
fn date_end(cs: &[char], at: usize) -> Option<usize> {
    let run = |i: usize| (i..cs.len()).take_while(|k| cs[*k].is_ascii_digit()).count();
    let value = |i: usize, n: usize| cs[i..i + n].iter().collect::<String>().parse::<u32>().ok();
    if run(at) != 4 || !(1800..=2200).contains(&value(at, 4)?) {
        return None;
    }
    let sep = *cs.get(at + 4)?;
    if !matches!(sep, '-' | '/' | '.') {
        return None;
    }
    let m = run(at + 5);
    if !(1..=2).contains(&m) || !(1..=12).contains(&value(at + 5, m)?) {
        return None;
    }
    let after_month = at + 5 + m;
    if cs.get(after_month) == Some(&sep) {
        let d = run(after_month + 1);
        if (1..=2).contains(&d) && (1..=31).contains(&value(after_month + 1, d)?) {
            return Some(after_month + 1 + d);
        }
    }
    // 只有年月：只认横线（「2024.05」可能是小数，「2024/5」少见）
    (sep == '-' && m == 2).then_some(after_month)
}

fn is_range_sep(c: char) -> bool {
    matches!(c, '-' | '~' | '–' | '—' | '至' | '到')
}

/// 找出文字里所有的数。位置按字符，对着传进来的原文。
pub fn scan(text: &str) -> Vec<Found> {
    let cs: Vec<char> = text.chars().map(norm).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if !c.is_ascii_digit() {
            i += 1;
            continue;
        }
        let prev = i.checked_sub(1).map(|p| cs[p]);
        // 编号的一部分（F12、S3、5G 的 G 之前不管）：跳过这串数字
        if prev.is_some_and(|p| p.is_ascii_alphabetic() || p == '_') {
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            continue;
        }
        if let Some(end) = date_end(&cs, i) {
            i = end;
            continue;
        }
        // 符号：紧挨着的减号（前面不是字母、数字、右括号、百分号），或者「负」
        let (negative, start) = match prev {
            Some('-') => {
                let before = i.checked_sub(2).map(|p| cs[p]);
                let sign = !before.is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, ')' | '%' | '‰' | '.'));
                (sign, if sign { i - 1 } else { i })
            }
            Some('负') => (true, i - 1),
            Some(' ') if i >= 2 && cs[i - 2] == '负' => (true, i - 2),
            _ => (false, i),
        };
        let Some((mut lo, next)) = quantity_at(&cs, i, negative, start) else {
            i += 1;
            continue;
        };
        // 区间：数 + 连接符 + 数
        let j = skip_spaces(&cs, next, 2);
        if j < cs.len() && is_range_sep(cs[j]) {
            let k = skip_spaces(&cs, j + 1, 2);
            if date_end(&cs, k).is_none()
                && let Some((hi, next2)) = quantity_at(&cs, k, false, k)
            {
                if lo.unit.is_empty() && lo.scale == 0 {
                    lo.unit = hi.unit.clone();
                    lo.scale = hi.scale;
                }
                out.push(Found::Range(lo, hi));
                i = next2;
                continue;
            }
        }
        out.push(Found::One(lo));
        i = next;
    }
    out
}

/// 模型给的单位（「%」「万人」「元/月」「万元/年」）→（量级, 规范单位）。
/// 不认识的单位原样留着（去掉「/月」「每月」这样的后缀）。
pub fn parse_unit(text: &str) -> (u32, String) {
    let cs: Vec<char> = text.trim().chars().map(norm).filter(|c| !c.is_whitespace()).collect();
    if cs.is_empty() {
        return (0, String::new());
    }
    let (scale, unit, end) = scale_and_unit(&cs, 0);
    if !unit.is_empty() {
        return (scale, unit);
    }
    let rest: String = cs[end..].iter().take_while(|c| !matches!(c, '/' | '每')).collect();
    (scale, rest)
}

/// 把模型给的「数值原文 + 单位」变成一个数或区间。数值原文里必须正好有一个数（或一个区间）。
pub fn parse_value(value_text: &str, unit_text: &str) -> Result<Found, String> {
    let mut found = scan(value_text);
    match found.len() {
        0 => return Err(format!("「{value_text}」里没有数")),
        1 => {}
        _ => return Err(format!("「{value_text}」里不止一个数")),
    }
    let mut one = found.remove(0);
    let (scale, unit) = parse_unit(unit_text);
    let apply = |q: &mut Quantity| {
        if q.unit.is_empty() {
            q.unit = unit.clone();
            if q.scale == 0 {
                q.scale = scale;
            }
        }
    };
    match &mut one {
        Found::One(q) => apply(q),
        Found::Range(a, b) => {
            apply(a);
            apply(b);
        }
    }
    Ok(one)
}

/// 认不了的中文数量词（「三成」「一半」「翻了一番」「两倍」）：结论里出现就没法核对
pub fn chinese_quantities(text: &str) -> Vec<String> {
    const NUM: &str = "一二两三四五六七八九十百千半几数";
    const UNIT: &[&str] = &["成", "倍", "个百分点", "百分点", "万", "亿", "番"];
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if NUM.contains(cs[i]) && !i.checked_sub(1).is_some_and(|p| NUM.contains(cs[p]) || cs[p].is_ascii_digit() || cs[p] == '第') {
            let mut j = i;
            while j < cs.len() && NUM.contains(cs[j]) {
                j += 1;
            }
            if let Some(u) = UNIT.iter().find(|u| starts_with(&cs, j, u)) {
                // 「一倍」「三成」算；「一万个理由」里的万后面还要是数量语境，这里从宽：都报
                out.push(cs[i..j + u.chars().count()].iter().collect());
                i = j + u.chars().count();
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    for word in ["一半", "过半", "翻番", "翻倍", "百分之"] {
        if text.contains(word) && !out.iter().any(|o: &String| o.contains(word)) {
            out.push(word.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fold::slice_chars;

    fn one(text: &str) -> Quantity {
        match scan(text).as_slice() {
            [Found::One(q)] => q.clone(),
            other => panic!("「{text}」→ {other:?}"),
        }
    }

    fn show(q: &Quantity) -> String {
        format!("{}e{}{}", q.number, q.scale, q.unit)
    }

    #[test]
    fn plain_numbers_and_units() {
        assert_eq!(show(&one("12.3%")), "12.3e0%");
        assert_eq!(show(&one("１２．３％")), "12.3e0%");
        assert_eq!(show(&one("12.3 %")), "12.3e0%");
        assert_eq!(show(&one("4.5‰")), "4.5e0‰");
        assert_eq!(show(&one("下降 2.1 个百分点")), "2.1e0pp");
        assert_eq!(show(&one("8,520 元/月")), "8520e0元");
        assert_eq!(show(&one("1,234,567.89")), "1234567.89e0");
        assert_eq!(show(&one("求人倍率为 1.25")), "1.25e0");
        assert_eq!(show(&one("共 35 家企业")), "35e0家");
    }

    #[test]
    fn scales() {
        assert_eq!(show(&one("1.2万人")), "1.2e4人");
        assert_eq!(show(&one("1.2 万人")), "1.2e4人");
        assert_eq!(show(&one("3.5亿元")), "3.5e8元");
        assert_eq!(show(&one("2.4万亿元")), "2.4e12元");
        assert_eq!(show(&one("3 千万人")), "3e7人");
        assert_eq!(show(&one("5千元")), "5e3元");
        assert_eq!(show(&one("3万多人")), "3e4人");
        assert_eq!(show(&one("约 1.5 亿")), "1.5e8");
        assert_eq!(one("1.2万人").base().unwrap().to_string(), "12000");
        // 不是量级
        assert_eq!(show(&one("5千米")), "5e0");
        assert_eq!(show(&one("万分之3")), "3e0");
    }

    #[test]
    fn negatives() {
        assert_eq!(show(&one("同比-3.2%")), "-3.2e0%");
        assert_eq!(show(&one("增速为 −0.5%")), "-0.5e0%");
        assert_eq!(show(&one("增速为负 1.5%")), "-1.5e0%");
        assert_eq!(show(&one("负1.5%")), "-1.5e0%");
        // 连字符不是负号
        assert_eq!(show(&one("表A-12")), "12e0");
    }

    #[test]
    fn ranges() {
        let r = |text: &str| match scan(text).as_slice() {
            [Found::Range(a, b)] => format!("{}..{}", show(a), show(b)),
            other => panic!("「{text}」→ {other:?}"),
        };
        assert_eq!(r("12.3%–15.6%"), "12.3e0%..15.6e0%");
        assert_eq!(r("12.3~15.6%"), "12.3e0%..15.6e0%");
        assert_eq!(r("16-24岁"), "16e0岁..24e0岁");
        assert_eq!(r("16—24 岁"), "16e0岁..24e0岁");
        assert_eq!(r("1.2至1.5万人"), "1.2e4人..1.5e4人");
        assert_eq!(r("2022－2026年"), "2022e0年..2026e0年");
        assert_eq!(r("8000 到 10000 元"), "8000e0元..10000e0元");
    }

    #[test]
    fn dates_and_ids_are_skipped() {
        assert!(scan("发布于 2024-05-01。").is_empty());
        assert!(scan("2024/5/1 更新").is_empty());
        assert!(scan("2024-05 月报").is_empty());
        assert!(scan("见 [F12] 和 S3、Q1").is_empty());
        assert_eq!(scan("2024年5月").len(), 2);
        // 位置对着原文
        let text = "示例市 2024 年青年失业率为 12.3％，比上年低 1.1 个百分点";
        let all = scan(text);
        let parts: Vec<String> = all.iter().map(|f| slice_chars(text, f.span().start, f.span().end)).collect();
        assert_eq!(parts, ["2024 年", "12.3％", "1.1 个百分点"]);
    }

    #[test]
    fn model_units_and_values() {
        assert_eq!(parse_unit("%"), (0, "%".into()));
        assert_eq!(parse_unit("万人"), (4, "人".into()));
        assert_eq!(parse_unit("元/月"), (0, "元".into()));
        assert_eq!(parse_unit("万元/年"), (4, "元".into()));
        assert_eq!(parse_unit(" 个百分点 "), (0, "pp".into()));
        assert_eq!(parse_unit("岗位"), (0, "岗位".into()));
        assert_eq!(parse_unit(""), (0, String::new()));

        let v = |value: &str, unit: &str| match parse_value(value, unit).unwrap() {
            Found::One(q) => show(&q),
            Found::Range(a, b) => format!("{}..{}", show(&a), show(&b)),
        };
        assert_eq!(v("12.3", "%"), "12.3e0%");
        assert_eq!(v("12.3%", "%"), "12.3e0%");
        assert_eq!(v("1.2", "万人"), "1.2e4人");
        assert_eq!(v("1.2万", "人"), "1.2e4人");
        assert_eq!(v("1.2万", "万人"), "1.2e4人");
        assert_eq!(v("8,520", "元/月"), "8520e0元");
        assert_eq!(v("12.3–15.6", "%"), "12.3e0%..15.6e0%");
        assert_eq!(v("1.25", ""), "1.25e0");
        assert!(parse_value("很多", "%").is_err());
        assert!(parse_value("12.3 和 15.6", "%").is_err());
    }

    #[test]
    fn chinese_quantity_words_are_reported() {
        assert_eq!(chinese_quantities("约三成受访者，比去年多了一倍"), ["三成", "一倍"]);
        assert_eq!(chinese_quantities("超过一半"), ["一半"]);
        assert!(chinese_quantities("第三产业的就业人数 12.3 万").is_empty());
        assert!(chinese_quantities("示例市青年失业率为 12.3%").is_empty());
    }
}
