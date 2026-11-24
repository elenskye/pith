//! 结构类排版（W4）：每一类都有编辑逻辑和独立核对。只用合成文档。

use docx_engine::package::Package;
use docx_engine::plan::Plan;
use docx_engine::spec::Spec;
use docx_engine::{fixture, format, outline, verify};

const SPEC: &str = r#"{
  "name": "结构测试",
  "page": { "paper": "A4", "margins_cm": { "top": 3.0, "bottom": 2.5, "left": 2.5, "right": 2.5 }, "gutter_cm": 1.0 },
  "body": { "font_east_asia": "宋体", "size_pt": 12, "line_spacing_exact_pt": 20, "first_line_indent_chars": 2 },
  "headings": [
    { "level": 1, "size_pt": 16, "align": "center", "space_before_pt": 20, "space_after_pt": 20, "first_line_indent_chars": 0, "page_break_before": true },
    { "level": 2, "size_pt": 15, "align": "left", "space_before_pt": 20, "space_after_pt": 0, "first_line_indent_chars": 0 }
  ],
  "header_footer": { "font_east_asia": "宋体", "size_pt": 9 },
  "table_text": { "size_pt": 10.5, "first_line_indent_chars": 0, "align": "center" },
  "structure": {
    "header": {
      "single": { "left": "某某大学本科毕业论文", "right": "{章名}" },
      "odd": { "center": "{章名}" },
      "even": { "center": "某某大学本科毕业论文" },
      "rule": true
    },
    "page_numbers": { "align": "center", "front_format": "upper_roman", "body_format": "decimal" },
    "body_on_odd_page": true,
    "three_line_tables": true,
    "footnote_numbering": { "format": "circled", "restart": "each_page" },
    "tight_headings": [2]
  }
}"#;

fn thesis() -> Package {
    Package::read(&fixture::build_thesis_like().write().unwrap()).unwrap()
}

fn anchor(o: &outline::Outline, text: &str) -> String {
    let p = o.paragraphs.iter().find(|p| p.text == text).unwrap_or_else(|| panic!("没有段落「{text}」"));
    format!(r#"{{ "paragraph": {}, "text": "{}" }}"#, p.index, text)
}

fn plan(duplex: bool, extra: &str) -> Plan {
    let o = outline::read(&thesis()).unwrap();
    Plan::from_json(&format!(
        r#"{{ "layout": {{ "front_start": {}, "body_start": {}, "back_start": {}, "duplex": {duplex}{extra} }} }}"#,
        anchor(&o, "摘  要"),
        anchor(&o, "1 引言"),
        anchor(&o, "原创性声明")
    ))
    .unwrap()
}

fn run(spec: &Spec, plan: &Plan) -> (Package, Package, format::Report) {
    let before = thesis();
    let mut pkg = before.clone();
    let rep = format::apply_with_plan(&mut pkg, spec, plan).unwrap();
    let after = Package::read(&pkg.write().unwrap()).unwrap();
    (before, after, rep)
}

fn assert_clean(before: &Package, after: &Package, spec: &Spec, plan: &Plan) {
    let (problems, checks) = verify::verify_all(before, after, spec, plan).unwrap();
    assert!(problems.is_empty(), "保真：{problems:#?}");
    let bad: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
    assert!(bad.is_empty(), "规范：{bad:#?}");
}

#[test]
fn single_sided_thesis_gets_all_six_kinds() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = plan(false, "");
    let (before, after, rep) = run(&spec, &plan);
    assert_clean(&before, &after, &spec, &plan);
    let st = rep.structure.unwrap();
    assert_eq!(st.breaks_inserted.len(), 3, "{:?}", st.breaks_inserted);
    assert_eq!(st.zones, [("封面".to_string(), 1), ("前置部分".to_string(), 1), ("正文".to_string(), 1), ("正文之后".to_string(), 1)]);
    assert_eq!((st.three_line.as_slice(), st.tables_skipped.len()), (&[1usize][..], 1));
    assert!(st.tables_skipped[0].1.contains("没有边框"));
    // 「1.1」紧跟「1 引言」、「2.1」紧跟「2 方法」；「1.2」前面是正文，不算
    assert_eq!(st.tight_headings, 2);
    assert!(st.body_on_odd_page && st.footnote_numbering && !st.duplex);

    let doc = after.get_str("word/document.xml").unwrap();
    assert_eq!(doc.matches("<w:sectPr").count(), 4);
    // 正文一个字没变、段落没多没少
    let text = |p: &Package| verify::story_of(&p.get_str("word/document.xml").unwrap()).unwrap().text;
    assert_eq!(text(&before), text(&after));
    // 页码：前置部分大写罗马数字从 1 起，正文阿拉伯数字从 1 起且从奇数页开始
    assert!(doc.contains(r#"<w:pgNumType w:fmt="upperRoman" w:start="1"/>"#));
    assert!(doc.contains(r#"<w:type w:val="oddPage"/>"#) && doc.contains(r#"<w:pgNumType w:fmt="decimal" w:start="1"/>"#));
    // 封面那一节：原来的页眉页脚不带过去
    let cover = &doc[..doc.find("摘  要").unwrap()];
    assert!(cover.contains("<w:sectPr") && !cover.contains("headerReference") && !cover.contains("footerReference"));
    // 单面：左校名、右章名，用制表位；章名是不随界面语言变的 STYLEREF 1
    let header = after.get_str("word/header_pith1.xml").unwrap();
    assert!(header.contains("某某大学本科毕业论文") && header.contains(" STYLEREF 1 ") && header.contains(r#"w:val="right""#) && header.contains("<w:pBdr>"));
    // 版心宽 = 21 - 2.5 - 2.5 - 1 = 15 cm：11906 - 1417 - 1417 - 567 = 8505 twips
    assert!(header.contains(r#"w:pos="8505""#), "{header}");
    assert!(!after.get_str("word/settings.xml").unwrap().contains("evenAndOddHeaders"));
    let settings = after.get_str("word/settings.xml").unwrap();
    assert!(settings.contains(r#"<w:numFmt w:val="decimalEnclosedCircleChinese"/><w:numRestart w:val="eachPage"/>"#), "{settings}");
    // 原来的页眉页脚部件还在（没人引用了，文字没变；Word 下次存盘会清掉）
    let old = |p: &Package| verify::story_of(&p.get_str("word/header1.xml").unwrap()).unwrap().text;
    assert_eq!(old(&before), old(&after));
    assert!(!doc.contains("rIdHeader1") && !doc.contains("rIdFooter1"));
    // 三线表：表格只有顶线底线，栏目线在表头行下面
    let tbl = &doc[doc.find("<w:tbl>").unwrap()..doc.find("</w:tbl>").unwrap()];
    assert!(tbl.contains(r#"<w:top w:val="single" w:sz="12""#) && tbl.contains(r#"<w:insideV w:val="none""#));
    assert_eq!(tbl.matches(r#"<w:bottom w:val="single" w:sz="6""#).count(), 3, "栏目线：表头行的 3 个单元格");
    // 报告里写了结构类的改动
    let md = docx_engine::report::markdown(Some(&format::Report { structure: Some(st), ..Default::default() }), &[], &[]);
    for part in ["## 结构", "新加的分节符", "排成三线表：第 1 个表格", "第 2 个表格没有动", "正文从奇数页开始"] {
        assert!(md.contains(part), "{md}");
    }
}

#[test]
fn double_sided_uses_odd_and_even_headers() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = plan(true, "");
    let (before, after, _) = run(&spec, &plan);
    assert_clean(&before, &after, &spec, &plan);
    assert!(after.get_str("word/settings.xml").unwrap().contains("<w:evenAndOddHeaders/>"));
    let doc = after.get_str("word/document.xml").unwrap();
    assert!(doc.contains(r#"<w:headerReference w:type="even""#) && doc.contains(r#"<w:footerReference w:type="even""#));
    let headers: Vec<String> = after.names().into_iter().filter(|n| n.starts_with("word/header_pith")).map(|n| after.get_str(n).unwrap()).collect();
    // 奇数页居中章名、偶数页居中校名、正文之后的空页眉
    assert_eq!(headers.len(), 3);
    assert!(headers.iter().any(|h| h.contains("STYLEREF 1") && h.contains(r#"<w:jc w:val="center"/>"#) && !h.contains("某某大学")));
    assert!(headers.iter().any(|h| h.contains("某某大学本科毕业论文") && !h.contains("STYLEREF")));
}

#[test]
fn running_twice_gives_the_same_result() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = plan(false, "");
    let (_, once, _) = run(&spec, &plan);
    let mut again = once.clone();
    let rep = format::apply_with_plan(&mut again, &spec, &plan).unwrap();
    let again = Package::read(&again.write().unwrap()).unwrap();
    assert!(rep.structure.unwrap().breaks_inserted.is_empty(), "分节符已经有了，不再加");
    assert_clean(&once, &again, &spec, &plan);
    assert_eq!(once.get_str("word/document.xml").unwrap().matches("<w:sectPr").count(), again.get_str("word/document.xml").unwrap().matches("<w:sectPr").count());
}

#[test]
fn tables_can_be_skipped_and_protected() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = plan(false, r#", "skip_tables": [1]"#);
    let (before, after, rep) = run(&spec, &plan);
    assert_clean(&before, &after, &spec, &plan);
    let st = rep.structure.unwrap();
    assert!(st.three_line.is_empty() && st.tables_skipped.iter().any(|(n, why)| *n == 1 && why.contains("计划")));
    assert!(!after.get_str("word/document.xml").unwrap().contains("<w:tblBorders>"));
}

#[test]
fn without_a_body_start_zoned_kinds_are_reported_not_guessed() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = Plan::default();
    let (before, after, rep) = run(&spec, &plan);
    let st = rep.structure.unwrap();
    assert!(st.notes.iter().any(|n| n.contains("没有指出正文")));
    assert_eq!(st.header_sections, 0);
    // 不需要分区的几类照做
    assert_eq!((st.three_line.len(), st.tight_headings, st.footnote_numbering), (1, 2, true));
    let (problems, checks) = verify::verify_all(&before, &after, &spec, &plan).unwrap();
    assert!(problems.is_empty(), "{problems:#?}");
    let bad: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.item.as_str()).collect();
    assert_eq!(bad, ["分区"], "页眉页码没处理要如实标出来");
}

#[test]
fn stale_or_impossible_anchors_are_refused() {
    let spec = Spec::from_json(SPEC).unwrap();
    let o = outline::read(&thesis()).unwrap();
    let body = o.paragraphs.iter().find(|p| p.text == "1 引言").unwrap().index;
    // 文字对不上（文档改过）
    let stale = Plan::from_json(&format!(r#"{{ "layout": {{ "body_start": {{ "paragraph": {body}, "text": "第一章 绪论" }} }} }}"#)).unwrap();
    let e = format::apply_with_plan(&mut thesis(), &spec, &stale).unwrap_err().to_string();
    assert!(e.contains("不是计划里写的"), "{e}");
    // 表格里的段落不能当分界
    let cell = o.paragraphs.iter().find(|p| p.in_table).unwrap();
    let inside = Plan::from_json(&format!(r#"{{ "layout": {{ "body_start": {{ "paragraph": {}, "text": "{}" }} }} }}"#, cell.index, cell.text)).unwrap();
    assert!(format::apply_with_plan(&mut thesis(), &spec, &inside).unwrap_err().to_string().contains("表格"));
    // 顺序不对
    assert!(Plan::from_json(r#"{ "layout": { "front_start": { "paragraph": 9, "text": "a" }, "body_start": { "paragraph": 3, "text": "b" } } }"#).is_err());
}

#[test]
fn verifier_catches_wrong_structure() {
    let spec = Spec::from_json(SPEC).unwrap();
    let plan = plan(false, "");
    let (before, after, _) = run(&spec, &plan);
    let failing = |pkg: &Package| -> Vec<String> { verify::verify_all(&before, pkg, &spec, &plan).unwrap().1.into_iter().filter(|c| !c.ok).map(|c| c.item).collect() };
    let tamper = |part: &str, from: &str, to: &str| {
        let mut p = after.clone();
        let x = p.get_str(part).unwrap();
        assert!(x.contains(from), "{part} 里没有 {from}");
        p.set(part, x.replacen(from, to, 1).into_bytes());
        Package::read(&p.write().unwrap()).unwrap()
    };
    // 页眉文字被改
    assert!(failing(&tamper("word/header_pith1.xml", "某某大学本科毕业论文", "别的学校")).iter().any(|i| i.contains("页眉")));
    // 页码形式不对
    assert!(failing(&tamper("word/document.xml", r#"w:fmt="upperRoman""#, r#"w:fmt="lowerRoman""#)).iter().any(|i| i == "前置部分：页码"));
    // 不是奇数页起
    assert_eq!(failing(&tamper("word/document.xml", r#"<w:type w:val="oddPage"/>"#, "")), ["正文从奇数页开始"]);
    // 表里多了一条竖线
    assert!(failing(&tamper("word/document.xml", r#"<w:left w:val="nil"/>"#, r#"<w:left w:val="single" w:sz="4"/>"#)).iter().any(|i| i.contains("三线表")));
    // 脚注编号
    assert_eq!(failing(&tamper("word/settings.xml", "decimalEnclosedCircleChinese", "decimal")), ["脚注编号"]);
    // 紧跟的标题段前又有了
    assert!(failing(&tamper("word/document.xml", r#"w:before="0""#, r#"w:before="240""#)).iter().any(|i| i.contains("紧跟")));
    // 子元素顺序排错（Word 会报文件损坏）：把页码设置挪到分节属性最前面
    let doc = after.get_str("word/document.xml").unwrap();
    let pg = r#"<w:pgNumType w:fmt="decimal" w:start="1"/>"#;
    let moved = doc.replacen(pg, "", 1).replacen("<w:sectPr>", &format!("<w:sectPr>{pg}"), 1);
    assert_ne!(moved, doc);
    let mut wrong = after.clone();
    wrong.set("word/document.xml", moved.into_bytes());
    let got = verify::verify_all(&before, &Package::read(&wrong.write().unwrap()).unwrap(), &spec, &plan).unwrap().0;
    assert!(got.iter().any(|p| p.kind == "元素顺序不对"), "{got:#?}");
    // 保真：新页眉以外的东西变了照样拦
    let problems = |pkg: &Package| verify::verify_all(&before, pkg, &spec, &plan).unwrap().0;
    assert!(problems(&tamper("word/document.xml", "正文第二段。", "正文第二段！")).iter().any(|p| p.kind == "正文文字变化"));
    let mut extra = after.clone();
    extra.set("word/header_other.xml", b"<x/>".to_vec());
    assert!(problems(&Package::read(&extra.write().unwrap()).unwrap()).iter().any(|p| p.kind == "新增部件"));
    let mut rels = after.clone();
    let r = rels.get_str("word/_rels/document.xml.rels").unwrap().replacen("</Relationships>", r#"<Relationship Id="rIdX" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org" TargetMode="External"/></Relationships>"#, 1);
    rels.set("word/_rels/document.xml.rels", r.into_bytes());
    assert!(problems(&Package::read(&rels.write().unwrap()).unwrap()).iter().any(|p| p.kind == "关系变化"));
}

#[test]
fn protected_cover_stays_protected_when_sections_are_added_before_it() {
    // 受保护的是原来的第 1 节（整篇只有一节时就是全文）：加了分节符以后，仍按段落次序核对同一批段落
    let spec = Spec::from_json(SPEC).unwrap();
    let mut p = plan(false, "");
    p.protect_sections = vec![1];
    p.compile().unwrap();
    let (before, after, rep) = run(&spec, &p);
    let (problems, _) = verify::verify_all(&before, &after, &spec, &p).unwrap();
    assert!(problems.is_empty(), "{problems:#?}");
    // 受保护分节里的表格、标题不动
    let st = rep.structure.unwrap();
    assert!(st.three_line.is_empty() && st.tight_headings == 0);
}

#[test]
fn outline_lists_tables() {
    let o = outline::read(&thesis()).unwrap();
    assert_eq!(o.table_list.len(), 2);
    let (data, layout) = (&o.table_list[0], &o.table_list[1]);
    assert_eq!((data.rows, data.cols, data.bordered, data.nested), (3, 3, true, false));
    assert!(data.first_row.starts_with("变量 | 均值"));
    assert!(!layout.bordered);
    assert_eq!(o.paragraphs[data.paragraph - 1].text, "变量");
}

#[test]
fn blank_paragraphs_after_an_existing_break_join_the_next_zone() {
    // 真实论文里常见：封面后的分节符和「摘要」之间隔着一个空段落。不该为它多切一节（多一张白页）
    let spec = Spec::from_json(SPEC).unwrap();
    let mut before = thesis();
    let doc = before.get_str("word/document.xml").unwrap();
    let at = doc.find("<w:p><w:pPr><w:pStyle w:val=\"1\"/></w:pPr><w:r><w:t xml:space=\"preserve\">摘  要").unwrap();
    let brk = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1418" w:bottom="1418" w:left="1418" w:header="907" w:footer="850" w:gutter="567"/></w:sectPr></w:pPr></w:p><w:p/>"#;
    before.set("word/document.xml", format!("{}{brk}{}", &doc[..at], &doc[at..]).into_bytes());
    let before = Package::read(&before.write().unwrap()).unwrap();
    let o = outline::read(&before).unwrap();
    let plan = Plan::from_json(&format!(r#"{{ "layout": {{ "front_start": {}, "body_start": {} }} }}"#, anchor(&o, "摘  要"), anchor(&o, "1 引言"))).unwrap();
    let mut pkg = before.clone();
    let rep = format::apply_with_plan(&mut pkg, &spec, &plan).unwrap();
    let after = Package::read(&pkg.write().unwrap()).unwrap();
    assert_clean(&before, &after, &spec, &plan);
    let st = rep.structure.unwrap();
    assert_eq!(st.breaks_inserted, ["在「1 引言」之前"]);
    assert_eq!(st.zones, [("封面".to_string(), 1), ("前置部分".to_string(), 1), ("正文".to_string(), 1)]);
}
