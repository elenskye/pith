//! 文档里的引文数据：一个 Custom XML Part，命名空间固定，内含一段 JSON。
//! Word 内（宏）与 Word 外（直接改 .docx）读写同一份格式。旧命名空间（0-B 原型）照样读，写出时换成新的。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::CitationData;

pub const STORE_NS: &str = "urn:pith:citations:v1";
pub const LEGACY_STORE_NS: &str = "urn:research-lab:citations:v1";
pub const STORE_VERSION: u32 = 1;

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocStore {
    #[serde(default = "one")]
    pub version: u32,
    /// 样式 ID（`apa`、`gbt7714-2015-numeric` …）
    pub style: String,
    /// 文献快照（CSL-JSON，按 uid）：换电脑或文献库里没有这条时，仍能用文档自带数据刷新
    #[serde(default)]
    pub items: BTreeMap<String, Value>,
    /// 引文 ID → 引文内容。文档里删掉的引文不立即清理（Word 撤销后还能恢复）
    #[serde(default)]
    pub citations: BTreeMap<String, CitationData>,
    #[serde(default)]
    pub updated: String,
    /// 本版本不认识的字段原样保留
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DocStore {
    pub fn new(style: &str, now: &str) -> Self {
        DocStore { version: STORE_VERSION, style: style.to_string(), items: BTreeMap::new(), citations: BTreeMap::new(), updated: now.to_string(), extra: Map::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StoreError {
    NotStore,
    NoVersion,
    /// 文档由更新版本的 Pith 写入：只读不改
    Newer(u32),
    NoJson,
    BadJson(String),
}

impl StoreError {
    pub fn zh(&self) -> String {
        match self {
            StoreError::NotStore => "不是 Pith 引文数据".into(),
            StoreError::NoVersion => "引文数据缺少版本号".into(),
            StoreError::Newer(v) => format!("文档由更新版本的 Pith 写入（数据版本 {v}），本版本只读不改"),
            StoreError::NoJson => "引文数据缺少 json 节点".into(),
            StoreError::BadJson(e) => format!("引文数据无法解析：{e}"),
        }
    }

    /// Word 里的提示一律英文（`decisions/0006`）
    pub fn en(&self) -> String {
        match self {
            StoreError::NotStore => "This is not Pith citation data.".into(),
            StoreError::NoVersion => "The Pith citation data in this document has no version.".into(),
            StoreError::Newer(v) => format!("This document was written by a newer version of Pith (data version {v}). Update Pith to edit its citations."),
            StoreError::NoJson => "The Pith citation data in this document is incomplete.".into(),
            StoreError::BadJson(_) => "The Pith citation data in this document is damaged.".into(),
        }
    }
}

/// 粗筛：这段 XML 是不是引文数据（新旧命名空间）
pub fn looks_like_store(xml: &str) -> bool {
    xml.contains(STORE_NS) || xml.contains(LEGACY_STORE_NS)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 不带 XML 声明（Word 宏的 `CustomXMLParts.Add` 不接受声明里的 encoding）
pub fn serialize_bare(store: &DocStore) -> String {
    let mut s = store.clone();
    s.version = STORE_VERSION;
    let json = serde_json::to_string(&s).expect("DocStore 一定能序列化");
    format!(r#"<pith:citations xmlns:pith="{STORE_NS}" version="{STORE_VERSION}"><pith:json>{}</pith:json></pith:citations>"#, escape(&json))
}

pub fn serialize(store: &DocStore) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>{}"#, serialize_bare(store))
}

/// 解析文档里的数据。Word 可能改写前缀、空白和转义，所以按命名空间取元素。
pub fn parse(xml: &str) -> Result<DocStore, StoreError> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|_| StoreError::NotStore)?;
    let root = doc.root_element();
    let ns = root.tag_name().namespace().unwrap_or("");
    if root.tag_name().name() != "citations" || (ns != STORE_NS && ns != LEGACY_STORE_NS) {
        return Err(StoreError::NotStore);
    }
    let version: u32 = root.attribute("version").and_then(|v| v.trim().parse().ok()).filter(|v| *v >= 1).ok_or(StoreError::NoVersion)?;
    if version > STORE_VERSION {
        return Err(StoreError::Newer(version));
    }
    let node = root.children().find(|n| n.is_element() && n.tag_name().name() == "json" && n.tag_name().namespace() == Some(ns)).ok_or(StoreError::NoJson)?;
    let text: String = node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
    let mut store: DocStore = serde_json::from_str(&text).map_err(|e| StoreError::BadJson(e.to_string()))?;
    store.version = version;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cite::CiteItemRef;

    fn sample() -> DocStore {
        let mut s = DocStore::new("apa", "2026-11-10T00:00:00Z");
        s.items.insert("u1".into(), serde_json::json!({"id": "u1", "type": "book", "title": "A & B <测试>"}));
        s.citations.insert("c1".into(), CitationData::new(vec![CiteItemRef::new("u1").with_locator("17")], "2026-11-10T00:00:00Z"));
        s
    }

    #[test]
    fn round_trip() {
        let s = sample();
        let xml = serialize(&s);
        assert!(xml.starts_with("<?xml") && xml.contains(STORE_NS));
        assert_eq!(parse(&xml).unwrap(), s);
        assert_eq!(parse(&serialize_bare(&s)).unwrap(), s);
    }

    /// Word 读回来的样子：换了前缀、加了空白、把 > 也转义了
    #[test]
    fn reads_word_rewrites_and_legacy_namespace() {
        let json = serde_json::to_string(&sample()).unwrap().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let xml = format!("<?xml version=\"1.0\" standalone=\"no\"?>\n<ns0:citations xmlns:ns0=\"{LEGACY_STORE_NS}\" version=\"1\">\n  <ns0:json>{json}</ns0:json>\n</ns0:citations>");
        let s = parse(&xml).unwrap();
        assert_eq!(s.items["u1"]["title"], "A & B <测试>");
        assert!(serialize(&s).contains(STORE_NS), "写出时换成新命名空间");
    }

    #[test]
    fn rejects_bad_data() {
        assert_eq!(parse("<x/>"), Err(StoreError::NotStore));
        assert_eq!(parse(&format!(r#"<c:citations xmlns:c="{STORE_NS}"><c:json>{{}}</c:json></c:citations>"#)), Err(StoreError::NoVersion));
        assert_eq!(parse(&format!(r#"<c:citations xmlns:c="{STORE_NS}" version="2"><c:json>{{}}</c:json></c:citations>"#)), Err(StoreError::Newer(2)));
        assert_eq!(parse(&format!(r#"<c:citations xmlns:c="{STORE_NS}" version="1"/>"#)), Err(StoreError::NoJson));
        assert!(matches!(parse(&format!(r#"<c:citations xmlns:c="{STORE_NS}" version="1"><c:json>{{"items":{{}}}}</c:json></c:citations>"#)), Err(StoreError::BadJson(_))));
        assert!(StoreError::Newer(2).en().is_ascii(), "Word 里显示的提示只用 ASCII");
    }

    #[test]
    fn keeps_unknown_fields() {
        let xml = format!(r#"<c:citations xmlns:c="{STORE_NS}" version="1"><c:json>{{"style":"apa","future":[1,2]}}</c:json></c:citations>"#);
        let s = parse(&xml).unwrap();
        assert!(serialize(&s).contains(r#""future":[1,2]"#));
    }
}
