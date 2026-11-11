//! 条目字段的规则（导入与编辑共用，只在这里写一份）：全部原始字段存在 `item_fields`，常用列由字段推出。
//! 编辑时先改字段，再 `refresh_columns` 重算列，列与字段永远一致。

use crate::common::err;
use crate::text;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

/// 由字段推出的常用列（排序、搜索、去重、列表显示）。
#[derive(Debug, Default, PartialEq)]
pub struct Columns {
    pub title: Option<String>,
    pub title_norm: Option<String>,
    pub date_raw: Option<String>,
    pub date_sort: Option<String>,
    pub year: Option<i64>,
    pub container_title: Option<String>,
    pub doi: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub detected_lang: Option<String>,
    pub abstract_text: Option<String>,
    pub extra: Option<String>,
}

/// Zotero 的「基础字段」在部分类型里换了名字（案例的 caseName、法规的 dateEnacted……），按顺序取第一个非空的。
pub const TITLE_FIELDS: &[&str] = &["title", "caseName", "nameOfAct", "subject"];
pub const DATE_FIELDS: &[&str] = &["date", "dateDecided", "dateEnacted", "issueDate"];
pub const CONTAINER_FIELDS: &[&str] = &[
    "publicationTitle", "bookTitle", "proceedingsTitle", "websiteTitle", "blogTitle", "forumTitle", "encyclopediaTitle", "dictionaryTitle",
    "programTitle", "seriesTitle", "reporter", "code",
];

pub fn derive_columns(f: &BTreeMap<String, String>) -> Columns {
    let first = |names: &[&str]| names.iter().filter_map(|n| f.get(*n)).map(|v| v.trim()).find(|v| !v.is_empty()).map(String::from);
    let title = first(TITLE_FIELDS);
    let (date_sort, date_raw) = match first(DATE_FIELDS) {
        Some(v) => {
            let (sort, raw) = text::split_multipart(&v);
            // 不是 Zotero 两段式（手动、其他来源）：按原文解析
            let sort = sort.or_else(|| text::parse_date(&raw).sort_key());
            (sort.filter(|s| s != "0000-00-00"), Some(raw).filter(|r| !r.is_empty()))
        }
        None => (None, None),
    };
    let year = date_sort.as_deref().and_then(text::year_of);
    let extra = first(&["extra"]);
    // 没有 DOI 字段的类型，Zotero 习惯写在 extra 的「DOI: …」行里
    let doi = first(&["DOI"]).or_else(|| extra.as_deref().and_then(|e| e.lines().find_map(|l| l.trim().strip_prefix("DOI:").map(|d| d.trim().to_string()))));
    let probe = [title.as_deref(), first(CONTAINER_FIELDS).as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    Columns {
        title_norm: title.as_deref().map(text::title_norm).filter(|t| !t.is_empty()),
        detected_lang: text::detect_lang(&probe).or_else(|| first(&["abstractNote"]).as_deref().and_then(text::detect_lang)).map(String::from),
        title,
        date_raw,
        date_sort,
        year,
        container_title: first(CONTAINER_FIELDS),
        doi: doi.as_deref().and_then(text::normalize_doi),
        isbn: first(&["ISBN"]).as_deref().and_then(text::normalize_isbn),
        language: first(&["language"]),
        abstract_text: first(&["abstractNote"]),
        extra,
    }
}

/// 这个类型的「期刊 / 书名」写在哪个字段（Zotero 的叫法）；没有这个概念的类型（图书、学位论文……）返回 None。
pub fn natural_container(item_type: &str) -> Option<&'static str> {
    Some(match item_type {
        "journalArticle" | "magazineArticle" | "newspaperArticle" => "publicationTitle",
        "bookSection" => "bookTitle",
        "conferencePaper" => "proceedingsTitle",
        "webpage" => "websiteTitle",
        "blogPost" => "blogTitle",
        "forumPost" => "forumTitle",
        "encyclopediaArticle" => "encyclopediaTitle",
        "dictionaryEntry" => "dictionaryTitle",
        "radioBroadcast" | "tvBroadcast" => "programTitle",
        _ => return None,
    })
}

fn first_present<'a>(names: &[&'a str], f: &BTreeMap<String, String>) -> Option<&'a str> {
    names.iter().copied().find(|n| f.get(*n).is_some_and(|v| !v.trim().is_empty()))
}

/// 标题写在哪个字段：已有的优先（案例是 caseName……），否则 title。
pub fn title_field(f: &BTreeMap<String, String>) -> &'static str {
    first_present(TITLE_FIELDS, f).unwrap_or("title")
}

pub fn date_field(f: &BTreeMap<String, String>) -> &'static str {
    first_present(DATE_FIELDS, f).unwrap_or("date")
}

/// 「期刊 / 书名」写在哪个字段：已有值的优先（导入来的数据可能用了别的字段），否则按类型。
pub fn container_field(item_type: &str, f: &BTreeMap<String, String>) -> Option<&'static str> {
    first_present(CONTAINER_FIELDS, f).or_else(|| natural_container(item_type))
}

pub fn load(c: &Connection, item_id: i64) -> Result<BTreeMap<String, String>, String> {
    let mut st = c.prepare_cached("SELECT field, value FROM item_fields WHERE item_id = ?1").map_err(err)?;
    st.query_map([item_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 按 `item_fields` 重算这条文献的常用列，并更新 `updated_at`。
pub fn refresh_columns(c: &Connection, item_id: i64, now: &str) -> Result<(), String> {
    let cols = derive_columns(&load(c, item_id)?);
    c.execute(
        "UPDATE items SET title = ?1, title_norm = ?2, date_raw = ?3, date_sort = ?4, year = ?5, container_title = ?6, doi = ?7, isbn = ?8,
                          language = ?9, detected_lang = ?10, abstract = ?11, extra = ?12, updated_at = ?13 WHERE id = ?14",
        params![
            cols.title,
            cols.title_norm,
            cols.date_raw,
            cols.date_sort,
            cols.year,
            cols.container_title,
            cols.doi,
            cols.isbn,
            cols.language,
            cols.detected_lang,
            cols.abstract_text,
            cols.extra,
            now,
            item_id
        ],
    )
    .map_err(err)?;
    Ok(())
}

/// 显示用：日期字段的原文部分（两段式去掉排序值）。
pub fn display_value(field: &str, value: &str) -> String {
    if DATE_FIELDS.contains(&field) { text::split_multipart(value).1 } else { value.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn columns_from_zotero_fields() {
        let c = derive_columns(&f(&[
            ("title", "Synthetic Panel Study of Efficacy"),
            ("date", "2019-03-00 March 2019"),
            ("publicationTitle", "Journal of Synthetic Politics"),
            ("DOI", "https://doi.org/10.0000/X"),
            ("language", "en"),
            ("abstractNote", "Abstract"),
        ]));
        assert_eq!(c.title.as_deref(), Some("Synthetic Panel Study of Efficacy"));
        assert_eq!(c.title_norm.as_deref(), Some("synthetic panel study of efficacy"));
        assert_eq!((c.date_sort.as_deref(), c.date_raw.as_deref(), c.year), (Some("2019-03-00"), Some("March 2019"), Some(2019)));
        assert_eq!(c.container_title.as_deref(), Some("Journal of Synthetic Politics"));
        assert_eq!(c.doi.as_deref(), Some("10.0000/X"));
        assert_eq!(c.detected_lang.as_deref(), Some("en"));

        let zh = derive_columns(&f(&[("title", "论分工"), ("bookTitle", "国民财富的性质和原因的研究"), ("ISBN", "978-0-00-000000-1"), ("date", "1972")]));
        assert_eq!(zh.detected_lang.as_deref(), Some("zh"));
        assert_eq!(zh.container_title.as_deref(), Some("国民财富的性质和原因的研究"));
        assert_eq!(zh.isbn.as_deref(), Some("9780000000001"));
        assert_eq!((zh.date_sort.as_deref(), zh.year), (Some("1972-00-00"), Some(1972)), "不是两段式也能解析");

        let odd = derive_columns(&f(&[("caseName", "Roe v. Wade"), ("dateDecided", "0000-00-00 n.d."), ("extra", "PMID: 1\nDOI: 10.1/abc")]));
        assert_eq!(odd.title.as_deref(), Some("Roe v. Wade"));
        assert_eq!((odd.date_sort, odd.date_raw.as_deref(), odd.year), (None, Some("n.d."), None));
        assert_eq!(odd.doi.as_deref(), Some("10.1/abc"));
        assert_eq!(derive_columns(&BTreeMap::new()), Columns::default());
    }

    #[test]
    fn which_field_holds_what() {
        assert_eq!(container_field("journalArticle", &BTreeMap::new()), Some("publicationTitle"));
        assert_eq!(container_field("bookSection", &BTreeMap::new()), Some("bookTitle"));
        assert_eq!(container_field("book", &BTreeMap::new()), None);
        assert_eq!(container_field("book", &f(&[("publicationTitle", "Imported")])), Some("publicationTitle"), "已有值的字段优先");
        assert_eq!(title_field(&f(&[("caseName", "Roe v. Wade")])), "caseName");
        assert_eq!(title_field(&BTreeMap::new()), "title");
        assert_eq!(date_field(&f(&[("dateEnacted", "2001")])), "dateEnacted");
        assert_eq!(display_value("date", "2019-03-00 March 2019"), "March 2019");
        assert_eq!(display_value("volume", "12"), "12");
    }
}
