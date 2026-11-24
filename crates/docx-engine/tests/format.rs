//! 排版器与校验器的测试：只用代码生成的合成文档。

use docx_engine::package::Package;
use docx_engine::spec::Spec;
use docx_engine::{fixture, format, verify};

fn spec() -> Spec {
    Spec::from_json(include_str!("../spec/thesis-sample.json")).unwrap()
}

/// 经过一次 zip 往返，模拟从磁盘读入的文档（未改部件走原字节复制）
fn fixture_pkg() -> Package {
    Package::read(&fixture::build().write().unwrap()).unwrap()
}

fn formatted() -> (Package, Package, format::Report) {
    let original = fixture_pkg();
    let mut pkg = original.clone();
    let rep = format::apply(&mut pkg, &spec()).unwrap();
    let after = Package::read(&pkg.write().unwrap()).unwrap();
    (original, after, rep)
}

fn problems(a: &Package, b: &Package) -> Vec<verify::Problem> {
    verify::compare(&verify::snapshot(a).unwrap(), &verify::snapshot(b).unwrap())
}

#[test]
fn fixture_covers_the_structures() {
    let snap = verify::snapshot(&fixture_pkg()).unwrap();
    let doc = &snap.stories["word/document.xml"];
    for (k, v) in [("表格", 1), ("单元格", 8), ("公式", 2), ("独立公式段", 1), ("图片引用", 1), ("脚注引用", 2), ("尾注引用", 1), ("批注引用", 1), ("修订：插入", 1), ("修订：删除", 1), ("分节", 2), ("内容控件", 3), ("列表编号", 2), ("简单域", 2), ("复杂域", 5)] {
        assert_eq!(doc.counts.get(k).copied().unwrap_or(0), v, "{k}");
    }
    assert!(doc.text.contains("政治效能感（political efficacy）"));
    assert_eq!(doc.deleted, "，且样本较小");
    assert!(doc.field_codes.iter().any(|c| c.starts_with("TOC")));
    assert!(snap.stories.contains_key("word/footnotes.xml") && snap.stories.contains_key("word/header1.xml"));
    assert!(problems(&fixture_pkg(), &fixture_pkg()).is_empty());
}

#[test]
fn formatting_keeps_content_and_applies_spec() {
    let (original, after, rep) = formatted();
    let p = problems(&original, &after);
    assert!(p.is_empty(), "{p:?}");
    let checks = verify::check_spec(&after, &spec()).unwrap();
    let failed: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert!(checks.len() >= 40);
    assert!(rep.runs_cleaned > 10 && rep.paragraphs_cleaned > 5);
    assert_eq!(rep.table_paragraphs, 8);
    assert_eq!(rep.figure_paragraphs, 2);
    assert_eq!(rep.code_paragraphs_kept, 3);
    assert_eq!(rep.sections, 2);
    for part in &rep.parts_changed {
        assert!(verify::may_change(part), "不该改的部件被改了：{part}");
    }
}

#[test]
fn spec_check_fails_on_unformatted_document() {
    let checks = verify::check_spec(&fixture_pkg(), &spec()).unwrap();
    let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.item.as_str()).collect();
    assert!(failed.contains(&"正文：中文字体"), "{failed:?}");
    assert!(failed.contains(&"第 1 节：页边距"));
    assert!(failed.contains(&"正文：冲突的直接字体 / 字号已清除"));
}

#[test]
fn untouched_parts_are_byte_identical() {
    let (original, after, _) = formatted();
    for part in ["word/media/image1.png", "customXml/item1.xml", "word/comments.xml", "word/_rels/document.xml.rels", "[Content_Types].xml", "word/numbering.xml", "word/settings.xml"] {
        assert_eq!(original.get(part), after.get(part), "{part}");
    }
}

#[test]
fn formatting_is_idempotent() {
    let (_, once, _) = formatted();
    let mut twice = once.clone();
    let rep = format::apply(&mut twice, &spec()).unwrap();
    assert!(rep.parts_changed.is_empty(), "第二次排版又改了：{:?}", rep.parts_changed);
    assert_eq!(rep.runs_cleaned, 0);
}

#[test]
fn styles_are_found_by_name_and_theme_fonts_removed() {
    let (_, after, _) = formatted();
    let styles = after.get_str("word/styles.xml").unwrap();
    // 标题 1 的 ID 是 "1"（中文版 Word 习惯）
    let h1 = &styles[styles.find(r#"w:styleId="1""#).unwrap()..];
    let h1 = &h1[..h1.find("</w:style>").unwrap()];
    assert!(h1.contains(r#"w:eastAsia="黑体""#), "{h1}");
    assert!(!h1.contains("majorHAnsi") && !h1.contains("themeColor"), "{h1}");
    assert!(h1.contains(r#"<w:color w:val="000000"/>"#));
    // 原有的 keepNext、outlineLvl 保留
    assert!(h1.contains("<w:keepNext/>") && h1.contains(r#"<w:outlineLvl w:val="0"/>"#));
    let defaults = &styles[styles.find("<w:docDefaults>").unwrap()..styles.find("</w:docDefaults>").unwrap()];
    assert!(!defaults.contains("minorHAnsi") && defaults.contains(r#"w:eastAsia="宋体""#));
}

#[test]
fn code_math_and_meaningful_formatting_survive() {
    let (_, after, _) = formatted();
    let doc = after.get_str("word/document.xml").unwrap();
    assert_eq!(doc.matches(r#"w:ascii="Menlo""#).count(), 3, "代码段落的等宽字体");
    assert!(doc.contains(r#"<w:vertAlign w:val="superscript"/>"#) && doc.contains(r#"<w:vertAlign w:val="subscript"/>"#));
    assert!(doc.contains("<w:i/>") && doc.contains("<w:b/>") && doc.contains(r#"<w:u w:val="single"/>"#));
    assert!(!doc.contains("Calibri"), "冲突的直接字体应被清除");
    assert!(doc.contains(r#"<w:pStyle w:val="RLTableText"/>"#) && doc.contains(r#"<w:pStyle w:val="RLFigure"/>"#));
}

#[test]
fn landscape_section_keeps_orientation() {
    let (_, after, _) = formatted();
    let doc = after.get_str("word/document.xml").unwrap();
    assert!(doc.contains(r#"<w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/>"#), "横向节");
    assert!(doc.contains(r#"<w:pgSz w:w="11906" w:h="16838"/>"#), "纵向节");
    assert_eq!(doc.matches(r#"w:left="1797""#).count(), 2, "左边距 3.17 cm");
}

#[test]
fn tracked_changes_and_comments_are_reported_not_touched() {
    let (_, _, rep) = formatted();
    assert_eq!(rep.untouched.get("修订（插入）"), Some(&1));
    assert_eq!(rep.untouched.get("修订（删除）"), Some(&1));
    assert_eq!(rep.untouched.get("批注"), Some(&1));
    assert!(rep.notes.iter().any(|n| n.contains("F9")));
}

#[test]
fn verifier_catches_every_kind_of_tampering() {
    let (_, good, _) = formatted();
    let edit = |part: &str, from: &str, to: &str| -> Package {
        let mut p = good.clone();
        let s = p.get_str(part).unwrap();
        assert!(s.contains(from), "{part} 里找不到 {from}");
        p.set(part, s.replacen(from, to, 1).into_bytes());
        Package::read(&p.write().unwrap()).unwrap()
    };
    let doc = good.get_str("word/document.xml").unwrap();
    let drawing = &doc[doc.find("<w:drawing>").unwrap()..doc.find("</w:drawing>").unwrap() + "</w:drawing>".len()];
    let cases: Vec<(&str, Package, &str)> = vec![
        ("改一个字", edit("word/document.xml", "三种测量方法", "三种测量办法"), "正文文字变化"),
        ("删图片", edit("word/document.xml", drawing, ""), "结构数量变化"),
        ("改公式", edit("word/document.xml", "x+ε", "x+δ"), "公式文字变化"),
        ("改域代码", edit("word/document.xml", "SEQ 表", "SEQ 图"), "域代码变化"),
        ("改修订删除的文字", edit("word/document.xml", "，且样本较小", "，样本较小"), "修订删除文字变化"),
        ("改引文控件 tag", edit("word/document.xml", "rl:cite:fx-a", "rl:cite:zz"), "标识变化"),
        ("改脚注", edit("word/footnotes.xml", "虚构", "真实"), "正文文字变化"),
        ("改页眉", edit("word/header1.xml", "排版测试", "排版测验"), "正文文字变化"),
        ("改批注", edit("word/comments.xml", "名称", "名字"), "部件被改动"),
        (
            "改图片字节",
            {
                let mut p = good.clone();
                let mut img = p.get("word/media/image1.png").unwrap().to_vec();
                let n = img.len();
                img[n - 20] ^= 0xff;
                p.set("word/media/image1.png", img);
                Package::read(&p.write().unwrap()).unwrap()
            },
            "部件被改动",
        ),
        (
            "删数据部件",
            {
                let mut p = good.clone();
                p.entries.retain(|e| e.name != "customXml/item1.xml");
                Package::read(&p.write().unwrap()).unwrap()
            },
            "部件丢失",
        ),
    ];
    for (label, tampered, kind) in cases {
        let found = problems(&good, &tampered);
        assert!(found.iter().any(|p| p.kind == kind), "{label}：期望发现「{kind}」，实际 {found:?}");
    }
}

#[test]
fn missing_builtin_styles_are_created_by_name() {
    // 只有 Normal 的最小文档（Word 只写出用到的样式）
    let mut pkg = fixture_pkg();
    let styles = pkg.get_str("word/styles.xml").unwrap();
    let start = styles.find(r#"<w:style w:type="paragraph" w:styleId="1">"#).unwrap();
    let end = start + styles[start..].find(r#"<w:style w:type="paragraph" w:styleId="a4">"#).unwrap();
    pkg.set("word/styles.xml", format!("{}{}", &styles[..start], &styles[end..]).into_bytes());
    let mut after = pkg.clone();
    let rep = format::apply(&mut after, &spec()).unwrap();
    assert!(rep.styles_created.iter().any(|s| s.starts_with("heading 1（Heading1）")), "{:?}", rep.styles_created);
    let after = Package::read(&after.write().unwrap()).unwrap();
    let checks = verify::check_spec(&after, &spec()).unwrap();
    assert!(checks.iter().filter(|c| c.item.contains("级标题")).all(|c| c.ok));
}

#[test]
fn bad_specs_are_rejected() {
    let base = include_str!("../spec/thesis-sample.json");
    assert!(Spec::from_json(&base.replace(r#""paper": "A4""#, r#""paper": "Letter""#)).is_err());
    assert!(Spec::from_json(&base.replace(r#""align": "both""#, r#""align": "middle""#)).is_err());
    assert!(Spec::from_json(&base.replace(r#""name":"#, r#""unknown_field": 1, "name":"#)).is_err());
}

/// 真实 Word 存过的文档（不进仓库）：RL_WORD_DOCX=<路径> cargo test -p docx-engine -- --ignored
#[test]
#[ignore]
fn word_saved_document_keeps_content() {
    let path = std::path::PathBuf::from(std::env::var("RL_WORD_DOCX").expect("设置 RL_WORD_DOCX（绝对路径）"));
    let original = Package::read(&std::fs::read(&path).unwrap()).unwrap();
    let mut pkg = original.clone();
    format::apply(&mut pkg, &spec()).unwrap();
    let after = Package::read(&pkg.write().unwrap()).unwrap();
    let p = problems(&original, &after);
    assert!(p.is_empty(), "{p:?}");
    let failed: Vec<_> = verify::check_spec(&after, &spec()).unwrap().into_iter().filter(|c| !c.ok).collect();
    assert!(failed.is_empty(), "{failed:?}");
}
