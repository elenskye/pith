//! 文档结构摘要（只读）：段落、样式用量、分节。排版 Agent 用它给模型看论文、预览计划会改到哪些段落，
//! 「只检查」的项目（`checks`）也在它上面统计。另有 `doc_text`：把规范、模板等要求文档读成纯文字。
//!
//! 用 roxmltree 解析，与排版器（quick-xml 的可编辑树）分开；分节的数法与排版器、校验器一致：
//! 分节符在段落属性里，属于该段所在的节。

use std::collections::BTreeMap;

use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;

use crate::package::Package;
use crate::plan::Plan;
use crate::{Error, Result, NS_M, NS_W};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleUse {
    pub id: String,
    /// 样式名（Word 界面里看到的名字，如 Normal (Web)、heading 1）
    pub name: String,
    pub based_on: Option<String>,
    /// 大纲级别（1 = 一级标题）；样式自己或祖先样式上设的
    pub outline_level: Option<u8>,
    /// 正文（document.xml）里用它的段落数、字数
    pub count: usize,
    pub chars: usize,
    /// 其中在表格里的段落数
    pub in_table: usize,
    /// 头几段的开头（非空段落）
    pub samples: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Para {
    /// 从 1 开始，按文档顺序
    pub index: usize,
    /// 所在分节（从 1 开始）
    pub section: usize,
    /// 样式名；没写样式的段落是默认段落样式
    pub style: String,
    pub text: String,
    pub in_table: bool,
    pub in_textbox: bool,
    /// 含图片 / 图形
    pub figure: bool,
    /// 含公式
    pub math: bool,
    /// 段落上直接设置的对齐（center / left / right / both）
    pub align: Option<String>,
    /// 带自动编号（列表）
    pub numbered: bool,
    /// 大纲级别（段落直接设的，或样式带的）
    pub outline_level: Option<u8>,
}

impl Para {
    pub fn chars(&self) -> usize {
        self.text.chars().filter(|c| !c.is_whitespace()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.chars() == 0 && !self.figure && !self.math
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionInfo {
    pub index: usize,
    pub width_twips: i64,
    pub height_twips: i64,
    pub paragraphs: usize,
    pub chars: usize,
    /// 这一节头几个非空段落的开头
    pub opening: Vec<String>,
}

/// 一个表格的概况（三线表的取舍、界面上让用户挑表格用）
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    /// 文档里第几个表格（从 1 起，嵌套的也算）
    pub index: usize,
    pub section: usize,
    pub rows: usize,
    pub cols: usize,
    /// 表格里第一个段落的编号
    pub paragraph: usize,
    /// 第一行各格的文字
    pub first_row: String,
    /// 看得见边框（表格、单元格或表格样式上有）
    pub bordered: bool,
    /// 嵌在别的表格里
    pub nested: bool,
    /// 行或单元格外面包着内容控件、修订等，结构不规整
    pub irregular: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outline {
    /// 默认段落样式的名字（一般是 Normal）
    pub default_style: String,
    pub styles: Vec<StyleUse>,
    pub paragraphs: Vec<Para>,
    pub sections: Vec<SectionInfo>,
    pub tables: usize,
    pub table_list: Vec<TableInfo>,
    pub figures: usize,
    pub footnotes: usize,
    /// Word 上次存盘时记的页数（docProps/app.xml；只是提示，判断单面 / 双面用）
    pub pages_hint: Option<usize>,
}

fn parse(s: &str) -> Result<Document<'_>> {
    Document::parse_with_options(s, ParsingOptions { allow_dtd: true, ..ParsingOptions::default() }).map_err(|e| Error::Xml(e.to_string()))
}

fn is(n: &Node, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(NS_W)
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| is(c, name))
}

fn wval<'a>(n: Node<'a, 'a>) -> Option<&'a str> {
    n.attribute((NS_W, "val"))
}

/// 段落文字：w:t、制表符、换行；不含删除的文字、域代码，也不进文本框和图形里的段落（它们单独算）
fn para_text(n: Node, out: &mut String) {
    for c in n.children().filter(|c| c.is_element()) {
        let name = c.tag_name().name();
        if c.tag_name().namespace() == Some(NS_W) {
            match name {
                "t" => out.push_str(c.text().unwrap_or("")),
                "tab" => out.push('\t'),
                "br" | "cr" => out.push('\n'),
                "noBreakHyphen" => out.push('-'),
                "del" | "moveFrom" | "pPr" | "rPr" | "drawing" | "pict" | "object" | "txbxContent" => {}
                _ => para_text(c, out),
            }
        } else if c.tag_name().namespace() == Some(NS_M) {
            // 公式文字不算进段落文字（有 math 标记）
        } else if name != "AlternateContent" {
            para_text(c, out);
        }
    }
}

pub fn snippet(text: &str, max: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max { t } else { format!("{}…", t.chars().take(max).collect::<String>()) }
}

struct StyleDef {
    name: String,
    based_on: Option<String>,
    outline: Option<u8>,
}

fn read_styles(pkg: &Package) -> Result<(BTreeMap<String, StyleDef>, Option<String>)> {
    let Ok(xml) = pkg.get_str("word/styles.xml") else { return Ok((BTreeMap::new(), None)) };
    let doc = parse(&xml)?;
    let mut map = BTreeMap::new();
    let mut default = None;
    for s in doc.descendants().filter(|n| is(n, "style") && n.attribute((NS_W, "type")) == Some("paragraph")) {
        let Some(id) = s.attribute((NS_W, "styleId")) else { continue };
        let name = child(s, "name").and_then(wval).unwrap_or(id).to_string();
        let outline = child(s, "pPr").and_then(|p| child(p, "outlineLvl")).and_then(wval).and_then(|v| v.parse::<u8>().ok()).filter(|v| *v < 9).map(|v| v + 1);
        if matches!(s.attribute((NS_W, "default")), Some("1" | "true")) {
            default = Some(id.to_string());
        }
        map.insert(id.to_string(), StyleDef { name, based_on: child(s, "basedOn").and_then(wval).map(String::from), outline });
    }
    Ok((map, default))
}

fn outline_of(styles: &BTreeMap<String, StyleDef>, id: &str) -> Option<u8> {
    let mut cur = Some(id);
    for _ in 0..20 {
        let s = styles.get(cur?)?;
        if s.outline.is_some() {
            return s.outline;
        }
        cur = s.based_on.as_deref();
    }
    None
}

fn visible(n: Node) -> bool {
    n.is_element() && n.attribute((NS_W, "val")).is_some_and(|v| v != "nil" && v != "none")
}

/// 表格样式（连同它基于的样式）里有没有看得见的边框
fn style_has_borders(styles: &Document, id: &str) -> bool {
    let mut cur = Some(id.to_string());
    for _ in 0..10 {
        let Some(id) = cur else { break };
        let Some(st) = styles.descendants().find(|n| is(n, "style") && n.attribute((NS_W, "styleId")) == Some(id.as_str())) else { break };
        if st.descendants().filter(|n| is(n, "tblBorders") || is(n, "tcBorders")).any(|b| b.children().any(visible)) {
            return true;
        }
        cur = child(st, "basedOn").and_then(wval).map(String::from);
    }
    false
}

fn table_info(tbl: Node, index: usize, section: usize, paragraph: usize, styles: Option<&Document>) -> TableInfo {
    let rows: Vec<Node> = tbl.children().filter(|c| is(c, "tr")).collect();
    fn cells<'a>(tr: Node<'a, 'a>) -> Vec<Node<'a, 'a>> {
        tr.children().filter(|c| is(c, "tc")).collect()
    }
    let span = |tc: &Node| child(*tc, "tcPr").and_then(|p| child(p, "gridSpan")).and_then(wval).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
    let cols = rows.iter().map(|r| cells(*r).iter().map(span).sum::<usize>()).max().unwrap_or(0);
    let pr = child(tbl, "tblPr");
    let own = pr.and_then(|p| child(p, "tblBorders"));
    let cell_borders = tbl.descendants().filter(|n| is(n, "tcBorders") && n.ancestors().find(|a| is(a, "tbl")).map(|a| a.id()) == Some(tbl.id())).any(|b| b.children().any(visible));
    let bordered = cell_borders
        || match own {
            Some(b) => b.children().any(visible),
            None => pr.and_then(|p| child(p, "tblStyle")).and_then(wval).is_some_and(|id| styles.is_some_and(|s| style_has_borders(s, id))),
        };
    let odd = |n: Node| n.children().any(|c| c.is_element() && matches!(c.tag_name().name(), "sdt" | "customXml" | "ins" | "del" | "moveFrom" | "moveTo" | "smartTag"));
    let first_row = rows
        .first()
        .map(|r| {
            cells(*r)
                .iter()
                .map(|tc| {
                    let mut t = String::new();
                    for p in tc.descendants().filter(|n| is(n, "p")) {
                        para_text(p, &mut t);
                        t.push(' ');
                    }
                    snippet(&t, 12)
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default();
    TableInfo { index, section, rows: rows.len(), cols, paragraph, first_row: snippet(&first_row, 60), bordered, nested: tbl.ancestors().skip(1).any(|a| is(&a, "tc")), irregular: odd(tbl) || rows.iter().any(|r| odd(*r)) }
}

pub fn read(pkg: &Package) -> Result<Outline> {
    let (styles, default_id) = read_styles(pkg)?;
    let default_name = default_id.as_ref().and_then(|d| styles.get(d)).map(|s| s.name.clone()).unwrap_or_else(|| "Normal".into());
    let xml = pkg.get_str("word/document.xml")?;
    let doc = parse(&xml)?;
    let body = doc.descendants().find(|n| is(n, "body")).ok_or_else(|| Error::Missing("word/document.xml 的 body".into()))?;

    // 顶层元素 → 分节
    let mut top_section = std::collections::HashMap::new();
    let mut ends: Vec<Node> = Vec::new();
    let mut no = 1;
    for c in body.children().filter(|c| c.is_element()) {
        top_section.insert(c.id(), no);
        if is(&c, "p")
            && let Some(sect) = child(c, "pPr").and_then(|pp| child(pp, "sectPr"))
        {
            ends.push(sect);
            no += 1;
        }
    }
    let last_sect = child(body, "sectPr");
    let section_count = no;

    let mut paragraphs = Vec::new();
    for p in body.descendants().filter(|n| is(n, "p")) {
        let section = p.ancestors().find_map(|a| top_section.get(&a.id()).copied()).unwrap_or(section_count);
        let ppr = child(p, "pPr");
        let sid = ppr.and_then(|pp| child(pp, "pStyle")).and_then(wval);
        let style = match sid {
            Some(id) => styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()),
            None => default_name.clone(),
        };
        let direct_outline = ppr.and_then(|pp| child(pp, "outlineLvl")).and_then(wval).and_then(|v| v.parse::<u8>().ok()).filter(|v| *v < 9).map(|v| v + 1);
        let mut text = String::new();
        para_text(p, &mut text);
        let anc = |name: &str| p.ancestors().skip(1).any(|a| is(&a, name));
        let has = |pred: &dyn Fn(&Node) -> bool| p.descendants().any(|d| pred(&d) && d.ancestors().find(|a| is(a, "p")).map(|a| a.id()) == Some(p.id()));
        paragraphs.push(Para {
            index: paragraphs.len() + 1,
            section,
            style,
            text,
            in_table: anc("tc"),
            in_textbox: anc("txbxContent"),
            figure: has(&|d| is(d, "drawing") || is(d, "pict")),
            math: p.children().any(|c| c.is_element() && c.tag_name().namespace() == Some(NS_M)),
            align: ppr.and_then(|pp| child(pp, "jc")).and_then(wval).map(String::from),
            numbered: ppr.and_then(|pp| child(pp, "numPr")).is_some(),
            outline_level: direct_outline.or_else(|| outline_of(&styles, sid.or(default_id.as_deref()).unwrap_or(""))),
        });
    }

    // 样式用量
    let mut uses: BTreeMap<String, StyleUse> = BTreeMap::new();
    let by_name: BTreeMap<&str, (&String, &StyleDef)> = styles.iter().map(|(id, s)| (s.name.as_str(), (id, s))).collect();
    for p in &paragraphs {
        let u = uses.entry(p.style.clone()).or_insert_with(|| {
            let def = by_name.get(p.style.as_str());
            StyleUse {
                id: def.map(|d| d.0.clone()).unwrap_or_default(),
                name: p.style.clone(),
                based_on: def.and_then(|d| d.1.based_on.as_ref()).map(|b| styles.get(b).map(|s| s.name.clone()).unwrap_or_else(|| b.clone())),
                outline_level: def.and_then(|d| outline_of(&styles, d.0)),
                count: 0,
                chars: 0,
                in_table: 0,
                samples: Vec::new(),
            }
        });
        u.count += 1;
        u.chars += p.chars();
        if p.in_table {
            u.in_table += 1;
        }
        if u.samples.len() < 4 && p.chars() > 0 {
            u.samples.push(snippet(&p.text, 24));
        }
    }
    let mut style_list: Vec<StyleUse> = uses.into_values().collect();
    style_list.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    // 分节
    let size = |sect: Option<Node>| -> (i64, i64) {
        let sz = sect.and_then(|s| child(s, "pgSz"));
        let num = |k: &str| sz.and_then(|n| n.attribute((NS_W, k))).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        (num("w"), num("h"))
    };
    let mut sections = Vec::new();
    for i in 1..=section_count {
        let sect = if i <= ends.len() { Some(ends[i - 1]) } else { last_sect };
        let (w, h) = size(sect);
        let mine: Vec<&Para> = paragraphs.iter().filter(|p| p.section == i).collect();
        sections.push(SectionInfo {
            index: i,
            width_twips: w,
            height_twips: h,
            paragraphs: mine.len(),
            chars: mine.iter().map(|p| p.chars()).sum(),
            opening: mine.iter().filter(|p| p.chars() > 0).take(6).map(|p| snippet(&p.text, 30)).collect(),
        });
    }

    let footnotes = match pkg.get_str("word/footnotes.xml") {
        Ok(f) => parse(&f)?.descendants().filter(|n| is(n, "footnote") && n.attribute((NS_W, "type")).is_none()).count(),
        Err(_) => 0,
    };
    // 表格：编号按文档顺序（与结构类排版里走的顺序一致）
    let styles_xml = pkg.get_str("word/styles.xml").ok();
    let styles_doc = styles_xml.as_deref().and_then(|x| parse(x).ok());
    let para_ids: std::collections::HashMap<roxmltree::NodeId, usize> = body.descendants().filter(|n| is(n, "p")).enumerate().map(|(i, p)| (p.id(), i + 1)).collect();
    let table_list: Vec<TableInfo> = body
        .descendants()
        .filter(|n| is(n, "tbl"))
        .enumerate()
        .map(|(i, t)| {
            let section = t.ancestors().find_map(|a| top_section.get(&a.id()).copied()).unwrap_or(section_count);
            let first_p = t.descendants().find(|n| is(n, "p")).and_then(|p| para_ids.get(&p.id()).copied()).unwrap_or(0);
            table_info(t, i + 1, section, first_p, styles_doc.as_ref())
        })
        .collect();
    let pages_hint = pkg.get_str("docProps/app.xml").ok().and_then(|x| {
        let i = x.find("<Pages>")? + 7;
        x[i..].split('<').next()?.trim().parse::<usize>().ok()
    });
    Ok(Outline {
        default_style: default_name,
        styles: style_list,
        tables: table_list.len(),
        table_list,
        pages_hint,
        figures: paragraphs.iter().filter(|p| p.figure).count(),
        footnotes,
        paragraphs,
        sections,
    })
}

/// 排版计划会改到正文里的哪些段落（与排版器的映射规则一致：文字规则优先于样式映射，受保护的分节不动）
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// 「normal (web) → normal（样式映射）」，与排版报告里的写法相同
    pub label: String,
    pub from: String,
    pub to: String,
    /// rule = 文字规则，style = 样式映射
    pub how: String,
    pub count: usize,
    pub samples: Vec<String>,
}

pub fn preview(outline: &Outline, plan: &Plan) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    for p in &outline.paragraphs {
        if plan.is_protected(p.section) {
            continue;
        }
        let name = p.style.to_lowercase();
        let (target, how) = match plan.rule_for(&p.text) {
            Some(t) => (t, "rule"),
            None => match plan.style_map.get(&name) {
                Some(t) => (t.clone(), "style"),
                None => continue,
            },
        };
        if target == name {
            continue;
        }
        let label = format!("{name} → {target}（{}）", if how == "rule" { "文字规则" } else { "样式映射" });
        let h = match hits.iter_mut().find(|h| h.label == label) {
            Some(h) => h,
            None => {
                hits.push(Hit { label, from: name, to: target, how: how.into(), count: 0, samples: Vec::new() });
                hits.last_mut().unwrap()
            }
        };
        h.count += 1;
        if h.samples.len() < 5 && p.chars() > 0 {
            h.samples.push(snippet(&p.text, 40));
        }
    }
    hits.sort_by(|a, b| a.label.cmp(&b.label));
    hits
}

/// 要求文档（规范、模板、附件）读成纯文字：一段一行，表格一行一行、单元格用「 | 」隔开。
pub fn doc_text(pkg: &Package) -> Result<String> {
    let xml = pkg.get_str("word/document.xml")?;
    let doc = parse(&xml)?;
    let body = doc.descendants().find(|n| is(n, "body")).ok_or_else(|| Error::Missing("word/document.xml 的 body".into()))?;
    let mut out = String::new();
    blocks(body, &mut out);
    Ok(out)
}

fn blocks(n: Node, out: &mut String) {
    for c in n.children().filter(|c| c.is_element()) {
        if is(&c, "p") {
            let mut t = String::new();
            para_text(c, &mut t);
            // 文本框里的字（模板的批注式说明常放在文本框里）
            for inner in c.descendants().filter(|d| is(d, "txbxContent")) {
                let mut boxed = String::new();
                blocks(inner, &mut boxed);
                let boxed = boxed.trim();
                if !boxed.is_empty() && !t.contains(boxed) {
                    t.push_str(&format!("〔文本框：{}〕", boxed.replace('\n', " / ")));
                }
            }
            out.push_str(t.trim_end());
            out.push('\n');
        } else if is(&c, "tbl") {
            for tr in c.children().filter(|r| is(r, "tr")) {
                let cells: Vec<String> = tr
                    .descendants()
                    .filter(|d| is(d, "tc") && d.ancestors().find(|a| is(a, "tr")).map(|a| a.id()) == Some(tr.id()))
                    .map(|tc| {
                        let mut t = String::new();
                        blocks(tc, &mut t);
                        t.trim().replace('\n', " / ")
                    })
                    .collect();
                out.push_str(&cells.join(" | "));
                out.push('\n');
            }
        } else if !is(&c, "sectPr") && !is(&c, "tblPr") && !is(&c, "tblGrid") && !is(&c, "tcPr") {
            blocks(c, out);
        }
    }
}
