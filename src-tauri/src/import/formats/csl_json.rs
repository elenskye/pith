//! CSL JSON → Record。接受条目数组（标准）、单个条目对象、`{"items": [...]}`。

use serde_json::{Map, Value};

use super::common::{
    add_tags, clean_doi, container_field, creator_from_text, iso_text, join_words, new_record, non_empty, publisher_field, put, raw_date,
    structured_date,
};
use super::{Parsed, Record, RecordCreator, RecordDate};

/// CSL 姓名变量 → Zotero 角色；作者列表按这个顺序排
const NAME_ROLES: [(&str, &str); 10] = [
    ("author", "author"),
    ("editor", "editor"),
    ("translator", "translator"),
    ("container-author", "bookAuthor"),
    ("collection-editor", "seriesEditor"),
    ("reviewed-author", "reviewedAuthor"),
    ("director", "director"),
    ("interviewer", "interviewer"),
    ("recipient", "recipient"),
    ("contributor", "contributor"),
];

/// 其余 CSL 姓名变量：Zotero 没有对应角色，按 contributor 导入并提示
const OTHER_NAMES: [&str; 16] = [
    "chair", "compiler", "composer", "curator", "editorial-director", "executive-producer", "guest", "host", "illustrator", "narrator",
    "organizer", "original-author", "performer", "producer", "script-writer", "series-creator",
];

pub(super) fn parse(text: &str) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let items = match serde_json::from_str::<Value>(text).map_err(|e| json_error(&e))? {
        Value::Array(a) => a,
        Value::Object(mut o) => match o.remove("items") {
            Some(Value::Array(a)) => a,
            Some(other) => {
                o.insert("items".into(), other);
                vec![Value::Object(o)]
            }
            None => vec![Value::Object(o)],
        },
        _ => return Err("不是有效的 CSL JSON：最外层应是条目数组或条目对象".into()),
    };
    let mut parsed = Parsed::default();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        match item {
            Value::Object(o) if looks_like_csl(o) => parsed.records.push(to_record(o)),
            Value::Object(_) => parsed.warnings.push(format!("第 {n} 个条目没有 type、title、id 等 CSL 字段，已跳过")),
            other => parsed.warnings.push(format!("第 {n} 个元素不是对象（是{}），已跳过", kind_name(other))),
        }
    }
    if parsed.records.is_empty() && !items.is_empty() {
        return Err("不是有效的 CSL JSON：没有找到文献条目（条目应是带 type、title、id 等字段的对象）".into());
    }
    Ok(parsed)
}

fn json_error(e: &serde_json::Error) -> String {
    match e.classify() {
        serde_json::error::Category::Eof => format!("不是有效的 CSL JSON：内容在第 {} 行第 {} 列提前结束（括号没有闭合？）", e.line(), e.column()),
        _ => format!("不是有效的 CSL JSON：第 {} 行第 {} 列有语法错误", e.line(), e.column()),
    }
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "空值 null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

/// 防止把随便一个 JSON（配置文件之类）当成文献导进来
fn looks_like_csl(o: &Map<String, Value>) -> bool {
    ["type", "title", "id", "author", "editor", "issued", "DOI", "URL", "container-title", "ISBN"].iter().any(|k| o.contains_key(*k))
}

fn item_type(t: &str) -> &'static str {
    match t {
        "article-journal" | "review" | "review-book" => "journalArticle",
        "article-magazine" => "magazineArticle",
        "article-newspaper" => "newspaperArticle",
        "article" => "preprint",
        "book" => "book",
        "chapter" => "bookSection",
        "paper-conference" => "conferencePaper",
        "thesis" => "thesis",
        "report" => "report",
        "webpage" => "webpage",
        "post-weblog" => "blogPost",
        "post" => "forumPost",
        "dataset" => "dataset",
        "patent" => "patent",
        "manuscript" => "manuscript",
        "entry-encyclopedia" => "encyclopediaArticle",
        "entry-dictionary" => "dictionaryEntry",
        "software" => "computerProgram",
        "standard" => "standard",
        "legislation" => "statute",
        "legal_case" => "case",
        "map" => "map",
        "motion_picture" => "film",
        "broadcast" => "tvBroadcast",
        "interview" => "interview",
        "speech" => "presentation",
        "personal_communication" => "letter",
        "graphic" => "artwork",
        "song" => "audioRecording",
        _ => "document",
    }
}

fn to_record(o: &Map<String, Value>) -> Record {
    let item_type = item_type(o.get("type").and_then(Value::as_str).unwrap_or("").trim());
    let mut r = new_record(item_type, serde_json::to_string(o).unwrap_or_default());
    for (var, role) in NAME_ROLES {
        if let Some(v) = o.get(var) {
            add_names(&mut r, role, v);
        }
    }
    for var in OTHER_NAMES {
        let before = r.creators.len();
        if let Some(v) = o.get(var) {
            add_names(&mut r, "contributor", v);
        }
        if r.creators.len() > before {
            r.warnings.push(format!("CSL 姓名变量 {var} 没有对应的 Zotero 角色，已按 contributor 导入"));
        }
    }

    for (k, v) in o {
        if NAME_ROLES.iter().any(|(n, _)| *n == k.as_str()) || OTHER_NAMES.contains(&k.as_str()) {
            continue;
        }
        let raw_key = format!("csl:{k}");
        let f = &mut r.fields;
        let zotero = match k.as_str() {
            "type" => continue,
            "id" => {
                r.source_key = scalar(v);
                continue;
            }
            "issued" => {
                r.date = csl_date(v);
                // 季节、circa 这些结构化字段放不下，原对象另存
                if r.date.is_none() || v.get("season").is_some() || v.get("circa").is_some() {
                    put(f, &raw_key, &json_text(v), &raw_key);
                }
                continue;
            }
            "accessed" => {
                let text = csl_date(v).map(|d| match d.year {
                    Some(y) => iso_text(y, d.month, d.day),
                    None => d.raw,
                });
                match text {
                    Some(t) => put(f, "accessDate", &t, &raw_key),
                    None => put(f, &raw_key, &json_text(v), &raw_key),
                }
                continue;
            }
            "keyword" => {
                match scalar_or_list(v) {
                    Some(t) => add_tags(&mut r.tags, &t, &[',', ';']),
                    None => put(f, &raw_key, &json_text(v), &raw_key),
                }
                continue;
            }
            "DOI" => {
                match scalar(v) {
                    Some(t) => put(f, "DOI", &clean_doi(&t), &raw_key),
                    None => put(f, &raw_key, &json_text(v), &raw_key),
                }
                continue;
            }
            "title" => Some("title"),
            "container-title" => Some(container_field(item_type).unwrap_or("publicationTitle")),
            "container-title-short" | "journalAbbreviation" => Some("journalAbbreviation"),
            "collection-title" => Some("series"),
            "collection-number" => Some("seriesNumber"),
            "volume" | "issue" | "edition" | "language" | "medium" | "archive" | "ISBN" | "ISSN" => Some(k.as_str()),
            "publisher" => Some(publisher_field(item_type)),
            "publisher-place" => Some("place"),
            "page" => Some("pages"),
            "number-of-pages" => Some("numPages"),
            "event-title" | "event" => Some("conferenceName"),
            "URL" => Some("url"),
            "abstract" => Some("abstractNote"),
            "note" => Some("extra"),
            "citation-key" => Some("citationKey"),
            "title-short" | "shortTitle" => Some("shortTitle"),
            "genre" => match item_type {
                "thesis" => Some("thesisType"),
                "report" => Some("reportType"),
                _ => None,
            },
            "number" => match item_type {
                "report" => Some("reportNumber"),
                "patent" => Some("patentNumber"),
                _ => None,
            },
            "archive_location" | "archive-location" => Some("archiveLocation"),
            "call-number" => Some("callNumber"),
            "source" => Some("libraryCatalog"),
            "version" => Some("versionNumber"),
            _ => None,
        };
        match (zotero, scalar_or_list(v)) {
            (Some(key), Some(t)) => put(f, key, &t, &raw_key),
            _ => put(f, &raw_key, &json_text(v), &raw_key),
        }
    }
    r
}

/// 字符串、数字、布尔 → 文本
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => non_empty(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 标量，或全是标量的数组（逗号连接）
fn scalar_or_list(v: &Value) -> Option<String> {
    match v {
        Value::Array(a) => a.iter().map(scalar).collect::<Option<Vec<_>>>().map(|parts| parts.join(", ")),
        _ => scalar(v),
    }
}

/// 映射不到的值：字符串原样，其余序列化成 JSON 文本
fn json_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}

fn add_names(r: &mut Record, role: &str, v: &Value) {
    match v {
        Value::Array(a) => r.creators.extend(a.iter().filter_map(|n| csl_name(role, n))),
        other => r.creators.extend(csl_name(role, other)),
    }
}

fn csl_name(role: &str, n: &Value) -> Option<RecordCreator> {
    let o = match n {
        Value::Object(o) => o,
        Value::String(s) => return creator_from_text(role, s),
        _ => return None,
    };
    let get = |k: &str| o.get(k).and_then(scalar).unwrap_or_default();
    if let Some(literal) = non_empty(&get("literal")) {
        return Some(RecordCreator { role: role.into(), family: None, given: None, literal: Some(literal) });
    }
    // 前置 particle 并进姓（"van Gogh"），后置 particle 接在名后（"Ludwig van"），后缀接在名后（"John, Jr."），与 Zotero 两栏姓名一致。
    // 只有 family 没有 given 的（含中文单栏姓名）照原样作 family、不改成 literal：来源本来就这么标，Zotero 单栏模式也只存姓这一栏。
    let family = join_words(&[&get("non-dropping-particle"), &get("family")]);
    let mut given = join_words(&[&get("given"), &get("dropping-particle")]);
    if let Some(suffix) = non_empty(&get("suffix")) {
        given = Some(match given {
            Some(g) => format!("{g}, {suffix}"),
            None => suffix,
        });
    }
    (family.is_some() || given.is_some()).then(|| RecordCreator { role: role.into(), family, given, literal: None })
}

/// `{"date-parts": [[2019, 3, 15]]}`（数字或数字字符串，范围取第一个）、`{"raw": …}`、`{"literal": …}`；
/// 有 date-parts 填结构化年月日，原文优先用 raw / literal，没有就拼 ISO
fn csl_date(v: &Value) -> Option<RecordDate> {
    let o = match v {
        Value::Object(o) => o,
        Value::String(s) => return raw_date(s),
        Value::Number(n) => return raw_date(&n.to_string()),
        _ => return None,
    };
    let text = ["raw", "literal"].iter().find_map(|k| o.get(*k).and_then(scalar));
    let all = o.get("date-parts").and_then(Value::as_array);
    // 标准是嵌套数组；也容忍拍平的 [2019, 3]
    let first = match all.and_then(|a| a.first()) {
        Some(Value::Array(p)) => Some(p),
        Some(_) => all,
        None => None,
    };
    if let Some(year) = first.and_then(|p| p.first()).and_then(int) {
        let part = |i: usize| first.and_then(|p| p.get(i)).and_then(int).and_then(|n| u32::try_from(n).ok());
        return Some(structured_date(text, year, part(1), part(2)));
    }
    text.and_then(|t| raw_date(&t))
}

fn int(v: &Value) -> Option<i32> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).and_then(|n| i32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(r: &'a Record, k: &str) -> Option<&'a str> {
        r.fields.get(k).map(String::as_str)
    }

    /// （角色，姓，名，单栏）
    type Name<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<&'a str>);

    fn names(r: &Record) -> Vec<Name<'_>> {
        r.creators.iter().map(|c| (c.role.as_str(), c.family.as_deref(), c.given.as_deref(), c.literal.as_deref())).collect()
    }

    const SAMPLE: &str = r#"[
      {"id": "item1", "type": "article-journal", "title": "气候变化与政策", "container-title": "Journal of Tests", "container-title-short": "J. Tests",
       "author": [{"family": "Smith", "given": "John"}, {"literal": "World Health Organization"}, {"family": "王", "given": "小明"}, {"family": "李四"},
                  {"family": "Gogh", "given": "Vincent", "non-dropping-particle": "van"}, {"family": "Beethoven", "given": "Ludwig", "dropping-particle": "van"},
                  {"family": "King", "given": "Martin Luther", "suffix": "Jr."}, "Doe, Jane", {}],
       "editor": [{"family": "Editor", "given": "Ed"}], "translator": [{"literal": "张三"}],
       "issued": {"date-parts": [[2019, 3, 15]]}, "volume": 12, "issue": "3", "page": "100-120", "DOI": "https://doi.org/10.1000/abc",
       "ISSN": "1234-5678", "URL": "https://example.com", "abstract": "摘要", "language": "zh-CN", "note": "Extra note",
       "keyword": "climate, policy; climate", "PMID": "123456", "custom": {"a": 1}, "citation-key": "smith2019",
       "accessed": {"date-parts": [["2024", "1", "5"]]}},
      42,
      {"id": 7, "type": "chapter", "title": "A Chapter", "container-title": "The Book", "container-author": [{"family": "Homer"}],
       "issued": {"raw": "Spring 2020"}, "collection-title": "Series", "collection-number": 3, "publisher": "ACM", "publisher-place": "New York",
       "composer": [{"family": "Bach", "given": "J. S."}]},
      {"type": "paper-conference", "title": "Talk", "container-title": "Proc. X", "event-title": "X Conf", "issued": {"date-parts": [[2020, 7]], "season": 2}},
      {"type": "webpage", "title": "Page", "container-title": "Site", "issued": {"literal": "n.d."}},
      {"type": "report", "title": "R", "genre": "Working Paper", "number": "WP-1", "publisher": "RAND"},
      {"type": "thesis", "title": "T", "genre": "PhD thesis", "publisher": "MIT"},
      {"type": "post-weblog", "title": "B", "container-title": "My Blog", "number": "9"},
      {"type": "something-new", "title": "X", "issued": {"date-parts": [["2001", "13"]]}},
      "stray string",
      {"foo": "bar"}
    ]"#;

    #[test]
    fn records_fields_creators_dates() {
        let p = parse(SAMPLE).unwrap();
        let types: Vec<_> = p.records.iter().map(|r| r.item_type.as_str()).collect();
        assert_eq!(types, ["journalArticle", "bookSection", "conferencePaper", "webpage", "report", "thesis", "blogPost", "document"]);
        assert_eq!(
            p.warnings,
            ["第 2 个元素不是对象（是数字），已跳过", "第 10 个元素不是对象（是字符串），已跳过", "第 11 个条目没有 type、title、id 等 CSL 字段，已跳过"]
        );

        let a = &p.records[0];
        assert_eq!(a.source_key.as_deref(), Some("item1"));
        assert_eq!(field(a, "citationKey"), Some("smith2019"));
        assert_eq!(field(a, "title"), Some("气候变化与政策"));
        assert_eq!((field(a, "publicationTitle"), field(a, "journalAbbreviation")), (Some("Journal of Tests"), Some("J. Tests")));
        assert_eq!(
            names(a),
            [
                ("author", Some("Smith"), Some("John"), None),
                ("author", None, None, Some("World Health Organization")),
                ("author", Some("王"), Some("小明"), None),
                ("author", Some("李四"), None, None),
                ("author", Some("van Gogh"), Some("Vincent"), None),
                ("author", Some("Beethoven"), Some("Ludwig van"), None),
                ("author", Some("King"), Some("Martin Luther, Jr."), None),
                ("author", Some("Doe"), Some("Jane"), None),
                ("editor", Some("Editor"), Some("Ed"), None),
                ("translator", None, None, Some("张三")),
            ]
        );
        assert_eq!(a.date, Some(RecordDate { raw: "2019-03-15".into(), year: Some(2019), month: Some(3), day: Some(15) }));
        assert!(!a.fields.contains_key("csl:issued"));
        assert_eq!((field(a, "volume"), field(a, "issue"), field(a, "pages")), (Some("12"), Some("3"), Some("100-120")));
        assert_eq!((field(a, "DOI"), field(a, "ISSN"), field(a, "url")), (Some("10.1000/abc"), Some("1234-5678"), Some("https://example.com")));
        assert_eq!((field(a, "abstractNote"), field(a, "language"), field(a, "extra")), (Some("摘要"), Some("zh-CN"), Some("Extra note")));
        assert_eq!(a.tags, ["climate", "policy"]);
        assert_eq!((field(a, "csl:PMID"), field(a, "csl:custom")), (Some("123456"), Some(r#"{"a":1}"#)));
        assert_eq!(field(a, "accessDate"), Some("2024-01-05"));
        assert!(a.raw.starts_with('{') && a.raw.contains(r#""id":"item1""#));
        assert!(a.warnings.is_empty());

        let c = &p.records[1];
        assert_eq!(c.source_key.as_deref(), Some("7"));
        assert_eq!(field(c, "bookTitle"), Some("The Book"));
        assert_eq!(names(c), [("bookAuthor", Some("Homer"), None, None), ("contributor", Some("Bach"), Some("J. S."), None)]);
        assert_eq!(c.warnings, ["CSL 姓名变量 composer 没有对应的 Zotero 角色，已按 contributor 导入"]);
        assert_eq!(c.date, Some(RecordDate { raw: "Spring 2020".into(), year: None, month: None, day: None }));
        assert_eq!((field(c, "series"), field(c, "seriesNumber")), (Some("Series"), Some("3")));
        assert_eq!((field(c, "publisher"), field(c, "place")), (Some("ACM"), Some("New York")));

        let conf = &p.records[2];
        assert_eq!((field(conf, "proceedingsTitle"), field(conf, "conferenceName")), (Some("Proc. X"), Some("X Conf")));
        assert_eq!(conf.date, Some(RecordDate { raw: "2020-07".into(), year: Some(2020), month: Some(7), day: None }));
        assert!(field(conf, "csl:issued").is_some_and(|s| s.contains("season")));

        let web = &p.records[3];
        assert_eq!(field(web, "websiteTitle"), Some("Site"));
        assert_eq!(web.date.as_ref().map(|d| (d.raw.as_str(), d.year)), Some(("n.d.", None)));
        let rep = &p.records[4];
        assert_eq!((field(rep, "reportType"), field(rep, "reportNumber"), field(rep, "institution")), (Some("Working Paper"), Some("WP-1"), Some("RAND")));
        let th = &p.records[5];
        assert_eq!((field(th, "thesisType"), field(th, "university")), (Some("PhD thesis"), Some("MIT")));
        let blog = &p.records[6];
        assert_eq!((field(blog, "blogTitle"), field(blog, "csl:number")), (Some("My Blog"), Some("9")));
        // 月份 13 不合法：只留年
        assert_eq!(p.records[7].date, Some(RecordDate { raw: "2001".into(), year: Some(2001), month: None, day: None }));
    }

    #[test]
    fn wrappers_and_invalid_files() {
        let one = parse(r#"{"id": "x", "type": "book", "title": "单个对象"}"#).unwrap();
        assert_eq!(field(&one.records[0], "title"), Some("单个对象"));
        let wrapped = parse("\u{feff}{\"items\": [{\"type\": \"book\"}, {\"type\": \"report\"}]}").unwrap();
        assert_eq!(wrapped.records.len(), 2);
        assert!(parse("[]").unwrap().records.is_empty());

        let e = parse("[\n  {\"id\": 1,\n    \"title\": }\n]").unwrap_err();
        assert!(e.starts_with("不是有效的 CSL JSON：第 3 行第 "), "{e}");
        assert!(parse("[{\"id\": 1").unwrap_err().contains("提前结束"));
        assert!(parse("TY  - JOUR").unwrap_err().contains("CSL JSON"));
        assert!(parse("42").unwrap_err().contains("最外层"));
        assert!(parse(r#"{"name": "my-app", "version": "1.0.0"}"#).unwrap_err().contains("没有找到文献条目"));
        assert!(parse("[1, 2]").unwrap_err().contains("没有找到文献条目"));
    }
}
