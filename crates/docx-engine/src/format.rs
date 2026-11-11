//! 排版器：按规范改样式、页面设置，清理与样式冲突的直接格式。只碰格式节点，不碰文字。
//!
//! - 样式按「内置样式名」找（heading 1、caption…），不按 ID（中文版 Word 的 ID 是 1、a5 这类）
//! - 设置字体时去掉主题字体属性（asciiTheme 等），否则主题会盖过显式字体
//! - 代码段落保留等宽字体；列表段落保留缩进；公式里的 run 不动
//! - 修订、批注、文本框、嵌入对象不处理，只在报告里列出
//! - 文档排版计划（Plan）：粘贴来的正文样式映射回正文、按文字识别题注、受保护的分节（封面）冻结原外观

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::package::{story_parts, Package};
use crate::plan::Plan;
use crate::spec::{cm_to_twips, same_font, Spec, TextStyle, A4_TWIPS};
use crate::xml::{self, unescape, El, XmlDoc};
use crate::{Error, Result, NS_M, NS_W};

const P_ORDER: &[&str] = &[
    "pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs",
    "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd",
    "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment",
    "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange",
];
const R_ORDER: &[&str] = &[
    "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof",
    "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd",
    "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout", "specVanish", "oMath", "rPrChange",
];
const STYLE_ORDER: &[&str] = &[
    "name", "aliases", "basedOn", "next", "link", "autoRedefine", "hidden", "uiPriority", "semiHidden", "unhideWhenUsed", "qFormat", "locked",
    "personal", "personalCompose", "personalReply", "rsid", "pPr", "rPr", "tblPr", "trPr", "tcPr", "tblStylePr",
];
const SECT_ORDER: &[&str] = &[
    "headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz", "pgMar", "paperSrc", "pgBorders", "lnNumType", "pgNumType",
    "cols", "formProt", "vAlign", "noEndnote", "titlePg", "textDirection", "bidi", "rtlGutter", "docGrid", "printerSettings", "sectPrChange",
];
const STYLES_ROOT_ORDER: &[&str] = &["docDefaults", "latentStyles", "style"];
const DEFAULTS_ORDER: &[&str] = &["rPrDefault", "pPrDefault"];

pub const TABLE_TEXT_STYLE: (&str, &str) = ("RLTableText", "RL Table Text");
pub const FIGURE_STYLE: (&str, &str) = ("RLFigure", "RL Figure");

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub spec: String,
    pub styles_changed: Vec<String>,
    pub styles_created: Vec<String>,
    pub parts_changed: Vec<String>,
    pub runs_cleaned: usize,
    pub paragraphs_cleaned: usize,
    pub table_paragraphs: usize,
    pub figure_paragraphs: usize,
    pub code_paragraphs_kept: usize,
    pub list_paragraphs_kept_indent: usize,
    pub sections: usize,
    /// 样式映射：「原样式 → 目标样式」→ 段落数
    pub remapped: BTreeMap<String, usize>,
    /// 受保护分节里的段落数，以及为冻结外观写入的直接格式个数
    pub protected_paragraphs: usize,
    pub frozen_properties: usize,
    /// 未处理、原样保留的结构
    pub untouched: BTreeMap<String, usize>,
    pub notes: Vec<String>,
    /// 结构类排版做了什么（规范里有结构类要求时）
    pub structure: Option<crate::structure::StructReport>,
}

// ---------- 样式 ----------

struct Book {
    /// styleId → (小写名, 类型, basedOn)
    styles: BTreeMap<String, (String, String, Option<String>)>,
    normal_id: String,
    code_ids: HashSet<String>,
}

impl Book {
    fn read(doc: &XmlDoc, w: &str, spec: &Spec) -> Book {
        let mut styles = BTreeMap::new();
        let mut normal_id = String::new();
        for s in doc.root.els().filter(|e| e.name == format!("{w}:style")) {
            let id = s.attr(&format!("{w}:styleId")).unwrap_or("").to_string();
            let name = s.child(&format!("{w}:name")).and_then(|n| n.attr(&format!("{w}:val"))).unwrap_or("").to_lowercase();
            let ty = s.attr(&format!("{w}:type")).unwrap_or("").to_string();
            let based = s.child(&format!("{w}:basedOn")).and_then(|n| n.attr(&format!("{w}:val"))).map(String::from);
            if ty == "paragraph" && (name == "normal" || s.attr(&format!("{w}:default")) == Some("1")) && normal_id.is_empty() {
                normal_id = id.clone();
            }
            styles.insert(id, (name, ty, based));
        }
        let wanted: Vec<String> = spec.code.as_ref().map(|c| c.style_names.iter().map(|s| s.to_lowercase()).collect()).unwrap_or_default();
        let code_ids = styles
            .iter()
            .filter(|(id, (name, ty, _))| ty == "paragraph" && (wanted.contains(name) || name.contains("code") || name.contains("代码") || id.to_lowercase().contains("code")))
            .map(|(id, _)| id.clone())
            .collect();
        Book { styles, normal_id, code_ids }
    }

    fn id_by_name(&self, name: &str) -> Option<String> {
        self.styles.iter().find(|(_, (n, _, _))| n == name).map(|(id, _)| id.clone())
    }
}

fn q(w: &str, local: &str) -> String {
    format!("{w}:{local}")
}

fn twips(pt: f64) -> String {
    format!("{}", (pt * 20.0).round() as i64)
}

fn set_fonts(rpr: &mut El, w: &str, latin: Option<&str>, east: Option<&str>) {
    if latin.is_none() && east.is_none() {
        return;
    }
    let f = rpr.ensure_child(&q(w, "rFonts"), w, R_ORDER);
    f.remove_attrs_where(|k| k.ends_with("Theme") || k.ends_with(":cstheme"));
    if let Some(l) = latin {
        f.set_attr(&q(w, "ascii"), l);
        f.set_attr(&q(w, "hAnsi"), l);
        f.set_attr(&q(w, "cs"), l);
    }
    if let Some(e) = east {
        f.set_attr(&q(w, "eastAsia"), e);
    }
}

fn apply_text_style(style: &mut El, w: &str, ts: &TextStyle, body_size: f64) {
    {
        let rpr = style.ensure_child(&q(w, "rPr"), w, STYLE_ORDER);
        set_fonts(rpr, w, ts.font_latin.as_deref(), ts.font_east_asia.as_deref());
        if let Some(b) = ts.bold {
            for tag in ["b", "bCs"] {
                let e = rpr.ensure_child(&q(w, tag), w, R_ORDER);
                if b {
                    e.remove_attr(&q(w, "val"));
                } else {
                    e.set_attr(&q(w, "val"), "0");
                }
            }
        }
        if let Some(c) = &ts.color {
            let e = rpr.ensure_child(&q(w, "color"), w, R_ORDER);
            e.remove_attrs_where(|k| k.contains(":theme"));
            e.set_attr(&q(w, "val"), c);
        }
        if let Some(pt) = ts.size_pt {
            let v = format!("{}", (pt * 2.0).round() as i64);
            rpr.ensure_child(&q(w, "sz"), w, R_ORDER).set_attr(&q(w, "val"), &v);
            rpr.ensure_child(&q(w, "szCs"), w, R_ORDER).set_attr(&q(w, "val"), &v);
        }
        if rpr.children.is_empty() {
            style.remove_children(&q(w, "rPr"));
        }
    }
    let needs_ppr = ts.align.is_some()
        || ts.line_spacing.is_some()
        || ts.line_spacing_exact_pt.is_some()
        || ts.space_before_pt.is_some()
        || ts.space_after_pt.is_some()
        || ts.first_line_indent_chars.is_some()
        || ts.page_break_before.is_some();
    if !needs_ppr {
        return;
    }
    let ppr = style.ensure_child(&q(w, "pPr"), w, STYLE_ORDER);
    if let Some(b) = ts.page_break_before {
        let e = ppr.ensure_child(&q(w, "pageBreakBefore"), w, P_ORDER);
        if b {
            e.remove_attr(&q(w, "val"));
        } else {
            e.set_attr(&q(w, "val"), "0");
        }
    }
    if ts.line_spacing.is_some() || ts.line_spacing_exact_pt.is_some() || ts.space_before_pt.is_some() || ts.space_after_pt.is_some() {
        let sp = ppr.ensure_child(&q(w, "spacing"), w, P_ORDER);
        sp.remove_attrs_where(|k| k.ends_with("Lines") || k.ends_with("Autospacing"));
        if let Some(b) = ts.space_before_pt {
            sp.set_attr(&q(w, "before"), &twips(b));
        }
        if let Some(a) = ts.space_after_pt {
            sp.set_attr(&q(w, "after"), &twips(a));
        }
        if let Some(m) = ts.line_spacing {
            sp.set_attr(&q(w, "line"), &format!("{}", (240.0 * m).round() as i64));
            sp.set_attr(&q(w, "lineRule"), "auto");
        }
        if let Some(pt) = ts.line_spacing_exact_pt {
            sp.set_attr(&q(w, "line"), &twips(pt));
            sp.set_attr(&q(w, "lineRule"), "exact");
        }
    }
    if let Some(c) = ts.first_line_indent_chars {
        let ind = ppr.ensure_child(&q(w, "ind"), w, P_ORDER);
        ind.remove_attrs_where(|k| k.ends_with(":hanging") || k.ends_with(":hangingChars") || k.ends_with(":firstLine") || k.ends_with(":firstLineChars"));
        let size = ts.size_pt.unwrap_or(body_size);
        ind.set_attr(&q(w, "firstLineChars"), &format!("{}", (c * 100.0).round() as i64));
        ind.set_attr(&q(w, "firstLine"), &format!("{}", (c * size * 20.0).round() as i64));
    }
    if let Some(a) = &ts.align {
        ppr.ensure_child(&q(w, "jc"), w, P_ORDER).set_attr(&q(w, "val"), a);
    }
}

fn style_mut<'a>(root: &'a mut El, w: &str, id: &str) -> Option<&'a mut El> {
    let tag = q(w, "style");
    let key = q(w, "styleId");
    root.els_mut().find(|e| e.name == tag && e.attr(&key) == Some(id))
}

fn create_style(root: &mut El, w: &str, id: &str, name: &str, based_on: &str) {
    let mut s = El::new(q(w, "style")).with_attr(&q(w, "type"), "paragraph").with_attr(&q(w, "customStyle"), "1").with_attr(&q(w, "styleId"), id);
    s.children.push(xml::Node::El(El::new(q(w, "name")).with_attr(&q(w, "val"), name)));
    s.children.push(xml::Node::El(El::new(q(w, "basedOn")).with_attr(&q(w, "val"), based_on)));
    s.children.push(xml::Node::El(El::new(q(w, "qFormat"))));
    root.children.push(xml::Node::El(s));
}

/// 内置样式缺失时（Word 只写出用到的样式定义）按内置名新建，Word 按名字识别它们。
fn create_builtin(root: &mut El, w: &str, name: &str, normal: &str, existing: &BTreeMap<String, (String, String, Option<String>)>) -> Option<String> {
    let base_id = if let Some(n) = name.strip_prefix("heading ") {
        format!("Heading{n}")
    } else if let Some(n) = name.strip_prefix("toc ") {
        format!("TOC{n}")
    } else {
        match name {
            "caption" => "Caption".into(),
            "bibliography" => "Bibliography".into(),
            "footnote text" => "FootnoteText".into(),
            "endnote text" => "EndnoteText".into(),
            "header" => "Header".into(),
            "footer" => "Footer".into(),
            _ => return None,
        }
    };
    let mut id = base_id.clone();
    let mut k = 1;
    while existing.contains_key(&id) {
        id = format!("{base_id}RL{k}");
        k += 1;
    }
    let mut s = El::new(q(w, "style")).with_attr(&q(w, "type"), "paragraph").with_attr(&q(w, "styleId"), &id);
    s.children.push(xml::Node::El(El::new(q(w, "name")).with_attr(&q(w, "val"), name)));
    s.children.push(xml::Node::El(El::new(q(w, "basedOn")).with_attr(&q(w, "val"), normal)));
    s.children.push(xml::Node::El(El::new(q(w, "next")).with_attr(&q(w, "val"), normal)));
    s.children.push(xml::Node::El(El::new(q(w, "qFormat"))));
    if let Some(n) = name.strip_prefix("heading ").and_then(|n| n.parse::<u8>().ok()) {
        let mut ppr = El::new(q(w, "pPr"));
        ppr.children.push(xml::Node::El(El::new(q(w, "keepNext"))));
        ppr.children.push(xml::Node::El(El::new(q(w, "keepLines"))));
        ppr.children.push(xml::Node::El(El::new(q(w, "outlineLvl")).with_attr(&q(w, "val"), &(n - 1).to_string())));
        s.children.push(xml::Node::El(ppr));
    }
    root.children.push(xml::Node::El(s));
    Some(id)
}

fn format_styles(pkg: &mut Package, spec: &Spec, plan: &Plan, rep: &mut Report) -> Result<Book> {
    let src = pkg.get_str("word/styles.xml")?;
    let mut doc = xml::parse(&src)?;
    let w = doc.prefix_of(NS_W).ok_or_else(|| Error::Xml("styles.xml 没有 WordprocessingML 命名空间".into()))?;
    let mut book = Book::read(&doc, &w, spec);
    let body_size = spec.body.size_pt.unwrap_or(12.0);

    // 文档默认值：字体与字号（去掉主题字体）
    {
        let defaults = doc.root.ensure_child(&q(&w, "docDefaults"), &w, STYLES_ROOT_ORDER);
        let rprd = defaults.ensure_child(&q(&w, "rPrDefault"), &w, DEFAULTS_ORDER);
        let rpr = rprd.ensure_child(&q(&w, "rPr"), &w, &["rPr"]);
        set_fonts(rpr, &w, spec.body.font_latin.as_deref(), spec.body.font_east_asia.as_deref());
        if let Some(pt) = spec.body.size_pt {
            let v = format!("{}", (pt * 2.0).round() as i64);
            rpr.ensure_child(&q(&w, "sz"), &w, R_ORDER).set_attr(&q(&w, "val"), &v);
            rpr.ensure_child(&q(&w, "szCs"), &w, R_ORDER).set_attr(&q(&w, "val"), &v);
        }
        rep.styles_changed.push("文档默认值".into());
    }

    // 需要的自建样式
    let normal = book.normal_id.clone();
    if spec.table_text.is_some() && book.id_by_name(&TABLE_TEXT_STYLE.1.to_lowercase()).is_none() {
        create_style(&mut doc.root, &w, TABLE_TEXT_STYLE.0, TABLE_TEXT_STYLE.1, &normal);
        rep.styles_created.push(TABLE_TEXT_STYLE.1.into());
    }
    if spec.figure.is_some() && book.id_by_name(&FIGURE_STYLE.1.to_lowercase()).is_none() {
        create_style(&mut doc.root, &w, FIGURE_STYLE.0, FIGURE_STYLE.1, &normal);
        rep.styles_created.push(FIGURE_STYLE.1.into());
    }
    book = Book::read(&doc, &w, spec);

    // (样式名, 规范, 报告名)
    let mut targets: Vec<(String, &TextStyle, String)> = vec![("normal".into(), &spec.body, "正文 Normal".into())];
    for h in &spec.headings {
        targets.push((format!("heading {}", h.level), &h.style, format!("标题 {}", h.level)));
    }
    for (names, ts, label) in [
        (vec!["caption"], spec.caption.as_ref(), "题注"),
        (vec!["footnote text", "endnote text"], spec.footnote.as_ref(), "脚注 / 尾注"),
        (vec!["bibliography"], spec.bibliography.as_ref(), "参考文献"),
        (vec!["header", "footer"], spec.header_footer.as_ref(), "页眉 / 页脚"),
        (vec!["toc 1", "toc 2", "toc 3"], spec.toc.as_ref(), "目录"),
        (vec!["rl table text"], spec.table_text.as_ref(), "表格文字"),
        (vec!["rl figure"], spec.figure.as_ref(), "图片与公式段落"),
    ] {
        if let Some(ts) = ts {
            for n in names {
                targets.push((n.to_string(), ts, format!("{label}（{n}）")));
            }
        }
    }
    // 排版计划要用到的目标样式：缺了就按内置名新建，建不了就报错
    for t in plan.targets() {
        if book.id_by_name(&t).is_none() {
            match create_builtin(&mut doc.root, &w, &t, &normal, &book.styles) {
                Some(id) => {
                    rep.styles_created.push(format!("{t}（{id}）"));
                    book = Book::read(&doc, &w, spec);
                }
                None => return Err(Error::Spec(format!("排版计划里的样式「{t}」在文档里不存在，也不是能自动新建的内置样式"))),
            }
        }
    }
    for (name, _, _) in &targets {
        if book.id_by_name(name).is_none()
            && let Some(id) = create_builtin(&mut doc.root, &w, name, &normal, &book.styles) {
                rep.styles_created.push(format!("{name}（{id}）"));
                book = Book::read(&doc, &w, spec);
            }
    }
    for (name, ts, label) in targets {
        match book.id_by_name(&name) {
            Some(id) => {
                if let Some(s) = style_mut(&mut doc.root, &w, &id) {
                    apply_text_style(s, &w, ts, body_size);
                    rep.styles_changed.push(label);
                }
            }
            None => rep.notes.push(format!("文档里没有样式「{name}」，跳过（{label}）")),
        }
    }
    if let Some(code) = &spec.code {
        let ids: Vec<String> = book.code_ids.iter().cloned().collect();
        for id in ids {
            if let Some(s) = style_mut(&mut doc.root, &w, &id) {
                apply_text_style(s, &w, &code.style, body_size);
                rep.styles_changed.push(format!("代码（{id}）"));
            }
        }
    }
    pkg.set("word/styles.xml", doc.to_xml().into_bytes());
    Ok(book)
}

// ---------- 受保护段落：冻结外观 ----------

/// 元素本地名 → 属性（本地名 → 未转义值）
type Props = BTreeMap<String, BTreeMap<String, String>>;

const FREEZE_P: &[&str] = &["spacing", "ind", "jc", "pageBreakBefore"];
const FREEZE_R: &[&str] = &["rFonts", "b", "bCs", "color", "sz", "szCs"];
const TOGGLES: &[&str] = &["pageBreakBefore", "b", "bCs"];

/// 样式解析（排版器一侧）：沿 basedOn 链合并 pPr / rPr 的子元素。用于冻结受保护段落的外观。
struct Resolver {
    /// styleId → (basedOn, pPr 属性, rPr 属性)
    styles: BTreeMap<String, (Option<String>, Props, Props)>,
    default_para: Option<String>,
    defaults_p: Props,
    defaults_r: Props,
}

fn props_of(container: Option<&El>, keys: &[&str]) -> Props {
    let mut out = Props::new();
    if let Some(c) = container {
        for e in c.els().filter(|e| keys.contains(&e.local())) {
            let attrs = e.attrs.iter().filter_map(|(k, v)| k.split_once(':').map(|(_, l)| (l.to_string(), unescape(v)))).collect();
            out.insert(e.local().to_string(), attrs);
        }
    }
    out
}

/// 把 top 合并到 acc 上（top 优先）。字体按「显式字体 / 主题字体」成对覆盖。
fn merge(acc: &mut Props, top: &Props) {
    for (key, attrs) in top {
        let slot = acc.entry(key.clone()).or_default();
        for (k, v) in attrs {
            if key == "rFonts" {
                let pair = match k.as_str() {
                    "cstheme" => "cs".to_string(),
                    "cs" => "cstheme".to_string(),
                    k if k.ends_with("Theme") => k.trim_end_matches("Theme").to_string(),
                    k => format!("{k}Theme"),
                };
                slot.remove(&pair);
            }
            slot.insert(k.clone(), v.clone());
        }
    }
}

impl Resolver {
    fn read(doc: &XmlDoc, w: &str) -> Resolver {
        let mut styles = BTreeMap::new();
        let mut default_para = None;
        for s in doc.root.els().filter(|e| e.name == q(w, "style")) {
            let id = s.attr(&q(w, "styleId")).unwrap_or("").to_string();
            if s.attr(&q(w, "type")) == Some("paragraph") && s.attr(&q(w, "default")) == Some("1") && default_para.is_none() {
                default_para = Some(id.clone());
            }
            let based = s.child(&q(w, "basedOn")).and_then(|n| n.attr(&q(w, "val"))).map(String::from);
            styles.insert(id, (based, props_of(s.child(&q(w, "pPr")), FREEZE_P), props_of(s.child(&q(w, "rPr")), FREEZE_R)));
        }
        let defaults = doc.root.child(&q(w, "docDefaults"));
        let inner = |outer: &str, c: &str| defaults.and_then(|d| d.child(&q(w, outer))).and_then(|o| o.child(&q(w, c)));
        Resolver { styles, default_para, defaults_p: props_of(inner("pPrDefault", "pPr"), FREEZE_P), defaults_r: props_of(inner("rPrDefault", "rPr"), FREEZE_R) }
    }

    /// 样式链，最底层（最远的 basedOn）在前
    fn chain(&self, id: Option<&str>) -> Vec<&(Option<String>, Props, Props)> {
        let mut out = Vec::new();
        let mut cur = id.map(String::from);
        while let Some(i) = cur {
            if out.len() > 20 {
                break;
            }
            let Some(s) = self.styles.get(&i) else { break };
            out.push(s);
            cur = s.0.clone();
        }
        out.reverse();
        out
    }

    fn para(&self, pstyle: Option<&str>) -> Props {
        let mut acc = self.defaults_p.clone();
        for s in self.chain(pstyle.or(self.default_para.as_deref())) {
            merge(&mut acc, &s.1);
        }
        acc
    }

    fn run(&self, pstyle: Option<&str>, rstyle: Option<&str>) -> Props {
        let mut acc = self.defaults_r.clone();
        for s in self.chain(pstyle.or(self.default_para.as_deref())) {
            merge(&mut acc, &s.2);
        }
        for s in self.chain(rstyle) {
            merge(&mut acc, &s.2);
        }
        acc
    }
}

/// 新外观与旧外观不同时要写入的直接格式：旧值为准；新的一侧多出来的属性写成默认值。
fn frozen(key: &str, old: Option<&BTreeMap<String, String>>, new: Option<&BTreeMap<String, String>>) -> Option<BTreeMap<String, String>> {
    if old == new {
        return None;
    }
    if TOGGLES.contains(&key) {
        let on = |m: Option<&BTreeMap<String, String>>| m.is_some_and(|a| !matches!(a.get("val").map(String::as_str), Some("0" | "false" | "off")));
        if on(old) == on(new) {
            return None;
        }
        return Some(if on(old) { BTreeMap::new() } else { BTreeMap::from([("val".to_string(), "0".to_string())]) });
    }
    let mut out = old.cloned().unwrap_or_default();
    if key == "rFonts" {
        // 旧值里的主题字体、显式字体原样写回即可覆盖样式；旧值缺失时无法表达「默认字体」，跳过
        return if out.is_empty() { None } else { Some(out) };
    }
    for k in new.map(|n| n.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let v = match (key, k.as_str()) {
            (_, "lineRule") => "auto",
            ("spacing", "line") => "240",
            ("jc", "val") => "left",
            ("color", "val") => "auto",
            ("sz" | "szCs", "val") => "20",
            _ => "0",
        };
        out.entry(k).or_insert_with(|| v.to_string());
    }
    Some(out)
}

/// 段落文字（不含文本框、公式内部）
fn text_of(el: &El, w: &str, out: &mut String) {
    for c in &el.children {
        if let xml::Node::El(e) = c {
            if e.name == q(w, "t") {
                for t in &e.children {
                    if let xml::Node::Raw(r) = t {
                        out.push_str(&unescape(r));
                    }
                }
            } else if (e.prefix() == w && e.name != q(w, "txbxContent")) || (e.prefix() != w && !e.prefix().is_empty() && e.local() != "oMath" && e.local() != "oMathPara") {
                text_of(e, w, out);
            }
        }
    }
}

// ---------- 正文、脚注、页眉页脚 ----------

struct Walker<'a> {
    w: String,
    m: String,
    spec: &'a Spec,
    plan: &'a Plan,
    book: &'a Book,
    old: &'a Resolver,
    new: &'a Resolver,
    rep: &'a mut Report,
    table_style: Option<String>,
    figure_style: Option<String>,
    bib_style: Option<String>,
    /// 只在 document.xml 里按分节计数
    track_sections: bool,
    sect_no: usize,
}

impl Walker<'_> {
    fn n(&self, local: &str) -> String {
        q(&self.w, local)
    }

    fn block(&mut self, el: &mut El, in_table: bool) {
        let (p, tbl, sect) = (self.n("p"), self.n("tbl"), self.n("sectPr"));
        for i in 0..el.children.len() {
            let xml::Node::El(child) = &mut el.children[i] else { continue };
            if child.name == p {
                self.paragraph(child, in_table);
            } else if child.name == tbl {
                self.block(child, true);
            } else if child.name == sect {
                self.section(child);
            } else if !child.name.starts_with(&format!("{}:", self.m)) {
                self.block(child, in_table);
            }
        }
    }

    fn pstyle(&self, p: &El) -> Option<String> {
        let w = &self.w;
        p.child(&q(w, "pPr")).and_then(|pp| pp.child(&q(w, "pStyle"))).and_then(|s| s.attr(&q(w, "val"))).map(String::from)
    }

    /// 按排版计划把段落改到目标样式（文字规则优先于样式映射）
    fn remap(&mut self, p: &mut El) {
        if self.plan.style_map.is_empty() && self.plan.paragraph_rules.is_empty() {
            return;
        }
        let w = self.w.clone();
        let sid = self.pstyle(p).unwrap_or_else(|| self.book.normal_id.clone());
        let name = self.book.styles.get(&sid).map(|s| s.0.clone()).unwrap_or_default();
        let mut text = String::new();
        text_of(p, &w, &mut text);
        let (target, how) = match self.plan.rule_for(&text) {
            Some(t) => (t, "文字规则"),
            None => match self.plan.style_map.get(&name) {
                Some(t) => (t.clone(), "样式映射"),
                None => return,
            },
        };
        if target == name {
            return;
        }
        let Some(id) = self.book.id_by_name(&target) else { return };
        let ppr = p.ensure_child(&q(&w, "pPr"), &w, &["pPr"]);
        if id == self.book.normal_id {
            ppr.remove_children(&q(&w, "pStyle"));
        } else {
            ppr.ensure_child(&q(&w, "pStyle"), &w, P_ORDER).set_attr(&q(&w, "val"), &id);
        }
        *self.rep.remapped.entry(format!("{name} → {target}（{how}）")).or_default() += 1;
    }

    fn paragraph(&mut self, p: &mut El, in_table: bool) {
        let w = self.w.clone();
        if self.track_sections && self.plan.is_protected(self.sect_no) {
            self.freeze_paragraph(p);
        } else {
            self.remap(p);
            self.format_paragraph(p, in_table);
        }
        if let Some(sect) = p.child_mut(&q(&w, "pPr")).and_then(|pp| pp.child_mut(&q(&w, "sectPr"))) {
            self.section(sect);
            if self.track_sections {
                self.sect_no += 1;
            }
        }
    }

    fn format_paragraph(&mut self, p: &mut El, in_table: bool) {
        let w = self.w.clone();
        let style_id = self.pstyle(p);
        let is_code = style_id.as_ref().map(|s| self.book.code_ids.contains(s)).unwrap_or(false);
        let has_num = p.child(&q(&w, "pPr")).map(|pp| pp.child(&q(&w, "numPr")).is_some()).unwrap_or(false);
        let is_bib = style_id.is_some() && style_id == self.bib_style;
        let drawing = q(&w, "drawing");
        let pict = q(&w, "pict");
        let has_figure = p.has_descendant(&|e: &El| e.name == drawing || e.name == pict) || p.child(&q(&self.m, "oMathPara")).is_some();
        let plain = style_id.is_none() || style_id.as_deref() == Some(self.book.normal_id.as_str());
        let df = &self.spec.direct_formatting;

        if is_code {
            self.rep.code_paragraphs_kept += 1;
        } else {
            if let Some(ppr) = p.child_mut(&q(&w, "pPr")) {
                let mut removed = 0;
                if df.strip_paragraph_spacing {
                    removed += ppr.remove_children(&q(&w, "spacing"));
                    removed += ppr.remove_children(&q(&w, "jc"));
                    if has_num || is_bib {
                        if ppr.child(&q(&w, "ind")).is_some() {
                            self.rep.list_paragraphs_kept_indent += 1;
                        }
                    } else {
                        removed += ppr.remove_children(&q(&w, "ind"));
                    }
                }
                if let Some(mark) = ppr.child_mut(&q(&w, "rPr")) {
                    removed += clean_rpr(mark, &w, self.spec);
                }
                if removed > 0 {
                    self.rep.paragraphs_cleaned += 1;
                }
            }
            let target = if in_table && plain {
                self.table_style.clone()
            } else if has_figure && plain {
                self.figure_style.clone()
            } else {
                None
            };
            if let Some(sid) = target {
                let ppr = p.ensure_child(&q(&w, "pPr"), &w, &["pPr"]);
                ppr.ensure_child(&q(&w, "pStyle"), &w, P_ORDER).set_attr(&q(&w, "val"), &sid);
                if in_table {
                    self.rep.table_paragraphs += 1;
                } else {
                    self.rep.figure_paragraphs += 1;
                }
            }
        }
        self.runs(p, is_code);
    }

    /// 受保护段落：不清理、不映射；样式改动若会改变它的外观，就把旧外观写成直接格式。
    fn freeze_paragraph(&mut self, p: &mut El) {
        let w = self.w.clone();
        self.rep.protected_paragraphs += 1;
        let pstyle = self.pstyle(p);
        let direct = props_of(p.child(&q(&w, "pPr")), FREEZE_P);
        let (mut old, mut new) = (self.old.para(pstyle.as_deref()), self.new.para(pstyle.as_deref()));
        merge(&mut old, &direct);
        merge(&mut new, &direct);
        for key in FREEZE_P {
            if let Some(attrs) = frozen(key, old.get(*key), new.get(*key)) {
                let ppr = p.ensure_child(&q(&w, "pPr"), &w, &["pPr"]);
                ppr.put_child(build(&w, key, &attrs), &w, P_ORDER);
                self.rep.frozen_properties += 1;
            }
        }
        // 段落标记（决定空段落的行高）
        let had_ppr = p.child(&q(&w, "pPr")).is_some();
        let mark_rstyle = p.child(&q(&w, "pPr")).and_then(|pp| pp.child(&q(&w, "rPr"))).and_then(|r| r.child(&q(&w, "rStyle"))).and_then(|s| s.attr(&q(&w, "val"))).map(String::from);
        let ppr = p.ensure_child(&q(&w, "pPr"), &w, &["pPr"]);
        let n = self.freeze_rpr(ppr, pstyle.as_deref(), mark_rstyle.as_deref(), true);
        if n == 0 && !had_ppr && ppr.children.is_empty() {
            p.remove_children(&q(&w, "pPr"));
        }
        self.freeze_runs(p, pstyle.as_deref());
    }

    fn freeze_runs(&mut self, el: &mut El, pstyle: Option<&str>) {
        let w = self.w.clone();
        for i in 0..el.children.len() {
            let xml::Node::El(child) = &mut el.children[i] else { continue };
            if child.prefix() == self.m || child.local() == "pPr" {
                continue;
            }
            if child.name == q(&w, "r") {
                let rstyle = child.child(&q(&w, "rPr")).and_then(|r| r.child(&q(&w, "rStyle"))).and_then(|s| s.attr(&q(&w, "val"))).map(String::from);
                self.freeze_rpr(child, pstyle, rstyle.as_deref(), false);
                self.freeze_nested(child);
            } else if child.name != q(&w, "p") {
                self.freeze_runs(child, pstyle);
            }
        }
    }

    /// 文本框里的段落同样冻结
    fn freeze_nested(&mut self, el: &mut El) {
        let (txbx, p) = (q(&self.w, "txbxContent"), q(&self.w, "p"));
        for i in 0..el.children.len() {
            let xml::Node::El(child) = &mut el.children[i] else { continue };
            if child.name == txbx {
                for j in 0..child.children.len() {
                    if let xml::Node::El(para) = &mut child.children[j]
                        && para.name == p {
                            self.freeze_paragraph(para);
                        }
                }
            } else {
                self.freeze_nested(child);
            }
        }
    }

    /// owner 是 w:r（is_mark = false）或 w:pPr（段落标记）。返回写入个数。
    fn freeze_rpr(&mut self, owner: &mut El, pstyle: Option<&str>, rstyle: Option<&str>, is_mark: bool) -> usize {
        let w = self.w.clone();
        let direct = props_of(owner.child(&q(&w, "rPr")), FREEZE_R);
        let (mut old, mut new) = (self.old.run(pstyle, rstyle), self.new.run(pstyle, rstyle));
        merge(&mut old, &direct);
        merge(&mut new, &direct);
        let mut n = 0;
        for key in FREEZE_R {
            if let Some(attrs) = frozen(key, old.get(*key), new.get(*key)) {
                let rpr = if is_mark { owner.ensure_child(&q(&w, "rPr"), &w, P_ORDER) } else { owner.ensure_child(&q(&w, "rPr"), &w, &["rPr"]) };
                rpr.put_child(build(&w, key, &attrs), &w, R_ORDER);
                n += 1;
            }
        }
        self.rep.frozen_properties += n;
        n
    }

    fn runs(&mut self, el: &mut El, code: bool) {
        let w = self.w.clone();
        for i in 0..el.children.len() {
            let xml::Node::El(child) = &mut el.children[i] else { continue };
            if child.prefix() == self.m {
                continue; // 公式内部的 run（含 Cambria Math 字体）不动
            }
            match child.local() {
                "pPr" => {}
                "r" if child.prefix() == w => {
                    if !code
                        && let Some(rpr) = child.child_mut(&q(&w, "rPr"))
                            && clean_rpr(rpr, &w, self.spec) > 0 {
                                self.rep.runs_cleaned += 1;
                            }
                    self.nested(child);
                }
                "p" if child.prefix() == w => self.paragraph(child, false),
                _ => self.runs(child, code),
            }
        }
    }

    /// run 里的文本框（w:txbxContent）按正文处理
    fn nested(&mut self, el: &mut El) {
        let txbx = q(&self.w, "txbxContent");
        for i in 0..el.children.len() {
            let xml::Node::El(child) = &mut el.children[i] else { continue };
            if child.name == txbx {
                self.block(child, false);
            } else {
                self.nested(child);
            }
        }
    }

    fn section(&mut self, sect: &mut El) {
        let w = self.w.clone();
        let page = &self.spec.page;
        if page.paper.is_some() {
            let sz = sect.ensure_child(&q(&w, "pgSz"), &w, SECT_ORDER);
            let num = |k: &str, e: &El| e.attr(&q(&w, k)).and_then(|v| v.parse::<i64>().ok());
            let landscape = sz.attr(&q(&w, "orient")) == Some("landscape") || matches!((num("w", sz), num("h", sz)), (Some(a), Some(b)) if a > b);
            let (pw, ph) = if landscape { (A4_TWIPS.1, A4_TWIPS.0) } else { A4_TWIPS };
            sz.set_attr(&q(&w, "w"), &pw.to_string());
            sz.set_attr(&q(&w, "h"), &ph.to_string());
        }
        // 规范没写的边距保留原值
        let settings = page.margin_settings();
        if !settings.is_empty() {
            let mar = sect.ensure_child(&q(&w, "pgMar"), &w, SECT_ORDER);
            for (k, cm) in settings {
                mar.set_attr(&q(&w, k), &cm_to_twips(cm).to_string());
            }
            if mar.attr(&q(&w, "gutter")).is_none() {
                mar.set_attr(&q(&w, "gutter"), "0");
            }
        }
        self.rep.sections += 1;
    }
}

fn build(w: &str, key: &str, attrs: &BTreeMap<String, String>) -> El {
    let mut e = El::new(q(w, key));
    for (k, v) in attrs {
        e.set_attr(&q(w, k), v);
    }
    e
}

/// 去掉 run 属性里与样式冲突的直接格式；加粗、斜体、上下标、下划线等有含义的格式保留。返回删除个数。
fn clean_rpr(rpr: &mut El, w: &str, spec: &Spec) -> usize {
    let df = &spec.direct_formatting;
    let mut n = 0;
    if df.strip_fonts {
        let keep = rpr.child(&q(w, "rFonts")).is_some_and(|f| f.attrs.iter().any(|(k, v)| !k.ends_with("Theme") && !k.ends_with("cstheme") && !k.ends_with(":hint") && df.keep_fonts.iter().any(|kf| same_font(kf, &unescape(v)))));
        if !keep {
            n += rpr.remove_children(&q(w, "rFonts"));
        }
    }
    if df.strip_sizes {
        n += rpr.remove_children(&q(w, "sz"));
        n += rpr.remove_children(&q(w, "szCs"));
    }
    if df.strip_colors {
        n += rpr.remove_children(&q(w, "color"));
    }
    n
}

fn count(xml: &str, needle: &str) -> usize {
    xml.matches(needle).count()
}

pub fn apply(pkg: &mut Package, spec: &Spec) -> Result<Report> {
    apply_with_plan(pkg, spec, &Plan::default())
}

pub fn apply_with_plan(pkg: &mut Package, spec: &Spec, plan: &Plan) -> Result<Report> {
    let mut rep = Report { spec: spec.name.clone(), ..Report::default() };
    let styles_resolver = |pkg: &Package| -> Result<Resolver> {
        let doc = xml::parse(&pkg.get_str("word/styles.xml")?)?;
        let w = doc.prefix_of(NS_W).ok_or_else(|| Error::Xml("styles.xml 没有 WordprocessingML 命名空间".into()))?;
        Ok(Resolver::read(&doc, &w))
    };
    let old = styles_resolver(pkg)?;
    let book = format_styles(pkg, spec, plan, &mut rep)?;
    let new = styles_resolver(pkg)?;
    let table_style = spec.table_text.as_ref().and_then(|_| book.id_by_name(&TABLE_TEXT_STYLE.1.to_lowercase()));
    let figure_style = spec.figure.as_ref().and_then(|_| book.id_by_name(&FIGURE_STYLE.1.to_lowercase()));
    let bib_style = book.id_by_name("bibliography");

    for part in story_parts(pkg) {
        if part.ends_with("comments.xml") {
            continue; // 批注不排版
        }
        let src = pkg.get_str(&part)?;
        let mut doc = xml::parse(&src)?;
        let Some(w) = doc.prefix_of(NS_W) else { continue };
        let m = doc.prefix_of(NS_M).unwrap_or_else(|| "m".into());
        for (needle, label) in [
            (format!("<{w}:ins "), "修订（插入）"),
            (format!("<{w}:del "), "修订（删除）"),
            (format!("<{w}:txbxContent"), "文本框（已按正文排版）"),
            (format!("<{w}:object"), "嵌入对象"),
            ("drawingml/2006/chart".to_string(), "图表"),
            ("drawingml/2006/diagram".to_string(), "SmartArt"),
            (format!("<{w}:altChunk"), "外部内容块"),
        ] {
            let c = count(&src, &needle);
            if c > 0 {
                *rep.untouched.entry(label.to_string()).or_default() += c;
            }
        }
        if part == "word/document.xml" {
            let c = count(&src, &format!("<{w}:commentReference "));
            if c > 0 {
                rep.untouched.insert("批注".into(), c);
            }
            if src.contains(" TOC ") || src.contains("PAGEREF") {
                rep.notes.push("文档含目录或页码引用：显示的页码是缓存值，排版后可能变化。在 Word 里全选后按 F9 更新域，或在规范里打开 update_fields_on_open".into());
            }
        }
        let mut walker = Walker {
            w: w.clone(),
            m,
            spec,
            plan,
            book: &book,
            old: &old,
            new: &new,
            rep: &mut rep,
            table_style: table_style.clone(),
            figure_style: figure_style.clone(),
            bib_style: bib_style.clone(),
            track_sections: part == "word/document.xml",
            sect_no: 1,
        };
        walker.block(&mut doc.root, false);
        pkg.set(&part, doc.to_xml().into_bytes());
    }
    if !plan.protect_sections.is_empty() {
        rep.notes.push(format!("受保护的分节 {:?}：段落不清理、不映射，样式改动波及处已写回原外观（{} 处）", plan.protect_sections, rep.frozen_properties));
    }

    if spec.update_fields_on_open {
        let src = pkg.get_str("word/settings.xml")?;
        let mut doc = xml::parse(&src)?;
        if let Some(w) = doc.prefix_of(NS_W) {
            // updateFields 在 settings 里的位置很靠前之后的元素多，放到末尾之前也可被 Word 接受；这里放在 compat 之前
            let tag = q(&w, "updateFields");
            if doc.root.child(&tag).is_none() {
                let pos = doc.root.children.iter().position(|n| matches!(n, xml::Node::El(e) if e.local() == "compat" || e.local() == "rsids")).unwrap_or(doc.root.children.len());
                doc.root.children.insert(pos, xml::Node::El(El::new(tag).with_attr(&q(&w, "val"), "true")));
            }
            pkg.set("word/settings.xml", doc.to_xml().into_bytes());
        }
    }
    // 结构类要求（页眉、页码、三线表等）在格式排完之后做
    rep.structure = crate::structure::apply(pkg, spec, plan)?;
    rep.parts_changed = pkg.changed_parts().into_iter().map(String::from).collect();
    Ok(rep)
}
