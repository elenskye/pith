//! 引文测试用的合成材料：7 条虚构文献（CSL-JSON）、带引文控件与书目控件的合成文档、
//! 不依赖 citeproc 的确定性排版器（顺序编码，按首次出现编号）。全部为虚构内容。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::plan::{Render, RenderRequest, Rendered, RenderedBibliography};
use super::store::{self, DocStore, LEGACY_STORE_NS, STORE_NS};
use super::{CitationData, CiteItemRef, Run, BIB_TAG, CITE_PREFIX, LEGACY_BIB_TAG, LEGACY_CITE_PREFIX};
use super::zotero::ItemKey;
use crate::package::Package;
use crate::xml::escape_text;

pub const NOW: &str = "2026-11-10T00:00:00Z";
const W_NS: &str = crate::NS_W;

/// 7 条虚构文献（与 0-B 原型相同），uid → CSL-JSON
pub fn library() -> BTreeMap<String, Value> {
    let list: Vec<Value> = serde_json::from_str(include_str!("refs.json")).expect("refs.json");
    list.into_iter().map(|v| (v["id"].as_str().unwrap_or_default().to_string(), v)).collect()
}

/// 只取用得到的那些（模拟 App 按 uid 查文献库）
pub fn lookup(ids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String> {
    Ok(library().into_iter().filter(|(k, _)| ids.contains(k)).collect())
}

/// 一处合成引文：引文 ID → [(文献, 页码)]
pub type SeedCitation = (&'static str, &'static [(&'static str, Option<&'static str>)]);

/// 合成文档里的引文
pub const SEED_CITATIONS: &[SeedCitation] = &[
    ("seed-a", &[("rl-item-0001", None)]),
    ("seed-b", &[("rl-item-0003", Some("17")), ("rl-item-0002", None)]),
    ("seed-c", &[("rl-item-0005", None), ("rl-item-0006", None)]),
    ("seed-d", &[("rl-item-0007", None), ("rl-item-0004", None)]),
];

fn run(t: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#, escape_text(t))
}

fn cite(prefix: &str, id: &str, n: u32) -> String {
    format!(r#"<w:sdt><w:sdtPr><w:alias w:val="Pith"/><w:tag w:val="{prefix}{id}"/><w:id w:val="{n}"/><w:lock w:val="contentLocked"/></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>"#, run("[…]"))
}

/// 合成的 Zotero 引文域：结构仿照 Zotero 7 写入 Word 的格式，内容全部是合成数据。很长的域代码拆成多个 instrText。
pub fn zotero_field() -> String {
    let json = r#"{"citationID":"zSyn0001","properties":{"formattedCitation":"(Halvorsen, 2018)","plainCitation":"(Halvorsen, 2018)","noteIndex":0},"citationItems":[{"id":101,"uris":["http://zotero.org/users/local/SYNTHETIC/items/SYNTH002"],"itemData":{"id":101,"type":"book","title":"The architecture of evidence","author":[{"family":"Halvorsen","given":"Ingrid"}],"issued":{"date-parts":[["2018"]]}}}],"schema":"https://github.com/citation-style-language/schema/raw/master/csl-citation.json"}"#;
    let code = format!(" ADDIN ZOTERO_ITEM CSL_CITATION {json} ");
    let chars: Vec<char> = code.chars().collect();
    let instr: String = chars.chunks(120).map(|c| format!(r#"<w:r><w:instrText xml:space="preserve">{}</w:instrText></w:r>"#, escape_text(&c.iter().collect::<String>()))).collect();
    format!(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>{instr}<w:r><w:fldChar w:fldCharType="separate"/></w:r>{}<w:r><w:fldChar w:fldCharType="end"/></w:r>"#, run("(Halvorsen, 2018)"))
}

/// 最小的 .docx：内容类型、关系、settings（Word 2013+ 兼容模式）、正文
fn base_package(body: &str) -> Package {
    let mut pkg = Package::new();
    pkg.set(
        "[Content_Types].xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#.to_vec(),
    );
    pkg.set(
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
    );
    pkg.set(
        "word/_rels/document.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#.to_vec(),
    );
    // 声明 Word 2013+ 兼容模式，否则 Word 以「兼容模式」打开
    pkg.set(
        "word/settings.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W_NS}"><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#).into_bytes(),
    );
    pkg.set(
        "word/document.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body}</w:body></w:document>"#).into_bytes(),
    );
    pkg
}

/// 合成文档（引文控件里还是占位符，要刷新一次才有文字）。`legacy`：用 0-B 原型的旧 tag 与旧命名空间。
pub fn seed(style: &str, zotero: bool, legacy: bool) -> Vec<u8> {
    let (cp, bib) = if legacy { (LEGACY_CITE_PREFIX, LEGACY_BIB_TAG) } else { (CITE_PREFIX, BIB_TAG) };
    let body = [
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>Pith 引文测试文档</w:t></w:r></w:p>"#.to_string(),
        format!("<w:p>{}{}{}{}{}{}{}</w:p>", run("政治效能感的测量方法近年变化很大"), cite(cp, "seed-a", 11), run("。早期研究依赖问卷"), cite(cp, "seed-b", 12), run("，后来出现了基于合成数据的方法"), cite(cp, "seed-c", 13), run("。")),
        format!("<w:p>{}{}{}</w:p>", run("多作者研究常见于方法学期刊"), cite(cp, "seed-d", 14), run("。")),
        if zotero { format!("<w:p>{}{}{}</w:p>", run("下面这处引文由 Zotero 插入，Pith 不应改动它："), zotero_field(), run("。")) } else { String::new() },
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>参考文献</w:t></w:r></w:p>"#.to_string(),
        format!(r#"<w:sdt><w:sdtPr><w:alias w:val="Pith"/><w:tag w:val="{bib}"/><w:id w:val="20"/><w:lock w:val="contentLocked"/></w:sdtPr><w:sdtContent><w:p><w:pPr><w:spacing w:after="0"/></w:pPr>{}</w:p></w:sdtContent></w:sdt>"#, run("（参考文献生成中）")),
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>"#.to_string(),
    ]
    .join("");
    let mut pkg = base_package(&body);
    let mut s = DocStore::new(style, NOW);
    for (id, items) in SEED_CITATIONS {
        let refs = items.iter().map(|(i, loc)| match loc { Some(l) => CiteItemRef::new(*i).with_locator(l), None => CiteItemRef::new(*i) }).collect();
        s.citations.insert(id.to_string(), CitationData::new(refs, NOW));
    }
    let mut xml = store::serialize(&s);
    if legacy {
        xml = xml.replace(STORE_NS, LEGACY_STORE_NS);
    }
    // 数据部件：0-B 原型同样的三件套
    pkg.set("customXml/item1.xml", xml.into_bytes());
    pkg.set(
        "customXml/itemProps1.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><ds:datastoreItem ds:itemID="{{8E2F0D52-6A43-4C11-9D5E-0F00C0FFEE01}}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"><ds:schemaRefs><ds:schemaRef ds:uri="{}"/></ds:schemaRefs></ds:datastoreItem>"#, if legacy { LEGACY_STORE_NS } else { STORE_NS }).into_bytes(),
    );
    pkg.set(
        "customXml/_rels/item1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#.to_vec(),
    );
    let rels = String::from_utf8(pkg.get("word/_rels/document.xml.rels").unwrap_or_default().to_vec()).unwrap_or_default().replace(
        "</Relationships>",
        r#"<Relationship Id="rIdData" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#,
    );
    pkg.set("word/_rels/document.xml.rels", rels.into_bytes());
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap_or_default().to_vec()).unwrap_or_default().replace(
        "</Types>",
        r#"<Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/></Types>"#,
    );
    pkg.set("[Content_Types].xml", ct.into_bytes());
    pkg.write().expect("合成文档")
}

/// 不依赖 citeproc 的确定性排版器：按首次出现编号，引文是上标「[1,2]」，书目是「[n]\t作者. 标题.」。
/// 只用来测数据流（数据部件、控件改写、核对），不测样式本身（样式由 WebView 里的 citeproc-js 负责）。
pub struct NumberedRender;

impl Render for NumberedRender {
    fn render(&self, req: &RenderRequest) -> Result<Rendered, String> {
        let mut order: Vec<String> = Vec::new();
        let mut citations = BTreeMap::new();
        for c in &req.citations {
            let mut nums = Vec::new();
            for i in &c.items {
                let n = match order.iter().position(|x| *x == i.id) {
                    Some(p) => p + 1,
                    None => {
                        order.push(i.id.clone());
                        order.len()
                    }
                };
                nums.push(match &i.locator {
                    Some(l) => format!("{n}:{l}"),
                    None => n.to_string(),
                });
            }
            citations.insert(c.id.clone(), vec![Run { text: format!("[{}]", nums.join(",")), sup: true, ..Run::default() }]);
        }
        let entries = order
            .iter()
            .enumerate()
            .map(|(k, id)| {
                let item = &req.items[id];
                let who: Vec<&str> = item["author"].as_array().map(|a| a.iter().filter_map(|n| n["literal"].as_str().or_else(|| n["family"].as_str())).collect()).unwrap_or_default();
                vec![Run::plain(format!("[{}]\t{}. ", k + 1, who.join(", "))), Run { text: item["title"].as_str().unwrap_or("").to_string(), italic: true, ..Run::default() }, Run::plain(".")]
            })
            .collect();
        Ok(Rendered { citations, bibliography: Some(RenderedBibliography { entries, second_field_align: Value::String("flush".into()), max_offset: 3.0, ..RenderedBibliography::default() }) })
    }
}

// ---------- Zotero 转换用的合成文档 ----------

/// 合成文献在「Zotero」里的条目 key（rl-item-0001 → SYNTH001）
pub fn zotero_key(uid: &str) -> String {
    format!("SYNTH{}", &uid[uid.len() - 3..])
}

/// 合成的文献库对应：只有 0001–0005 在库里（0006、0007 只在文档里有数据）
pub fn zotero_resolve(k: &ItemKey) -> Option<String> {
    let n: u32 = k.key.strip_prefix("SYNTH")?.parse().ok()?;
    (1..=5).contains(&n).then(|| format!("rl-item-{n:04}"))
}

fn instr_runs(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    chars.chunks(97).map(|c| format!(r#"<w:r><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman"/></w:rPr><w:instrText xml:space="preserve">{}</w:instrText></w:r>"#, escape_text(&c.iter().collect::<String>()))).collect()
}

fn fld(kind: &str) -> String {
    format!(r#"<w:r><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman"/></w:rPr><w:fldChar w:fldCharType="{kind}"/></w:r>"#)
}

/// 一处合成的 Zotero 引文域（Zotero 7 的写法）：items 是 (文献, 页码)
pub fn zotero_citation(id: &str, items: &[(&str, Option<&str>)], shown: &str) -> String {
    let lib = library();
    let cits: Vec<Value> = items
        .iter()
        .enumerate()
        .map(|(n, (uid, loc))| {
            let mut data = lib[*uid].clone();
            data["id"] = Value::from(n as u64 + 100);
            let mut it = serde_json::json!({ "id": n + 100, "uris": [format!("http://zotero.org/users/9999/items/{}", zotero_key(uid))], "itemData": data });
            if let Some(l) = loc {
                it["locator"] = Value::from(*l);
                it["label"] = Value::from("page");
            }
            it
        })
        .collect();
    let json = serde_json::json!({ "citationID": id, "properties": { "formattedCitation": format!("\\super {shown}\\nosupersub{{}}"), "plainCitation": shown, "noteIndex": 0 }, "citationItems": cits, "schema": "https://github.com/citation-style-language/schema/raw/master/csl-citation.json" });
    format!(
        r#"{}{}{}<w:r><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman"/><w:vertAlign w:val="superscript"/></w:rPr><w:t>{}</w:t></w:r>{}"#,
        fld("begin"),
        instr_runs(&format!(" ADDIN ZOTERO_ITEM CSL_CITATION {json} ")),
        fld("separate"),
        escape_text(shown),
        fld("end")
    )
}

const BIB_PPR: &str = r#"<w:pPr><w:pStyle w:val="Bibliography"/><w:spacing w:line="400" w:lineRule="exact"/><w:ind w:left="505" w:hanging="505"/></w:pPr>"#;
const BIB_RPR: &str = r#"<w:rPr><w:rFonts w:ascii="Times New Roman" w:eastAsia="SimSun" w:hAnsi="Times New Roman"/><w:sz w:val="21"/></w:rPr>"#;

/// 合成的 Zotero 文档：5 处 Zotero 引文（含页码、多条、库里没有的）、一个别的域（PAGE）、一处结构特殊不转换的引文（里面有修订）、
/// 跨 3 段的 Zotero 书目（最后一段在域后面还有分页符），文档设置里是 `style_uri`。
pub fn zotero_doc(style_uri: &str) -> Vec<u8> {
    let entries = ["[1]\tLindqvist M, Okafor D. Civic attitudes.", "[2]\t王晓明, 李华. 合成数据中的政治效能感测量.", "[3]\tHalvorsen I. The architecture of evidence."];
    let mut bib = String::new();
    for (i, e) in entries.iter().enumerate() {
        let mut runs = String::new();
        if i == 0 {
            runs.push_str(&fld("begin"));
            runs.push_str(&instr_runs(r#" ADDIN ZOTERO_BIBL {"uncited":[],"omitted":[],"custom":[]} CSL_BIBLIOGRAPHY "#));
            runs.push_str(&fld("separate"));
        }
        let (num, rest) = e.split_once('\t').unwrap_or(("", e));
        runs.push_str(&format!(r#"<w:r>{BIB_RPR}<w:t>{}</w:t></w:r><w:r>{BIB_RPR}<w:tab/></w:r><w:r>{BIB_RPR}<w:t xml:space="preserve">{}</w:t></w:r>"#, escape_text(num), escape_text(rest)));
        if i == entries.len() - 1 {
            runs.push_str(&fld("end"));
            runs.push_str(r#"<w:r><w:br w:type="page"/></w:r>"#);
        }
        bib.push_str(&format!("<w:p>{BIB_PPR}{runs}</w:p>"));
    }
    let tracked = format!(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>{}<w:r><w:fldChar w:fldCharType="separate"/></w:r><w:ins w:id="90" w:author="synthetic" w:date="2026-01-01T00:00:00Z"><w:r><w:t>[9]</w:t></w:r></w:ins><w:r><w:fldChar w:fldCharType="end"/></w:r>"#, instr_runs(&format!(" ADDIN ZOTERO_ITEM CSL_CITATION {} ", serde_json::json!({"citationID":"zTracked","properties":{"plainCitation":"[9]"},"citationItems":[{"id":1,"uris":["http://zotero.org/users/9999/items/SYNTH001"]}]}))));
    let body = [
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Zotero 转换测试文档（合成）</w:t></w:r></w:p>"#.to_string(),
        format!("<w:p>{}{}{}{}{}</w:p>", run("政治效能感的测量方法近年变化很大"), zotero_citation("zA", &[("rl-item-0001", None)], "[1]"), run("。早期研究依赖问卷"), zotero_citation("zB", &[("rl-item-0003", Some("17")), ("rl-item-0002", None)], "[2,3]"), run("。")),
        format!("<w:p>{}{}{}</w:p>", run("只在文档里有数据的文献"), zotero_citation("zC", &[("rl-item-0006", None)], "[4]"), run("。")),
        format!("<w:p>{}{}{}{}</w:p>", run("再引一次"), zotero_citation("zD", &[("rl-item-0001", Some("3"))], "[1]"), run("，以及修订中的引文"), tracked),
        format!(r#"<w:p>{}{}<w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>{}<w:r><w:t>1</w:t></w:r>{}{}</w:p>"#, run("页码域："), fld("begin"), fld("separate"), fld("end"), run("。")),
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>参考文献</w:t></w:r></w:p>"#.to_string(),
        bib,
        format!("<w:p>{}</w:p>", run("附录从这一页开始。")),
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#.to_string(),
    ]
    .join("");
    let mut pkg = base_package(&body);
    let pref = format!(r#"<data data-version="3" zotero-version="7.0"><session id="SYNTHSES"/><style id="{style_uri}" hasBibliography="1" bibliographyStyleHasBeenSet="1"/><prefs><pref name="fieldType" value="Field"/></prefs></data>"#);
    let (p1, p2) = pref.split_at(pref.len() / 2);
    pkg.set(
        "docProps/custom.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="2" name="ZOTERO_PREF_1"><vt:lpwstr>{}</vt:lpwstr></property><property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="3" name="ZOTERO_PREF_2"><vt:lpwstr>{}</vt:lpwstr></property></Properties>"#,
            escape_text(p1),
            escape_text(p2)
        )
        .into_bytes(),
    );
    let rels = String::from_utf8(pkg.get("_rels/.rels").unwrap_or_default().to_vec()).unwrap_or_default().replace(
        "</Relationships>",
        r#"<Relationship Id="rIdCustom" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" Target="docProps/custom.xml"/></Relationships>"#,
    );
    pkg.set("_rels/.rels", rels.into_bytes());
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap_or_default().to_vec()).unwrap_or_default().replace(
        "</Types>",
        r#"<Override PartName="/docProps/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.custom-properties+xml"/></Types>"#,
    );
    pkg.set("[Content_Types].xml", ct.into_bytes());
    pkg.write().expect("合成 Zotero 文档")
}
