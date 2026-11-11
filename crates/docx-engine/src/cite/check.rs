//! 独立核对：用 roxmltree（写入用的是 quick-xml 的可编辑树）读出引文、书目、域与控件以外的文字。
//! - `against_plan`：刷新后，文档是不是就是计划里的样子
//! - `outside_changed`：刷新前后，控件以外的文字、域、其他部件有没有被动过
//! - `verify`：不改文档，用当前数据重新排一遍，逐处比对

use std::collections::BTreeMap;

use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::plan::{plan_refresh, referenced_items, DocCitation, Library, Plan, Render};
use super::store::{DocStore, StoreError};
use super::{citation_id, is_bibliography, is_ours, merge_runs, runs_signature, runs_text, zotero, Run, EMPTY_BIBLIOGRAPHY};
use crate::package::Package;
use crate::NS_W;

const CUSTOM_XML: &str = "customXml/";

/// 文档里的一个域（复杂域或简单域）
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Field {
    pub code: String,
    pub result: String,
}

pub struct Snapshot {
    /// 引文控件：(tag, 文字)
    pub citations: Vec<(String, Vec<Run>)>,
    /// 书目控件：每个控件的各段文字
    pub bibliographies: Vec<Vec<String>>,
    /// 正文里控件以外的文字（我们的控件记成 ⟦C⟧ / ⟦B⟧）
    pub outside: String,
    /// 同 `outside`，但 Zotero 引文域 / 书目域的显示结果也记成 ⟦C⟧ / ⟦B⟧（核对 Zotero 转换用）
    pub masked: String,
    pub fields: Vec<Field>,
    /// 部件名 → SHA-256
    pub parts: BTreeMap<String, String>,
    pub store: Option<Result<DocStore, StoreError>>,
}

fn is(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(NS_W) && n.tag_name().name() == local
}

fn wattr<'a>(n: &Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.attribute((NS_W, name))
}

fn child<'a>(n: Node<'a, 'a>, local: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| is(c, local))
}

fn tag(sdt: Node) -> String {
    child(sdt, "sdtPr").and_then(|p| child(p, "tag")).and_then(|t| wattr(&t, "val")).unwrap_or("").to_string()
}

fn on(rpr: Option<Node>, local: &str) -> bool {
    rpr.and_then(|p| child(p, local)).is_some_and(|e| !matches!(wattr(&e, "val"), Some("0" | "false" | "off" | "none")))
}

fn runs_of(container: Node) -> Vec<Run> {
    let mut out = Vec::new();
    for r in container.descendants().filter(|n| is(n, "r")) {
        let rpr = child(r, "rPr");
        let mut text = String::new();
        for c in r.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(NS_W)) {
            match c.tag_name().name() {
                "t" => text.push_str(c.text().unwrap_or("")),
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                _ => {}
            }
        }
        let va = rpr.and_then(|p| child(p, "vertAlign")).and_then(|e| wattr(&e, "val"));
        out.push(Run {
            text,
            italic: on(rpr, "i"),
            bold: on(rpr, "b"),
            sup: va == Some("superscript"),
            sub: va == Some("subscript"),
            small_caps: on(rpr, "smallCaps"),
            underline: on(rpr, "u"),
        });
    }
    merge_runs(out)
}

struct OpenField {
    code: String,
    result: String,
    in_result: bool,
    /// Zotero 域：显示结果在 `masked` 里记成的标记
    zotero: Option<&'static str>,
}

struct Walk {
    citations: Vec<(String, Vec<Run>)>,
    bibliographies: Vec<Vec<String>>,
    outside: String,
    masked: String,
    fields: Vec<Field>,
    open: Vec<OpenField>,
}

impl Walk {
    fn in_zotero(&self) -> bool {
        self.open.iter().any(|f| f.in_result && f.zotero.is_some())
    }

    fn text(&mut self, s: &str) {
        self.outside.push_str(s);
        if !self.in_zotero() {
            self.masked.push_str(s);
        }
    }

    fn mark(&mut self, m: &str) {
        self.outside.push_str(m);
        self.masked.push_str(m);
    }

    fn node(&mut self, n: Node) {
        for c in n.children().filter(|c| c.is_element()) {
            if is(&c, "sdt") {
                let t = tag(c);
                if is_ours(&t) {
                    let content = child(c, "sdtContent");
                    if citation_id(&t).is_some() {
                        self.citations.push((t, content.map(runs_of).unwrap_or_default()));
                        self.mark("⟦C⟧");
                    } else if is_bibliography(&t) {
                        let paras = content.map(|ct| ct.descendants().filter(|p| is(p, "p")).map(|p| runs_text(&runs_of(p))).collect()).unwrap_or_default();
                        self.bibliographies.push(paras);
                        self.mark("⟦B⟧");
                    }
                    continue;
                }
            }
            if c.tag_name().namespace() == Some(NS_W) {
                match c.tag_name().name() {
                    "t" => {
                        let s = c.text().unwrap_or("");
                        self.text(s);
                        if let Some(f) = self.open.last_mut().filter(|f| f.in_result) {
                            f.result.push_str(s);
                        }
                    }
                    "tab" if n.tag_name().name() == "r" => self.text("\t"),
                    "br" | "cr" if n.tag_name().name() == "r" => self.text("\n"),
                    "instrText" => {
                        if let Some(f) = self.open.last_mut() {
                            f.code.push_str(c.text().unwrap_or(""));
                        }
                    }
                    "fldChar" => match wattr(&c, "fldCharType") {
                        Some("begin") => self.open.push(OpenField { code: String::new(), result: String::new(), in_result: false, zotero: None }),
                        Some("separate") => {
                            if let Some(f) = self.open.last_mut() {
                                f.in_result = true;
                                f.zotero = match zotero::field_kind(&f.code) {
                                    Some(zotero::FieldKind::Citation) => Some("⟦C⟧"),
                                    Some(zotero::FieldKind::Bibliography) => Some("⟦B⟧"),
                                    None => None,
                                };
                            }
                        }
                        Some("end") => {
                            if let Some(f) = self.open.pop() {
                                if let Some(m) = f.zotero.filter(|_| f.in_result)
                                    && !self.in_zotero()
                                {
                                    self.masked.push_str(m);
                                }
                                self.fields.push(Field { code: f.code.trim().to_string(), result: f.result });
                            }
                        }
                        _ => {}
                    },
                    "fldSimple" => {
                        let result = c.descendants().filter(|t| is(t, "t")).filter_map(|t| t.text()).collect();
                        self.fields.push(Field { code: wattr(&c, "instr").unwrap_or("").trim().to_string(), result });
                    }
                    _ => {}
                }
            }
            self.node(c);
            if is(&c, "p") {
                self.text("\n");
            }
        }
    }
}

pub fn read_package(pkg: &Package) -> Result<Snapshot, String> {
    let xml = pkg.get_str(super::docx::DOCUMENT).map_err(|e| e.to_string())?;
    let doc = Document::parse_with_options(&xml, ParsingOptions { allow_dtd: true, ..ParsingOptions::default() }).map_err(|e| format!("document.xml 解析失败：{e}"))?;
    let mut w = Walk { citations: Vec::new(), bibliographies: Vec::new(), outside: String::new(), masked: String::new(), fields: Vec::new(), open: Vec::new() };
    w.node(doc.root());
    let parts = pkg.names().into_iter().map(|n| (n.to_string(), hex(&Sha256::digest(pkg.get(n).unwrap_or(&[]))))).collect();
    Ok(Snapshot { citations: w.citations, bibliographies: w.bibliographies, outside: w.outside, masked: w.masked, fields: w.fields, parts, store: super::docx::read_store(pkg) })
}

pub fn read(bytes: &[u8]) -> Result<Snapshot, String> {
    read_package(&Package::read(bytes).map_err(|e| e.to_string())?)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct ZoteroFields {
    pub citations: usize,
    pub bibliographies: usize,
    /// 所有 Zotero 域（代码 + 显示结果）的指纹：刷新前后应相同
    pub digest: String,
}

pub fn zotero_fields(fields: &[Field]) -> ZoteroFields {
    let z: Vec<&Field> = fields.iter().filter(|f| f.code.starts_with("ADDIN ZOTERO_")).collect();
    let joined: Vec<String> = z.iter().map(|f| format!("{}\u{0}{}", f.code, f.result)).collect();
    ZoteroFields {
        citations: z.iter().filter(|f| f.code.starts_with("ADDIN ZOTERO_ITEM")).count(),
        bibliographies: z.iter().filter(|f| f.code.starts_with("ADDIN ZOTERO_BIBL")).count(),
        digest: hex(&Sha256::digest(joined.join("\u{1}").as_bytes())),
    }
}

/// 刷新前后：控件以外的文字、域、其他部件都不该变（只允许正文、数据部件与新建数据部件要用的关系 / 内容类型变）
pub fn outside_changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    if before.outside != after.outside {
        out.push("引文控件以外的正文文字变了".into());
    }
    if before.citations.len() != after.citations.len() || before.bibliographies.len() != after.bibliographies.len() {
        out.push("引文 / 书目控件的个数变了".into());
    }
    if before.fields != after.fields {
        out.push("正文里的域（含 Zotero 引文）变了".into());
    }
    out.extend(parts_changed(before, after));
    out
}

/// 部件：只允许正文、数据部件与新建数据部件要用的关系 / 内容类型变
fn parts_changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let new_store = before.store.is_none();
    let may_change = |p: &str| p == super::docx::DOCUMENT || p.starts_with(CUSTOM_XML) || (new_store && (p == "word/_rels/document.xml.rels" || p == "[Content_Types].xml"));
    for (p, h) in &before.parts {
        match after.parts.get(p) {
            None => out.push(format!("部件 {p} 不见了")),
            Some(h2) if h2 != h && !may_change(p) => out.push(format!("部件 {p} 被改了")),
            _ => {}
        }
    }
    for p in after.parts.keys().filter(|p| !before.parts.contains_key(*p)) {
        if !p.starts_with(CUSTOM_XML) {
            out.push(format!("多出了部件 {p}"));
        }
    }
    out
}

/// 书目紧挨着的段落分隔不算（Zotero 书目域在段落里、结束的那段还有段落标记，Pith 书目控件是块级）。
/// 书目前后的段落本身转换时不碰（只换书目域所在的那几段）。
fn normalize_masked(s: &str) -> String {
    let parts: Vec<&str> = s.split("⟦B⟧").collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let p = if i > 0 { p.trim_start_matches('\n') } else { p };
            if i + 1 < n { p.trim_end_matches('\n') } else { p }
        })
        .collect::<Vec<_>>()
        .join("⟦B⟧")
}

/// 第一处不同附近的文字（报错用）
fn where_differs(a: &str, b: &str) -> String {
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let i = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let show = |v: &[char]| v[i.saturating_sub(20)..(i + 20).min(v.len())].iter().collect::<String>().replace('\n', "⏎");
    format!("第 {} 个字附近：原来「{}」，现在「{}」", i + 1, show(&ac), show(&bc))
}

fn is_zotero(f: &Field) -> bool {
    zotero::field_kind(&f.code).is_some()
}

/// Zotero 转换前后：Zotero 域以外的文字、别的域、其他部件都没变；转换了的域变成了 Pith 控件，没转换的原样留着。
/// `converted`：转换了的 Zotero 引文域代码；`bibliography`：书目域也转换了。
pub fn zotero_converted(before: &Snapshot, after: &Snapshot, converted: &[String], bibliography: bool) -> Vec<String> {
    let mut out = Vec::new();
    let (a, b) = (normalize_masked(&before.masked), normalize_masked(&after.masked));
    if a != b {
        out.push(format!("Zotero 引文以外的正文文字变了（{}）", where_differs(&a, &b)));
    }
    let others = |s: &Snapshot| s.fields.iter().filter(|f| !is_zotero(f)).cloned().collect::<Vec<_>>();
    if others(before) != others(after) {
        out.push("Zotero 以外的域变了".into());
    }
    let mut left: Vec<&Field> = before.fields.iter().filter(|f| is_zotero(f)).collect();
    for code in converted {
        match left.iter().position(|f| f.code == code.trim() && zotero::field_kind(&f.code) == Some(zotero::FieldKind::Citation)) {
            Some(i) => {
                left.remove(i);
            }
            None => out.push("转换的引文在原文档里找不到".into()),
        }
    }
    if bibliography {
        match left.iter().position(|f| zotero::field_kind(&f.code) == Some(zotero::FieldKind::Bibliography)) {
            Some(i) => {
                left.remove(i);
            }
            None => out.push("转换的书目在原文档里找不到".into()),
        }
    }
    let remain: Vec<&Field> = after.fields.iter().filter(|f| is_zotero(f)).collect();
    if remain != left {
        out.push("没转换的 Zotero 域被动过".into());
    }
    if after.citations.len() != before.citations.len() + converted.len() {
        out.push(format!("引文控件应有 {} 个，实际 {} 个", before.citations.len() + converted.len(), after.citations.len()));
    }
    if after.bibliographies.len() != before.bibliographies.len() + usize::from(bibliography) {
        out.push("书目控件的个数不对".into());
    }
    out.extend(parts_changed(before, after));
    out
}

/// 刷新后：每处改写的引文、每个书目控件、数据部件，是不是就是计划里的样子
pub fn against_plan(after: &Snapshot, plan: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    for u in &plan.citations {
        let n = u.index + 1;
        match after.citations.get(u.index) {
            None => out.push(format!("第 {n} 处引文不见了")),
            Some((tag, runs)) => {
                if citation_id(tag) != Some(u.citation_id.as_str()) {
                    out.push(format!("第 {n} 处引文的 ID 不对"));
                }
                if *runs != merge_runs(u.runs.clone()) {
                    out.push(format!("第 {n} 处引文是「{}」，应为「{}」", runs_text(runs), u.text));
                }
            }
        }
    }
    let expected: Vec<String> = plan.bibliography.paragraphs().iter().map(|p| runs_text(p)).collect();
    for (i, b) in after.bibliographies.iter().enumerate() {
        if *b != expected {
            out.push(format!("第 {} 个书目与计划不一致", i + 1));
        }
    }
    match &after.store {
        Some(Ok(s)) if *s == plan.store => {}
        Some(Ok(_)) => out.push("数据部件与计划不一致".into()),
        Some(Err(e)) => out.push(format!("数据部件读不回来：{}", e.zh())),
        None => out.push("数据部件没写进去".into()),
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn line_breaks_next_to_the_bibliography_do_not_count() {
        use super::normalize_masked as n;
        assert_eq!(n("参考文献\n⟦B⟧\n\n\n附录"), "参考文献⟦B⟧附录");
        assert_eq!(n("a\n⟦B⟧b⟦C⟧\n"), "a⟦B⟧b⟦C⟧\n");
        assert_eq!(n("no bib\n"), "no bib\n");
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationCheck {
    pub index: usize,
    pub tag: String,
    pub actual: String,
    pub expected: Option<String>,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BibliographyCheck {
    pub count: usize,
    pub ok: bool,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ok: bool,
    pub style: Option<String>,
    pub citations: Vec<CitationCheck>,
    pub bibliography: BibliographyCheck,
    pub zotero: ZoteroFields,
    pub problems: Vec<String>,
}

/// 不改文档：用文档数据（加文献库的最新数据）重新排一遍，逐处比对引文与书目。
pub fn verify(
    bytes: &[u8],
    library: Library,
    render: &dyn Render,
) -> Result<Report, String> {
    let snap = read(bytes)?;
    let zotero = zotero_fields(&snap.fields);
    let empty_bib = BibliographyCheck { count: snap.bibliographies.len(), ok: false, expected: Vec::new(), actual: snap.bibliographies.first().cloned().unwrap_or_default() };
    let store = match &snap.store {
        None => return Ok(Report { ok: false, style: None, citations: Vec::new(), bibliography: empty_bib, zotero, problems: vec!["文档里没有 Pith 引文数据".into()] }),
        Some(Err(e)) => return Ok(Report { ok: false, style: None, citations: Vec::new(), bibliography: empty_bib, zotero, problems: vec![e.zh()] }),
        Some(Ok(s)) => s.clone(),
    };
    let doc: Vec<DocCitation> = snap.citations.iter().map(|(t, r)| DocCitation { tag: t.clone(), signature: Some(runs_signature(r)) }).collect();
    let lib = library(&referenced_items(&doc, &store))?;
    let plan = plan_refresh(&doc, &store, &lib, render, &mut || "verify-new-id".to_string(), &store.updated)?;
    let mut problems = Vec::new();
    let by_index: BTreeMap<usize, _> = plan.citations.iter().map(|u| (u.index, u)).collect();
    let citations: Vec<CitationCheck> = snap
        .citations
        .iter()
        .enumerate()
        .map(|(index, (tag, runs))| {
            let u = by_index.get(&index);
            CitationCheck { index, tag: tag.clone(), actual: runs_text(runs), expected: u.map(|u| u.text.clone()), ok: u.is_some_and(|u| !u.changed) }
        })
        .collect();
    for c in citations.iter().filter(|c| !c.ok) {
        problems.push(format!("第 {} 处引文不一致：现为「{}」，应为「{}」", c.index + 1, c.actual, c.expected.as_deref().unwrap_or("（无法生成）")));
    }
    problems.extend(plan.issues.iter().map(|i| i.zh()));
    let expected: Vec<String> = if plan.bibliography.texts.is_empty() { vec![EMPTY_BIBLIOGRAPHY.to_string()] } else { plan.bibliography.texts.clone() };
    let actual = snap.bibliographies.first().cloned().unwrap_or_default();
    let bib_ok = snap.bibliographies.len() == 1 && actual == expected;
    if snap.bibliographies.len() != 1 {
        problems.push(format!("书目控件应有 1 个，实际 {} 个", snap.bibliographies.len()));
    } else if !bib_ok {
        problems.push("书目与当前引文不一致，需要刷新".into());
    }
    Ok(Report {
        ok: problems.is_empty(),
        style: Some(store.style.clone()),
        citations,
        bibliography: BibliographyCheck { count: snap.bibliographies.len(), ok: bib_ok, expected, actual },
        zotero,
        problems,
    })
}
