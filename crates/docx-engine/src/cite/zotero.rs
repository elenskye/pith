//! Zotero 写进 Word 的引文：识别、解析、换成 Pith 引文数据（`decisions/0017`）。纯函数，不碰 Word。
//!
//! - 引文域代码：` ADDIN ZOTERO_ITEM CSL_CITATION {json}`（Zotero 5 起）；更早的 ` ADDIN ZOTERO_ITEM {json}` 也认。
//! - 书目域代码：` ADDIN ZOTERO_BIBL {json} CSL_BIBLIOGRAPHY`。
//! - 文档设置：自定义文档属性 `ZOTERO_PREF_1`、`_2`…（一段 XML 拆开存），里面有样式 URI。
//!
//! 文献按 Zotero 条目 key 对应到文献库（导入 Zotero 时记下的 source_key）；库里没有的，
//! 用域里自带的 itemData 作为文档快照（`DocStore::items`），引文照样能排、能刷新。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{CitationData, CiteItemRef};

pub const CITATION_MARK: &str = "ADDIN ZOTERO_ITEM";
pub const BIBLIOGRAPHY_MARK: &str = "ADDIN ZOTERO_BIBL";
/// 文档设置所在的自定义文档属性名前缀
pub const PREF_PROPERTY: &str = "ZOTERO_PREF_";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Citation,
    Bibliography,
}

/// 这段域代码是不是 Zotero 的（引文 / 书目）
pub fn field_kind(code: &str) -> Option<FieldKind> {
    let c = code.trim_start();
    if c.starts_with(CITATION_MARK) {
        Some(FieldKind::Citation)
    } else if c.starts_with(BIBLIOGRAPHY_MARK) {
        Some(FieldKind::Bibliography)
    } else {
        None
    }
}

/// Zotero 条目所在的库：个人库（`users/<ID>` 或 `users/local/<本机 key>`）或群组库（`groups/<ID>`）
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum Library {
    User,
    Group(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ItemKey {
    pub library: Library,
    pub key: String,
}

impl ItemKey {
    /// 文献库里没有这条时，文档快照用的 ID（稳定、可读）
    pub fn snapshot_id(&self) -> String {
        match &self.library {
            Library::User => format!("zotero-u-{}", self.key),
            Library::Group(g) => format!("zotero-g{g}-{}", self.key),
        }
    }
}

/// `http://zotero.org/users/123/items/ABCD2345`、`…/users/local/xxxx/items/…`、`…/groups/9/items/…`
pub fn parse_uri(uri: &str) -> Option<ItemKey> {
    let rest = uri.split_once("zotero.org/")?.1;
    let (lib, key) = rest.split_once("/items/")?;
    let key = key.trim_end_matches('/');
    if key.len() != 8 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let library = if lib.starts_with("users/") {
        Library::User
    } else {
        Library::Group(lib.strip_prefix("groups/")?.to_string())
    };
    Some(ItemKey { library, key: key.to_string() })
}

/// 域里的一条文献
#[derive(Clone, Debug, PartialEq)]
pub struct ZoteroItem {
    pub key: Option<ItemKey>,
    /// 域里自带的 CSL-JSON（Zotero 写入时的快照）
    pub data: Option<Value>,
    pub locator: Option<String>,
    pub label: Option<String>,
    /// prefix / suffix / suppress-author / author-only，原样带过来
    pub extra: Map<String, Value>,
}

/// 一处 Zotero 引文
#[derive(Clone, Debug, PartialEq)]
pub struct ZoteroCitation {
    pub zotero_id: String,
    pub items: Vec<ZoteroItem>,
    /// Zotero 显示的文字（plainCitation）
    pub plain: String,
    /// 用户在 Zotero 里手改过这处引文的文字（dontUpdate / custom）：换成 Pith 引文后按样式重排
    pub edited: bool,
}

const ITEM_EXTRA: &[&str] = &["prefix", "suffix", "suppress-author", "author-only"];

/// 取出文本里的第一个 JSON 对象（后面可能还跟着 ` CSL_BIBLIOGRAPHY` 等）
fn first_json(s: &str) -> Option<Value> {
    let start = s.find('{')?;
    serde_json::Deserializer::from_str(&s[start..]).into_iter::<Value>().next()?.ok()
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn parse_citation(code: &str) -> Result<ZoteroCitation, String> {
    if field_kind(code) != Some(FieldKind::Citation) {
        return Err("不是 Zotero 引文域".into());
    }
    let json = first_json(code).ok_or("Zotero 引文域里没有可读的数据")?;
    let props = &json["properties"];
    let items = json["citationItems"].as_array().ok_or("Zotero 引文域里没有文献")?;
    let items = items
        .iter()
        .map(|it| {
            let key = it["uris"].as_array().and_then(|u| u.iter().filter_map(Value::as_str).find_map(parse_uri)).or_else(|| it["uri"].as_str().and_then(parse_uri));
            let mut extra = Map::new();
            for k in ITEM_EXTRA {
                if let Some(v) = it.get(*k).filter(|v| !v.is_null() && *v != &Value::Bool(false) && *v != &Value::String(String::new())) {
                    extra.insert(k.to_string(), v.clone());
                }
            }
            ZoteroItem { key, data: it.get("itemData").filter(|d| d.is_object()).cloned(), locator: it.get("locator").and_then(text), label: it.get("label").and_then(text), extra }
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        return Err("Zotero 引文域里没有文献".into());
    }
    Ok(ZoteroCitation {
        zotero_id: json["citationID"].as_str().map(str::to_string).or_else(|| json["citationID"].as_i64().map(|n| n.to_string())).unwrap_or_default(),
        items,
        plain: props["plainCitation"].as_str().unwrap_or("").to_string(),
        edited: props["dontUpdate"].as_bool() == Some(true) || props.get("custom").is_some_and(|c| !c.is_null()),
    })
}

/// Zotero 的文档设置（Document Preferences）
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// 样式 URI（`http://www.zotero.org/styles/apa`）
    pub style: Option<String>,
    pub locale: Option<String>,
    /// 脚注 / 尾注样式时非 0
    pub note_type: Option<i64>,
}

/// 解析拼好的 ZOTERO_PREF 文本：`<data data-version="3"><style id="…" locale="…"/><prefs><pref name="…" value="…"/>…`
pub fn parse_prefs(xml: &str) -> Option<Prefs> {
    let doc = roxmltree::Document::parse(xml.trim()).ok()?;
    let root = doc.root_element();
    if root.tag_name().name() != "data" {
        return None;
    }
    let style = root.children().find(|n| n.has_tag_name("style"));
    let pref = |name: &str| root.descendants().find(|n| n.has_tag_name("pref") && n.attribute("name") == Some(name)).and_then(|n| n.attribute("value"));
    Some(Prefs {
        style: style.and_then(|s| s.attribute("id")).map(str::to_string),
        locale: style.and_then(|s| s.attribute("locale")).filter(|l| !l.is_empty()).map(str::to_string),
        note_type: pref("noteType").and_then(|v| v.parse().ok()),
    })
}

/// 从 docProps/custom.xml 拼出 ZOTERO_PREF_1、_2… 的文本
pub fn prefs_from_custom_xml(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let mut parts: Vec<(u32, String)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("property"))
        .filter_map(|n| {
            let i: u32 = n.attribute("name")?.strip_prefix(PREF_PROPERTY)?.parse().ok()?;
            let t: String = n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect();
            Some((i, t))
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    parts.sort();
    Some(parts.into_iter().map(|(_, t)| t).collect())
}

/// 一处换好的引文
#[derive(Clone, Debug)]
pub struct Converted {
    pub data: CitationData,
    pub zotero_plain: String,
    pub edited: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Zotero 引文域
    pub citations: usize,
    pub converted: usize,
    /// 引文里的文献（逐处计）
    pub items: usize,
    /// 对应到文献库的
    pub in_library: usize,
    /// 文献库里没有，用文档自带数据的
    pub from_document: usize,
    /// 文献库里没有、域里也没有数据的（丢掉）
    pub missing: usize,
    /// 在 Zotero 里手改过文字的引文
    pub edited: usize,
}

/// 转换中值得告诉用户的事
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Note {
    /// 域代码读不懂（index 是第几个 Zotero 引文域，从 0 起）
    Unreadable { index: usize, error: String },
    /// 这处的文献全都找不到
    Empty { index: usize },
    /// 文献库里没有这条，用文档自带的数据
    FromDocument { index: usize, item: String, title: String },
    Missing { index: usize },
    Edited { index: usize, text: String },
}

impl Note {
    pub fn zh(&self) -> String {
        match self {
            Note::Unreadable { index, error } => format!("第 {} 处 Zotero 引文读不懂，没有转换：{error}", index + 1),
            Note::Empty { index } => format!("第 {} 处 Zotero 引文里的文献都找不到，没有转换", index + 1),
            Note::FromDocument { index, title, .. } => format!("第 {} 处：「{title}」不在文献库里，用文档里自带的数据", index + 1),
            Note::Missing { index } => format!("第 {} 处：有一条文献既不在文献库里、域里也没有数据，去掉了", index + 1),
            Note::Edited { index, text } => format!("第 {} 处在 Zotero 里手改过文字（「{text}」），转换后按样式重新生成", index + 1),
        }
    }

    pub fn en(&self) -> String {
        match self {
            Note::Unreadable { index, .. } => format!("Zotero citation {} could not be read and was left as it is.", index + 1),
            Note::Empty { index } => format!("Zotero citation {}: none of its references can be found. It was left as it is.", index + 1),
            Note::FromDocument { index, .. } => format!("Citation {}: one reference is not in your Pith library; Pith uses the copy stored in this document.", index + 1),
            Note::Missing { index } => format!("Citation {}: one reference has no data anywhere and was dropped.", index + 1),
            Note::Edited { index, .. } => format!("Citation {} had been edited by hand in Zotero; Pith formats it again from the style.", index + 1),
        }
    }
}

pub struct Mapping {
    /// 与输入的域一一对应；None：没有转换（原样留着）
    pub citations: Vec<Option<Converted>>,
    /// 文献库里没有的文献的快照（CSL-JSON，id 已换成快照 ID）
    pub snapshots: BTreeMap<String, Value>,
    pub stats: Stats,
    pub notes: Vec<Note>,
}

fn data_id(data: &Value) -> String {
    let h = Sha256::digest(serde_json::to_string(data).unwrap_or_default().as_bytes());
    format!("zotero-x-{}", h.iter().take(6).map(|b| format!("{b:02x}")).collect::<String>())
}

/// 把 Zotero 引文域（代码）换成 Pith 引文数据。`resolve`：Zotero 条目 → 文献库 uid。
pub fn convert(codes: &[String], resolve: &dyn Fn(&ItemKey) -> Option<String>, now: &str) -> Mapping {
    let mut m = Mapping { citations: Vec::new(), snapshots: BTreeMap::new(), stats: Stats { citations: codes.len(), ..Stats::default() }, notes: Vec::new() };
    for (index, code) in codes.iter().enumerate() {
        let z = match parse_citation(code) {
            Ok(z) => z,
            Err(error) => {
                m.notes.push(Note::Unreadable { index, error });
                m.citations.push(None);
                continue;
            }
        };
        let mut refs = Vec::new();
        for it in &z.items {
            m.stats.items += 1;
            let id = match it.key.as_ref().and_then(resolve) {
                Some(uid) => {
                    m.stats.in_library += 1;
                    uid
                }
                None => match &it.data {
                    Some(data) => {
                        let id = it.key.as_ref().map(ItemKey::snapshot_id).unwrap_or_else(|| data_id(data));
                        let mut snap = data.clone();
                        snap["id"] = Value::String(id.clone());
                        let title = snap["title"].as_str().unwrap_or("（无标题）").to_string();
                        m.snapshots.insert(id.clone(), snap);
                        m.stats.from_document += 1;
                        m.notes.push(Note::FromDocument { index, item: id.clone(), title });
                        id
                    }
                    None => {
                        m.stats.missing += 1;
                        m.notes.push(Note::Missing { index });
                        continue;
                    }
                },
            };
            refs.push(CiteItemRef { id, locator: it.locator.clone(), label: it.label.clone(), extra: it.extra.clone() });
        }
        if refs.is_empty() {
            m.notes.push(Note::Empty { index });
            m.citations.push(None);
            continue;
        }
        if z.edited {
            m.stats.edited += 1;
            m.notes.push(Note::Edited { index, text: z.plain.clone() });
        }
        m.stats.converted += 1;
        m.citations.push(Some(Converted { data: CitationData::new(refs, now), zotero_plain: z.plain, edited: z.edited }));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn code(id: &str, items: &str, props: &str) -> String {
        format!(r#" ADDIN ZOTERO_ITEM CSL_CITATION {{"citationID":"{id}","properties":{{{props}"noteIndex":0}},"citationItems":[{items}],"schema":"https://github.com/citation-style-language/schema/raw/master/csl-citation.json"}} "#)
    }

    #[test]
    fn kinds_and_uris() {
        assert_eq!(field_kind(" ADDIN ZOTERO_ITEM CSL_CITATION {}"), Some(FieldKind::Citation));
        assert_eq!(field_kind("ADDIN ZOTERO_BIBL {\"uncited\":[]} CSL_BIBLIOGRAPHY"), Some(FieldKind::Bibliography));
        assert_eq!(field_kind(" PAGE "), None);
        assert_eq!(parse_uri("http://zotero.org/users/123/items/ABCD2345"), Some(ItemKey { library: Library::User, key: "ABCD2345".into() }));
        assert_eq!(parse_uri("http://zotero.org/users/local/n6bt7wc1/items/ABCD2345").unwrap().library, Library::User);
        assert_eq!(parse_uri("http://zotero.org/groups/77/items/ABCD2345").unwrap().library, Library::Group("77".into()));
        assert_eq!(parse_uri("http://zotero.org/users/1/items/short"), None);
        assert_eq!(parse_uri("http://example.org/x"), None);
        assert_eq!(ItemKey { library: Library::Group("7".into()), key: "K".into() }.snapshot_id(), "zotero-g7-K");
    }

    #[test]
    fn parses_citation_with_locator_and_affixes() {
        let c = code(
            "abc",
            r#"{"id":5,"uris":["http://zotero.org/users/9/items/SYNTH001"],"itemData":{"id":5,"type":"book","title":"T"},"locator":"17","label":"page","prefix":"见","suppress-author":true},{"id":6,"uris":["http://zotero.org/users/9/items/SYNTH002"],"itemData":{"id":6,"type":"book","title":"U"},"suffix":""}"#,
            r#""formattedCitation":"[1]","plainCitation":"[1]","#,
        );
        let z = parse_citation(&c).unwrap();
        assert_eq!(z.zotero_id, "abc");
        assert_eq!(z.plain, "[1]");
        assert!(!z.edited);
        assert_eq!(z.items.len(), 2);
        assert_eq!(z.items[0].locator.as_deref(), Some("17"));
        assert_eq!(z.items[0].extra["prefix"], "见");
        assert_eq!(z.items[0].extra["suppress-author"], true);
        assert!(z.items[1].extra.is_empty(), "空后缀不带");
        let edited = code("e", r#"{"id":1,"uris":["http://zotero.org/users/9/items/SYNTH001"]}"#, r#""plainCitation":"(my text)","dontUpdate":true,"#);
        assert!(parse_citation(&edited).unwrap().edited);
        assert!(parse_citation(" ADDIN ZOTERO_ITEM CSL_CITATION {broken").is_err());
        // 早期格式：没有 CSL_CITATION
        assert!(parse_citation(r#" ADDIN ZOTERO_ITEM {"citationID":"o","citationItems":[{"uri":"http://zotero.org/users/1/items/SYNTH003"}]}"#).unwrap().items[0].key.is_some());
    }

    #[test]
    fn prefs_from_document_properties() {
        let custom = r#"<?xml version="1.0"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="ZOTERO_PREF_2"><vt:lpwstr>/&gt;&lt;pref name="noteType" value="0"/&gt;&lt;/prefs&gt;&lt;/data&gt;</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="ZOTERO_PREF_1"><vt:lpwstr>&lt;data data-version="3"&gt;&lt;session id="s"/&gt;&lt;style id="http://www.zotero.org/styles/apa" locale="en-US" hasBibliography="1"/&gt;&lt;prefs&gt;&lt;pref name="fieldType" value="Field"</vt:lpwstr></property></Properties>"#;
        let joined = prefs_from_custom_xml(custom).unwrap();
        let p = parse_prefs(&joined).unwrap();
        assert_eq!(p.style.as_deref(), Some("http://www.zotero.org/styles/apa"));
        assert_eq!(p.locale.as_deref(), Some("en-US"));
        assert_eq!(p.note_type, Some(0));
        assert_eq!(prefs_from_custom_xml(r#"<Properties xmlns="x"/>"#), None);
        assert_eq!(parse_prefs("<x/>"), None);
    }

    #[test]
    fn maps_to_library_or_document_copy() {
        let codes = vec![
            code("a", r#"{"id":1,"uris":["http://zotero.org/users/9/items/INLIB001"],"itemData":{"id":1,"type":"book","title":"In library"},"locator":"3"}"#, r#""plainCitation":"[1]","#),
            code("b", r#"{"id":2,"uris":["http://zotero.org/users/9/items/NOTLIB01"],"itemData":{"id":2,"type":"book","title":"Only in doc"}},{"id":3,"uris":["http://zotero.org/users/9/items/GONE0001"]}"#, r#""plainCitation":"[2]","#),
            code("c", r#"{"id":3,"uris":["http://zotero.org/users/9/items/GONE0001"]}"#, r#""plainCitation":"[3]","#),
            "ADDIN ZOTERO_ITEM CSL_CITATION nonsense".to_string(),
        ];
        let resolve = |k: &ItemKey| (k.key == "INLIB001").then(|| "uid-1".to_string());
        let m = convert(&codes, &resolve, "2026-11-10T00:00:00Z");
        assert_eq!(m.citations.len(), 4);
        let a = m.citations[0].as_ref().unwrap();
        assert_eq!(a.data.items[0].id, "uid-1");
        assert_eq!(a.data.items[0].locator.as_deref(), Some("3"));
        let b = m.citations[1].as_ref().unwrap();
        assert_eq!(b.data.items.len(), 1, "没有数据的那条去掉");
        assert_eq!(b.data.items[0].id, "zotero-u-NOTLIB01");
        assert_eq!(m.snapshots["zotero-u-NOTLIB01"]["id"], "zotero-u-NOTLIB01");
        assert!(m.citations[2].is_none() && m.citations[3].is_none());
        assert_eq!(m.stats, Stats { citations: 4, converted: 2, items: 4, in_library: 1, from_document: 1, missing: 2, edited: 0 });
        assert!(m.notes.iter().all(|n| n.en().is_ascii()), "Word 里的提示只用 ASCII");
        assert!(matches!(m.notes.last(), Some(Note::Unreadable { index: 3, .. })));
    }
}
