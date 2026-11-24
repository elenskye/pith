//! 真实论文才有的情况：粘贴来的正文样式、手动题注、靠直接格式排版的封面。只用合成文档。

use docx_engine::package::Package;
use docx_engine::plan::Plan;
use docx_engine::spec::{same_font, Spec};
use docx_engine::{fixture, format, verify};

const SPEC: &str = r#"{
  "name": "测试规范（仿某校要求）",
  "page": { "paper": "A4", "margins_cm": { "top": 3.0, "bottom": 2.5, "left": 2.5, "right": 2.5 }, "gutter_cm": 1.0, "header_cm": 1.6, "footer_cm": 1.5 },
  "body": { "font_latin": "Times New Roman", "font_east_asia": "宋体", "size_pt": 12, "align": "both", "line_spacing_exact_pt": 20, "space_before_pt": 0, "space_after_pt": 0, "first_line_indent_chars": 2 },
  "headings": [
    { "level": 1, "font_east_asia": "黑体", "size_pt": 16, "align": "center", "line_spacing_exact_pt": 20, "space_before_pt": 20, "space_after_pt": 20, "first_line_indent_chars": 0, "page_break_before": true }
  ],
  "caption": { "font_east_asia": "宋体", "size_pt": 10.5, "align": "center", "line_spacing_exact_pt": 20, "first_line_indent_chars": 0 },
  "bibliography": { "size_pt": 10.5, "line_spacing_exact_pt": 20 },
  "direct_formatting": { "strip_fonts": true, "strip_sizes": true, "strip_colors": true, "strip_paragraph_spacing": true }
}"#;

const PLAN: &str = r#"{
  "style_map": { "Normal (Web)": "Normal", "p1": "Normal" },
  "paragraph_rules": [ { "pattern": "^(图|表)\\s*\\d+(\\s|：)", "max_chars": 40, "style": "caption" } ],
  "protect_sections": [1]
}"#;

fn spec() -> Spec {
    Spec::from_json(SPEC).unwrap()
}

fn plan() -> Plan {
    Plan::from_json(PLAN).unwrap()
}

fn real_like() -> Package {
    Package::read(&fixture::build_real_like().write().unwrap()).unwrap()
}

fn run(plan: &Plan) -> (Package, Package, format::Report) {
    let original = real_like();
    let mut pkg = original.clone();
    let rep = format::apply_with_plan(&mut pkg, &spec(), plan).unwrap();
    let after = Package::read(&pkg.write().unwrap()).unwrap();
    (original, after, rep)
}

#[test]
fn real_like_document_is_formatted_with_plan() {
    let (before, after, rep) = run(&plan());
    let fidelity = verify::compare(&verify::snapshot(&before).unwrap(), &verify::snapshot(&after).unwrap());
    assert!(fidelity.is_empty(), "{fidelity:?}");
    let protected = verify::check_protected(&before, &after, &plan()).unwrap();
    assert!(protected.is_empty(), "{protected:?}");
    let checks = verify::check_spec_with_plan(&after, &spec(), &plan()).unwrap();
    let failed: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert!(checks.iter().any(|c| c.item == "正文：行距") && checks.iter().any(|c| c.item == "1 级标题：段前分页"));

    assert_eq!(rep.remapped.get("normal (web) → normal（样式映射）"), Some(&2), "{:?}", rep.remapped);
    assert_eq!(rep.remapped.get("p1 → normal（样式映射）"), Some(&1));
    assert_eq!(rep.remapped.get("normal (web) → caption（文字规则）"), Some(&1));
    assert_eq!(rep.protected_paragraphs, 4);
    assert!(rep.frozen_properties > 0);

    let doc = after.get_str("word/document.xml").unwrap();
    assert!(doc.contains(r#"<w:rFonts w:ascii="SimHei" w:eastAsia="SimHei" w:hAnsi="SimHei"/><w:b/>"#), "「关键词：」的黑体是有意的，应保留");
    assert!(!doc.contains(r#"w:eastAsia="SimSun""#), "与正文相同的直接字体应清除");
    assert!(doc.contains(r#"<w:pgMar w:top="1701" w:right="1417" w:bottom="1417" w:left="1417" w:header="907" w:footer="850" w:gutter="567"/>"#), "{doc}");
    // 封面的空标题 1 不应因「标题换页」而多出一页
    assert!(doc.contains(r#"<w:pageBreakBefore w:val="0"/>"#));
    let styles = after.get_str("word/styles.xml").unwrap();
    assert!(styles.contains(r#"w:line="400" w:lineRule="exact""#));
}

#[test]
fn caption_rule_skips_long_sentences() {
    let (_, after, _) = run(&plan());
    let doc = after.get_str("word/document.xml").unwrap();
    let caption = doc.split("<w:p>").find(|p| p.contains("图1   示例图")).unwrap();
    assert!(caption.contains(r#"<w:pStyle w:val="a5"/>"#), "{caption}");
    let sentence = doc.split("<w:p>").find(|p| p.contains("图 1 展示了")).unwrap();
    assert!(!sentence.contains("pStyle"), "{sentence}");
}

#[test]
fn cover_changes_without_protection_are_caught() {
    let mut no_protect = plan();
    no_protect.protect_sections.clear();
    let (before, after, rep) = run(&no_protect);
    assert_eq!(rep.protected_paragraphs, 0);
    let problems = verify::check_protected(&before, &after, &plan()).unwrap();
    assert!(problems.len() >= 3, "封面三段都应被发现：{problems:?}");
    assert!(problems.iter().all(|p| p.kind == "保护区域外观变化"));
}

#[test]
fn unformatted_document_fails_new_checks() {
    let doc = real_like();
    let checks = verify::check_spec_with_plan(&doc, &spec(), &plan()).unwrap();
    let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.item.as_str()).collect();
    for item in ["正文：行距", "1 级标题：段前分页", "第 2 节：页边距", "排版计划：需映射的样式已不再使用", "排版计划：按文字识别的段落已套用目标样式"] {
        assert!(failed.contains(&item), "{item} 应不通过：{failed:?}");
    }
}

#[test]
fn formatting_with_plan_is_idempotent() {
    let (_, once, _) = run(&plan());
    let mut twice = once.clone();
    let rep = format::apply_with_plan(&mut twice, &spec(), &plan()).unwrap();
    assert!(rep.parts_changed.is_empty(), "第二次又改了：{:?}", rep.parts_changed);
    assert_eq!(rep.frozen_properties, 0);
}

#[test]
fn bad_plans_are_rejected() {
    assert!(Plan::from_json(r#"{"paragraph_rules":[{"pattern":"(","style":"caption"}]}"#).is_err());
    assert!(Plan::from_json(r#"{"protect_sections":[0]}"#).is_err());
    assert!(Plan::from_json(r#"{"unknown":1}"#).is_err());
    let odd = Plan::from_json(r#"{"style_map":{"p1":"no such style"}}"#).unwrap();
    let mut pkg = real_like();
    assert!(format::apply_with_plan(&mut pkg, &spec(), &odd).is_err());
    assert!(Spec::from_json(&SPEC.replace(r#""line_spacing_exact_pt": 20, "space_before_pt": 0, "space_after_pt": 0, "first_line_indent_chars": 2"#, r#""line_spacing_exact_pt": 20, "line_spacing": 1.5"#)).is_err());
}

#[test]
fn chinese_font_names_have_aliases() {
    assert!(same_font("宋体", "SimSun") && same_font("黑体", "simhei") && same_font("Times New Roman", "times new roman"));
    assert!(!same_font("宋体", "黑体"));
}
