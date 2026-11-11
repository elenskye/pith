//! RIS → Record。逐行解析：「两位标签 + 空格 + `-` + 空格 + 值」，TY 开始一条、ER 结束一条。
//! 容忍：BOM、CRLF / 只有 CR、标签后只有一个空格、ER 后没值、缺 ER（遇到下一条 TY 或文件尾就收尾）、续行、空行。

use std::borrow::Cow;

use super::common::{
    Fields, add_note, add_tags, append, clean_doi, container_field, creator_from_text, digits, iso_text, new_record, parse_iso, publisher_field,
    put, raw_date, structured_date,
};
use super::{Parsed, Record, RecordDate};

/// 标签行 → （标签，值）。标签是大写字母开头的两位（字母或数字），后面至少一个空格再接 `-`。
/// 不认数字开头的标签：摘要续行里 "20 - 30% of …" 这种不该被当成标签。
pub(super) fn tag_line(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_uppercase() || !(b[1].is_ascii_uppercase() || b[1].is_ascii_digit()) {
        return None;
    }
    let rest = &line[2..];
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.len() == rest.len() {
        return None;
    }
    let value = after_ws.strip_prefix('-')?;
    if !(value.is_empty() || value.starts_with([' ', '\t', '\r'])) {
        return None;
    }
    Some((&line[..2], value.trim()))
}

struct Draft {
    ty: String,
    /// 第几条（1 起）
    n: usize,
    line: usize,
    lines: Vec<String>,
    tags: Vec<(String, String)>,
}

impl Draft {
    /// 续行接到上一个标签的值后面：摘要、笔记、关键词用换行（关键词逐行一个也能拆开），其余用空格
    fn continue_line(&mut self, line: &str) {
        let Some((tag, v)) = self.tags.last_mut() else { return };
        if !v.is_empty() {
            v.push_str(if matches!(tag.as_str(), "AB" | "N1" | "N2" | "KW") { "\n" } else { " " });
        }
        v.push_str(line.trim());
    }
}

pub(super) fn parse(text: &str) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // 老式 Mac 文件只用 CR 换行
    let text: Cow<'_, str> = if !text.contains('\n') && text.contains('\r') { Cow::Owned(text.replace('\r', "\n")) } else { Cow::Borrowed(text) };
    let mut parsed = Parsed::default();
    let mut cur: Option<Draft> = None;
    // 不属于任何记录的连续行（起止行号）
    let mut stray: Option<(usize, usize)> = None;
    let mut count = 0;
    for (i, line) in text.lines().enumerate() {
        let no = i + 1;
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let tagged = tag_line(line);
        if let Some(("TY", ty)) = tagged {
            if let Some(d) = cur.take() {
                parsed.warnings.push(format!("第 {} 条记录（第 {} 行起）缺少 ER 结束行，已在第 {no} 行的下一条 TY 处截断", d.n, d.line));
                parsed.records.push(to_record(d));
            }
            flush_stray(&mut parsed, &mut stray, count == 0);
            count += 1;
            cur = Some(Draft { ty: ty.to_string(), n: count, line: no, lines: vec![line.to_string()], tags: Vec::new() });
            continue;
        }
        let Some(d) = cur.as_mut() else {
            stray = Some((stray.map_or(no, |s| s.0), no));
            continue;
        };
        d.lines.push(line.to_string());
        match tagged {
            Some(("ER", _)) => {
                if let Some(d) = cur.take() {
                    parsed.records.push(to_record(d));
                }
            }
            Some((tag, v)) => d.tags.push((tag.to_string(), v.to_string())),
            None => d.continue_line(line),
        }
    }
    if let Some(d) = cur.take() {
        parsed.warnings.push(format!("第 {} 条记录（第 {} 行起）没有 ER 结束行，已按文件结尾收尾", d.n, d.line));
        parsed.records.push(to_record(d));
    }
    if count == 0 {
        return Err("不是有效的 RIS：没有找到以 TY 开头的记录".into());
    }
    flush_stray(&mut parsed, &mut stray, false);
    Ok(parsed)
}

fn flush_stray(parsed: &mut Parsed, stray: &mut Option<(usize, usize)>, before_first: bool) {
    let Some((a, b)) = stray.take() else { return };
    let lines = if a == b { format!("第 {a} 行") } else { format!("第 {a}–{b} 行") };
    parsed.warnings.push(if before_first {
        format!("{lines}在第一条记录之前，不是 RIS 内容，已忽略")
    } else {
        format!("{lines}不属于任何记录（前面缺少 TY 行），已跳过")
    });
}

fn item_type(ty: &str) -> &'static str {
    match ty.trim().to_ascii_uppercase().as_str() {
        "JOUR" | "JFULL" | "INPR" | "EJOUR" => "journalArticle",
        "MGZN" => "magazineArticle",
        "NEWS" => "newspaperArticle",
        "BOOK" | "EBOOK" | "EDBOOK" | "SER" => "book",
        "CHAP" | "ECHAP" => "bookSection",
        "CONF" | "CPAPER" => "conferencePaper",
        "THES" => "thesis",
        "RPRT" | "GOVDOC" => "report",
        "ELEC" | "WEB" => "webpage",
        // 与 Zotero 的 RIS 导入一致；T2 随之进 blogTitle
        "BLOG" => "blogPost",
        "DATA" => "dataset",
        "PAT" => "patent",
        "UNPB" | "MANSCPT" => "manuscript",
        "ENCYC" => "encyclopediaArticle",
        "DICT" => "dictionaryEntry",
        "COMP" => "computerProgram",
        "STAND" => "standard",
        "STAT" => "statute",
        "CASE" => "case",
        "MAP" => "map",
        "PCOMM" => "letter",
        "VIDEO" | "MPCT" => "film",
        "SOUND" | "MUSIC" => "audioRecording",
        "ART" => "artwork",
        _ => "document",
    }
}

type Pool = Vec<(String, String)>;

/// 取出某标签的全部非空值（按出现顺序）
fn take(pool: &mut Pool, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    pool.retain(|(t, v)| {
        if t != tag {
            return true;
        }
        if !v.is_empty() {
            out.push(v.clone());
        }
        false
    });
    out
}

/// 按优先级取值：第一个有值的标签的第一个值作选中值；其余与选中值不同的写进 `ris:标签`
fn pick(pool: &mut Pool, fields: &mut Fields, order: &[&str]) -> Option<String> {
    let groups: Vec<(&str, Vec<String>)> = order.iter().map(|t| (*t, take(pool, t))).collect();
    let chosen = groups.iter().find_map(|(_, vs)| vs.first().cloned());
    for (tag, vs) in &groups {
        for v in vs.iter().filter(|v| Some(*v) != chosen.as_ref()) {
            append(fields, &format!("ris:{tag}"), v);
        }
    }
    chosen
}

fn pick_into(pool: &mut Pool, fields: &mut Fields, key: &str, order: &[&str]) {
    if let Some(v) = pick(pool, fields, order) {
        put(fields, key, &v, &format!("ris:{}", order[0]));
    }
}

fn to_record(d: Draft) -> Record {
    let item_type = item_type(&d.ty);
    let mut r = new_record(item_type, d.lines.join("\n"));
    let mut pool = d.tags;
    // 姓名按出现顺序；"Smith, John" 拆两栏，不含逗号的（机构、中文单栏姓名）作 literal
    pool.retain(|(tag, v)| {
        let role = match tag.as_str() {
            "AU" | "A1" => "author",
            "A2" | "ED" => "editor",
            "A3" => "seriesEditor",
            "A4" => "translator",
            _ => return true,
        };
        r.creators.extend(creator_from_text(role, v));
        false
    });

    let f = &mut r.fields;
    pick_into(&mut pool, f, "title", &["TI", "T1"]);
    // 期刊名 / 书名 / 论文集名……看类型；类型没有所属出版物的，这几个标签留到最后进 ris:
    if let Some(k) = container_field(item_type) {
        pick_into(&mut pool, f, k, &["JF", "T2", "JO"]);
    }
    pick_into(&mut pool, f, "journalAbbreviation", &["JA", "J2"]);
    pick_into(&mut pool, f, "series", &["T3"]);

    // DA 有完整日期（至少到月）时优先，否则 PY
    let da_full = pool.iter().any(|(t, v)| t == "DA" && ris_date(v).is_some_and(|d| d.month.is_some()));
    let order: &[&str] = if da_full { &["DA", "PY", "Y1"] } else { &["PY", "Y1", "DA"] };
    r.date = pick(&mut pool, f, order).and_then(|v| ris_date(&v));

    let sp = take(&mut pool, "SP");
    let ep = take(&mut pool, "EP");
    let (pages, ep_used) = match (sp.first(), ep.first()) {
        (Some(s), _) if s.contains(['-', '–']) => (Some(s.clone()), 0),
        (Some(s), Some(e)) if s == e => (Some(s.clone()), 1),
        (Some(s), Some(e)) => (Some(format!("{s}-{e}")), 1),
        (Some(s), None) => (Some(s.clone()), 0),
        (None, _) => (None, 0),
    };
    if let Some(p) = pages {
        put(f, "pages", &p, "ris:SP");
    }
    for v in sp.iter().skip(1) {
        append(f, "ris:SP", v);
    }
    for v in ep.iter().skip(ep_used) {
        append(f, "ris:EP", v);
    }

    for (i, v) in take(&mut pool, "SN").iter().enumerate() {
        if i == 0 {
            put(f, isbn_or_issn(item_type, v), v, "ris:SN");
        } else {
            append(f, "ris:SN", v);
        }
    }
    if let Some(v) = pick(&mut pool, f, &["DO"]) {
        put(f, "DOI", &clean_doi(&v), "ris:DO");
    }
    if let Some(v) = pick(&mut pool, f, &["Y2"]) {
        let text = match ris_date(&v) {
            Some(RecordDate { year: Some(y), month, day, .. }) => iso_text(y, month, day),
            _ => v,
        };
        put(f, "accessDate", &text, "ris:Y2");
    }
    pick_into(&mut pool, f, "url", &["UR"]);
    pick_into(&mut pool, f, "abstractNote", &["AB", "N2"]);
    for (key, order) in [
        ("volume", &["VL"][..]),
        ("issue", &["IS"]),
        (publisher_field(item_type), &["PB"]),
        ("place", &["CY", "PP"]),
        ("edition", &["ET"]),
        ("shortTitle", &["ST"]),
        ("language", &["LA"]),
        ("libraryCatalog", &["DB", "DP"]),
        // AN 是数据库的收录号（如 WOS:000…），Zotero 也放「库中位置」
        ("archiveLocation", &["AN"]),
        ("callNumber", &["CN"]),
    ] {
        pick_into(&mut pool, f, key, order);
    }
    for v in take(&mut pool, "KW") {
        add_tags(&mut r.tags, &v, &[';', '\n']);
    }
    for v in take(&mut pool, "N1") {
        add_note(&mut r.notes, &v);
    }
    r.source_key = pick(&mut pool, f, &["ID"]);
    for (tag, v) in pool {
        append(f, &format!("ris:{tag}"), &v);
    }
    r
}

/// SN 既可能是 ISBN 也可能是 ISSN：按位数判断最可靠（8 位 ISSN，10 / 13 位 ISBN），判断不了再看类型
fn isbn_or_issn(item_type: &str, v: &str) -> &'static str {
    match v.chars().filter(|c| c.is_ascii_digit() || matches!(c, 'X' | 'x')).count() {
        8 => "ISSN",
        10 | 13 => "ISBN",
        _ if matches!(item_type, "book" | "bookSection") => "ISBN",
        _ => "ISSN",
    }
}

/// RIS 日期 "2019/03/15/"、"2019///"、"2019/03//Spring"；也认 ISO。认不出只留原文
fn ris_date(v: &str) -> Option<RecordDate> {
    let v = v.trim();
    if !v.contains('/') {
        return match parse_iso(v) {
            Some((y, m, d)) => Some(structured_date(Some(v.to_string()), y, m, d)),
            None => raw_date(v),
        };
    }
    let mut parts = v.split('/');
    let year = parts.next().map(str::trim).filter(|y| y.len() == 4).and_then(|y| digits(y, 4)).and_then(|y| i32::try_from(y).ok());
    match year {
        Some(y) => {
            let m = parts.next().and_then(|m| digits(m, 2));
            let d = parts.next().and_then(|d| digits(d, 2));
            Some(structured_date(Some(v.to_string()), y, m, d))
        }
        None => raw_date(v),
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

    // BOM + CRLF；第一条之前有垃圾行、第二条缺 ER、第三条后面有不属于任何记录的行、最后一条到文件尾都没有 ER
    const SAMPLE: &str = "\u{feff}Exported from Test DB\r\nfoo bar\r\n\r\n\
        TY  - JOUR\r\n\
        TI  - 气候变化研究\r\n\
        T1  - Alternative title\r\n\
        AU  - Smith, John\r\n\
        AU  - 王小明\r\n\
        A1  - World Bank\r\n\
        A2  - Editor, Ed\r\n\
        A4  - Trans, Tina\r\n\
        PY  - 2019///\r\n\
        DA  - 2019/03/15/\r\n\
        JO  - J. Tests\r\n\
        JF  - Journal of Tests\r\n\
        JA  - J Test\r\n\
        VL  - 12\r\n\
        IS  - 3\r\n\
        SP  - 100\r\n\
        EP  - 120\r\n\
        SN  - 1234-5678\r\n\
        DO  - https://doi.org/10.1000/abc\r\n\
        UR  - https://a.example\r\n\
        UR  - https://b.example\r\n\
        KW  - climate; models\r\n\
        KW  - climate\r\n\
        KW  - ocean\r\n\
        N1  - 第一条笔记\r\n\
        N1  - Second note\r\n\
        AB  - First line of abstract\r\n\
        continued here.\r\n\
        N2  - Different abstract\r\n\
        M3  - one\r\n\
        M3  - two\r\n\
        LA  - zh\r\n\
        ID  - rec1\r\n\
        AN  - WOS:0001\r\n\
        DB  - Test DB\r\n\
        Y2  - 2024/01/05/\r\n\
        ER  - \r\n\r\n\
        TY - BOOK\r\n\
        TI - 一本书\r\n\
        AU - 李四\r\n\
        ED - Smith, Jane\r\n\
        A3 - Series, Ed\r\n\
        PY - 2001\r\n\
        PB - 商务印书馆\r\n\
        CY - 北京\r\n\
        ET - 2\r\n\
        SN - 978-7-100-00000-0\r\n\
        T3 - 汉译名著\r\n\
        TY  - CHAP\r\n\
        TI  - A Chapter\r\n\
        T2  - The Book\r\n\
        SP  - 5-9\r\n\
        PY  - Spring 2020\r\n\
        ER  -\r\n\
        AU  - Orphan, A\r\n\
        TI  - Orphan title\r\n\
        TY  - CONF\r\n\
        T2  - Proc. Test\r\n\
        TI  - Talk\r\n\
        KW  - a\r\n\
        b\r\n\
        c;a\r\n";

    #[test]
    fn records_fields_and_creators() {
        let p = parse(SAMPLE).unwrap();
        let types: Vec<_> = p.records.iter().map(|r| r.item_type.as_str()).collect();
        assert_eq!(types, ["journalArticle", "book", "bookSection", "conferencePaper"]);

        let a = &p.records[0];
        assert_eq!(field(a, "title"), Some("气候变化研究"));
        assert_eq!(field(a, "ris:T1"), Some("Alternative title"));
        assert_eq!(
            names(a),
            [
                ("author", Some("Smith"), Some("John"), None),
                ("author", None, None, Some("王小明")),
                ("author", None, None, Some("World Bank")),
                ("editor", Some("Editor"), Some("Ed"), None),
                ("translator", Some("Trans"), Some("Tina"), None),
            ]
        );
        assert_eq!(a.date, Some(RecordDate { raw: "2019/03/15/".into(), year: Some(2019), month: Some(3), day: Some(15) }));
        assert_eq!(field(a, "ris:PY"), Some("2019///"));
        assert_eq!(field(a, "publicationTitle"), Some("Journal of Tests"));
        assert_eq!(field(a, "ris:JO"), Some("J. Tests"));
        assert_eq!(field(a, "journalAbbreviation"), Some("J Test"));
        assert_eq!((field(a, "volume"), field(a, "issue"), field(a, "pages")), (Some("12"), Some("3"), Some("100-120")));
        assert_eq!((field(a, "ISSN"), field(a, "DOI")), (Some("1234-5678"), Some("10.1000/abc")));
        assert_eq!((field(a, "url"), field(a, "ris:UR")), (Some("https://a.example"), Some("https://b.example")));
        assert_eq!(a.tags, ["climate", "models", "ocean"]);
        assert_eq!(a.notes, ["第一条笔记", "Second note"]);
        assert_eq!(field(a, "abstractNote"), Some("First line of abstract\ncontinued here."));
        assert_eq!(field(a, "ris:N2"), Some("Different abstract"));
        assert_eq!(field(a, "ris:M3"), Some("one\ntwo"));
        assert_eq!((field(a, "language"), field(a, "archiveLocation"), field(a, "libraryCatalog")), (Some("zh"), Some("WOS:0001"), Some("Test DB")));
        assert_eq!(field(a, "accessDate"), Some("2024-01-05"));
        assert_eq!(a.source_key.as_deref(), Some("rec1"));
        assert!(a.raw.starts_with("TY  - JOUR\nTI  - 气候变化研究\n") && a.raw.ends_with("Y2  - 2024/01/05/\nER  -"), "{}", a.raw);
        assert!(!a.fields.keys().any(|k| k == "ris:TY" || k == "ris:ER" || k == "date"));

        let b = &p.records[1];
        assert_eq!(field(b, "title"), Some("一本书"));
        assert_eq!(names(b), [("author", None, None, Some("李四")), ("editor", Some("Smith"), Some("Jane"), None), ("seriesEditor", Some("Series"), Some("Ed"), None)]);
        assert_eq!(b.date, Some(RecordDate { raw: "2001".into(), year: Some(2001), month: None, day: None }));
        assert_eq!((field(b, "publisher"), field(b, "place"), field(b, "edition")), (Some("商务印书馆"), Some("北京"), Some("2")));
        assert_eq!((field(b, "ISBN"), field(b, "series")), (Some("978-7-100-00000-0"), Some("汉译名著")));

        let c = &p.records[2];
        assert_eq!((field(c, "bookTitle"), field(c, "pages")), (Some("The Book"), Some("5-9")));
        assert_eq!(c.date, Some(RecordDate { raw: "Spring 2020".into(), year: None, month: None, day: None }));

        let d = &p.records[3];
        assert_eq!((field(d, "proceedingsTitle"), field(d, "title")), (Some("Proc. Test"), Some("Talk")));
        assert_eq!(d.tags, ["a", "b", "c"]);

        assert_eq!(
            p.warnings,
            [
                "第 1–2 行在第一条记录之前，不是 RIS 内容，已忽略",
                "第 2 条记录（第 42 行起）缺少 ER 结束行，已在第 53 行的下一条 TY 处截断",
                "第 59–60 行不属于任何记录（前面缺少 TY 行），已跳过",
                "第 4 条记录（第 61 行起）没有 ER 结束行，已按文件结尾收尾",
            ]
        );
    }

    #[test]
    fn types_dates_and_odd_lines() {
        let src = "TY  - BLOG\nT2  - My Blog\nTI  - Post\nER  - \nTY  - THES\nM3  - PhD\nER  - \nTY  - GEN\nER  - \nTY  - ELEC\nER  - \n\
                   TY  - MGZN\nER  - \nTY  - XYZ\nAB  - 20 - 30% of cases\n20 - 30% again\nER  - \n";
        let p = parse(src).unwrap();
        let types: Vec<_> = p.records.iter().map(|r| r.item_type.as_str()).collect();
        assert_eq!(types, ["blogPost", "thesis", "document", "webpage", "magazineArticle", "document"]);
        assert_eq!(field(&p.records[0], "blogTitle"), Some("My Blog"));
        assert_eq!(field(&p.records[1], "ris:M3"), Some("PhD"));
        assert_eq!(field(&p.records[5], "abstractNote"), Some("20 - 30% of cases\n20 - 30% again"));
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);

        // 书的 T2 没有对应字段 → ris:T2；只有 EP → ris:EP；SN 判断不了位数时看类型
        let r = &parse("TY  - BOOK\nT2  - Series?\nEP  - 9\nSN  - 12345\nER  -").unwrap().records[0];
        assert_eq!((field(r, "ris:T2"), field(r, "ris:EP"), field(r, "ISBN")), (Some("Series?"), Some("9"), Some("12345")));

        assert_eq!(ris_date("2019/03//Spring").map(|d| (d.year, d.month, d.day)), Some((Some(2019), Some(3), None)));
        assert_eq!(ris_date("2019-03-15").map(|d| (d.year, d.month, d.day)), Some((Some(2019), Some(3), Some(15))));
        assert_eq!(ris_date("/03/15/").map(|d| (d.raw, d.year)), Some(("/03/15/".into(), None)));
        assert_eq!(ris_date("  "), None);
    }

    #[test]
    fn tag_lines_and_invalid_files() {
        assert_eq!(tag_line("TY  - JOUR"), Some(("TY", "JOUR")));
        assert_eq!(tag_line("TY - JOUR"), Some(("TY", "JOUR")));
        assert_eq!(tag_line("ER  -"), Some(("ER", "")));
        assert_eq!(tag_line("A1  - Smith, J."), Some(("A1", "Smith, J.")));
        assert_eq!(tag_line("TY-JOUR"), None);
        assert_eq!(tag_line("TY  -JOUR"), None);
        assert_eq!(tag_line("12 - 15 May"), None);
        assert_eq!(tag_line("ty  - jour"), None);

        assert!(parse("just\nsome text").unwrap_err().contains("RIS"));
        // 只有 CR 换行
        let p = parse("TY  - JOUR\rTI  - Old Mac\rER  - \r").unwrap();
        assert_eq!(field(&p.records[0], "title"), Some("Old Mac"));
    }
}
