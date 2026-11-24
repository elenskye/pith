//! 「只检查、不改」的项目：由这里的确定性代码统计（字数、个数、编号形式），不由模型数。
//! 找不到要统计的地方（比如文档里没有「摘要」标题）时结果是「没找到」，交给人工核对，不猜。

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::outline::{snippet, Outline, Para};

/// 能自动统计的检查项。规范里别的「只检查」要求列进报告，标明需人工核对。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 中文摘要字数（min–max）
    AbstractZhChars,
    /// 中文关键词个数（min–max）、分隔符、末尾标点
    KeywordsCount,
    /// 参考文献条数（min），并报告外文条目占比
    ReferencesCount,
    /// 图、表编号是否按章（图 2.1）
    CaptionByChapter,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::AbstractZhChars => "中文摘要字数",
            Kind::KeywordsCount => "关键词个数",
            Kind::ReferencesCount => "参考文献条数",
            Kind::CaptionByChapter => "图表按章编号",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub kind: Kind,
    /// Some(true) 符合、Some(false) 不符合、None 没找到要统计的地方
    pub ok: Option<bool>,
    /// 统计到的数（「462 字」「5 个」）
    pub value: String,
    pub detail: String,
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn in_range(n: usize, min: Option<f64>, max: Option<f64>) -> bool {
    min.is_none_or(|m| n as f64 >= m) && max.is_none_or(|m| n as f64 <= m)
}

fn range_text(min: Option<f64>, max: Option<f64>, unit: &str) -> String {
    match (min, max) {
        (Some(a), Some(b)) => format!("要求 {a}–{b} {unit}"),
        (Some(a), None) => format!("要求不少于 {a} {unit}"),
        (None, Some(b)) => format!("要求不超过 {b} {unit}"),
        (None, None) => "规范没给出范围".into(),
    }
}

/// Word 的「字数」算法：每个汉字、每个全角标点算一个，连续的西文字母数字算一个词
pub fn word_count(text: &str) -> usize {
    let mut n = 0;
    let mut in_word = false;
    for c in text.chars() {
        if c.is_whitespace() {
            in_word = false;
        } else if c.is_ascii() || (c as u32) < 0x2E80 {
            if !in_word {
                n += 1;
            }
            in_word = true;
        } else {
            n += 1;
            in_word = false;
        }
    }
    n
}

fn title_is(p: &Para, names: &[&str]) -> bool {
    let t = squash(&p.text);
    t.chars().count() <= 12 && names.iter().any(|n| t.eq_ignore_ascii_case(n))
}

fn keyword_line(p: &Para) -> Option<String> {
    let t = squash(&p.text);
    let rest = t.strip_prefix("关键词").or_else(|| t.strip_prefix("关键字"))?;
    Some(rest.trim_start_matches(['：', ':']).to_string())
}

fn looks_like_heading(p: &Para) -> bool {
    p.outline_level.is_some() || p.style.to_lowercase().starts_with("heading") || p.style.starts_with("标题")
}

fn abstract_zh(o: &Outline, min: Option<f64>, max: Option<f64>) -> Outcome {
    let kind = Kind::AbstractZhChars;
    let Some(start) = o.paragraphs.iter().position(|p| title_is(p, &["摘要", "中文摘要"])) else {
        return Outcome { kind, ok: None, value: "没找到".into(), detail: "文档里没有单独成段的「摘要」标题".into() };
    };
    let mut text = String::new();
    let mut paras = 0;
    for p in &o.paragraphs[start + 1..] {
        if keyword_line(p).is_some() || title_is(p, &["abstract", "目录", "目次"]) || looks_like_heading(p) {
            break;
        }
        if p.chars() > 0 {
            paras += 1;
            text.push_str(&p.text);
            text.push('\n');
        }
    }
    let n = word_count(&text);
    Outcome { kind, ok: Some(in_range(n, min, max)), value: format!("{n} 字"), detail: format!("{}；摘要正文 {paras} 段，按 Word「字数」的算法统计", range_text(min, max, "字")) }
}

fn keywords(o: &Outline, min: Option<f64>, max: Option<f64>) -> Outcome {
    let kind = Kind::KeywordsCount;
    let Some(line) = o.paragraphs.iter().find_map(keyword_line) else {
        return Outcome { kind, ok: None, value: "没找到".into(), detail: "文档里没有以「关键词」开头的段落".into() };
    };
    let sep = ['；', ';', '，', ',', '、'].into_iter().find(|s| line.contains(*s));
    let words: Vec<&str> = match sep {
        Some(s) => line.split(s).filter(|w| !w.is_empty()).collect(),
        None => vec![line.as_str()].into_iter().filter(|w| !w.is_empty()).collect(),
    };
    let trailing = line.chars().last().filter(|c| "；;，,、。.".contains(*c));
    let n = words.len();
    let mut notes = vec![range_text(min, max, "个")];
    if let Some(s) = sep {
        notes.push(format!("用「{s}」分隔"));
    }
    notes.push(match trailing {
        Some(c) => format!("末尾有标点「{c}」"),
        None => "末尾没有标点".into(),
    });
    Outcome { kind, ok: Some(in_range(n, min, max)), value: format!("{n} 个"), detail: notes.join("；") }
}

fn references(o: &Outline, min: Option<f64>) -> Outcome {
    let kind = Kind::ReferencesCount;
    // 先认参考文献样式（Pith、Zotero 的书目都用它），没有再按「参考文献」标题往后数
    let mut entries: Vec<&Para> = o.paragraphs.iter().filter(|p| p.style.eq_ignore_ascii_case("bibliography") && p.chars() > 0).collect();
    let mut how = "按参考文献样式的段落数";
    if entries.is_empty() {
        let Some(start) = o.paragraphs.iter().rposition(|p| title_is(p, &["参考文献", "references"])) else {
            return Outcome { kind, ok: None, value: "没找到".into(), detail: "文档里没有参考文献样式的段落，也没有「参考文献」标题".into() };
        };
        entries = o.paragraphs[start + 1..].iter().take_while(|p| !looks_like_heading(p) && !title_is(p, &["致谢", "附录", "谢辞"])).filter(|p| p.chars() > 0).collect();
        how = "按「参考文献」标题后的段落数";
    }
    let n = entries.len();
    let foreign = entries.iter().filter(|p| !p.text.chars().any(is_cjk)).count();
    let share = if n == 0 { 0.0 } else { foreign as f64 * 100.0 / n as f64 };
    Outcome {
        kind,
        ok: Some(in_range(n, min, None)),
        value: format!("{n} 条"),
        detail: format!("{}；{how}；外文 {foreign} 条（{share:.0}%）", range_text(min, None, "条")),
    }
}

fn captions(o: &Outline) -> Outcome {
    let kind = Kind::CaptionByChapter;
    let re = Regex::new(r"^(图|表)\s*([A-Z]?\d+)([.\-－]\d+)?").expect("固定的正则");
    let mut flat = Vec::new();
    let mut by_chapter = 0;
    for p in o.paragraphs.iter().filter(|p| p.text.trim().chars().count() <= 60) {
        let t = p.text.trim();
        let Some(c) = re.captures(t) else { continue };
        // 「图 3 展示了……」这类正文句子不算：编号后面得是空白、冒号或结尾
        let rest = &t[c.get(0).unwrap().end()..];
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with(['：', ':', '　'])) || t.contains(['，', '。', '；']) {
            continue;
        }
        if c.get(3).is_some() {
            by_chapter += 1;
        } else {
            flat.push(snippet(t, 20));
        }
    }
    if flat.is_empty() && by_chapter == 0 {
        return Outcome { kind, ok: None, value: "没找到".into(), detail: "没有认出图题、表题（以「图 1」「表 2.1」开头的短段落）".into() };
    }
    let detail = if flat.is_empty() { "题注都按章编号".to_string() } else { format!("没按章编号的：{}{}", flat.iter().take(4).cloned().collect::<Vec<_>>().join("、"), if flat.len() > 4 { " 等" } else { "" }) };
    Outcome { kind, ok: Some(flat.is_empty()), value: format!("按章 {by_chapter} 个，不按章 {} 个", flat.len()), detail }
}

pub fn run(o: &Outline, kind: Kind, min: Option<f64>, max: Option<f64>) -> Outcome {
    match kind {
        Kind::AbstractZhChars => abstract_zh(o, min, max),
        Kind::KeywordsCount => keywords(o, min, max),
        Kind::ReferencesCount => references(o, min),
        Kind::CaptionByChapter => captions(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(i: usize, style: &str, text: &str) -> Para {
        Para { index: i, section: 1, style: style.into(), text: text.into(), in_table: false, in_textbox: false, figure: false, math: false, align: None, numbered: false, outline_level: None }
    }

    fn outline(rows: &[(&str, &str)]) -> Outline {
        Outline {
            default_style: "Normal".into(),
            styles: Vec::new(),
            paragraphs: rows.iter().enumerate().map(|(i, (s, t))| para(i + 1, s, t)).collect(),
            sections: Vec::new(),
            tables: 0,
            table_list: Vec::new(),
            figures: 0,
            footnotes: 0,
            pages_hint: None,
        }
    }

    #[test]
    fn counts_like_word() {
        assert_eq!(word_count("政治效能感（political efficacy）是 2 个维度。"), 15);
        assert_eq!(word_count("  "), 0);
    }

    #[test]
    fn abstract_and_keywords() {
        let o = outline(&[
            ("Normal", "摘  要"),
            ("Normal", "本文研究分工。"),
            ("Normal", "结论是 A 与 B 有关。"),
            ("Normal", "关键词：分工；市场；效率；制度；交换"),
            ("Normal", "ABSTRACT"),
            ("Normal", "This is English."),
        ]);
        let a = run(&o, Kind::AbstractZhChars, Some(300.0), Some(500.0));
        assert_eq!(a.value, "16 字");
        assert_eq!(a.ok, Some(false));
        let k = run(&o, Kind::KeywordsCount, Some(3.0), Some(5.0));
        assert_eq!((k.value.as_str(), k.ok), ("5 个", Some(true)));
        assert!(k.detail.contains("用「；」分隔") && k.detail.contains("末尾没有标点"), "{}", k.detail);
        let none = run(&outline(&[("Normal", "正文")]), Kind::AbstractZhChars, None, None);
        assert_eq!(none.ok, None);
    }

    #[test]
    fn references_by_style_or_title() {
        let by_style = outline(&[("Normal", "正文"), ("Bibliography", "[1] SMITH A. The wealth of nations[M]."), ("Bibliography", "[2] 王晓明. 分工[J].")]);
        let r = run(&by_style, Kind::ReferencesCount, Some(15.0), None);
        assert_eq!((r.value.as_str(), r.ok), ("2 条", Some(false)));
        assert!(r.detail.contains("外文 1 条（50%）"), "{}", r.detail);
        let by_title = outline(&[("Normal", "参考文献"), ("Normal", "[1] A."), ("Normal", ""), ("Normal", "[2] B."), ("Normal", "致  谢"), ("Normal", "谢谢")]);
        assert_eq!(run(&by_title, Kind::ReferencesCount, Some(2.0), None).ok, Some(true));
    }

    #[test]
    fn captions_by_chapter() {
        let o = outline(&[("Normal", "图1 模型"), ("Normal", "表 2.1 变量"), ("Normal", "图 3 展示了结果，可以看到差异很大"), ("Normal", "图3展示了结果")]);
        let c = run(&o, Kind::CaptionByChapter, None, None);
        assert_eq!(c.ok, Some(false));
        assert_eq!(c.value, "按章 1 个，不按章 1 个");
    }
}
