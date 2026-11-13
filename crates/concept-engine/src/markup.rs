//! 容错的 XML / XHTML 读取：EPUB 里的 OPF、NCX、导航文档和正文都用它。
//!
//! 为什么不用现成的：`roxmltree` 遇到 `&nbsp;` 这类 HTML 实体就报错；`scraper`（html5ever）按 HTML 解析，
//! 会把 XHTML 的自闭合 `<a id="page_110"/>` 当成没关上的 `<a>`，并在后面的段落里重建它（同一个 id 出现很多次）。
//! 这里自己读：认得自闭合、HTML 实体、没关上的标签、多余的结束标签，读不懂的地方当成文字，从不失败。
//!
//! 名字一律转小写，去掉命名空间前缀（`epub:type` 这类属性名保留前缀，方便按原名取）。

/// 一个元素
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    /// 本地名（小写，去掉前缀）
    pub name: String,
    /// 属性：名字小写，保留前缀（`epub:type`、`xml:lang`）
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// 不管前缀，按本地名取属性（`type` 能取到 `epub:type`）
    pub fn attr_local(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == local || k.rsplit(':').next() == Some(local)).map(|(_, v)| v.as_str())
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id").or_else(|| self.attr("xml:id"))
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// 第一个叫这个名字的子元素
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// 深度优先找第一个满足条件的后代（含自己）
    pub fn find(&self, pred: &dyn Fn(&Element) -> bool) -> Option<&Element> {
        if pred(self) {
            return Some(self);
        }
        self.elements().find_map(|e| e.find(pred))
    }

    /// 全部满足条件的后代（含自己），文档顺序
    pub fn find_all<'a>(&'a self, pred: &dyn Fn(&Element) -> bool, out: &mut Vec<&'a Element>) {
        if pred(self) {
            out.push(self);
        }
        for e in self.elements() {
            e.find_all(pred, out);
        }
    }

    /// 全部文字（空白合并成一个空格，去掉首尾空白）
    pub fn text(&self) -> String {
        let mut raw = String::new();
        self.raw_text(&mut raw);
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn raw_text(&self, out: &mut String) {
        for n in &self.children {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => {
                    if e.name == "br" {
                        out.push(' ');
                    }
                    e.raw_text(out);
                }
            }
        }
    }

    /// `epub:type` 或 `role` 里有没有这个词（`noteref`、`doc-noteref`）
    pub fn has_semantic(&self, word: &str) -> bool {
        let in_list = |v: Option<&str>| v.is_some_and(|v| v.split_whitespace().any(|w| w.eq_ignore_ascii_case(word) || w.eq_ignore_ascii_case(&format!("doc-{word}"))));
        in_list(self.attr("epub:type")) || in_list(self.attr("role"))
    }
}

/// 不能有内容的 HTML 元素：没写自闭合也当自闭合
const VOID: &[&str] = &["br", "hr", "img", "meta", "link", "input", "col", "area", "base", "wbr", "source", "embed", "param", "track"];

/// 读一份文档。返回一个叫 `#document` 的根，下面是顶层元素。
pub fn parse(src: &str) -> Element {
    let mut stack: Vec<Element> = vec![Element { name: "#document".into(), ..Default::default() }];
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let flush_text = |stack: &mut Vec<Element>, s: &str| {
        if s.is_empty() {
            return;
        }
        let t = decode_entities(s);
        if let Some(top) = stack.last_mut() {
            if let Some(Node::Text(prev)) = top.children.last_mut() {
                prev.push_str(&t);
            } else {
                top.children.push(Node::Text(t));
            }
        }
    };
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &src[i..];
        // 注释、CDATA、声明、处理指令
        if rest.starts_with("<!--") {
            flush_text(&mut stack, &src[text_start..i]);
            let end = rest.find("-->").map(|e| i + e + 3).unwrap_or(bytes.len());
            i = end;
            text_start = i;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            flush_text(&mut stack, &src[text_start..i]);
            let body_start = i + 9;
            let end = src[body_start..].find("]]>").map(|e| body_start + e).unwrap_or(bytes.len());
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Text(src[body_start..end].to_string()));
            }
            i = (end + 3).min(bytes.len());
            text_start = i;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            flush_text(&mut stack, &src[text_start..i]);
            i = rest.find('>').map(|e| i + e + 1).unwrap_or(bytes.len());
            text_start = i;
            continue;
        }
        let next = rest[1..].chars().next();
        let is_end = next == Some('/');
        let name_start = if is_end { i + 2 } else { i + 1 };
        let starts_name = src[name_start..].chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
        if !starts_name {
            // 不是标签，当成文字里的「<」
            i += 1;
            continue;
        }
        let Some((tag_end, tag)) = read_tag(src, name_start) else {
            i += 1;
            continue;
        };
        flush_text(&mut stack, &src[text_start..i]);
        i = tag_end;
        text_start = i;
        if is_end {
            let name = local(&tag.name);
            if let Some(pos) = stack.iter().rposition(|e| e.name == name)
                && pos > 0
            {
                while stack.len() > pos {
                    close_top(&mut stack);
                }
            }
            // 没有对应的开始标签：忽略
            continue;
        }
        let name = local(&tag.name);
        let el = Element { name: name.clone(), attrs: tag.attrs, children: Vec::new() };
        // 段落里又开一个段落：先把前一个关上（马虎的 HTML）
        if (name == "p" || name == "li")
            && let Some(pos) = stack.iter().rposition(|e| e.name == name)
            && pos > 0
            && stack[pos + 1..].iter().all(|e| is_inline(&e.name))
        {
            while stack.len() > pos {
                close_top(&mut stack);
            }
        }
        if tag.self_closing || VOID.contains(&name.as_str()) {
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Element(el));
            }
            continue;
        }
        if name == "script" || name == "style" {
            // 内容不当文字：跳到结束标签
            let close = format!("</{}", tag.name);
            let end = find_ci(&src[i..], &close).map(|e| i + e).unwrap_or(bytes.len());
            i = src[end..].find('>').map(|e| end + e + 1).unwrap_or(bytes.len());
            text_start = i;
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Element(el));
            }
            continue;
        }
        stack.push(el);
    }
    flush_text(&mut stack, &src[text_start..]);
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    stack.pop().unwrap_or_default()
}

fn close_top(stack: &mut Vec<Element>) {
    if let Some(done) = stack.pop()
        && let Some(parent) = stack.last_mut()
    {
        parent.children.push(Node::Element(done));
    }
}

pub fn is_inline(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr" | "b" | "bdi" | "bdo" | "big" | "cite" | "code" | "del" | "dfn" | "em" | "font" | "i" | "ins" | "kbd" | "mark" | "q" | "rb" | "rp" | "rt" | "ruby" | "s" | "samp" | "small" | "span" | "strike" | "strong" | "sub" | "sup" | "time" | "tt" | "u" | "var" | "br" | "img" | "wbr"
    )
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase())
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
}

/// 从名字开始读一个标签，返回 `>` 之后的位置。读到文件尾还没有 `>` 返回 None（当成文字）
fn read_tag(src: &str, start: usize) -> Option<(usize, Tag)> {
    let b = src.as_bytes();
    let mut i = start;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    let name = src[start..i].to_string();
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        match b[i] {
            b'>' => return Some((i + 1, Tag { name, attrs, self_closing })),
            b'/' => {
                self_closing = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        self_closing = false;
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let key = src[ns..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                let ve = src[vs..].find(q as char).map(|e| vs + e)?;
                value = decode_entities(&src[vs..ve]);
                i = ve + 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = decode_entities(&src[vs..i]);
            }
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
}

/// 实体换成字：XML 的五个、数字的、常见的 HTML 实体。不认识的原样留着（不丢字）
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').filter(|e| *e <= 32);
        let decoded = end.and_then(|e| {
            let name = &after[..e];
            let c = if let Some(num) = name.strip_prefix('#') {
                let v = if let Some(hex) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(hex, 16).ok() } else { num.parse().ok() };
                v.and_then(char::from_u32).map(|c| c.to_string())
            } else {
                entity(name).map(str::to_string)
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push_str(&c);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{A0}",
        "ensp" => "\u{2002}",
        "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}",
        "zwnj" => "\u{200C}",
        "zwj" => "\u{200D}",
        "shy" => "\u{AD}",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "lsquo" => "‘",
        "rsquo" => "’",
        "sbquo" => "‚",
        "ldquo" => "“",
        "rdquo" => "”",
        "bdquo" => "„",
        "laquo" => "«",
        "raquo" => "»",
        "lsaquo" => "‹",
        "rsaquo" => "›",
        "prime" => "′",
        "Prime" => "″",
        "bull" => "•",
        "middot" => "·",
        "dagger" => "†",
        "Dagger" => "‡",
        "sect" => "§",
        "para" => "¶",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "deg" => "°",
        "plusmn" => "±",
        "times" => "×",
        "divide" => "÷",
        "frac12" => "½",
        "frac14" => "¼",
        "frac34" => "¾",
        "pound" => "£",
        "cent" => "¢",
        "yen" => "¥",
        "euro" => "€",
        "iexcl" => "¡",
        "iquest" => "¿",
        "acute" => "´",
        "uml" => "¨",
        "cedil" => "¸",
        "ordf" => "ª",
        "ordm" => "º",
        "sup1" => "¹",
        "sup2" => "²",
        "sup3" => "³",
        "Agrave" => "À",
        "Aacute" => "Á",
        "Acirc" => "Â",
        "Atilde" => "Ã",
        "Auml" => "Ä",
        "Aring" => "Å",
        "AElig" => "Æ",
        "Ccedil" => "Ç",
        "Egrave" => "È",
        "Eacute" => "É",
        "Ecirc" => "Ê",
        "Euml" => "Ë",
        "Igrave" => "Ì",
        "Iacute" => "Í",
        "Icirc" => "Î",
        "Iuml" => "Ï",
        "Ntilde" => "Ñ",
        "Ograve" => "Ò",
        "Oacute" => "Ó",
        "Ocirc" => "Ô",
        "Otilde" => "Õ",
        "Ouml" => "Ö",
        "Oslash" => "Ø",
        "Ugrave" => "Ù",
        "Uacute" => "Ú",
        "Ucirc" => "Û",
        "Uuml" => "Ü",
        "Yacute" => "Ý",
        "szlig" => "ß",
        "agrave" => "à",
        "aacute" => "á",
        "acirc" => "â",
        "atilde" => "ã",
        "auml" => "ä",
        "aring" => "å",
        "aelig" => "æ",
        "ccedil" => "ç",
        "egrave" => "è",
        "eacute" => "é",
        "ecirc" => "ê",
        "euml" => "ë",
        "igrave" => "ì",
        "iacute" => "í",
        "icirc" => "î",
        "iuml" => "ï",
        "ntilde" => "ñ",
        "ograve" => "ò",
        "oacute" => "ó",
        "ocirc" => "ô",
        "otilde" => "õ",
        "ouml" => "ö",
        "oslash" => "ø",
        "ugrave" => "ù",
        "uacute" => "ú",
        "ucirc" => "û",
        "uuml" => "ü",
        "yacute" => "ý",
        "yuml" => "ÿ",
        "OElig" => "Œ",
        "oelig" => "œ",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_xhtml_with_html_entities_and_self_closing_anchors() {
        let doc = parse(r#"<?xml version="1.0"?><!DOCTYPE html><html xmlns:epub="x"><body><p class="a">T<small>HE</small>&nbsp;first<a id="page_110"/> half &mdash; &amp; &#8217;s &unknown; end</p><p>two</p></body></html>"#);
        let body = doc.find(&|e| e.name == "body").unwrap();
        let ps: Vec<&Element> = body.elements().collect();
        assert_eq!(ps.len(), 2);
        // text() 把不换行空格也当空白合并
        assert_eq!(ps[0].text(), "THE first half — & ’s &unknown; end");
        // 自闭合的 a 没有把后面的文字包进去
        let a = ps[0].find(&|e| e.name == "a").unwrap();
        assert!(a.children.is_empty());
        assert_eq!(a.id(), Some("page_110"));
        assert_eq!(ps[1].text(), "two");
    }

    #[test]
    fn tolerates_broken_markup() {
        // 没关上的段落、多余的结束标签、没写斜线的 br、属性没引号、文字里的 <
        let doc = parse("<body><p>one<br>line<p>two</span></p><div class=x>a < b</div><p>end");
        let body = doc.find(&|e| e.name == "body").unwrap();
        let names: Vec<&str> = body.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["p", "p", "div", "p"]);
        assert_eq!(body.elements().nth(2).unwrap().attr("class"), Some("x"));
        assert_eq!(body.elements().nth(2).unwrap().text(), "a < b");
        assert_eq!(body.elements().last().unwrap().text(), "end");
    }

    #[test]
    fn namespaced_attributes_and_script() {
        let doc = parse(r#"<nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><script>if (a<b) {}</script><ol><li><a href="c1.xhtml#x">One</a></li></ol></nav>"#);
        let nav = doc.find(&|e| e.name == "nav").unwrap();
        assert_eq!(nav.attr("epub:type"), Some("toc"));
        assert_eq!(nav.attr_local("type"), Some("toc"));
        assert_eq!(nav.text(), "One");
    }
}
