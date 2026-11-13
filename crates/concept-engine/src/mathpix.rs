//! PDF 的文本层（`decisions/0024`「PDF」）：Mathpix `v3/pdf` 的 `lines.json`（每页的行，带页号和位置）→ 段落，页码用 PDF 的页序。
//! 没有 Mathpix 密钥时，App 用 v0.3 的本机取文字（`research/content.rs`）按页给文字，走 `from_pages`。
//!
//! 由行拼段落的规则（C4 用真实回复核对）：
//! - 行按回复里的顺序（Mathpix 给的是阅读顺序）；`conversion_output: false` 的、页眉页脚页码（`page_info` 等）、图不要；
//! - 两行的 `parent_id` 都有：相同的接着拼，不同的另起一段；没有 `parent_id` 时看位置：上下间距超过上一行行高的 0.8 倍、
//!   或这一行比上一行缩进超过半个行高（首行缩进），另起一段；
//! - 标题（`title`、`section_header`）单独成段，角色是标题；`footnote` 是注释；
//! - 上一页最后一段没有句末标点、下一页第一行是正文：接着拼，记一个分页点；
//! - 英文行尾的连字符和下一行小写开头：去掉连字符直接接上；中文和中文之间不加空格，其余加一个空格。

use serde_json::Value;

use crate::text::{PageBreak, Paragraph, Role, Text, Unit, UnitKind, Numbering, PARSER_VERSION, is_cjk};

#[derive(Clone, Debug)]
struct Line {
    page: String,
    kind: LineKind,
    text: String,
    parent: Option<String>,
    x: f64,
    y: f64,
    h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineKind {
    Body,
    Heading,
    Note,
}

fn line_kind(t: &str) -> Option<LineKind> {
    Some(match t {
        "title" | "section_header" | "chapter_title" | "heading" => LineKind::Heading,
        "footnote" | "footnotes" => LineKind::Note,
        "page_info" | "page_header" | "page_footer" | "header" | "footer" | "page_number" | "diagram" | "chart" | "image" | "figure" | "equation_number" => return None,
        _ => LineKind::Body,
    })
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end().ends_with(['.', '!', '?', '。', '！', '？', '”', '"', '…', ':', '：', ';', '；', '」', '』'])
}

/// 两行拼起来：英文去连字符、中文不加空格
fn join(a: &mut String, b: &str) {
    let b = b.trim();
    if b.is_empty() {
        return;
    }
    let last = a.chars().last();
    let first = b.chars().next();
    if a.ends_with('-') && first.is_some_and(|c| c.is_lowercase()) && a.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic()) {
        a.pop();
        a.push_str(b);
        return;
    }
    let cjk = |c: Option<char>| c.is_some_and(|c| is_cjk(c) || matches!(c, '，' | '。' | '、' | '；' | '：' | '“' | '”' | '（' | '）' | '《' | '》'));
    if !a.is_empty() && !(cjk(last) && cjk(first)) {
        a.push(' ');
    }
    a.push_str(b);
}

struct Draft {
    kind: LineKind,
    text: String,
    page: String,
    breaks: Vec<PageBreak>,
}

/// `lines.json` → 文本。`mmd` 只在 lines.json 里一行字都没有时用（没有页码）。
pub fn from_lines(lines_json: &Value, mmd: Option<&str>, title: &str, language: Option<&str>) -> Result<Text, String> {
    let pages = lines_json.get("pages").and_then(Value::as_array).ok_or("Mathpix 的 lines.json 里没有 pages。")?;
    let mut lines: Vec<Line> = Vec::new();
    let mut ordered: Vec<&Value> = pages.iter().collect();
    ordered.sort_by_key(|p| p.get("page").and_then(Value::as_u64).unwrap_or(0));
    for (i, p) in ordered.iter().enumerate() {
        let page = p.get("page").and_then(Value::as_u64).map(|n| n.to_string()).unwrap_or_else(|| (i + 1).to_string());
        for l in p.get("lines").and_then(Value::as_array).into_iter().flatten() {
            if l.get("conversion_output").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(kind) = line_kind(l.get("type").and_then(Value::as_str).unwrap_or("text")) else { continue };
            let text = l.get("text").and_then(Value::as_str).or_else(|| l.get("text_display").and_then(Value::as_str)).unwrap_or("").trim().to_string();
            if text.is_empty() {
                continue;
            }
            let r = l.get("region");
            let num = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
            lines.push(Line { page: page.clone(), kind, text, parent: l.get("parent_id").and_then(Value::as_str).map(str::to_string), x: num("top_left_x"), y: num("top_left_y"), h: num("height").max(1.0) });
        }
    }
    if lines.is_empty() {
        if let Some(m) = mmd.filter(|m| !m.trim().is_empty()) {
            let mut t = from_pages(&[m.to_string()], title, language);
            t.page_source = "none".into();
            for p in &mut t.paragraphs {
                p.page = None;
                p.breaks.clear();
            }
            t.warnings.push("Mathpix 的 lines.json 里没有行，改用 mmd 拼段落，没有页码。".into());
            return Ok(t);
        }
        return Err("Mathpix 没有从这份 PDF 里读出文字。".into());
    }
    let mut drafts: Vec<Draft> = Vec::new();
    let mut prev: Option<&Line> = None;
    for l in &lines {
        let new_para = match prev {
            None => true,
            Some(p) if p.kind != l.kind || l.kind == LineKind::Heading => true,
            Some(p) if p.page != l.page => {
                // 跨页：上一段没写完、这一行是正文就接着
                !(l.kind == LineKind::Body && drafts.last().is_some_and(|d| !ends_sentence(&d.text)))
            }
            Some(p) => match (&p.parent, &l.parent) {
                (Some(a), Some(b)) => a != b,
                _ => {
                    let gap = l.y - (p.y + p.h);
                    gap > p.h * 0.8 || l.x > p.x + p.h * 0.5
                }
            },
        };
        if new_para {
            drafts.push(Draft { kind: l.kind, text: String::new(), page: l.page.clone(), breaks: Vec::new() });
        }
        let crossed = !new_para && prev.is_some_and(|p| p.page != l.page);
        if let Some(d) = drafts.last_mut() {
            let before = d.text.chars().count();
            join(&mut d.text, &l.text);
            if crossed {
                // 分页点在新一页的第一个字前（拼接时加的空格不算）
                let spaced = d.text.chars().nth(before) == Some(' ');
                d.breaks.push(PageBreak { at: before + usize::from(spaced), page: l.page.clone() });
            }
        }
        prev = Some(l);
    }
    Ok(build(drafts, title, language, "pdf"))
}

/// 按页给的文字（本机取文字的后备；mmd）→ 文本：空行分段，页码用页序
pub fn from_pages(pages: &[String], title: &str, language: Option<&str>) -> Text {
    let mut drafts: Vec<Draft> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let name = (i + 1).to_string();
        let mut first = true;
        for chunk in page.split("\n\n").map(str::trim).filter(|c| !c.is_empty()) {
            let heading = chunk.starts_with('#') || chunk.starts_with("\\section") || chunk.starts_with("\\title");
            let clean = chunk.trim_start_matches('#').trim_start_matches("\\section*").trim_start_matches("\\section").trim_start_matches("\\title").trim_matches(['{', '}', ' ']).to_string();
            let mut text = String::new();
            for l in clean.lines() {
                join(&mut text, l);
            }
            let kind = if heading { LineKind::Heading } else { LineKind::Body };
            // 上一页最后一段没写完：接着
            if first
                && kind == LineKind::Body
                && let Some(d) = drafts.last_mut()
                && d.kind == LineKind::Body
                && !ends_sentence(&d.text)
            {
                let at = d.text.chars().count() + 1;
                join(&mut d.text, &text);
                d.breaks.push(PageBreak { at: at.min(d.text.chars().count()), page: name.clone() });
                first = false;
                continue;
            }
            first = false;
            drafts.push(Draft { kind, text, page: name.clone(), breaks: Vec::new() });
        }
    }
    build(drafts, title, language, "pdf")
}

fn build(drafts: Vec<Draft>, title: &str, language: Option<&str>, format: &str) -> Text {
    let unit = Unit { index: 0, kind: UnitKind::Part, label: "全文".into(), key: String::new(), depth: 1, parent: None, book: None, chapter: None };
    let mut paragraphs = Vec::new();
    let mut body = 0;
    let mut last_body = None;
    for d in drafts {
        let role = match d.kind {
            LineKind::Body => Role::Body,
            LineKind::Heading => Role::Heading,
            LineKind::Note => Role::Note,
        };
        let seq = paragraphs.len();
        let number = (role == Role::Body).then(|| {
            body += 1;
            body
        });
        if role == Role::Body {
            last_body = Some(seq);
        }
        let marker = (role == Role::Note).then(|| d.text.chars().take_while(|c| c.is_ascii_digit()).collect::<String>()).filter(|m| !m.is_empty());
        paragraphs.push(Paragraph { seq, unit: 0, role, number, key: String::new(), attached: if role == Role::Note { last_body } else { None }, text: d.text, page: Some(d.page), breaks: d.breaks, marker, noterefs: Vec::new() });
    }
    let all: String = paragraphs.iter().filter(|p| p.role == Role::Body).take(20).map(|p| p.text.as_str()).collect();
    let cjk = all.chars().filter(|c| is_cjk(*c)).count();
    let letters = all.chars().filter(|c| c.is_alphabetic()).count().max(1);
    let language = language.map(str::to_string).unwrap_or_else(|| if cjk * 3 > letters { "zh".into() } else { "en".into() });
    let mut t = Text { title: title.to_string(), language, format: format.into(), parser: PARSER_VERSION.into(), numbering: Some(Numbering::Plain), page_source: "pdf".into(), units: vec![unit], paragraphs, ..Default::default() };
    t.renumber(Numbering::Plain);
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(t: &str, text: &str, x: f64, y: f64, parent: Option<&str>) -> Value {
        json!({ "type": t, "text": text, "region": { "top_left_x": x, "top_left_y": y, "width": 1000, "height": 40 }, "parent_id": parent, "conversion_output": true })
    }

    #[test]
    fn lines_become_paragraphs_with_page_numbers() {
        let v = json!({ "pages": [
            { "page": 2, "lines": [
                line("text", "with a hyphen-", 100.0, 100.0, None),
                line("text", "ated word and the end.", 100.0, 145.0, None),
                line("page_info", "2", 500.0, 3000.0, None),
            ]},
            { "page": 1, "lines": [
                line("title", "A Short Article", 100.0, 50.0, None),
                line("text", "The first paragraph goes", 140.0, 120.0, None),
                line("text", "on to the next line.", 100.0, 165.0, None),
                line("text", "A new paragraph after a gap that runs", 140.0, 260.0, None),
                line("text", "over to the next page", 100.0, 305.0, None),
                line("footnote", "1 A note.", 100.0, 2900.0, None),
                line("text", "hidden", 100.0, 3000.0, None),
            ]},
        ]});
        // 被标成不输出的行不要
        let mut v = v;
        v["pages"][1]["lines"][6]["conversion_output"] = json!(false);
        let t = from_lines(&v, None, "x", None).unwrap();
        let roles: Vec<Role> = t.paragraphs.iter().map(|p| p.role).collect();
        assert_eq!(roles, [Role::Heading, Role::Body, Role::Body, Role::Note, Role::Body]);
        assert_eq!(t.paragraphs[1].text, "The first paragraph goes on to the next line.");
        assert_eq!(t.paragraphs[1].key, "1");
        assert_eq!(t.paragraphs[2].page.as_deref(), Some("1"));
        // 第 1 页最后一段正文没写完，但中间隔着脚注：脚注之后第 2 页的行另起一段
        assert_eq!(t.paragraphs[4].text, "with a hyphenated word and the end.");
        assert_eq!(t.paragraphs[4].page.as_deref(), Some("2"));
        assert_eq!(t.numbering, Some(Numbering::Plain));
        assert_eq!(t.language, "en");
    }

    #[test]
    fn a_paragraph_running_across_pages_gets_a_break() {
        let v = json!({ "pages": [
            { "page": 1, "lines": [ line("text", "劳动分工使每个人的", 100.0, 100.0, Some("p1")), line("text", "技能得到提高，", 100.0, 145.0, Some("p1")) ]},
            { "page": 2, "lines": [ line("text", "也节省了时间。", 100.0, 100.0, Some("p2")), line("text", "第二段。", 140.0, 145.0, Some("p3")) ]},
        ]});
        let t = from_lines(&v, None, "x", None).unwrap();
        assert_eq!(t.language, "zh");
        assert_eq!(t.paragraphs.len(), 2);
        assert_eq!(t.paragraphs[0].text, "劳动分工使每个人的技能得到提高，也节省了时间。");
        assert_eq!(t.paragraphs[0].breaks, vec![PageBreak { at: 16, page: "2".into() }]);
        assert_eq!(t.pages_of(0, 0, 20).as_deref(), Some("1–2"));
        assert_eq!(t.pages_of(0, 0, 10).as_deref(), Some("1"));
    }

    #[test]
    fn plain_pages_from_local_extraction() {
        let t = from_pages(&["# Title\n\nOne para\nsplit in lines.\n\nAnother that runs".into(), "on here.\n\nLast.".into()], "x", Some("en"));
        assert_eq!(t.paragraphs.iter().map(|p| p.role).collect::<Vec<_>>(), [Role::Heading, Role::Body, Role::Body, Role::Body]);
        assert_eq!(t.paragraphs[2].text, "Another that runs on here.");
        assert_eq!(t.paragraphs[2].breaks.len(), 1);
        assert_eq!(t.paragraphs[3].key, "3");
    }
}
