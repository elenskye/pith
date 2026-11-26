//! 在知识库里找段落（`kb_search` 工具）：按意思（本机向量）+ 按词（FTS）→ 本机重排模型打分 → 前几段。
//!
//! - **范围**：这段对话勾的知识库；没勾就是全部。
//! - **原著和译本**：书有对齐的译本（或原著）时，找到的一段带上对面的那几段（对照译文）；两边都找到的并成一条，原著在前。
//!   中文问题、原著是英文时，重排比的是中译（同一种语言比得准）。
//! - **退路**：本机模型用不了、或者范围里一块向量都还没有 → 只按词；重排用不了 → 按向量的相似度排。都会说明原因。
//!   向量和重排只决定给模型看哪几段；原文、段落号、页码都是存下的，不经模型改写。

use std::collections::HashMap;

use concept_engine::align::Alignment;
use rusqlite::{Connection, OptionalExtension, params};

use super::passages::{self, Passage};
use super::{Db, index, store};
use crate::common::err;
use crate::local::{LocalModels, Svc, embed, rerank};

const QUERY_TASK: &str = "Given a question, retrieve passages from the user's books, notes and reports that help answer it";
const TOP_SEMANTIC: usize = 40;
const TOP_KEYWORD: usize = 24;
const POOL: usize = 64;
/// 给模型看的一段最长多少字（再长的说还有多少，用 kb_read 看全文）
pub const CLIP: usize = 1500;

/// 一份材料的几样（检索结果、页边注要用）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub id: i64,
    pub uid: String,
    pub kind: String,
    pub title: String,
    pub sub: String,
    /// 家（打开知识库时去哪个库）
    pub kb: String,
    /// 书在文献库里的条目（一步到文献库）
    pub item: Option<i64>,
    /// 网页快照的网址
    pub url: Option<String>,
    /// 书是译本（连着原著）
    pub translation: bool,
}

pub fn source(c: &Connection, material: i64) -> Result<Source, String> {
    let (uid, kind, title, sub, kb, sk, sr): (String, String, String, String, String, Option<String>, Option<String>) = c
        .query_row(
            "SELECT m.uid, m.kind, m.title, m.sub, k.uid, m.source_kind, m.source_ref FROM kb_materials m JOIN knowledge_bases k ON k.id = m.kb_id WHERE m.id = ?1",
            [material],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .map_err(err)?;
    let item = match (sk.as_deref(), sr.as_deref()) {
        (Some("concept_text"), Some(r)) => c.query_row("SELECT item_id FROM concept_texts WHERE uid = ?1", [r], |r| r.get::<_, Option<i64>>(0)).optional().map_err(err)?.flatten(),
        (Some("item"), Some(r)) => r.parse().ok(),
        _ => None,
    };
    let url = match (sk.as_deref(), sr.as_deref()) {
        (Some("web_page"), Some(r)) => c.query_row("SELECT final_url FROM web_pages WHERE id = ?1", [r], |r| r.get::<_, String>(0)).optional().map_err(err)?,
        _ => None,
    };
    let translation = match (sk.as_deref(), sr.as_deref()) {
        (Some("concept_text"), Some(r)) => c.query_row("SELECT original_id IS NOT NULL FROM concept_texts WHERE uid = ?1", [r], |r| r.get::<_, bool>(0)).optional().map_err(err)?.unwrap_or(false),
        _ => false,
    };
    Ok(Source { id: material, uid, kind, title, sub, kb, item, url, translation })
}

/// 书的对面（原著 ↔ 译本）：对面的材料、对齐、这一边是不是原著
pub struct Partner {
    pub material: i64,
    pub alignment: Alignment,
    pub original: bool,
}

impl Partner {
    /// 这一边的 seq 对着对面的哪几段
    pub fn other(&self, seq: i64) -> Vec<i64> {
        let s = seq as usize;
        let v = if self.original { self.alignment.other_of(s) } else { self.alignment.original_of(s) };
        v.into_iter().map(|x| x as i64).collect()
    }
}

/// 材料是书、有对面的书也在知识库里、对齐过：给出对面
pub fn partner(c: &Connection, material: i64) -> Result<Option<Partner>, String> {
    let (sk, sr): (Option<String>, Option<String>) = c.query_row("SELECT source_kind, source_ref FROM kb_materials WHERE id = ?1", [material], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
    let (Some("concept_text"), Some(uid)) = (sk.as_deref(), sr) else { return Ok(None) };
    if crate::concept::store::text_id(c, &uid).is_err() {
        return Ok(None);
    }
    let Some((other, other_is_translation)) = crate::concept::store::partner(c, &uid)? else { return Ok(None) };
    let Some(other_material) = store::by_source(c, "concept_text", &other)? else { return Ok(None) };
    let (o, t) = if other_is_translation { (uid.clone(), other.clone()) } else { (other.clone(), uid.clone()) };
    let alignment = match crate::concept::store::alignment_load(c, &o, &t)? {
        Some(a) => a,
        None => {
            let (ot, tt) = (crate::concept::store::text_load(c, &o)?, crate::concept::store::text_load(c, &t)?);
            crate::concept::store::alignment_ensure(c, &o, &ot, &t, &tt, &crate::text::now_iso())?
        }
    };
    Ok(Some(Partner { material: store::material_id(c, &other_material)?, alignment, original: other_is_translation }))
}

/// 一段对面的那几段
pub fn pair_of(c: &Connection, p: &Passage, partners: &mut HashMap<i64, Option<Partner>>) -> Result<Vec<Passage>, String> {
    if !partners.contains_key(&p.material_id) {
        partners.insert(p.material_id, partner(c, p.material_id)?);
    }
    let Some(Some(x)) = partners.get(&p.material_id) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for seq in x.other(p.seq) {
        if let Some(q) = passages::at(c, x.material, seq)? {
            out.push(q);
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub passage: Passage,
    pub source: Source,
    /// 对照译文（或原著）
    pub pair: Vec<Passage>,
    pub pair_source: Option<Source>,
    /// 相关度：重排的分数；重排用不了时是向量的相似度；只按词时没有
    pub score: Option<f32>,
    /// semantic / keyword / both
    pub via: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub hits: Vec<Hit>,
    /// semantic：按意思找的；keyword：只按词
    pub mode: &'static str,
    pub reranked: bool,
    /// 给模型和站主看的一句说明（为什么只按词、索引建到哪了）
    pub note: Option<String>,
    /// 范围里有几份材料
    pub materials: usize,
}

/// 范围（知识库 uid → 材料 id）；没给就是全部（空）
pub fn scope(c: &Connection, kbs: &[String]) -> Result<Vec<i64>, String> {
    let mut out: Vec<i64> = Vec::new();
    for kb in kbs {
        let Ok(id) = store::kb_id(c, kb) else { continue };
        let mut st = c.prepare("SELECT material_id FROM kb_members WHERE kb_id = ?1").map_err(err)?;
        for m in st.query_map([id], |r| r.get::<_, i64>(0)).map_err(err)? {
            let m = m.map_err(err)?;
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    Ok(out)
}

fn count(c: &Connection, scope: &[i64]) -> Result<usize, String> {
    if !scope.is_empty() {
        return Ok(scope.len());
    }
    c.query_row("SELECT COUNT(*) FROM kb_materials", [], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(err)
}

fn vector_count(c: &Connection, model: &str, scope: &[i64]) -> Result<usize, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS search.kb_vecs (passage_id INTEGER PRIMARY KEY, model TEXT NOT NULL, sig INTEGER NOT NULL, vec BLOB NOT NULL)").map_err(err)?;
    let s = if scope.is_empty() { String::new() } else { format!(" AND p.material_id IN ({})", scope.iter().map(i64::to_string).collect::<Vec<_>>().join(",")) };
    c.query_row(&format!("SELECT COUNT(*) FROM search.kb_vecs v JOIN passages p ON p.id = v.passage_id WHERE v.model = ?1{s}"), [model], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(err)
}

/// 一条候选：主的那段（原著在前）、对面的、相似度、怎么找到的
struct Cand {
    primary: Passage,
    pair: Vec<Passage>,
    sim: f32,
    semantic: bool,
    keyword: bool,
}

/// 在知识库里找。`kbs`：勾的知识库（空 = 全部）
pub fn search(db: &impl Db, local: &LocalModels, query: &str, kbs: &[String], limit: usize) -> Result<Found, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("要给搜索词。".into());
    }
    let limit = limit.clamp(1, 20);
    let (scope, materials) = db.with(|c| {
        let s = scope(c, kbs)?;
        Ok((s.clone(), count(c, &s)?))
    })?;
    if materials == 0 || (!kbs.is_empty() && scope.is_empty()) {
        return Ok(Found { mode: "keyword", note: Some("勾的知识库里还没有材料。".into()), ..Default::default() });
    }
    let mut note: Option<String> = None;
    let keyword = db.with(|c| index::keyword(c, q, &scope, TOP_KEYWORD))?;
    // 按意思：本机向量模型能用、范围里有向量
    let mut semantic: Vec<(i64, f32)> = Vec::new();
    let mut mode = "keyword";
    match local.model(Svc::Embedding) {
        Err(e) => note = Some(format!("{e}这次只按词找。")),
        Ok(m) => {
            let (have, (todo, total)) = db.with(|c| Ok((vector_count(c, &m.key, &scope)?, index::missing(c, &m.key)?)))?;
            if have == 0 {
                note = Some("知识库的语义索引还没建好，这次只按词找；建好以后就是按意思找。".into());
            } else {
                match local.ensure(Svc::Embedding).and_then(|m| embed::embed(local, &m, &[q.to_string()], Some(QUERY_TASK)).map(|mut v| (m, v.remove(0)))) {
                    Ok((m, qv)) => {
                        semantic = db.with(|c| index::nearest(c, &m.key, &qv, &scope, TOP_SEMANTIC))?;
                        mode = "semantic";
                        if !todo.is_empty() {
                            note = Some(format!("语义索引还在建（{} / {} 段），没建到的这次只能按词找到。", total - todo.len(), total));
                        }
                    }
                    Err(e) => note = Some(format!("{e}这次只按词找。")),
                }
            }
        }
    }

    // 候选：书的译本那一边映射回原著，两边都找到的并成一条
    let pool: Vec<Cand> = db.with(|c| {
        let mut partners: HashMap<i64, Option<Partner>> = HashMap::new();
        let mut pool: Vec<Cand> = Vec::new();
        let mut add = |c: &Connection, id: i64, sim: f32, sem: bool, pool: &mut Vec<Cand>| -> Result<(), String> {
            let Some(p) = passages::get(c, id)? else { return Ok(()) };
            let pair = pair_of(c, &p, &mut partners)?;
            // 译本里找到的：主的换成原著那几段的第一段
            let is_original = partners.get(&p.material_id).and_then(|x| x.as_ref()).is_none_or(|x| x.original);
            let (primary, pair) = if !is_original && !pair.is_empty() {
                let first = pair[0].clone();
                let back = pair_of(c, &first, &mut partners)?;
                (first, if back.is_empty() { vec![p] } else { back })
            } else {
                (p, pair)
            };
            match pool.iter().position(|x| x.primary.id == primary.id) {
                Some(i) => {
                    let x = &mut pool[i];
                    x.sim = x.sim.max(sim);
                    x.semantic |= sem;
                    x.keyword |= !sem;
                }
                None if pool.len() < POOL => pool.push(Cand { primary, pair, sim, semantic: sem, keyword: !sem }),
                None => {}
            }
            Ok(())
        };
        for (id, s) in &semantic {
            add(c, *id, *s, true, &mut pool)?;
        }
        for (id, _) in &keyword {
            add(c, *id, 0.0, false, &mut pool)?;
        }
        Ok(pool)
    })?;
    if pool.is_empty() {
        return Ok(Found { mode, note, materials, ..Default::default() });
    }

    // 重排：中文问题、主的那段不是中文而对面是中文时比对面
    let cjk = |s: &str| s.chars().any(concept_engine::text::is_cjk);
    let texts: Vec<String> = pool.iter().map(|x| if cjk(q) && !cjk(&x.primary.text) && x.pair.iter().any(|p| cjk(&p.text)) { x.pair.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n") } else { x.primary.text.clone() }).collect();
    let ranked = local.ensure(Svc::Rerank).and_then(|m| rerank::scores(local, &m, q, &texts));
    let (order, scores, reranked): (Vec<usize>, Option<Vec<f32>>, bool) = match ranked {
        Ok(s) => {
            let mut o = rerank::order(&s, limit);
            if o.is_empty() {
                o = (0..pool.len().min(limit.min(6))).collect();
            }
            (o, Some(s), true)
        }
        Err(e) => {
            if mode == "semantic" {
                let more = format!("{e}这次只按向量的相似度排。");
                note = Some(match note {
                    Some(n) => format!("{more}{n}"),
                    None => more,
                });
            }
            let mut idx: Vec<usize> = (0..pool.len()).collect();
            if mode == "semantic" {
                idx.sort_by(|a, b| pool[*b].sim.total_cmp(&pool[*a].sim).then(a.cmp(b)));
            }
            idx.truncate(limit);
            (idx, None, false)
        }
    };
    let hits = db.with(|c| {
        let mut out = Vec::new();
        let mut sources: HashMap<i64, Source> = HashMap::new();
        let mut src = |c: &Connection, id: i64| -> Result<Source, String> {
            if let Some(s) = sources.get(&id) {
                return Ok(s.clone());
            }
            let s = source(c, id)?;
            sources.insert(id, s.clone());
            Ok(s)
        };
        for i in order {
            let x = &pool[i];
            let score = match &scores {
                Some(s) => s.get(i).copied(),
                None if mode == "semantic" && x.semantic => Some(x.sim),
                None => None,
            };
            let via = match (x.semantic, x.keyword) {
                (true, true) => "both",
                (true, false) => "semantic",
                _ => "keyword",
            };
            let pair_source = match x.pair.first() {
                Some(p) => Some(src(c, p.material_id)?),
                None => None,
            };
            out.push(Hit { passage: x.primary.clone(), source: src(c, x.primary.material_id)?, pair: x.pair.clone(), pair_source, score, via });
        }
        Ok(out)
    })?;
    Ok(Found { hits, mode, reranked, note, materials })
}

// ---------- 写给模型看 ----------

/// 一段的出处：段落号 · 第几页（没有段落号的写章名）
pub fn locator(p: &Passage) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !p.key.is_empty() {
        parts.push(p.key.clone());
    } else if !p.unit_label.is_empty() {
        parts.push(p.unit_label.clone());
    }
    if let Some(pg) = p.page.as_ref().filter(|s| !s.is_empty()) {
        parts.push(format!("第 {pg} 页"));
    }
    match p.role.as_str() {
        "sidenote" => parts.push("旁注".into()),
        "note" => parts.push("注释".into()),
        "guide" => parts.push("导读".into()),
        _ => {}
    }
    parts.join(" · ")
}

pub fn clip(text: &str, n: usize) -> String {
    let len = text.chars().count();
    if len <= n { text.to_string() } else { format!("{}……（这一段还有 {} 字，kb_read 看全文）", text.chars().take(n).collect::<String>(), len - n) }
}

pub fn title_line(s: &Source) -> String {
    let mut t = format!("《{}》", s.title);
    if !s.sub.is_empty() {
        t.push_str(&format!(" {}", s.sub));
    }
    t
}

/// 检索结果写成给模型看的文字：每段前面是它的编号 [P123]
pub fn format(q: &str, f: &Found) -> String {
    if f.hits.is_empty() {
        let mut s = format!("知识库里没有找到和「{q}」有关的段落（范围里 {} 份材料）。", f.materials);
        if let Some(n) = &f.note {
            s.push_str(&format!("\n{n}"));
        }
        return s;
    }
    let how = match (f.mode, f.reranked) {
        ("semantic", true) => "按意思和词找、重排过",
        ("semantic", false) => "按意思和词找",
        (_, true) => "按词找、重排过",
        _ => "按词找",
    };
    let mut out = format!("知识库里和「{q}」最相关的 {} 段（{how}）：\n", f.hits.len());
    if let Some(n) = &f.note {
        out.push_str(&format!("（{n}）\n"));
    }
    for h in &f.hits {
        let unit = if h.passage.unit_label.is_empty() || h.passage.key.is_empty() { String::new() } else { format!(" · {}", h.passage.unit_label) };
        out.push_str(&format!("\n[P{}] {} [材料 {}] · {}{unit}\n{}\n", h.passage.id, title_line(&h.source), h.source.id, locator(&h.passage), clip(&h.passage.text, CLIP)));
        if let (Some(first), Some(ps)) = (h.pair.first(), &h.pair_source) {
            let handles = h.pair.iter().map(|p| format!("P{}", p.id)).collect::<Vec<_>>().join(", ");
            let text = h.pair.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n");
            out.push_str(&format!("  对照 [{handles}] 《{}》 {}：{}\n", ps.title, locator(first), clip(&text, CLIP / 2)));
        }
    }
    out.push_str("\n（写到这些段落时在句子后面写上编号，如 [P123]，程序会换成 [1] 并在页边给出原文。要看上下文、整章、对照译文用 kb_read。）");
    out
}

/// 范围里有哪些材料（搜索词空着时给模型看）
pub fn list_materials(c: &Connection, kbs: &[String]) -> Result<String, String> {
    let scope = scope(c, kbs)?;
    if !kbs.is_empty() && scope.is_empty() {
        return Ok("勾的知识库里还没有材料。".into());
    }
    let mut st = c.prepare("SELECT m.id, m.kind, m.title, m.sub, (SELECT COUNT(*) FROM passages p WHERE p.material_id = m.id AND p.role IN ('body','sidenote','note','guide')) FROM kb_materials m ORDER BY m.tended_at DESC, m.id DESC").map_err(err)?;
    let rows: Vec<(i64, String, String, String, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let rows: Vec<_> = rows.into_iter().filter(|r| scope.is_empty() || scope.contains(&r.0)).collect();
    if rows.is_empty() {
        return Ok("知识库里还没有材料。".into());
    }
    let mut out = format!("{}里有 {} 份材料：\n", if kbs.is_empty() { "知识库" } else { "勾的知识库" }, rows.len());
    for (id, kind, title, sub, n) in rows.iter().take(60) {
        let sub = if sub.is_empty() { String::new() } else { format!(" {sub}") };
        out.push_str(&format!("[材料 {id}] {} 《{title}》{sub} · {n} 段\n", kind_name(kind)));
    }
    if rows.len() > 60 {
        out.push_str(&format!("……还有 {} 份没列出来。\n", rows.len() - 60));
    }
    out.push_str("（kb_read 加 material 看一份材料的目录；kb_search 给搜索词找段落。）");
    Ok(out)
}

pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        "full" => "书",
        "excerpt" => "摘抄",
        "note" => "笔记",
        "web" => "网页快照",
        "file" => "本机文件",
        "scan" => "扫描件",
        "concept" => "概念卡",
        "report" => "调查报告",
        "wonder" => "WonderLab",
        _ => "存下的回答",
    }
}

/// 一份材料的 id（kb_read 的 material：「材料 12」里的数）存在不存在
pub fn material_exists(c: &Connection, id: i64) -> Result<bool, String> {
    c.query_row("SELECT 1 FROM kb_materials WHERE id = ?1", params![id], |_| Ok(())).optional().map(|x| x.is_some()).map_err(err)
}
