//! Word 全局模板（.dotm）：RibbonX 选项卡 + VBA 工程，放进 Word 启动文件夹后每次启动 Word 自动加载。
//! 文档内容只有一段说明（模板作为加载项时不显示）。生成结果确定：同样的输入得到同样的字节。

use crate::package::Package;
use crate::vba::{self, Project};

const NS_W: &str = crate::NS_W;
const REL_OFFICE_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
/// customUI（2006 版命名空间，Word 2007 起都认）
const REL_UI: &str = "http://schemas.microsoft.com/office/2006/relationships/ui/extensibility";
const REL_VBA: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const REL_SETTINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
pub const RIBBON_NS: &str = "http://schemas.microsoft.com/office/2006/01/customui";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

pub struct Template<'a> {
    pub project: &'a Project,
    /// customUI.xml 全文（命名空间 `RIBBON_NS`）
    pub ribbon: &'a str,
    /// 选项卡用的图片（PNG）：(名字, 字节)，customUI 里用 `image="名字"` 引用
    pub images: &'a [(&'a str, &'a [u8])],
    /// 模板正文里的一段说明（ASCII 以外也可以）
    pub note: &'a str,
}

fn esc(s: &str) -> String {
    crate::xml::escape_text(s)
}

pub fn build_dotm(t: &Template) -> Result<Vec<u8>, String> {
    roxmltree::Document::parse(t.ribbon).map_err(|e| format!("customUI.xml 不是合法的 XML：{e}"))?;
    let mut pkg = Package::new();
    pkg.set(
        "[Content_Types].xml",
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/>"#,
            r#"<Default Extension="png" ContentType="image/png"/>"#,
            r#"<Override PartName="/word/document.xml" ContentType="application/vnd.ms-word.template.macroEnabledTemplate.main+xml"/>"#,
            r#"<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>"#,
            r#"</Types>"#
        )
        .as_bytes()
        .to_vec(),
    );
    pkg.set(
        "_rels/.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL_OFFICE_DOC}" Target="word/document.xml"/><Relationship Id="rIdPithUI" Type="{REL_UI}" Target="customUI/customUI.xml"/></Relationships>"#
        )
        .into_bytes(),
    );
    pkg.set(
        "word/_rels/document.xml.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL_SETTINGS}" Target="settings.xml"/><Relationship Id="rIdVba" Type="{REL_VBA}" Target="vbaProject.bin"/></Relationships>"#
        )
        .into_bytes(),
    );
    pkg.set(
        "word/document.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{NS_W}"><w:body><w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#, esc(t.note)).into_bytes(),
    );
    pkg.set(
        "word/settings.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{NS_W}"><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#).into_bytes(),
    );
    pkg.set("word/vbaProject.bin", vba::build(t.project)?);
    pkg.set("customUI/customUI.xml", t.ribbon.as_bytes().to_vec());
    if !t.images.is_empty() {
        let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
        for (name, bytes) in t.images {
            if !name.chars().all(|c| c.is_ascii_alphanumeric()) || !bytes.starts_with(b"\x89PNG") {
                return Err(format!("选项卡图片 {name} 不对（名字只用字母数字，要 PNG）"));
            }
            rels.push_str(&format!(r#"<Relationship Id="{name}" Type="{REL_IMAGE}" Target="images/{name}.png"/>"#));
            pkg.set(&format!("customUI/images/{name}.png"), bytes.to_vec());
        }
        rels.push_str("</Relationships>");
        pkg.set("customUI/_rels/customUI.xml.rels", rels.into_bytes());
    }
    pkg.write().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vba::{Module, ModuleKind};

    #[test]
    fn dotm_is_deterministic_and_complete() {
        let p = Project::word("Pith", vec![Module { name: "PithMain".into(), kind: ModuleKind::Standard, code: "Public Sub PithRibbon(ByVal control As Object)\nEnd Sub".into() }]);
        let ribbon = format!(r#"<customUI xmlns="{RIBBON_NS}"><ribbon><tabs><tab id="PithTab" label="Pith"/></tabs></ribbon></customUI>"#);
        let png: &[u8] = b"\x89PNG\r\n\x1a\n-synthetic";
        let t = Template { project: &p, ribbon: &ribbon, images: &[("icon", png)], note: "Pith 的 Word 模板" };
        let a = build_dotm(&t).unwrap();
        assert_eq!(a, build_dotm(&t).unwrap());
        let pkg = Package::read(&a).unwrap();
        for part in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/settings.xml", "word/_rels/document.xml.rels", "word/vbaProject.bin", "customUI/customUI.xml", "customUI/images/icon.png", "customUI/_rels/customUI.xml.rels"] {
            assert!(pkg.get(part).is_some(), "{part}");
        }
        assert_eq!(vba::read_back(pkg.get("word/vbaProject.bin").unwrap()).unwrap().modules.len(), 2);
        assert!(pkg.get_str("customUI/_rels/customUI.xml.rels").unwrap().contains(r#"Id="icon""#));
        let bad = Template { project: &p, ribbon: "<customUI", images: &[], note: "" };
        assert!(build_dotm(&bad).is_err());
        let bad_img = Template { project: &p, ribbon: &ribbon, images: &[("a b", png)], note: "" };
        assert!(build_dotm(&bad_img).is_err());
    }
}
