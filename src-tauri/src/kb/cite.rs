//! 引用编号：模型在回答里写段落的编号（`[P123]`，检索和读原文的结果里给的），程序换成 `[1]`、`[2]`，
//! 页边注是那一段原文（段落号、页码、对照译文、一步到知识库和文献库）。
//!
//! - 编号按第一次出现的顺序；一处写几个（`[P12, P34]`、`[P12][P34]`）就是几个编号。
//! - 库里没有的编号原样留着（程序不改模型的话，`0030`）。
//! - 存进知识库的回答：编号后面附上「出处」一节（换了地方 [1] 还看得懂）。

use rusqlite::Connection;
use serde::Serialize;

use super::passages;
use super::retrieve::{self, Partner};

/// 一处 `[P12, P34]`：在文字里的位置（字节）和里面的编号
fn groups(text: &str) -> Vec<(usize, usize, Vec<i64>)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let (open, olen) = if b[i] == b'[' {
            (true, 1)
        } else if text[i..].starts_with('【') {
            (true, '【'.len_utf8())
        } else {
            (false, 0)
        };
        if !open {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let start = i;
        let mut j = i + olen;
        let mut ids = Vec::new();
        let mut ok = false;
        loop {
            while j < text.len() && text[j..].starts_with(' ') {
                j += 1;
            }
            if j < text.len() && (b[j] == b'P' || b[j] == b'p') {
                let k = j + 1;
                let mut e = k;
                while e < text.len() && b[e].is_ascii_digit() {
                    e += 1;
                }
                if e == k {
                    break;
                }
                ids.push(text[k..e].parse::<i64>().unwrap_or(0));
                j = e;
                while j < text.len() && text[j..].starts_with(' ') {
                    j += 1;
                }
                let sep = [",", "，", ";", "；", "、"].iter().find(|s| text[j..].starts_with(**s));
                if let Some(s) = sep {
                    j += s.len();
                    continue;
                }
                if text[j..].starts_with(']') {
                    j += 1;
                    ok = true;
                } else if text[j..].starts_with('】') {
                    j += '】'.len_utf8();
                    ok = true;
                }
            }
            break;
        }
        if ok && !ids.is_empty() {
            out.push((start, j, ids));
            i = j;
        } else {
            i += olen;
        }
    }
    out
}

/// 文字里的编号（按第一次出现的顺序，不重复）
pub fn handles(text: &str) -> Vec<i64> {
    let mut out = Vec::new();
    for (_, _, ids) in groups(text) {
        for id in ids {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// 换成 [1]、[2]：`order` 是编号的顺序（第几个就是几）；里面有不认识的那一处原样留着
pub fn number(text: &str, order: &[i64]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (s, e, ids) in groups(text) {
        let ns: Option<Vec<usize>> = ids.iter().map(|id| order.iter().position(|x| x == id).map(|i| i + 1)).collect();
        if let Some(ns) = ns {
            out.push_str(&text[last..s]);
            let mut seen = Vec::new();
            for n in ns {
                if !seen.contains(&n) {
                    out.push_str(&format!("[{n}]"));
                    seen.push(n);
                }
            }
            last = e;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// 正在写的时候：所有编号都按出现的顺序换（还不查库）
pub fn number_live(text: &str) -> String {
    number(text, &handles(text))
}

/// 页边注的一条
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cite {
    pub n: usize,
    /// 原文 / 旁注 / 注释 / 导读 / 笔记 / 摘抄 / 调查报告……
    pub kind: String,
    /// 《书名》 作者
    pub src: String,
    /// 段落号 · 第几页
    pub loc: String,
    pub text: String,
    /// 对照译文（或原著）
    pub translation: Option<String>,
    pub translation_loc: Option<String>,
    /// 知识库里的材料、它的家，打开时去那一段
    pub material: Option<String>,
    pub kb: Option<String>,
    pub passage: i64,
    /// 文献库里的条目（一步到文献库）
    pub item: Option<i64>,
}

fn kind_of(source_kind: &str, role: &str) -> String {
    match (source_kind, role) {
        ("full", "sidenote") => "旁注".into(),
        ("full", "note") => "注释".into(),
        ("full", "guide") => "导读".into(),
        ("full", _) => "原文".into(),
        (k, _) => retrieve::kind_name(k).into(),
    }
}

/// 查出编号对着的段落，按 `ids` 的顺序编号；库里没有的跳过
pub fn cites(c: &Connection, ids: &[i64]) -> Result<Vec<Cite>, String> {
    let mut partners: std::collections::HashMap<i64, Option<Partner>> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for id in ids {
        let Some(p) = passages::get(c, *id)? else { continue };
        let src = retrieve::source(c, p.material_id)?;
        let pair = retrieve::pair_of(c, &p, &mut partners)?;
        let by = if src.sub.is_empty() { String::new() } else { format!(" {}", src.sub) };
        out.push(Cite {
            n: out.len() + 1,
            kind: kind_of(&src.kind, &p.role),
            src: format!("《{}》{by}", src.title),
            loc: retrieve::locator(&p),
            text: p.text.clone(),
            translation: (!pair.is_empty()).then(|| pair.iter().map(|q| q.text.as_str()).collect::<Vec<_>>().join("\n")),
            translation_loc: pair.first().map(retrieve::locator),
            material: Some(src.uid.clone()),
            kb: Some(src.kb.clone()),
            passage: p.id,
            item: src.item,
        });
    }
    Ok(out)
}

/// 一段回答里的编号：查库、换成 [n]，返回（换好的文字, 页边注）
#[cfg(test)]
pub fn resolve(c: &Connection, text: &str) -> Result<(String, Vec<Cite>), String> {
    let cites = cites(c, &handles(text))?;
    let order: Vec<i64> = cites.iter().map(|x| x.passage).collect();
    Ok((number(text, &order), cites))
}

/// 存进知识库的正文：编号换成 [n]，后面附上出处
pub fn with_sources(text: &str, cites: &[Cite]) -> String {
    let order: Vec<i64> = cites.iter().map(|x| x.passage).collect();
    let mut out = number(text, &order);
    let used: Vec<&Cite> = cites.iter().filter(|x| out.contains(&format!("[{}]", x.n))).collect();
    if !used.is_empty() {
        out.push_str("\n\n出处：");
        for x in used {
            out.push_str(&format!("\n[{}] {} {}", x.n, x.src, x.loc));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_become_numbers_in_order() {
        let t = "分工靠交换 [P12]，市场小就分不细【P34】。又见 [P12, P56] 和 [P56][P99]。这里 [1] 和 [P] 不动，[P7 不完整。";
        assert_eq!(handles(t), [12, 34, 56, 99]);
        assert_eq!(number(t, &[12, 34, 56, 99]), "分工靠交换 [1]，市场小就分不细[2]。又见 [1][3] 和 [3][4]。这里 [1] 和 [P] 不动，[P7 不完整。");
        // 不认识的那一处原样留着
        assert_eq!(number("甲 [P12] 乙 [P5, P12]", &[12]), "甲 [1] 乙 [P5, P12]");
        assert_eq!(number_live("还在写 [P8]，接着 [P3]"), "还在写 [1]，接着 [2]");
        assert_eq!(handles("[p7]；[ P8 ]"), [7, 8]);
    }
}
