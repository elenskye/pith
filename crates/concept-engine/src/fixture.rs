//! 夹具：小 EPUB 由代码现拼（不在仓库里放二进制）。
//!
//! - 英文：《国富论》第一篇第 1–3 章的公有领域文字（Project Gutenberg #3300，见 `fixtures/wealth/README.md`），
//!   按 Glasgow 版分段（11 / 5 / 8 段），加上合成的目录、编者导言、注释，和每约 1500 字一个的假分页点。
//!   两种拼法：EPUB 3（nav 目录 + page-list + `epub:type="pagebreak"`）、EPUB 2（ncx + `page-map.xml`，
//!   页码表里写拆分之前的文件名，正文拆成 `book1_split_00N.html`，和 Penguin 版一样）。
//! - 中文：合成的片段（不是任何真实译本），故意带导读、旁注（`blockquote`）、注释（上标引用 + 章末注释、译者按接在注释后面），
//!   第三章只有 7 段（和英文的 8 段对不上）。

use std::io::Write;

use crate::text::{self, Text};

pub const EN_SOURCE: &str = include_str!("../fixtures/wealth/en-book1-ch1-3.txt");
pub const ZH_SOURCE: &str = include_str!("../fixtures/wealth/zh-synthetic.txt");

/// 每多少字放一个假分页点
pub const PAGE_EVERY: usize = 1500;
/// 第一篇的标题在第几页
pub const FIRST_PAGE: u32 = 101;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Epub3,
    Epub2Split,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

struct Zip {
    w: zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
}

impl Zip {
    fn new() -> Zip {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("mimetype", stored).expect("zip");
        w.write_all(b"application/epub+zip").expect("zip");
        Zip { w }
    }

    fn add(&mut self, name: &str, body: &str) {
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        self.w.start_file(name, opts).expect("zip");
        self.w.write_all(body.as_bytes()).expect("zip");
    }

    fn finish(self) -> Vec<u8> {
        self.w.finish().expect("zip").into_inner()
    }
}

const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

fn xhtml(title: &str, lang: &str, body: &str) -> String {
    format!("<?xml version='1.0' encoding='utf-8'?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\"><head><title>{}</title><link href=\"style.css\" rel=\"stylesheet\" type=\"text/css\"/></head>\n<body class=\"calibre\">\n{body}\n</body></html>", esc(title))
}

/// 英文夹具的一章
pub struct EnChapter {
    pub number: u32,
    pub title: String,
    pub paragraphs: Vec<String>,
}

pub fn en_chapters() -> (String, Vec<EnChapter>) {
    let mut book_title = String::new();
    let mut chapters: Vec<EnChapter> = Vec::new();
    let mut section: Option<&str> = None;
    let mut pending_title = false;
    for block in EN_SOURCE.split("\n\n") {
        let mut lines = block.lines().filter(|l| !l.trim().is_empty()).peekable();
        while let Some(line) = lines.next() {
            if let Some(h) = line.strip_prefix("# ") {
                section = Some(if h.starts_with("BOOK") { "book" } else { "chapter" });
                if h.starts_with("CHAPTER") {
                    let n = match h.trim_start_matches("CHAPTER ").trim() {
                        "I" => 1,
                        "II" => 2,
                        _ => 3,
                    };
                    chapters.push(EnChapter { number: n, title: String::new(), paragraphs: Vec::new() });
                }
                pending_title = true;
                continue;
            }
            match (section, pending_title) {
                (Some("book"), true) => {
                    book_title = line.trim().to_string();
                    pending_title = false;
                }
                (Some("chapter"), true) => {
                    if let Some(c) = chapters.last_mut() {
                        c.title = line.trim().to_string();
                    }
                    pending_title = false;
                }
                (Some("chapter"), false) => {
                    if let Some(c) = chapters.last_mut() {
                        c.paragraphs.push(line.trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    (book_title, chapters)
}

/// 段落里插假分页点：返回 XHTML 片段。`budget` 是到下一个分页点还剩几个字，`page` 是现在的页
fn with_breaks(text: &str, budget: &mut usize, page: &mut u32, flavor: Flavor) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if *budget == 0 && chars[i] == ' ' {
            *page += 1;
            *budget = PAGE_EVERY;
            out.push(' ');
            out.push_str(&anchor(*page, flavor));
            i += 1;
            continue;
        }
        let c = chars[i];
        out.push_str(&esc(&c.to_string()));
        *budget = budget.saturating_sub(1);
        i += 1;
    }
    out
}

fn anchor(page: u32, flavor: Flavor) -> String {
    match flavor {
        Flavor::Epub3 => format!("<span epub:type=\"pagebreak\" id=\"page_{page}\" title=\"{page}\"/>"),
        Flavor::Epub2Split => format!("<a id=\"page_{page}\"></a>"),
    }
}

/// 英文夹具：书名页、目录、编者导言（合成）、第一篇第 1–3 章、注释（合成）
pub fn english_epub(flavor: Flavor) -> Vec<u8> {
    let (book_title, chapters) = en_chapters();
    let mut z = Zip::new();
    z.add("META-INF/container.xml", CONTAINER);
    z.add("OEBPS/style.css", "p { text-indent: 1em; }");
    let mut files: Vec<(String, String, String)> = Vec::new(); // (文件名, 标题, 内容)
    let mut page = FIRST_PAGE;
    let mut budget = PAGE_EVERY;
    let mut pages: Vec<(u32, String, String)> = Vec::new(); // (页, 文件, id)
    let name = |base: &str, i: usize| match flavor {
        Flavor::Epub3 => format!("{base}{i}.xhtml"),
        Flavor::Epub2Split => format!("{base}_split_{i:03}.html"),
    };
    files.push((name("front", 0), "Contents".into(), "<h1>Contents</h1>\n<p class=\"toc\">Editor’s Introduction</p>\n<p class=\"toc\">Book I</p>\n<p class=\"toc\">Notes</p>".into()));
    files.push((
        name("front", 1),
        "Editor’s Introduction".into(),
        "<h1>Editor’s Introduction</h1>\n<p class=\"x1\">This synthetic introduction stands in for an editor’s essay. Smith regarded the division of labour as the great cause of improvement.</p>\n<p class=\"x1\">It is not part of Smith’s text and must never be quoted as the original.</p>".into(),
    ));
    let book_file = name("book1", 0);
    pages.push((page, book_file.clone(), format!("page_{page}")));
    files.push((book_file, "Book I".into(), format!("<h2>{}BOOK I</h2>\n<h4><em>{}</em></h4>", anchor(page, flavor), esc(book_title.trim_start_matches("BOOK I. ")))));
    for c in &chapters {
        let file = name("book1", c.number as usize);
        let mut body = format!("<h2><a id=\"ch{:02}\"></a>CHAPTER {}</h2>\n<h3><small>{}</small></h3>\n", c.number, text::roman(c.number), esc(&c.title));
        for (i, p) in c.paragraphs.iter().enumerate() {
            let before = page;
            let html = with_breaks(p, &mut budget, &mut page, flavor);
            for n in before + 1..=page {
                pages.push((n, file.clone(), format!("page_{n}")));
            }
            // 第一段段首是小型大写（和 Penguin 版一样）
            let html = if i == 0 {
                let mut words = html.splitn(2, ' ');
                let first = words.next().unwrap_or("");
                let rest = words.next().unwrap_or("");
                let mut cs = first.chars();
                let head: String = cs.next().map(|c| c.to_string()).unwrap_or_default();
                let tail: String = cs.collect();
                format!("{head}<small class=\"sc\">{}</small> {rest}", tail.to_uppercase())
            } else {
                html
            };
            body.push_str(&format!("<p class=\"{}\">{html}</p>\n", if i == 0 { "authorbio" } else { "subtitle" }));
        }
        files.push((file, format!("CHAPTER {}", text::roman(c.number)), body));
    }
    files.push((
        name("notes", 0),
        "Notes".into(),
        "<h1>Notes</h1>\n<p>1. A synthetic editor’s note on the pin factory, for tests only.</p>\n<p>2. Another synthetic note on the extent of the market.</p>".into(),
    ));

    let ids: Vec<String> = (0..files.len()).map(|i| format!("f{i}")).collect();
    for (f, title, body) in &files {
        z.add(&format!("OEBPS/{f}"), &xhtml(title, "en-GB", body));
    }
    let manifest: String = files.iter().zip(&ids).map(|((f, _, _), id)| format!("<item id=\"{id}\" href=\"{f}\" media-type=\"application/xhtml+xml\"/>")).collect();
    let spine: String = ids.iter().map(|id| format!("<itemref idref=\"{id}\"/>")).collect();
    let chapter_file = |n: usize| files[2 + n].0.clone();
    match flavor {
        Flavor::Epub3 => {
            let mut nav = String::from("<nav epub:type=\"toc\"><ol>");
            nav.push_str(&format!("<li><a href=\"{}\">Contents</a></li><li><a href=\"{}\">Editor’s Introduction</a></li>", files[0].0, files[1].0));
            nav.push_str(&format!("<li><a href=\"{}\">Book One</a><ol>", files[2].0));
            for c in &chapters {
                nav.push_str(&format!("<li><a href=\"{}#ch{:02}\">CHAPTER {}</a></li>", chapter_file(c.number as usize), c.number, text::roman(c.number)));
            }
            nav.push_str(&format!("</ol></li><li><a href=\"{}\">Notes</a></li></ol></nav>", files.last().map(|f| f.0.as_str()).unwrap_or("")));
            nav.push_str("<nav epub:type=\"page-list\" hidden=\"\"><ol>");
            for (n, f, id) in &pages {
                nav.push_str(&format!("<li><a href=\"{f}#{id}\">{n}</a></li>"));
            }
            nav.push_str("</ol></nav>");
            z.add("OEBPS/nav.xhtml", &xhtml("Contents", "en", &nav));
            z.add(
                "OEBPS/content.opf",
                &format!(
                    "<?xml version='1.0' encoding='utf-8'?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>The Wealth of Nations (test fixture)</dc:title><dc:creator>Adam Smith</dc:creator><dc:language>en</dc:language><dc:identifier id=\"id\">urn:pith:fixture:en</dc:identifier></metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>{manifest}</manifest><spine>{spine}</spine></package>"
                ),
            );
        }
        Flavor::Epub2Split => {
            let mut np = String::new();
            let mut order = 1;
            let mut point = |label: &str, src: &str, inner: &str| {
                let s = format!("<navPoint id=\"n{order}\" playOrder=\"{order}\"><navLabel><text>{}</text></navLabel><content src=\"{src}\"/>{inner}</navPoint>", esc(label));
                order += 1;
                s
            };
            np.push_str(&point("Contents", &files[0].0, ""));
            np.push_str(&point("Editor’s Introduction", &files[1].0, ""));
            let chapters_np: String = chapters.iter().map(|c| point(&format!("CHAPTER {}", text::roman(c.number)), &chapter_file(c.number as usize), "")).collect();
            np.push_str(&point("Book One", &files[2].0, &chapters_np));
            np.push_str(&point("Notes", &files.last().map(|f| f.0.clone()).unwrap_or_default(), ""));
            z.add("OEBPS/toc.ncx", &format!("<?xml version='1.0' encoding='utf-8'?><ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><head/><docTitle><text>The Wealth of Nations (test fixture)</text></docTitle><navMap>{np}</navMap></ncx>"));
            // 页码表写拆分之前的文件名（book1.html#page_N），和 calibre 拆过的 Penguin 版一样
            let pm: String = pages.iter().map(|(n, _, id)| format!("<page name=\"{n}\" href=\"book1.html#{id}\"/>")).collect();
            z.add("OEBPS/page-map.xml", &format!("<?xml version='1.0' encoding='utf-8'?><page-map xmlns=\"http://www.idpf.org/2007/opf\">{pm}</page-map>"));
            z.add(
                "OEBPS/content.opf",
                &format!(
                    "<?xml version='1.0' encoding='utf-8'?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:opf=\"http://www.idpf.org/2007/opf\"><dc:title>The Wealth of Nations (test fixture)</dc:title><dc:creator opf:role=\"aut\">Adam Smith</dc:creator><dc:language>en</dc:language><dc:identifier id=\"id\">urn:pith:fixture:en2</dc:identifier></metadata><manifest><item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/><item id=\"pm\" href=\"page-map.xml\" media-type=\"application/oebps-page-map+xml\"/>{manifest}</manifest><spine toc=\"ncx\">{spine}</spine></package>"
                ),
            );
        }
    }
    z.finish()
}

/// 中文夹具（EPUB 2，ncx）：合成的目录、译者导言、第一编、前三章导读、三章（带旁注、注释、译者按）
pub fn chinese_epub() -> Vec<u8> {
    let mut z = Zip::new();
    z.add("META-INF/container.xml", CONTAINER);
    let mut files: Vec<(String, String, String, u32)> = Vec::new(); // (文件, 目录文字, 内容, 层级)
    let mut current: Option<(String, u32, String)> = None;
    let mut note_no = 0;
    let mut file_no = 0;
    let flush = |cur: &mut Option<(String, u32, String)>, files: &mut Vec<(String, String, String, u32)>| {
        if let Some((label, depth, body)) = cur.take() {
            files.push((format!("index_split_{:03}.html", files.len()), label, body, depth));
        }
    };
    for line in ZH_SOURCE.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("# ") {
            flush(&mut current, &mut files);
            let (kind, label) = h.split_once(' ').unwrap_or(("OTHER", h));
            let depth = match kind {
                "MATTER" | "BOOK" => 1,
                "GUIDE" if label.contains("前三章") => 2,
                "GUIDE" => 1,
                _ => 2,
            };
            file_no += 1;
            // 章名：粗体、链回目录的 <p>（和中文 EPUB 一样不是 h 标签）
            let title = match kind {
                "CHAPTER" | "BOOK" => {
                    let (a, b) = label.split_once('\u{3000}').unwrap_or((label, ""));
                    format!("<p id=\"filepos{file_no}00\" class=\"calibre_\"><a href=\"index_split_000.html\"><span class=\"bold\">{}</span></a><br class=\"calibre3\"/><a href=\"index_split_000.html\"><span class=\"bold\">{}</span></a></p>\n", esc(a), esc(b))
                }
                _ => String::new(),
            };
            current = Some((label.to_string(), depth, title));
            note_no = 0;
            continue;
        }
        let Some((_, _, body)) = current.as_mut() else { continue };
        let refs = |s: &str| -> String {
            let mut out = esc(s);
            for n in 1..=9 {
                out = out.replace(&format!("[[{n}]]"), &format!("<sup class=\"calibre5\"><small id=\"r{file_no}_{n}\"><a href=\"#n{file_no}_{n}\"><span>［{n}］</span></a></small></sup>"));
            }
            out
        };
        if let Some(t) = line.strip_prefix("P: ") {
            body.push_str(&format!("<p class=\"calibre_10\">{}</p>\n", refs(t)));
        } else if let Some(t) = line.strip_prefix("S: ") {
            body.push_str(&format!("<blockquote class=\"calibre_65\">{}</blockquote>\n", esc(t)));
        } else if let Some(t) = line.strip_prefix("N: ") {
            if note_no == 0 {
                body.push_str("<p class=\"calibre_11\"> </p>\n");
            }
            if let Some(rest) = t.strip_prefix('［') {
                let (n, after) = rest.split_once('］').unwrap_or(("0", rest));
                note_no = n.parse().unwrap_or(note_no + 1);
                body.push_str(&format!("<p id=\"n{file_no}_{note_no}\" class=\"calibre_63\"><a href=\"#r{file_no}_{note_no}\">［{note_no}］</a>{}</p>\n", esc(after)));
            } else {
                note_no = note_no.max(1);
                body.push_str(&format!("<p class=\"calibre_48\">{}</p>\n", esc(t)));
            }
        } else {
            body.push_str(&format!("<p class=\"calibre_1\">{}</p>\n", refs(line)));
        }
    }
    flush(&mut current, &mut files);
    // 每章文件末尾都有 calibre 的分页 div
    for (f, label, body, _) in &files {
        z.add(&format!("OEBPS/{f}"), &xhtml(label, "zh", &format!("{body}<div class=\"mbp_pagebreak\" id=\"calibre_pb_{}\"></div>", f.len())));
    }
    let manifest: String = files.iter().enumerate().map(|(i, (f, _, _, _))| format!("<item id=\"id{i}\" href=\"{f}\" media-type=\"application/xhtml+xml\"/>")).collect();
    let spine: String = (0..files.len()).map(|i| format!("<itemref idref=\"id{i}\"/>")).collect();
    // 目录：第一编下面是导读和三章
    let mut np = String::new();
    let mut i = 0;
    let mut order = 1;
    while i < files.len() {
        let (f, label, _, depth) = &files[i];
        let mut inner = String::new();
        let mut j = i + 1;
        while j < files.len() && files[j].3 > *depth {
            inner.push_str(&format!("<navPoint id=\"p{order}\" playOrder=\"{order}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/></navPoint>", esc(&files[j].1), files[j].0));
            order += 1;
            j += 1;
        }
        np.push_str(&format!("<navPoint id=\"p{order}\" playOrder=\"{order}\"><navLabel><text>{}</text></navLabel><content src=\"{f}\"/>{inner}</navPoint>", esc(label)));
        order += 1;
        i = j;
    }
    z.add("OEBPS/toc.ncx", &format!("<?xml version='1.0' encoding='utf-8'?><ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><head/><docTitle><text>国富论（合成夹具）</text></docTitle><navMap>{np}</navMap></ncx>"));
    z.add(
        "OEBPS/content.opf",
        &format!(
            "<?xml version='1.0' encoding='utf-8'?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>国富论（合成夹具）</dc:title><dc:creator>亚当·斯密</dc:creator><dc:language>zh</dc:language><dc:identifier id=\"id\">urn:pith:fixture:zh</dc:identifier></metadata><manifest><item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>{manifest}</manifest><spine toc=\"ncx\">{spine}</spine></package>"
        ),
    );
    z.finish()
}

pub fn english() -> Text {
    text::from_epub(&english_epub(Flavor::Epub3), None).expect("英文夹具")
}

pub fn chinese() -> Text {
    text::from_epub(&chinese_epub(), None).expect("中文夹具")
}
