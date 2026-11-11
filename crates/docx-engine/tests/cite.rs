//! 引文写入（0-B 移植）的测试：合成文档 + 不依赖 citeproc 的确定性排版器。
//! 样式本身（citeproc-js 排出的文字）由前端 `src/cite/` 的测试负责。

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use docx_engine::cite::check::{self, Snapshot};
use docx_engine::cite::docx::{self, refresh};
use docx_engine::cite::fixture::{self, NumberedRender, NOW};
use docx_engine::cite::plan::{plan_refresh, DocCitation, Issue, Render, RenderRequest, Rendered};
use docx_engine::cite::store::{self, DocStore, STORE_NS};
use docx_engine::cite::{runs_text, BIB_TAG, CITE_PREFIX, EMPTY_BIBLIOGRAPHY};
use docx_engine::package::Package;
use serde_json::Value;

const LATER: &str = "2026-11-11T08:00:00Z";

fn ids() -> impl FnMut() -> String {
    let n = Cell::new(0);
    move || {
        n.set(n.get() + 1);
        format!("new-{}", n.get())
    }
}

fn refreshed(bytes: &[u8]) -> (Vec<u8>, docx_engine::cite::plan::Plan) {
    let r = refresh(bytes, None, &fixture::lookup, &NumberedRender, &mut ids(), LATER).unwrap();
    (r.bytes, r.plan)
}

fn texts(s: &Snapshot) -> Vec<String> {
    s.citations.iter().map(|(_, r)| runs_text(r)).collect()
}

fn doc_xml(bytes: &[u8]) -> String {
    Package::read(bytes).unwrap().get_str("word/document.xml").unwrap()
}

fn with_doc_xml(bytes: &[u8], f: impl Fn(String) -> String) -> Vec<u8> {
    let mut pkg = Package::read(bytes).unwrap();
    let xml = f(pkg.get_str("word/document.xml").unwrap());
    pkg.set("word/document.xml", xml.into_bytes());
    pkg.write().unwrap()
}

#[test]
fn seed_refresh_writes_citations_bibliography_and_snapshots() {
    let seed = fixture::seed("gbt7714-2015-numeric", true, false);
    let (out, plan) = refreshed(&seed);
    let s = check::read(&out).unwrap();
    assert_eq!(texts(&s), ["[1]", "[2:17,3]", "[4,5]", "[6,7]"]);
    assert!(s.citations.iter().all(|(t, r)| t.starts_with(CITE_PREFIX) && r.iter().all(|x| x.sup)));
    assert_eq!(s.bibliographies.len(), 1);
    assert_eq!(s.bibliographies[0].len(), 7);
    assert_eq!(s.bibliographies[0][2], "[3]\tHalvorsen. The architecture of evidence.");
    assert!(plan.issues.is_empty());
    assert_eq!(plan.bibliography.hanging_indent_pt, 28.0, "顺序编码：编号列宽 (3+1)*7");
    let store = s.store.clone().unwrap().unwrap();
    assert_eq!(store.items.len(), 7, "文献快照写进文档");
    assert_eq!((store.updated.as_str(), store.style.as_str()), (LATER, "gbt7714-2015-numeric"));
    // 悬挂缩进、斜体写进去了
    let xml = doc_xml(&out);
    assert!(xml.contains(r#"<w:ind w:left="560" w:hanging="560"/>"#));
    assert!(xml.contains("<w:i/><w:iCs/>"));
    // 独立核对：用同样的数据重新排一遍，逐处一致；Zotero 域没动
    let before = check::read(&seed).unwrap();
    assert!(check::outside_changed(&before, &s).is_empty());
    let v = check::verify(&out, &fixture::lookup, &NumberedRender).unwrap();
    assert!(v.ok, "{:?}", v.problems);
    assert_eq!(v.zotero.citations, 1);
    assert_eq!(v.zotero.digest, check::zotero_fields(&before.fields).digest);
}

#[test]
fn second_refresh_changes_nothing_but_the_timestamp() {
    let (once, _) = refreshed(&fixture::seed("apa", false, false));
    let (twice, plan) = refreshed(&once);
    assert!(plan.citations.iter().all(|u| !u.changed), "文字已经对了，不再改写");
    assert_eq!(texts(&check::read(&once).unwrap()), texts(&check::read(&twice).unwrap()));
}

#[test]
fn switching_style_is_recorded_in_the_document() {
    let (once, _) = refreshed(&fixture::seed("apa", false, false));
    let r = refresh(&once, Some("chicago-author-date"), &fixture::lookup, &NumberedRender, &mut ids(), LATER).unwrap();
    assert_eq!(check::read(&r.bytes).unwrap().store.unwrap().unwrap().style, "chicago-author-date");
}

/// 0-B 原型写的文档（旧 tag、旧命名空间）：照样刷新，写出时换成新名
#[test]
fn legacy_documents_are_migrated() {
    let seed = fixture::seed("apa", false, true);
    assert!(doc_xml(&seed).contains("rl:cite:seed-a"));
    let (out, plan) = refreshed(&seed);
    assert!(plan.citations.iter().all(|u| u.retag.is_some()));
    let xml = doc_xml(&out);
    assert!(!xml.contains("rl:cite:") && !xml.contains("rl:bibliography"));
    assert!(xml.contains("pith:cite:seed-a") && xml.contains(BIB_TAG));
    let pkg = Package::read(&out).unwrap();
    assert!(pkg.get_str("customXml/item1.xml").unwrap().contains(STORE_NS));
    assert!(pkg.get_str("customXml/itemProps1.xml").unwrap().contains(STORE_NS));
    assert!(check::verify(&out, &fixture::lookup, &NumberedRender).unwrap().ok);
}

/// 复制粘贴产生的重复引文：后一处分配新 ID，数据跟着复制一份
#[test]
fn duplicated_citations_get_new_ids() {
    let seed = with_doc_xml(&fixture::seed("apa", false, false), |x| {
        let start = x.find("<w:sdt>").unwrap();
        let end = x[start..].find("</w:sdt>").unwrap() + start + "</w:sdt>".len();
        let copy = x[start..end].to_string();
        x.replacen("。</w:t></w:r></w:p>", &format!("。</w:t></w:r>{copy}</w:p>"), 1)
    });
    let (out, plan) = refreshed(&seed);
    assert!(plan.issues.contains(&Issue::Duplicate { index: 3 }), "{:?}", plan.issues);
    let s = check::read(&out).unwrap();
    assert_eq!(s.citations.len(), 5);
    assert_eq!(s.citations[3].0, "pith:cite:new-1");
    assert_eq!(runs_text(&s.citations[3].1), runs_text(&s.citations[0].1), "同一处引文，文字相同");
    assert!(s.store.unwrap().unwrap().citations.contains_key("new-1"));
}

/// 没有数据的控件（从别的文档粘贴来的）跳过并报告；文献库里没有的文献用文档快照
#[test]
fn missing_data_and_library_fallback() {
    let seed = with_doc_xml(&fixture::seed("apa", false, false), |x| x.replace("pith:cite:seed-d", "pith:cite:pasted"));
    let (out, plan) = refreshed(&seed);
    assert!(matches!(plan.issues.as_slice(), [Issue::NoData { index: 3, .. }]));
    let s = check::read(&out).unwrap();
    assert_eq!(runs_text(&s.citations[3].1), "[…]", "跳过的不改");
    assert_eq!(s.bibliographies[0].len(), 5);
    let v = check::verify(&out, &fixture::lookup, &NumberedRender).unwrap();
    assert!(!v.ok && v.problems.iter().any(|p| p.contains("缺少它的数据")));

    // 换电脑：文献库是空的，靠文档里的快照照样刷新
    let empty = |_: &BTreeSet<String>| -> Result<BTreeMap<String, Value>, String> { Ok(BTreeMap::new()) };
    let r = refresh(&out, Some("ieee"), &empty, &NumberedRender, &mut ids(), LATER).unwrap();
    assert_eq!(check::read(&r.bytes).unwrap().bibliographies[0].len(), 5);
}

/// 有人手改了引文文字：核对报出来
#[test]
fn verify_reports_tampering() {
    let (out, _) = refreshed(&fixture::seed("apa", false, false));
    let tampered = with_doc_xml(&out, |x| x.replacen("[1]", "[9]", 1));
    let v = check::verify(&tampered, &fixture::lookup, &NumberedRender).unwrap();
    assert!(!v.ok);
    assert_eq!(v.citations.iter().filter(|c| !c.ok).count(), 1);
    assert!(v.problems[0].contains("[9]"));
}

/// 引文控件原来的字体、字号、语言保留；原来的斜体等会被引文格式覆盖的属性去掉
#[test]
fn keeps_fonts_of_the_original_run() {
    let seed = with_doc_xml(&fixture::seed("apa", false, false), |x| {
        x.replacen(
            r#"<w:r><w:t xml:space="preserve">[…]</w:t></w:r>"#,
            r#"<w:r><w:rPr><w:rFonts w:ascii="Times New Roman" w:eastAsia="宋体"/><w:i/><w:sz w:val="21"/><w:lang w:eastAsia="zh-CN"/></w:rPr><w:t xml:space="preserve">[…]</w:t></w:r>"#,
            1,
        )
    });
    let xml = doc_xml(&refreshed(&seed).0);
    assert!(xml.contains(r#"<w:rPr><w:rFonts w:ascii="Times New Roman" w:eastAsia="宋体"/><w:sz w:val="21"/><w:vertAlign w:val="superscript"/><w:lang w:eastAsia="zh-CN"/></w:rPr><w:t xml:space="preserve">[1]</w:t>"#), "{xml}");
}

#[test]
fn refuses_documents_without_or_with_newer_data() {
    let plain = fixture::seed("apa", false, false);
    let mut pkg = Package::read(&plain).unwrap();
    pkg.set("customXml/item1.xml", br#"<x/>"#.to_vec());
    let e = refresh(&pkg.write().unwrap(), None, &fixture::lookup, &NumberedRender, &mut ids(), LATER).err().unwrap();
    assert!(e.contains("没有 Pith 引文数据"), "{e}");
    let mut pkg = Package::read(&plain).unwrap();
    let newer = pkg.get_str("customXml/item1.xml").unwrap().replace(r#"version="1""#, r#"version="9""#);
    pkg.set("customXml/item1.xml", newer.into_bytes());
    let e = refresh(&pkg.write().unwrap(), None, &fixture::lookup, &NumberedRender, &mut ids(), LATER).err().unwrap();
    assert!(e.contains("更新版本"), "{e}");
}

/// 没有数据部件的文档：写数据时新建部件、关系与内容类型，Word 能找到它
#[test]
fn creates_the_data_part_when_missing() {
    let mut pkg = Package::read(&fixture::seed("apa", false, false)).unwrap();
    let mut slim = Package::new();
    for name in pkg.names().into_iter().filter(|n| !n.starts_with("customXml/")).map(str::to_string).collect::<Vec<_>>() {
        let data = pkg.get(&name).unwrap().to_vec();
        let data = if name == "word/_rels/document.xml.rels" {
            String::from_utf8(data).unwrap().replace(r#"<Relationship Id="rIdData" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/>"#, "").into_bytes()
        } else {
            data
        };
        slim.set(&name, data);
    }
    pkg = Package::read(&slim.write().unwrap()).unwrap();
    let bytes = pkg.write().unwrap();
    let mut d = docx::open(&bytes).unwrap();
    assert!(d.store().is_none());
    let mut s = DocStore::new("apa", NOW);
    s.citations.insert("seed-a".into(), docx_engine::cite::CitationData::new(vec![docx_engine::cite::CiteItemRef::new("rl-item-0001")], NOW));
    let lib = fixture::library();
    let plan = plan_refresh(&d.citations(), &s, &lib, &NumberedRender, &mut ids(), LATER).unwrap();
    d.apply(&plan).unwrap();
    let out = d.into_bytes().unwrap();
    let after = Package::read(&out).unwrap();
    assert!(after.get_str("word/_rels/document.xml.rels").unwrap().contains(r#"Id="rIdPith1""#));
    assert!(after.get_str("[Content_Types].xml").unwrap().contains("/customXml/itemProps1.xml"));
    assert!(after.get_str("customXml/itemProps1.xml").unwrap().contains(STORE_NS));
    let snap = check::read(&out).unwrap();
    assert!(check::against_plan(&snap, &plan).is_empty());
    assert!(check::outside_changed(&check::read(&bytes).unwrap(), &snap).is_empty());
    assert_eq!(snap.citations.iter().filter(|(_, r)| runs_text(r) != "[…]").count(), 1, "只有有数据的那处被改写");
}

/// 0-C 的合成论文（脚注、页眉、图片、修订、域……，带 0-B 旧格式引文）：刷新只动正文与数据部件
#[test]
fn refresh_on_a_rich_document_touches_only_citations() {
    let bytes = docx_engine::fixture::build().write().unwrap();
    let (out, plan) = refreshed(&bytes);
    assert_eq!(plan.citations.len(), 2);
    let (a, b) = (check::read(&bytes).unwrap(), check::read(&out).unwrap());
    assert!(check::outside_changed(&a, &b).is_empty());
    let changed: Vec<&String> = b.parts.iter().filter(|(k, v)| a.parts.get(*k) != Some(v)).map(|(k, _)| k).collect();
    assert_eq!(changed, ["customXml/item1.xml", "customXml/itemProps1.xml", "word/document.xml"]);
    // 排版器的保真校验也认：除引文控件内的文字外一切不变
    let snap = |b: &[u8]| docx_engine::verify::snapshot(&Package::read(b).unwrap()).unwrap();
    let (sa, sb) = (snap(&bytes), snap(&out));
    for part in sa.stories.keys().filter(|p| *p != "word/document.xml") {
        assert_eq!(sa.stories[part], sb.stories[part], "{part}");
    }
}

/// 排版器出错（比如样式文件坏了）：整个刷新失败，不写半截
#[test]
fn render_errors_abort_the_refresh() {
    struct Broken;
    impl Render for Broken {
        fn render(&self, _: &RenderRequest) -> Result<Rendered, String> {
            Err("样式文件坏了".into())
        }
    }
    let e = refresh(&fixture::seed("apa", false, false), None, &fixture::lookup, &Broken, &mut ids(), LATER).err().unwrap();
    assert_eq!(e, "样式文件坏了");
}

#[test]
fn empty_documents_get_the_placeholder_bibliography() {
    let seed = fixture::seed("apa", false, false);
    let d = docx::open(&seed).unwrap();
    let s = DocStore::new("apa", NOW);
    let doc: Vec<DocCitation> = Vec::new();
    let plan = plan_refresh(&doc, &s, &BTreeMap::new(), &NumberedRender, &mut ids(), LATER).unwrap();
    assert_eq!(runs_text(&plan.bibliography.paragraphs()[0]), EMPTY_BIBLIOGRAPHY);
    assert_eq!(plan.bibliography.hanging_indent_pt, 0.0);
    assert_eq!(d.citations().len(), 4);
    assert!(store::looks_like_store(&store::serialize(&s)));
}

// ---------- Zotero 转换（Word 外） ----------

use docx_engine::cite::convert::{self, Options};
use docx_engine::cite::zotero::ItemKey;

fn style_for_uri(uri: &str) -> Option<String> {
    match uri {
        "http://www.zotero.org/styles/apa" => Some("apa".into()),
        "http://www.zotero.org/styles/china-national-standard-gb-t-7714-2015-numeric" => Some("gbt7714-2015-numeric".into()),
        _ => None,
    }
}

fn resolve(k: &ItemKey) -> Option<String> {
    fixture::zotero_resolve(k)
}

fn convert_doc(bytes: &[u8]) -> Result<convert::Converted, String> {
    let o = Options { resolve: &resolve, style_for_uri: &style_for_uri, default_style: "gbt7714-2015-numeric", library: &fixture::lookup, render: &NumberedRender, now: LATER };
    convert::convert(bytes, &o, &mut ids())
}

#[test]
fn zotero_inspect_finds_citations_bibliography_and_style() {
    let f = convert::inspect(&fixture::zotero_doc("http://www.zotero.org/styles/apa")).unwrap();
    assert_eq!(f.codes.len(), 4, "修订里的那处不算能转换的");
    assert!(f.codes.iter().all(|c| c.trim_start().starts_with("ADDIN ZOTERO_ITEM")));
    assert_eq!(f.fields, 6, "5 处引文 + 1 个书目");
    assert!(f.bibliography);
    assert_eq!(f.bibliography_texts, ["[1]\tLindqvist M, Okafor D. Civic attitudes.", "[2]\t王晓明, 李华. 合成数据中的政治效能感测量.", "[3]\tHalvorsen I. The architecture of evidence."]);
    assert_eq!(f.zotero_style.as_deref(), Some("http://www.zotero.org/styles/apa"));
    assert!(!f.has_pith && !f.note_style);
}

#[test]
fn zotero_convert_replaces_fields_and_keeps_everything_else() {
    let src = fixture::zotero_doc("http://www.zotero.org/styles/apa");
    let c = convert_doc(&src).unwrap();
    let r = &c.report;
    assert_eq!(r.style, "apa", "按 Zotero 的样式对应");
    assert!(r.style_note.is_none());
    assert_eq!((r.stats.citations, r.stats.converted, r.stats.items, r.stats.in_library, r.stats.from_document, r.stats.missing), (4, 4, 5, 4, 1, 0));
    assert!(r.bibliography);
    assert_eq!(r.left, 1, "修订里的那处原样留着");
    assert_eq!(r.same_citations, 2);
    assert_eq!(r.citation_diffs.iter().map(|d| (d.zotero.as_str(), d.pith.as_str())).collect::<Vec<_>>(), [("[2,3]", "[2:17,3]"), ("[1]", "[1:3]")]);
    assert_eq!(r.zotero_entries, 3);
    let s = check::read(&c.bytes).unwrap();
    assert_eq!(texts(&s), ["[1]", "[2:17,3]", "[4]", "[1:3]"]);
    assert!(s.citations.iter().all(|(t, runs)| t.starts_with(CITE_PREFIX) && runs.iter().all(|x| x.sup)));
    assert_eq!(s.bibliographies.len(), 1);
    assert_eq!(s.bibliographies[0].len(), 4);
    // 别的域、修订里的 Zotero 引文、书目后面的分页符都还在
    assert!(s.fields.iter().any(|f| f.code == "PAGE"));
    assert_eq!(s.fields.iter().filter(|f| f.code.contains("ZOTERO")).count(), 1);
    let xml = doc_xml(&c.bytes);
    assert!(xml.contains(r#"<w:br w:type="page"/>"#));
    assert!(xml.contains(r#"<w:pStyle w:val="Bibliography"/><w:spacing w:line="400" w:lineRule="exact"/>"#), "书目沿用原来的段落样式与行距");
    assert!(xml.contains(r#"w:eastAsia="SimSun""#), "沿用原来的字体");
    // 数据部件：库里没有的那条用文档自带的数据
    let store = s.store.clone().unwrap().unwrap();
    assert_eq!(store.style, "apa");
    assert_eq!(store.items["zotero-u-SYNTH006"]["title"], fixture::library()["rl-item-0006"]["title"]);
    assert!(store.items.contains_key("rl-item-0001"));
    assert_eq!(store.citations.len(), 4);
    // 独立核对：Pith 再核对一遍一致；再刷新什么都不变；Zotero 的文档设置不动
    assert!(check::verify(&c.bytes, &fixture::lookup, &NumberedRender).unwrap().ok);
    let (_, plan) = refreshed(&c.bytes);
    assert!(plan.citations.iter().all(|u| !u.changed));
    let before = Package::read(&src).unwrap();
    let after = Package::read(&c.bytes).unwrap();
    assert_eq!(before.get("docProps/custom.xml"), after.get("docProps/custom.xml"));
}

#[test]
fn zotero_convert_with_an_unknown_style_uses_the_default() {
    let c = convert_doc(&fixture::zotero_doc("http://www.zotero.org/styles/nature")).unwrap();
    assert_eq!(c.report.style, "gbt7714-2015-numeric");
    assert!(c.report.style_note.as_deref().unwrap().contains("nature"));
}

#[test]
fn zotero_convert_refuses_documents_without_zotero_and_converts_once() {
    let plain = fixture::seed("apa", false, false);
    assert_eq!(convert_doc(&plain).err().as_deref(), Some("文档里没有 Zotero 引文"));
    let once = convert_doc(&fixture::zotero_doc("http://www.zotero.org/styles/apa")).unwrap().bytes;
    // 只剩修订里那处：结构特殊，没法转换
    assert!(convert_doc(&once).err().unwrap().contains("结构特殊"));
}

/// 文档里已经有 Pith 引文与书目：沿用 Pith 的样式，不再加第二个书目
#[test]
fn zotero_convert_into_a_pith_document() {
    let (pith, _) = refreshed(&fixture::seed("chicago-author-date", false, false));
    let mixed = with_doc_xml(&pith, |x| {
        x.replacen("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>参考文献</w:t>", &format!("<w:p>{}</w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>参考文献</w:t>", fixture::zotero_citation("zX", &[("rl-item-0002", None)], "[9]")), 1)
    });
    let c = convert_doc(&mixed).unwrap();
    assert_eq!(c.report.style, "chicago-author-date");
    assert!(c.report.style_note.is_some() && !c.report.bibliography);
    let s = check::read(&c.bytes).unwrap();
    assert_eq!(s.citations.len(), 5);
    assert_eq!(s.bibliographies.len(), 1);
}
