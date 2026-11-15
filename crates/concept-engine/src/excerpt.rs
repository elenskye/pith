//! 来源太长时挑片段（v0.5，修 `decisions/0025` 的已知问题「反方每个来源只发前 1.2 万字」）。
//!
//! 一篇长文章里真正评论这本书的地方常常在后面。超过 `prompts::SOURCE_CHARS` 的来源不再只取开头，而是：
//! 1. 按段落切成约 900 字的小块（原样的连续片段，不改一个字）；
//! 2. 按和「这次研究在找什么」（目标卡片的标题和句子、检索词、问题和要求）的字面重合给每块打分（中文按相邻两字、西文按词，少见的词分量重）；
//! 3. 取分数最高的 40 块；App 一侧有本机的重排模型时（`Shelf::relevance`）让它按意思重新打分，没有就用字面的分数；
//! 4. 开头一块总是带上（标题、摘要），再按分数从高到低装到满 1.2 万字；相关的块不多、还有余量时，接着开头往下装；
//! 5. 按原文的顺序排回去，不相邻的片段之间写「……（中间略）……」。
//!
//! 发给模型的是片段，核对「他人」的引文照旧用全文（片段是全文里原样的连续文字，跨着省略号的引文核对不上，正好拦下）。
//! 一块都没有字面重合时退回只取开头（和以前一样）。

use std::collections::{HashMap, HashSet};

use crate::flow::{self, Shelf};
use crate::model::Run;
use crate::prompts::{SOURCE_CHARS, SourceText};
use crate::text::is_cjk;

pub const CHUNK_CHARS: usize = 900;
/// 送去按意思重排的块数上限
pub const CANDIDATES: usize = 40;
/// 重排模型低于这个分的算不相关
const FLOOR: f32 = 0.01;
const GAP: &str = "\n……（中间略）……\n";

/// 按意思打分：`rank(问题, 候选块)` 返回和候选块一一对应的分数，没有重排模型就返回 None
pub type Rank<'a> = &'a dyn Fn(&str, &[String]) -> Option<Vec<f32>>;

/// 这次研究在找什么：目标卡片的标题和句子、检索词、问题、要求
pub fn focus(run: &Run) -> String {
    let mut parts: Vec<String> = flow::search_terms(run);
    for c in &run.targets {
        parts.push(c.title.clone());
        parts.extend(c.sentences.iter().map(|s| s.text.clone()));
    }
    if let Some(scope) = &run.scope {
        parts.extend(scope.question.clone());
        parts.extend(scope.instruction.clone());
    }
    parts.join("\n")
}

/// 西文的词（小写，至少 3 个字母或数字）和中文的相邻两字
fn tokens(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut HashSet<String>| {
        if word.chars().count() >= 3 {
            out.insert(std::mem::take(word));
        } else {
            word.clear();
        }
    };
    for (i, c) in chars.iter().enumerate() {
        if is_cjk(*c) {
            flush(&mut word, &mut out);
            if let Some(n) = chars.get(i + 1).filter(|n| is_cjk(**n)) {
                out.insert([*c, *n].iter().collect());
            }
        } else if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else {
            flush(&mut word, &mut out);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// 切成小块：每块是原文里连续的一段（合起来就是原文），尽量在段落、句子的边界上断
pub fn chunks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0; // 当前块的起点（字节）
    let mut count = 0; // 当前块的字数
    let mut last_break: Option<usize> = None; // 当前块里最近一个可以断的地方（字节，断在它后面）
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        count += 1;
        let end = i + c.len_utf8();
        let boundary = c == '\n' || matches!(c, '。' | '！' | '？' | '；') || (matches!(c, '.' | '!' | '?' | ';') && it.peek().is_none_or(|(_, n)| n.is_whitespace()));
        if boundary {
            last_break = Some(end);
        }
        // 段落结尾并且够长了：断；太长了还没到段落结尾：在最近的句子边界断，实在没有就硬断
        let cut = if c == '\n' && count >= CHUNK_CHARS {
            Some(end)
        } else if count >= CHUNK_CHARS * 2 {
            Some(last_break.filter(|b| *b > start).unwrap_or(end))
        } else {
            None
        };
        if let Some(at) = cut {
            out.push(&text[start..at]);
            count = text[at..end].chars().count();
            start = at;
            last_break = None;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// 从一篇长文里挑出最相关的片段，不超过 `budget` 字。`rank(问题, 候选块)` 是按意思打的分（没有就返回 None）
pub fn pick(text: &str, focus: &str, budget: usize, rank: Rank) -> String {
    let total = text.chars().count();
    if total <= budget {
        return text.to_string();
    }
    let head = || text.chars().take(budget).collect::<String>().trim_end().to_string() + "\n（后面略）";
    let parts = chunks(text);
    let want = tokens(focus);
    if parts.len() < 2 || want.is_empty() {
        return head();
    }
    // 字面的分数：重合的词越少见分量越重
    let toks: Vec<HashSet<String>> = parts.iter().map(|p| tokens(p)).collect();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for t in &toks {
        for w in t.iter().filter(|w| want.contains(*w)) {
            *df.entry(w.as_str()).or_default() += 1;
        }
    }
    let n = parts.len() as f32;
    let lexical: Vec<f32> = toks.iter().map(|t| t.iter().filter_map(|w| df.get(w.as_str())).map(|d| (1.0 + n / *d as f32).ln()).sum()).collect();
    let mut cand: Vec<usize> = (1..parts.len()).filter(|i| lexical[*i] > 0.0).collect();
    if cand.is_empty() {
        return head();
    }
    cand.sort_by(|a, b| lexical[*b].total_cmp(&lexical[*a]).then(a.cmp(b)));
    cand.truncate(CANDIDATES);
    // 有重排模型：按意思重新排，明显不相关的不要（全都不相关就还按字面的）
    let passages: Vec<String> = cand.iter().map(|i| parts[*i].to_string()).collect();
    if let Some(scores) = rank(focus, &passages).filter(|s| s.len() == cand.len()) {
        let mut order: Vec<usize> = (0..cand.len()).filter(|k| scores[*k] >= FLOOR).collect();
        if !order.is_empty() {
            order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
            cand = order.into_iter().map(|k| cand[k]).collect();
        }
    }
    // 开头一块总带上，再按分数装到满
    let len = |i: usize| parts[i].chars().count();
    let mut chosen = vec![0usize];
    let mut used = len(0).min(budget);
    let room = budget.saturating_sub(GAP.chars().count() * 2);
    for i in cand {
        if used + len(i) + GAP.chars().count() > room {
            continue;
        }
        used += len(i) + GAP.chars().count();
        chosen.push(i);
    }
    // 相关的块不多、还有余量：接着开头往下装（至少不比「只取开头」少看）
    for i in 1..parts.len() {
        if chosen.contains(&i) {
            continue;
        }
        if used + len(i) > room {
            break;
        }
        used += len(i);
        chosen.push(i);
    }
    chosen.sort_unstable();
    let mut out = format!("（原文约 {total} 字，下面是其中和这次研究最相关的片段，中间有省略）\n");
    for (k, i) in chosen.iter().enumerate() {
        if k > 0 && chosen[k - 1] + 1 != *i {
            out.push_str(GAP);
        }
        let piece: String = if *i == 0 { parts[0].chars().take(budget / 2).collect() } else { parts[*i].to_string() };
        out.push_str(&piece);
    }
    if chosen.last().is_some_and(|l| l + 1 < parts.len()) {
        out.push_str("\n……（后面略）");
    }
    out
}

/// 发给模型之前：太长的来源换成挑出来的片段
pub fn for_prompt(sources: &[SourceText], run: &Run, shelf: &dyn Shelf) -> Vec<SourceText> {
    if sources.iter().all(|s| s.text.chars().count() <= SOURCE_CHARS) {
        return sources.to_vec();
    }
    let focus = focus(run);
    sources.iter().map(|s| SourceText { text: pick(&s.text, &focus, SOURCE_CHARS, &|q, p| shelf.relevance(q, p)), ..s.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_text() -> String {
        let filler = |n: usize, tag: &str| (0..n).map(|i| format!("{tag}第 {i} 段：这里讲的是天气、饮食和旅行，和我们要找的题目没有关系。Nothing here concerns the topic at hand; it is filler about weather and travel.\n")).collect::<String>();
        format!("评论文章的标题和摘要：本文讨论若干话题。\n{}关键的一段：斯密说分工受市场范围的限制，但这位评论者认为交通和制度同样决定分工的程度，这是对「市场范围」论的批评。\n{}结尾。\n", filler(200, "前"), filler(200, "后"))
    }

    #[test]
    fn chunks_cover_the_text_exactly() {
        let t = long_text();
        let c = chunks(&t);
        assert!(c.len() > 20);
        assert_eq!(c.concat(), t);
        assert!(c.iter().all(|p| p.chars().count() <= CHUNK_CHARS * 2 + 1));
        // 没有换行的长文：在句子边界上断，仍然一字不差
        let one = "这是一句话。".repeat(600);
        let c = chunks(&one);
        assert!(c.len() > 1 && c.concat() == one && c.iter().all(|p| p.ends_with('。')));
        assert_eq!(chunks("短"), ["短"]);
        assert!(chunks("").is_empty());
    }

    #[test]
    fn the_relevant_part_is_found_wherever_it_is() {
        let t = long_text();
        assert!(t.chars().count() > SOURCE_CHARS * 2);
        let focus = "劳动分工\n分工受市场范围的限制\nextent of the market";
        let out = pick(&t, focus, SOURCE_CHARS, &|_, _| None);
        assert!(out.chars().count() <= SOURCE_CHARS + 60, "{}", out.chars().count());
        assert!(out.contains("评论文章的标题和摘要") && out.contains("对「市场范围」论的批评") && out.contains("（中间略）"));
        // 片段都是原文里原样的连续文字
        for piece in out.split(GAP.trim()).skip(1) {
            let piece = piece.trim().trim_end_matches("……（后面略）").trim();
            assert!(piece.is_empty() || t.contains(piece), "片段被改过");
        }
        // 以前的做法（只取开头）拿不到这一段
        assert!(!t.chars().take(SOURCE_CHARS).collect::<String>().contains("对「市场范围」论的批评"));
    }

    #[test]
    fn short_texts_and_unrelated_texts_keep_the_old_behaviour() {
        assert_eq!(pick("很短的来源。", "分工", SOURCE_CHARS, &|_, _| None), "很短的来源。");
        let t = long_text();
        let out = pick(&t, "量子计算 qubit", SOURCE_CHARS, &|_, _| None);
        assert!(out.starts_with("评论文章的标题和摘要") && out.ends_with("（后面略）") && !out.contains("中间略"));
        assert!(pick(&t, "", SOURCE_CHARS, &|_, _| None).ends_with("（后面略）"));
    }

    #[test]
    fn long_sources_are_excerpted_for_the_prompt_only() {
        struct Plain;
        impl Shelf for Plain {
            fn find(&self, _: &[String], _: usize, _: &std::sync::atomic::AtomicBool) -> Result<Vec<flow::ShelfHit>, String> {
                Ok(Vec::new())
            }
            fn keep(&self, _: &str) -> Result<String, String> {
                Ok(String::new())
            }
            fn text(&self, _: &crate::model::CommentSource) -> Result<String, String> {
                Err("没有".into())
            }
        }
        let card: crate::model::Card = serde_json::from_value(serde_json::json!({ "id": "K1", "kind": "concept", "title": "市场范围 / extent of the market", "sentences": [], "step": "analyse" })).unwrap();
        let run = Run { targets: vec![card], ..Default::default() };
        let short = SourceText { id: "S1".into(), title: "短评".into(), where_from: "文献库".into(), text: "一篇短评。".into() };
        let long = SourceText { id: "S2".into(), title: "长文".into(), where_from: "https://example.org/a".into(), text: long_text() };
        // 都不长：原样
        assert_eq!(for_prompt(std::slice::from_ref(&short), &run, &Plain), std::slice::from_ref(&short));
        let shown = for_prompt(&[short.clone(), long.clone()], &run, &Plain);
        assert_eq!(shown[0], short);
        assert_eq!((shown[1].id.as_str(), shown[1].title.as_str()), ("S2", "长文"));
        assert!(shown[1].text.chars().count() <= SOURCE_CHARS + 60 && shown[1].text.contains("对「市场范围」论的批评"));
        assert!(long.text.chars().count() > SOURCE_CHARS * 2, "全文没有被改");
    }

    #[test]
    fn a_reranker_decides_among_the_candidates() {
        // 两处都提到「分工」，字面分数接近；重排模型说后一处才相关
        let filler = "无关的内容，讲天气和旅行。\n".repeat(1400);
        let t = format!("摘要。\n{filler}甲处：分工这个词在这里只是顺带一提，说的是家务分工。\n{filler}乙处：评论者反驳斯密的分工理论，认为市场范围不是唯一的限制。\n{filler}");
        let rank = |_: &str, passages: &[String]| Some(passages.iter().map(|p| if p.contains("乙处") { 0.9 } else { 0.0001 }).collect());
        let out = pick(&t, "分工", SOURCE_CHARS, &rank);
        assert!(out.contains("乙处") && !out.contains("甲处"));
        // 重排说全都不相关：还按字面的来
        let none = |_: &str, passages: &[String]| Some(vec![0.0; passages.len()]);
        let out = pick(&t, "分工", SOURCE_CHARS, &none);
        assert!(out.contains("甲处") && out.contains("乙处"));
    }
}
