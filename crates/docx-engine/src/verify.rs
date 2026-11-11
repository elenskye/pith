//! 独立校验器：用 roxmltree（与排版器不同的解析器）抽取文档内容与结构，比较排版前后。
//! 「只改格式」意味着：各故事部件的文字、删除文字、域代码、公式文字逐字相同；结构计数与 ID 相同；
//! 不该动的部件（图片、数据部件、关系、内容类型等）逐字节相同。

use std::collections::BTreeMap;

use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::package::{relationships, story_parts, Package};
use crate::plan::Plan;
use crate::spec::{cm_to_twips, same_font, Spec, TextStyle, A4_TWIPS};
use crate::{Error, Result, NS_A, NS_M, NS_V, NS_W};

const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// 统计的结构：(命名空间, 元素名, 报告用中文名)
const COUNTED: &[(&str, &str, &str)] = &[
    (NS_W, "p", "段落"),
    (NS_W, "tbl", "表格"),
    (NS_W, "tr", "表格行"),
    (NS_W, "tc", "单元格"),
    (NS_W, "drawing", "DrawingML 对象"),
    (NS_W, "pict", "VML 对象"),
    (NS_A, "blip", "图片引用"),
    (NS_V, "imagedata", "VML 图片"),
    (NS_M, "oMath", "公式"),
    (NS_M, "oMathPara", "独立公式段"),
    (NS_W, "fldSimple", "简单域"),
    (NS_W, "footnoteReference", "脚注引用"),
    (NS_W, "endnoteReference", "尾注引用"),
    (NS_W, "footnote", "脚注"),
    (NS_W, "endnote", "尾注"),
    (NS_W, "comment", "批注"),
    (NS_W, "commentReference", "批注引用"),
    (NS_W, "commentRangeStart", "批注范围"),
    (NS_W, "bookmarkStart", "书签"),
    (NS_W, "hyperlink", "超链接"),
    (NS_W, "sdt", "内容控件"),
    (NS_W, "ins", "修订：插入"),
    (NS_W, "del", "修订：删除"),
    (NS_W, "moveFrom", "修订：移出"),
    (NS_W, "moveTo", "修订：移入"),
    (NS_W, "sectPr", "分节"),
    (NS_W, "txbxContent", "文本框"),
    (NS_W, "numPr", "列表编号"),
    (NS_W, "object", "嵌入对象"),
    (NS_W, "altChunk", "外部内容块"),
    (NS_MC, "AlternateContent", "兼容替代内容"),
];

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Story {
    pub text: String,
    pub deleted: String,
    pub field_codes: Vec<String>,
    pub math: String,
    pub counts: BTreeMap<String, usize>,
    pub ids: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub stories: BTreeMap<String, Story>,
    /// 部件名 → SHA-256
    pub parts: BTreeMap<String, String>,
    pub rels: BTreeMap<String, Vec<(String, String, String, String)>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Problem {
    pub part: String,
    pub kind: String,
    pub detail: String,
}

fn parse(s: &str) -> Result<Document<'_>> {
    Document::parse_with_options(s, ParsingOptions { allow_dtd: true, ..ParsingOptions::default() })
        .map_err(|e| Error::Xml(e.to_string()))
}

fn is(n: &Node, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(ns)
}

fn wattr<'a>(n: &Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.attribute((NS_W, name))
}

struct Walk {
    story: Story,
    fields: Vec<String>,
}

impl Walk {
    fn id(&mut self, key: &str, v: impl Into<String>) {
        self.story.ids.entry(key.to_string()).or_default().push(v.into());
    }

    fn node(&mut self, n: Node) {
        for c in n.children().filter(|c| c.is_element()) {
            let name = c.tag_name().name();
            let ns = c.tag_name().namespace().unwrap_or("");
            for (cns, cname, label) in COUNTED {
                if ns == *cns && name == *cname {
                    *self.story.counts.entry(label.to_string()).or_default() += 1;
                }
            }
            let parent_is_run = is(&n, NS_W, "r");
            match (ns, name) {
                (NS_W, "t") => self.story.text.push_str(c.text().unwrap_or("")),
                (NS_W, "delText") => self.story.deleted.push_str(c.text().unwrap_or("")),
                (NS_W, "instrText") => {
                    if let Some(top) = self.fields.last_mut() {
                        top.push_str(c.text().unwrap_or(""));
                    }
                }
                (NS_W, "tab") if parent_is_run => self.story.text.push('\t'),
                (NS_W, "br") | (NS_W, "cr") if parent_is_run => self.story.text.push('\n'),
                (NS_W, "noBreakHyphen") => self.story.text.push('\u{2011}'),
                (NS_W, "softHyphen") => self.story.text.push('\u{ad}'),
                (NS_W, "sym") => self.story.text.push_str(&format!("[sym {}]", wattr(&c, "char").unwrap_or(""))),
                (NS_M, "t") => self.story.math.push_str(c.text().unwrap_or("")),
                (NS_W, "fldChar") => match wattr(&c, "fldCharType") {
                    Some("begin") => {
                        self.fields.push(String::new());
                        *self.story.counts.entry("复杂域".into()).or_default() += 1;
                    }
                    Some("end") => {
                        if let Some(code) = self.fields.pop() {
                            self.story.field_codes.push(code.trim().to_string());
                        }
                    }
                    _ => {}
                },
                (NS_W, "fldSimple") => self.story.field_codes.push(wattr(&c, "instr").unwrap_or("").trim().to_string()),
                (NS_W, "bookmarkStart") => self.id("书签", wattr(&c, "name").unwrap_or("")),
                (NS_W, "tag") => self.id("内容控件 tag", wattr(&c, "val").unwrap_or("")),
                (NS_W, "hyperlink") => {
                    let target = c.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id")).map(|s| format!("rel:{s}"));
                    self.id("超链接", target.or_else(|| wattr(&c, "anchor").map(|a| format!("#{a}"))).unwrap_or_default());
                }
                (NS_A, "blip") => self.id("图片", c.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "embed")).unwrap_or("")),
                (NS_W, "footnoteReference") | (NS_W, "endnoteReference") | (NS_W, "commentReference") => {
                    self.id(name, wattr(&c, "id").unwrap_or(""))
                }
                (NS_W, "numPr") => {
                    let get = |k: &str| c.children().find(|x| is(x, NS_W, k)).and_then(|x| wattr(&x, "val")).unwrap_or("").to_string();
                    self.id("列表", format!("{}:{}", get("numId"), get("ilvl")));
                }
                _ => {}
            }
            self.node(c);
            if ns == NS_W && name == "p" {
                self.story.text.push('\n');
            }
        }
    }
}

pub fn story_of(xml: &str) -> Result<Story> {
    let doc = parse(xml)?;
    let mut w = Walk { story: Story::default(), fields: Vec::new() };
    w.node(doc.root());
    Ok(w.story)
}

pub fn snapshot(pkg: &Package) -> Result<Snapshot> {
    let mut snap = Snapshot::default();
    for name in pkg.names() {
        let bytes = pkg.get(name).unwrap_or(&[]);
        snap.parts.insert(name.to_string(), hex(&Sha256::digest(bytes)));
        if name.ends_with(".rels") {
            snap.rels.insert(name.to_string(), relationships(pkg, name));
        }
    }
    for part in story_parts(pkg) {
        snap.stories.insert(part.clone(), story_of(&pkg.get_str(&part)?)?);
    }
    Ok(snap)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 排版可以改的部件；其余部件必须逐字节不变。
pub fn may_change(part: &str) -> bool {
    matches!(part, "word/document.xml" | "word/styles.xml" | "word/settings.xml" | "word/footnotes.xml" | "word/endnotes.xml" | "word/stylesWithEffects.xml")
        || (part.starts_with("word/header") && part.ends_with(".xml"))
        || (part.starts_with("word/footer") && part.ends_with(".xml"))
}

fn first_diff(a: &str, b: &str) -> String {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let i = ac.iter().zip(&bc).position(|(x, y)| x != y).unwrap_or(ac.len().min(bc.len()));
    let from = i.saturating_sub(12);
    let snip = |v: &[char]| v[from.min(v.len())..(i + 12).min(v.len())].iter().collect::<String>().replace('\n', "⏎");
    format!("第 {i} 个字符起不同：原「{}」→ 现「{}」", snip(&ac), snip(&bc))
}

pub fn compare(before: &Snapshot, after: &Snapshot) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut push = |part: &str, kind: &str, detail: String| out.push(Problem { part: part.to_string(), kind: kind.to_string(), detail });
    for (name, hash) in &before.parts {
        match after.parts.get(name) {
            None => push(name, "部件丢失", "排版后的文档里没有这个部件".into()),
            Some(h) if h != hash && !may_change(name) => push(name, "部件被改动", "这个部件不该被排版改动".into()),
            _ => {}
        }
    }
    for name in after.parts.keys().filter(|n| !before.parts.contains_key(*n)) {
        push(name, "新增部件", "排版前没有这个部件".into());
    }
    for (name, rels) in &before.rels {
        if after.rels.get(name) != Some(rels) {
            push(name, "关系变化", "部件之间的引用关系变了".into());
        }
    }
    for (part, b) in &before.stories {
        let Some(a) = after.stories.get(part) else {
            push(part, "故事丢失", "正文 / 脚注 / 页眉等部件不见了".into());
            continue;
        };
        if a.text != b.text {
            push(part, "正文文字变化", first_diff(&b.text, &a.text));
        }
        if a.deleted != b.deleted {
            push(part, "修订删除文字变化", first_diff(&b.deleted, &a.deleted));
        }
        if a.math != b.math {
            push(part, "公式文字变化", first_diff(&b.math, &a.math));
        }
        if a.field_codes != b.field_codes {
            push(part, "域代码变化", format!("原 {} 个 → 现 {} 个", b.field_codes.len(), a.field_codes.len()));
        }
        for (k, v) in &b.counts {
            let n = a.counts.get(k).copied().unwrap_or(0);
            if n != *v {
                push(part, "结构数量变化", format!("{k}：{v} → {n}"));
            }
        }
        for (k, v) in &a.counts {
            if !b.counts.contains_key(k) {
                push(part, "结构数量变化", format!("{k}：0 → {v}"));
            }
        }
        for (k, v) in &b.ids {
            if a.ids.get(k) != Some(v) {
                push(part, "标识变化", format!("{k} 不一致"));
            }
        }
    }
    out
}

// ---------- 规范是否生效 ----------

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub item: String,
    pub ok: bool,
    pub detail: String,
}

struct StyleInfo<'a> {
    name: String,
    based_on: Option<String>,
    node: Node<'a, 'a>,
}

struct Styles<'a> {
    by_id: BTreeMap<String, StyleInfo<'a>>,
    defaults_rpr: Option<Node<'a, 'a>>,
    defaults_ppr: Option<Node<'a, 'a>>,
}

impl<'a> Styles<'a> {
    fn new(doc: &'a Document<'a>) -> Self {
        let mut by_id = BTreeMap::new();
        for s in doc.descendants().filter(|n| is(n, NS_W, "style")) {
            let id = wattr(&s, "styleId").unwrap_or("").to_string();
            let name = s.children().find(|c| is(c, NS_W, "name")).and_then(|c| wattr(&c, "val")).unwrap_or("").to_lowercase();
            let based_on = s.children().find(|c| is(c, NS_W, "basedOn")).and_then(|c| wattr(&c, "val")).map(String::from);
            by_id.insert(id, StyleInfo { name, based_on, node: s });
        }
        let find = |outer: &str, inner: &str| {
            doc.descendants().find(|n| is(n, NS_W, outer)).and_then(|d| d.children().find(|c| is(c, NS_W, inner)))
        };
        Styles { by_id, defaults_rpr: find("rPrDefault", "rPr"), defaults_ppr: find("pPrDefault", "pPr") }
    }

    fn id_by_name(&self, name: &str) -> Option<&str> {
        self.by_id.iter().find(|(_, s)| s.name == name.to_lowercase()).map(|(id, _)| id.as_str())
    }

    /// 沿 basedOn 链找 pPr / rPr 下某个元素（最近的优先），最后看文档默认值。
    fn resolve(&self, id: &str, container: &str, el: &str) -> Option<Node<'a, 'a>> {
        let mut cur = Some(id.to_string());
        let mut guard = 0;
        while let Some(i) = cur {
            guard += 1;
            if guard > 20 {
                break;
            }
            let s = self.by_id.get(&i)?;
            if let Some(found) = s.node.children().find(|c| is(c, NS_W, container)).and_then(|c| c.children().find(|x| is(x, NS_W, el))) {
                return Some(found);
            }
            cur = s.based_on.clone();
        }
        let d = if container == "rPr" { self.defaults_rpr } else { self.defaults_ppr };
        d.and_then(|d| d.children().find(|x| is(x, NS_W, el)))
    }
}

fn check_style(styles: &Styles, label: &str, id: &str, ts: &TextStyle, out: &mut Vec<Check>) {
    let mut add = |what: &str, ok: bool, detail: String| out.push(Check { item: format!("{label}：{what}"), ok, detail });
    let fonts = styles.resolve(id, "rPr", "rFonts");
    let font_attr = |k: &str| fonts.and_then(|f| wattr(&f, k));
    let themed = fonts.map(|f| f.attributes().any(|a| a.name().ends_with("Theme") || a.name() == "cstheme")).unwrap_or(false);
    if let Some(want) = &ts.font_east_asia {
        let got = font_attr("eastAsia").unwrap_or("（未设置）");
        add("中文字体", same_font(got, want) && !themed, format!("期望 {want}，实际 {got}{}", if themed { "（仍有主题字体覆盖）" } else { "" }));
    }
    if let Some(want) = &ts.font_latin {
        let got = font_attr("ascii").unwrap_or("（未设置）");
        add("西文字体", same_font(got, want) && !themed, format!("期望 {want}，实际 {got}"));
    }
    if let Some(pt) = ts.size_pt {
        let got = styles.resolve(id, "rPr", "sz").and_then(|n| wattr(&n, "val")).and_then(|v| v.parse::<f64>().ok()).map(|v| v / 2.0);
        add("字号", got == Some(pt), format!("期望 {pt} 磅，实际 {}", got.map(|g| format!("{g} 磅")).unwrap_or("（未设置）".into())));
    }
    if let Some(b) = ts.bold {
        let got = styles.resolve(id, "rPr", "b").map(|n| !matches!(wattr(&n, "val"), Some("0") | Some("false"))).unwrap_or(false);
        add("加粗", got == b, format!("期望 {b}，实际 {got}"));
    }
    if let Some(a) = &ts.align {
        let got = styles.resolve(id, "pPr", "jc").and_then(|n| wattr(&n, "val")).unwrap_or("left");
        add("对齐", got == a, format!("期望 {a}，实际 {got}"));
    }
    if let Some(m) = ts.line_spacing {
        let sp = styles.resolve(id, "pPr", "spacing");
        let line = sp.and_then(|n| wattr(&n, "line")).and_then(|v| v.parse::<f64>().ok());
        let rule = sp.and_then(|n| wattr(&n, "lineRule")).unwrap_or("auto");
        let want = (240.0 * m).round();
        add("行距", line == Some(want) && rule == "auto", format!("期望 {m} 倍（{want}），实际 {}", line.map(|l| format!("{} {rule}", l)).unwrap_or("（未设置）".into())));
    }
    if let Some(pt) = ts.line_spacing_exact_pt {
        let sp = styles.resolve(id, "pPr", "spacing");
        let line = sp.and_then(|n| wattr(&n, "line")).and_then(|v| v.parse::<f64>().ok());
        let rule = sp.and_then(|n| wattr(&n, "lineRule")).unwrap_or("auto");
        let want = (pt * 20.0).round();
        add("行距", line == Some(want) && rule == "exact", format!("期望固定值 {pt} 磅（{want} exact），实际 {}", line.map(|l| format!("{l} {rule}")).unwrap_or("（未设置）".into())));
    }
    if let Some(b) = ts.page_break_before {
        let got = styles.resolve(id, "pPr", "pageBreakBefore").map(|n| !matches!(wattr(&n, "val"), Some("0") | Some("false"))).unwrap_or(false);
        add("段前分页", got == b, format!("期望 {b}，实际 {got}"));
    }
    if let Some(c) = ts.first_line_indent_chars {
        let ind = styles.resolve(id, "pPr", "ind");
        let chars = ind.and_then(|n| wattr(&n, "firstLineChars")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 100.0;
        let hanging = ind.and_then(|n| wattr(&n, "hanging")).is_some();
        add("首行缩进", (chars - c).abs() < 0.01 && !hanging, format!("期望 {c} 字符，实际 {chars} 字符"));
    }
}

/// 核对排版规范在样式层与页面设置上是否生效，以及正文里冲突的直接格式是否已清除。
pub fn check_spec(pkg: &Package, spec: &Spec) -> Result<Vec<Check>> {
    check_spec_with_plan(pkg, spec, &Plan::default())
}

/// 同上；受保护分节里的段落不算「残留直接格式」，并核对排版计划是否落实。
pub fn check_spec_with_plan(pkg: &Package, spec: &Spec, plan: &Plan) -> Result<Vec<Check>> {
    let mut out = Vec::new();
    let styles_xml = pkg.get_str("word/styles.xml")?;
    let sdoc = parse(&styles_xml)?;
    let styles = Styles::new(&sdoc);
    let named = |label: &str, name: &str, ts: &TextStyle, out: &mut Vec<Check>| match styles.id_by_name(name) {
        Some(id) => check_style(&styles, label, id, ts, out),
        None => out.push(Check { item: format!("{label}：样式存在"), ok: false, detail: format!("找不到样式 {name}") }),
    };
    named("正文", "Normal", &spec.body, &mut out);
    for h in &spec.headings {
        named(&format!("{} 级标题", h.level), &format!("heading {}", h.level), &h.style, &mut out);
    }
    if let Some(c) = &spec.caption {
        named("题注", "caption", c, &mut out);
    }
    if let Some(f) = &spec.footnote {
        named("脚注", "footnote text", f, &mut out);
    }
    if let Some(t) = &spec.table_text {
        named("表格文字", "RL Table Text", t, &mut out);
    }
    if let Some(b) = &spec.bibliography {
        named("参考文献", "bibliography", b, &mut out);
    }
    // 页面设置
    let doc_xml = pkg.get_str("word/document.xml")?;
    let ddoc = parse(&doc_xml)?;
    let want = spec.page.margin_settings();
    for (i, sect) in ddoc.descendants().filter(|n| is(n, NS_W, "sectPr")).enumerate() {
        let mar = sect.children().find(|c| is(c, NS_W, "pgMar"));
        let bad: Vec<String> = want
            .iter()
            .filter(|(k, cm)| mar.and_then(|n| wattr(&n, k)).and_then(|v| v.parse::<i64>().ok()) != Some(cm_to_twips(*cm)))
            .map(|(k, _)| k.to_string())
            .collect();
        if !want.is_empty() {
            out.push(Check { item: format!("第 {} 节：页边距", i + 1), ok: bad.is_empty(), detail: if bad.is_empty() { "符合".into() } else { format!("不符：{}", bad.join("、")) } });
        }
        if spec.page.paper.is_none() {
            continue;
        }
        let sz = sect.children().find(|c| is(c, NS_W, "pgSz"));
        let w = sz.and_then(|n| wattr(&n, "w")).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let h = sz.and_then(|n| wattr(&n, "h")).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let ok = (w, h) == A4_TWIPS || (h, w) == A4_TWIPS;
        out.push(Check { item: format!("第 {} 节：纸张", i + 1), ok, detail: format!("{w}×{h}（{}）", if w > h { "横向" } else { "纵向" }) });
    }
    // 正文里残留的冲突直接格式（代码段落除外）
    let code_names: Vec<String> = spec.code.as_ref().map(|c| c.style_names.iter().map(|s| s.to_lowercase()).collect()).unwrap_or_default();
    let is_code = |id: &str| {
        styles.by_id.get(id).map(|s| code_names.contains(&s.name) || s.name.contains("code") || s.name.contains("代码")).unwrap_or(false)
            || id.to_lowercase().contains("code")
    };
    let sections = section_map(&ddoc);
    let keep = |f: &Node| f.attributes().any(|a| !a.name().ends_with("Theme") && a.name() != "cstheme" && a.name() != "hint" && spec.direct_formatting.keep_fonts.iter().any(|k| same_font(k, a.value())));
    let mut leftovers = 0;
    let mut stale: BTreeMap<String, usize> = BTreeMap::new();
    let mut rule_misses = 0;
    let normal_name = styles.by_id.iter().find(|(_, s)| s.name == "normal").map(|(id, _)| id.clone()).unwrap_or_default();
    for p in ddoc.descendants().filter(|n| is(n, NS_W, "p")) {
        if plan.is_protected(section_of(&p, &sections)) {
            continue;
        }
        let sid = p.children().find(|c| is(c, NS_W, "pPr")).and_then(|pp| pp.children().find(|c| is(c, NS_W, "pStyle"))).and_then(|s| wattr(&s, "val")).unwrap_or("");
        let sname = styles.by_id.get(if sid.is_empty() { normal_name.as_str() } else { sid }).map(|s| s.name.clone()).unwrap_or_default();
        let text: String = p.descendants().filter(|n| is(n, NS_W, "t") && !n.ancestors().any(|a| is(&a, NS_W, "txbxContent"))).filter_map(|n| n.text()).collect();
        match plan.rule_for(&text) {
            Some(t) if t != sname => rule_misses += 1,
            Some(_) => {}
            None if plan.style_map.contains_key(&sname) && !plan.style_map.values().any(|v| *v == sname) => *stale.entry(sname.clone()).or_default() += 1,
            None => {}
        }
        if is_code(sid) {
            continue;
        }
        for r in p.children().flat_map(|c| if is(&c, NS_W, "r") { vec![c] } else { c.descendants().filter(|d| is(d, NS_W, "r") && !d.ancestors().any(|a| is(&a, NS_M, "oMath"))).collect() }) {
            if let Some(rpr) = r.children().find(|c| is(c, NS_W, "rPr")) {
                let has = |n: &str| rpr.children().any(|c| is(&c, NS_W, n));
                let fonts_left = rpr.children().find(|c| is(c, NS_W, "rFonts")).is_some_and(|f| !keep(&f));
                if (spec.direct_formatting.strip_fonts && fonts_left) || (spec.direct_formatting.strip_sizes && (has("sz") || has("szCs"))) {
                    leftovers += 1;
                }
            }
        }
    }
    out.push(Check { item: "正文：冲突的直接字体 / 字号已清除".into(), ok: leftovers == 0, detail: format!("残留 {leftovers} 处") });
    if !plan.style_map.is_empty() {
        out.push(Check {
            item: "排版计划：需映射的样式已不再使用".into(),
            ok: stale.is_empty(),
            detail: if stale.is_empty() { "符合".into() } else { stale.iter().map(|(k, v)| format!("{k} 仍有 {v} 段")).collect::<Vec<_>>().join("、") },
        });
    }
    if !plan.paragraph_rules.is_empty() {
        out.push(Check { item: "排版计划：按文字识别的段落已套用目标样式".into(), ok: rule_misses == 0, detail: format!("未套用 {rule_misses} 段") });
    }
    Ok(out)
}

/// 正文顶层元素 → 所在分节（从 1 开始）。分节符在段落属性里，属于该段所在的节。
fn section_map(doc: &Document) -> std::collections::HashMap<roxmltree::NodeId, usize> {
    let mut map = std::collections::HashMap::new();
    let Some(body) = doc.descendants().find(|n| is(n, NS_W, "body")) else { return map };
    let mut no = 1;
    for c in body.children().filter(|c| c.is_element()) {
        map.insert(c.id(), no);
        if is(&c, NS_W, "p") && c.children().any(|pp| is(&pp, NS_W, "pPr") && pp.children().any(|s| is(&s, NS_W, "sectPr"))) {
            no += 1;
        }
    }
    map
}

fn section_of(n: &Node, map: &std::collections::HashMap<roxmltree::NodeId, usize>) -> usize {
    n.ancestors().find_map(|a| map.get(&a.id()).copied()).unwrap_or(0)
}

// ---------- 受保护分节的外观核对（与排版器的冻结逻辑分开实现） ----------

type Attrs = BTreeMap<String, String>;

/// 默认值：缺省与写成默认值视为相同
fn default_of(el: &str, attr: &str) -> &'static str {
    match (el, attr) {
        ("spacing", "line") => "240",
        (_, "lineRule") => "auto",
        ("jc", "val") => "left",
        ("color", "val") => "auto",
        ("sz" | "szCs", "val") => "20",
        _ => "0",
    }
}

fn normalized(el: &str, attrs: Option<&Attrs>) -> String {
    const TOGGLE: &[&str] = &["b", "bCs", "pageBreakBefore"];
    if TOGGLE.contains(&el) {
        let on = attrs.is_some_and(|a| !matches!(a.get("val").map(String::as_str), Some("0" | "false" | "off")));
        return if on { "on".into() } else { "off".into() };
    }
    let Some(a) = attrs else { return String::new() };
    let mut parts: Vec<String> = a
        .iter()
        .filter(|(k, v)| v.as_str() != default_of(el, k) && !(el == "jc" && v.as_str() == "start"))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    parts.sort();
    parts.join(",")
}

/// 一层属性叠到 acc 上；字体按槽位（ascii / hAnsi / eastAsia / cs）合并，主题字体与显式字体互相覆盖。
fn layer(acc: &mut BTreeMap<String, Attrs>, container: Option<Node>, keys: &[&str]) {
    let Some(c) = container else { return };
    for e in c.children().filter(|e| e.is_element() && e.tag_name().namespace() == Some(NS_W) && keys.contains(&e.tag_name().name())) {
        let name = e.tag_name().name().to_string();
        let slot = acc.entry(name.clone()).or_default();
        let is_theme = |n: &str| n.ends_with("Theme") || n == "cstheme";
        // 同一元素里主题字体优先于显式字体：先放显式，再放主题
        let mut attrs: Vec<_> = e.attributes().filter(|a| a.namespace() == Some(NS_W)).collect();
        attrs.sort_by_key(|a| is_theme(a.name()));
        for a in attrs {
            if name == "rFonts" {
                let slot_name = if a.name() == "cstheme" { "cs" } else { a.name().trim_end_matches("Theme") };
                let v = if is_theme(a.name()) { format!("theme:{}", a.value()) } else { a.value().to_string() };
                slot.insert(slot_name.to_string(), v);
            } else {
                slot.insert(a.name().to_string(), a.value().to_string());
            }
        }
    }
}

fn style_chain<'a>(styles: &Styles<'a>, id: Option<&str>) -> Vec<Node<'a, 'a>> {
    let mut out = Vec::new();
    let mut cur = id.map(String::from);
    while let Some(i) = cur {
        let Some(s) = styles.by_id.get(&i) else { break };
        if out.len() > 20 {
            break;
        }
        out.push(s.node);
        cur = s.based_on.clone();
    }
    out.reverse();
    out
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| is(c, NS_W, name))
}

/// 段落的有效外观签名：段落属性 + 段落标记 + 每个 run
fn appearance(p: Node, styles: &Styles, default_para: Option<&str>) -> String {
    const PK: &[&str] = &["spacing", "ind", "jc", "pageBreakBefore"];
    const RK: &[&str] = &["rFonts", "b", "bCs", "color", "sz", "szCs"];
    let ppr = child(p, "pPr");
    let pstyle = ppr.and_then(|pp| child(pp, "pStyle")).and_then(|s| wattr(&s, "val")).map(String::from);
    let pchain = style_chain(styles, pstyle.as_deref().or(default_para));
    let mut para = BTreeMap::new();
    layer(&mut para, styles.defaults_ppr, PK);
    for s in &pchain {
        layer(&mut para, child(*s, "pPr"), PK);
    }
    layer(&mut para, ppr, PK);
    let run_sig = |rpr: Option<Node>| {
        let rstyle = rpr.and_then(|r| child(r, "rStyle")).and_then(|s| wattr(&s, "val")).map(String::from);
        let mut acc = BTreeMap::new();
        layer(&mut acc, styles.defaults_rpr, RK);
        for s in &pchain {
            layer(&mut acc, child(*s, "rPr"), RK);
        }
        for s in style_chain(styles, rstyle.as_deref()) {
            layer(&mut acc, child(s, "rPr"), RK);
        }
        layer(&mut acc, rpr, RK);
        RK.iter().map(|k| format!("{k}[{}]", fonts_norm(k, acc.get(*k)))).collect::<Vec<_>>().join(" ")
    };
    let mut sig = PK.iter().map(|k| format!("{k}[{}]", normalized(k, para.get(*k)))).collect::<Vec<_>>().join(" ");
    sig.push_str(&format!(" | 标记 {}", run_sig(ppr.and_then(|pp| child(pp, "rPr")))));
    for r in p.descendants().filter(|n| is(n, NS_W, "r") && !n.ancestors().any(|a| is(&a, NS_W, "txbxContent") || is(&a, NS_M, "oMath"))) {
        if r.ancestors().skip(1).find(|a| is(a, NS_W, "p")).map(|a| a.id()) != Some(p.id()) {
            continue;
        }
        sig.push_str(&format!(" | {}", run_sig(child(r, "rPr"))));
    }
    sig
}

fn fonts_norm(k: &str, a: Option<&Attrs>) -> String {
    if k != "rFonts" {
        return normalized(k, a);
    }
    let Some(a) = a else { return String::new() };
    ["ascii", "hAnsi", "eastAsia", "cs"]
        .iter()
        .map(|slot| {
            let v = a.get(*slot).map(String::as_str).unwrap_or("");
            let canon = ["宋体", "黑体", "楷体", "仿宋", "新宋体", "微软雅黑", "等线"].iter().find(|zh| same_font(zh, v)).map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
            format!("{slot}={canon}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// 受保护分节（如封面）排版前后的有效外观必须一致。
pub fn check_protected(before: &Package, after: &Package, plan: &Plan) -> Result<Vec<Problem>> {
    if plan.protect_sections.is_empty() {
        return Ok(Vec::new());
    }
    // 按段落在文档里的次序对：结构类排版可能在前面加分节符，分节号会变，段落的次序不变
    type Sig = (usize, usize, String, String);
    let sigs = |pkg: &Package, pick: &dyn Fn(usize, usize) -> bool| -> Result<Vec<Sig>> {
        let sx = pkg.get_str("word/styles.xml")?;
        let sdoc = parse(&sx)?;
        let styles = Styles::new(&sdoc);
        let default_para = sdoc
            .descendants()
            .find(|n| is(n, NS_W, "style") && wattr(n, "type") == Some("paragraph") && wattr(n, "default") == Some("1"))
            .and_then(|n| wattr(&n, "styleId"))
            .map(String::from);
        let dx = pkg.get_str("word/document.xml")?;
        let ddoc = parse(&dx)?;
        let map = section_map(&ddoc);
        Ok(ddoc
            .descendants()
            .filter(|n| is(n, NS_W, "p"))
            .enumerate()
            .filter(|(i, n)| pick(*i, section_of(n, &map)))
            .map(|(i, p)| {
                let text: String = p.descendants().filter(|n| is(n, NS_W, "t")).filter_map(|n| n.text()).collect();
                (i, section_of(&p, &map), text.chars().take(20).collect(), appearance(p, &styles, default_para.as_deref()))
            })
            .collect())
    };
    let a = sigs(before, &|_, section| plan.is_protected(section))?;
    let wanted: std::collections::BTreeSet<usize> = a.iter().map(|x| x.0).collect();
    let b = sigs(after, &|i, _| wanted.contains(&i))?;
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(Problem { part: "word/document.xml".into(), kind: "保护区域外观变化".into(), detail: format!("受保护段落数 {} → {}", a.len(), b.len()) });
        return Ok(out);
    }
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x.3 != y.3 {
            out.push(Problem { part: "word/document.xml".into(), kind: "保护区域外观变化".into(), detail: format!("第 {} 节第 {} 个受保护段落「{}」：{} → {}", x.1, i + 1, x.2, x.3, y.3) });
        }
    }
    Ok(out)
}

/// 排版结果的全部核对：保真（结构类排版允许的改动之外一概不许变）+ 受保护分节的外观 + 规范与结构逐项是否生效。
pub fn verify_all(before: &Package, after: &Package, spec: &Spec, plan: &Plan) -> Result<(Vec<Problem>, Vec<Check>)> {
    let mut problems = crate::verify_structure::compare(before, after, spec, plan)?;
    problems.extend(check_protected(before, after, plan)?);
    let mut checks = check_spec_with_plan(after, spec, plan)?;
    checks.extend(crate::verify_structure::check(before, after, spec, plan)?);
    Ok((problems, checks))
}
