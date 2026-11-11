//! Word 宏的协议（经本机服务 `POST /v1/word`）。宏把 Word 里的情况（引文控件的 tag、数据部件）发来，
//! Pith 算好每处引文与书目应有的内容，宏照着写回 Word。业务规则（刷新计划、Zotero 对应）在 Rust，排版在 WebView。
//!
//! 请求：一段 JSON，`cmd` 为 ping / pick / refresh / styles / zotero / show。
//! 回复：按行的制表符分隔文本（VBA 好解析）。第一行 `OK` 或 `ERR\t<原因>`。Word 里的文字一律英文（`decisions/0006`）。
//!   P\t<文献 JSON>                      pick：选好的文献（[{id, locator?, label?, …}]）
//!   X                                   pick：取消了
//!   D                                   pick：要删除正在编辑的这处引文
//!   C\t<序号>\t<新tag或空>\t<文字>\t<格式>  refresh：每处引文
//!   B\t<悬挂缩进pt>\t<条数>              书目格式
//!   E\t<文字>\t<格式>                    每条书目
//!   Y\t<样式 ID>\t<名字>\t<1 当前 / 0>    styles：Word 里「Style」菜单的每一项
//!   Z\t<第几个 Zotero 域>\t<新引文 ID>    zotero：这个域换成哪处 Pith 引文（没列出的不换）
//!   M\t<总结>                            zotero：转换结果（一段话）
//!   N\t<提示>                            要告诉用户的事（跳过的引文等）
//!   S\t<数据部件 XML>                    新的数据部件（不带 XML 声明）
//! 文字转义：\\ \t \n \r。格式：「长度:标记」逗号分隔，长度按 UTF-16 计（与 VBA 一致），标记 i 斜体 b 粗体 p 上标 d 下标 c 小型大写 u 下划线。

use std::collections::{BTreeMap, BTreeSet};

use docx_engine::cite::plan::{DocCitation, Render, plan_refresh, referenced_items};
use docx_engine::cite::store::{self, DocStore};
use docx_engine::cite::zotero::{self, ItemKey};
use docx_engine::cite::{CitationData, CiteItemRef, Run};
use serde::Deserialize;
use serde_json::Value;

use super::styles::{self, Style};

/// 置顶搜索窗要的东西
#[derive(Clone, Debug, Default)]
pub struct PickRequest {
    /// 编辑已有引文：它现在的文献（新插入时为 None）
    pub editing: Option<Vec<CiteItemRef>>,
    /// 文档里的文献快照（文献库里没有的文献，显示标题用）
    pub snapshots: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PickOutcome {
    Cancel,
    Items(Vec<CiteItemRef>),
    /// 删除正在编辑的引文
    Remove,
}

pub trait Host {
    fn version(&self) -> String;
    fn library(&self, uids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String>;
    fn renderer(&self) -> &dyn Render;
    /// 打开置顶搜索窗，等用户选完（或取消）
    fn pick(&self, req: PickRequest) -> Result<PickOutcome, String>;
    /// 全部样式（内置 + 导入）
    fn styles(&self) -> Vec<Style>;
    /// Zotero 条目 → 文献库 uid
    fn resolve_zotero(&self, key: &ItemKey) -> Option<String>;
    fn show(&self);
    fn now(&self) -> String;
    fn new_id(&self) -> String;
}

#[derive(Deserialize)]
struct Add {
    #[serde(rename = "citationID")]
    citation_id: String,
    items: Vec<CiteItemRef>,
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
enum Payload {
    Ping,
    Show,
    Pick {
        /// 正在编辑的引文 ID；空：新插入
        #[serde(default)]
        edit: String,
        #[serde(default)]
        store: String,
    },
    Refresh {
        #[serde(default)]
        style: String,
        #[serde(default)]
        store: String,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        add: Option<Add>,
    },
    Styles {
        #[serde(default)]
        store: String,
    },
    Zotero {
        #[serde(default)]
        codes: Vec<String>,
        /// 拼好的 ZOTERO_PREF_1、_2… 文本
        #[serde(default)]
        prefs: String,
        #[serde(default)]
        store: String,
    },
}

pub fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

/// 片段格式：「长度:标记」，长度按 UTF-16
pub fn fmt(runs: &[Run]) -> String {
    runs.iter()
        .map(|r| {
            let mut f = String::new();
            for (on, c) in [(r.italic, 'i'), (r.bold, 'b'), (r.sup, 'p'), (r.sub, 'd'), (r.small_caps, 'c'), (r.underline, 'u')] {
                if on {
                    f.push(c);
                }
            }
            format!("{}:{f}", r.text.encode_utf16().count())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn err(msg: &str) -> String {
    format!("ERR\t{}", esc(msg))
}

const LIBRARY_ERROR: &str = "Pith could not read its library.";

pub fn handle(host: &dyn Host, body: &[u8]) -> String {
    let payload: Payload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Word 请求解析失败：{e}");
            return err("Pith did not understand the request from Word. Please reinstall the Word integration in Pith settings.");
        }
    };
    let r = match payload {
        Payload::Ping => Ok(format!("OK\tPith {}", host.version())),
        Payload::Show => {
            host.show();
            Ok("OK".into())
        }
        Payload::Pick { edit, store } => pick(host, &edit, &store),
        Payload::Refresh { style, store, tags, add } => refresh(host, &style, &store, tags, add),
        Payload::Styles { store } => style_menu(host, &store),
        Payload::Zotero { codes, prefs, store } => from_zotero(host, &codes, &prefs, &store),
    };
    r.unwrap_or_else(|e| err(&e))
}

/// 文档里的数据部件：空 → None；读不了 → 英文原因
fn parse_store(xml: &str) -> Result<Option<DocStore>, String> {
    if xml.trim().is_empty() { Ok(None) } else { store::parse(xml).map(Some).map_err(|e| e.en()) }
}

fn pick(host: &dyn Host, edit: &str, store_xml: &str) -> Result<String, String> {
    let doc = parse_store(store_xml)?;
    let editing = (!edit.is_empty()).then(|| doc.as_ref().and_then(|s| s.citations.get(edit)).map(|c| c.items.clone()).unwrap_or_default());
    let req = PickRequest { editing, snapshots: doc.map(|s| s.items).unwrap_or_default() };
    let out = host.pick(req).map_err(|e| {
        eprintln!("搜索窗出错：{e}");
        "Pith could not open its citation window. Make sure Pith has finished starting, then try again.".to_string()
    })?;
    Ok(match out {
        PickOutcome::Cancel => "OK\nX".into(),
        PickOutcome::Remove => "OK\nD".into(),
        PickOutcome::Items(items) if items.is_empty() => "OK\nX".into(),
        PickOutcome::Items(items) => format!("OK\nP\t{}", esc(&serde_json::to_string(&items).map_err(|e| e.to_string())?)),
    })
}

fn style_menu(host: &dyn Host, store_xml: &str) -> Result<String, String> {
    let current = parse_store(store_xml)?.map(|s| s.style).unwrap_or_else(|| styles::DEFAULT.to_string());
    let mut out = vec!["OK".to_string()];
    for s in host.styles() {
        out.push(format!("Y\t{}\t{}\t{}", esc(&s.id), esc(&s.en), u8::from(s.id == current)));
    }
    Ok(out.join("\n"))
}

fn refresh(host: &dyn Host, style: &str, store_xml: &str, tags: Vec<String>, add: Option<Add>) -> Result<String, String> {
    let now = host.now();
    let mut doc_store = parse_store(store_xml)?.unwrap_or_else(|| DocStore::new(styles::DEFAULT, &now));
    let known = |id: &str| host.styles().iter().any(|s| s.id == id);
    if !style.is_empty() {
        if !known(style) {
            return Err(format!("Unknown citation style: {style}"));
        }
        doc_store.style = style.to_string();
    }
    if !known(&doc_store.style) {
        return Err(format!("This document uses a citation style Pith does not have ({}). Import the .csl file in Pith settings, or choose another style.", doc_store.style));
    }
    if let Some(a) = add {
        // 编辑：保留原来的创建时间
        let created = doc_store.citations.get(&a.citation_id).map(|c| c.created.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| now.clone());
        doc_store.citations.insert(a.citation_id, CitationData::new(a.items, &created));
    }
    let doc: Vec<DocCitation> = tags.into_iter().map(|tag| DocCitation { tag, signature: None }).collect();
    let lib = host.library(&referenced_items(&doc, &doc_store)).map_err(|e| {
        eprintln!("读文献库失败：{e}");
        LIBRARY_ERROR.to_string()
    })?;
    let plan = plan_refresh(&doc, &doc_store, &lib, host.renderer(), &mut || host.new_id(), &now).map_err(|e| {
        eprintln!("引文排版失败：{e}");
        "Pith could not format the citations. Make sure the Pith window has finished loading, then try again.".to_string()
    })?;
    let mut out = vec!["OK".to_string()];
    for c in &plan.citations {
        out.push(format!("C\t{}\t{}\t{}\t{}", c.index, c.retag.as_deref().unwrap_or(""), esc(&c.text), fmt(&c.runs)));
    }
    let paragraphs = plan.bibliography.paragraphs();
    out.push(format!("B\t{}\t{}", plan.bibliography.hanging_indent_pt, paragraphs.len()));
    for p in &paragraphs {
        out.push(format!("E\t{}\t{}", esc(&docx_engine::cite::runs_text(p)), fmt(p)));
    }
    for i in &plan.issues {
        out.push(format!("N\t{}", esc(&i.en())));
    }
    out.push(format!("S\t{}", esc(&store::serialize_bare(&plan.store))));
    Ok(out.join("\n"))
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// Word 里的 Zotero 引文换成 Pith 引文：给每个能换的域一个新引文 ID，数据写进数据部件；宏负责换控件、再刷新一次
fn from_zotero(host: &dyn Host, codes: &[String], prefs: &str, store_xml: &str) -> Result<String, String> {
    let now = host.now();
    let existing = parse_store(store_xml)?;
    let had_store = existing.is_some();
    let all = host.styles();
    let zotero_style = zotero::parse_prefs(prefs).and_then(|p| p.style);
    let mapped = zotero_style.as_deref().and_then(|u| all.iter().find(|s| s.uri == u)).map(|s| s.id.clone());
    let mut doc_store = existing.unwrap_or_else(|| DocStore::new(mapped.as_deref().unwrap_or(styles::DEFAULT), &now));
    let m = zotero::convert(codes, &|k| host.resolve_zotero(k), &now);
    let mut out = vec!["OK".to_string()];
    for (i, c) in m.citations.iter().enumerate() {
        if let Some(c) = c {
            let id = host.new_id();
            doc_store.citations.insert(id.clone(), c.data.clone());
            out.push(format!("Z\t{i}\t{}", esc(&id)));
        }
    }
    for (id, snap) in m.snapshots {
        doc_store.items.entry(id).or_insert(snap);
    }
    let st = &m.stats;
    let mut lines = vec![format!("Converted {} of {} Zotero citation(s).", st.converted, st.citations)];
    lines.push(format!("{} matched your Pith library.", plural(st.in_library, "reference")));
    if st.from_document > 0 {
        lines.push(format!("{} not in your Pith library; Pith uses the copy stored in this document.", if st.from_document == 1 { "1 reference is".to_string() } else { format!("{} references are", st.from_document) }));
    }
    if st.missing > 0 {
        lines.push(format!("{} had no data anywhere and dropped.", plural(st.missing, "reference")));
    }
    if st.edited > 0 {
        lines.push(format!("{} had been edited by hand in Zotero; Pith formats them from the style again.", plural(st.edited, "citation")));
    }
    let left = st.citations - st.converted;
    if left > 0 {
        lines.push(format!("{} could not be converted and stayed as Zotero citations.", plural(left, "citation")));
    }
    let style_name = |id: &str| all.iter().find(|s| s.id == id).map(|s| s.en.clone()).unwrap_or_else(|| id.to_string());
    match (had_store, &zotero_style, &mapped) {
        (true, _, _) => lines.push(format!("Style: {} (this document already had Pith citations).", style_name(&doc_store.style))),
        (false, Some(z), None) => lines.push(format!("Zotero used a style Pith does not have ({z}). Pith uses {} for now; import that .csl file in Pith to switch.", style_name(&doc_store.style))),
        _ => lines.push(format!("Style: {}.", style_name(&doc_store.style))),
    }
    out.push(format!("M\t{}", esc(&lines.join("\n"))));
    out.push(format!("S\t{}", esc(&store::serialize_bare(&doc_store))));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_engine::cite::fixture::{self, NumberedRender};
    use std::cell::Cell;

    struct Fake {
        n: Cell<u32>,
        /// 搜索窗的结果
        outcome: PickOutcome,
        /// 搜索窗收到的请求
        asked: std::cell::RefCell<Option<PickRequest>>,
        shown: Cell<bool>,
    }

    impl Host for Fake {
        fn version(&self) -> String {
            "0.2.0".into()
        }
        fn library(&self, uids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String> {
            fixture::lookup(uids)
        }
        fn renderer(&self) -> &dyn Render {
            &NumberedRender
        }
        fn pick(&self, req: PickRequest) -> Result<PickOutcome, String> {
            *self.asked.borrow_mut() = Some(req);
            Ok(self.outcome.clone())
        }
        fn styles(&self) -> Vec<Style> {
            let mut all = styles::builtin().to_vec();
            all.push(Style { id: "http://x/imported".into(), label: "导入的样式".into(), en: "Imported & co".into(), file: "x.csl".into(), uri: "http://x/imported".into(), kind: "author-date".into(), locator: None, imported: true, parent: None });
            all
        }
        fn resolve_zotero(&self, k: &ItemKey) -> Option<String> {
            fixture::zotero_resolve(k)
        }
        fn show(&self) {
            self.shown.set(true);
        }
        fn now(&self) -> String {
            fixture::NOW.into()
        }
        fn new_id(&self) -> String {
            self.n.set(self.n.get() + 1);
            format!("n{}", self.n.get())
        }
    }

    fn host() -> Fake {
        Fake { n: Cell::new(0), outcome: PickOutcome::Items(vec![CiteItemRef::new("rl-item-0003").with_locator("17")]), asked: Default::default(), shown: Cell::new(false) }
    }

    fn lines(s: &str) -> Vec<Vec<String>> {
        s.split('\n').map(|l| l.split('\t').map(str::to_string).collect()).collect()
    }

    #[test]
    fn ping_show_and_errors() {
        let h = host();
        assert_eq!(handle(&h, br#"{"cmd":"ping"}"#), "OK\tPith 0.2.0");
        assert_eq!(handle(&h, br#"{"cmd":"show"}"#), "OK");
        assert!(h.shown.get());
        assert!(handle(&h, b"not json").starts_with("ERR\t"));
        assert!(handle(&h, br#"{"cmd":"format-my-disk"}"#).starts_with("ERR\t"));
    }

    /// 新插入：搜索窗没有预填；选好的文献（带页码）原样交给宏
    #[test]
    fn pick_new_citation() {
        let h = host();
        assert_eq!(handle(&h, br#"{"cmd":"pick","edit":"","store":""}"#), "OK\nP\t[{\"id\":\"rl-item-0003\",\"locator\":\"17\"}]");
        assert!(h.asked.borrow().as_ref().unwrap().editing.is_none());
        let cancel = Fake { outcome: PickOutcome::Cancel, ..host() };
        assert_eq!(handle(&cancel, br#"{"cmd":"pick"}"#), "OK\nX");
        let none = Fake { outcome: PickOutcome::Items(vec![]), ..host() };
        assert_eq!(handle(&none, br#"{"cmd":"pick"}"#), "OK\nX", "什么都没选等于取消");
    }

    /// 编辑：搜索窗预填这处引文现在的文献与文档快照；可以删除
    #[test]
    fn pick_edits_or_removes_an_existing_citation() {
        let mut s = DocStore::new("apa", fixture::NOW);
        s.citations.insert("c1".into(), CitationData::new(vec![CiteItemRef::new("rl-item-0001").with_locator("5")], fixture::NOW));
        s.items.insert("zotero-u-X".into(), serde_json::json!({ "id": "zotero-u-X", "title": "Only in doc" }));
        let body = serde_json::json!({ "cmd": "pick", "edit": "c1", "store": store::serialize_bare(&s) }).to_string();
        let h = host();
        assert!(handle(&h, body.as_bytes()).starts_with("OK\nP\t"));
        let asked = h.asked.borrow().clone().unwrap();
        assert_eq!(asked.editing.unwrap(), vec![CiteItemRef::new("rl-item-0001").with_locator("5")]);
        assert!(asked.snapshots.contains_key("zotero-u-X"));
        // 文档里没有这处引文的数据（从别的文档粘贴来的）：照样打开，空的，重新选
        let body2 = serde_json::json!({ "cmd": "pick", "edit": "pasted", "store": store::serialize_bare(&s) }).to_string();
        handle(&h, body2.as_bytes());
        assert_eq!(h.asked.borrow().clone().unwrap().editing, Some(vec![]));
        let rm = Fake { outcome: PickOutcome::Remove, ..host() };
        assert_eq!(handle(&rm, body.as_bytes()), "OK\nD");
    }

    /// 编辑已有引文：同一个 ID 换成新的文献，保留创建时间
    #[test]
    fn refresh_with_an_edited_citation_keeps_its_id() {
        let h = host();
        let mut s = DocStore::new("gbt7714-2015-numeric", "2026-01-01T00:00:00Z");
        s.citations.insert("c1".into(), CitationData::new(vec![CiteItemRef::new("rl-item-0001")], "2026-01-01T00:00:00Z"));
        let body = serde_json::json!({ "cmd": "refresh", "store": store::serialize_bare(&s), "tags": ["pith:cite:c1"], "add": { "citationID": "c1", "items": [{ "id": "rl-item-0002", "locator": "9" }] } });
        let l = lines(&handle(&h, body.to_string().as_bytes()));
        assert_eq!(l[1], ["C", "0", "", "[1:9]", "5:p"]);
        let st = store::parse(&l.last().unwrap()[1].replace("\\\\", "\\")).unwrap();
        assert_eq!(st.citations["c1"].items[0].id, "rl-item-0002");
        assert_eq!(st.citations["c1"].created, "2026-01-01T00:00:00Z");
    }

    /// Word 的 Style 菜单：内置 + 导入的，标出当前样式
    #[test]
    fn style_menu_lists_all_and_marks_the_current_one() {
        let h = host();
        let l = lines(&handle(&h, br#"{"cmd":"styles","store":""}"#));
        assert_eq!(l[0], ["OK"]);
        assert_eq!(l[1], ["Y", "gbt7714-2015-numeric", "GB/T 7714-2015 (numeric)", "1"], "新文档默认 GB/T");
        assert_eq!(l.len(), 1 + styles::builtin().len() + 1);
        assert_eq!(l.last().unwrap(), &["Y", "http://x/imported", "Imported & co", "0"]);
        let s = store::serialize_bare(&DocStore::new("apa", fixture::NOW));
        let l = lines(&handle(&h, serde_json::json!({ "cmd": "styles", "store": s }).to_string().as_bytes()));
        assert_eq!(l.iter().filter(|x| x.get(3).map(String::as_str) == Some("1")).map(|x| x[1].as_str()).collect::<Vec<_>>(), ["apa"]);
    }

    /// Word 里转换 Zotero 引文：每个能换的域一个新 ID；样式按 Zotero 的对应；库里没有的用文档自带数据
    #[test]
    fn zotero_conversion_in_word() {
        let h = host();
        let codes: Vec<String> = [("zA", vec![("rl-item-0001", None)]), ("zB", vec![("rl-item-0006", Some("4"))])]
            .iter()
            .map(|(id, items)| {
                let x = fixture::zotero_citation(id, items, "[1]");
                // 取出域代码（instrText 拼起来）
                let xml = format!("<r xmlns:w=\"{}\">{x}</r>", docx_engine::NS_W);
                let doc = roxmltree::Document::parse(&xml).unwrap();
                doc.descendants().filter(|n| n.has_tag_name((docx_engine::NS_W, "instrText"))).filter_map(|n| n.text()).collect()
            })
            .chain(["ADDIN ZOTERO_ITEM CSL_CITATION {broken".to_string()])
            .collect();
        let prefs = r#"<data data-version="3"><style id="http://www.zotero.org/styles/apa"/></data>"#;
        let body = serde_json::json!({ "cmd": "zotero", "codes": codes, "prefs": prefs, "store": "" }).to_string();
        let out = handle(&h, body.as_bytes());
        let l = lines(&out);
        assert_eq!(l[0], ["OK"]);
        assert_eq!(l[1], ["Z", "0", "n1"]);
        assert_eq!(l[2], ["Z", "1", "n2"]);
        let m = &l.iter().find(|x| x[0] == "M").unwrap()[1];
        assert!(m.starts_with("Converted 2 of 3 Zotero citation(s).") && m.contains("Style: APA 7th edition."), "{m}");
        assert!(m.is_ascii());
        let st = store::parse(&l.last().unwrap()[1].replace("\\\\", "\\")).unwrap();
        assert_eq!(st.style, "apa");
        assert_eq!(st.citations["n2"].items[0].id, "zotero-u-SYNTH006");
        assert_eq!(st.citations["n2"].items[0].locator.as_deref(), Some("4"));
        assert!(st.items.contains_key("zotero-u-SYNTH006"));
        // 没有 Zotero 设置、样式对不上：默认 GB/T
        let unknown = serde_json::json!({ "cmd": "zotero", "codes": codes, "prefs": r#"<data><style id="http://www.zotero.org/styles/nature"/></data>"#, "store": "" }).to_string();
        let l = lines(&handle(&h, unknown.as_bytes()));
        assert!(l.iter().any(|x| x[0] == "M" && x[1].contains("nature")));
        assert_eq!(store::parse(&l.last().unwrap()[1].replace("\\\\", "\\")).unwrap().style, styles::DEFAULT);
    }

    /// 新文档插入第一处引文：没有数据部件 → 新建（默认 GB/T 顺序编码），返回引文、书目、数据部件
    #[test]
    fn first_citation_in_a_new_document() {
        let h = host();
        let body = r#"{"cmd":"refresh","style":"","store":"","tags":["pith:cite:c1"],"add":{"citationID":"c1","items":[{"id":"rl-item-0003","locator":"17"}]}}"#;
        let out = handle(&h, body.as_bytes());
        let l = lines(&out);
        assert_eq!(l[0], ["OK"]);
        assert_eq!(l[1], ["C", "0", "", "[1:17]", "6:p"]);
        assert_eq!(l[2], ["B", "28", "1"]);
        assert_eq!(l[3][0], "E");
        assert_eq!(l[3][1], "[1]\\t王晓明, 李华. 合成数据中的政治效能感测量.", "制表符转义；中文姓名合成全名");
        assert_eq!(l[3][2], "13:,13:i,1:", "「[1]\\t王晓明, 李华. 」13 个、题名 13 个、句点 1 个");
        let s = &l[4];
        assert_eq!(s[0], "S");
        let xml = s[1].replace("\\\\", "\\");
        assert!(!xml.starts_with("<?xml"));
        let parsed = store::parse(&xml).unwrap();
        assert_eq!(parsed.style, super::styles::DEFAULT);
        assert!(parsed.items.contains_key("rl-item-0003") && parsed.citations.contains_key("c1"));
    }

    /// 已有数据部件：0-B 旧 tag 换新、复制粘贴的重复引文给新 ID、缺数据的报告（英文）
    #[test]
    fn refresh_existing_document() {
        let h = host();
        let mut s = DocStore::new("apa", fixture::NOW);
        s.citations.insert("a".into(), CitationData::new(vec![CiteItemRef::new("rl-item-0001")], fixture::NOW));
        let body = serde_json::json!({ "cmd": "refresh", "style": "", "store": store::serialize(&s), "tags": ["rl:cite:a", "pith:cite:a", "pith:cite:zz"], "add": null });
        let out = handle(&h, body.to_string().as_bytes());
        let l = lines(&out);
        assert_eq!(l[1][..3], ["C", "0", "pith:cite:a"]);
        assert_eq!(l[2][..3], ["C", "1", "pith:cite:n1"]);
        let notes: Vec<&Vec<String>> = l.iter().filter(|x| x[0] == "N").collect();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|n| n[1].is_ascii()), "Word 里的提示只用 ASCII");
        let xml = l.last().unwrap()[1].replace("\\\\", "\\");
        assert_eq!(store::parse(&xml).unwrap().style, "apa");
    }

    #[test]
    fn style_switch_and_errors() {
        let h = host();
        let body = r#"{"cmd":"refresh","style":"chicago-author-date","store":"","tags":[]}"#;
        let out = handle(&h, body.as_bytes());
        let l = lines(&out);
        assert_eq!(l[1], ["B", "0", "1"]);
        assert_eq!(l[2], ["E", "（暂无引文）", "6:"]);
        assert!(store::parse(&l[3][1].replace("\\\\", "\\")).unwrap().style == "chicago-author-date");
        assert!(handle(&h, br#"{"cmd":"refresh","style":"nope","store":"","tags":[]}"#).starts_with("ERR\tUnknown citation style"));
        let newer = r#"{"cmd":"refresh","store":"<p:citations xmlns:p=\"urn:pith:citations:v1\" version=\"7\"><p:json>{}</p:json></p:citations>","tags":[]}"#;
        assert!(handle(&h, newer.as_bytes()).starts_with("ERR\tThis document was written by a newer version of Pith"));
    }

    #[test]
    fn escaping_and_format() {
        assert_eq!(esc("a\\b\tc\nd\re"), "a\\\\b\\tc\\nd\\re");
        let runs = [Run { text: "𝑥y".into(), italic: true, sup: true, ..Run::default() }, Run::plain("z")];
        assert_eq!(fmt(&runs), "3:ip,1:");
    }
}
