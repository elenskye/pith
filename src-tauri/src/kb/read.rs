//! 读原文（`kb_read` 工具、知识库面板里的「原文」、子代理读整章）。
//!
//! - 一段的上下文：同一章里前后各几段；
//! - 整章：太长的只给前一部分（按「大约多少 token」算，中文一个字算一个），说还剩多少、整章交给子代理；
//! - 一份材料：目录（每章第一段的编号）；短的直接给全文；
//! - 对照译文：书有对齐的译本（或原著）时每段带上对面的那几段。

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use super::passages::{self, Passage, UnitInfo};
use super::retrieve::{self, Partner};

/// 给主对话的整章最多这么多（约 token）；子代理读整章最多这么多
pub const CHAPTER_BUDGET: usize = 9_000;
pub const SUB_BUDGET: usize = 60_000;
/// 短的材料不给目录，直接给全文
const WHOLE_BUDGET: usize = 3_000;

/// 大约多少 token：中日韩一个字算一个，别的四个字符算一个
pub fn weight(text: &str) -> usize {
    let (mut cjk, mut other) = (0usize, 0usize);
    for ch in text.chars() {
        if concept_engine::text::is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

/// 解析编号：「P123」「p123」「123」
pub fn handle(s: &str) -> Option<i64> {
    let t = s.trim().trim_start_matches(['[', '【']).trim_end_matches([']', '】']).trim();
    t.strip_prefix(['P', 'p']).unwrap_or(t).parse().ok()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Req {
    pub passage: Option<i64>,
    pub material: Option<i64>,
    pub chapter: Option<String>,
    /// context / chapter
    pub scope: String,
    pub around: i64,
    pub translation: bool,
}

fn line(p: &Passage, pairs: &[Passage]) -> String {
    let head = match p.role.as_str() {
        "heading" => return format!("## {}", p.text),
        "matter" => return String::new(),
        _ => retrieve::locator(p),
    };
    let mut s = format!("[P{}] {head}\n{}", p.id, p.text);
    if !pairs.is_empty() {
        let handles = pairs.iter().map(|q| format!("P{}", q.id)).collect::<Vec<_>>().join(", ");
        s.push_str(&format!("\n  对照 [{handles}]：{}", pairs.iter().map(|q| q.text.as_str()).collect::<Vec<_>>().join("\n")));
    }
    s
}

/// 一串段落写成文字，到 `budget` 为止。返回（文字, 没写进去的段数, 没写进去的大约 token, 下一段）
fn render(c: &Connection, ps: &[Passage], translation: bool, budget: usize) -> Result<(String, usize, usize, Option<i64>), String> {
    let mut partners: HashMap<i64, Option<Partner>> = HashMap::new();
    let mut out: Vec<String> = Vec::new();
    let mut used = 0;
    for (i, p) in ps.iter().enumerate() {
        let pairs = if translation && passages::searchable(&p.role) { retrieve::pair_of(c, p, &mut partners)? } else { Vec::new() };
        let l = line(p, &pairs);
        if l.is_empty() {
            continue;
        }
        let w = weight(&l);
        if used + w > budget && !out.is_empty() {
            let rest = &ps[i..];
            return Ok((out.join("\n\n"), rest.iter().filter(|p| passages::searchable(&p.role)).count(), rest.iter().map(|p| weight(&p.text)).sum(), Some(p.id)));
        }
        used += w;
        out.push(l);
    }
    Ok((out.join("\n\n"), 0, 0, None))
}

fn outline_text(units: &[UnitInfo]) -> String {
    units
        .iter()
        .map(|u| {
            let name = match (u.key.is_empty(), u.label.is_empty()) {
                (false, false) if u.key != u.label => format!("{} {}", u.key, u.label),
                (false, _) => u.key.clone(),
                (true, false) => u.label.clone(),
                _ => "（正文）".into(),
            };
            format!("- {name}：{} 段，约 {} 字，从 [P{}] 起", u.count, u.chars, u.first)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn whole_unit(c: &Connection, p: &Passage) -> Result<Vec<Passage>, String> {
    passages::unit(c, p.material_id, &p.unit_key, &p.unit_label)
}

/// `kb_read`：写成给模型看的文字
pub fn read(c: &Connection, r: &Req) -> Result<String, String> {
    if let Some(id) = r.passage {
        let p = passages::get(c, id)?.ok_or_else(|| format!("没有编号 P{id} 的段落。编号是 kb_search、kb_read 给的。"))?;
        let src = retrieve::source(c, p.material_id)?;
        let head = format!("{} [材料 {}]", retrieve::title_line(&src), src.id);
        if r.scope == "chapter" {
            let all = whole_unit(c, &p)?;
            let (text, left, tokens, next) = render(c, &all, r.translation, CHAPTER_BUDGET)?;
            let name = if p.unit_label.is_empty() { "这一份".to_string() } else { format!("「{}」", p.unit_label) };
            let mut out = format!("{head} 的{name}（{} 段）：\n\n{text}", all.iter().filter(|p| passages::searchable(&p.role)).count());
            if let Some(n) = next {
                out.push_str(&format!("\n\n（这一章还有 {left} 段、约 {tokens} token 没给出。要整章读，用 delegate 派子代理，chapter 写 P{id}；或者 kb_read 从 P{n} 接着读上下文。）"));
            }
            return Ok(out);
        }
        let n = r.around.clamp(1, 8);
        let ps = passages::around(c, &p, n)?;
        let (text, ..) = render(c, &ps, r.translation, CHAPTER_BUDGET)?;
        return Ok(format!("{head} {} 的上下文（前后各 {n} 段）：\n\n{text}", retrieve::locator(&p)));
    }
    let Some(m) = r.material else {
        return Err("要给 passage（段落编号，如 P123）或者 material（材料号）。".into());
    };
    if !retrieve::material_exists(c, m)? {
        return Err(format!("没有材料 {m}。材料号是 kb_search 给的（[材料 12] 里的数）。"));
    }
    let src = retrieve::source(c, m)?;
    let head = format!("{} [材料 {}]", retrieve::title_line(&src), src.id);
    let units = passages::outline(c, m)?;
    if let Some(ch) = r.chapter.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let Some(u) = units.iter().find(|u| u.key == ch || u.label == ch).or_else(|| units.iter().find(|u| u.label.contains(ch))) else {
            return Ok(format!("{head} 里没有「{ch}」这一章。目录：\n{}", outline_text(&units)));
        };
        let first = passages::get(c, u.first)?.ok_or("段落不在了。")?;
        return read(c, &Req { passage: Some(first.id), material: None, chapter: None, scope: "chapter".into(), around: r.around, translation: r.translation });
    }
    let all = passages::all(c, m)?;
    let total: usize = all.iter().map(|p| weight(&p.text)).sum();
    if total <= WHOLE_BUDGET || units.len() <= 1 {
        let (text, left, tokens, next) = render(c, &all, r.translation, CHAPTER_BUDGET)?;
        let mut out = format!("{head} 全文：\n\n{text}");
        if let Some(n) = next {
            out.push_str(&format!("\n\n（后面还有 {left} 段、约 {tokens} token 没给出，kb_read 从 P{n} 接着读。）"));
        }
        return Ok(out);
    }
    Ok(format!("{head} 的目录（{} 章；读一章用 kb_read 加 material 和 chapter，或者 passage 加 scope=chapter）：\n{}", units.len(), outline_text(&units)))
}

/// 子代理读整章：这一章的文字（带编号），到 `SUB_BUDGET` 为止
pub fn chapter_for_subagent(c: &Connection, id: i64, translation: bool) -> Result<(String, String), String> {
    let p = passages::get(c, id)?.ok_or_else(|| format!("没有编号 P{id} 的段落。"))?;
    let src = retrieve::source(c, p.material_id)?;
    let all = whole_unit(c, &p)?;
    let (text, left, _, _) = render(c, &all, translation, SUB_BUDGET)?;
    let name = if p.unit_label.is_empty() { src.title.clone() } else { p.unit_label.clone() };
    let mut out = format!("{}「{name}」（每段前面是编号，报告里引用原文时照样写上编号，如 [P{id}]）：\n\n{text}", retrieve::title_line(&src));
    if left > 0 {
        out.push_str(&format!("\n\n（这一章太长，后面还有 {left} 段没给出。）"));
    }
    Ok((name, out))
}

// ---------- 知识库面板 ----------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub units: Vec<UnitInfo>,
    /// 现在这一章（目录里的下标）
    pub unit: usize,
    pub passages: Vec<Passage>,
    /// 要看的那一段
    pub focus: Option<i64>,
    /// 对照：这一章每段对面的文字（块 id → 对面的几段连起来）
    pub pairs: HashMap<i64, String>,
    pub pair_title: Option<String>,
}

/// 面板里读一份材料：给了段落就读它那一章，给了章（目录下标）就读那一章，都没给读第一章
pub fn view(c: &Connection, material: i64, unit: Option<usize>, focus: Option<i64>) -> Result<View, String> {
    let units = passages::outline(c, material)?;
    let focus_p = match focus {
        Some(id) => passages::get(c, id)?.filter(|p| p.material_id == material),
        None => None,
    };
    let at = match (&focus_p, unit) {
        (Some(p), _) => units.iter().position(|u| u.key == p.unit_key && u.label == p.unit_label).unwrap_or(0),
        (None, Some(u)) => u.min(units.len().saturating_sub(1)),
        _ => 0,
    };
    let ps = match units.get(at) {
        Some(u) => passages::get(c, u.first)?.map(|p| whole_unit(c, &p)).transpose()?.unwrap_or_default(),
        None => passages::all(c, material)?,
    };
    let mut partners: HashMap<i64, Option<Partner>> = HashMap::new();
    let mut pairs = HashMap::new();
    let mut pair_title = None;
    for p in ps.iter().filter(|p| passages::searchable(&p.role)) {
        let other = retrieve::pair_of(c, p, &mut partners)?;
        if let Some(first) = other.first() {
            if pair_title.is_none() {
                pair_title = Some(retrieve::source(c, first.material_id)?.title);
            }
            pairs.insert(p.id, other.iter().map(|q| q.text.as_str()).collect::<Vec<_>>().join("\n"));
        }
    }
    Ok(View { units, unit: at, passages: ps, focus: focus_p.map(|p| p.id), pairs, pair_title })
}
