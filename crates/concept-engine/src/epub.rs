//! EPUB 读取（输入是字节）：容器 → OPF（元数据、清单、书脊）→ 目录（EPUB 3 nav / EPUB 2 ncx）→ 页码表
//! （EPUB 3 page-list、EPUB 2 `page-map.xml`、ncx 的 pageList、正文里的分页标记）→ 每份正文的「块」。
//!
//! 这里只管把文件读成有顺序的块和目录项；哪一块是正文、旁注、注释，段落号怎么编，在 `text` 里。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;

use crate::markup::{self, Element, Node};

/// 一个目录项
#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    pub label: String,
    /// 在压缩包里的完整路径
    pub file: String,
    pub fragment: Option<String>,
    /// 从 1 起
    pub depth: u32,
}

/// 块：一段文字（段落、标题、列表项……）和它的位置信息
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    /// 块元素的名字（p、h2、blockquote、li……）
    pub tag: String,
    /// 外层的块元素（由外到里），认旁注、注释用
    pub ancestors: Vec<String>,
    /// 自己和外层元素的 epub:type / role（小写）
    pub semantics: Vec<String>,
    pub text: String,
    /// 块里出现的 id 和它们在文字里的位置（按字符）
    pub ids: Vec<(String, usize)>,
    /// 注释的引用标记（上标的「［1］」等）：不进正文，记下位置、标记和指向
    pub noterefs: Vec<NoteRef>,
    /// 段内分页：位置（按字符）和页码
    pub pages: Vec<(usize, String)>,
    /// 全是粗体或链接（中文版的章名是这种 `<p>`）
    pub emphatic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteRef {
    pub at: usize,
    pub marker: String,
    /// 指向的 (文件, id)
    pub target: Option<(String, String)>,
}

/// 一份正文
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub file: String,
    pub blocks: Vec<Block>,
    /// 出现在第一块之前、或不在任何块里的 id → 归到哪一块（下标）
    pub loose_ids: Vec<(String, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub title: String,
    pub language: String,
    pub creators: Vec<String>,
    pub publisher: String,
    pub date: String,
    pub identifiers: Vec<String>,
    pub toc: Vec<TocEntry>,
    /// 按书脊顺序
    pub docs: Vec<Doc>,
    /// 页码从哪来：page-list / page-map / ncx-pagelist / anchors / none
    pub page_source: String,
    pub warnings: Vec<String>,
}

fn read_zip(bytes: &[u8]) -> Result<zip::ZipArchive<std::io::Cursor<&[u8]>>, String> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("这不是一个能打开的 EPUB（压缩包读不了：{e}）。"))
}

fn read_file(z: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, path: &str) -> Option<String> {
    let mut f = z.by_name(path).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(decode_text(&buf))
}

/// 字节 → 文字：去掉 BOM；UTF-16 也认；坏字节换成替换符
fn decode_text(buf: &[u8]) -> String {
    if buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&buf[3..]).into_owned();
    }
    if buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF]) {
        let le = buf[0] == 0xFF;
        let units: Vec<u16> = buf[2..].as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) }).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(buf).into_owned()
}

/// `a/b/c.xhtml` 里引用 `../d.xhtml#x` → (`a/d.xhtml`, Some("x"))
pub fn resolve(base_file: &str, href: &str) -> (String, Option<String>) {
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(percent_decode(f))),
        None => (href, None),
    };
    if path.is_empty() {
        return (base_file.to_string(), fragment);
    }
    let path = percent_decode(path);
    let mut parts: Vec<&str> = if path.starts_with('/') { Vec::new() } else { base_file.split('/').collect() };
    if !path.starts_with('/') {
        parts.pop();
    }
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    (parts.join("/"), fragment)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && s.is_char_boundary(i + 3)
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Item {
    href: String,
    media_type: String,
    properties: String,
}

/// 读一本 EPUB
pub fn read(bytes: &[u8]) -> Result<Book, String> {
    let mut z = read_zip(bytes)?;
    let names: HashSet<String> = z.file_names().filter_map(|n| n.ok().map(|n| n.to_string())).collect();
    let container = read_file(&mut z, "META-INF/container.xml").ok_or("EPUB 里缺 META-INF/container.xml。")?;
    let container = markup::parse(&container);
    let opf_path = container.find(&|e| e.name == "rootfile" && e.attr("full-path").is_some()).and_then(|e| e.attr("full-path")).ok_or("EPUB 的 container.xml 没写内容文件在哪。")?.to_string();
    let opf_text = read_file(&mut z, &opf_path).ok_or_else(|| format!("EPUB 里找不到 {opf_path}。"))?;
    let opf = markup::parse(&opf_text);
    let package = opf.find(&|e| e.name == "package").ok_or("EPUB 的内容文件里没有 package。")?;
    let mut book = Book::default();
    if let Some(meta) = package.child("metadata") {
        let all = |name: &str| meta.elements().filter(|e| e.name == name).map(|e| e.text()).filter(|t| !t.is_empty()).collect::<Vec<_>>();
        book.title = all("title").into_iter().next().unwrap_or_default();
        book.language = all("language").into_iter().next().unwrap_or_default();
        book.creators = all("creator");
        book.publisher = all("publisher").into_iter().next().unwrap_or_default();
        book.date = all("date").into_iter().next().unwrap_or_default();
        book.identifiers = all("identifier");
    }
    let mut manifest: HashMap<String, Item> = HashMap::new();
    if let Some(m) = package.child("manifest") {
        for it in m.elements().filter(|e| e.name == "item") {
            let (Some(id), Some(href)) = (it.attr("id"), it.attr("href")) else { continue };
            let (path, _) = resolve(&opf_path, href);
            manifest.insert(id.to_string(), Item { href: path, media_type: it.attr("media-type").unwrap_or("").to_string(), properties: it.attr("properties").unwrap_or("").to_string() });
        }
    }
    let spine_el = package.child("spine");
    let spine: Vec<String> = spine_el
        .map(|s| s.elements().filter(|e| e.name == "itemref" && e.attr("linear") != Some("no")).filter_map(|e| e.attr("idref")).filter_map(|id| manifest.get(id)).filter(|i| i.media_type.contains("html") || i.href.ends_with("html") || i.href.ends_with(".htm")).map(|i| i.href.clone()).collect())
        .unwrap_or_default();
    if spine.is_empty() {
        return Err("EPUB 的书脊是空的，没有能读的正文。".into());
    }

    // 目录：EPUB 3 的 nav 优先，没有就用 ncx
    let nav_item = manifest.values().find(|i| i.properties.split_whitespace().any(|p| p == "nav")).map(|i| i.href.clone());
    let ncx_item = spine_el.and_then(|s| s.attr("toc")).and_then(|id| manifest.get(id)).map(|i| i.href.clone()).or_else(|| manifest.values().find(|i| i.media_type == "application/x-dtbncx+xml").map(|i| i.href.clone()));
    let mut page_list: Vec<(String, Option<String>, String)> = Vec::new();
    if let Some(nav_path) = &nav_item
        && let Some(text) = read_file(&mut z, nav_path)
    {
        let doc = markup::parse(&text);
        if let Some(toc) = doc.find(&|e| e.name == "nav" && e.has_semantic("toc")) {
            nav_entries(toc, nav_path, 1, &mut book.toc);
        }
        if let Some(pl) = doc.find(&|e| e.name == "nav" && e.has_semantic("page-list")) {
            let mut links = Vec::new();
            pl.find_all(&|e| e.name == "a" && e.attr("href").is_some(), &mut links);
            for a in links {
                let (file, frag) = resolve(nav_path, a.attr("href").unwrap_or(""));
                page_list.push((file, frag, a.text()));
            }
            if !page_list.is_empty() {
                book.page_source = "page-list".into();
            }
        }
    }
    if let Some(ncx_path) = &ncx_item
        && let Some(text) = read_file(&mut z, ncx_path)
    {
        let doc = markup::parse(&text);
        if book.toc.is_empty()
            && let Some(map) = doc.find(&|e| e.name == "navmap")
        {
            ncx_entries(map, ncx_path, 1, &mut book.toc);
        }
        if page_list.is_empty()
            && let Some(pl) = doc.find(&|e| e.name == "pagelist")
        {
            let mut targets = Vec::new();
            pl.find_all(&|e| e.name == "pagetarget", &mut targets);
            for t in targets {
                let label = t.child("navlabel").map(|l| l.text()).unwrap_or_default();
                let label = if label.is_empty() { t.attr("value").unwrap_or("").to_string() } else { label };
                if let Some(src) = t.child("content").and_then(|c| c.attr("src")) {
                    let (file, frag) = resolve(ncx_path, src);
                    page_list.push((file, frag, label));
                }
            }
            if !page_list.is_empty() {
                book.page_source = "ncx-pagelist".into();
            }
        }
    }
    if page_list.is_empty() {
        // EPUB 2 的 page-map.xml：清单里登记的，或者就在内容文件旁边
        let pm = manifest.values().find(|i| i.media_type == "application/oebps-page-map+xml").map(|i| i.href.clone()).or_else(|| names.iter().find(|n| n.rsplit('/').next() == Some("page-map.xml")).cloned());
        if let Some(pm) = pm
            && let Some(text) = read_file(&mut z, &pm)
        {
            let doc = markup::parse(&text);
            let mut pages = Vec::new();
            doc.find_all(&|e| e.name == "page", &mut pages);
            for p in pages {
                let (Some(href), Some(name)) = (p.attr("href"), p.attr("name")) else { continue };
                let (file, frag) = resolve(&pm, href);
                page_list.push((file, frag, name.trim().to_string()));
            }
            if !page_list.is_empty() {
                book.page_source = "page-map".into();
            }
        }
    }

    // 页码表里的文件可能是拆分之前的名字（calibre 把 book1.html 拆成 book1_split_000.html…）：换成实际的文件
    let spine_set: HashSet<&String> = spine.iter().collect();
    let mut page_ids: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut page_starts: HashMap<String, String> = HashMap::new();
    let mut unresolved = 0;
    for (file, frag, name) in &page_list {
        if name.is_empty() {
            continue;
        }
        let candidates: Vec<String> = if spine_set.contains(file) { vec![file.clone()] } else { split_files(file, &spine) };
        if candidates.is_empty() {
            unresolved += 1;
            continue;
        }
        match frag {
            Some(f) => {
                for c in &candidates {
                    page_ids.entry(c.clone()).or_default().insert(f.clone(), name.clone());
                }
            }
            None => {
                page_starts.entry(candidates[0].clone()).or_insert_with(|| name.clone());
            }
        }
    }
    if unresolved > 0 {
        book.warnings.push(format!("页码表里有 {unresolved} 项指向找不到的文件，跳过。"));
    }
    let anchors_only = page_list.is_empty();

    for file in &spine {
        let Some(text) = read_file(&mut z, file) else {
            book.warnings.push(format!("书脊里的 {file} 读不到，跳过。"));
            continue;
        };
        let root = markup::parse(&text);
        let ids = page_ids.get(file);
        let mut doc = extract(&root, file, ids, anchors_only);
        if let Some(start) = page_starts.get(file) {
            if let Some(first) = doc.blocks.first_mut() {
                if first.pages.first().is_none_or(|(at, _)| *at > 0) {
                    first.pages.insert(0, (0, start.clone()));
                }
            } else {
                doc.blocks.push(Block { tag: "#page".into(), pages: vec![(0, start.clone())], ..Default::default() });
            }
        }
        book.docs.push(doc);
    }
    if anchors_only && book.docs.iter().any(|d| d.blocks.iter().any(|b| !b.pages.is_empty())) {
        book.page_source = "anchors".into();
    }
    if book.page_source.is_empty() {
        book.page_source = "none".into();
    }
    Ok(book)
}

/// `dir/book1.html` → 书脊里的 `dir/book1_split_000.html`、`dir/book1_split_001.html`…
fn split_files(file: &str, spine: &[String]) -> Vec<String> {
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => (file, ""),
    };
    let prefix = format!("{stem}_split_");
    spine.iter().filter(|s| s.starts_with(&prefix) && s.ends_with(&format!(".{ext}"))).cloned().collect()
}

fn nav_entries(el: &Element, base: &str, depth: u32, out: &mut Vec<TocEntry>) {
    for child in el.elements() {
        match child.name.as_str() {
            "ol" | "ul" => nav_entries(child, base, depth, out),
            "li" => {
                let link = child.elements().find(|e| e.name == "a" || e.name == "span");
                if let Some(a) = link {
                    let label = a.text();
                    if let Some(href) = a.attr("href") {
                        let (file, fragment) = resolve(base, href);
                        out.push(TocEntry { label, file, fragment, depth });
                    }
                }
                for sub in child.elements().filter(|e| e.name == "ol" || e.name == "ul") {
                    nav_entries(sub, base, depth + 1, out);
                }
            }
            _ => nav_entries(child, base, depth, out),
        }
    }
}

fn ncx_entries(el: &Element, base: &str, depth: u32, out: &mut Vec<TocEntry>) {
    for p in el.elements().filter(|e| e.name == "navpoint") {
        let label = p.child("navlabel").map(|l| l.text()).unwrap_or_default();
        if let Some(src) = p.child("content").and_then(|c| c.attr("src")) {
            let (file, fragment) = resolve(base, src);
            out.push(TocEntry { label, file, fragment, depth });
        }
        ncx_entries(p, base, depth + 1, out);
    }
}

// ---------- 正文 → 块 ----------

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "ul" | "ol" | "dl" | "dt" | "dd" | "pre" | "section" | "article" | "aside" | "header" | "footer" | "nav" | "figure" | "figcaption" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption" | "address" | "main" | "center" | "hr" | "body" | "html"
    )
}

/// 叶子块：里面的文字合成一块（再嵌块元素就拆开）
fn makes_block(name: &str) -> bool {
    matches!(name, "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "dt" | "dd" | "pre" | "figcaption" | "caption" | "td" | "th" | "address" | "section" | "aside" | "article" | "header" | "footer" | "center" | "body")
}

/// 一块正在攒的文字：空白合并，位置按合并后的字符算
#[derive(Default)]
struct Builder {
    text: String,
    chars: usize,
    /// 上一个字是空白（或还没有字）
    space: bool,
    ids: Vec<(String, usize)>,
    noterefs: Vec<NoteRef>,
    pages: Vec<(usize, String)>,
    /// 粗体或链接里的字数、全部字数（判断「全是粗体或链接」）
    emphatic_chars: usize,
    plain_chars: usize,
}

impl Builder {
    fn push_text(&mut self, s: &str, emphatic: bool) {
        for c in s.chars() {
            if c.is_whitespace() && c != '\u{A0}' && c != '\u{3000}' {
                if !self.space && self.chars > 0 {
                    self.text.push(' ');
                    self.chars += 1;
                }
                self.space = true;
                continue;
            }
            self.text.push(c);
            self.chars += 1;
            self.space = false;
            if emphatic {
                self.emphatic_chars += 1;
            } else {
                self.plain_chars += 1;
            }
        }
    }

    fn line_break(&mut self) {
        // 行尾的空格换成换行
        if self.text.ends_with(' ') {
            self.text.pop();
            self.chars -= 1;
        }
        if self.chars > 0 {
            self.text.push('\n');
            self.chars += 1;
        }
        self.space = true;
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.ids.is_empty() && self.pages.is_empty()
    }

    /// 收尾：去掉首尾空白，位置跟着挪
    fn finish(mut self, tag: &str, ancestors: &[String], semantics: &[String]) -> Block {
        let lead = self.text.chars().take_while(|c| c.is_whitespace() && *c != '\u{A0}').count();
        let text: String = self.text.chars().skip(lead).collect();
        let text = text.trim_end_matches(|c: char| c.is_whitespace() && c != '\u{A0}').to_string();
        let len = text.chars().count();
        let fix = |at: usize| at.saturating_sub(lead).min(len);
        for (_, at) in &mut self.ids {
            *at = fix(*at);
        }
        for n in &mut self.noterefs {
            n.at = fix(n.at);
        }
        for (at, _) in &mut self.pages {
            *at = fix(*at);
        }
        let emphatic = self.plain_chars == 0 && self.emphatic_chars > 0;
        Block { tag: tag.to_string(), ancestors: ancestors.to_vec(), semantics: semantics.to_vec(), text, ids: self.ids, noterefs: self.noterefs, pages: self.pages, emphatic }
    }
}

struct Walker<'a> {
    file: &'a str,
    page_ids: Option<&'a HashMap<String, String>>,
    anchors_only: bool,
    blocks: Vec<Block>,
    loose: Vec<String>,
    loose_pages: Vec<String>,
    loose_ids: Vec<(String, usize)>,
}

/// 注释引用的标记：［1］ [1] (1) 1 * † ‡
pub fn note_marker(text: &str) -> Option<String> {
    let t = text.trim();
    let inner = t.trim_start_matches(['［', '[', '(', '（', '〔']).trim_end_matches(['］', ']', ')', '）', '〕']).trim();
    if !inner.is_empty() && inner.len() <= 4 && inner.chars().all(|c| c.is_ascii_digit()) {
        return Some(inner.to_string());
    }
    if !t.is_empty() && t.chars().all(|c| matches!(c, '*' | '†' | '‡' | '§')) && t.chars().count() <= 3 {
        return Some(t.to_string());
    }
    None
}

fn page_name_from_id(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    let rest = lower.strip_prefix("page")?;
    let rest = rest.trim_start_matches(['_', '-', '.']);
    if rest.is_empty() || rest.len() > 8 || !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    // 罗马数字页码（前言）只认小写字母 ivxlcdm
    if rest.chars().all(|c| c.is_ascii_digit()) || rest.chars().all(|c| "ivxlcdm".contains(c)) {
        return Some(id[id.len() - rest.len()..].to_string());
    }
    None
}

impl Walker<'_> {
    fn semantics_of(e: &Element) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["epub:type", "role"] {
            if let Some(v) = e.attr(key) {
                out.extend(v.split_whitespace().map(|w| w.to_ascii_lowercase()));
            }
        }
        out
    }

    /// 一个元素是不是分页标记；是的话返回页码
    fn page_mark(&self, e: &Element) -> Option<String> {
        if let Some(id) = e.id() {
            if let Some(map) = self.page_ids
                && let Some(name) = map.get(id)
            {
                return Some(name.clone());
            }
            if self.anchors_only
                && let Some(name) = page_name_from_id(id)
            {
                return Some(name);
            }
        }
        if e.has_semantic("pagebreak") {
            let name = e.attr("title").or_else(|| e.attr("aria-label")).map(str::to_string).unwrap_or_else(|| e.text());
            let name = name.trim().trim_start_matches(['p', 'P']).trim_start_matches(['.', ' ']).to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    }

    fn walk(&mut self, e: &Element, ancestors: &mut Vec<String>, semantics: &mut Vec<String>) {
        let mine = Self::semantics_of(e);
        let pushed = mine.len();
        semantics.extend(mine);
        if !makes_block(&e.name) {
            // 不出块的容器（ul、table……）：只往里走
            ancestors.push(e.name.clone());
            for n in &e.children {
                if let Node::Element(c) = n {
                    self.walk(c, ancestors, semantics);
                }
            }
            ancestors.pop();
            semantics.truncate(semantics.len() - pushed);
            return;
        }
        let mut b = Builder { space: true, ..Default::default() };
        // 容器自己的 id 和分页记在第一块上
        if let Some(id) = e.id() {
            self.loose.push(id.to_string());
        }
        if let Some(p) = self.page_mark(e) {
            self.loose_pages.push(p);
        }
        self.take_loose(&mut b);
        ancestors.push(e.name.clone());
        for n in &e.children {
            match n {
                Node::Text(t) => b.push_text(t, false),
                Node::Element(c) if is_block(&c.name) && c.name != "hr" => {
                    self.flush(&mut b, &e.name, &ancestors[..ancestors.len() - 1], semantics);
                    self.walk(c, ancestors, semantics);
                }
                Node::Element(c) if c.name == "hr" => {
                    self.flush(&mut b, &e.name, &ancestors[..ancestors.len() - 1], semantics);
                }
                Node::Element(c) => self.inline(c, &mut b, false),
            }
        }
        ancestors.pop();
        self.flush(&mut b, &e.name, ancestors, semantics);
        semantics.truncate(semantics.len() - pushed);
    }

    fn take_loose(&mut self, b: &mut Builder) {
        for id in self.loose.drain(..) {
            b.ids.push((id, 0));
        }
        for p in self.loose_pages.drain(..) {
            b.pages.push((0, p));
        }
    }

    fn flush(&mut self, b: &mut Builder, tag: &str, ancestors: &[String], semantics: &[String]) {
        let done = std::mem::replace(b, Builder { space: true, ..Default::default() });
        if done.is_empty() {
            // 空块：id 和分页留给下一块
            for (id, _) in done.ids {
                self.loose.push(id);
            }
            for (_, p) in done.pages {
                self.loose_pages.push(p);
            }
            return;
        }
        self.blocks.push(done.finish(tag, ancestors, semantics));
    }

    fn inline(&mut self, e: &Element, b: &mut Builder, emphatic: bool) {
        if let Some(id) = e.id() {
            b.ids.push((id.to_string(), b.chars));
        }
        if let Some(p) = self.page_mark(e) {
            b.pages.push((b.chars, p));
            // 分页标记里的字（「110」）不是正文
            if e.has_semantic("pagebreak") {
                return;
            }
        }
        if e.name == "br" {
            b.line_break();
            return;
        }
        if e.name == "img" {
            if let Some(alt) = e.attr("alt").filter(|a| !a.trim().is_empty() && a.chars().count() <= 4) {
                b.push_text(alt, emphatic);
            }
            return;
        }
        // 注释引用：上标里的「［1］」、epub:type="noteref" 的链接
        if let Some(r) = self.noteref(e) {
            b.noterefs.push(NoteRef { at: b.chars, ..r });
            // 引用标记里的 id（回链用的锚点）照记
            let mut inner = Vec::new();
            e.find_all(&|x| x.id().is_some(), &mut inner);
            for x in inner {
                if let Some(id) = x.id()
                    && Some(id) != e.id()
                {
                    b.ids.push((id.to_string(), b.chars));
                }
            }
            return;
        }
        let emph = emphatic || matches!(e.name.as_str(), "b" | "strong" | "a") || e.attr("class").is_some_and(|c| c.split_whitespace().any(|w| w == "bold"));
        for n in &e.children {
            match n {
                Node::Text(t) => b.push_text(t, emph),
                Node::Element(c) => self.inline(c, b, emph),
            }
        }
    }

    fn noteref(&self, e: &Element) -> Option<NoteRef> {
        let is_sup = e.name == "sup";
        let links = {
            let mut v = Vec::new();
            e.find_all(&|x| x.name == "a" && x.attr("href").is_some_and(|h| h.contains('#')), &mut v);
            v
        };
        let semantic = e.has_semantic("noteref") || links.iter().any(|a| a.has_semantic("noteref"));
        let link_ok = e.name == "a" && e.attr("href").is_some_and(|h| h.contains('#'));
        // 上标里要有链接才算（没有链接的上标可能是指数「x²」）；单独的链接要标了 noteref
        if !(semantic || (is_sup && !links.is_empty())) {
            return None;
        }
        let marker = note_marker(&e.text())?;
        let target = links.first().or(if link_ok { Some(&e) } else { None }).and_then(|a| a.attr("href")).map(|h| resolve(self.file, h)).and_then(|(f, frag)| frag.map(|x| (f, x)));
        Some(NoteRef { at: 0, marker, target })
    }
}

fn extract(root: &Element, file: &str, page_ids: Option<&HashMap<String, String>>, anchors_only: bool) -> Doc {
    let mut w = Walker { file, page_ids, anchors_only, blocks: Vec::new(), loose: Vec::new(), loose_pages: Vec::new(), loose_ids: Vec::new() };
    let body = root.find(&|e| e.name == "body").unwrap_or(root);
    w.walk(body, &mut Vec::new(), &mut Vec::new());
    // 最后剩下的 id 和分页（文件末尾的空锚点）：挂到最后一块的末尾
    if !w.loose.is_empty() || !w.loose_pages.is_empty() {
        let n = w.blocks.len();
        if n > 0 {
            let last = &mut w.blocks[n - 1];
            let end = last.text.chars().count();
            for id in w.loose.drain(..) {
                last.ids.push((id, end));
            }
            for p in w.loose_pages.drain(..) {
                last.pages.push((end, p));
            }
        } else {
            for id in w.loose.drain(..) {
                w.loose_ids.push((id, 0));
            }
            if !w.loose_pages.is_empty() {
                w.blocks.push(Block { tag: "#page".into(), pages: w.loose_pages.drain(..).map(|p| (0, p)).collect(), ..Default::default() });
            }
        }
    }
    Doc { file: file.to_string(), blocks: w.blocks, loose_ids: w.loose_ids }
}

/// 块里有没有这个 id（目录项的 #片段 落在哪一块）
pub fn block_with_id(doc: &Doc, id: &str) -> Option<usize> {
    doc.blocks.iter().position(|b| b.ids.iter().any(|(x, _)| x == id))
}

/// 所有被注释引用指向的 (文件, id)
pub fn note_targets(book: &Book) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for d in &book.docs {
        for b in &d.blocks {
            for r in &b.noterefs {
                if let Some(t) = &r.target {
                    out.insert(t.clone(), r.marker.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve("OEBPS/content.opf", "text/c1.xhtml"), ("OEBPS/text/c1.xhtml".into(), None));
        assert_eq!(resolve("OEBPS/text/nav.xhtml", "../text/c%201.xhtml#p_5"), ("OEBPS/text/c 1.xhtml".into(), Some("p_5".into())));
        assert_eq!(resolve("a/b.html", "#x"), ("a/b.html".into(), Some("x".into())));
        assert_eq!(resolve("content.opf", "1/OEBPS/html/book1.html"), ("1/OEBPS/html/book1.html".into(), None));
    }

    #[test]
    fn blocks_keep_small_caps_and_record_marks() {
        let root = markup::parse(r#"<html><body><h2><a id="page_109"/><a id="ch01"/>CHAPTER I</h2><p>T<small>HE</small> greatest <a id="page_110"/>improvement<sup><a href="n.html#n1">［1］</a></sup> here.</p><blockquote>A side note</blockquote><div><p>inner</p>tail text</div></body></html>"#);
        let doc = extract(&root, "c.html", None, true);
        let b = &doc.blocks;
        assert_eq!(b.len(), 5);
        assert_eq!(b[0].text, "CHAPTER I");
        assert_eq!(b[0].pages, vec![(0, "109".to_string())]);
        assert_eq!(b[1].text, "THE greatest improvement here.");
        assert_eq!(b[1].pages, vec![(13, "110".to_string())]);
        assert_eq!(b[1].noterefs.len(), 1);
        assert_eq!(b[1].noterefs[0].marker, "1");
        assert_eq!(b[1].noterefs[0].at, 24);
        assert_eq!(b[1].noterefs[0].target, Some(("n.html".into(), "n1".into())));
        assert_eq!(b[2].tag, "blockquote");
        assert_eq!(b[3].text, "inner");
        assert_eq!(b[3].ancestors, vec!["body".to_string(), "div".to_string()]);
        assert_eq!(b[4].text, "tail text");
    }

    #[test]
    fn line_breaks_and_emphasis() {
        let root = markup::parse(r#"<body><p><a href="toc.html"><span class="bold">第一章</span></a><br/><span class="bold">论分工</span></p><p>普通<b>粗</b></p></body>"#);
        let doc = extract(&root, "c.html", None, true);
        assert_eq!(doc.blocks[0].text, "第一章\n论分工");
        assert!(doc.blocks[0].emphatic);
        assert!(!doc.blocks[1].emphatic);
    }

    #[test]
    fn page_ids_only_look_like_pages() {
        assert_eq!(page_name_from_id("page_110").as_deref(), Some("110"));
        assert_eq!(page_name_from_id("page-xii").as_deref(), Some("xii"));
        assert_eq!(page_name_from_id("pagebreak"), None);
        assert_eq!(page_name_from_id("filepos103460"), None);
        assert_eq!(note_marker("［12］").as_deref(), Some("12"));
        assert_eq!(note_marker("*").as_deref(), Some("*"));
        assert_eq!(note_marker("note"), None);
    }
}
