//! 报告导出成 Word（`decisions/0020`「Word 导出：用 docx-engine 生成」）。
//!
//! 和 Markdown、HTML 用同一份 `render::Doc`，内容一致。用文档引擎的 `Package` 拼一个最小的 .docx：
//! 正文、标题 1–3、表格（细线）、列表、超链接（外部来源）、书签和文档内的跳转（结论里的数字点回事实表那一行）。
//! 不改 `docx-engine` 的任何东西；写完用它的 `outline::doc_text` 读回来核对（测试）。

use docx_engine::package::Package;
use docx_engine::xml::{escape_attr, escape_text};
use research_engine::render::{Block, Doc, Inline};

const DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

struct Builder {
    body: String,
    /// 外部链接：（关系编号, 网址）
    links: Vec<(String, String)>,
    bookmark: u32,
    /// 用到了编号列表
    numbered: u32,
}

/// 书签名：Word 只认字母数字下划线、字母开头、不超过 40 个字符
fn bookmark_name(target: &str) -> String {
    let clean: String = target.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(32).collect();
    format!("r_{clean}")
}

fn run(text: &str, rpr: &str) -> String {
    let mut out = String::new();
    // 换行写成 <w:br/>，制表符写成 <w:tab/>
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<w:br/>");
        }
        for (j, part) in line.split('\t').enumerate() {
            if j > 0 {
                out.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                out.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, escape_text(part)));
            }
        }
    }
    if rpr.is_empty() { format!("<w:r>{out}</w:r>") } else { format!("<w:r><w:rPr>{rpr}</w:rPr>{out}</w:r>") }
}

impl Builder {
    fn link_id(&mut self, url: &str) -> String {
        if let Some((id, _)) = self.links.iter().find(|(_, u)| u == url) {
            return id.clone();
        }
        // rId1 是样式，rId2 是编号
        let id = format!("rId{}", self.links.len() + 10);
        self.links.push((id.clone(), url.to_string()));
        id
    }

    fn inlines(&mut self, list: &[Inline]) -> String {
        let mut out = String::new();
        for i in list {
            match i {
                Inline::Text(t) => out.push_str(&run(t, "")),
                Inline::Strong(t) => out.push_str(&run(t, "<w:b/><w:bCs/>")),
                Inline::Ref { text, target } => out.push_str(&format!(r#"<w:hyperlink w:anchor="{}" w:history="1">{}</w:hyperlink>"#, bookmark_name(target), run(text, r#"<w:rStyle w:val="Hyperlink"/><w:b/>"#))),
                Inline::Link { text, url } => {
                    let id = self.link_id(url);
                    out.push_str(&format!(r#"<w:hyperlink r:id="{id}" w:history="1">{}</w:hyperlink>"#, run(text, r#"<w:rStyle w:val="Hyperlink"/>"#)));
                }
                Inline::Flag { text, why } => {
                    out.push_str(&run(text, r#"<w:b/><w:color w:val="C00000"/><w:highlight w:val="yellow"/>"#));
                    out.push_str(&run(&format!("（{why}）"), r#"<w:color w:val="C00000"/><w:sz w:val="18"/>"#));
                }
                Inline::Tag(t) => out.push_str(&run(&format!("〔{t}〕"), r#"<w:color w:val="7F6000"/><w:sz w:val="18"/>"#)),
            }
        }
        out
    }

    fn para(&mut self, style: Option<&str>, extra_ppr: &str, inner: &str) {
        let ps = style.map(|s| format!(r#"<w:pStyle w:val="{s}"/>"#)).unwrap_or_default();
        let ppr = if ps.is_empty() && extra_ppr.is_empty() { String::new() } else { format!("<w:pPr>{ps}{extra_ppr}</w:pPr>") };
        self.body.push_str(&format!("<w:p>{ppr}{inner}</w:p>"));
    }

    fn with_bookmark(&mut self, anchor: Option<&str>, inner: String) -> String {
        match anchor {
            Some(a) => {
                self.bookmark += 1;
                format!(r#"<w:bookmarkStart w:id="{id}" w:name="{name}"/>{inner}<w:bookmarkEnd w:id="{id}"/>"#, id = self.bookmark, name = escape_attr(&bookmark_name(a)))
            }
            None => inner,
        }
    }

    fn blocks(&mut self, blocks: &[Block]) {
        for b in blocks {
            match b {
                Block::Para(i) => {
                    let inner = self.inlines(i);
                    self.para(None, "", &inner);
                }
                Block::Sub(t) => self.para(Some("Heading3"), "", &run(t, "")),
                Block::Quote(q) => self.para(Some("Quote"), "", &run(q, "")),
                Block::Bullets(items) => {
                    for i in items {
                        let inner = self.inlines(i);
                        self.para(Some("ListParagraph"), r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#, &inner);
                    }
                }
                Block::Numbered(items) => {
                    // 每个编号列表从 1 起：各用一个 num（都指向同一个编号定义，靠 lvlOverride 重新开始）
                    self.numbered += 1;
                    let num = self.numbered + 1;
                    for i in items {
                        let inner = self.inlines(i);
                        self.para(Some("ListParagraph"), &format!(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num}"/></w:numPr>"#), &inner);
                    }
                }
                Block::Table { head, rows } => {
                    let cols = head.len().max(1);
                    let width = 9000 / cols;
                    let mut t = format!(
                        r#"<w:tbl><w:tblPr><w:tblStyle w:val="ReportTable"/><w:tblW w:w="5000" w:type="pct"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>{}</w:tblGrid>"#,
                        (0..cols).map(|_| format!(r#"<w:gridCol w:w="{width}"/>"#)).collect::<String>()
                    );
                    let cell = |inner: &str| format!(r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="TableText"/></w:pPr>{inner}</w:p></w:tc>"#);
                    t.push_str(r#"<w:tr><w:trPr><w:tblHeader/></w:trPr>"#);
                    for h in head {
                        t.push_str(&cell(&run(h, "<w:b/><w:bCs/>")));
                    }
                    t.push_str("</w:tr>");
                    for r in rows {
                        t.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
                        for (n, c) in r.cells.iter().enumerate() {
                            let inner = self.inlines(c);
                            // 行的锚点放在第一格
                            let inner = if n == 0 { self.with_bookmark(r.anchor.as_deref(), inner) } else { inner };
                            t.push_str(&cell(&inner));
                        }
                        for _ in r.cells.len()..cols {
                            t.push_str(&cell(""));
                        }
                        t.push_str("</w:tr>");
                    }
                    t.push_str("</w:tbl>");
                    self.body.push_str(&t);
                    // 表格后面跟一个空段落，免得两张表粘在一起
                    self.para(None, r#"<w:spacing w:after="0"/>"#, "");
                }
            }
        }
    }
}

fn styles() -> String {
    let font = r#"<w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:eastAsia="宋体" w:cs="Times New Roman"/>"#;
    let heading = |id: &str, name: &str, level: u32, size: u32| {
        format!(
            r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="{level}"/></w:pPr><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="黑体"/><w:b/><w:bCs/><w:sz w:val="{size}"/><w:szCs w:val="{size}"/></w:rPr></w:style>"#
        )
    };
    format!(
        "{DECL}<w:styles {NS}><w:docDefaults><w:rPrDefault><w:rPr>{font}<w:sz w:val=\"21\"/><w:szCs w:val=\"21\"/><w:lang w:val=\"en-US\" w:eastAsia=\"zh-CN\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"80\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:eastAsia=\"黑体\"/><w:b/><w:sz w:val=\"36\"/><w:szCs w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Subtitle\"><w:name w:val=\"Subtitle\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:rPr><w:color w:val=\"595959\"/><w:sz w:val=\"18\"/></w:rPr></w:style>\
{}{}{}\
<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"420\"/></w:pPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:ind w:left=\"420\"/></w:pPr><w:rPr><w:color w:val=\"595959\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"TableText\"><w:name w:val=\"Table Text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"260\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:uiPriority w:val=\"99\"/><w:unhideWhenUsed/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
<w:style w:type=\"table\" w:styleId=\"ReportTable\"><w:name w:val=\"Report Table\"/><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"808080\"/><w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"808080\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/></w:tblBorders><w:tblCellMar><w:top w:w=\"40\" w:type=\"dxa\"/><w:left w:w=\"80\" w:type=\"dxa\"/><w:bottom w:w=\"40\" w:type=\"dxa\"/><w:right w:w=\"80\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>\
</w:styles>",
        heading("Heading1", "heading 1", 0, 30),
        heading("Heading2", "heading 2", 1, 26),
        heading("Heading3", "heading 3", 2, 22)
    )
}

fn numbering(lists: u32) -> String {
    let level = |fmt: &str, text: &str| format!(r#"<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="420" w:hanging="420"/></w:pPr></w:lvl>"#);
    let mut nums = String::from(r#"<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#);
    for n in 0..lists {
        nums.push_str(&format!(r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>"#, n + 2));
    }
    format!(
        r#"{DECL}<w:numbering {NS}><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="singleLevel"/>{}</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="singleLevel"/>{}</w:abstractNum>{nums}</w:numbering>"#,
        level("bullet", "•"),
        level("decimal", "%1.")
    )
}

/// 报告 → .docx 的字节
pub fn write(doc: &Doc) -> Result<Vec<u8>, String> {
    let mut b = Builder { body: String::new(), links: Vec::new(), bookmark: 0, numbered: 0 };
    b.para(Some("Title"), "", &run(&doc.title, ""));
    b.para(Some("Subtitle"), "", &run(&doc.byline, ""));
    b.para(Some("Heading1"), "", &run("短答", ""));
    b.blocks(&doc.lead);
    b.para(Some("Heading1"), "", &run("全过程", ""));
    for s in &doc.sections {
        b.para(Some("Heading2"), "", &run(&s.title, ""));
        b.blocks(&s.blocks);
    }
    let sect = r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1200" w:bottom="1440" w:left="1200" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr>"#;
    let document = format!("{DECL}<w:document {NS}><w:body>{}{sect}</w:body></w:document>", b.body);
    let mut rels = String::from(r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#);
    for (id, url) in &b.links {
        rels.push_str(&format!(r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#, escape_attr(url)));
    }
    let mut pkg = Package::new();
    pkg.set(
        "[Content_Types].xml",
        format!(r#"{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#).into_bytes(),
    );
    pkg.set(
        "_rels/.rels",
        format!(r#"{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#).into_bytes(),
    );
    pkg.set(
        "docProps/core.xml",
        format!(r#"{DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Pith</dc:creator></cp:coreProperties>"#, escape_text(&doc.title)).into_bytes(),
    );
    pkg.set("word/document.xml", document.into_bytes());
    pkg.set("word/styles.xml", styles().into_bytes());
    pkg.set("word/numbering.xml", numbering(b.numbered).into_bytes());
    pkg.set("word/_rels/document.xml.rels", format!(r#"{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).into_bytes());
    pkg.write().map_err(|e| format!("写不出 Word 文档：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use research_engine::render::{Row, Section, text};

    fn sample() -> Doc {
        Doc {
            title: "示例市 <青年> 就业 & 工资".into(),
            byline: "第 1 次调查".into(),
            lead: vec![
                Block::Numbered(vec![vec![text("失业率为 "), Inline::Ref { text: "12.3%".into(), target: "F1".into() }, text("，"), Inline::Flag { text: "17.6%".into(), why: "对不上".into() }, Inline::Tag("互相矛盾".into())]]),
                Block::Sub("没有查到的".into()),
                Block::Bullets(vec![vec![text("灵活就业")]]),
            ],
            sections: vec![Section {
                title: "事实表".into(),
                blocks: vec![Block::Table {
                    head: vec!["编号".into(), "来源".into(), "摘录".into()],
                    rows: vec![Row { anchor: Some("F1".into()), cells: vec![vec![text("F1")], vec![Inline::Link { text: "公报".into(), url: "https://stats.example.invalid/a?x=1&y=2".into() }], vec![text("2024\t12.3")]] }],
                }],
            }],
        }
    }

    #[test]
    fn report_becomes_a_valid_word_document() {
        let bytes = write(&sample()).unwrap();
        let pkg = Package::read(&bytes).unwrap();
        for part in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml", "word/numbering.xml", "word/_rels/document.xml.rels", "docProps/core.xml"] {
            let xml = pkg.get_str(part).unwrap();
            roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{part} 不是合法的 XML：{e}"));
        }
        // 用文档引擎读回来：内容都在
        let text = docx_engine::outline::doc_text(&pkg).unwrap();
        for part in ["示例市 <青年> 就业 & 工资", "短答", "失业率为 12.3%，17.6%（对不上）〔互相矛盾〕", "没有查到的", "全过程", "事实表", "编号 | 来源 | 摘录", "F1 | 公报"] {
            assert!(text.contains(part), "缺「{part}」：\n{text}");
        }
        let doc = pkg.get_str("word/document.xml").unwrap();
        // 数字点回事实表那一行；外部链接走关系
        assert!(doc.contains(r#"<w:hyperlink w:anchor="r_F1""#) && doc.contains(r#"<w:bookmarkStart w:id="1" w:name="r_F1"/>"#));
        let rels = pkg.get_str("word/_rels/document.xml.rels").unwrap();
        assert!(rels.contains(r#"Target="https://stats.example.invalid/a?x=1&amp;y=2" TargetMode="External""#));
        assert!(doc.contains(r#"<w:hyperlink r:id="rId10""#));
        assert!(doc.contains("<w:tab/>"));
        // 每个编号列表从 1 起
        let numbering = pkg.get_str("word/numbering.xml").unwrap();
        assert!(numbering.contains(r#"<w:num w:numId="2"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/>"#));
    }

    #[test]
    fn a_whole_synthetic_report_exports() {
        let c = crate::db::tests::memory_db();
        let q = crate::research::store::question_create(&c, research_engine::fixture::QUESTION, "t").unwrap();
        let run = crate::research::store::tests::investigate(&c, q, "01RUNWORDAAAAAAAAAAAAAAAAA", 1, None);
        let doc = research_engine::render::document(&research_engine::report::build(&run, None));
        let pkg = Package::read(&write(&doc).unwrap()).unwrap();
        let text = docx_engine::outline::doc_text(&pkg).unwrap();
        assert!(text.contains("比 2023 年的 13.4% 低 1.1 个百分点") && text.contains("默认等级表") && text.contains("未核实：摘录在保存的正文里找不到"));
        // 每个被引用的事实都有书签
        let xml = pkg.get_str("word/document.xml").unwrap();
        for k in &run.conclusions {
            for f in &k.facts {
                assert!(xml.contains(&format!(r#"w:name="r_{f}""#)), "{f} 没有书签");
            }
        }
    }
}
