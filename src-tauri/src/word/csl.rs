//! 文献库条目 → CSL-JSON（Word 引文用）。id 用条目 uid（`decisions/0006`）；类型、字段、作者角色按 Zotero 的 CSL 映射。
//! 库里存的是 Zotero 的字段名（导入与编辑都只认这一套），这里是唯一的换算处。

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};

use docx_engine::cite::zotero::{ItemKey, Library as ZoteroLibrary};

use crate::common::err;

fn csl_type(zotero: &str) -> &'static str {
    match zotero {
        "artwork" => "graphic",
        "audioRecording" | "podcast" => "song",
        "bill" => "bill",
        "blogPost" => "post-weblog",
        "book" => "book",
        "bookSection" => "chapter",
        "case" => "legal_case",
        "computerProgram" => "software",
        "conferencePaper" => "paper-conference",
        "dataset" => "dataset",
        "dictionaryEntry" => "entry-dictionary",
        "email" | "letter" | "instantMessage" => "personal_communication",
        "encyclopediaArticle" => "entry-encyclopedia",
        "film" | "videoRecording" => "motion_picture",
        "forumPost" => "post",
        "hearing" => "hearing",
        "interview" => "interview",
        "journalArticle" => "article-journal",
        "magazineArticle" => "article-magazine",
        "manuscript" => "manuscript",
        "map" => "map",
        "newspaperArticle" => "article-newspaper",
        "patent" => "patent",
        "preprint" => "article",
        "presentation" => "speech",
        "radioBroadcast" | "tvBroadcast" => "broadcast",
        "report" => "report",
        "standard" => "standard",
        "statute" => "legislation",
        "thesis" => "thesis",
        "webpage" => "webpage",
        _ => "document",
    }
}

/// Zotero 字段 → CSL 变量（日期另算）。同一个 CSL 变量有多个来源时取先出现的非空值。
const FIELDS: &[(&str, &str)] = &[
    ("title", "title"),
    ("caseName", "title"),
    ("nameOfAct", "title"),
    ("subject", "title"),
    ("shortTitle", "title-short"),
    ("abstractNote", "abstract"),
    ("publicationTitle", "container-title"),
    ("bookTitle", "container-title"),
    ("proceedingsTitle", "container-title"),
    ("websiteTitle", "container-title"),
    ("blogTitle", "container-title"),
    ("forumTitle", "container-title"),
    ("encyclopediaTitle", "container-title"),
    ("dictionaryTitle", "container-title"),
    ("programTitle", "container-title"),
    ("reporter", "container-title"),
    ("code", "container-title"),
    ("journalAbbreviation", "container-title-short"),
    ("seriesTitle", "collection-title"),
    ("series", "collection-title"),
    ("seriesNumber", "collection-number"),
    ("volume", "volume"),
    ("codeVolume", "volume"),
    ("reporterVolume", "volume"),
    ("issue", "issue"),
    ("pages", "page"),
    ("codePages", "page"),
    ("firstPage", "page"),
    ("numPages", "number-of-pages"),
    ("numberOfVolumes", "number-of-volumes"),
    ("edition", "edition"),
    ("section", "section"),
    ("publisher", "publisher"),
    ("university", "publisher"),
    ("institution", "publisher"),
    ("company", "publisher"),
    ("label", "publisher"),
    ("distributor", "publisher"),
    ("studio", "publisher"),
    ("network", "publisher"),
    ("place", "publisher-place"),
    ("conferenceName", "event-title"),
    ("meetingName", "event-title"),
    ("thesisType", "genre"),
    ("reportType", "genre"),
    ("genre", "genre"),
    ("websiteType", "genre"),
    ("letterType", "genre"),
    ("manuscriptType", "genre"),
    ("mapType", "genre"),
    ("presentationType", "genre"),
    ("postType", "genre"),
    ("medium", "medium"),
    ("artworkMedium", "medium"),
    ("audioRecordingFormat", "medium"),
    ("videoRecordingFormat", "medium"),
    ("interviewMedium", "medium"),
    ("audioFileType", "medium"),
    ("number", "number"),
    ("reportNumber", "number"),
    ("billNumber", "number"),
    ("patentNumber", "number"),
    ("docketNumber", "number"),
    ("publicLawNumber", "number"),
    ("episodeNumber", "number"),
    ("versionNumber", "version"),
    ("runningTime", "dimensions"),
    ("scale", "scale"),
    ("court", "authority"),
    ("authority", "authority"),
    ("legislativeBody", "authority"),
    ("url", "URL"),
    ("DOI", "DOI"),
    ("ISBN", "ISBN"),
    ("ISSN", "ISSN"),
    ("archive", "archive"),
    ("archiveLocation", "archive_location"),
    ("callNumber", "call-number"),
    ("libraryCatalog", "source"),
    ("rights", "license"),
    ("extra", "note"),
];

/// Zotero 的「额外」字段里可以写「Original Date: 1776」「Volume: 46」这样的行补字段；Zotero 交给 citeproc 前把键换成
/// CSL 变量名（citeproc 只认 `original-date: …` 这种写法）。这里做同样的换算：键是 Zotero 字段名的换成对应的 CSL 变量，
/// 其余的换成小写连字符形式（citeproc 不认识的会忽略）。不是「键: 值」的行原样保留。
fn extra_note(extra: &str) -> String {
    extra
        .lines()
        .map(|line| {
            let Some((k, v)) = line.split_once(':') else { return line.to_string() };
            let key = k.trim();
            let ok = !key.is_empty() && key.len() <= 40 && key.chars().all(|c| c.is_ascii_alphabetic() || c == ' ' || c == '-') && !v.trim().is_empty();
            if !ok {
                return line.to_string();
            }
            let flat: String = key.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
            let var = FIELDS.iter().find(|(z, _)| z.to_lowercase() == flat).map(|(_, c)| c.to_string()).unwrap_or_else(|| key.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"));
            format!("{var}: {}", v.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 与 Zotero 的默认做法一致（偏好 export.citePaperJournalArticleURL 默认关）：
/// 有页码的期刊、报纸、杂志文章是「印刷品」，引文与书目里不出网址和访问日期。
fn printed_article(item_type: &str, fields: &BTreeMap<String, String>) -> bool {
    matches!(item_type, "journalArticle" | "newspaperArticle" | "magazineArticle") && fields.get("pages").is_some_and(|p| !p.trim().is_empty())
}

fn csl_role(zotero: &str) -> &'static str {
    match zotero {
        "author" | "artist" | "cartographer" | "inventor" | "podcaster" | "presenter" | "programmer" | "sponsor" | "interviewee" => "author",
        "editor" => "editor",
        "bookAuthor" => "container-author",
        "seriesEditor" => "collection-editor",
        "translator" => "translator",
        "reviewedAuthor" => "reviewed-author",
        "recipient" => "recipient",
        "interviewer" => "interviewer",
        "director" => "director",
        "composer" => "composer",
        "illustrator" => "illustrator",
        "performer" | "castMember" => "performer",
        "producer" => "producer",
        "scriptwriter" => "script-writer",
        "guest" => "guest",
        "counsel" => "counsel",
        _ => "contributor",
    }
}

/// 'YYYY-MM-DD'（未知的月日是 00）→ date-parts
fn date_parts(sort: &str) -> Option<Value> {
    let mut parts = sort.split('-').map(|p| p.parse::<i64>().unwrap_or(0));
    let y = parts.next().filter(|y| *y > 0)?;
    let m = parts.next().unwrap_or(0);
    let d = parts.next().unwrap_or(0);
    let dp = match (m, d) {
        (0, _) => json!([[y]]),
        (m, 0) => json!([[y, m]]),
        (m, d) => json!([[y, m, d]]),
    };
    Some(json!({ "date-parts": dp }))
}

/// 语言：原字段优先（「中文」「Chinese」「zh」都算中文），没有就用按内容判定的。GB/T 7714 靠它分中英文著录。
fn language(raw: Option<&str>, detected: Option<&str>) -> Option<String> {
    let norm = |s: &str| -> Option<String> {
        let l = s.trim().to_lowercase();
        if l.is_empty() {
            None
        } else if l.starts_with("zh") || l.contains('中') || l.contains("chinese") || l == "chi" || l == "zho" {
            Some("zh-CN".into())
        } else if l.starts_with("en") || l == "eng" || l.contains("英") {
            Some("en-US".into())
        } else {
            Some(s.trim().to_string())
        }
    };
    raw.and_then(norm).or_else(|| detected.and_then(norm))
}

struct Row {
    id: i64,
    uid: String,
    item_type: String,
    date_sort: Option<String>,
    date_raw: Option<String>,
    language: Option<String>,
    detected: Option<String>,
}

fn convert(c: &Connection, r: &Row) -> Result<Value, String> {
    let mut out = Map::new();
    out.insert("id".into(), json!(r.uid));
    out.insert("type".into(), json!(csl_type(&r.item_type)));
    let mut stmt = c.prepare_cached("SELECT field, value FROM item_fields WHERE item_id = ?1").map_err(err)?;
    let fields: BTreeMap<String, String> = stmt.query_map([r.id], |x| Ok((x.get(0)?, x.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let printed = printed_article(&r.item_type, &fields);
    for (z, csl) in FIELDS {
        if out.contains_key(*csl) || (printed && *csl == "URL") {
            continue;
        }
        if let Some(v) = fields.get(*z).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            let v = if *z == "extra" { extra_note(v) } else { v.to_string() };
            out.insert(csl.to_string(), json!(v));
        }
    }
    // 期刊文章的 seriesTitle 是丛刊名，不是书名；其余类型的 series 同理落到 collection-title（上表已处理）
    match r.date_sort.as_deref().and_then(date_parts) {
        Some(d) => {
            out.insert("issued".into(), d);
        }
        None => {
            if let Some(raw) = r.date_raw.as_deref().filter(|s| !s.trim().is_empty()) {
                out.insert("issued".into(), json!({ "literal": raw }));
            }
        }
    }
    if let Some(a) = fields.get("accessDate").and_then(|v| v.get(..10)).and_then(date_parts).filter(|_| !printed) {
        out.insert("accessed".into(), a);
    }
    if let Some(l) = language(r.language.as_deref(), r.detected.as_deref()) {
        out.insert("language".into(), json!(l));
    }
    let mut stmt = c.prepare_cached("SELECT role, family, given, literal FROM creators WHERE item_id = ?1 ORDER BY position").map_err(err)?;
    let rows = stmt
        .query_map([r.id], |x| Ok((x.get::<_, String>(0)?, x.get::<_, Option<String>>(1)?, x.get::<_, Option<String>>(2)?, x.get::<_, Option<String>>(3)?)))
        .map_err(err)?;
    for row in rows {
        let (role, family, given, literal) = row.map_err(err)?;
        let name = match (literal.filter(|s| !s.trim().is_empty()), family.filter(|s| !s.trim().is_empty()), given.filter(|s| !s.trim().is_empty())) {
            (Some(l), _, _) => json!({ "literal": l }),
            (None, Some(f), Some(g)) => json!({ "family": f, "given": g }),
            (None, Some(f), None) => json!({ "family": f }),
            (None, None, Some(g)) => json!({ "literal": g }),
            (None, None, None) => continue,
        };
        let list = out.entry(csl_role(&role).to_string()).or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            a.push(name);
        }
    }
    Ok(Value::Object(out))
}

/// 按 uid 取 CSL-JSON（回收站里的也给：文档里已经引用了的文献，刷新时仍要能排出来）
pub fn items_csl(c: &Connection, uids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String> {
    let mut out = BTreeMap::new();
    let mut stmt = c.prepare_cached("SELECT id, uid, item_type, date_sort, date_raw, language, detected_lang FROM items WHERE uid = ?1").map_err(err)?;
    for uid in uids {
        let row = stmt
            .query_row([uid], |x| Ok(Row { id: x.get(0)?, uid: x.get(1)?, item_type: x.get(2)?, date_sort: x.get(3)?, date_raw: x.get(4)?, language: x.get(5)?, detected: x.get(6)? }))
            .optional()
            .map_err(err)?;
        if let Some(r) = row {
            out.insert(uid.clone(), convert(c, &r)?);
        }
    }
    Ok(out)
}

/// Zotero 条目 → 文献库 uid（导入 Zotero 时记下了 source_key）。
/// 个人库在 Zotero 里总是 libraryID 1；群组库的 ID 对应没有记，只按 key 找（8 位随机串，几乎不会撞）。
/// 同一个 key 有多条时：不在回收站的、库对得上的优先。
pub fn resolve_zotero(c: &Connection, key: &ItemKey) -> Result<Option<String>, String> {
    let user = i64::from(key.library == ZoteroLibrary::User);
    c.query_row(
        "SELECT uid FROM items WHERE source = 'zotero' AND source_key = ?1
         ORDER BY deleted_at IS NOT NULL, (COALESCE(source_library, 0) = 1) <> ?2, id LIMIT 1",
        params![key.key, user],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::edit::{self, CreatorInput, ItemPatch};

    fn person(role: &str, family: &str, given: &str) -> CreatorInput {
        CreatorInput { role: role.into(), family: Some(family.into()), given: Some(given.into()), literal: None }
    }

    fn make(c: &Connection, ty: &str, fields: &[(&str, &str)], creators: Vec<CreatorInput>) -> String {
        let id = edit::item_create(c, ty, None).unwrap();
        let patch = ItemPatch { fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), creators: Some(creators), ..Default::default() };
        edit::item_update(c, id, &patch).unwrap();
        c.query_row("SELECT uid FROM items WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn journal_article_and_chinese_book() {
        let c = memory_db();
        let a = make(
            &c,
            "journalArticle",
            &[("title", "Civic attitudes in synthetic panels"), ("container", "Journal of Synthetic Social Science"), ("volume", "12"), ("issue", "3"), ("pages", "45-67"), ("date", "2021-06"), ("DOI", "10.5555/rl.0001"), ("accessDate", "2026-11-07 10:00:00")],
            vec![person("author", "Lindqvist", "Mara"), person("author", "Okafor", "Daniel"), person("editor", "Park", "Jiwon")],
        );
        let b = make(
            &c,
            "book",
            &[("title", "证据与写作"), ("publisher", "示例出版社"), ("place", "北京"), ("date", "2019")],
            vec![CreatorInput { role: "author".into(), family: None, given: None, literal: Some("张伟".into()) }, person("translator", "李", "华")],
        );
        let m = items_csl(&c, &[a.clone(), b.clone(), "missing".into()].into_iter().collect()).unwrap();
        assert_eq!(m.len(), 2, "库里没有的不给");
        let j = &m[&a];
        assert_eq!(j["id"], a);
        assert_eq!(j["type"], "article-journal");
        assert_eq!(j["container-title"], "Journal of Synthetic Social Science");
        assert_eq!((j["volume"].as_str(), j["issue"].as_str(), j["page"].as_str()), (Some("12"), Some("3"), Some("45-67")));
        assert_eq!(j["issued"], json!({ "date-parts": [[2021, 6]] }));
        assert!(j.get("accessed").is_none() && j.get("URL").is_none(), "有页码的期刊文章不出网址与访问日期（与 Zotero 默认一致）");
        assert_eq!(j["DOI"], "10.5555/rl.0001");
        assert_eq!(j["author"], json!([{ "family": "Lindqvist", "given": "Mara" }, { "family": "Okafor", "given": "Daniel" }]));
        assert_eq!(j["editor"], json!([{ "family": "Park", "given": "Jiwon" }]));
        assert_eq!(j["language"], "en-US", "按内容判定");
        let k = &m[&b];
        assert_eq!((k["type"].as_str(), k["publisher"].as_str(), k["publisher-place"].as_str()), (Some("book"), Some("示例出版社"), Some("北京")));
        assert_eq!(k["author"], json!([{ "literal": "张伟" }]));
        assert_eq!(k["translator"], json!([{ "family": "李", "given": "华" }]));
        assert_eq!(k["issued"], json!({ "date-parts": [[2019]] }));
        assert_eq!(k["language"], "zh-CN");
    }

    #[test]
    fn online_articles_keep_url_and_access_date() {
        let c = memory_db();
        let a = make(&c, "journalArticle", &[("title", "Online only"), ("url", "https://example.org/a"), ("accessDate", "2026-11-07 10:00:00"), ("rights", "CC BY 4.0")], vec![]);
        let w = make(&c, "webpage", &[("title", "Page"), ("url", "https://example.org/w"), ("accessDate", "2026-11-08 10:00:00"), ("pages", "3")], vec![]);
        let m = items_csl(&c, &[a.clone(), w.clone()].into_iter().collect()).unwrap();
        assert_eq!((m[&a]["URL"].as_str(), m[&a]["accessed"].clone()), (Some("https://example.org/a"), json!({ "date-parts": [[2026, 11, 7]] })), "没有页码：照常");
        assert_eq!(m[&a]["license"], "CC BY 4.0");
        assert_eq!(m[&w]["URL"], "https://example.org/w", "网页不受影响");
    }

    #[test]
    fn chapter_thesis_and_language_field() {
        let c = memory_db();
        let ch = make(&c, "bookSection", &[("title", "Measuring trust"), ("container", "Methods for synthetic research"), ("pages", "101-120"), ("language", "English")], vec![person("bookAuthor", "Park", "Jiwon")]);
        let th = make(&c, "thesis", &[("title", "政治效能感研究"), ("university", "重庆大学"), ("thesisType", "硕士学位论文"), ("date", "2023-05-20"), ("language", "中文")], vec![]);
        let m = items_csl(&c, &[ch.clone(), th.clone()].into_iter().collect()).unwrap();
        assert_eq!((m[&ch]["type"].as_str(), m[&ch]["container-title"].as_str()), (Some("chapter"), Some("Methods for synthetic research")));
        assert_eq!(m[&ch]["container-author"], json!([{ "family": "Park", "given": "Jiwon" }]));
        assert_eq!(m[&ch]["language"], "en-US");
        assert_eq!((m[&th]["type"].as_str(), m[&th]["publisher"].as_str(), m[&th]["genre"].as_str()), (Some("thesis"), Some("重庆大学"), Some("硕士学位论文")));
        assert_eq!(m[&th]["issued"], json!({ "date-parts": [[2023, 5, 20]] }));
        assert_eq!(m[&th]["language"], "zh-CN");
    }

    #[test]
    fn resolves_zotero_keys() {
        let c = memory_db();
        let a = make(&c, "book", &[("title", "A")], vec![]);
        let b = make(&c, "book", &[("title", "B")], vec![]);
        let g = make(&c, "book", &[("title", "G")], vec![]);
        c.execute("UPDATE items SET source = 'zotero', source_library = 1, source_key = 'AAAA1111' WHERE uid = ?1", [&a]).unwrap();
        c.execute("UPDATE items SET source = 'zotero', source_library = 1, source_key = 'BBBB2222' WHERE uid = ?1", [&b]).unwrap();
        c.execute("UPDATE items SET source = 'zotero', source_library = 7, source_key = 'BBBB2222' WHERE uid = ?1", [&g]).unwrap();
        let k = |lib: ZoteroLibrary, key: &str| ItemKey { library: lib, key: key.into() };
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::User, "AAAA1111")).unwrap().as_deref(), Some(a.as_str()));
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::Group("3".into()), "AAAA1111")).unwrap().as_deref(), Some(a.as_str()), "群组 ID 对不上也按 key 找");
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::User, "BBBB2222")).unwrap().as_deref(), Some(b.as_str()), "个人库优先");
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::Group("3".into()), "BBBB2222")).unwrap().as_deref(), Some(g.as_str()), "群组库优先");
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::User, "NONE0000")).unwrap(), None);
        let id: i64 = c.query_row("SELECT id FROM items WHERE uid = ?1", [&b], |r| r.get(0)).unwrap();
        edit::trash(&c, &[edit::Target::item(id)]).unwrap();
        assert_eq!(resolve_zotero(&c, &k(ZoteroLibrary::User, "BBBB2222")).unwrap().as_deref(), Some(g.as_str()), "回收站里的排后面");
    }

    #[test]
    fn extra_lines_become_csl_variables() {
        assert_eq!(extra_note("Series: OECD Working Papers\nVolume: 46"), "collection-title: OECD Working Papers\nvolume: 46");
        assert_eq!(extra_note("Original Date: 1776\nPublisher Place: London"), "original-date: 1776\npublisher-place: London");
        assert_eq!(extra_note("tex.ids: a, b\nJust a note\nURL: http://x"), "tex.ids: a, b\nJust a note\nURL: http://x", "不像键的、没有值的原样");
        assert_eq!(extra_note("DOI: 10.1/x"), "DOI: 10.1/x");
    }

    #[test]
    fn dates_and_languages() {
        assert_eq!(date_parts("2021-00-00"), Some(json!({ "date-parts": [[2021]] })));
        assert_eq!(date_parts("0000-00-00"), None);
        assert_eq!(language(None, Some("zh")).as_deref(), Some("zh-CN"));
        assert_eq!(language(Some("fr"), Some("en")).as_deref(), Some("fr"));
        assert_eq!(language(Some(" "), None), None);
    }
}
