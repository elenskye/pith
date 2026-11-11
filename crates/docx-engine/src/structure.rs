//! 结构类排版（v0.2 W4，`decisions/0019`）：要改文档结构才能满足的要求。在格式排完之后做，仍然不碰正文文字。
//!
//! - 分区：按计划里指出的段落，把文档分成 封面 / 前置部分（摘要、目录）/ 正文 / 正文之后。
//!   分界处没有分节符就加一个（写进分界前那个段落的属性里，不新增段落）。
//! - 页眉：前置部分与正文用规范写的内容（文字、章名 = STYLEREF 域）和横线；单面一种、双面分奇偶页；
//!   正文之后给空页眉；封面不动。新页眉写成新的部件，原来的页眉部件留着不改。
//! - 页码：页脚里一个 PAGE 域；前置部分、正文各自从 1 起，各用各的数字形式。
//! - 正文从奇数页开始：正文第一节的分节类型设为 oddPage。
//! - 三线表：有边框的数据表格只留顶线、栏目线、底线；没有边框的（用来排版的）表格、嵌套表格、受保护分节里的不动。
//! - 脚注编号：文档设置里的编号形式与重新编号方式。
//! - 标题紧跟上一级标题时段前不空：在那个标题段落上直接写段前 0。
//!
//! 做了什么都记在 `StructReport` 里；是否做对由 `verify::check_structure` 用另一套解析器独立核对。

use std::collections::BTreeMap;

use serde::Serialize;

use crate::outline::{self, Outline};
use crate::package::Package;
use crate::plan::{Anchor, Plan};
use crate::spec::{page_format_ooxml, HeaderLine, Spec, CHAPTER, RULE_HEAVY, RULE_LIGHT};
use crate::xml::{self, escape_attr, escape_text, El, Node};
use crate::{Error, Result, NS_R, NS_W};

pub const P_ORDER: &[&str] = &[
    "pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs",
    "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd",
    "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment",
    "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange",
];
pub const SECT_ORDER: &[&str] = &[
    "headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz", "pgMar", "paperSrc", "pgBorders", "lnNumType", "pgNumType",
    "cols", "formProt", "vAlign", "noEndnote", "titlePg", "textDirection", "bidi", "rtlGutter", "docGrid", "printerSettings", "sectPrChange",
];
pub const TBLPR_ORDER: &[&str] = &[
    "tblStyle", "tblpPr", "tblOverlap", "bidiVisual", "tblStyleRowBandSize", "tblStyleColBandSize", "tblW", "jc", "tblCellSpacing", "tblInd",
    "tblBorders", "shd", "tblLayout", "tblCellMar", "tblLook", "tblCaption", "tblDescription", "tblPrChange",
];
pub const TCPR_ORDER: &[&str] = &[
    "cnfStyle", "tcW", "gridSpan", "hMerge", "vMerge", "tcBorders", "shd", "noWrap", "tcMar", "textDirection", "tcFitText", "vAlign", "hideMark",
    "headers", "cellIns", "cellDel", "cellMerge", "tcPrChange",
];
pub const FTNPR_ORDER: &[&str] = &["pos", "numFmt", "numStart", "numRestart", "footnote"];
/// settings.xml 根元素下的顺序（CT_Settings）
pub const SETTINGS_ORDER: &[&str] = &[
    "writeProtection", "view", "zoom", "removePersonalInformation", "removeDateAndTime", "doNotDisplayPageBoundaries", "displayBackgroundShape",
    "printPostScriptOverText", "printFractionalCharacterWidth", "printFormsData", "embedTrueTypeFonts", "embedSystemFonts", "saveSubsetFonts",
    "saveFormsData", "mirrorMargins", "alignBordersAndEdges", "bordersDoNotSurroundHeader", "bordersDoNotSurroundFooter", "gutterAtTop",
    "hideSpellingErrors", "hideGrammaticalErrors", "activeWritingStyle", "proofState", "formsDesign", "attachedTemplate", "linkStyles",
    "stylePaneFormatFilter", "stylePaneSortMethod", "documentType", "mailMerge", "revisionView", "trackRevisions", "doNotTrackMoves",
    "doNotTrackFormatting", "documentProtection", "autoFormatOverride", "styleLockTheme", "styleLockQFSet", "defaultTabStop", "autoHyphenation",
    "consecutiveHyphenLimit", "hyphenationZone", "doNotHyphenateCaps", "showEnvelope", "summaryLength", "clickAndTypeStyle", "defaultTableStyle",
    "evenAndOddHeaders", "bookFoldRevPrinting", "bookFoldPrinting", "bookFoldPrintingSheets", "drawingGridHorizontalSpacing",
    "drawingGridVerticalSpacing", "displayHorizontalDrawingGridEvery", "displayVerticalDrawingGridEvery", "doNotUseMarginsForDrawingGridOrigin",
    "drawingGridHorizontalOrigin", "drawingGridVerticalOrigin", "doNotShadeFormData", "noPunctuationKerning", "characterSpacingControl",
    "printTwoOnOne", "strictFirstAndLastChars", "noLineBreaksAfter", "noLineBreaksBefore", "savePreviewPicture", "doNotValidateAgainstSchema",
    "saveInvalidXml", "ignoreMixedContent", "alwaysShowPlaceholderText", "doNotDemarcateInvalidXml", "saveXmlDataOnly", "useXSLTWhenSaving",
    "saveThroughXslt", "showXMLTags", "alwaysMergeEmptyNamespace", "updateFields", "hdrShapeDefaults", "footnotePr", "endnotePr", "compat",
    "docVars", "rsids", "mathPr", "attachedSchema", "themeFontLang", "clrSchemeMapping", "doNotIncludeSubdocsInStats", "doNotAutoCompressPictures",
    "forceUpgrade", "captions", "readModeInkLockDown", "smartTagType", "schemaLibrary", "shapeDefaults", "doNotEmbedSmartTags", "decimalSymbol",
    "listSeparator",
];

/// 新页眉页脚部件的名字：`word/header_pith<N>.xml`、`word/footer_pith<N>.xml`（校验器只放行这样的新部件）
pub const HEADER_PART: &str = "header_pith";
pub const FOOTER_PART: &str = "footer_pith";
const TYPE_HEADER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const TYPE_FOOTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";

/// 文档的分区
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    /// 前置部分之前：封面、题名页
    Cover,
    /// 前置部分：摘要、目录
    Front,
    /// 正文（到致谢）
    Main,
    /// 正文之后：声明等
    Back,
}

impl Zone {
    pub fn label(self) -> &'static str {
        match self {
            Zone::Cover => "封面",
            Zone::Front => "前置部分",
            Zone::Main => "正文",
            Zone::Back => "正文之后",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructReport {
    /// 新加的分节符（「在『摘要』之前」）
    pub breaks_inserted: Vec<String>,
    /// 各分区的分节数（封面、前置部分、正文、正文之后）
    pub zones: Vec<(String, usize)>,
    pub duplex: bool,
    /// 设了页眉 / 页码的分节数
    pub header_sections: usize,
    pub page_number_sections: usize,
    pub body_on_odd_page: bool,
    /// 排成三线表的表格（第几个）
    pub three_line: Vec<usize>,
    /// 没动的表格和原因
    pub tables_skipped: Vec<(usize, String)>,
    pub footnote_numbering: bool,
    /// 紧跟上一级标题、段前改成 0 的标题数
    pub tight_headings: usize,
    pub notes: Vec<String>,
}

fn q(w: &str, local: &str) -> String {
    format!("{w}:{local}")
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 计划里的一个分界点对得上文档吗：那一段得是正文里独立的一段，并且以给的文字开头
pub fn check_anchor(o: &Outline, a: &Anchor, what: &str) -> Result<()> {
    let p = o.paragraphs.get(a.paragraph.wrapping_sub(1)).ok_or_else(|| Error::Spec(format!("{what}：文档里没有第 {} 段", a.paragraph)))?;
    if p.in_table || p.in_textbox {
        return Err(Error::Spec(format!("{what}：第 {} 段在表格或文本框里，不能作为分界", a.paragraph)));
    }
    let (want, got) = (squash(&a.text), squash(&p.text));
    if want.is_empty() || !got.starts_with(&want) {
        return Err(Error::Spec(format!("{what}：第 {} 段现在是「{}」，不是计划里写的「{}」。文档可能改过，请重新生成计划", a.paragraph, outline::snippet(&p.text, 20), outline::snippet(&a.text, 20))));
    }
    Ok(())
}

/// 这份文档里哪些表格要排成三线表（按排版前的结构判断；校验器用同一个判断推出预期）
pub fn three_line_targets(o: &Outline, plan: &Plan) -> (Vec<usize>, Vec<(usize, String)>) {
    let (mut yes, mut no) = (Vec::new(), Vec::new());
    for t in &o.table_list {
        let why = if plan.layout.skip_tables.contains(&t.index) {
            Some("计划里说不排")
        } else if plan.is_protected(t.section) {
            Some("在受保护的分节里")
        } else if t.nested {
            Some("嵌在别的表格里")
        } else if !t.bordered {
            Some("没有边框，多半是用来排版的表格")
        } else if t.rows < 2 {
            Some("只有一行")
        } else if t.irregular {
            Some("行或单元格外面包着别的结构，没有把握")
        } else {
            None
        };
        match why {
            Some(w) => no.push((t.index, w.to_string())),
            None => yes.push(t.index),
        }
    }
    (yes, no)
}

/// 紧跟上一级标题的标题段落（段落编号）：`levels` 里的级别 L，前一段正好是 L-1 级标题
pub fn tight_heading_paragraphs(o: &Outline, plan: &Plan, levels: &[u8]) -> Vec<usize> {
    let level = |style: &str| style.to_lowercase().strip_prefix("heading ").and_then(|n| n.parse::<u8>().ok());
    let mut out = Vec::new();
    for pair in o.paragraphs.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.in_table || b.in_table || a.in_textbox || b.in_textbox || plan.is_protected(b.section) {
            continue;
        }
        if let (Some(la), Some(lb)) = (level(&a.style), level(&b.style))
            && levels.contains(&lb)
            && la + 1 == lb
        {
            out.push(b.index);
        }
    }
    out
}

// ---------- 页眉页脚部件 ----------

struct HeaderStyle {
    id: Option<String>,
    /// 样式自带的制表位（要清掉，不然和我们按版心宽度算的对不上）
    tabs: Vec<i64>,
}

fn named_style(pkg: &Package, name: &str) -> HeaderStyle {
    let mut out = HeaderStyle { id: None, tabs: Vec::new() };
    let Ok(sx) = pkg.get_str("word/styles.xml") else { return out };
    let Ok(doc) = roxmltree::Document::parse_with_options(&sx, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) else { return out };
    let is = |n: &roxmltree::Node, l: &str| n.is_element() && n.tag_name().name() == l && n.tag_name().namespace() == Some(NS_W);
    for s in doc.descendants().filter(|n| is(n, "style") && n.attribute((NS_W, "type")) == Some("paragraph")) {
        let n = s.children().find(|c| is(c, "name")).and_then(|c| c.attribute((NS_W, "val"))).unwrap_or("");
        if n.eq_ignore_ascii_case(name) {
            out.id = s.attribute((NS_W, "styleId")).map(String::from);
            out.tabs = s.descendants().filter(|t| is(t, "tab")).filter_map(|t| t.attribute((NS_W, "pos")).and_then(|v| v.parse().ok())).collect();
            break;
        }
    }
    out
}

fn text_run(text: &str) -> String {
    if text.is_empty() { String::new() } else { format!(r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#, escape_text(text)) }
}

fn field(code: &str, shown: &str) -> String {
    format!(
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>{}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        escape_text(shown)
    )
}

/// 章名用 `STYLEREF 1`：指内置的一级标题，不随 Word 的界面语言变
pub const CHAPTER_FIELD: &str = "STYLEREF 1";

/// 页眉里的一段内容：文字照写，`{章名}` 换成域
fn content_runs(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(CHAPTER) {
        out.push_str(&text_run(&rest[..i]));
        out.push_str(&field(CHAPTER_FIELD, ""));
        rest = &rest[i + CHAPTER.len()..];
    }
    out.push_str(&text_run(rest));
    out
}

const NS_DECL: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

fn rule_xml(rule: Option<bool>) -> &'static str {
    match rule {
        Some(true) => r#"<w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="auto"/></w:pBdr>"#,
        Some(false) => r#"<w:pBdr><w:bottom w:val="nil"/></w:pBdr>"#,
        None => "",
    }
}

/// 一行页眉。只有居中 / 只有一侧时用段落对齐；左右（或再加居中）并存时用制表位，位置按版心宽度算。
fn header_xml(line: &HeaderLine, rule: Option<bool>, width: i64, style: &HeaderStyle) -> String {
    let pstyle = style.id.as_ref().map(|id| format!(r#"<w:pStyle w:val="{}"/>"#, escape_attr(id))).unwrap_or_default();
    let ind = r#"<w:ind w:firstLine="0" w:firstLineChars="0"/>"#;
    let (l, c, r) = (line.left.as_deref().unwrap_or(""), line.center.as_deref().unwrap_or(""), line.right.as_deref().unwrap_or(""));
    let count = [l, c, r].iter().filter(|s| !s.is_empty()).count();
    let (tabs, jc, runs) = if count <= 1 {
        let (jc, text) = if !c.is_empty() { ("center", c) } else if !r.is_empty() { ("right", r) } else { ("left", l) };
        (String::new(), jc, content_runs(text))
    } else {
        let (center, right) = (width / 2, width);
        let mut t = String::from("<w:tabs>");
        for pos in style.tabs.iter().filter(|p| **p != center && **p != right) {
            t.push_str(&format!(r#"<w:tab w:val="clear" w:pos="{pos}"/>"#));
        }
        if !c.is_empty() {
            t.push_str(&format!(r#"<w:tab w:val="center" w:pos="{center}"/>"#));
        }
        t.push_str(&format!(r#"<w:tab w:val="right" w:pos="{right}"/></w:tabs>"#));
        let tab = "<w:r><w:tab/></w:r>";
        let mut runs = content_runs(l);
        if !c.is_empty() {
            runs.push_str(tab);
            runs.push_str(&content_runs(c));
        }
        if !r.is_empty() {
            runs.push_str(tab);
            runs.push_str(&content_runs(r));
        }
        (t, "left", runs)
    };
    format!(r#"{DECL}<w:hdr {NS_DECL}><w:p><w:pPr>{pstyle}{}{tabs}{ind}<w:jc w:val="{jc}"/></w:pPr>{runs}</w:p></w:hdr>"#, rule_xml(rule))
}

/// 空页眉（正文之后）：一个空段落，明确不要横线
fn empty_header_xml(style: &HeaderStyle) -> String {
    let pstyle = style.id.as_ref().map(|id| format!(r#"<w:pStyle w:val="{}"/>"#, escape_attr(id))).unwrap_or_default();
    format!(r#"{DECL}<w:hdr {NS_DECL}><w:p><w:pPr>{pstyle}{}</w:pPr></w:p></w:hdr>"#, rule_xml(Some(false)))
}

fn footer_xml(align: &str, style: &HeaderStyle) -> String {
    let pstyle = style.id.as_ref().map(|id| format!(r#"<w:pStyle w:val="{}"/>"#, escape_attr(id))).unwrap_or_default();
    format!(r#"{DECL}<w:ftr {NS_DECL}><w:p><w:pPr>{pstyle}<w:ind w:firstLine="0" w:firstLineChars="0"/><w:jc w:val="{align}"/></w:pPr>{}</w:p></w:ftr>"#, field("PAGE", "1"))
}

/// 新部件：攒起来最后一起写进包、关系和内容类型
struct Parts {
    taken: Vec<String>,
    rels: String,
    new: Vec<(String, String, bool)>,
    cache: BTreeMap<String, String>,
    next: usize,
}

impl Parts {
    fn new(pkg: &Package) -> Result<Self> {
        Ok(Parts { taken: pkg.names().into_iter().map(String::from).collect(), rels: pkg.get_str("word/_rels/document.xml.rels")?, new: Vec::new(), cache: BTreeMap::new(), next: 1 })
    }

    /// 同样内容的部件只建一个；返回关系 id
    fn get(&mut self, key: &str, header: bool, make: impl FnOnce() -> String) -> String {
        if let Some(id) = self.cache.get(key) {
            return id.clone();
        }
        let base = if header { HEADER_PART } else { FOOTER_PART };
        let (file, id) = loop {
            let n = self.next;
            self.next += 1;
            let (file, id) = (format!("{base}{n}.xml"), format!("rIdPith{n}"));
            if !self.taken.contains(&format!("word/{file}")) && !self.rels.contains(&format!(r#"Id="{id}""#)) {
                break (file, id);
            }
        };
        self.taken.push(format!("word/{file}"));
        let rel = format!(r#"<Relationship Id="{id}" Type="{}" Target="{file}"/>"#, if header { TYPE_HEADER } else { TYPE_FOOTER });
        self.rels = self.rels.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1);
        self.new.push((file, make(), header));
        self.cache.insert(key.to_string(), id.clone());
        id
    }

    fn write(self, pkg: &mut Package) -> Result<()> {
        if self.new.is_empty() {
            return Ok(());
        }
        let mut ct = pkg.get_str("[Content_Types].xml")?;
        for (file, xml, header) in &self.new {
            let over = format!(r#"<Override PartName="/word/{file}" ContentType="{}"/>"#, if *header { CT_HEADER } else { CT_FOOTER });
            ct = ct.replacen("</Types>", &format!("{over}</Types>"), 1);
            pkg.set(&format!("word/{file}"), xml.clone().into_bytes());
        }
        pkg.set("[Content_Types].xml", ct.into_bytes());
        pkg.set("word/_rels/document.xml.rels", self.rels.into_bytes());
        Ok(())
    }
}

// ---------- 分区、分节 ----------

fn count_paragraphs(el: &El, p: &str) -> usize {
    (el.name == p) as usize + el.els().map(|c| count_paragraphs(c, p)).sum::<usize>()
}

/// 正文顶层的元素（不含最后的分节属性）：(在 body.children 里的位置, 是不是段落, 它里面第一个段落的编号)
fn top_items(body: &El, w: &str) -> Vec<(usize, bool, usize)> {
    let (p, sect) = (q(w, "p"), q(w, "sectPr"));
    let mut ord = 0;
    let mut out = Vec::new();
    for (pos, n) in body.children.iter().enumerate() {
        let Node::El(e) = n else { continue };
        if e.name == sect {
            continue;
        }
        out.push((pos, e.name == p, ord + 1));
        ord += count_paragraphs(e, &p);
    }
    out
}

fn para_sect<'a>(body: &'a El, pos: usize, w: &str) -> Option<&'a El> {
    match &body.children[pos] {
        Node::El(p) => p.child(&q(w, "pPr")).and_then(|pp| pp.child(&q(w, "sectPr"))),
        _ => None,
    }
}

fn para_sect_mut<'a>(body: &'a mut El, pos: usize, w: &str) -> Option<&'a mut El> {
    match &mut body.children[pos] {
        Node::El(p) => p.child_mut(&q(w, "pPr")).and_then(|pp| pp.child_mut(&q(w, "sectPr"))),
        _ => None,
    }
}

fn num(e: Option<&El>, w: &str, k: &str) -> Option<i64> {
    e.and_then(|e| e.attr(&q(w, k))).and_then(|v| v.parse().ok())
}

/// 版心宽度（twips）
fn text_width(sect: &El, w: &str) -> i64 {
    let (sz, mar) = (sect.child(&q(w, "pgSz")), sect.child(&q(w, "pgMar")));
    let width = num(sz, w, "w").unwrap_or(11906) - num(mar, w, "left").unwrap_or(1800) - num(mar, w, "right").unwrap_or(1800) - num(mar, w, "gutter").unwrap_or(0);
    width.max(2000)
}

struct Managed {
    header: bool,
    numbers: bool,
}

/// 把分界处切成分节，按分区设置每一节的页眉、页脚、页码、分节类型
#[allow(clippy::too_many_arguments)]
fn zones(doc: &mut xml::XmlDoc, w: &str, r: &str, pkg: &Package, spec: &Spec, plan: &Plan, o: &Outline, parts: &mut Parts, rep: &mut StructReport) -> Result<bool> {
    let st = &spec.structure;
    let layout = &plan.layout;
    let Some(body_anchor) = &layout.body_start else {
        rep.notes.push("计划里没有指出正文从哪一段开始，页眉、页码、奇数页起这几项没有处理".into());
        return Ok(false);
    };
    check_anchor(o, body_anchor, "正文的起点")?;
    for (a, what) in [(&layout.front_start, "前置部分的起点"), (&layout.back_start, "正文之后的起点")] {
        if let Some(a) = a {
            check_anchor(o, a, what)?;
        }
    }
    let managed = Managed { header: st.header.is_some(), numbers: st.page_numbers.is_some() };
    let body = doc.root.child_mut(&q(w, "body")).ok_or_else(|| Error::Missing("word/document.xml 的 body".into()))?;
    let (sect_name, ppr_name) = (q(w, "sectPr"), q(w, "pPr"));

    // 1 分界处补分节符
    let items = top_items(body, w);
    let item_of = |a: &Anchor, what: &str| -> Result<usize> {
        items.iter().position(|(_, is_p, ord)| *is_p && *ord == a.paragraph).ok_or_else(|| Error::Spec(format!("{what}：第 {} 段不是正文里独立的一段", a.paragraph)))
    };
    // 分界点前面如果只隔着空段落就是已有的分节符（或文档开头），那些空段落算进后面的分区，不再多切一节
    let blank = |i: usize| -> bool {
        let (pos, is_p, ord) = items[i];
        is_p && o.paragraphs.get(ord - 1).is_some_and(|p| p.is_empty()) && items.get(i + 1).is_none_or(|n| n.2 == ord + 1) && para_sect(body, pos, w).is_none()
    };
    let settle = |i: usize| -> (usize, bool) {
        let mut j = i;
        while j > 0 && blank(j - 1) {
            j -= 1;
        }
        if j == 0 || (items[j - 1].1 && para_sect(body, items[j - 1].0, w).is_some()) { (j, false) } else { (i, true) }
    };
    let f = layout.front_start.as_ref().map(|a| item_of(a, "前置部分的起点")).transpose()?.map(settle);
    let b = settle(item_of(body_anchor, "正文的起点")?);
    let k = layout.back_start.as_ref().map(|a| item_of(a, "正文之后的起点")).transpose()?.map(settle);
    let cuts = [(f, layout.front_start.as_ref()), (Some(b), Some(body_anchor)), (k, layout.back_start.as_ref())];
    let (f, b, k) = (f.map(|x| x.0), b.0, k.map(|x| x.0));
    let zone_of = |i: usize| {
        if k.is_some_and(|k| i >= k) {
            Zone::Back
        } else if i >= b {
            Zone::Main
        } else if f.is_some_and(|f| i >= f) {
            Zone::Front
        } else {
            Zone::Cover
        }
    };
    for (cut, anchor) in cuts {
        let (Some((at, insert)), Some(anchor)) = (cut, anchor) else { continue };
        if !insert {
            continue;
        }
        let (prev_pos, prev_is_p, _) = items[at - 1];
        if !prev_is_p {
            return Err(Error::Spec(format!("「{}」前面紧挨着的不是普通段落（是表格等），没法在这里分节。请在 Word 里在它前面加一个空段落再试", outline::snippet(&anchor.text, 16))));
        }
        // 管着这一段的分节属性：后面第一个带分节符的段落，没有就是正文末尾的
        let governing = items[at..].iter().find(|(pos, is_p, _)| *is_p && para_sect(body, *pos, w).is_some()).map(|(pos, _, _)| *pos);
        let template = match governing {
            Some(pos) => para_sect(body, pos, w).cloned(),
            None => body.child(&sect_name).cloned(),
        };
        let mut first_half = template.unwrap_or_else(|| El::new(sect_name.clone()));
        // 切出来的前一半是另一个分区：我们要管的那几样不带过去（封面不该有正文的页眉页码）
        if managed.header {
            first_half.remove_children(&q(w, "headerReference"));
        }
        if managed.numbers {
            first_half.remove_children(&q(w, "footerReference"));
            first_half.remove_children(&q(w, "pgNumType"));
        }
        first_half.remove_children(&q(w, "footnotePr"));
        first_half.remove_children(&q(w, "endnotePr"));
        // 原来的分节怎么开始（如奇数页）跟着前一半走；后一半默认另起一页
        let second = match governing {
            Some(pos) => para_sect_mut(body, pos, w),
            None => body.child_mut(&sect_name),
        };
        if let Some(s) = second {
            s.remove_children(&q(w, "type"));
        }
        if let Node::El(p) = &mut body.children[prev_pos] {
            p.ensure_child(&ppr_name, w, &["pPr"]).put_child(first_half, w, P_ORDER);
        }
        rep.breaks_inserted.push(format!("在「{}」之前", outline::snippet(&anchor.text, 16)));
    }

    // 2 列出分节：每个带分节符的段落结束一节，正文末尾的结束最后一节
    let ends: Vec<Option<usize>> = items.iter().filter(|(pos, is_p, _)| *is_p && para_sect(body, *pos, w).is_some()).map(|(pos, _, _)| Some(*pos)).chain(std::iter::once(None)).collect();
    let pos_to_item = |pos: usize| items.iter().position(|(p, _, _)| *p == pos).unwrap_or(0);
    if body.child(&sect_name).is_none() {
        body.children.push(Node::El(El::new(sect_name.clone())));
    }
    let header_style = named_style(pkg, "header");
    let footer_style = named_style(pkg, "footer");
    let duplex = layout.duplex;
    let mut start = 0usize;
    let mut seen: BTreeMap<Zone, usize> = BTreeMap::new();
    for end in ends {
        let zone = zone_of(start);
        let first_in_zone = !seen.contains_key(&zone);
        *seen.entry(zone).or_default() += 1;
        if let Some(pos) = end {
            start = pos_to_item(pos) + 1;
        }
        let sect = match end {
            Some(pos) => para_sect_mut(body, pos, w),
            None => body.child_mut(&sect_name),
        };
        let Some(sect) = sect else { continue };
        if zone == Zone::Cover {
            continue;
        }
        let width = text_width(sect, w);
        let mut refs: Vec<El> = Vec::new();
        let make_ref = |kind: &str, ty: &str, id: &str| El::new(q(w, kind)).with_attr(&q(w, "type"), ty).with_attr(&q(r, "id"), id);
        if let Some(h) = &st.header {
            sect.remove_children(&q(w, "headerReference"));
            let (odd, even) = h.lines(duplex);
            if zone == Zone::Back {
                let id = parts.get("header:empty", true, || empty_header_xml(&header_style));
                refs.push(make_ref("headerReference", "default", &id));
                if duplex {
                    refs.push(make_ref("headerReference", "even", &id));
                }
            } else {
                let id = parts.get(&format!("header:odd:{width}"), true, || header_xml(&odd, h.rule, width, &header_style));
                refs.push(make_ref("headerReference", "default", &id));
                if let Some(even) = even {
                    let id = parts.get(&format!("header:even:{width}"), true, || header_xml(&even, h.rule, width, &header_style));
                    refs.push(make_ref("headerReference", "even", &id));
                }
            }
            rep.header_sections += 1;
        }
        if let Some(pn) = &st.page_numbers {
            sect.remove_children(&q(w, "footerReference"));
            let align = pn.align.as_deref().unwrap_or("center");
            let id = parts.get(&format!("footer:{align}"), false, || footer_xml(align, &footer_style));
            refs.push(make_ref("footerReference", "default", &id));
            if duplex {
                refs.push(make_ref("footerReference", "even", &id));
            }
            let fmt = match zone {
                Zone::Front => pn.front_format.as_deref(),
                _ => pn.body_format.as_deref(),
            }
            .and_then(page_format_ooxml);
            let mut pg = El::new(q(w, "pgNumType"));
            if let Some(fmt) = fmt {
                pg.set_attr(&q(w, "fmt"), fmt);
            }
            if first_in_zone && zone != Zone::Back {
                pg.set_attr(&q(w, "start"), "1");
            }
            if pg.attrs.is_empty() {
                sect.remove_children(&q(w, "pgNumType"));
            } else {
                sect.put_child(pg, w, SECT_ORDER);
            }
            rep.page_number_sections += 1;
        }
        if managed.header || managed.numbers {
            // 「首页不同」会让每节第一页没有页眉页码
            sect.remove_children(&q(w, "titlePg"));
        }
        // 引用放在分节属性最前面：页眉在前、页脚在后（没管页眉时，页脚接在原来的页眉引用后面）
        let at = if managed.header { 0 } else { sect.children.iter().rposition(|n| matches!(n, Node::El(e) if e.name == q(w, "headerReference"))).map(|i| i + 1).unwrap_or(0) };
        for (k, el) in refs.into_iter().enumerate() {
            sect.children.insert(at + k, Node::El(el));
        }
        if st.body_on_odd_page == Some(true) && zone == Zone::Main && first_in_zone {
            sect.put_child(El::new(q(w, "type")).with_attr(&q(w, "val"), "oddPage"), w, SECT_ORDER);
            rep.body_on_odd_page = true;
        }
    }
    rep.zones = [Zone::Cover, Zone::Front, Zone::Main, Zone::Back].iter().filter_map(|z| seen.get(z).map(|n| (z.label().to_string(), *n))).collect();
    rep.duplex = duplex;
    Ok(managed.header || managed.numbers)
}

// ---------- 三线表 ----------

fn border(w: &str, side: &str, size: Option<u32>) -> El {
    match size {
        Some(sz) => El::new(q(w, side)).with_attr(&q(w, "val"), "single").with_attr(&q(w, "sz"), &sz.to_string()).with_attr(&q(w, "space"), "0").with_attr(&q(w, "color"), "auto"),
        None => El::new(q(w, side)).with_attr(&q(w, "val"), "nil"),
    }
}

fn grid_span(tc: &El, w: &str) -> usize {
    tc.child(&q(w, "tcPr")).and_then(|p| p.child(&q(w, "gridSpan"))).and_then(|g| g.attr(&q(w, "val"))).and_then(|v| v.parse().ok()).unwrap_or(1).max(1)
}

/// 纵向合并：Some(true) 起头，Some(false) 接着上面，None 没合并
fn v_merge(tc: &El, w: &str) -> Option<bool> {
    let m = tc.child(&q(w, "tcPr"))?.child(&q(w, "vMerge"))?;
    Some(m.attr(&q(w, "val")) == Some("restart"))
}

/// 表头占几行：第一行里纵向合并的单元格往下延伸到哪，表头就到哪
fn header_rows(rows: &[&El], w: &str) -> usize {
    let tc = q(w, "tc");
    let Some(first) = rows.first() else { return 1 };
    let mut col = 0;
    let mut depth = 1;
    for cell in first.els().filter(|e| e.name == tc) {
        if v_merge(cell, w) == Some(true) {
            let mut d = 1;
            for row in &rows[1..] {
                let mut c = 0;
                let cont = row.els().filter(|e| e.name == tc).find_map(|x| {
                    let hit = c == col;
                    c += grid_span(x, w);
                    hit.then(|| v_merge(x, w) == Some(false))
                });
                if cont == Some(true) { d += 1 } else { break }
            }
            depth = depth.max(d);
        }
        col += grid_span(cell, w);
    }
    depth.min(rows.len())
}

fn three_line(tbl: &mut El, w: &str) {
    let (tr, tc) = (q(w, "tr"), q(w, "tc"));
    let (n_rows, head) = {
        let rows: Vec<&El> = tbl.els().filter(|e| e.name == tr).collect();
        (rows.len(), header_rows(&rows, w))
    };
    let mut borders = El::new(q(w, "tblBorders"));
    for (side, sz) in [("top", Some(RULE_HEAVY)), ("left", None), ("bottom", Some(RULE_HEAVY)), ("right", None), ("insideH", None), ("insideV", None)] {
        borders.children.push(Node::El(match sz {
            Some(_) => border(w, side, sz),
            None => El::new(q(w, side)).with_attr(&q(w, "val"), "none").with_attr(&q(w, "sz"), "0").with_attr(&q(w, "space"), "0").with_attr(&q(w, "color"), "auto"),
        }));
    }
    tbl.ensure_child(&q(w, "tblPr"), w, &["tblPr"]).put_child(borders, w, TBLPR_ORDER);
    for (i, row) in tbl.els_mut().filter(|e| e.name == tr).enumerate() {
        // 行自己的边框例外会盖过表格的
        if let Some(ex) = row.child_mut(&q(w, "tblPrEx")) {
            ex.remove_children(&q(w, "tblBorders"));
        }
        let top = (i == 0).then_some(RULE_HEAVY);
        let bottom = if i + 1 == n_rows { Some(RULE_HEAVY) } else if i + 1 == head { Some(RULE_LIGHT) } else { None };
        for cell in row.els_mut().filter(|e| e.name == tc) {
            // 每个单元格的四边都写明：单元格的设置优先于表格样式里的条件格式
            let mut b = El::new(q(w, "tcBorders"));
            b.children = vec![Node::El(border(w, "top", top)), Node::El(border(w, "left", None)), Node::El(border(w, "bottom", bottom)), Node::El(border(w, "right", None))];
            cell.ensure_child(&q(w, "tcPr"), w, &["tcPr"]).put_child(b, w, TCPR_ORDER);
        }
    }
}

/// 按文档顺序走一遍段落和表格（编号与 `outline` 的一致）
fn walk(el: &mut El, p: &str, tbl: &str, n_p: &mut usize, n_tbl: &mut usize, f: &mut dyn FnMut(&mut El, bool, usize)) {
    for c in el.els_mut() {
        if c.name == p {
            *n_p += 1;
            f(c, true, *n_p);
        } else if c.name == tbl {
            *n_tbl += 1;
            f(c, false, *n_tbl);
        }
        walk(c, p, tbl, n_p, n_tbl, f);
    }
}

// ---------- 文档设置 ----------

fn edit_settings(pkg: &mut Package, spec: &Spec, duplex: Option<bool>, rep: &mut StructReport) -> Result<()> {
    let st = &spec.structure;
    let fmt = st.footnote_numbering.as_ref().and_then(|f| f.format_ooxml());
    let restart = st.footnote_numbering.as_ref().and_then(|f| f.restart_ooxml());
    if duplex.is_none() && fmt.is_none() && restart.is_none() {
        return Ok(());
    }
    let src = pkg.get_str("word/settings.xml")?;
    let mut doc = xml::parse(&src)?;
    let Some(w) = doc.prefix_of(NS_W) else { return Ok(()) };
    if let Some(d) = duplex {
        let name = q(&w, "evenAndOddHeaders");
        if d {
            doc.root.put_child(El::new(name), &w, SETTINGS_ORDER);
        } else {
            doc.root.remove_children(&name);
        }
    }
    if fmt.is_some() || restart.is_some() {
        let pr = doc.root.ensure_child(&q(&w, "footnotePr"), &w, SETTINGS_ORDER);
        if let Some(f) = fmt {
            pr.put_child(El::new(q(&w, "numFmt")).with_attr(&q(&w, "val"), f), &w, FTNPR_ORDER);
        }
        if let Some(r) = restart {
            // 每页 / 每节重新编号时不能再指定起始号
            pr.remove_children(&q(&w, "numStart"));
            pr.put_child(El::new(q(&w, "numRestart")).with_attr(&q(&w, "val"), r), &w, FTNPR_ORDER);
        }
        rep.footnote_numbering = true;
    }
    pkg.set("word/settings.xml", doc.to_xml().into_bytes());
    Ok(())
}

/// 分节里单独写的脚注编号会盖过文档设置：去掉
fn clear_section_footnote_numbering(el: &mut El, w: &str) {
    let (sect, pr) = (q(w, "sectPr"), q(w, "footnotePr"));
    for c in el.els_mut() {
        if c.name == sect {
            let mut empty = false;
            if let Some(f) = c.child_mut(&pr) {
                for k in ["numFmt", "numStart", "numRestart"] {
                    f.remove_children(&q(w, k));
                }
                empty = f.els().next().is_none();
            }
            if empty {
                c.remove_children(&pr);
            }
        } else {
            clear_section_footnote_numbering(c, w);
        }
    }
}

pub fn apply(pkg: &mut Package, spec: &Spec, plan: &Plan) -> Result<Option<StructReport>> {
    let st = &spec.structure;
    if st.is_empty() {
        return Ok(None);
    }
    // 排完格式之后的状态；分节还是原来的数法（计划里受保护的分节按它算）
    let o = outline::read(pkg)?;
    let mut rep = StructReport::default();
    let src = pkg.get_str("word/document.xml")?;
    let mut doc = xml::parse(&src)?;
    let w = doc.prefix_of(NS_W).ok_or_else(|| Error::Xml("document.xml 没有 WordprocessingML 命名空间".into()))?;

    let mut duplex = None;
    if st.needs_zones() {
        let r = doc.prefix_of(NS_R).ok_or_else(|| Error::Xml("document.xml 没有声明关系命名空间，无法引用页眉页脚".into()))?;
        let mut parts = Parts::new(pkg)?;
        if zones(&mut doc, &w, &r, pkg, spec, plan, &o, &mut parts, &mut rep)? {
            duplex = Some(plan.layout.duplex);
        }
        parts.write(pkg)?;
    }

    let tables: Vec<usize> = if st.three_line_tables == Some(true) {
        let (yes, no) = three_line_targets(&o, plan);
        rep.tables_skipped = no;
        yes
    } else {
        Vec::new()
    };
    let tight = tight_heading_paragraphs(&o, plan, &st.tight_headings);
    if !tables.is_empty() || !tight.is_empty() {
        let (p, tbl) = (q(&w, "p"), q(&w, "tbl"));
        let (mut n_p, mut n_tbl) = (0, 0);
        let body = doc.root.child_mut(&q(&w, "body")).ok_or_else(|| Error::Missing("word/document.xml 的 body".into()))?;
        let mut done_tables = Vec::new();
        let mut done_tight = 0;
        walk(body, &p, &tbl, &mut n_p, &mut n_tbl, &mut |el, is_p, n| {
            if is_p && tight.contains(&n) {
                let sp = el.ensure_child(&q(&w, "pPr"), &w, &["pPr"]).ensure_child(&q(&w, "spacing"), &w, P_ORDER);
                sp.set_attr(&q(&w, "before"), "0");
                sp.set_attr(&q(&w, "beforeLines"), "0");
                sp.remove_attr(&q(&w, "beforeAutospacing"));
                done_tight += 1;
            } else if !is_p && tables.contains(&n) {
                three_line(el, &w);
                done_tables.push(n);
            }
        });
        rep.three_line = done_tables;
        rep.tight_headings = done_tight;
    }
    if st.footnote_numbering.is_some() {
        clear_section_footnote_numbering(&mut doc.root, &w);
    }
    pkg.set("word/document.xml", doc.to_xml().into_bytes());
    edit_settings(pkg, spec, duplex, &mut rep)?;
    Ok(Some(rep))
}
