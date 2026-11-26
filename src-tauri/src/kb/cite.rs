//! 引用编号：模型在回答里写工具给的编号，程序换成 `[1]`、`[2]`，页边注是那一段原文。
//!
//! - `[P123]`：知识库里的一块（检索、读原文给的）：段落号、页码、对照译文、一步到知识库和文献库；
//! - `[W45]`：对话里取回的网页的一块（`web_fetch` 给的，1.1.0 第 4 期）：网站、取回的日子、打开网页、存进知识库。
//! - 编号按第一次出现的顺序；一处写几个（`[P12, W34]`、`[P12][P34]`）就是几个编号。
//! - 库里没有的编号原样留着（程序不改模型的话，`0030`）。
//! - 存进知识库的回答：编号后面附上「出处」一节（换了地方 [1] 还看得懂）。

use rusqlite::Connection;
use serde::Serialize;

use super::passages;
use super::retrieve::{self, Partner};

/// 一个编号：知识库的一块（P）或网页的一块（W）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    P(i64),
    W(i64),
}

impl std::fmt::Display for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Handle::P(n) => write!(f, "P{n}"),
            Handle::W(n) => write!(f, "W{n}"),
        }
    }
}

/// 一处 `[P12, W34]`：在文字里的位置（字节）和里面的编号
fn groups(text: &str) -> Vec<(usize, usize, Vec<Handle>)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let olen = if b[i] == b'[' {
            1
        } else if text[i..].starts_with('【') {
            '【'.len_utf8()
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        let start = i;
        let mut j = i + olen;
        let mut ids = Vec::new();
        let mut ok = false;
        loop {
            while j < text.len() && b[j] == b' ' {
                j += 1;
            }
            let kind = match b.get(j) {
                Some(b'P' | b'p') => Handle::P as fn(i64) -> Handle,
                Some(b'W' | b'w') => Handle::W as fn(i64) -> Handle,
                _ => break,
            };
            let k = j + 1;
            let mut e = k;
            while e < text.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            if e == k {
                break;
            }
            ids.push(kind(text[k..e].parse::<i64>().unwrap_or(0)));
            j = e;
            while j < text.len() && b[j] == b' ' {
                j += 1;
            }
            if let Some(s) = [",", "，", ";", "；", "、"].iter().find(|s| text[j..].starts_with(**s)) {
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
pub fn handles(text: &str) -> Vec<Handle> {
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
pub fn number(text: &str, order: &[Handle]) -> String {
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
    /// 原文 / 旁注 / 注释 / 导读 / 笔记 / 摘抄 / 调查报告 / 网页……
    pub kind: String,
    /// 《书名》 作者；网页的标题
    pub src: String,
    /// 段落号 · 第几页；网站 · 取回的日子
    pub loc: String,
    pub text: String,
    /// 对照译文（或原著）
    pub translation: Option<String>,
    pub translation_loc: Option<String>,
    /// 知识库里的材料、它的家，打开时去那一段（网页存进知识库以后也有）
    pub material: Option<String>,
    pub kb: Option<String>,
    /// 知识库的块
    pub passage: Option<i64>,
    /// 文献库里的条目（一步到文献库）
    pub item: Option<i64>,
    /// 网页：网址、取回的那一页（存进知识库用）
    pub url: Option<String>,
    pub web_page: Option<i64>,
    #[serde(skip)]
    pub key: Option<Handle>,
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
pub fn cites(c: &Connection, ids: &[Handle]) -> Result<Vec<Cite>, String> {
    let mut partners: std::collections::HashMap<i64, Option<Partner>> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for id in ids {
        let n = out.len() + 1;
        match *id {
            Handle::P(pid) => {
                let Some(p) = passages::get(c, pid)? else { continue };
                let src = retrieve::source(c, p.material_id)?;
                let pair = retrieve::pair_of(c, &p, &mut partners)?;
                let by = if src.sub.is_empty() { String::new() } else { format!(" {}", src.sub) };
                out.push(Cite {
                    n,
                    kind: kind_of(&src.kind, &p.role),
                    src: format!("《{}》{by}", src.title),
                    loc: retrieve::locator(&p),
                    text: p.text.clone(),
                    translation: (!pair.is_empty()).then(|| pair.iter().map(|q| q.text.as_str()).collect::<Vec<_>>().join("\n")),
                    translation_loc: pair.first().map(retrieve::locator),
                    material: Some(src.uid.clone()),
                    kb: Some(src.kb.clone()),
                    passage: Some(p.id),
                    item: src.item,
                    url: src.url.clone(),
                    web_page: None,
                    key: Some(*id),
                });
            }
            Handle::W(bid) => {
                let Some((b, page)) = crate::web::store::block(c, bid)? else { continue };
                let saved = match super::store::by_source(c, "web_page", &page.id.to_string())? {
                    Some(uid) => Some(super::store::material_get(c, &uid)?),
                    None => None,
                };
                let mut loc = vec![page.site.clone()];
                if !b.heading.is_empty() {
                    loc.push(b.heading.clone());
                }
                loc.push(format!("取于 {}", page.fetched_at.get(..10).unwrap_or("")));
                out.push(Cite {
                    n,
                    kind: "网页".into(),
                    src: if page.title.is_empty() { page.site.clone() } else { page.title.clone() },
                    loc: loc.join(" · "),
                    text: b.text.clone(),
                    translation: None,
                    translation_loc: None,
                    material: saved.as_ref().map(|m| m.uid.clone()),
                    kb: saved.as_ref().map(|m| m.kb.clone()),
                    passage: None,
                    item: None,
                    url: Some(page.final_url.clone()),
                    web_page: Some(page.id),
                    key: Some(*id),
                });
            }
        }
    }
    Ok(out)
}

/// 页边注的编号顺序
pub fn order_of(cites: &[Cite]) -> Vec<Handle> {
    cites.iter().filter_map(|x| x.key).collect()
}

/// 一段回答里的编号：查库、换成 [n]，返回（换好的文字, 页边注）
#[cfg(test)]
pub fn resolve(c: &Connection, text: &str) -> Result<(String, Vec<Cite>), String> {
    let cites = cites(c, &handles(text))?;
    Ok((number(text, &order_of(&cites)), cites))
}

/// 存进知识库的正文：编号换成 [n]，后面附上出处
pub fn with_sources(text: &str, cites: &[Cite]) -> String {
    let mut out = number(text, &order_of(cites));
    let used: Vec<&Cite> = cites.iter().filter(|x| out.contains(&format!("[{}]", x.n))).collect();
    if !used.is_empty() {
        out.push_str("\n\n出处：");
        for x in used {
            let src = if x.kind == "网页" { format!("「{}」", x.src) } else { x.src.clone() };
            let url = x.url.as_ref().map(|u| format!(" {u}")).unwrap_or_default();
            out.push_str(&format!("\n[{}] {src} {}{url}", x.n, x.loc));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::Handle::{P, W};
    use super::*;

    #[test]
    fn handles_become_numbers_in_order() {
        let t = "分工靠交换 [P12]，市场小就分不细【P34】。又见 [P12, W56] 和 [W56][P99]。这里 [1] 和 [P] 不动，[P7 不完整。";
        assert_eq!(handles(t), [P(12), P(34), W(56), P(99)]);
        assert_eq!(number(t, &[P(12), P(34), W(56), P(99)]), "分工靠交换 [1]，市场小就分不细[2]。又见 [1][3] 和 [3][4]。这里 [1] 和 [P] 不动，[P7 不完整。");
        // 不认识的那一处原样留着；P5 和 W5 是两个编号
        assert_eq!(number("甲 [P12] 乙 [P5, P12] 丙 [W12]", &[P(12)]), "甲 [1] 乙 [P5, P12] 丙 [W12]");
        assert_eq!(number_live("还在写 [P8]，接着 [w3]"), "还在写 [1]，接着 [2]");
        assert_eq!(handles("[p7]；[ P8 ]"), [P(7), P(8)]);
        assert_eq!(W(3).to_string(), "W3");
    }
}
