//! Pith 引文（`decisions/0006`、`0016`）：Word 内外共用的数据格式与刷新逻辑。
//!
//! - `store`：文档里的数据部件（Custom XML，内含 JSON：样式、每处引文的文献与页码、文献快照）
//! - `plan`：刷新计划（纯函数）：文档里按顺序出现的引文 + 数据 + 文献库 → 每处引文与书目应有的内容
//! - `docx`：Word 外直接改 .docx（内容控件、数据部件）；只改我们自己的控件与数据部件
//! - `check`：独立核对（roxmltree，与写入用的不是同一套解析器）
//! - `zotero`：Zotero 引文域的识别与解析、按条目 key 对应到文献库（纯函数）
//! - `convert`：Word 外把 .docx 里的 Zotero 引文与书目换成 Pith 的控件（写新文件，写完核对）
//! - `csl_style`：用户导入的 .csl 的元数据与检查
//! - `node`：在 Node 里跑前端的排版器（citeproc-js），给命令行与真实数据测试用（App 里用 WebView）
//!
//! 引文文字由 citeproc-js 排出（App 的 WebView 里），这里通过 `plan::Render` 拿到带格式的片段。

pub mod check;
pub mod convert;
pub mod csl_style;
pub mod docx;
pub mod fixture;
pub mod node;
pub mod plan;
pub mod store;
pub mod zotero;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 行内引文控件的 tag 前缀：`pith:cite:<引文 ID>`
pub const CITE_PREFIX: &str = "pith:cite:";
/// 参考文献表（块级控件）的 tag
pub const BIB_TAG: &str = "pith:bibliography";
/// Phase 0-B 原型写的旧名：照样识别，刷新时换成新名
pub const LEGACY_CITE_PREFIX: &str = "rl:cite:";
pub const LEGACY_BIB_TAG: &str = "rl:bibliography";

/// 书目里还没有条目时显示的文字
pub const EMPTY_BIBLIOGRAPHY: &str = "（暂无引文）";

/// tag 对应的引文 ID（新旧前缀都认）
pub fn citation_id(tag: &str) -> Option<&str> {
    tag.strip_prefix(CITE_PREFIX).or_else(|| tag.strip_prefix(LEGACY_CITE_PREFIX)).filter(|id| !id.is_empty())
}

pub fn is_bibliography(tag: &str) -> bool {
    tag == BIB_TAG || tag == LEGACY_BIB_TAG
}

/// 是我们的控件（引文或书目）：Word 外写入与核对都不进它们里面找别的控件
pub fn is_ours(tag: &str) -> bool {
    citation_id(tag).is_some() || is_bibliography(tag)
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 一段带格式的文字。citeproc 输出的 HTML 在 WebView 里拆成这种片段再交给 Rust。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sup: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sub: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub small_caps: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Run { text: text.into(), ..Run::default() }
    }

    pub fn same_format(&self, o: &Run) -> bool {
        (self.italic, self.bold, self.sup, self.sub, self.small_caps, self.underline) == (o.italic, o.bold, o.sup, o.sub, o.small_caps, o.underline)
    }
}

pub fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// 比较文档现有内容与应有内容用：只比文字与上下标（与 0-B 原型相同）。
pub fn runs_signature(runs: &[Run]) -> String {
    runs.iter()
        .map(|r| if r.sup { format!("^{{{}}}", r.text) } else if r.sub { format!("_{{{}}}", r.text) } else { r.text.clone() })
        .collect()
}

/// 相邻同格式的片段合并
pub fn merge_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.same_format(&r) => last.text.push_str(&r.text),
            _ => out.push(r),
        }
    }
    out
}

/// 引文里的一条文献：文献 uid（CSL id）+ 可选页码。其余字段（前后缀等，以后用）原样保留。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CiteItemRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl CiteItemRef {
    pub fn new(id: impl Into<String>) -> Self {
        CiteItemRef { id: id.into(), locator: None, label: None, extra: Map::new() }
    }

    pub fn with_locator(mut self, locator: &str) -> Self {
        self.locator = Some(locator.to_string());
        self
    }
}

/// 一处引文（一个内容控件）对应的数据
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CitationData {
    pub items: Vec<CiteItemRef>,
    #[serde(default)]
    pub created: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl CitationData {
    pub fn new(items: Vec<CiteItemRef>, created: &str) -> Self {
        CitationData { items, created: created.to_string(), extra: Map::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(citation_id("pith:cite:abc"), Some("abc"));
        assert_eq!(citation_id("rl:cite:abc"), Some("abc"));
        assert_eq!(citation_id("pith:cite:"), None);
        assert_eq!(citation_id("other"), None);
        assert!(is_bibliography("pith:bibliography") && is_bibliography("rl:bibliography"));
        assert!(is_ours("rl:cite:x") && !is_ours("zotero"));
    }

    #[test]
    fn signature_and_merge() {
        let runs = merge_runs(vec![Run::plain("a"), Run::plain("b"), Run { text: "1".into(), sup: true, ..Run::default() }, Run::plain("")]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs_signature(&runs), "ab^{1}");
        assert_eq!(runs_text(&runs), "ab1");
        let json = serde_json::to_string(&runs[1]).unwrap();
        assert_eq!(json, r#"{"text":"1","sup":true}"#);
        let back: Run = serde_json::from_str(r#"{"text":"x","smallCaps":true}"#).unwrap();
        assert!(back.small_caps && !back.italic);
    }

    #[test]
    fn item_refs_keep_unknown_fields() {
        let r: CiteItemRef = serde_json::from_str(r#"{"id":"u1","locator":"17","prefix":"见"}"#).unwrap();
        assert_eq!(r.locator.as_deref(), Some("17"));
        assert_eq!(serde_json::to_value(&r).unwrap()["prefix"], "见");
    }
}
