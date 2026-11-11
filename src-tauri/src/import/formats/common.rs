//! 三个解析器共用的小工具：字段写入（冲突不丢）、标签拆分去重、单栏姓名拆分、DOI 清理、日期拼装。

use std::collections::BTreeMap;

use super::{Record, RecordCreator, RecordDate};

pub(super) type Fields = BTreeMap<String, String>;

pub(super) fn new_record(item_type: &str, raw: String) -> Record {
    Record {
        item_type: item_type.to_string(),
        fields: BTreeMap::new(),
        date: None,
        creators: Vec::new(),
        tags: Vec::new(),
        notes: Vec::new(),
        source_key: None,
        raw,
        warnings: Vec::new(),
    }
}

/// 写入 Zotero 字段：值先 trim，空的不写；该字段已有**不同的**值时改写进 `fallback`（原始键），保证不丢。
pub(super) fn put(fields: &mut Fields, key: &str, value: &str, fallback: &str) {
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    match fields.get(key) {
        None => {
            fields.insert(key.to_string(), v.to_string());
        }
        Some(old) if old == v => {}
        Some(_) => append(fields, fallback, v),
    }
}

/// 追加到字段末尾，换行分隔（同一原始键出现多次、多条 note 拼 extra）
pub(super) fn append(fields: &mut Fields, key: &str, value: &str) {
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    match fields.get_mut(key) {
        Some(old) => {
            old.push('\n');
            old.push_str(v);
        }
        None => {
            fields.insert(key.to_string(), v.to_string());
        }
    }
}

/// 按分隔符拆标签：trim、去空、去重保序
pub(super) fn add_tags(tags: &mut Vec<String>, text: &str, seps: &[char]) {
    for t in text.split(seps) {
        let t = t.trim();
        if !t.is_empty() && !tags.iter().any(|x| x == t) {
            tags.push(t.to_string());
        }
    }
}

pub(super) fn add_note(notes: &mut Vec<String>, text: &str) {
    let t = text.trim();
    if !t.is_empty() {
        notes.push(t.to_string());
    }
}

/// 「姓, 名」写法：第一个逗号前是姓，后面整段是名（"Smith, John, Jr." → 名 "John, Jr."，与 Zotero 把后缀写在名后面的约定一致）。
/// 没有逗号的（机构名、中文单栏姓名、不规范的 "John Smith"）作单栏 literal，不猜怎么拆。
pub(super) fn creator_from_text(role: &str, name: &str) -> Option<RecordCreator> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let (family, given, literal) = match name.split_once(',') {
        Some((f, g)) if !f.trim().is_empty() => (non_empty(f), non_empty(g), None),
        _ => (None, None, Some(name.to_string())),
    };
    Some(RecordCreator { role: role.to_string(), family, given, literal })
}

pub(super) fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// 用空格连接非空的几段（姓名的 particle + family 等）
pub(super) fn join_words(parts: &[&str]) -> Option<String> {
    let s = parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// 各类型的「所属出版物」字段（期刊名、书名、会议论文集名……），没有就 None
pub(super) fn container_field(item_type: &str) -> Option<&'static str> {
    Some(match item_type {
        "journalArticle" | "magazineArticle" | "newspaperArticle" | "preprint" => "publicationTitle",
        "bookSection" => "bookTitle",
        "conferencePaper" => "proceedingsTitle",
        "webpage" => "websiteTitle",
        "blogPost" => "blogTitle",
        "forumPost" => "forumTitle",
        "encyclopediaArticle" => "encyclopediaTitle",
        "dictionaryEntry" => "dictionaryTitle",
        _ => return None,
    })
}

/// 出版者在 Zotero 里按类型叫不同名字：学位论文是 university，报告是 institution
pub(super) fn publisher_field(item_type: &str) -> &'static str {
    match item_type {
        "thesis" => "university",
        "report" => "institution",
        _ => "publisher",
    }
}

/// 去掉 `https://doi.org/`、`doi:` 等前缀，只留 10.xxx/yyy
pub(super) fn clean_doi(s: &str) -> String {
    let mut s = s.trim();
    for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi.org/", "doi:"] {
        if s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p)) {
            s = s[p.len()..].trim_start();
            break;
        }
    }
    s.to_string()
}

pub(super) fn raw_date(raw: &str) -> Option<RecordDate> {
    non_empty(raw).map(|raw| RecordDate { raw, year: None, month: None, day: None })
}

/// 结构化年月日 → RecordDate；月、日超出范围的丢掉。`raw` 为 None 时按 ISO 拼（"2019-03-15" / "2019-03" / "2019"）。
pub(super) fn structured_date(raw: Option<String>, year: i32, month: Option<u32>, day: Option<u32>) -> RecordDate {
    let month = month.filter(|m| (1..=12).contains(m));
    let day = if month.is_some() { day.filter(|d| (1..=31).contains(d)) } else { None };
    let raw = raw.unwrap_or_else(|| iso_text(year, month, day));
    RecordDate { raw, year: Some(year), month, day }
}

pub(super) fn iso_text(year: i32, month: Option<u32>, day: Option<u32>) -> String {
    match (month, day) {
        (Some(m), Some(d)) => format!("{year:04}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    }
}

/// 只认简单的 ISO 写法："2019"、"2019-03"、"2019-03-15"（可带时间、`/` 范围取前半、EDTF 的 ? ~ % 标记）。
/// 别的写法返回 None，由写入层按原文解析。
pub(super) fn parse_iso(s: &str) -> Option<(i32, Option<u32>, Option<u32>)> {
    let s = s.trim();
    let s = s.split('/').next()?.trim().trim_end_matches(['?', '~', '%']);
    let s = s.split(['T', ' ']).next()?;
    let mut parts = s.split('-');
    let year = parts.next().filter(|y| y.len() == 4).and_then(|y| digits(y, 4))?;
    // 有这一段就必须是数字，否则整个不认
    let mut next = || parts.next().map(|p| digits(p, 2).ok_or(()));
    let month = next().transpose().ok()?;
    let day = next().transpose().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((i32::try_from(year).ok()?, month, day.filter(|_| month.is_some())))
}

/// 1 到 `max_len` 位纯数字
pub(super) fn digits(s: &str, max_len: usize) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.len() > max_len || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// 字节偏移 → 行号（1 起）。先记下所有换行位置再二分查找，大文件逐条报行号也不慢
pub(super) struct Lines(Vec<usize>);

impl Lines {
    pub(super) fn new(text: &str) -> Self {
        Lines(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i).collect())
    }

    pub(super) fn line_at(&self, offset: usize) -> usize {
        self.0.partition_point(|&p| p < offset) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_keeps_conflicting_values() {
        let mut f = Fields::new();
        put(&mut f, "title", "  A  ", "x:title");
        put(&mut f, "title", "A", "x:title");
        put(&mut f, "title", "B", "x:title");
        put(&mut f, "title", "C", "x:title");
        put(&mut f, "volume", "   ", "x:volume");
        assert_eq!(f.get("title").map(String::as_str), Some("A"));
        assert_eq!(f.get("x:title").map(String::as_str), Some("B\nC"));
        assert!(!f.contains_key("volume"));
    }

    #[test]
    fn tags_names_doi_dates() {
        let mut tags = vec![];
        add_tags(&mut tags, " a; b ,a,, c ", &[',', ';']);
        assert_eq!(tags, ["a", "b", "c"]);

        let c = creator_from_text("author", "Smith, John, Jr.").unwrap();
        assert_eq!((c.family.as_deref(), c.given.as_deref()), (Some("Smith"), Some("John, Jr.")));
        let c = creator_from_text("author", "World Bank").unwrap();
        assert_eq!(c.literal.as_deref(), Some("World Bank"));
        assert!(creator_from_text("author", "  ").is_none());

        assert_eq!(clean_doi("https://doi.org/10.1000/xyz"), "10.1000/xyz");
        assert_eq!(clean_doi("DOI: 10.1000/xyz"), "10.1000/xyz");
        assert_eq!(clean_doi("10.1000/xyz"), "10.1000/xyz");

        assert_eq!(parse_iso("2019-03-15"), Some((2019, Some(3), Some(15))));
        assert_eq!(parse_iso("1972-05"), Some((1972, Some(5), None)));
        assert_eq!(parse_iso("2020-04-04T18:30:31/2021"), Some((2020, Some(4), Some(4))));
        assert_eq!(parse_iso("March 2019"), None);
        assert_eq!(parse_iso("2019-03-15-01"), None);
        let d = structured_date(None, 2019, Some(13), Some(5));
        assert_eq!((d.raw.as_str(), d.month, d.day), ("2019", None, None));
        let lines = Lines::new("a\nb\nc");
        assert_eq!((lines.line_at(0), lines.line_at(1), lines.line_at(2), lines.line_at(4), lines.line_at(99)), (1, 1, 2, 3, 3));
    }
}
