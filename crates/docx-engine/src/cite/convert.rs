//! Word 外把 .docx 里的 Zotero 引文换成 Pith 引文（`decisions/0017`）。调用方写新文件，原文件不动。
//!
//! - 引文域（同一段里的 begin … end）→ 行内引文控件 `pith:cite:<新 ID>`；
//! - 书目域（通常跨很多段）→ 块级书目控件 `pith:bibliography`；书目所在段落里域外面的内容（比如分页符）留在旁边的段落里；
//! - 然后按 Pith 数据刷新一遍（排版、书目、数据部件）；
//! - 写完用 `check`（另一套解析器）核对：Zotero 域以外的文字、别的域、其他部件都没变，引文与书目就是计划里的样子。
//!
//! 没把握的结构一律不动、只报告：域跨了别的元素、域里有书签或修订、读不懂的域代码。

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::check;
use super::docx::{self, build_run, is_w, q, tag_of};
use super::plan::{plan_refresh, referenced_items, Library, Plan, Render};
use super::store::DocStore;
use super::zotero::{self, FieldKind, ItemKey, Stats};
use super::{is_ours, Run, BIB_TAG, CITE_PREFIX};
use crate::xml::{unescape, El, Node};

const CUSTOM_PROPS: &str = "docProps/custom.xml";
const PLACEHOLDER: &str = "[…]";
const BIB_PLACEHOLDER: &str = "（参考文献生成中）";

fn fld_type(run: &El, w: &str) -> Option<String> {
    run.els().find(|e| is_w(e, w, "fldChar")).and_then(|e| e.attr(&q(w, "fldCharType"))).map(str::to_string)
}

fn raw_text(el: &El) -> String {
    el.children.iter().filter_map(|n| if let Node::Raw(r) = n { Some(unescape(r)) } else { None }).collect()
}

fn instr(run: &El, w: &str) -> String {
    run.els().filter(|e| is_w(e, w, "instrText")).map(raw_text).collect()
}

fn has_fld(el: &El, w: &str) -> bool {
    el.has_descendant(&|e| is_w(e, w, "fldChar"))
}

/// 一个 run（或别的行内元素）里看得见的文字
fn visible_text(el: &El, w: &str, out: &mut String) {
    for c in el.els() {
        if is_w(c, w, "t") {
            out.push_str(&raw_text(c));
        } else if is_w(c, w, "tab") {
            out.push('\t');
        } else if !is_w(c, w, "instrText") && !is_w(c, w, "rPr") {
            visible_text(c, w, out);
        }
    }
}

/// 域里允许出现、替换时可以丢掉的非 run 元素：拼写标记、首尾都在域里的书签
#[derive(Default)]
struct Inside {
    starts: BTreeSet<String>,
    ends: BTreeSet<String>,
    bad: bool,
}

impl Inside {
    fn see(&mut self, e: &El, w: &str) {
        if is_w(e, w, "proofErr") {
        } else if is_w(e, w, "bookmarkStart") {
            self.starts.insert(e.attr(&q(w, "id")).unwrap_or("").to_string());
        } else if is_w(e, w, "bookmarkEnd") {
            self.ends.insert(e.attr(&q(w, "id")).unwrap_or("").to_string());
        } else {
            self.bad = true;
        }
    }

    fn ok(&self) -> bool {
        !self.bad && self.starts == self.ends
    }
}

/// 从第 i 个子元素（begin）开始，在同一层找到配对的 end：返回 (end 的位置, 域代码)。
/// 域结构伸进别的元素、或夹着别的东西时返回 None（不动它）。
fn scan_field(children: &[Node], i: usize, w: &str) -> Option<(usize, String)> {
    let (mut depth, mut sep, mut code, mut inside) = (0usize, false, String::new(), Inside::default());
    for (j, n) in children.iter().enumerate().skip(i) {
        let Node::El(e) = n else { continue };
        if is_w(e, w, "r") {
            match fld_type(e, w).as_deref() {
                Some("begin") => depth += 1,
                Some("separate") => sep |= depth == 1,
                Some("end") => {
                    depth -= 1;
                    if depth == 0 {
                        return inside.ok().then_some((j, code));
                    }
                }
                _ if depth == 1 && !sep => code.push_str(&instr(e, w)),
                _ => {}
            }
        } else if has_fld(e, w) {
            return None;
        } else {
            inside.see(e, w);
        }
    }
    None
}

/// 文档顺序遍历「begin 与 end 在同一层」的域；`f` 拿到 Zotero 引文域的代码与它的全部节点，返回替换用的元素（或不换）。
/// 两遍（先收集、后替换）用同一个遍历，顺序一致。
fn walk_inline(el: &mut El, w: &str, f: &mut dyn FnMut(&str, &[Node]) -> Option<El>) {
    let mut i = 0;
    while i < el.children.len() {
        let begin = matches!(&el.children[i], Node::El(c) if is_w(c, w, "r") && fld_type(c, w).as_deref() == Some("begin"));
        if begin && let Some((end, code)) = scan_field(&el.children, i, w) {
            if zotero::field_kind(&code) == Some(FieldKind::Citation)
                && let Some(new) = f(&code, &el.children[i..=end])
            {
                el.children.splice(i..=end, [Node::El(new)]);
                i += 1;
            } else {
                // 别的域（或不换的）整个跳过，不进里面找
                i = end + 1;
            }
            continue;
        }
        if let Node::El(c) = &mut el.children[i]
            && !(is_w(c, w, "sdt") && is_ours(&tag_of(c, w)))
        {
            walk_inline(c, w, f);
        }
        i += 1;
    }
}

/// 一个 Zotero 书目域：从第 a 段的第 bi 个子元素开始，到第 b 段的第 ei 个子元素结束
struct BibSpan {
    a: usize,
    bi: usize,
    b: usize,
    ei: usize,
    /// Zotero 书目的各段文字
    texts: Vec<String>,
    /// 显示结果里第一个有文字的 run 的格式（新书目沿用字体、字号）
    base_rpr: Option<El>,
}

fn find_bib(children: &[Node], a: usize, w: &str) -> Option<BibSpan> {
    let Node::El(pa) = &children[a] else { return None };
    if !is_w(pa, w, "p") {
        return None;
    }
    let bi = pa.children.iter().position(|n| matches!(n, Node::El(c) if is_w(c, w, "r") && fld_type(c, w).is_some()))?;
    let Node::El(first) = &pa.children[bi] else { return None };
    if fld_type(first, w).as_deref() != Some("begin") {
        return None;
    }
    let (mut depth, mut sep, mut code, mut inside) = (0usize, false, String::new(), Inside::default());
    let mut texts = vec![String::new()];
    let mut base_rpr = None;
    for (pi, n) in children.iter().enumerate().skip(a) {
        let Node::El(p) = n else { continue };
        if !is_w(p, w, "p") {
            return None;
        }
        if pi > a && sep {
            texts.push(String::new());
        }
        let start = if pi == a { bi } else { 0 };
        for (ci, c) in p.children.iter().enumerate().skip(start) {
            let Node::El(e) = c else { continue };
            if is_w(e, w, "pPr") {
                continue;
            }
            if is_w(e, w, "r") {
                match fld_type(e, w).as_deref() {
                    Some("begin") => depth += 1,
                    Some("separate") => {
                        if depth == 1 {
                            sep = true;
                            if zotero::field_kind(&code) != Some(FieldKind::Bibliography) {
                                return None;
                            }
                        }
                    }
                    Some("end") => {
                        depth -= 1;
                        if depth == 0 {
                            texts.retain(|t| !t.trim().is_empty());
                            return inside.ok().then_some(BibSpan { a, bi, b: pi, ei: ci, texts, base_rpr });
                        }
                    }
                    _ if depth == 1 && !sep => code.push_str(&instr(e, w)),
                    _ if sep => {
                        let mut t = String::new();
                        visible_text(e, w, &mut t);
                        if base_rpr.is_none() && !t.is_empty() {
                            base_rpr = e.child(&q(w, "rPr")).cloned();
                        }
                        texts.last_mut()?.push_str(&t);
                    }
                    _ => {}
                }
            } else if has_fld(e, w) {
                return None;
            } else if sep && (is_w(e, w, "hyperlink") || is_w(e, w, "smartTag")) {
                visible_text(e, w, texts.last_mut()?);
            } else {
                inside.see(e, w);
            }
        }
    }
    None
}

fn ppr_without_sectpr(p: &El, w: &str) -> Option<El> {
    let mut ppr = p.child(&q(w, "pPr")).cloned()?;
    ppr.remove_children(&q(w, "sectPr"));
    Some(ppr)
}

fn has_content(nodes: &[Node], w: &str) -> bool {
    nodes.iter().any(|n| matches!(n, Node::El(e) if !is_w(e, w, "pPr") && !is_w(e, w, "proofErr")))
}

fn paragraph(w: &str, ppr: Option<El>, nodes: Vec<Node>) -> El {
    let mut p = El::new(q(w, "p"));
    if let Some(ppr) = ppr {
        p.children.push(Node::El(ppr));
    }
    p.children.extend(nodes);
    p
}

fn sdt(w: &str, tag: &str, id: i64, content: Vec<Node>) -> El {
    let mut pr = El::new(q(w, "sdtPr"));
    for (name, val) in [("alias", "Pith".to_string()), ("tag", tag.to_string()), ("id", id.to_string()), ("lock", "contentLocked".to_string())] {
        pr.children.push(Node::El(El::new(q(w, name)).with_attr(&q(w, "val"), &val)));
    }
    let mut c = El::new(q(w, "sdtContent"));
    c.children = content;
    let mut s = El::new(q(w, "sdt"));
    s.children.push(Node::El(pr));
    s.children.push(Node::El(c));
    s
}

/// 把找到的书目换成：[域前面的内容] + 书目控件 + [域后面的内容（含分节符）]
fn replace_bib(children: &mut Vec<Node>, span: &BibSpan, w: &str, sdt_id: i64) {
    let (Node::El(pa), Node::El(pb)) = (&children[span.a], &children[span.b]) else { return };
    let pre: Vec<Node> = pa.children[..span.bi].iter().filter(|n| !matches!(n, Node::El(e) if is_w(e, w, "pPr"))).cloned().collect();
    let post: Vec<Node> = pb.children[span.ei + 1..].to_vec();
    let pb_ppr = pb.child(&q(w, "pPr")).cloned();
    let pb_sect = pb_ppr.as_ref().is_some_and(|p| p.child(&q(w, "sectPr")).is_some());
    let inner_ppr = ppr_without_sectpr(pa, w);
    let mut new = Vec::new();
    if has_content(&pre, w) {
        new.push(Node::El(paragraph(w, inner_ppr.clone(), pre)));
    }
    let placeholder = build_run(w, span.base_rpr.as_ref(), &Run::plain(BIB_PLACEHOLDER));
    new.push(Node::El(sdt(w, BIB_TAG, sdt_id, vec![Node::El(paragraph(w, inner_ppr, vec![Node::El(placeholder)]))])));
    if has_content(&post, w) || pb_sect {
        new.push(Node::El(paragraph(w, pb_ppr, post)));
    }
    children.splice(span.a..=span.b, new);
}

/// 文档顺序找 Zotero 书目域（只换第一个）；`f` 决定换不换
fn walk_blocks(el: &mut El, w: &str, f: &mut dyn FnMut(&BibSpan) -> Option<i64>) -> bool {
    let mut i = 0;
    while i < el.children.len() {
        if let Some(span) = find_bib(&el.children, i, w) {
            if let Some(id) = f(&span) {
                replace_bib(&mut el.children, &span, w, id);
                return true;
            }
            i = span.b + 1;
            continue;
        }
        if let Node::El(c) = &mut el.children[i]
            && !is_w(c, w, "p")
            && !(is_w(c, w, "sdt") && is_ours(&tag_of(c, w)))
            && walk_blocks(c, w, f)
        {
            return true;
        }
        i += 1;
    }
    false
}

fn max_sdt_id(el: &El, w: &str, max: &mut i64) {
    for c in el.els() {
        if is_w(c, w, "sdtPr")
            && let Some(id) = c.child(&q(w, "id")).and_then(|e| e.attr(&q(w, "val"))).and_then(|v| v.parse::<i64>().ok())
        {
            *max = (*max).max(id);
        }
        max_sdt_id(c, w, max);
    }
}

/// 文档里的 Zotero 引文（不改文档）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// 能转换的引文域代码（文档顺序）
    pub codes: Vec<String>,
    /// 有能转换的书目域
    pub bibliography: bool,
    /// Zotero 书目的各段文字
    pub bibliography_texts: Vec<String>,
    /// 文档里 Zotero 域的总数（含没法转换的）
    pub fields: usize,
    /// Zotero 文档设置里的样式 URI
    pub zotero_style: Option<String>,
    /// Zotero 用的是脚注 / 尾注样式
    pub note_style: bool,
    /// 已经有 Pith 引文数据
    pub has_pith: bool,
}

pub fn inspect(bytes: &[u8]) -> Result<Found, String> {
    let mut d = docx::open(bytes)?;
    let w = d.w.clone();
    let mut codes = Vec::new();
    walk_inline(&mut d.doc.root, &w, &mut |code, _| {
        codes.push(code.to_string());
        None
    });
    let mut texts = None;
    walk_blocks(&mut d.doc.root, &w, &mut |span| {
        texts.get_or_insert_with(|| span.texts.clone());
        None
    });
    let snap = check::read_package(&d.pkg)?;
    let prefs = d.pkg.get_str(CUSTOM_PROPS).ok().and_then(|x| zotero::prefs_from_custom_xml(&x)).and_then(|p| zotero::parse_prefs(&p));
    Ok(Found {
        codes,
        bibliography: texts.is_some(),
        bibliography_texts: texts.unwrap_or_default(),
        fields: snap.fields.iter().filter(|f| zotero::field_kind(&f.code).is_some()).count(),
        zotero_style: prefs.as_ref().and_then(|p| p.style.clone()),
        note_style: prefs.as_ref().and_then(|p| p.note_type).is_some_and(|n| n != 0),
        has_pith: d.store().is_some(),
    })
}

pub struct Options<'a> {
    /// Zotero 条目 → 文献库 uid
    pub resolve: &'a dyn Fn(&ItemKey) -> Option<String>,
    /// Zotero 样式 URI → Pith 样式 ID
    pub style_for_uri: &'a dyn Fn(&str) -> Option<String>,
    pub default_style: &'a str,
    pub library: Library<'a>,
    pub render: &'a dyn Render,
    pub now: &'a str,
}

/// Zotero 原来显示的与 Pith 排出的不一样的一处
#[derive(Clone, Debug, Serialize)]
pub struct Diff {
    pub index: usize,
    pub zotero: String,
    pub pith: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub stats: Stats,
    /// 逐条说明（中文）
    pub notes: Vec<String>,
    pub bibliography: bool,
    pub zotero_style: Option<String>,
    /// 转换后文档用的 Pith 样式
    pub style: String,
    pub style_note: Option<String>,
    /// 没转换、原样留下的 Zotero 域（含书目）
    pub left: usize,
    /// 与 Zotero 原来显示的文字一致的引文 / 不一致的
    pub same_citations: usize,
    pub citation_diffs: Vec<Diff>,
    pub zotero_entries: usize,
    pub same_entries: usize,
    pub entry_diffs: Vec<Diff>,
}

pub struct Converted {
    pub bytes: Vec<u8>,
    pub plan: Plan,
    pub report: Report,
}

/// 比较用：空白归一
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn diffs(zotero: &[String], pith: &[String]) -> (usize, Vec<Diff>) {
    let mut same = 0;
    let mut out = Vec::new();
    for i in 0..zotero.len().max(pith.len()) {
        let (z, p) = (zotero.get(i).cloned().unwrap_or_default(), pith.get(i).cloned().unwrap_or_default());
        if norm(&z) == norm(&p) {
            same += 1;
        } else {
            out.push(Diff { index: i, zotero: z, pith: p });
        }
    }
    (same, out)
}

/// 转换（Word 外）。文档里没有能转换的 Zotero 引文时报错。
pub fn convert(bytes: &[u8], o: &Options, new_id: &mut dyn FnMut() -> String) -> Result<Converted, String> {
    let found = inspect(bytes)?;
    let before = check::read(bytes)?;
    if found.codes.is_empty() && !found.bibliography {
        return Err(if found.fields > 0 { "文档里的 Zotero 引文结构特殊（域跨了别的元素或夹着修订），没法转换".into() } else { "文档里没有 Zotero 引文".into() });
    }
    let mut d = docx::open(bytes)?;
    let w = d.w.clone();
    let mut store = match d.store() {
        None => None,
        Some(Err(e)) => return Err(e.zh()),
        Some(Ok(s)) => Some(s),
    };
    // 样式：文档已有 Pith 数据就沿用；否则按 Zotero 的样式对应；对不上用默认
    let mapped = found.zotero_style.as_deref().and_then(o.style_for_uri);
    let style_note = match (&store, &found.zotero_style, &mapped) {
        (Some(s), _, _) => Some(format!("文档里已有 Pith 引文，沿用它的样式 {}", s.style)),
        (None, Some(z), None) => Some(format!("Zotero 用的样式 {z} Pith 里没有，先用默认样式；导入同一个 .csl 后可以在 Word 里切换")),
        _ => None,
    };
    let mut store = store.take().unwrap_or_else(|| DocStore::new(mapped.as_deref().unwrap_or(o.default_style), o.now));

    let mapping = zotero::convert(&found.codes, o.resolve, o.now);
    let mut next_sdt = 0;
    max_sdt_id(&d.doc.root, &w, &mut next_sdt);
    // 换引文域
    let mut k = 0;
    let mut converted_codes = Vec::new();
    let mut zotero_plain = Vec::new();
    let mut new_ids: Vec<String> = Vec::new();
    walk_inline(&mut d.doc.root, &w, &mut |code, nodes| {
        let c = mapping.citations.get(k).cloned().flatten();
        k += 1;
        let c = c?;
        let id = new_id();
        next_sdt += 1;
        let base = nodes.iter().skip_while(|n| !matches!(n, Node::El(e) if fld_type(e, &w).as_deref() == Some("separate"))).find_map(|n| match n {
            Node::El(e) if is_w(e, &w, "r") && e.child(&q(&w, "t")).is_some() => e.child(&q(&w, "rPr")).cloned(),
            _ => None,
        });
        let run = build_run(&w, base.as_ref(), &Run::plain(PLACEHOLDER));
        store.citations.insert(id.clone(), c.data.clone());
        converted_codes.push(code.to_string());
        zotero_plain.push(c.zotero_plain.clone());
        new_ids.push(id.clone());
        Some(sdt(&w, &format!("{CITE_PREFIX}{id}"), next_sdt, vec![Node::El(run)]))
    });
    for (id, snap) in &mapping.snapshots {
        store.items.entry(id.clone()).or_insert_with(|| snap.clone());
    }
    // 换书目域（文档已有 Pith 书目就不再加）
    let mut bib_texts = Vec::new();
    let bibliography = before.bibliographies.is_empty()
        && walk_blocks(&mut d.doc.root, &w, &mut |span| {
            bib_texts = span.texts.clone();
            next_sdt += 1;
            Some(next_sdt)
        });

    // 刷新：排版、写数据部件
    let doc = d.citations();
    let lib = (o.library)(&referenced_items(&doc, &store))?;
    let plan = plan_refresh(&doc, &store, &lib, o.render, new_id, o.now)?;
    d.apply(&plan)?;
    let out = d.into_bytes()?;

    // 独立核对
    let after = check::read(&out)?;
    let mut problems = check::zotero_converted(&before, &after, &converted_codes, bibliography);
    problems.extend(check::against_plan(&after, &plan));
    if !problems.is_empty() {
        return Err(format!("转换后核对没通过，没有交付：{}", problems.join("；")));
    }

    let by_id: BTreeMap<&str, &str> = plan.citations.iter().map(|u| (u.citation_id.as_str(), u.text.as_str())).collect();
    let pith_texts: Vec<String> = new_ids.iter().map(|id| by_id.get(id.as_str()).map(|s| s.to_string()).unwrap_or_default()).collect();
    let (same_citations, citation_diffs) = diffs(&zotero_plain, &pith_texts);
    let (same_entries, entry_diffs) = if bibliography { diffs(&bib_texts, &plan.bibliography.texts) } else { (0, Vec::new()) };
    let left = after.fields.iter().filter(|f| zotero::field_kind(&f.code).is_some()).count();
    let mut notes: Vec<String> = mapping.notes.iter().map(|n| n.zh()).collect();
    if found.note_style {
        notes.push("Zotero 用的是脚注 / 尾注样式；Pith 只转换正文里的引文，脚注里的没动".into());
    }
    notes.extend(plan.issues.iter().map(|i| i.zh()));
    Ok(Converted {
        report: Report {
            stats: mapping.stats,
            notes,
            bibliography,
            zotero_style: found.zotero_style,
            style: plan.store.style.clone(),
            style_note,
            left,
            same_citations,
            citation_diffs,
            zotero_entries: bib_texts.len(),
            same_entries,
            entry_diffs,
        },
        bytes: out,
        plan,
    })
}
