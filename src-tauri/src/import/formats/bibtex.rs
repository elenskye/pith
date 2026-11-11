//! BibTeX / BibLaTeX → Record。语法、@string、`#` 拼接、LaTeX 转义（{\"u} → ü、-- → –）交给 biblatex crate，
//! 这里做三件事：坏条目兜底（逐条切开再解析）、Zotero 字段映射、简单 LaTeX 格式命令去壳。
//!
//! 每条单独交给 `Bibliography::from_raw`：一条的缩写未定义、键重复不会连累整个文件；
//! 也就不用 biblatex 的 crossref 解析（互相引用的条目会让它无限递归），改在映射后做一层简单继承（`inherit_crossrefs`）。

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

use biblatex::{Bibliography, ChunksExt, ChunksRef, Pair, ParseErrorKind, Person, RawBibliography, RawChunk, RawEntry, Spanned, Token};

use super::common::{
    Lines, add_note, add_tags, append, clean_doi, container_field, digits, iso_text, join_words, new_record, non_empty, parse_iso,
    publisher_field, put, raw_date, structured_date,
};
use super::{Parsed, Record, RecordCreator, RecordDate};

pub(super) fn parse(text: &str) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parsed = Parsed::default();
    // biblatex 内部有 unwrap，用户文件千奇百怪：它 panic 一律当作语法错误，走兜底
    let found = match catch_unwind(|| RawBibliography::parse(text)).ok().and_then(Result::ok) {
        Some(mut raw) => {
            drop_cyclic_strings(&mut raw.abbreviations, &mut parsed.warnings);
            let lines = Lines::new(text);
            for (i, entry) in raw.entries.iter().enumerate() {
                add_entry(&mut parsed, i + 1, lines.line_at(entry.span.start), entry, &raw.abbreviations, entry_text(text, &entry.span));
            }
            !raw.entries.is_empty() || !raw.abbreviations.is_empty() || has_entry_head(text)
        }
        None => fallback(text, &mut parsed),
    };
    if !found {
        return Err("不是有效的 BibTeX：没有找到 @article{…} 这样的条目".into());
    }
    inherit_crossrefs(&mut parsed.records);
    Ok(parsed)
}

/// 文本里有没有 `@类型{` / `@类型(`（格式识别也用）
pub(super) fn has_entry_head(text: &str) -> bool {
    text.match_indices('@').any(|(i, _)| entry_head(text, i).is_some())
}

/// 原文里这一条的完整文本。biblatex 给的范围不含末尾的 `}`，补上。
fn entry_text(text: &str, span: &Range<usize>) -> Option<String> {
    let rest = text.get(span.end..)?;
    let trimmed = rest.trim_start();
    let end = if trimmed.starts_with(['}', ')']) { span.end + (rest.len() - trimmed.len()) + 1 } else { span.end };
    text.get(span.start..end).map(str::to_string)
}

fn add_entry<'s>(parsed: &mut Parsed, n: usize, line: usize, entry: &Spanned<RawEntry<'s>>, abbr: &[Pair<'s>], raw: Option<String>) {
    match catch_unwind(AssertUnwindSafe(|| convert(entry, abbr, raw))) {
        Ok(Ok(record)) => parsed.records.push(record),
        Ok(Err(why)) => parsed.warnings.push(skip_msg(n, entry.v.key.v, line, &why)),
        Err(_) => parsed.warnings.push(skip_msg(n, entry.v.key.v, line, "解析器内部错误")),
    }
}

fn skip_msg(n: usize, key: &str, line: usize, why: &str) -> String {
    let key = key.trim();
    let key = if key.is_empty() { String::new() } else { format!("键 {key}，") };
    format!("第 {n} 条（{key}第 {line} 行）无法解析，已跳过：{why}")
}

fn describe(kind: &ParseErrorKind) -> String {
    let token = |t: &Token| match t {
        Token::Identifier => "名称",
        Token::OpeningBrace => "左花括号 {",
        Token::ClosingBrace => "右花括号 }",
        Token::Comma => "逗号",
        Token::QuotationMark => "双引号",
        Token::Equals => "等号",
        Token::DecimalPoint => "小数点",
    };
    match kind {
        ParseErrorKind::UnexpectedEof => "条目没有正常结束（括号不配对？）".into(),
        ParseErrorKind::Expected(t) => format!("缺少{}", token(t)),
        ParseErrorKind::Unexpected(t) => format!("多余的{}", token(t)),
        ParseErrorKind::UnknownAbbreviation(s) => format!("字符串缩写 {s} 没有定义"),
        ParseErrorKind::MalformedCommand => "LaTeX 命令写法有误".into(),
        ParseErrorKind::DuplicateKey(s) => format!("键 {s} 重复"),
        _ => "语法错误".into(),
    }
}

/// `@string{a = b} @string{b = a}` 这种互相引用会让 biblatex 无限递归（栈溢出兜不住），先剔掉
fn drop_cyclic_strings(abbr: &mut Vec<Pair<'_>>, warnings: &mut Vec<String>) {
    let refs: HashMap<&str, Vec<&str>> = abbr.iter().map(|p| (p.key.v, abbreviation_refs(&p.value.v))).collect();
    let mut bad: Vec<String> = Vec::new();
    for &start in refs.keys() {
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(k) = stack.pop() {
            let next = refs.get(k).map(Vec::as_slice).unwrap_or_default();
            if next.contains(&start) {
                bad.push(start.to_string());
                break;
            }
            stack.extend(next.iter().filter(|n| seen.insert(**n)));
        }
    }
    if !bad.is_empty() {
        bad.sort();
        warnings.push(format!("字符串缩写 {} 互相引用、无法展开，已忽略这些定义", bad.join("、")));
        abbr.retain(|p| !bad.iter().any(|b| b == p.key.v));
    }
}

fn abbreviation_refs<'s>(field: &[Spanned<RawChunk<'s>>]) -> Vec<&'s str> {
    field
        .iter()
        .filter_map(|c| match c.v {
            RawChunk::Abbreviation(s) => Some(s),
            RawChunk::Normal(_) => None,
        })
        .collect()
}

fn convert<'s>(entry: &Spanned<RawEntry<'s>>, abbr: &[Pair<'s>], raw: Option<String>) -> Result<Record, String> {
    // crossref / xdata 不交给 biblatex：它只看得到本条，自引用会无限递归。原文另存进 bibtex:crossref，映射后再继承
    let mut own = entry.clone();
    let mut refs = Vec::new();
    own.v.fields.retain(|p| {
        let k = p.key.v.to_ascii_lowercase();
        let keep = k != "crossref" && k != "xdata";
        if !keep {
            refs.push((k, raw_value(&p.value.v)));
        }
        keep
    });
    let mut abbr = abbr.to_vec();
    let mut warnings = Vec::new();
    let e = loop {
        let one = RawBibliography { preamble: String::new(), entries: vec![own.clone()], abbreviations: abbr.clone() };
        let err = match Bibliography::from_raw(one) {
            Ok(bib) => break bib.into_vec().into_iter().next().ok_or("条目为空")?,
            Err(err) => err,
        };
        // 未定义的缩写：BibTeX 本身不分大小写，先找大小写不同的定义；找不到就把缩写名当原文保留
        let ParseErrorKind::UnknownAbbreviation(name) = &err.kind else { return Err(describe(&err.kind)) };
        let mut used = own.v.fields.iter().chain(abbr.iter()).flat_map(|p| abbreviation_refs(&p.value.v));
        let Some(s) = used.find(|s| *s == name.as_str()) else { return Err(describe(&err.kind)) };
        if abbr.iter().any(|p| p.key.v == s) {
            return Err(describe(&err.kind));
        }
        let alias = abbr.iter().find(|p| p.key.v.eq_ignore_ascii_case(s) && abbreviation_refs(&p.value.v).is_empty());
        let value = match alias {
            Some(p) => p.value.clone(),
            None => {
                warnings.push(format!("字符串缩写 {s} 没有定义，按原文保留"));
                Spanned::zero(vec![Spanned::zero(RawChunk::Normal(s))])
            }
        };
        abbr.push(Pair::new(Spanned::zero(s), value));
    };

    let kind = entry.v.kind.v.to_ascii_lowercase();
    let item_type = item_type(&kind);
    let raw = raw.unwrap_or_else(|| e.to_bibtex_string().unwrap_or_else(|_| e.to_biblatex_string()));
    let mut r = new_record(item_type, raw);
    r.warnings = warnings;
    let key = e.key.trim();
    if !key.is_empty() {
        r.source_key = Some(key.to_string());
        r.fields.insert("citationKey".into(), key.to_string());
    }

    for (role, field) in [("author", "author"), ("editor", "editor"), ("translator", "translator"), ("bookAuthor", "bookauthor")] {
        if let Some(chunks) = e.get(field) {
            add_people(&mut r, role, &chunks.parse::<Vec<Person>>().unwrap_or_default());
        }
    }

    let text = |name: &str| e.get(name).map(|c| field_text(name, c)).filter(|s| !s.is_empty());
    let has_date = text("date").is_some();
    r.date = entry_date(text("date"), text("year"), text("month"), text("day"));

    for (name, chunks) in &e.fields {
        let value = field_text(name, chunks);
        let raw_key = format!("bibtex:{name}");
        let f = &mut r.fields;
        let zotero = match name.as_str() {
            "author" | "editor" | "translator" | "bookauthor" | "date" => continue,
            "year" | "month" | "day" if !has_date => continue,
            "title" => Some("title"),
            "journal" | "journaltitle" => Some("publicationTitle"),
            "booktitle" => match item_type {
                "bookSection" => Some("bookTitle"),
                "conferencePaper" => Some("proceedingsTitle"),
                _ => None,
            },
            "number" => match item_type {
                "journalArticle" => Some("issue"),
                "report" => Some("reportNumber"),
                "book" | "bookSection" => Some("seriesNumber"),
                "patent" => Some("patentNumber"),
                _ => None,
            },
            "volume" => Some("volume"),
            "issue" => Some("issue"),
            "edition" => Some("edition"),
            "series" => Some("series"),
            "publisher" => Some(publisher_field(item_type)),
            "address" | "location" => Some("place"),
            "school" => Some(if item_type == "report" { "institution" } else { "university" }),
            "institution" => match item_type {
                "report" => Some("institution"),
                "thesis" => Some("university"),
                _ => None,
            },
            "pages" => {
                put(f, "pages", &value.replace(['–', '—'], "-"), &raw_key);
                continue;
            }
            "doi" => {
                put(f, "DOI", &clean_doi(&value), &raw_key);
                continue;
            }
            "isbn" => Some("ISBN"),
            "issn" => Some("ISSN"),
            "url" => Some("url"),
            "urldate" => Some("accessDate"),
            "abstract" => Some("abstractNote"),
            "keywords" => {
                add_tags(&mut r.tags, &value, &[',', ';']);
                continue;
            }
            "note" => {
                append(f, "extra", &value);
                continue;
            }
            "annote" | "annotation" => {
                add_note(&mut r.notes, &value);
                continue;
            }
            "language" | "langid" => Some("language"),
            "type" => match item_type {
                "thesis" => Some("thesisType"),
                "report" => Some("reportType"),
                _ => None,
            },
            "shorttitle" => Some("shortTitle"),
            "pagetotal" => Some("numPages"),
            "eventtitle" => Some("conferenceName"),
            "shortjournal" => Some("journalAbbreviation"),
            "version" => Some("versionNumber"),
            _ => None,
        };
        put(f, zotero.unwrap_or(&raw_key), &value, &raw_key);
    }
    for (k, v) in refs {
        let k = format!("bibtex:{k}");
        put(&mut r.fields, &k, &v, &k);
    }
    let default_thesis = match kind.as_str() {
        "phdthesis" => Some("PhD thesis"),
        "mastersthesis" => Some("Master's thesis"),
        _ => None,
    };
    if let Some(t) = default_thesis {
        r.fields.entry("thesisType".into()).or_insert_with(|| t.into());
    }
    Ok(r)
}

/// 未经 biblatex 解析的字段原文（crossref 的键）
fn raw_value(field: &[Spanned<RawChunk<'_>>]) -> String {
    let s: String = field
        .iter()
        .map(|c| match c.v {
            RawChunk::Normal(s) | RawChunk::Abbreviation(s) => s,
        })
        .collect();
    s.trim().to_string()
}

fn item_type(kind: &str) -> &'static str {
    match kind {
        "article" => "journalArticle",
        "book" | "booklet" | "proceedings" | "collection" | "mvbook" | "mvcollection" | "mvproceedings" | "reference" | "mvreference"
        | "manual" => "book",
        "inbook" | "incollection" | "bookinbook" | "suppbook" | "suppcollection" => "bookSection",
        "inreference" => "encyclopediaArticle",
        "inproceedings" | "conference" => "conferencePaper",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "electronic" | "www" => "webpage",
        "unpublished" => "manuscript",
        "patent" => "patent",
        "dataset" => "dataset",
        "software" => "computerProgram",
        "letter" => "letter",
        "artwork" => "artwork",
        "audio" => "audioRecording",
        "video" | "movie" => "film",
        "standard" => "standard",
        "legislation" => "statute",
        "jurisdiction" => "case",
        _ => "document",
    }
}

/// 字段值 → 纯文本：biblatex 已处理转义和花括号，这里再剥掉格式命令外壳；网址、文件路径原样
fn field_text(name: &str, chunks: ChunksRef<'_>) -> String {
    let s = chunks.format_verbatim();
    if matches!(name, "url" | "doi" | "file" | "pdf" | "eprint" | "urlraw") { s.trim().to_string() } else { strip_latex(&s).trim().to_string() }
}

fn add_people(r: &mut Record, role: &str, people: &[Person]) {
    for p in people {
        let (name, given, prefix, suffix) = (strip_latex(&p.name), strip_latex(&p.given_name), strip_latex(&p.prefix), strip_latex(&p.suffix));
        if name.trim() == "others" && given.trim().is_empty() && prefix.trim().is_empty() {
            let who = match role {
                "author" => "作者",
                "editor" => "编者",
                "translator" => "译者",
                _ => "原书作者",
            };
            r.warnings.push(format!("{who}列表里的 others（等人）已忽略"));
            continue;
        }
        // von 部分并进姓（"van Gogh"）；Jr. 之类后缀接在名后面（"John, Jr."），与 Zotero 两栏姓名的约定一致。
        // 没有名的（{World Health Organization}、王小明）作单栏 literal。
        let family = join_words(&[&prefix, &name]);
        let creator = match (non_empty(&given), non_empty(&suffix)) {
            (Some(g), suffix) => {
                let given = Some(match suffix {
                    Some(s) => format!("{g}, {s}"),
                    None => g,
                });
                RecordCreator { role: role.into(), family, given, literal: None }
            }
            (None, suffix) => {
                let Some(literal) = family else { continue };
                let literal = match suffix {
                    Some(s) => format!("{literal}, {s}"),
                    None => literal,
                };
                RecordCreator { role: role.into(), family: None, given: None, literal: Some(literal) }
            }
        };
        r.creators.push(creator);
    }
}

/// date 优先（biblatex，原文如 "1972-05"）；否则 year + month (+ day)，原文拼成 "March 2019" / "2019-03"
fn entry_date(date: Option<String>, year: Option<String>, month: Option<String>, day: Option<String>) -> Option<RecordDate> {
    if let Some(d) = date {
        return match parse_iso(&d) {
            Some((y, m, dd)) => Some(structured_date(Some(d), y, m, dd)),
            None => raw_date(&d),
        };
    }
    let y_num = year.as_deref().and_then(|y| digits(y, 4)).and_then(|y| i32::try_from(y).ok());
    let m_num = month.as_deref().and_then(month_number);
    let d_num = day.as_deref().and_then(|d| digits(d, 2));
    let raw = match (year.as_deref(), month.as_deref()) {
        (None, None) => return None,
        (Some(y), None) => y.to_string(),
        (y, Some(m)) => match (y_num, digits(m, 2)) {
            (Some(yn), Some(_)) => iso_text(yn, m_num, d_num.filter(|_| m_num.is_some())),
            _ => {
                let y = y.unwrap_or("");
                match day.as_deref() {
                    Some(d) => format!("{m} {d}, {y}"),
                    None => format!("{m} {y}"),
                }
            }
        },
    };
    match y_num {
        Some(y) => Some(structured_date(Some(raw.trim().to_string()), y, m_num, d_num)),
        None => raw_date(&raw),
    }
}

/// "3"、"03"、"mar"、"March"、"Sept." → 1–12
fn month_number(m: &str) -> Option<u32> {
    const MONTHS: [&str; 12] =
        ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    if let Some(n) = digits(m, 2) {
        return (1..=12).contains(&n).then_some(n);
    }
    let l = m.trim().trim_end_matches('.').to_lowercase();
    if l.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|full| full.starts_with(&l)).map(|i| i as u32 + 1)
}

/// 格式命令去壳：`\emph{x}` → x、`{\em x}`（biblatex 给出 "\em x"）→ x、`~` → 空格；认不出的命令原样保留
fn strip_latex(s: &str) -> String {
    const WRAPPERS: [&str; 27] = [
        "emph", "textit", "textbf", "textsc", "textsl", "texttt", "textrm", "textsf", "textup", "textmd", "textnormal", "underline", "uline",
        "url", "mkbibquote", "mkbibemph", "mkbibitalic", "mkbibbold", "enquote", "textsuperscript", "textsubscript", "mbox", "hbox",
        "nocase", "NoCaseChange", "MakeUppercase", "MakeLowercase",
    ];
    // 第一个参数（网址、语言名、颜色）丢掉，留第二个
    const TWO_ARGS: [&str; 3] = ["href", "foreignlanguage", "textcolor"];
    const DECLS: [&str; 18] = [
        "em", "it", "bf", "sc", "sl", "tt", "rm", "sf", "itshape", "bfseries", "scshape", "slshape", "upshape", "mdseries", "normalfont",
        "ttfamily", "relax", "protect",
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(['\\', '~']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if let Some(t) = tail.strip_prefix('~') {
            out.push(' ');
            rest = t;
            continue;
        }
        let name_len = tail[1..].bytes().take_while(u8::is_ascii_alphabetic).count();
        let name = &tail[1..1 + name_len];
        let after = &tail[1 + name_len..];
        if WRAPPERS.contains(&name)
            && let Some((arg, after_arg)) = group(after)
        {
            out.push_str(&if name == "url" { arg.to_string() } else { strip_latex(arg) });
            rest = after_arg;
            continue;
        }
        if TWO_ARGS.contains(&name)
            && let Some((_, after1)) = group(after)
        {
            // 第二个参数 biblatex 往往已经去掉了花括号（当作普通分组），这时只丢第一个参数
            rest = match group(after1) {
                Some((arg2, after2)) => {
                    out.push_str(&strip_latex(arg2));
                    after2
                }
                None => after1,
            };
            continue;
        }
        if name == "par" {
            out.push('\n');
            rest = after.trim_start();
        } else if name.is_empty() && after.starts_with(' ') {
            // `\ `（控制空格）：biblatex 原样给出 "\ " 且后面还跟着一个空格，整个去掉
            rest = &after[1..];
        } else if !name.is_empty() && (DECLS.contains(&name) || WRAPPERS.contains(&name)) {
            rest = after.trim_start_matches(' ');
        } else {
            out.push('\\');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// 以 `{` 开头的分组 → （组内文字，组后剩余）；括号不配对返回 None
fn group(s: &str) -> Option<(&str, &str)> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// crossref 一层继承（不递归）：子条目缺的出版信息从父条目补，父条目标题作子条目的所属出版物名
fn inherit_crossrefs(records: &mut [Record]) {
    let index: HashMap<String, usize> =
        records.iter().enumerate().filter_map(|(i, r)| r.source_key.as_ref().map(|k| (k.to_lowercase(), i))).collect();
    for i in 0..records.len() {
        let Some(&p) = records[i].fields.get("bibtex:crossref").and_then(|k| index.get(&k.trim().to_lowercase())) else { continue };
        if p == i {
            continue;
        }
        let parent = records[p].clone();
        let child = &mut records[i];
        if let (Some(t), Some(k)) = (parent.fields.get("title"), container_field(&child.item_type)) {
            child.fields.entry(k.into()).or_insert_with(|| t.clone());
        }
        for k in ["publisher", "place", "volume", "series", "seriesNumber", "edition", "ISBN", "ISSN", "conferenceName"] {
            if let Some(v) = parent.fields.get(k) {
                child.fields.entry(k.into()).or_insert_with(|| v.clone());
            }
        }
        if child.date.is_none() {
            child.date = parent.date.clone();
        }
        if !child.creators.iter().any(|c| c.role == "editor") {
            child.creators.extend(parent.creators.iter().filter(|c| c.role == "editor").cloned());
        }
    }
}

// ---------- 整个文件语法有错时的兜底 ----------

struct Segment<'a> {
    kind: String,
    key: String,
    line: usize,
    /// 原文
    raw: &'a str,
    /// 交给 biblatex 的文本：圆括号定界的条目改成花括号（biblatex 只认花括号）
    text: String,
    closed: bool,
}

/// 按顶层 `@类型{` 切成逐条文本；@string 先单独试、好的拼在一起作缩写表；每条再单独解析，坏的跳过并记 warning
fn fallback(text: &str, parsed: &mut Parsed) -> bool {
    let segments = split_segments(text);
    let mut strings_src = String::new();
    for seg in segments.iter().filter(|s| s.kind == "string") {
        if catch_unwind(|| RawBibliography::parse(&seg.text)).is_ok_and(|r| r.is_ok()) {
            strings_src.push_str(&seg.text);
            strings_src.push('\n');
        } else {
            parsed.warnings.push(format!("第 {} 行的 @string 定义无法解析，已跳过", seg.line));
        }
    }
    let mut abbr = catch_unwind(|| RawBibliography::parse(&strings_src)).ok().and_then(Result::ok).map(|r| r.abbreviations).unwrap_or_default();
    drop_cyclic_strings(&mut abbr, &mut parsed.warnings);

    for (i, seg) in segments.iter().filter(|s| !matches!(s.kind.as_str(), "string" | "preamble" | "comment")).enumerate() {
        let n = i + 1;
        match catch_unwind(|| RawBibliography::parse(&seg.text)) {
            Ok(Ok(raw)) if seg.closed && raw.entries.len() == 1 => add_entry(parsed, n, seg.line, &raw.entries[0], &abbr, Some(seg.raw.to_string())),
            Ok(Err(e)) if seg.closed => parsed.warnings.push(skip_msg(n, &seg.key, seg.line, &describe(&e.kind))),
            _ if !seg.closed => parsed.warnings.push(skip_msg(n, &seg.key, seg.line, "括号没有闭合")),
            _ => parsed.warnings.push(skip_msg(n, &seg.key, seg.line, "语法错误")),
        }
    }
    !segments.is_empty()
}

fn split_segments(text: &str) -> Vec<Segment<'_>> {
    let lines = Lines::new(text);
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // 条目外的 % 注释行（与 biblatex 一致）
            b'%' => i = text[i..].find('\n').map_or(b.len(), |p| i + p + 1),
            b'@' => match entry_head(text, i) {
                Some((kind, open)) => {
                    let (end, closed) = scan_body(text, open);
                    let raw = &text[i..end];
                    let seg_text = if b[open] == b'(' && closed {
                        format!("{}{{{}}}", &text[i..open], &text[open + 1..end - 1])
                    } else {
                        raw.to_string()
                    };
                    let key = text[open + 1..end].trim_start().split([',', '}', ')', ' ', '\t', '\r', '\n']).next().unwrap_or("").to_string();
                    out.push(Segment { kind, key, line: lines.line_at(i), raw, text: seg_text, closed });
                    i = end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    out
}

/// `@类型{` 或 `@类型(`：返回（小写类型名，定界符位置）
fn entry_head(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = text.get(at + 1..)?;
    let name_len = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    if name_len == 0 {
        return None;
    }
    let after = &rest[name_len..];
    let open = at + 1 + name_len + (after.len() - after.trim_start().len());
    matches!(text.as_bytes().get(open), Some(b'{' | b'(')).then(|| (rest[..name_len].to_ascii_lowercase(), open))
}

/// 从定界符开始找配对的结束位置：返回（结束后的位置，是否正常闭合）
fn scan_body(text: &str, open: usize) -> (usize, bool) {
    let b = text.as_bytes();
    let paren = b[open] == b'(';
    let mut depth = 0usize;
    let mut quoted = false;
    let mut line_start = false;
    let mut j = open + 1;
    while j < b.len() {
        let c = b[j];
        if c == b'\n' {
            line_start = true;
            j += 1;
            continue;
        }
        if line_start && matches!(c, b' ' | b'\t' | b'\r') {
            j += 1;
            continue;
        }
        // 本条还没闭合就在行首遇到下一个 `@类型{`：多半是本条括号不配对，在这里切断，免得吞掉后面所有条目
        if line_start && c == b'@' && entry_head(text, j).is_some() {
            return (j, false);
        }
        line_start = false;
        match c {
            b'\\' => {
                j += 2;
                continue;
            }
            b'"' if depth == 0 => quoted = !quoted,
            _ if quoted => {}
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'}' if !paren => return (j + 1, true),
            b')' if paren && depth == 0 => return (j + 1, true),
            _ => {}
        }
        j += 1;
    }
    (b.len(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(src: &str) -> Record {
        let p = parse(src).unwrap();
        assert_eq!(p.records.len(), 1, "{:?}", p.warnings);
        p.records.into_iter().next().unwrap()
    }

    fn field<'a>(r: &'a Record, k: &str) -> Option<&'a str> {
        r.fields.get(k).map(String::as_str)
    }

    /// （角色，姓，名，单栏）
    type Name = (String, Option<String>, Option<String>, Option<String>);

    fn names(r: &Record) -> Vec<Name> {
        r.creators.iter().map(|c| (c.role.clone(), c.family.clone(), c.given.clone(), c.literal.clone())).collect()
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    const MULTI: &str = r#"
% 合成样例
@string{ jgr = "Journal of Geophysical Research" }
@comment{ 这里随便写 }

@Article{smith2019,
  author    = {Smith, John and {World Health Organization} and 王小明},
  title     = {The {NASA} Mission: \emph{A} Study of {M\"uller}'s Data},
  journal   = jgr # " Atmospheres",
  year      = 2019,
  month     = mar,
  volume    = {12},
  number    = {3},
  pages     = {100--120},
  doi       = {https://doi.org/10.1000/xyz123},
  keywords  = {climate; models, climate ,  ocean},
  note      = {Accepted},
  annote    = {读后感：方法可借鉴},
  abstract  = {We study \textbf{things}.},
  file      = {:smith2019.pdf:PDF},
  howpublished = {\url{https://example.com/~smith}}
}

@inproceedings{lee2020,
  author = {Lee, Ann},
  editor = {Brown, Bob and others},
  title = {Fast Things},
  booktitle = {Proceedings of the Test Conference},
  publisher = {ACM},
  address = {New York},
  date = {2020-07-14},
  year = {2021},
  organization = {Test Org}
}

@incollection{chap1, title = {A Chapter}, booktitle = {A Book}, number = {7}, series = {LNCS}, translator = {Doe, Jane}, year = {1999}}
@phdthesis{phd1, author = {Zhang, San}, title = {论文题目}, school = {北京大学}, year = {2018}}
@mastersthesis{ms1, title = {MS}, school = {MIT}, type = {M.Eng. thesis}, year = {2001}}
@techreport{tr1, title = {Report}, institution = {RAND}, number = {TR-9}, type = {Working Paper}, year = {2010}}
@online{web1, title = {Page}, url = {https://example.com/a_b}, urldate = {2024-01-05}}
@misc{misc1, title = {Misc}, howpublished = {Online}}
@book{book1, title = {Book}, edition = {2}, isbn = {978-7-111-11111-1}, date = {1972-05}, publisher = {商务印书馆}, location = {北京}}
@unpublished{u1, title = {Draft}, note = {Manuscript}, year = {2022}}
@software{sw1, title = {Tool}, version = {1.2}}
"#;

    #[test]
    fn multiple_entries_types_and_fields() {
        let p = parse(MULTI).unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        let types: Vec<&str> = p.records.iter().map(|r| r.item_type.as_str()).collect();
        assert_eq!(
            types,
            [
                "journalArticle", "conferencePaper", "bookSection", "thesis", "thesis", "report", "webpage", "document", "book", "manuscript",
                "computerProgram"
            ]
        );

        let a = &p.records[0];
        assert_eq!(a.source_key.as_deref(), Some("smith2019"));
        assert_eq!(field(a, "citationKey"), Some("smith2019"));
        assert_eq!(field(a, "title"), Some("The NASA Mission: A Study of Müller's Data"));
        assert_eq!(field(a, "publicationTitle"), Some("Journal of Geophysical Research Atmospheres"));
        assert_eq!(field(a, "volume"), Some("12"));
        assert_eq!(field(a, "issue"), Some("3"));
        assert_eq!(field(a, "pages"), Some("100-120"));
        assert_eq!(field(a, "DOI"), Some("10.1000/xyz123"));
        assert_eq!(field(a, "extra"), Some("Accepted"));
        assert_eq!(field(a, "abstractNote"), Some("We study things."));
        assert_eq!(field(a, "bibtex:file"), Some(":smith2019.pdf:PDF"));
        assert_eq!(field(a, "bibtex:howpublished"), Some("https://example.com/~smith"));
        assert!(!a.fields.contains_key("bibtex:year") && !a.fields.contains_key("date"));
        assert_eq!(a.tags, ["climate", "models", "ocean"]);
        assert_eq!(a.notes, ["读后感：方法可借鉴"]);
        assert_eq!(a.date, Some(RecordDate { raw: "March 2019".into(), year: Some(2019), month: Some(3), day: None }));
        assert!(a.raw.starts_with("@Article{smith2019,") && a.raw.ends_with('}'), "{}", a.raw);
        assert!(a.raw.contains("howpublished = {\\url{https://example.com/~smith}}\n}"));

        let c = &p.records[1];
        assert_eq!(field(c, "proceedingsTitle"), Some("Proceedings of the Test Conference"));
        assert_eq!(field(c, "publisher"), Some("ACM"));
        assert_eq!(field(c, "place"), Some("New York"));
        assert_eq!(field(c, "bibtex:organization"), Some("Test Org"));
        // date 优先，year 保留进 bibtex:year
        assert_eq!(c.date, Some(RecordDate { raw: "2020-07-14".into(), year: Some(2020), month: Some(7), day: Some(14) }));
        assert_eq!(field(c, "bibtex:year"), Some("2021"));
        assert_eq!(names(c), [("author".into(), s("Lee"), s("Ann"), None), ("editor".into(), s("Brown"), s("Bob"), None)]);
        assert_eq!(c.warnings, ["编者列表里的 others（等人）已忽略"]);

        let ch = &p.records[2];
        assert_eq!(field(ch, "bookTitle"), Some("A Book"));
        assert_eq!(field(ch, "seriesNumber"), Some("7"));
        assert_eq!(field(ch, "series"), Some("LNCS"));
        assert_eq!(names(ch), [("translator".into(), s("Doe"), s("Jane"), None)]);

        let phd = &p.records[3];
        assert_eq!(field(phd, "title"), Some("论文题目"));
        assert_eq!(field(phd, "university"), Some("北京大学"));
        assert_eq!(field(phd, "thesisType"), Some("PhD thesis"));
        assert_eq!(field(&p.records[4], "thesisType"), Some("M.Eng. thesis"));

        let tr = &p.records[5];
        assert_eq!((field(tr, "institution"), field(tr, "reportNumber"), field(tr, "reportType")), (Some("RAND"), Some("TR-9"), Some("Working Paper")));
        let web = &p.records[6];
        assert_eq!((field(web, "url"), field(web, "accessDate")), (Some("https://example.com/a_b"), Some("2024-01-05")));
        assert!(web.date.is_none());
        assert_eq!(field(&p.records[7], "bibtex:howpublished"), Some("Online"));

        let book = &p.records[8];
        assert_eq!((field(book, "edition"), field(book, "ISBN")), (Some("2"), Some("978-7-111-11111-1")));
        assert_eq!((field(book, "publisher"), field(book, "place")), (Some("商务印书馆"), Some("北京")));
        assert_eq!(book.date, Some(RecordDate { raw: "1972-05".into(), year: Some(1972), month: Some(5), day: None }));
        assert_eq!(field(&p.records[9], "extra"), Some("Manuscript"));
        assert_eq!(field(&p.records[10], "versionNumber"), Some("1.2"));
    }

    #[test]
    fn creators_in_all_shapes() {
        let r = one(
            r#"@book{k,
              author = {Smith, John and {World Health Organization} and 王小明 and van Gogh, Vincent and King, Jr., Martin Luther
                        and Ludwig van Beethoven and {\'E}mile Zola and others},
              editor = {Wang, Xiaoming},
              translator = {李四},
              bookauthor = {Homer}
            }"#,
        );
        assert_eq!(
            names(&r),
            [
                ("author".into(), s("Smith"), s("John"), None),
                ("author".into(), None, None, s("World Health Organization")),
                ("author".into(), None, None, s("王小明")),
                ("author".into(), s("van Gogh"), s("Vincent"), None),
                ("author".into(), s("King"), s("Martin Luther, Jr."), None),
                ("author".into(), s("van Beethoven"), s("Ludwig"), None),
                ("author".into(), s("Zola"), s("Émile"), None),
                ("editor".into(), s("Wang"), s("Xiaoming"), None),
                ("translator".into(), None, None, s("李四")),
                ("bookAuthor".into(), None, None, s("Homer")),
            ]
        );
        assert_eq!(r.warnings, ["作者列表里的 others（等人）已忽略"]);
    }

    #[test]
    fn dates_structured_and_raw_only() {
        let d = |src: &str| one(src).date;
        assert_eq!(d("@misc{k, year = {2019}}"), Some(RecordDate { raw: "2019".into(), year: Some(2019), month: None, day: None }));
        assert_eq!(d("@misc{k, year = {2019}, month = {3}}"), Some(RecordDate { raw: "2019-03".into(), year: Some(2019), month: Some(3), day: None }));
        assert_eq!(
            d("@misc{k, year = {2019}, month = {Sept.}, day = {9}}"),
            Some(RecordDate { raw: "Sept. 9, 2019".into(), year: Some(2019), month: Some(9), day: Some(9) })
        );
        assert_eq!(d("@misc{k, year = {in press}}"), Some(RecordDate { raw: "in press".into(), year: None, month: None, day: None }));
        assert_eq!(d("@misc{k, date = {2019/2020}}"), Some(RecordDate { raw: "2019/2020".into(), year: Some(2019), month: None, day: None }));
        assert_eq!(d("@misc{k, date = {Spring 1999}}"), Some(RecordDate { raw: "Spring 1999".into(), year: None, month: None, day: None }));
        assert_eq!(d("@misc{k, title = {T}}"), None);
    }

    #[test]
    fn latex_commands_stripped() {
        let r = one(
            r"@article{k, title = {{\em Emphasis} and \textit{italic \textbf{bold}} \mkbibquote{q}~x \href{http://a.b}{link} \LaTeX\ and \alpha},
               journal = {\textsc{Small Caps} Review}, pages = {5---9}}",
        );
        assert_eq!(field(&r, "title"), Some(r"Emphasis and italic bold q x link LaTeX and \alpha"));
        assert_eq!(field(&r, "publicationTitle"), Some("Small Caps Review"));
        assert_eq!(field(&r, "pages"), Some("5-9"));
    }

    #[test]
    fn broken_entry_skipped_rest_ok() {
        let src = "@string{pub = {Springer}}\n\
                   @article{good1, title = {One}, year = {2001}}\n\
                   @article{bad1, title = {Unclosed, year = {2002}\n\
                   @book(good2, title = {Two}, publisher = pub)\n\
                   @misc{bad2 title = {no comma after key}}\n\
                   @misc{good3, title = {Three}, journal = undefinedstr}\n";
        let p = parse(src).unwrap();
        let keys: Vec<_> = p.records.iter().map(|r| r.source_key.clone().unwrap()).collect();
        assert_eq!(keys, ["good1", "good2", "good3"]);
        assert_eq!(field(&p.records[1], "publisher"), Some("Springer"));
        assert_eq!(p.records[1].raw, "@book(good2, title = {Two}, publisher = pub)");
        assert_eq!(field(&p.records[2], "bibtex:journal").or(field(&p.records[2], "publicationTitle")), Some("undefinedstr"));
        assert_eq!(p.records[2].warnings, ["字符串缩写 undefinedstr 没有定义，按原文保留"]);
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
        assert!(p.warnings[0].starts_with("第 2 条（键 bad1，第 3 行）无法解析，已跳过"), "{}", p.warnings[0]);
        assert!(p.warnings[1].starts_with("第 4 条（键 bad2"), "{}", p.warnings[1]);
    }

    #[test]
    fn strings_case_insensitive_and_cycles_do_not_hang() {
        let p = parse("@string{IEEE = {IEEE Press}}\n@string{a = b}\n@string{b = a # {x}}\n@book{k, publisher = ieee, series = a}").unwrap();
        let r = &p.records[0];
        assert_eq!(field(r, "publisher"), Some("IEEE Press"));
        assert_eq!(field(r, "series"), Some("a"));
        assert_eq!(p.warnings, ["字符串缩写 a、b 互相引用、无法展开，已忽略这些定义"]);
        // 同一个键出现两次也各自导入
        assert_eq!(parse("@misc{k, title={A}}\n@misc{k, title={B}}").unwrap().records.len(), 2);
    }

    #[test]
    fn crossref_inherits_one_level() {
        let p = parse(
            "@inproceedings{child, title = {Paper}, crossref = {DBLP:conf/x}, pages = {1--2}}\n\
             @proceedings{DBLP:conf/x, title = {Proc. of X}, editor = {Ed, Ita}, publisher = {Springer}, year = {2020}, crossref = {child}}",
        )
        .unwrap();
        let c = &p.records[0];
        assert_eq!(field(c, "proceedingsTitle"), Some("Proc. of X"));
        assert_eq!(field(c, "publisher"), Some("Springer"));
        assert_eq!(field(c, "bibtex:crossref"), Some("DBLP:conf/x"));
        assert_eq!(c.date.as_ref().and_then(|d| d.year), Some(2020));
        assert_eq!(names(c), [("editor".into(), s("Ed"), s("Ita"), None)]);
        assert_eq!(p.records[1].item_type, "book");
        // 自引用不会让 biblatex 无限递归
        let r = one("@misc{me, title = {Self}, crossref = {me}, xdata = {me}}");
        assert_eq!((field(&r, "bibtex:crossref"), field(&r, "bibtex:xdata")), (Some("me"), Some("me")));
    }

    #[test]
    fn invalid_files() {
        assert!(parse("这不是 BibTeX，只是一段话。").unwrap_err().contains("BibTeX"));
        // 只有一条坏条目：不报错，给 warning
        let p = parse("@article{only, title = {never closed").unwrap();
        assert!(p.records.is_empty());
        assert!(p.warnings[0].contains("括号没有闭合"), "{:?}", p.warnings);
        // 只有 @comment / @string：空结果
        assert!(parse("@comment{x}\n@string{a = {b}}").unwrap().records.is_empty());
    }

    #[test]
    fn splitter_handles_quotes_parens_and_comments() {
        let src = "% @article{fake, title={x}}\n@article(p1, title = \"A } quoted\", note = {(x)})\n  @misc{p2, title={B}}";
        let segs = split_segments(src);
        let kinds: Vec<_> = segs.iter().map(|s| (s.kind.as_str(), s.key.as_str(), s.closed, s.line)).collect();
        assert_eq!(kinds, [("article", "p1", true, 2), ("misc", "p2", true, 3)]);
        assert_eq!(segs[0].text, "@article{p1, title = \"A } quoted\", note = {(x)}}");
        assert!(has_entry_head("x\n@Book {k,}") && !has_entry_head("mail me@example.com {x}"));
    }
}
