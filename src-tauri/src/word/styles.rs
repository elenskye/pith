//! 引文样式：内置的（清单只写一份：`src/cite/styles.json`，CSL 文件打包进前端）+ 用户导入的 .csl（`数据目录/styles/`，`decisions/0017`）。
//! 导入的样式 ID 就是 CSL 文件里的 `<id>`（URI），与 Zotero 文档设置里记的一致；内置样式用短 ID（`apa`）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use docx_engine::cite::csl_style::{self, StyleMeta};
use serde::{Deserialize, Serialize};

/// 新文档的默认样式：学校要求的 GB/T 7714-2015 顺序编码制（`decisions/0015`）
pub const DEFAULT: &str = "gbt7714-2015-numeric";
/// 数据目录下放导入样式的文件夹
pub const DIR: &str = "styles";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub id: String,
    /// 中文名（App 里显示）；导入的样式是它自己的标题
    pub label: String,
    /// Word 里显示的名字（内置的是英文）
    pub en: String,
    /// 内置：打包的 CSL 文件名；导入：数据目录 styles/ 下的文件名
    pub file: String,
    /// CSL 文件里的 <id>
    pub uri: String,
    /// numeric / author-date / …
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default)]
    pub imported: bool,
    /// 导入的依赖样式：母样式的 ID（内置短 ID 或导入样式的 URI）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

pub fn builtin() -> &'static [Style] {
    static ALL: OnceLock<Vec<Style>> = OnceLock::new();
    ALL.get_or_init(|| serde_json::from_str(include_str!("../../../src/cite/styles.json")).expect("src/cite/styles.json"))
}

pub fn is_builtin(id: &str) -> bool {
    builtin().iter().any(|s| s.id == id)
}

/// 导入样式的文件夹
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR)
}

fn imported_style(file: &str, m: &StyleMeta) -> Style {
    Style {
        id: m.id.clone(),
        label: m.title.clone(),
        en: m.title.clone(),
        file: file.to_string(),
        uri: m.id.clone(),
        kind: m.format.clone().unwrap_or_else(|| "author-date".into()),
        locator: None,
        imported: true,
        parent: m.parent.as_deref().map(|p| builtin().iter().find(|b| b.uri == p).map(|b| b.id.clone()).unwrap_or_else(|| p.to_string())),
    }
}

/// 数据目录里导入的样式（读不懂的文件跳过），按标题排
pub fn imported(data_dir: &Path) -> Vec<Style> {
    let Ok(rd) = std::fs::read_dir(dir(data_dir)) else { return Vec::new() };
    let mut out: Vec<Style> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "csl"))
        .filter_map(|p| {
            let m = csl_style::read_meta(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some(imported_style(&p.file_name()?.to_string_lossy(), &m))
        })
        .collect();
    out.sort_by_key(|s| s.label.to_lowercase());
    out
}

/// 全部样式：内置在前（默认样式第一个），导入的在后
pub fn all(data_dir: &Path) -> Vec<Style> {
    builtin().iter().cloned().chain(imported(data_dir)).collect()
}

pub fn find(data_dir: &Path, id: &str) -> Option<Style> {
    builtin().iter().find(|s| s.id == id).cloned().or_else(|| imported(data_dir).into_iter().find(|s| s.id == id))
}

pub fn exists(data_dir: &Path, id: &str) -> bool {
    find(data_dir, id).is_some()
}

/// Zotero 文档设置里的样式 URI → Pith 样式 ID
pub fn for_uri(data_dir: &Path, uri: &str) -> Option<String> {
    all(data_dir).into_iter().find(|s| s.uri == uri).map(|s| s.id)
}

/// 排版要的样式来源：导入的独立样式给 CSL 全文；依赖样式换成母样式（内置母样式给 ID，前端自己加载）
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub xml: Option<String>,
    /// 母样式是内置的：它的 ID
    pub builtin: Option<String>,
    /// 依赖样式指定的语言
    pub lang: Option<String>,
}

pub fn source(data_dir: &Path, id: &str) -> Result<Source, String> {
    let s = imported(data_dir).into_iter().find(|s| s.id == id).ok_or_else(|| format!("没有这个引文样式：{id}"))?;
    let xml = std::fs::read_to_string(dir(data_dir).join(&s.file)).map_err(|e| format!("读不了样式文件 {}：{e}", s.file))?;
    let meta = csl_style::read_meta(&xml)?;
    let Some(parent) = s.parent else { return Ok(Source { xml: Some(xml), builtin: None, lang: None }) };
    if is_builtin(&parent) {
        return Ok(Source { xml: None, builtin: Some(parent), lang: meta.default_locale });
    }
    let p = imported(data_dir).into_iter().find(|x| x.id == parent && x.parent.is_none()).ok_or_else(|| format!("找不到母样式 {parent}，请先导入它"))?;
    let pxml = std::fs::read_to_string(dir(data_dir).join(&p.file)).map_err(|e| format!("读不了样式文件 {}：{e}", p.file))?;
    Ok(Source { xml: Some(pxml), builtin: None, lang: meta.default_locale })
}

/// 由样式 URI 定一个文件名（只用字母数字与 -）
fn file_name(uri: &str) -> String {
    let last = uri.trim_end_matches('/').rsplit('/').next().unwrap_or("style");
    let clean: String = last.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' }).take(80).collect();
    let clean = clean.trim_matches('-');
    format!("{}.csl", if clean.is_empty() { "style" } else { clean })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub style: Style,
    /// 替换了同一个 <id> 的旧版本
    pub replaced: bool,
}

/// 检查并复制进数据目录（还没试排；调用方试排失败要调 `remove`）
pub fn import(data_dir: &Path, xml: &str) -> Result<Imported, String> {
    let m = csl_style::read_meta(xml)?;
    if let Some(b) = builtin().iter().find(|b| b.uri == m.id) {
        return Err(format!("Pith 已经内置这个样式（{}），不用导入", b.label));
    }
    if let Some(p) = &m.parent
        && !builtin().iter().any(|b| &b.uri == p)
        && !imported(data_dir).iter().any(|s| &s.id == p && s.parent.is_none())
    {
        return Err(format!("这是依赖样式，要先导入它的母样式（{p}）"));
    }
    let d = dir(data_dir);
    std::fs::create_dir_all(&d).map_err(|e| format!("无法创建 {}：{e}", d.display()))?;
    let existing = imported(data_dir).into_iter().find(|s| s.id == m.id);
    let file = match &existing {
        Some(s) => s.file.clone(),
        None => {
            let base = file_name(&m.id);
            let mut f = base.clone();
            let mut n = 2;
            while d.join(&f).exists() {
                f = format!("{}-{n}.csl", base.trim_end_matches(".csl"));
                n += 1;
            }
            f
        }
    };
    let tmp = d.join(format!("{file}.tmp"));
    std::fs::write(&tmp, xml).map_err(|e| format!("无法写入样式文件：{e}"))?;
    std::fs::rename(&tmp, d.join(&file)).map_err(|e| format!("无法写入样式文件：{e}"))?;
    Ok(Imported { style: imported_style(&file, &m), replaced: existing.is_some() })
}

/// 删掉导入的样式。还有依赖它的样式时拒绝。
pub fn remove(data_dir: &Path, id: &str) -> Result<(), String> {
    if is_builtin(id) {
        return Err("内置样式不能删除".into());
    }
    let list = imported(data_dir);
    let s = list.iter().find(|s| s.id == id).ok_or_else(|| format!("没有这个导入的样式：{id}"))?;
    if let Some(child) = list.iter().find(|c| c.parent.as_deref() == Some(id)) {
        return Err(format!("「{}」依赖这个样式，先删掉它", child.label));
    }
    std::fs::remove_file(dir(data_dir).join(&s.file)).map_err(|e| format!("删不掉样式文件：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    #[test]
    fn builtin_list_is_consistent() {
        assert_eq!(builtin()[0].id, DEFAULT, "默认样式排第一");
        let ids: std::collections::BTreeSet<&str> = builtin().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), builtin().len(), "ID 不重复");
        for s in builtin() {
            assert!(s.en.is_ascii(), "Word 里显示的英文名只用 ASCII：{}", s.en);
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/cite/csl").join(&s.file);
            let m = csl_style::read_meta(&std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("缺 CSL 文件：{}", s.file))).unwrap();
            assert_eq!(m.id, s.uri, "styles.json 里的 uri 与 CSL 文件一致：{}", s.file);
            assert!(!s.imported);
        }
        assert_eq!(builtin().iter().filter(|s| s.locator.as_deref() == Some("after-brackets")).count(), 2, "两个 GB/T 顺序编码");
    }

    fn csl(id: &str, title: &str, extra: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0"><info><title>{title}</title><id>{id}</id>{extra}<category citation-format="author-date"/></info>{}</style>"#,
            if extra.contains("independent-parent") { "" } else { "<citation><layout><text variable=\"title\"/></layout></citation>" }
        )
    }

    #[test]
    fn import_list_find_and_remove() {
        let d = temp_dir("styles-import");
        assert!(imported(&d).is_empty());
        let a = import(&d, &csl("http://www.zotero.org/styles/synthetic-journal", "Synthetic Journal", "")).unwrap();
        assert!(!a.replaced && a.style.imported);
        assert_eq!(a.style.file, "synthetic-journal.csl");
        // 同一个 <id> 再导入：替换
        let again = import(&d, &csl("http://www.zotero.org/styles/synthetic-journal", "Synthetic Journal v2", "")).unwrap();
        assert!(again.replaced);
        assert_eq!(imported(&d).len(), 1);
        assert_eq!(find(&d, "http://www.zotero.org/styles/synthetic-journal").unwrap().label, "Synthetic Journal v2");
        // 文件名撞了（不同 <id>）
        let b = import(&d, &csl("http://example.org/other/synthetic-journal", "Another", "")).unwrap();
        assert_eq!(b.style.file, "synthetic-journal-2.csl");
        // 依赖样式：母样式内置 / 导入的 / 没有
        let dep = import(&d, &csl("http://x/dep-apa", "Dep of APA", r#"<link href="http://www.zotero.org/styles/apa" rel="independent-parent"/>"#)).unwrap();
        assert_eq!(dep.style.parent.as_deref(), Some("apa"));
        assert_eq!(source(&d, "http://x/dep-apa").unwrap(), Source { xml: None, builtin: Some("apa".into()), lang: None });
        let dep2 = import(&d, &csl("http://x/dep-syn", "Dep of Syn", r#"<link href="http://www.zotero.org/styles/synthetic-journal" rel="independent-parent"/>"#)).unwrap();
        assert!(source(&d, &dep2.style.id).unwrap().xml.unwrap().contains("Synthetic Journal v2"), "母样式的全文");
        assert!(import(&d, &csl("http://x/orphan", "Orphan", r#"<link href="http://nowhere/parent" rel="independent-parent"/>"#)).unwrap_err().contains("母样式"));
        // 内置的不用导入；Zotero URI 对应
        assert!(import(&d, &csl("http://www.zotero.org/styles/apa", "APA", "")).unwrap_err().contains("内置"));
        assert_eq!(for_uri(&d, "http://www.zotero.org/styles/apa").as_deref(), Some("apa"));
        assert_eq!(for_uri(&d, "http://www.zotero.org/styles/synthetic-journal").as_deref(), Some("http://www.zotero.org/styles/synthetic-journal"));
        assert_eq!(for_uri(&d, "http://www.zotero.org/styles/nature"), None);
        assert!(exists(&d, "apa") && exists(&d, "http://x/dep-apa") && !exists(&d, "nope"));
        assert_eq!(all(&d).len(), builtin().len() + 4);
        // 删除：有依赖的先拒绝；内置的拒绝
        assert!(remove(&d, "http://www.zotero.org/styles/synthetic-journal").unwrap_err().contains("依赖"));
        remove(&d, "http://x/dep-syn").unwrap();
        remove(&d, "http://www.zotero.org/styles/synthetic-journal").unwrap();
        assert!(remove(&d, "apa").is_err());
        assert_eq!(imported(&d).len(), 2);
        assert!(import(&d, "<x/>").is_err());
    }
}
