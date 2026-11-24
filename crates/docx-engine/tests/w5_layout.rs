//! W5 在 Word 里看到的问题（只用合成文档）：
//! - 段前分页叠在已有的分页上（分节符后隔着空段落、上一段以分页符结尾）多出空白页；一个标题分两段写被拆到两页。
//! - 表格里用参考文献样式的段落继承了缩进，窄单元格里的字被挤断。
//! - 正文改成按字符缩进后，参考文献、编号列表自带的悬挂缩进被继承来的字符缩进盖掉；参考文献跟着正文两端对齐。

use docx_engine::package::Package;
use docx_engine::plan::Plan;
use docx_engine::spec::Spec;
use docx_engine::{fixture, format, verify};

const SPEC: &str = r#"{
  "name": "段前分页测试",
  "body": { "font_east_asia": "宋体", "size_pt": 12, "align": "both", "first_line_indent_chars": 2 },
  "headings": [ { "level": 1, "size_pt": 16, "align": "center", "first_line_indent_chars": 0, "page_break_before": true } ],
  "table_text": { "size_pt": 10.5, "first_line_indent_chars": 0, "align": "center" },
  "bibliography": { "size_pt": 10.5 }
}"#;

const ITEM: &str = "段前分页：不多出空白页、不拆开标题";

fn para(style: Option<&str>, ppr: &str, text: &str, page_break_after: bool) -> String {
    let style = style.map(|s| format!(r#"<w:pStyle w:val="{s}"/>"#)).unwrap_or_default();
    let ppr = if style.is_empty() && ppr.is_empty() { String::new() } else { format!("<w:pPr>{style}{ppr}</w:pPr>") };
    let run = if text.is_empty() { String::new() } else { format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#) };
    let brk = if page_break_after { r#"<w:r><w:br w:type="page"/></w:r>"# } else { "" };
    format!("<w:p>{ppr}{run}{brk}</w:p>")
}

const SECT: &str = r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1418" w:bottom="1418" w:left="1418" w:header="907" w:footer="850" w:gutter="0"/></w:sectPr>"#;

fn bib_cell(text: &str) -> String {
    format!(r#"<w:tc><w:tcPr><w:tcW w:w="800" w:type="dxa"/></w:tcPr>{}</w:tc>"#, para(Some("Bibliography"), "", text, false))
}

/// 仿真实论文：封面一节；摘要、目录、引言前都已经用分节符或分页符另起一页；一个标题分两段写；
/// 数据表格的单元格用参考文献样式；致谢是新一节的第一段。`protect_cover` 时封面一节里也有叠在一起的分页。
fn doc(protect_cover: bool) -> Package {
    use fixture::sid::H1;
    let mut pkg = fixture::build_thesis_like();
    let table = format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="6" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="6" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="800"/><w:gridCol w:w="800"/></w:tblGrid><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>"#,
        bib_cell("模型"),
        bib_cell("RMSE"),
        bib_cell("线性回归"),
        bib_cell("0.789")
    );
    let cover_extra = if protect_cover { [para(None, "", "封面附页", true), para(Some(H1), r#"<w:pageBreakBefore/>"#, "封面标题", false)].concat() } else { String::new() };
    let body = [
        para(None, "", "某某大学本科毕业论文", false),
        cover_extra,
        para(None, SECT, "", false),
        para(None, "", "", false),
        para(Some(H1), "", "摘  要", false),
        para(None, "", "摘要正文。", true),
        para(Some(H1), "", "目  录", false),
        para(None, "", "（目录略）", true),
        para(None, "", "", false),
        para(Some(H1), "", "1 引言", false),
        para(None, "", "正文第一段。", false),
        para(Some(H1), "", "2 很长的一章标题", false),
        para(Some(H1), "", "分两段写的后半", false),
        para(None, "", "正文第二段。", false),
        para(None, r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr><w:ind w:left="840" w:hanging="420"/>"#, "列表第一项", false),
        para(Some("Bibliography"), r#"<w:ind w:left="505" w:hanging="505"/>"#, "[1] 作者. 题名[J]. 刊名, 2020.", false),
        table,
        para(None, SECT, "", false),
        para(Some(H1), "", "致  谢", false),
        para(None, "", "感谢所有人。", false),
        SECT.to_string(),
    ]
    .concat();
    let xml = pkg.get_str("word/document.xml").unwrap();
    let (head, _) = xml.split_once("<w:body>").unwrap();
    pkg.set("word/document.xml", format!("{head}<w:body>{body}</w:body></w:document>").into_bytes());
    // 正文样式原来不两端对齐（同真实论文），参考文献原来左对齐
    let styles = pkg.get_str("word/styles.xml").unwrap().replacen(r#"<w:widowControl w:val="0"/><w:jc w:val="both"/>"#, r#"<w:widowControl w:val="0"/>"#, 1).replace(
        "</w:styles>",
        &format!(
            r#"<w:style w:type="paragraph" w:styleId="Bibliography"><w:name w:val="Bibliography"/><w:basedOn w:val="{}"/><w:pPr><w:ind w:left="504" w:hanging="504"/></w:pPr></w:style></w:styles>"#,
            fixture::sid::NORMAL
        ),
    );
    pkg.set("word/styles.xml", styles.into_bytes());
    Package::read(&pkg.write().unwrap()).unwrap()
}

fn run(before: &Package, plan: &Plan) -> (Package, format::Report) {
    let spec = Spec::from_json(SPEC).unwrap();
    let mut pkg = before.clone();
    let rep = format::apply_with_plan(&mut pkg, &spec, plan).unwrap();
    (Package::read(&pkg.write().unwrap()).unwrap(), rep)
}

/// 含这段文字的那个段落（从 `<w:p>` 到 `</w:p>`）
fn paragraph_with(pkg: &Package, text: &str) -> String {
    let xml = pkg.get_str("word/document.xml").unwrap();
    let at = xml.find(&format!(">{text}</w:t>")).unwrap_or_else(|| panic!("没有「{text}」"));
    let start = xml[..at].rfind("<w:p>").or_else(|| xml[..at].rfind("<w:p ")).unwrap();
    let end = at + xml[at..].find("</w:p>").unwrap();
    xml[start..end].to_string()
}

fn no_break(pkg: &Package, text: &str) -> bool {
    paragraph_with(pkg, text).contains(r#"<w:pageBreakBefore w:val="0"/>"#)
}

#[test]
fn page_breaks_are_not_doubled_and_titles_stay_together() {
    let before = doc(false);
    let (after, rep) = run(&before, &Plan::default());
    // 分节符后隔着空段落、上一段以分页符结尾（有没有空段落都算）、紧跟另一段段前分页的：不再分页
    for t in ["摘  要", "目  录", "1 引言", "分两段写的后半"] {
        assert!(no_break(&after, t), "「{t}」应该写段前不分页");
    }
    // 前面是正文的、正好是新一节第一段的（Word 自己不会再分页）：照规范分页
    for t in ["2 很长的一章标题", "致  谢"] {
        assert!(!no_break(&after, t), "「{t}」不该改");
    }
    assert_eq!(rep.page_breaks_not_doubled, 4);
    let spec = Spec::from_json(SPEC).unwrap();
    let (problems, checks) = verify::verify_all(&before, &after, &spec, &Plan::default()).unwrap();
    assert!(problems.is_empty(), "{problems:#?}");
    assert!(checks.iter().all(|c| c.ok), "{:#?}", checks.iter().filter(|c| !c.ok).collect::<Vec<_>>());
    assert!(checks.iter().any(|c| c.item == ITEM && c.ok));
    let md = docx_engine::report::markdown(Some(&rep), &problems, &checks);
    assert!(md.contains("不再加段前分页：4 处"), "{md}");
}

#[test]
fn checker_catches_doubled_page_breaks() {
    let before = doc(false);
    let (after, _) = run(&before, &Plan::default());
    // 改坏：把「段前不分页」全部去掉，规范要求的段前分页又叠在已有的分页上
    let mut broken = after.clone();
    let xml = broken.get_str("word/document.xml").unwrap().replace(r#"<w:pageBreakBefore w:val="0"/>"#, "");
    broken.set("word/document.xml", xml.into_bytes());
    let broken = Package::read(&broken.write().unwrap()).unwrap();
    let checks = verify::check_page_breaks(&before, &broken).unwrap();
    let c = checks.iter().find(|c| c.item == ITEM).unwrap();
    assert!(!c.ok);
    assert!(c.detail.starts_with("4 处") && c.detail.contains("「摘  要」") && c.detail.contains("「分两段写的后半」"), "{}", c.detail);
}

#[test]
fn protected_section_keeps_its_own_page_breaks() {
    // 封面一节受保护：那里原来就叠在一起的分页不动，也不算排版带来的问题
    let before = doc(true);
    let plan = Plan::from_json(r#"{ "protect_sections": [1] }"#).unwrap();
    let (after, rep) = run(&before, &plan);
    assert!(!no_break(&after, "封面标题"));
    assert!(no_break(&after, "摘  要"));
    assert_eq!(rep.page_breaks_not_doubled, 4);
    let checks = verify::check_page_breaks(&before, &after).unwrap();
    assert!(checks.iter().all(|c| c.ok), "{checks:#?}");
}

#[test]
fn bibliography_style_in_table_cells_becomes_table_text() {
    let before = doc(false);
    let (after, rep) = run(&before, &Plan::default());
    for t in ["模型", "RMSE", "线性回归", "0.789"] {
        let p = paragraph_with(&after, t);
        assert!(p.contains(r#"<w:pStyle w:val="RLTableText"/>"#), "「{t}」：{p}");
        assert!(!p.contains("<w:ind "), "「{t}」不该再有缩进：{p}");
    }
    assert_eq!(rep.table_bib_paragraphs, 4);
    let md = docx_engine::report::markdown(Some(&rep), &[], &[]);
    assert!(md.contains("表格里原来用参考文献样式的段落：4 个"), "{md}");
}

const INDENT_ITEM: &str = "自带缩进：没有被继承来的字符缩进盖掉";

fn styles_xml(pkg: &Package) -> String {
    pkg.get_str("word/styles.xml").unwrap()
}

fn style_block(pkg: &Package, id: &str) -> String {
    let xml = styles_xml(pkg);
    let start = xml.find(&format!(r#"w:styleId="{id}""#)).unwrap();
    let end = start + xml[start..].find("</w:style>").unwrap();
    xml[start..end].to_string()
}

#[test]
fn own_hanging_indents_survive_character_indent_of_body() {
    let before = doc(false);
    let (after, rep) = run(&before, &Plan::default());
    // 正文改成按字符缩进（2 字符）、两端对齐
    assert!(style_block(&after, fixture::sid::NORMAL).contains(r#"w:firstLineChars="200""#));
    // 参考文献样式：自己的悬挂缩进写明 0 字符；规范没写对齐，保持原来的左对齐
    let bib = style_block(&after, "Bibliography");
    assert!(bib.contains(r#"w:firstLineChars="0""#) && bib.contains(r#"w:hanging="504""#), "{bib}");
    assert!(bib.contains(r#"<w:jc w:val="left"/>"#), "{bib}");
    assert_eq!(rep.own_indent_styles, ["Bibliography"]);
    assert_eq!(rep.bibliography_align_kept.as_deref(), Some("left"));
    // 编号列表段落上直接写的悬挂缩进也写明 0 字符；参考文献段落跟着样式，不用再写
    assert!(paragraph_with(&after, "列表第一项").contains(r#"w:firstLineChars="0""#));
    assert!(!paragraph_with(&after, "[1] 作者. 题名[J]. 刊名, 2020.").contains("Chars"));
    assert_eq!(rep.own_indent_paragraphs, 1);
    let spec = Spec::from_json(SPEC).unwrap();
    let (problems, checks) = verify::verify_all(&before, &after, &spec, &Plan::default()).unwrap();
    assert!(problems.is_empty(), "{problems:#?}");
    assert!(checks.iter().any(|c| c.item == INDENT_ITEM && c.ok), "{checks:#?}");
    let md = docx_engine::report::markdown(Some(&rep), &problems, &checks);
    assert!(md.contains("自带缩进写明 0 字符缩进：样式 Bibliography；段落 1 个") && md.contains("参考文献保持原来的对齐（左对齐）"), "{md}");
}

#[test]
fn checker_catches_lost_hanging_indents() {
    let before = doc(false);
    let (after, _) = run(&before, &Plan::default());
    // 改坏：去掉参考文献样式和列表段落上写明的 0 字符，继承来的字符缩进又盖掉了它们自己的悬挂缩进
    let mut broken = after.clone();
    let cut = |xml: String, anchor: &str| {
        let at = xml.find(anchor).unwrap();
        let end = at + xml[at..].find(r#" w:firstLineChars="0""#).unwrap();
        format!("{}{}", &xml[..end], &xml[end + r#" w:firstLineChars="0""#.len()..])
    };
    let styles = cut(broken.get_str("word/styles.xml").unwrap(), r#"w:styleId="Bibliography""#);
    broken.set("word/styles.xml", styles.into_bytes());
    let document = cut(broken.get_str("word/document.xml").unwrap(), r#"<w:numId w:val="1"/>"#);
    broken.set("word/document.xml", document.into_bytes());
    let broken = Package::read(&broken.write().unwrap()).unwrap();
    let checks = verify::check_own_indents(&before, &broken).unwrap();
    let c = checks.iter().find(|c| c.item == INDENT_ITEM).unwrap();
    assert!(!c.ok);
    // 列表段落、参考文献段落（跟着样式又被盖掉）两个
    assert!(c.detail.ends_with("样式「bibliography」、段落 2 个"), "{}", c.detail);
}
