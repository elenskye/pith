//! Word 外直接改 .docx 里的 Pith 引文：行内内容控件 + 书目控件 + Custom XML 数据部件。
//! 只改我们自己的控件内容与数据部件；其余部件按原压缩字节复制（`package`），其余 XML 按原字节写回（`xml`）。
//! 写完用 `check`（另一套解析器）核对：控件以外的文字、域、其他部件都没变，引文与书目就是计划里的样子。

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::check;
use super::plan::{plan_refresh, referenced_items, DocCitation, Library, Plan, Render};
use super::store::{self, DocStore, StoreError, LEGACY_STORE_NS, STORE_NS};
use super::{citation_id, is_bibliography, is_ours, merge_runs, runs_signature, Run};
use crate::package::Package;
use crate::xml::{self, escape_text, unescape, El, Node, XmlDoc};
use crate::NS_W;

pub const DOCUMENT: &str = "word/document.xml";
const DOC_RELS: &str = "word/_rels/document.xml.rels";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const PKG_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL_CUSTOM_XML: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
const REL_CUSTOM_XML_PROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
const CT_ITEM_PROPS: &str = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
const DS_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/customXml";

// 保留原有字体、字号、语言，去掉会被引文格式覆盖的属性；按 schema 顺序重建 rPr
const KEEP_RPR: &[&str] = &["rStyle", "rFonts", "color", "sz", "szCs", "lang"];
const RPR_ORDER: &[&str] = &["rStyle", "rFonts", "b", "bCs", "i", "iCs", "smallCaps", "color", "sz", "szCs", "u", "vertAlign", "lang"];

pub(crate) fn q(w: &str, local: &str) -> String {
    if w.is_empty() { local.to_string() } else { format!("{w}:{local}") }
}

pub(crate) fn is_w(el: &El, w: &str, local: &str) -> bool {
    el.prefix() == w && el.local() == local
}

/// 内容控件的 tag（sdtPr/tag/@w:val）
pub(crate) fn tag_of(sdt: &El, w: &str) -> String {
    sdt.child(&q(w, "sdtPr")).and_then(|p| p.child(&q(w, "tag"))).and_then(|t| t.attr(&q(w, "val"))).map(unescape).unwrap_or_default()
}

/// 按文档顺序列出内容控件；不进我们自己的控件里面找（与 `check` 的规则相同）
fn collect_sdts<'a>(el: &'a El, w: &str, out: &mut Vec<&'a El>) {
    for c in el.els() {
        if is_w(c, w, "sdt") {
            out.push(c);
            if is_ours(&tag_of(c, w)) {
                continue;
            }
        }
        collect_sdts(c, w, out);
    }
}

fn for_each_sdt_mut(el: &mut El, w: &str, f: &mut dyn FnMut(&mut El)) {
    for c in el.els_mut() {
        if is_w(c, w, "sdt") {
            let ours = is_ours(&tag_of(c, w));
            f(c);
            if ours {
                continue;
            }
        }
        for_each_sdt_mut(c, w, f);
    }
}

fn flag(rpr: Option<&El>, w: &str, local: &str) -> bool {
    rpr.and_then(|p| p.child(&q(w, local))).is_some_and(|e| !matches!(e.attr(&q(w, "val")), Some("0" | "false" | "off" | "none")))
}

fn text_children(el: &El) -> String {
    el.children.iter().filter_map(|n| if let Node::Raw(r) = n { Some(unescape(r)) } else { None }).collect()
}

/// 把 w:r 读成带格式的片段（正文文字、上下标、斜体、粗体、小型大写、下划线）
pub(crate) fn read_runs(container: &El, w: &str) -> Vec<Run> {
    fn walk(el: &El, w: &str, out: &mut Vec<Run>) {
        for c in el.els() {
            if !is_w(c, w, "r") {
                walk(c, w, out);
                continue;
            }
            let rpr = c.child(&q(w, "rPr"));
            let mut text = String::new();
            for e in c.els() {
                match e.local() {
                    "t" if e.prefix() == w => text.push_str(&text_children(e)),
                    "tab" if e.prefix() == w => text.push('\t'),
                    "br" | "cr" if e.prefix() == w => text.push('\n'),
                    _ => {}
                }
            }
            let va = rpr.and_then(|p| p.child(&q(w, "vertAlign"))).and_then(|e| e.attr(&q(w, "val")));
            out.push(Run {
                text,
                italic: flag(rpr, w, "i"),
                bold: flag(rpr, w, "b"),
                sup: va == Some("superscript"),
                sub: va == Some("subscript"),
                small_caps: flag(rpr, w, "smallCaps"),
                underline: flag(rpr, w, "u"),
            });
        }
    }
    let mut out = Vec::new();
    walk(container, w, &mut out);
    merge_runs(out)
}

fn first_run(el: &El, w: &str) -> Option<El> {
    for c in el.els() {
        if is_w(c, w, "r") {
            return Some(c.clone());
        }
        if let Some(r) = first_run(c, w) {
            return Some(r);
        }
    }
    None
}

fn build_rpr(w: &str, base: Option<&El>, run: &Run) -> El {
    let mut parts: BTreeMap<&str, El> = BTreeMap::new();
    if let Some(base) = base {
        for name in KEEP_RPR {
            if let Some(e) = base.child(&q(w, name)) {
                parts.insert(name, e.clone());
            }
        }
    }
    let mut mk = |name: &'static str, val: Option<&str>| {
        let mut e = El::new(q(w, name));
        if let Some(v) = val {
            e.set_attr(&q(w, "val"), v);
        }
        parts.insert(name, e);
    };
    if run.bold {
        mk("b", None);
        mk("bCs", None);
    }
    if run.italic {
        mk("i", None);
        mk("iCs", None);
    }
    if run.small_caps {
        mk("smallCaps", None);
    }
    if run.underline {
        mk("u", Some("single"));
    }
    if run.sup {
        mk("vertAlign", Some("superscript"));
    } else if run.sub {
        mk("vertAlign", Some("subscript"));
    }
    let mut rpr = El::new(q(w, "rPr"));
    for name in RPR_ORDER {
        if let Some(e) = parts.remove(name) {
            rpr.children.push(Node::El(e));
        }
    }
    rpr
}

pub(crate) fn build_run(w: &str, base: Option<&El>, run: &Run) -> El {
    let mut r = El::new(q(w, "r"));
    let rpr = build_rpr(w, base, run);
    if !rpr.children.is_empty() {
        r.children.push(Node::El(rpr));
    }
    let mut piece = String::new();
    let flush = |piece: &mut String, r: &mut El| {
        if !piece.is_empty() {
            let mut t = El::new(q(w, "t")).with_attr("xml:space", "preserve");
            t.children.push(Node::Raw(escape_text(piece)));
            r.children.push(Node::El(t));
            piece.clear();
        }
    };
    for ch in run.text.chars() {
        match ch {
            '\t' => {
                flush(&mut piece, &mut r);
                r.children.push(Node::El(El::new(q(w, "tab"))));
            }
            '\n' => {
                flush(&mut piece, &mut r);
                r.children.push(Node::El(El::new(q(w, "br"))));
            }
            c => piece.push(c),
        }
    }
    flush(&mut piece, &mut r);
    r
}

fn set_tag(sdt: &mut El, w: &str, tag: &str) {
    if let Some(t) = sdt.child_mut(&q(w, "sdtPr")).and_then(|p| p.child_mut(&q(w, "tag"))) {
        t.set_attr(&q(w, "val"), tag);
    }
}

/// 改写行内引文控件的内容（保留第一个 run 的字体、字号、语言）
fn set_inline(sdt: &mut El, w: &str, runs: &[Run], retag: Option<&str>) {
    if let Some(tag) = retag {
        set_tag(sdt, w, tag);
    }
    let Some(content) = sdt.child_mut(&q(w, "sdtContent")) else { return };
    let base = first_run(content, w).and_then(|r| r.child(&q(w, "rPr")).cloned());
    content.children = runs.iter().map(|r| Node::El(build_run(w, base.as_ref(), r))).collect();
}

/// 改写块级书目控件：每条一段，悬挂缩进；段落样式与段前段后沿用原来第一段
fn set_block(sdt: &mut El, w: &str, entries: &[Vec<Run>], hanging_pt: f64) {
    let Some(content) = sdt.child_mut(&q(w, "sdtContent")) else { return };
    let first_p = content.els().find(|e| is_w(e, w, "p")).cloned();
    let base_ppr = first_p.as_ref().and_then(|p| p.child(&q(w, "pPr")).cloned());
    let base_rpr = first_p.as_ref().and_then(|p| first_run(p, w)).and_then(|r| r.child(&q(w, "rPr")).cloned());
    let keep = |name: &str| base_ppr.as_ref().and_then(|p| p.child(&q(w, name)).cloned());
    let (style, spacing) = (keep("pStyle"), keep("spacing"));
    content.children = entries
        .iter()
        .map(|runs| {
            let mut ppr = El::new(q(w, "pPr"));
            for e in [style.clone(), spacing.clone()].into_iter().flatten() {
                ppr.children.push(Node::El(e));
            }
            if hanging_pt > 0.0 {
                let tw = ((hanging_pt * 20.0).round() as i64).to_string();
                ppr.children.push(Node::El(El::new(q(w, "ind")).with_attr(&q(w, "left"), &tw).with_attr(&q(w, "hanging"), &tw)));
            }
            let mut p = El::new(q(w, "p"));
            if !ppr.children.is_empty() {
                p.children.push(Node::El(ppr));
            }
            for r in runs {
                p.children.push(Node::El(build_run(w, base_rpr.as_ref(), r)));
            }
            Node::El(p)
        })
        .collect();
}

// ---------- 数据部件 ----------

fn store_part(pkg: &Package) -> Option<String> {
    let mut names: Vec<(u32, &str)> = pkg
        .names()
        .into_iter()
        .filter_map(|n| n.strip_prefix("customXml/item").and_then(|r| r.strip_suffix(".xml")).and_then(|d| d.parse().ok()).map(|i| (i, n)))
        .collect();
    names.sort();
    names.into_iter().map(|(_, n)| n).find(|n| pkg.get_str(n).is_ok_and(|s| store::looks_like_store(&s))).map(str::to_string)
}

pub fn read_store(pkg: &Package) -> Option<Result<DocStore, StoreError>> {
    store_part(pkg).map(|p| store::parse(&pkg.get_str(&p).unwrap_or_default()))
}

fn guid_from(seed: &str) -> String {
    let h = Sha256::digest(seed.as_bytes());
    let x: String = h.iter().take(16).map(|b| format!("{b:02X}")).collect();
    format!("{{{}-{}-{}-{}-{}}}", &x[0..8], &x[8..12], &x[12..16], &x[16..20], &x[20..32])
}

fn rels_root(pkg: &Package, path: &str) -> Result<XmlDoc, String> {
    match pkg.get_str(path) {
        Ok(s) => xml::parse(&s).map_err(|e| e.to_string()),
        Err(_) => xml::parse(&format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PKG_REL_NS}"></Relationships>"#)).map_err(|e| e.to_string()),
    }
}

fn write_store(pkg: &mut Package, store: &DocStore) -> Result<(), String> {
    let xml_bytes = store::serialize(store).into_bytes();
    if let Some(part) = store_part(pkg) {
        pkg.set(&part, xml_bytes);
        // 0-B 原型的部件：数据属性里的命名空间一起换成新的
        let props = part.replace("/item", "/itemProps");
        if let Ok(s) = pkg.get_str(&props)
            && s.contains(LEGACY_STORE_NS)
        {
            pkg.set(&props, s.replace(LEGACY_STORE_NS, STORE_NS).into_bytes());
        }
        return Ok(());
    }
    let mut n = 1;
    while pkg.get(&format!("customXml/item{n}.xml")).is_some() || pkg.get(&format!("customXml/itemProps{n}.xml")).is_some() {
        n += 1;
    }
    let item = format!("customXml/item{n}.xml");
    pkg.set(&item, xml_bytes);
    pkg.set(
        &format!("customXml/itemProps{n}.xml"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><ds:datastoreItem ds:itemID="{}" xmlns:ds="{DS_NS}"><ds:schemaRefs><ds:schemaRef ds:uri="{STORE_NS}"/></ds:schemaRefs></ds:datastoreItem>"#,
            guid_from(&format!("{item}\u{0}{}", store.updated))
        )
        .into_bytes(),
    );
    pkg.set(
        &format!("customXml/_rels/item{n}.xml.rels"),
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{PKG_REL_NS}"><Relationship Id="rId1" Type="{REL_CUSTOM_XML_PROPS}" Target="itemProps{n}.xml"/></Relationships>"#).into_bytes(),
    );
    // 文档主体 → customXml
    let mut rels = rels_root(pkg, DOC_RELS)?;
    let used: BTreeSet<String> = rels.root.els().filter_map(|e| e.attr("Id").map(str::to_string)).collect();
    let mut rid = 1;
    while used.contains(&format!("rIdPith{rid}")) {
        rid += 1;
    }
    let p = rels.root.prefix().to_string();
    let rel = El::new(q(&p, "Relationship")).with_attr("Id", &format!("rIdPith{rid}")).with_attr("Type", REL_CUSTOM_XML).with_attr("Target", &format!("../{item}"));
    rels.root.children.push(Node::El(rel));
    pkg.set(DOC_RELS, rels.to_xml().into_bytes());
    // 内容类型
    let mut ct = xml::parse(&pkg.get_str(CONTENT_TYPES).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let p = ct.root.prefix().to_string();
    if !ct.root.els().any(|e| e.local() == "Default" && e.attr("Extension").is_some_and(|x| x.eq_ignore_ascii_case("xml"))) {
        let d = El::new(q(&p, "Default")).with_attr("Extension", "xml").with_attr("ContentType", "application/xml");
        ct.root.children.insert(0, Node::El(d));
    }
    let o = El::new(q(&p, "Override")).with_attr("PartName", &format!("/customXml/itemProps{n}.xml")).with_attr("ContentType", CT_ITEM_PROPS);
    ct.root.children.push(Node::El(o));
    pkg.set(CONTENT_TYPES, ct.to_xml().into_bytes());
    Ok(())
}

// ---------- 整个文档 ----------

pub struct CiteDoc {
    pub pkg: Package,
    pub(crate) doc: XmlDoc,
    pub(crate) w: String,
}

pub fn open(bytes: &[u8]) -> Result<CiteDoc, String> {
    let pkg = Package::read(bytes).map_err(|e| e.to_string())?;
    let doc = xml::parse(&pkg.get_str(DOCUMENT).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let w = doc.prefix_of(NS_W).ok_or("document.xml 没有声明 WordprocessingML 命名空间")?;
    Ok(CiteDoc { pkg, doc, w })
}

impl CiteDoc {
    pub fn store(&self) -> Option<Result<DocStore, StoreError>> {
        read_store(&self.pkg)
    }

    fn sdts(&self) -> Vec<&El> {
        let mut out = Vec::new();
        collect_sdts(&self.doc.root, &self.w, &mut out);
        out
    }

    /// 文档里按顺序出现的引文控件（tag + 现有文字签名）
    pub fn citations(&self) -> Vec<DocCitation> {
        self.sdts()
            .into_iter()
            .filter(|s| citation_id(&tag_of(s, &self.w)).is_some())
            .map(|s| {
                let runs = s.child(&q(&self.w, "sdtContent")).map(|c| read_runs(c, &self.w)).unwrap_or_default();
                DocCitation { tag: tag_of(s, &self.w), signature: Some(runs_signature(&runs)) }
            })
            .collect()
    }

    /// 按计划改写引文、书目与数据部件
    pub fn apply(&mut self, plan: &Plan) -> Result<(), String> {
        let updates: BTreeMap<usize, _> = plan.citations.iter().filter(|u| u.changed).map(|u| (u.index, u)).collect();
        let paragraphs = plan.bibliography.paragraphs();
        let w = self.w.clone();
        let mut index = 0;
        for_each_sdt_mut(&mut self.doc.root, &w, &mut |sdt| {
            let tag = tag_of(sdt, &w);
            if citation_id(&tag).is_some() {
                if let Some(u) = updates.get(&index) {
                    set_inline(sdt, &w, &u.runs, u.retag.as_deref());
                }
                index += 1;
            } else if is_bibliography(&tag) {
                set_block(sdt, &w, &paragraphs, plan.bibliography.hanging_indent_pt);
                if tag != super::BIB_TAG {
                    set_tag(sdt, &w, super::BIB_TAG);
                }
            }
        });
        write_store(&mut self.pkg, &plan.store)
    }

    pub fn into_bytes(mut self) -> Result<Vec<u8>, String> {
        let xml = self.doc.to_xml();
        self.pkg.set(DOCUMENT, xml.into_bytes());
        self.pkg.write().map_err(|e| e.to_string())
    }
}

pub struct Refreshed {
    pub bytes: Vec<u8>,
    pub plan: Plan,
}

/// 刷新（可同时切换样式）。`library` 按 uid 取文献库里的最新 CSL-JSON。
/// 写完立即独立核对；核对不通过就报错，不交付。
pub fn refresh(
    bytes: &[u8],
    style: Option<&str>,
    library: Library,
    render: &dyn Render,
    new_id: &mut dyn FnMut() -> String,
    now: &str,
) -> Result<Refreshed, String> {
    let mut d = open(bytes)?;
    let mut store = match d.store() {
        None => return Err("文档里没有 Pith 引文数据".into()),
        Some(Err(e)) => return Err(e.zh()),
        Some(Ok(s)) => s,
    };
    if let Some(s) = style {
        store.style = s.to_string();
    }
    let doc = d.citations();
    let lib = library(&referenced_items(&doc, &store))?;
    let plan = plan_refresh(&doc, &store, &lib, render, new_id, now)?;
    d.apply(&plan)?;
    let out = d.into_bytes()?;
    let before = check::read(bytes)?;
    let after = check::read(&out)?;
    let mut problems = check::outside_changed(&before, &after);
    problems.extend(check::against_plan(&after, &plan));
    if !problems.is_empty() {
        return Err(format!("写入后核对没通过，没有交付：{}", problems.join("；")));
    }
    Ok(Refreshed { bytes: out, plan })
}
