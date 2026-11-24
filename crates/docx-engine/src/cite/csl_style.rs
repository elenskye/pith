//! CSL 样式文件（.csl）的元数据：导入用户自己的样式时检查、登记（`decisions/0017`）。纯函数。

use serde::Serialize;

pub const CSL_NS: &str = "http://purl.org/net/xbiblio/csl";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleMeta {
    /// `<info><id>`：样式的 URI，Zotero 文档设置里记的就是它
    pub id: String,
    pub title: String,
    /// 依赖样式（只有 info，借用母样式的排法）的母样式 URI
    pub parent: Option<String>,
    pub default_locale: Option<String>,
    /// `<category citation-format>`：numeric / author-date / author / label / note
    pub format: Option<String>,
}

fn el<'a, 'i>(parent: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    parent.children().find(|n| n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(CSL_NS))
}

/// 读出元数据并检查是不是能用的 CSL 1.0 样式
pub fn read_meta(xml: &str) -> Result<StyleMeta, String> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|e| format!("不是合法的 XML：{e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "style" || root.tag_name().namespace() != Some(CSL_NS) {
        return Err("不是 CSL 样式文件（根元素不是 CSL 的 <style>）".into());
    }
    let info = el(root, "info").ok_or("样式缺少 <info>")?;
    let text = |name: &str| el(info, name).and_then(|n| n.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let id = text("id").ok_or("样式缺少 <id>")?;
    let title = text("title").ok_or("样式缺少 <title>")?;
    let parent = info
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "link" && n.attribute("rel") == Some("independent-parent"))
        .and_then(|n| n.attribute("href"))
        .map(str::to_string);
    let format = info.children().filter(|n| n.is_element() && n.tag_name().name() == "category").find_map(|n| n.attribute("citation-format")).map(str::to_string);
    if parent.is_none() && el(root, "citation").is_none() {
        return Err("样式缺少 <citation>，排不出引文".into());
    }
    if format.as_deref() == Some("note") {
        return Err("这是脚注样式；Pith 暂不支持脚注引文".into());
    }
    Ok(StyleMeta { id, title, parent, default_locale: root.attribute("default-locale").map(str::to_string), format })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(info: &str, body: &str, attrs: &str) -> String {
        format!(r#"<?xml version="1.0" encoding="utf-8"?><style xmlns="{CSL_NS}" class="in-text" version="1.0" {attrs}><info>{info}</info>{body}</style>"#)
    }

    #[test]
    fn reads_independent_and_dependent_styles() {
        let ind = style(
            r#"<title>Synthetic Journal</title><id>http://www.zotero.org/styles/synthetic-journal</id><category citation-format="author-date"/>"#,
            "<citation><layout><text variable=\"title\"/></layout></citation>",
            r#"default-locale="en-GB""#,
        );
        let m = read_meta(&ind).unwrap();
        assert_eq!((m.id.as_str(), m.title.as_str(), m.format.as_deref(), m.default_locale.as_deref()), ("http://www.zotero.org/styles/synthetic-journal", "Synthetic Journal", Some("author-date"), Some("en-GB")));
        assert!(m.parent.is_none());
        let dep = style(r#"<title>Child</title><id>http://x/child</id><link href="http://www.zotero.org/styles/apa" rel="independent-parent"/><category citation-format="author-date"/>"#, "", "");
        assert_eq!(read_meta(&dep).unwrap().parent.as_deref(), Some("http://www.zotero.org/styles/apa"));
    }

    #[test]
    fn rejects_what_pith_cannot_use() {
        assert!(read_meta("not xml").unwrap_err().contains("XML"));
        assert!(read_meta("<style/>").unwrap_err().contains("不是 CSL"));
        assert!(read_meta(&style("<title>T</title>", "<citation/>", "")).unwrap_err().contains("<id>"));
        assert!(read_meta(&style("<id>i</id><title>T</title>", "", "")).unwrap_err().contains("<citation>"));
        assert!(read_meta(&style(r#"<id>i</id><title>T</title><category citation-format="note"/>"#, "<citation/>", "")).unwrap_err().contains("脚注"));
    }

    /// 打包进 App 的样式都能读
    #[test]
    fn bundled_styles_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/cite/csl");
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let m = read_meta(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert!(m.id.starts_with("http://www.zotero.org/styles/"), "{}", m.id);
        }
    }
}
