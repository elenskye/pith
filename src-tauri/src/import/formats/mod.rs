//! 非 Zotero 来源（BibTeX / RIS / CSL JSON）的解析：文件文本 → 统一的 `Record`。
//! 只做解析与映射，不碰数据库；写入、去重、日期排序值由 `import` 模块统一处理。
//!
//! 词汇约定：类型名、字段名、作者角色一律用 **Zotero 的**（与 Zotero 导入的条目同一套，v0.2 生成 CSL 只认一套）。
//! 映射不到的原始字段以「格式:原名」作键保存（`bibtex:howpublished`、`ris:M3`、`csl:genre`），保证不丢。

use std::collections::BTreeMap;

mod bibtex;
mod common;
mod csl_json;
mod ris;

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Zotero 类型名：journalArticle / book / bookSection / conferencePaper / thesis / report / webpage / document / …
    pub item_type: String,
    /// Zotero 字段名 → 值（title、publicationTitle、volume、issue、pages、DOI、ISBN、ISSN、url、abstractNote、
    /// publisher、place、language、extra、citationKey……）。**不含 date**（见 `date`）。
    /// 映射不到的原始字段用「bibtex:原名」「ris:标签」「csl:变量」作键。
    pub fields: BTreeMap<String, String>,
    pub date: Option<RecordDate>,
    /// 按原顺序
    pub creators: Vec<RecordCreator>,
    /// 去掉首尾空白、去重，保持原顺序
    pub tags: Vec<String>,
    /// 子笔记，纯文本（写入层转成 HTML 段落）
    pub notes: Vec<String>,
    /// 来源里的记录 ID：BibTeX citekey、RIS 的 ID、CSL JSON 的 id
    pub source_key: Option<String>,
    /// 这条记录的原始文本（整条 BibTeX 条目 / RIS 记录的各行 / CSL JSON 对象序列化），写入层存进 source_meta
    pub raw: String,
    /// 这条记录的解析提示（中文，给用户看），例如「作者列表里的 others 已忽略」「日期无法识别」
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordDate {
    /// 原文，显示用（"March 2019"、"2019-03-15"、"2019///"）
    pub raw: String,
    /// 来源本身给了结构化的年月日时填（CSL date-parts、BibTeX year + month、biblatex date）；
    /// 只有原文时留空，写入层按原文解析
    pub year: Option<i32>,
    /// 1–12
    pub month: Option<u32>,
    /// 1–31
    pub day: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordCreator {
    /// Zotero 角色：author / editor / translator / bookAuthor / seriesEditor / contributor / …
    pub role: String,
    pub family: Option<String>,
    pub given: Option<String>,
    /// 单栏姓名：机构名、不拆分的中文姓名。有 literal 时 family / given 为空
    pub literal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub records: Vec<Record>,
    /// 文件层面的问题（跳过的坏条目、无法识别的段落），带位置（第几条 / 第几行），中文
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bibtex,
    Ris,
    CslJson,
}

impl Format {
    /// 写进 items.source / import_runs.source 的值
    pub fn source_name(self) -> &'static str {
        match self {
            Format::Bibtex => "bibtex",
            Format::Ris => "ris",
            Format::CslJson => "csl-json",
        }
    }
}

/// 按扩展名判断，扩展名不认识时看内容开头。认不出返回 None。
pub fn detect_format(file_name: &str, text: &str) -> Option<Format> {
    let ext = std::path::Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "bib" | "bibtex" => return Some(Format::Bibtex),
        "ris" => return Some(Format::Ris),
        "json" | "csljson" => return Some(Format::CslJson),
        _ => {}
    }
    let body = strip_bom(text).trim_start();
    if body.starts_with('[') || body.starts_with('{') {
        Some(Format::CslJson)
    } else if body.lines().any(|l| ris::tag_line(l).is_some_and(|(tag, _)| tag == "TY")) {
        Some(Format::Ris)
    } else if bibtex::has_entry_head(body) {
        Some(Format::Bibtex)
    } else {
        None
    }
}

/// 整个文件解析失败（不是文本、JSON 语法错误等）才返回 Err；单条坏记录跳过并记进 `Parsed::warnings`。
pub fn parse(format: Format, text: &str) -> Result<Parsed, String> {
    let text = strip_bom(text);
    if text.trim().is_empty() {
        return Err("文件是空的，没有可导入的内容".into());
    }
    match format {
        Format::Bibtex => bibtex::parse(text),
        Format::Ris => ris::parse(text),
        Format::CslJson => csl_json::parse(text),
    }
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_by_extension_then_content() {
        assert_eq!(detect_format("refs.BIB", ""), Some(Format::Bibtex));
        assert_eq!(detect_format("a/b/refs.bibtex", "TY  - JOUR"), Some(Format::Bibtex));
        assert_eq!(detect_format("export.RIS", ""), Some(Format::Ris));
        assert_eq!(detect_format("items.json", ""), Some(Format::CslJson));
        assert_eq!(detect_format("items.csljson", ""), Some(Format::CslJson));

        assert_eq!(detect_format("x.txt", "\u{feff}  \n [{\"id\": 1}]"), Some(Format::CslJson));
        assert_eq!(detect_format("x.txt", "{\"items\": []}"), Some(Format::CslJson));
        assert_eq!(detect_format("x.txt", "Provider: X\r\n\r\nTY  - JOUR\r\nER  - \r\n"), Some(Format::Ris));
        assert_eq!(detect_format("x.txt", "TY - BOOK\nER -"), Some(Format::Ris));
        assert_eq!(detect_format("x", "% comment\n@Article{key,\n title={T}}"), Some(Format::Bibtex));
        assert_eq!(detect_format("x", "@book (key, title={T})"), Some(Format::Bibtex));
        assert_eq!(detect_format("x.txt", "联系 me@example.com 获取资料"), None);
        assert_eq!(detect_format("noext", "just some text"), None);
        assert_eq!(detect_format("x.txt", ""), None);
    }

    #[test]
    fn parse_rejects_blank_and_dispatches() {
        for f in [Format::Bibtex, Format::Ris, Format::CslJson] {
            assert!(parse(f, "\u{feff} \n\t").unwrap_err().contains("空"));
        }
        assert_eq!(parse(Format::Bibtex, "@misc{k, title={T}}").unwrap().records[0].item_type, "document");
        assert_eq!(parse(Format::Ris, "TY  - JOUR\nER  -").unwrap().records[0].item_type, "journalArticle");
        assert_eq!(parse(Format::CslJson, "\u{feff}[{\"type\": \"book\"}]").unwrap().records[0].item_type, "book");
        assert_eq!(Format::CslJson.source_name(), "csl-json");
    }

    /// 用户文件是任意文本：截断在任意位置、各种畸形输入都只能返回 Ok / Err，不能 panic 或卡死
    #[test]
    fn truncated_and_garbage_input_never_panics() {
        let samples = [
            "@string{j = {J}}\n@article{a, author = {Smith, John and 王小明 and others}, title = {\\emph{T} {\\\"u}}, journal = j # { X},\n\
             year = 2019, month = mar, pages = {1--2}, crossref = {b}}\n@book(b, title = \"B\", date = {2019-03-15/2020})",
            "TY  - JOUR\r\nAU  - Smith, John\r\nPY  - 2019/03/15/\r\nSP  - 1\r\nEP  - 2\r\nKW  - a; b\r\nER  - \r\nTY  - BOOK\r\nSN  - 978\r\n",
            r#"[{"id": 1, "type": "book", "author": [{"family": "A", "given": "B"}, {"literal": "C"}], "issued": {"date-parts": [["2019", 3]]}}, 5]"#,
        ];
        let garbage = [
            "@",
            "@a{",
            "@a{k,",
            "@a{k, t = {\\",
            "@a{k, author = { and and }}",
            "@a{k, t = \"x}",
            "@a{k, year = {99999999999}, month = {0}, date = {-0031-07%}}",
            "@a{k, t = {$x}",
            "TY  -",
            "ER  -\nTY - ",
            "{\"issued\": {\"date-parts\": [[]]}, \"author\": [null, 1, \"\", {}]}",
            "[{\"title\": 1e400}]",
            "[{\"type\": \"book\", \"issued\": {\"date-parts\": [[99999999999, -1, 400]]}}]",
        ];
        let formats = [Format::Bibtex, Format::Ris, Format::CslJson];
        for s in samples {
            for (i, _) in s.char_indices() {
                for f in formats {
                    let _ = parse(f, &s[..i]);
                }
            }
        }
        for s in garbage {
            for f in formats {
                let _ = parse(f, s);
            }
        }
    }
}
