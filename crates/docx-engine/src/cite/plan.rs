//! 刷新计划：输入「文档里按顺序出现的引文控件」与文档数据，输出每处引文和书目应有的内容。
//! 纯函数，不碰 Word；Word 宏（经本机服务）与 Word 外直接改文件两条路都用它。
//! 引文文字由 `Render`（App 里是 WebView 中的 citeproc-js）排出；消歧、编号依赖全文，所以每次整篇一起排。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::DocStore;
use super::{citation_id, runs_signature, runs_text, CiteItemRef, Run, CITE_PREFIX, EMPTY_BIBLIOGRAPHY, LEGACY_CITE_PREFIX};

/// 文档里的一处引文控件
#[derive(Clone, Debug)]
pub struct DocCitation {
    pub tag: String,
    /// 现有文字的签名（`runs_signature`）；不知道（Word 宏每次全部改写）时为 None
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderCitation {
    pub id: String,
    pub items: Vec<CiteItemRef>,
}

/// 交给排版器的整篇引文
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    pub style: String,
    /// CSL-JSON，按 uid
    pub items: BTreeMap<String, Value>,
    pub citations: Vec<RenderCitation>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedBibliography {
    pub entries: Vec<Vec<Run>>,
    #[serde(default)]
    pub hanging_indent: bool,
    /// citeproc 的 second-field-align：false / "flush" / "margin"
    #[serde(default)]
    pub second_field_align: Value,
    #[serde(default)]
    pub max_offset: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rendered {
    /// 引文 ID → 带格式的文字
    pub citations: BTreeMap<String, Vec<Run>>,
    pub bibliography: Option<RenderedBibliography>,
}

/// 按 uid 取文献库里的最新 CSL-JSON（只需返回库里有的）
pub type Library<'a> = &'a dyn Fn(&BTreeSet<String>) -> Result<BTreeMap<String, Value>, String>;

pub trait Render {
    fn render(&self, req: &RenderRequest) -> Result<Rendered, String>;
}

/// 刷新时发现的问题。中文给 App，英文给 Word（`decisions/0006`）。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Issue {
    /// 不是引文控件（跳过）
    NotCitation { index: usize, tag: String },
    /// 文档里缺少这处引文的数据（可能是从别的文档粘贴来的；跳过）
    NoData { index: usize, tag: String },
    /// 引文里的文献全部缺失（跳过）
    AllMissing { index: usize, tag: String },
    /// 与前面重复（复制粘贴），已分配新 ID
    Duplicate { index: usize },
    /// 这条文献在文献库和文档里都找不到（引文里其余文献照常）
    MissingItem { index: usize, item: String },
}

impl Issue {
    pub fn index(&self) -> usize {
        match self {
            Issue::NotCitation { index, .. } | Issue::NoData { index, .. } | Issue::AllMissing { index, .. } | Issue::Duplicate { index } | Issue::MissingItem { index, .. } => *index,
        }
    }

    /// 这处引文被跳过（没有改写）
    pub fn skipped(&self) -> bool {
        matches!(self, Issue::NotCitation { .. } | Issue::NoData { .. } | Issue::AllMissing { .. })
    }

    pub fn zh(&self) -> String {
        let n = self.index() + 1;
        match self {
            Issue::NotCitation { .. } => format!("第 {n} 处：不是引文控件，跳过"),
            Issue::NoData { .. } => format!("第 {n} 处引文：文档里缺少它的数据（可能是从别的文档粘贴来的），跳过"),
            Issue::AllMissing { .. } => format!("第 {n} 处引文：里面的文献全部找不到，跳过"),
            Issue::Duplicate { .. } => format!("第 {n} 处引文与前面重复（复制粘贴），已分配新 ID"),
            Issue::MissingItem { item, .. } => format!("第 {n} 处引文：文献 {item} 在文献库和文档里都找不到"),
        }
    }

    pub fn en(&self) -> String {
        let n = self.index() + 1;
        match self {
            Issue::NotCitation { .. } => format!("Item {n} is not a Pith citation and was skipped."),
            Issue::NoData { .. } => format!("Citation {n} has no data in this document (maybe pasted from another document) and was skipped."),
            Issue::AllMissing { .. } => format!("Citation {n} was skipped: none of its references can be found."),
            Issue::Duplicate { .. } => format!("Citation {n} duplicated an earlier one (copy and paste) and got a new ID."),
            Issue::MissingItem { .. } => format!("Citation {n}: one reference is missing from both Pith and this document."),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationUpdate {
    pub index: usize,
    pub citation_id: String,
    /// 需要改控件 tag 时的新 tag（复制粘贴产生的重复 ID、0-B 的旧前缀）
    pub retag: Option<String>,
    pub runs: Vec<Run>,
    pub text: String,
    pub changed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BibliographyPlan {
    pub entries: Vec<Vec<Run>>,
    pub texts: Vec<String>,
    /// 悬挂缩进（磅）
    pub hanging_indent_pt: f64,
}

impl BibliographyPlan {
    /// 写进书目控件的段落：没有条目时是一段提示
    pub fn paragraphs(&self) -> Vec<Vec<Run>> {
        if self.entries.is_empty() {
            vec![vec![Run::plain(EMPTY_BIBLIOGRAPHY)]]
        } else {
            self.entries.clone()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub citations: Vec<CitationUpdate>,
    pub issues: Vec<Issue>,
    pub bibliography: BibliographyPlan,
    /// 刷新后的文档数据（文献快照更新为文献库里的最新版本）
    pub store: DocStore,
}

/// 文档里这些引文用到的文献 uid（调用方据此去文献库取最新的 CSL-JSON）
pub fn referenced_items(doc: &[DocCitation], store: &DocStore) -> BTreeSet<String> {
    doc.iter()
        .filter_map(|d| citation_id(&d.tag))
        .filter_map(|id| store.citations.get(id))
        .flat_map(|c| c.items.iter().map(|i| i.id.clone()))
        .collect()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 全是中日韩文字（可夹「·」与空白），没有拉丁字母
fn cjk_only(s: &str) -> bool {
    s.chars().any(is_cjk) && s.chars().all(|c| is_cjk(c) || c == '·' || c == '・' || c.is_whitespace())
}

/// 交给排版器前的整理：分成姓、名两栏的中文（日文、韩文）姓名合成一个全名。
/// citeproc 的短格式（APA、Chicago、GB/T 著者-出版年的正文引文）只出姓，中文写作习惯用全名「王晓明」；
/// 长格式本来就是姓名连写，不受影响。文档快照与文献库里的数据不变，只影响这次排版。
pub fn for_render(item: &Value) -> Value {
    let mut v = item.clone();
    if let Some(obj) = v.as_object_mut() {
        for (_, list) in obj.iter_mut() {
            let Some(names) = list.as_array_mut() else { continue };
            for n in names.iter_mut() {
                let (Some(f), Some(g)) = (n.get("family").and_then(Value::as_str), n.get("given").and_then(Value::as_str)) else { continue };
                if n.get("literal").is_none() && cjk_only(f) && cjk_only(g) {
                    let full = format!("{}{}", f.trim(), g.trim());
                    *n = serde_json::json!({ "literal": full });
                }
            }
        }
    }
    v
}

pub fn hanging_indent_pt(bib: Option<&RenderedBibliography>) -> f64 {
    match bib {
        None => 0.0,
        Some(b) if b.hanging_indent => 36.0,
        Some(b) if b.second_field_align.is_string() => ((b.max_offset + 1.0) * 7.0).max(21.0),
        Some(_) => 0.0,
    }
}

/// `library`：文献库里的最新 CSL-JSON（只需含 `referenced_items` 的那些）；没有的退回文档快照。
pub fn plan_refresh(
    doc: &[DocCitation],
    store: &DocStore,
    library: &BTreeMap<String, Value>,
    render: &dyn Render,
    new_id: &mut dyn FnMut() -> String,
    now: &str,
) -> Result<Plan, String> {
    let mut store = store.clone();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    struct Ordered {
        index: usize,
        id: String,
        retag: Option<String>,
        items: Vec<CiteItemRef>,
    }
    let mut ordered: Vec<Ordered> = Vec::new();

    for (index, dc) in doc.iter().enumerate() {
        let Some(mut id) = citation_id(&dc.tag).map(str::to_string) else {
            issues.push(Issue::NotCitation { index, tag: dc.tag.clone() });
            continue;
        };
        let Some(data) = store.citations.get(&id).cloned() else {
            issues.push(Issue::NoData { index, tag: dc.tag.clone() });
            continue;
        };
        let mut retag = dc.tag.starts_with(LEGACY_CITE_PREFIX).then(|| format!("{CITE_PREFIX}{id}"));
        if seen.contains(&id) {
            let nid = new_id();
            store.citations.insert(nid.clone(), data.clone());
            retag = Some(format!("{CITE_PREFIX}{nid}"));
            issues.push(Issue::Duplicate { index });
            id = nid;
        }
        seen.insert(id.clone());
        let mut items = Vec::new();
        for r in &data.items {
            if let Some(lib) = library.get(&r.id) {
                store.items.insert(r.id.clone(), lib.clone());
            }
            if store.items.contains_key(&r.id) {
                items.push(r.clone());
            } else {
                issues.push(Issue::MissingItem { index, item: r.id.clone() });
            }
        }
        if items.is_empty() {
            issues.push(Issue::AllMissing { index, tag: dc.tag.clone() });
            continue;
        }
        ordered.push(Ordered { index, id, retag, items });
    }

    let rendered = if ordered.is_empty() {
        Rendered::default()
    } else {
        let used: BTreeSet<&str> = ordered.iter().flat_map(|o| o.items.iter().map(|i| i.id.as_str())).collect();
        let req = RenderRequest {
            style: store.style.clone(),
            items: store.items.iter().filter(|(k, _)| used.contains(k.as_str())).map(|(k, v)| (k.clone(), for_render(v))).collect(),
            citations: ordered.iter().map(|o| RenderCitation { id: o.id.clone(), items: o.items.clone() }).collect(),
        };
        render.render(&req)?
    };

    let citations = ordered
        .into_iter()
        .map(|o| {
            let runs = rendered.citations.get(&o.id).cloned().unwrap_or_default();
            let changed = o.retag.is_some() || doc[o.index].signature.as_deref() != Some(runs_signature(&runs).as_str());
            CitationUpdate { index: o.index, citation_id: o.id, retag: o.retag, text: runs_text(&runs), runs, changed }
        })
        .collect();

    let bib = rendered.bibliography.as_ref();
    let entries: Vec<Vec<Run>> = bib.map(|b| b.entries.clone()).unwrap_or_default();
    store.updated = now.to_string();
    Ok(Plan {
        citations,
        issues,
        bibliography: BibliographyPlan { texts: entries.iter().map(|e| runs_text(e)).collect(), hanging_indent_pt: hanging_indent_pt(bib), entries },
        store,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cjk_two_part_names_become_full_names() {
        let item = json!({
            "id": "x", "title": "t",
            "author": [{ "family": "王", "given": "晓明" }, { "family": "Lindqvist", "given": "Mara" }, { "literal": "张伟" }, { "family": "欧阳", "given": "修 " }],
            "editor": [{ "family": "李", "given": "Hua" }],
            "translator": [{ "family": "佐藤", "given": "花子" }]
        });
        let v = for_render(&item);
        assert_eq!(v["author"], json!([{ "literal": "王晓明" }, { "family": "Lindqvist", "given": "Mara" }, { "literal": "张伟" }, { "literal": "欧阳修" }]));
        assert_eq!(v["editor"], json!([{ "family": "李", "given": "Hua" }]), "姓名里有拉丁字母的不动");
        assert_eq!(v["translator"], json!([{ "literal": "佐藤花子" }]));
        assert_eq!(v["title"], "t");
        assert_eq!(item["author"][0]["family"], "王", "原数据不变");
    }
}
