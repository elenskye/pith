//! 文本小工具（确定性，无依赖）：去重用的标题规范化、按字符判定语言、日期解析、时间格式、DOI / ISBN、HTML 转纯文本。

/// 去重用的标题：全角转半角、小写、去掉标点与空白以外的符号、空白合并。中日韩文字原样保留。
pub fn title_norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        let c = halfwidth(c);
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out
}

fn halfwidth(c: char) -> char {
    match c as u32 {
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        0x3000 => ' ',
        _ => c,
    }
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0xF900..=0xFAFF)
}
fn is_kana(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF)
}
fn is_hangul(c: char) -> bool {
    matches!(c as u32, 0xAC00..=0xD7AF | 0x1100..=0x11FF)
}

/// 按字符判定语言：有假名 → ja，有谚文 → ko，有汉字 → zh，有拉丁字母 → en（只区分到文字体系，
/// 引用格式要的是「中文 / 非中文」）。没有文字返回 None。
pub fn detect_lang(text: &str) -> Option<&'static str> {
    let (mut han, mut kana, mut hangul, mut latin) = (false, false, false, false);
    for c in text.chars() {
        if is_kana(c) {
            kana = true;
        } else if is_hangul(c) {
            hangul = true;
        } else if is_han(c) {
            han = true;
        } else if c.is_ascii_alphabetic() || matches!(c as u32, 0xC0..=0x24F) {
            latin = true;
        }
    }
    if kana {
        Some("ja")
    } else if hangul {
        Some("ko")
    } else if han {
        Some("zh")
    } else if latin {
        Some("en")
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateParts {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

impl DateParts {
    /// 'YYYY-MM-DD'，未知月日为 00（Zotero 同款）；没有年份返回 None。
    pub fn sort_key(&self) -> Option<String> {
        let y = self.year?;
        Some(format!(
            "{y:04}-{:02}-{:02}",
            self.month.unwrap_or(0),
            if self.month.is_some() { self.day.unwrap_or(0) } else { 0 }
        ))
    }
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

#[derive(Debug, PartialEq)]
enum Tok {
    Num(String),
    Word(String),
    Mark(char),
}

/// 从原文里解析年月日：「2019-03-15」「2019/3」「3/2009」「15.03.2019」「March 2019」「15 Mar 2019」「2025年6月」「2019///」。
/// 认不出的部分留空。只取第一个日期（范围「2019–2020」取 2019）。
pub fn parse_date(raw: &str) -> DateParts {
    let mut toks: Vec<Tok> = Vec::new();
    let mut seps: Vec<char> = Vec::new(); // 每个数字 token 之前紧挨的分隔符
    let mut prev_sep = ' ';
    let mut it = raw.chars().map(halfwidth).peekable();
    while let Some(c) = it.next() {
        if c.is_ascii_digit() {
            let mut s = c.to_string();
            while let Some(d) = it.peek().filter(|d| d.is_ascii_digit()) {
                s.push(*d);
                it.next();
            }
            toks.push(Tok::Num(s));
            seps.push(prev_sep);
            prev_sep = ' ';
        } else if c.is_alphabetic() && !is_han(c) {
            let mut s = c.to_lowercase().to_string();
            while let Some(d) = it.peek().filter(|d| d.is_alphabetic() && !is_han(**d)) {
                s.extend(d.to_lowercase());
                it.next();
            }
            toks.push(Tok::Word(s));
            seps.push(' ');
        } else if matches!(c, '年' | '月' | '日') {
            toks.push(Tok::Mark(c));
            seps.push(' ');
        } else if !c.is_whitespace() {
            prev_sep = c;
        }
    }
    let num = |i: usize| match toks.get(i) {
        Some(Tok::Num(s)) => s.parse::<u32>().ok(),
        _ => None,
    };
    let year_idx = toks.iter().position(|t| matches!(t, Tok::Num(s) if s.len() == 4));
    let Some(yi) = year_idx else { return DateParts::default() };
    let year = num(yi).map(|y| y as i32);
    let valid_m = |m: Option<u32>| m.filter(|m| (1..=12).contains(m));
    let valid_d = |d: Option<u32>| d.filter(|d| (1..=31).contains(d));

    // 2025年6月15日
    if matches!(toks.get(yi + 1), Some(Tok::Mark('年'))) {
        let month = if matches!(toks.get(yi + 3), Some(Tok::Mark('月'))) {
            valid_m(num(yi + 2))
        } else {
            None
        };
        let day = if month.is_some() && matches!(toks.get(yi + 5), Some(Tok::Mark('日'))) {
            valid_d(num(yi + 4))
        } else {
            None
        };
        return DateParts { year, month, day };
    }
    // 月份名：March 2019、15 Mar 2019、Mar. 15, 2019
    if let Some(mi) = toks
        .iter()
        .position(|t| matches!(t, Tok::Word(w) if w.len() >= 3 && MONTHS.iter().any(|m| w.starts_with(m))))
    {
        let Tok::Word(w) = &toks[mi] else { unreachable!() };
        let month = MONTHS.iter().position(|m| w.starts_with(m)).map(|i| i as u32 + 1);
        let day = [mi.wrapping_sub(1), mi + 1]
            .into_iter()
            .filter(|&i| i != yi)
            .find_map(|i| valid_d(num(i)).filter(|_| matches!(&toks[i], Tok::Num(s) if s.len() <= 2)));
        return DateParts { year, month, day };
    }
    // 纯数字
    let small = |i: usize| match toks.get(i) {
        Some(Tok::Num(s)) if s.len() <= 2 => s.parse::<u32>().ok(),
        _ => None,
    };
    if yi == 0 {
        // 年在前：2019-03-15、2019/3、2019///
        let month = valid_m(small(1));
        let day = if month.is_some() { valid_d(small(2)) } else { None };
        return DateParts { year, month, day };
    }
    // 年在后：3/2009、3/15/2009（美式，Zotero 默认）、15.03.2019（点分为日.月）
    let (a, b) = (small(yi.wrapping_sub(2)).filter(|_| yi >= 2), small(yi - 1));
    let (month, day) = match (a, b) {
        (Some(a), Some(b)) if seps.get(yi - 1) == Some(&'.') => (valid_m(Some(b)), valid_d(Some(a))),
        (Some(a), Some(b)) if a > 12 && b <= 12 => (valid_m(Some(b)), valid_d(Some(a))),
        (Some(a), Some(b)) => (valid_m(Some(a)), valid_d(Some(b))),
        (None, Some(b)) => (valid_m(Some(b)), None),
        _ => (None, None),
    };
    DateParts {
        year,
        month: if day.is_some() || month.is_some() { month } else { None },
        day: if month.is_some() { day } else { None },
    }
}

/// Zotero 的日期字段是「排序值 原文」两段：`2009-03-00 3/2009`。返回 (排序值, 原文)。
/// 不是这个格式（来自其他来源）时排序值为空，原文是整个值。
pub fn split_multipart(value: &str) -> (Option<String>, String) {
    let b = value.as_bytes();
    let shape = b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && (b.len() == 10 || b[10] == b' ');
    if shape {
        (Some(value[..10].to_string()), value.get(11..).unwrap_or("").to_string())
    } else {
        (None, value.to_string())
    }
}

/// 拼成 Zotero 的两段式日期值。
pub fn multipart(parts: DateParts, raw: &str) -> String {
    format!("{} {raw}", parts.sort_key().unwrap_or_else(|| "0000-00-00".into()))
}

/// 排序值 → 年份（0000 视为未知）
pub fn year_of(sort: &str) -> Option<i64> {
    sort.get(..4).and_then(|y| y.parse::<i64>().ok()).filter(|y| *y > 0)
}

/// Zotero 时间戳「YYYY-MM-DD HH:MM:SS」（UTC）→ 库里的「YYYY-MM-DDTHH:MM:SS.000Z」。格式不对就原样返回。
pub fn zotero_time_to_iso(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() == 19 && b[10] == b' ' && b[4] == b'-' && b[13] == b':' {
        format!("{}T{}.000Z", &s[..10], &s[11..])
    } else {
        s.to_string()
    }
}

/// `zotero_time_to_iso` 的逆运算（导入核对用）。
pub fn iso_to_zotero_time(s: &str) -> String {
    match s.strip_suffix(".000Z") {
        Some(t) if t.len() == 19 && t.as_bytes()[10] == b'T' => format!("{} {}", &t[..10], &t[11..]),
        _ => s.to_string(),
    }
}

/// 现在的 UTC 时间，库里的格式「YYYY-MM-DDTHH:MM:SS.mmmZ」（毫秒，与 schema 约定的 strftime('%Y-%m-%dT%H:%M:%fZ') 一致）。
pub fn now_iso() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let (secs, ms) = (d.as_secs() as i64, d.subsec_millis());
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant 的 civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// DOI：去掉 `https://doi.org/`、`doi:` 前缀与空白；不像 DOI（不以 10. 开头）返回 None。
pub fn normalize_doi(s: &str) -> Option<String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let mut rest = t;
    for p in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi.org/",
        "doi:",
    ] {
        if lower.starts_with(p) {
            rest = t[p.len()..].trim();
            break;
        }
    }
    rest.starts_with("10.").then(|| rest.to_string())
}

/// ISBN：取第一个，去掉连字符，X 大写。先按空白 / 逗号 / 分号切开找 10 位或 13 位的一段；
/// 都不是时把全部数字连起来再看（「978 0 306 40615 7」）。长度不对返回 None。
pub fn normalize_isbn(s: &str) -> Option<String> {
    let clean = |t: &str| -> String {
        t.chars()
            .filter(|c| c.is_ascii_digit() || *c == 'x' || *c == 'X')
            .map(|c| c.to_ascii_uppercase())
            .collect()
    };
    let ok = |t: &String| (t.len() == 13 && !t.contains('X')) || (t.len() == 10 && !t[..9].contains('X'));
    s.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(clean)
        .find(ok)
        .or_else(|| Some(clean(s)).filter(ok))
}

/// 规范化后的 ISBN（10 或 13 位）统一成 13 位，去重比较用。
pub fn isbn13(isbn: &str) -> Option<String> {
    match isbn.len() {
        13 => Some(isbn.to_string()),
        10 => {
            let core = format!("978{}", &isbn[..9]);
            let sum: u32 = core.bytes().enumerate().map(|(i, b)| (b - b'0') as u32 * if i % 2 == 0 { 1 } else { 3 }).sum();
            Some(format!("{core}{}", (10 - sum % 10) % 10))
        }
        _ => None,
    }
}

/// HTML → 纯文本（笔记预览、以后的搜索索引）：去标签，块级元素换行，解码常见实体。不执行、不加载任何东西。
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.char_indices().peekable();
    let mut skip_until: Option<&str> = None;
    while let Some((i, c)) = chars.next() {
        if c == '<' {
            let end = html[i..].find('>').map(|e| i + e).unwrap_or(html.len() - 1);
            let tag = html[i + 1..end].trim().to_ascii_lowercase();
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if let Some(stop) = skip_until {
                if tag.starts_with('/') && name == stop {
                    skip_until = None;
                }
            } else if !tag.starts_with('/') && (name == "script" || name == "style") {
                skip_until = Some(if name == "script" { "script" } else { "style" });
            } else if matches!(
                name.as_str(),
                "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "table" | "ul" | "ol"
            ) && !out.ends_with('\n')
                && !out.is_empty()
            {
                out.push('\n');
            }
            while let Some((j, _)) = chars.peek() {
                if *j > end {
                    break;
                }
                chars.next();
            }
            continue;
        }
        if skip_until.is_some() {
            continue;
        }
        if c == '&'
            && let Some(semi) = html[i..].find(';').filter(|s| *s <= 10)
        {
            let ent = &html[i + 1..i + semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                _ => ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            if let Some(d) = decoded {
                out.push(d);
                for _ in 0..semi {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    // 每行去首尾空白，去掉空行（段落之间一个换行）
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// 纯文本 → 最简单的 HTML 段落（导入的子笔记）。
pub fn text_to_html(text: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", esc(p).replace('\n', "<br>")))
        .collect()
}

/// 中日文字与全角标点（摘录拼行时两边都是它就不加空格）
fn is_cjk_text(c: char) -> bool {
    is_han(c) || is_kana(c) || matches!(c as u32, 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

/// PDF 里选中的原文 → 摘录文字：合并断行（中文之间不加空格，西文行尾连字符断开的词接回去），
/// 连续空白并成一个，去掉软连字符、零宽字符，拆开 ﬁ ﬂ 等连字。
pub fn excerpt(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\u{AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}' => {}
            '\u{FB00}' => cleaned.push_str("ff"),
            '\u{FB01}' => cleaned.push_str("fi"),
            '\u{FB02}' => cleaned.push_str("fl"),
            '\u{FB03}' => cleaned.push_str("ffi"),
            '\u{FB04}' => cleaned.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => cleaned.push_str("st"),
            '\r' => cleaned.push('\n'),
            c => cleaned.push(c),
        }
    }
    let mut out = String::new();
    for line in cleaned.split('\n') {
        // 行内：空白并成一个；两个中文字之间的空格多半是 PDF 抽取出来的，去掉
        let chars: Vec<char> = line.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
        let mut l = String::with_capacity(chars.len());
        for (i, &c) in chars.iter().enumerate() {
            if c == ' ' && i > 0 && i + 1 < chars.len() && is_cjk_text(chars[i - 1]) && is_cjk_text(chars[i + 1]) {
                continue;
            }
            l.push(c);
        }
        if l.is_empty() {
            continue;
        }
        let (Some(p), Some(n)) = (out.chars().last(), l.chars().next()) else {
            out.push_str(&l);
            continue;
        };
        let before_hyphen = out.chars().rev().nth(1);
        if is_cjk_text(p) && is_cjk_text(n) {
        } else if p == '-' && before_hyphen.is_some_and(char::is_alphabetic) && n.is_lowercase() {
            out.pop();
        } else {
            out.push(' ');
        }
        out.push_str(&l);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: Option<u32>, day: Option<u32>) -> DateParts {
        DateParts { year: Some(y), month: m, day }
    }

    #[test]
    fn titles_normalize_for_dedup() {
        assert_eq!(title_norm("The  Efficacy: of Voting!"), "the efficacy of voting");
        assert_eq!(
            title_norm("ＡＢＣ　互联网使用与政治效能感：基于全国调查数据"),
            "abc 互联网使用与政治效能感 基于全国调查数据"
        );
        assert_eq!(title_norm("Müller-Lüdenscheidt"), "müller lüdenscheidt");
        assert_eq!(title_norm("  "), "");
    }

    #[test]
    fn language_by_script() {
        assert_eq!(detect_lang("国民财富的性质和原因的研究"), Some("zh"));
        assert_eq!(detect_lang("Wealth of Nations"), Some("en"));
        assert_eq!(detect_lang("政治の研究"), Some("ja"));
        assert_eq!(detect_lang("한국어 제목"), Some("ko"));
        assert_eq!(detect_lang("1776"), None);
    }

    #[test]
    fn dates_from_free_text() {
        assert_eq!(parse_date("2019-03-15"), d(2019, Some(3), Some(15)));
        assert_eq!(parse_date("2019/03/15/"), d(2019, Some(3), Some(15)));
        assert_eq!(parse_date("2019///"), d(2019, None, None));
        assert_eq!(parse_date("3/2009"), d(2009, Some(3), None));
        assert_eq!(parse_date("10/2016"), d(2016, Some(10), None));
        assert_eq!(parse_date("3/15/2009"), d(2009, Some(3), Some(15)));
        assert_eq!(parse_date("15/3/2009"), d(2009, Some(3), Some(15)));
        assert_eq!(parse_date("15.03.2019"), d(2019, Some(3), Some(15)));
        assert_eq!(parse_date("March 2019"), d(2019, Some(3), None));
        assert_eq!(parse_date("Nov 2022"), d(2022, Some(11), None));
        assert_eq!(parse_date("15 Mar 2019"), d(2019, Some(3), Some(15)));
        assert_eq!(parse_date("Mar. 15, 2019"), d(2019, Some(3), Some(15)));
        assert_eq!(parse_date("2025年6月"), d(2025, Some(6), None));
        assert_eq!(parse_date("２０２５年６月１日"), d(2025, Some(6), Some(1)));
        assert_eq!(parse_date("2019–2020"), d(2019, None, None));
        assert_eq!(parse_date("Spring 1999"), d(1999, None, None));
        assert_eq!(parse_date("n.d."), DateParts::default());
        assert_eq!(parse_date("2019-13-40"), d(2019, None, None));
        assert_eq!(d(2019, Some(3), None).sort_key().as_deref(), Some("2019-03-00"));
        assert_eq!(DateParts::default().sort_key(), None);
    }

    #[test]
    fn zotero_multipart_dates() {
        assert_eq!(split_multipart("2009-03-00 3/2009"), (Some("2009-03-00".into()), "3/2009".into()));
        assert_eq!(split_multipart("2013-05-01 2013-05-01"), (Some("2013-05-01".into()), "2013-05-01".into()));
        assert_eq!(split_multipart("2013-05-01"), (Some("2013-05-01".into()), "".into()));
        assert_eq!(split_multipart("March 2019"), (None, "March 2019".into()));
        assert_eq!(multipart(d(2019, Some(3), None), "March 2019"), "2019-03-00 March 2019");
        assert_eq!(multipart(DateParts::default(), "n.d."), "0000-00-00 n.d.");
        assert_eq!(year_of("2019-03-00"), Some(2019));
        assert_eq!(year_of("0000-00-00"), None);
    }

    #[test]
    fn timestamps_round_trip() {
        let z = "2026-01-01 10:00:00";
        assert_eq!(zotero_time_to_iso(z), "2026-01-01T10:00:00.000Z");
        assert_eq!(iso_to_zotero_time(&zotero_time_to_iso(z)), z);
        assert_eq!(zotero_time_to_iso("odd"), "odd");
        assert_eq!(iso_to_zotero_time("odd"), "odd");
        let now = now_iso();
        assert_eq!(now.len(), 24);
        assert!(now.ends_with('Z') && &now[10..11] == "T" && &now[19..20] == ".");
        assert!(now.as_str() > "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn doi_and_isbn() {
        assert_eq!(normalize_doi(" https://doi.org/10.1000/ABC "), Some("10.1000/ABC".into()));
        assert_eq!(normalize_doi("doi:10.1/x"), Some("10.1/x".into()));
        assert_eq!(normalize_doi("not a doi"), None);
        assert_eq!(normalize_isbn("978-0-00-000000-1"), Some("9780000000001".into()));
        assert_eq!(normalize_isbn("0-306-40615-X"), Some("030640615X".into()));
        assert_eq!(normalize_isbn("9780306406157 0306406152"), Some("9780306406157".into()));
        assert_eq!(normalize_isbn("ISBN 978 0 306 40615 7"), Some("9780306406157".into()));
        assert_eq!(normalize_isbn("123"), None);
        assert_eq!(isbn13("030640615X").as_deref(), Some("9780306406157"));
        assert_eq!(isbn13("9780306406157").as_deref(), Some("9780306406157"));
        assert_eq!(isbn13("12"), None);
    }

    #[test]
    fn html_to_plain_text() {
        let html = "<div data-schema-version=\"9\"><h1>读书笔记</h1><p>内部效能感 &amp; 外部效能感，公式 $E = mc^2$。</p><p>第二段&nbsp;&#x4E2D;&#25991;</p><script>alert(1)</script><img src=\"x\"></div>";
        assert_eq!(html_to_text(html), "读书笔记\n内部效能感 & 外部效能感，公式 $E = mc^2$。\n第二段 中文");
        assert_eq!(html_to_text("<p>a</p>\n\n<p></p><p>b</p>"), "a\nb");
        assert_eq!(html_to_text("plain &unknown; text"), "plain &unknown; text");
        assert_eq!(text_to_html("第一段 <b>\n同段\n\n第二段"), "<p>第一段 &lt;b&gt;<br>同段</p><p>第二段</p>");
    }

    #[test]
    fn excerpts_join_lines() {
        assert_eq!(excerpt("The effi-\ncacy of  voting\nmatters."), "The efficacy of voting matters.");
        assert_eq!(excerpt("well-\nKnown"), "well- Known", "下一行大写开头不接");
        assert_eq!(excerpt("政治效能\n感的测量 方法\n（2021）"), "政治效能感的测量方法（2021）");
        assert_eq!(excerpt("中文\nEnglish\n中文"), "中文 English 中文");
        assert_eq!(excerpt("  \u{FB01}nal\u{AD}ly\r\n\n  ok\u{200B} "), "finally ok");
        assert_eq!(excerpt("a\tb\u{A0}c"), "a b c");
        assert_eq!(excerpt(" \n "), "");
    }
}
