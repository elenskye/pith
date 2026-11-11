//! 结构摘要、计划预览、页面设置可缺省。只用合成文档。

use docx_engine::package::Package;
use docx_engine::plan::Plan;
use docx_engine::spec::Spec;
use docx_engine::{checks, fixture, format, outline, verify};

const PLAN: &str = r#"{
  "style_map": { "Normal (Web)": "Normal", "p1": "Normal" },
  "paragraph_rules": [ { "pattern": "^(图|表)\\s*\\d+(\\s|：)", "max_chars": 40, "style": "caption" } ],
  "protect_sections": [1]
}"#;

fn real_like() -> Package {
    Package::read(&fixture::build_real_like().write().unwrap()).unwrap()
}

#[test]
fn outline_of_real_like_document() {
    let o = outline::read(&real_like()).unwrap();
    assert_eq!(o.sections.len(), 2);
    assert_eq!(o.default_style.to_lowercase(), "normal");
    // 封面在第 1 节，分节符所在的段落也算第 1 节
    assert_eq!(o.paragraphs.iter().filter(|p| p.section == 1).count(), 4);
    assert!(o.sections[0].opening[0].starts_with("某某大学"));
    let web = o.styles.iter().find(|s| s.name == "Normal (Web)").expect("Normal (Web)");
    assert_eq!(web.count, 3);
    assert!(web.samples.iter().any(|s| s.starts_with("图1")));
    let h1 = o.paragraphs.iter().find(|p| p.text == "1 引言").unwrap();
    assert_eq!(h1.outline_level, Some(1));
    let caption = o.paragraphs.iter().find(|p| p.text.starts_with("图1")).unwrap();
    assert_eq!(caption.align.as_deref(), Some("center"));
    // Letter 纸
    assert_eq!((o.sections[1].width_twips, o.sections[1].height_twips), (12240, 15840));
}

#[test]
fn preview_matches_what_the_formatter_does() {
    let plan = Plan::from_json(PLAN).unwrap();
    let pkg = real_like();
    let hits = outline::preview(&outline::read(&pkg).unwrap(), &plan);
    let spec = Spec::from_json(r#"{ "name": "只有正文", "body": { "size_pt": 12 } }"#).unwrap();
    let mut out = pkg.clone();
    let rep = format::apply_with_plan(&mut out, &spec, &plan).unwrap();
    let got: Vec<(String, usize)> = hits.iter().map(|h| (h.label.clone(), h.count)).collect();
    let want: Vec<(String, usize)> = rep.remapped.iter().map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(got, want);
    assert!(hits.iter().any(|h| h.how == "rule" && h.to == "caption" && h.samples[0].starts_with("图1")));
}

#[test]
fn page_settings_left_out_are_left_alone() {
    let pkg = real_like();
    // 只规定上边距：纸张、其他边距都不动
    let spec = Spec::from_json(r#"{ "name": "只有上边距", "page": { "margins_cm": { "top": 3.0 } }, "body": { "size_pt": 12 } }"#).unwrap();
    let mut out = pkg.clone();
    format::apply(&mut out, &spec).unwrap();
    let after = Package::read(&out.write().unwrap()).unwrap();
    let xml = after.get_str("word/document.xml").unwrap();
    assert!(xml.contains(r#"w:w="12240" w:h="15840""#), "纸张不该变");
    assert!(xml.contains(r#"w:top="1701""#) && xml.contains(r#"w:right="1800""#), "只改上边距");
    assert!(verify::compare(&verify::snapshot(&pkg).unwrap(), &verify::snapshot(&after).unwrap()).is_empty());
    let checks = verify::check_spec(&after, &spec).unwrap();
    assert!(checks.iter().all(|c| c.ok), "{checks:?}");
    assert!(!checks.iter().any(|c| c.item.contains("纸张")));
    // 什么页面设置都没有：分节原样
    let none = Spec::from_json(r#"{ "name": "没有页面设置", "body": { "size_pt": 12 } }"#).unwrap();
    let mut out2 = pkg.clone();
    format::apply(&mut out2, &none).unwrap();
    assert!(out2.get_str("word/document.xml").unwrap().contains(r#"w:top="1440" w:right="1800""#));
}

#[test]
fn checks_on_real_like_document() {
    let o = outline::read(&real_like()).unwrap();
    let k = checks::run(&o, checks::Kind::KeywordsCount, Some(3.0), Some(5.0));
    assert_eq!((k.value.as_str(), k.ok), ("2 个", Some(false)));
    let r = checks::run(&o, checks::Kind::ReferencesCount, Some(1.0), None);
    assert_eq!((r.value.as_str(), r.ok), ("1 条", Some(true)));
    let c = checks::run(&o, checks::Kind::CaptionByChapter, None, None);
    assert_eq!(c.ok, Some(false));
}

#[test]
fn requirement_document_text() {
    let text = outline::doc_text(&Package::read(&fixture::build().write().unwrap()).unwrap()).unwrap();
    assert!(text.lines().count() > 10);
    assert!(text.contains(" | "), "表格按行、单元格用竖线隔开：{text}");
}
