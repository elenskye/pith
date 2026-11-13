//! 文本层：一本书 → 单元（篇、章、节、导读、注释、前后附）→ 段落（角色、段落号、页码）。
//!
//! - **角色**靠规则认，不看 class 名（两本真实 EPUB 的 class 名都没有意义）：
//!   1. 先按目录项的文字分单元：「Book One / 第一编」是篇，「CHAPTER I / 第一章」是章，「PART I / 第一部分」是节，
//!      「导读 / 导言 / Analytical Introduction」是导读，「Notes / 注释 / Footnotes」是注释，「Contents / 目录 / Index / 版权页 / 序」等是前后附；
//!      上一层是导读、注释、前后附的，下面都跟着它。
//!   2. 篇、章、节和其他正文单元里再逐块认：h1–h6、单元开头全是粗体或链接、或和目录文字一样的是标题；
//!      短的 `blockquote`（或 epub:type 是 marginalia / sidebar 的）是旁注；
//!      以「［1］」这类标记开头、并且是注释引用指向的（或者前面正文里有同一个标记）的段落开始「注释区」，到单元结尾都是注释；
//!      epub:type 是 footnote / endnote 的是注释；其余是正文。
//! - **段落号**只给正文：「篇.章.段」（Glasgow 式，`I.iii.5`）或「章.段」（`3.5`）；目录有篇、章两层时默认前一种。
//!   旁注、注释挂到前面的正文段上（注释先找带同一个引用标记的正文段）。
//! - **页码**：段落开头所在的页，加上段内的分页点（按字符位置）；一句引文的页码按它在段内的位置算，跨页写「110–111」。
//! - 原文一个字不改：小型大写照样取字（`T<small>HE</small>` → `THE`）；只有注释的引用标记（上标的「［1］」）不进正文，记下位置。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::epub::{self, Block};

/// 解析器版本：规则变了就改，存下的文本记着它（`decisions/0024`「存进主库」）
pub const PARSER_VERSION: &str = "concept-text-1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 原著正文
    Body,
    Heading,
    /// 旁注（页边提要）
    Sidenote,
    /// 脚注、尾注、译者按
    Note,
    /// 导读、导言
    Guide,
    /// 前后附：目录、前言、索引、版权页……
    Matter,
}

impl Role {
    pub const ALL: [Role; 6] = [Role::Body, Role::Heading, Role::Sidenote, Role::Note, Role::Guide, Role::Matter];

    pub fn label(self) -> &'static str {
        match self {
            Role::Body => "正文",
            Role::Heading => "标题",
            Role::Sidenote => "旁注",
            Role::Note => "注释",
            Role::Guide => "导读",
            Role::Matter => "前后附",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Role::Body => "body",
            Role::Heading => "heading",
            Role::Sidenote => "sidenote",
            Role::Note => "note",
            Role::Guide => "guide",
            Role::Matter => "matter",
        }
    }

    pub fn from_key(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Book,
    Chapter,
    /// 章里的一节（段落号接着章往下编）
    Section,
    /// 不在章里的正文单元（「引言和本书计划」）
    Part,
    /// 只装着别的单元（「The Wealth of Nations」「《国富论》（上）」）
    Container,
    Guide,
    Notes,
    Matter,
}

impl UnitKind {
    pub fn key(self) -> &'static str {
        match self {
            UnitKind::Book => "book",
            UnitKind::Chapter => "chapter",
            UnitKind::Section => "section",
            UnitKind::Part => "part",
            UnitKind::Container => "container",
            UnitKind::Guide => "guide",
            UnitKind::Notes => "notes",
            UnitKind::Matter => "matter",
        }
    }

    pub fn from_key(s: &str) -> Option<UnitKind> {
        [UnitKind::Book, UnitKind::Chapter, UnitKind::Section, UnitKind::Part, UnitKind::Container, UnitKind::Guide, UnitKind::Notes, UnitKind::Matter].into_iter().find(|k| k.key() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Numbering {
    /// 篇.章.段：I.iii.5
    Glasgow,
    /// 章.段：3.5
    Chapter,
    /// 只有段：5（短 PDF）
    Plain,
}

impl Numbering {
    pub fn key(self) -> &'static str {
        match self {
            Numbering::Glasgow => "glasgow",
            Numbering::Chapter => "chapter",
            Numbering::Plain => "plain",
        }
    }

    pub fn from_key(s: &str) -> Option<Numbering> {
        [Numbering::Glasgow, Numbering::Chapter, Numbering::Plain].into_iter().find(|n| n.key() == s)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {
    pub index: usize,
    pub kind: UnitKind,
    pub label: String,
    /// 章是「I.iii」（Glasgow）或「3」；别的单元见 `unit_key`
    pub key: String,
    pub depth: u32,
    pub parent: Option<usize>,
    /// 第几篇、第几章（从目录文字里读出来的序号）
    pub book: Option<u32>,
    pub chapter: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBreak {
    /// 段内第几个字之前分页（按字符）
    pub at: usize,
    pub page: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    /// 在全书里的顺序，从 0 起
    pub seq: usize,
    /// 编段落号的单元（章里的节记成它的章）；导读、注释、前后附是它们自己的单元
    pub unit: usize,
    pub role: Role,
    /// 正文段的序号（单元内从 1 起）
    pub number: Option<u32>,
    /// 段落号或别的引用名，全书唯一：I.i.3、I.i.3 旁注、I.i 注 1、前三章导读 ¶2
    pub key: String,
    /// 旁注、注释挂在哪个正文段上（seq）
    pub attached: Option<usize>,
    pub text: String,
    /// 段落开头在哪一页（版本里没有页码就是空）
    pub page: Option<String>,
    pub breaks: Vec<PageBreak>,
    /// 注释开头的标记（「1」）
    pub marker: Option<String>,
    /// 正文里注释引用的位置和标记（引用标记本身不在 `text` 里）
    pub noterefs: Vec<(usize, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub title: String,
    /// en / zh / 其他
    pub language: String,
    pub creators: Vec<String>,
    pub publisher: String,
    pub date: String,
    pub identifiers: Vec<String>,
    /// epub / pdf
    pub format: String,
    pub parser: String,
    pub numbering: Option<Numbering>,
    /// page-list / page-map / ncx-pagelist / anchors / pdf / none
    pub page_source: String,
    pub units: Vec<Unit>,
    pub paragraphs: Vec<Paragraph>,
    pub warnings: Vec<String>,
}

// ---------- 序号 ----------

pub fn roman(n: u32) -> String {
    let table = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut n = n;
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn parse_roman(s: &str) -> Option<u32> {
    let s = s.to_ascii_uppercase();
    if s.is_empty() || !s.chars().all(|c| "IVXLCDM".contains(c)) {
        return None;
    }
    let val = |c| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        _ => 1000,
    };
    let chars: Vec<u32> = s.chars().map(val).collect();
    let mut total = 0i64;
    for (i, v) in chars.iter().enumerate() {
        if chars.get(i + 1).is_some_and(|n| n > v) {
            total -= *v as i64;
        } else {
            total += *v as i64;
        }
    }
    (total > 0 && roman(total as u32) == s).then_some(total as u32)
}

fn parse_english_number(s: &str) -> Option<u32> {
    const ONES: [&str; 20] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    let s = s.to_ascii_lowercase();
    if let Some(i) = ONES.iter().position(|w| *w == s) {
        return Some(i as u32);
    }
    let (t, o) = s.split_once('-').unwrap_or((&s, ""));
    let ti = TENS.iter().position(|w| !w.is_empty() && *w == t)? as u32;
    let oi = if o.is_empty() { 0 } else { ONES.iter().position(|w| *w == o).filter(|i| *i < 10)? as u32 };
    Some(ti * 10 + oi)
}

pub fn parse_chinese_number(s: &str) -> Option<u32> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse().ok();
    }
    let digit = |c| match c {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    let mut total = 0;
    let mut cur = 0;
    let mut any = false;
    for c in s.chars() {
        any = true;
        match c {
            '十' => {
                total += if cur == 0 { 10 } else { cur * 10 };
                cur = 0;
            }
            '百' => {
                total += if cur == 0 { 100 } else { cur * 100 };
                cur = 0;
            }
            c => cur = cur * 10 + digit(c)?,
        }
    }
    any.then_some(total + cur)
}

fn ordinal_word(w: &str) -> Option<u32> {
    let w = w.trim_end_matches(['.', ':', ',']);
    w.parse().ok().or_else(|| parse_roman(w)).or_else(|| parse_english_number(w))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Label {
    Book(Option<u32>),
    Chapter(Option<u32>),
    Section,
    Guide,
    Notes,
    Matter,
    Other,
}

const MATTER_WORDS: &[&str] = &[
    "contents", "table of contents", "copyright", "title page", "titlepage", "cover", "index", "indexs", "preface", "foreword", "acknowledg", "abbreviation", "key dates", "chronology", "bibliography", "further reading", "glossary", "dedication", "epigraph", "colophon", "about the", "a note on", "list of", "目录", "目次", "版权", "书名页", "扉页", "封面", "索引", "前言", "序言", "总序", "丛书", "致谢", "缩略", "年表", "参考文献", "书目", "术语表", "关于译者", "关于作者", "译后记", "后记", "出版说明",
];
const NOTES_WORDS: &[&str] = &["footnotes", "endnotes", "notes", "注释", "尾注", "脚注"];
const GUIDE_WORDS: &[&str] = &["导读", "导言", "导论", "解读", "analytical introduction", "editor's introduction", "editor’s introduction", "translator's introduction", "translator’s introduction", "introduction by", "commentary"];

fn classify(label: &str) -> Label {
    let t = label.trim();
    let lower = t.to_lowercase();
    let has = |list: &[&str]| list.iter().any(|w| lower.contains(w));
    // 导读先认：「〔第四章导读〕」不是第四章
    if has(GUIDE_WORDS) {
        return Label::Guide;
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    if let [first, second, ..] = words.as_slice() {
        if matches!(*first, "book" | "livre" | "volume") {
            return Label::Book(ordinal_word(second));
        }
        if *first == "chapter" || *first == "chap." {
            return Label::Chapter(ordinal_word(second));
        }
        if matches!(*first, "part" | "section" | "sect.") {
            return Label::Section;
        }
    }
    // 第一编、第一篇、第三章、第一部分、第二节
    let cleaned: String = t.chars().filter(|c| !c.is_whitespace() && *c != '\u{3000}').collect();
    let cleaned = cleaned.trim_start_matches(['〔', '［', '【', '(', '（']);
    if let Some(rest) = cleaned.strip_prefix('第') {
        let num: String = rest.chars().take_while(|c| "零〇一二两三四五六七八九十百0123456789".contains(*c)).collect();
        let after: String = rest.chars().skip(num.chars().count()).collect();
        if !num.is_empty() {
            let n = parse_chinese_number(&num);
            if after.starts_with('编') || after.starts_with('篇') || after.starts_with('卷') {
                return Label::Book(n);
            }
            if after.starts_with('章') {
                return Label::Chapter(n);
            }
            if after.starts_with("部分") || after.starts_with('节') {
                return Label::Section;
            }
        }
    }
    if has(NOTES_WORDS) && !lower.contains("plan") {
        return Label::Notes;
    }
    if has(MATTER_WORDS) {
        return Label::Matter;
    }
    Label::Other
}

fn intro_like(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("introduction") || l.contains("引言") || l.contains("引 言")
}

/// 去掉括号、空白的单元名（引用名里用）
fn short_label(label: &str) -> String {
    let t: String = label.chars().filter(|c| !matches!(c, '〔' | '〕' | '［' | '］' | '【' | '】')).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.replace('\u{3000}', " ");
    let chars: Vec<char> = t.chars().collect();
    if chars.len() > 24 { chars[..24].iter().collect::<String>() + "…" } else { t }
}

// ---------- EPUB → Text ----------

struct Entry {
    toc_index: usize,
    label: String,
    depth: u32,
    pos: usize,
}

/// 解析一本 EPUB。`numbering` 为空时按目录自动选（有篇、章两层用 Glasgow 式）
pub fn from_epub(bytes: &[u8], numbering: Option<Numbering>) -> Result<Text, String> {
    let book = epub::read(bytes)?;
    Ok(from_book(&book, numbering))
}

pub fn from_book(book: &epub::Book, numbering: Option<Numbering>) -> Text {
    // 所有块按书脊顺序排平
    let mut flat: Vec<&Block> = Vec::new();
    let mut doc_start: HashMap<&str, usize> = HashMap::new();
    for d in &book.docs {
        doc_start.insert(d.file.as_str(), flat.len());
        flat.extend(d.blocks.iter());
    }
    let mut warnings = book.warnings.clone();
    // 目录项 → 在平铺块里的起点
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped = 0;
    for (i, e) in book.toc.iter().enumerate() {
        let Some(doc) = book.docs.iter().find(|d| d.file == e.file) else {
            skipped += 1;
            continue;
        };
        let start = doc_start[doc.file.as_str()];
        let offset = match &e.fragment {
            Some(f) => epub::block_with_id(doc, f).or_else(|| doc.loose_ids.iter().find(|(x, _)| x == f).map(|_| 0)).unwrap_or(0),
            None => 0,
        };
        entries.push(Entry { toc_index: i, label: e.label.trim().to_string(), depth: e.depth, pos: start + offset });
    }
    if skipped > 0 {
        warnings.push(format!("目录里有 {skipped} 项指向书脊以外的文件，跳过。"));
    }
    // 单元：目录项（按目录顺序，记上下级）
    let mut units: Vec<Unit> = Vec::new();
    let mut labels: Vec<Label> = Vec::new();
    let mut stack: Vec<(u32, usize)> = Vec::new();
    for e in &entries {
        while stack.last().is_some_and(|(d, _)| *d >= e.depth) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, i)| *i);
        let index = units.len();
        units.push(Unit { index, kind: UnitKind::Part, label: e.label.clone(), key: String::new(), depth: e.depth, parent, book: None, chapter: None });
        labels.push(classify(&e.label));
        stack.push((e.depth, index));
    }
    // 定种类：上一层是导读 / 注释 / 前后附的跟着它
    let has_descendant = |units: &[Unit], labels: &[Label], i: usize, want: &dyn Fn(Label) -> bool| -> bool {
        (i + 1..units.len()).take_while(|j| units[*j].depth > units[i].depth).any(|j| want(labels[j]))
    };
    // 目录是平的（篇和章同一层，Penguin 第二册）：章跟着前面最近的同层的篇
    let mut flat_book: Option<(u32, Option<usize>, Option<u32>)> = None;
    for i in 0..units.len() {
        let parent_kind = units[i].parent.map(|p| units[p].kind);
        let in_chapter = ancestors(&units, i).any(|a| units[a].kind == UnitKind::Chapter);
        let book_no = ancestors(&units, i)
            .find(|a| units[*a].kind == UnitKind::Book)
            .and_then(|a| units[a].book)
            .or_else(|| flat_book.filter(|(d, p, _)| *d == units[i].depth && *p == units[i].parent).and_then(|(_, _, b)| b));
        let kind = match (parent_kind, labels[i]) {
            (Some(k @ (UnitKind::Guide | UnitKind::Notes | UnitKind::Matter)), _) => k,
            (_, Label::Guide) => UnitKind::Guide,
            (_, Label::Notes) => UnitKind::Notes,
            (_, Label::Matter) => UnitKind::Matter,
            (_, Label::Book(n)) => {
                units[i].book = n;
                flat_book = Some((units[i].depth, units[i].parent, n));
                UnitKind::Book
            }
            (_, Label::Chapter(n)) => {
                units[i].chapter = n;
                units[i].book = book_no;
                UnitKind::Chapter
            }
            (_, Label::Section | Label::Other) if in_chapter => UnitKind::Section,
            (_, Label::Section | Label::Other) => {
                if has_descendant(&units, &labels, i, &|l| matches!(l, Label::Book(_) | Label::Chapter(_))) {
                    UnitKind::Container
                } else {
                    units[i].book = book_no;
                    UnitKind::Part
                }
            }
        };
        units[i].kind = kind;
    }
    // 章的序号没读出来的，按在本篇里的顺序补
    let mut counters: HashMap<Option<usize>, u32> = HashMap::new();
    for i in 0..units.len() {
        if units[i].kind == UnitKind::Chapter {
            let book_unit = ancestors(&units, i).find(|a| units[*a].kind == UnitKind::Book);
            let c = counters.entry(book_unit).or_insert(0);
            *c += 1;
            if units[i].chapter.is_none() {
                units[i].chapter = Some(*c);
            }
        }
    }
    // 第一项之前的内容（封面、书名页）：补一个「前附」单元
    let first_pos = entries.iter().map(|e| e.pos).min().unwrap_or(flat.len());
    let mut order: Vec<(usize, usize, usize)> = entries.iter().enumerate().map(|(u, e)| (e.pos, e.toc_index, u)).collect();
    if first_pos > 0 || entries.is_empty() {
        let index = units.len();
        units.push(Unit { index, kind: if entries.is_empty() { UnitKind::Part } else { UnitKind::Matter }, label: if entries.is_empty() { "全文".into() } else { "前附".into() }, key: String::new(), depth: 1, parent: None, book: None, chapter: None });
        order.push((0, 0, index));
    }
    order.sort();
    // 每一块归哪个单元：起点不晚于它的最后一个（同一处起的，目录里靠后、更深的那个）
    let owner_of = |pos: usize| -> usize {
        let i = order.partition_point(|(p, _, _)| *p <= pos);
        order[i.saturating_sub(1)].2
    };
    let default_numbering = {
        let glasgow = units.iter().any(|u| u.kind == UnitKind::Chapter && u.book.is_some());
        if glasgow { Numbering::Glasgow } else { Numbering::Chapter }
    };
    let numbering = numbering.unwrap_or(default_numbering);
    // 编段落号的单元：章里的节记到章上
    let numbering_unit = |u: usize| -> usize {
        if units[u].kind == UnitKind::Section {
            return ancestors(&units, u).find(|a| units[*a].kind == UnitKind::Chapter).unwrap_or(u);
        }
        u
    };
    let targets = epub::note_targets(book);
    let target_blocks: std::collections::HashSet<usize> = {
        let mut s = std::collections::HashSet::new();
        for ((file, id), _) in &targets {
            if let (Some(doc), Some(start)) = (book.docs.iter().find(|d| &d.file == file), doc_start.get(file.as_str())) {
                if let Some(b) = epub::block_with_id(doc, id) {
                    s.insert(start + b);
                }
            }
        }
        s
    };

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut page: Option<String> = None;
    // 每个编号单元（章）里的状态：正文数到几、上一段正文、见过的注释标记
    #[derive(Default)]
    struct Numbered {
        body: u32,
        last_body: Option<usize>,
        markers_seen: Vec<String>,
    }
    // 每个目录单元里的状态：有没有正文了（认标题用）、进没进注释区
    #[derive(Default)]
    struct Owned {
        has_body: bool,
        in_notes: bool,
    }
    let mut numbered: HashMap<usize, Numbered> = HashMap::new();
    let mut owned: HashMap<usize, Owned> = HashMap::new();
    for (pos, block) in flat.iter().enumerate() {
        let owner = owner_of(pos);
        let marks: Vec<(usize, String)> = {
            let mut m = block.pages.clone();
            m.sort_by_key(|(at, _)| *at);
            m
        };
        let text_len = block.text.chars().count();
        if block.text.trim().is_empty() {
            if let Some((_, p)) = marks.last() {
                page = Some(p.clone());
            }
            continue;
        }
        let start_page = marks.iter().filter(|(at, _)| *at == 0).last().map(|(_, p)| p.clone()).or_else(|| page.clone());
        let breaks: Vec<PageBreak> = marks.iter().filter(|(at, _)| *at > 0 && *at < text_len).map(|(at, p)| PageBreak { at: *at, page: p.clone() }).collect();
        if let Some((_, p)) = marks.last() {
            page = Some(p.clone());
        }
        let nu = numbering_unit(owner);
        let kind = units[owner].kind;
        let st = numbered.entry(nu).or_default();
        let ow = owned.entry(owner).or_default();
        let label_folded: String = units[owner].label.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        let text_folded: String = block.text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        let headingish = block.tag.starts_with('h') && block.tag.len() == 2
            || (!ow.has_body && text_len <= 120 && (block.emphatic || (!label_folded.is_empty() && (label_folded.contains(&text_folded) || (text_folded.starts_with(&label_folded) && text_folded.chars().count() <= label_folded.chars().count() + 8)))));
        let semantics = |w: &str| block.semantics.iter().any(|s| s == w || s == &format!("doc-{w}"));
        // 注释开头的标记：文字里的「［1］」，或者开头那个回到正文的链接（它被当成引用标记拿掉了）
        let leading_marker = leading_note_marker(&block.text).or_else(|| block.noterefs.first().filter(|r| r.at == 0).map(|r| r.marker.clone()));
        let role = match kind {
            UnitKind::Matter => Role::Matter,
            UnitKind::Guide => {
                if headingish {
                    Role::Heading
                } else {
                    Role::Guide
                }
            }
            UnitKind::Notes => {
                if headingish {
                    Role::Heading
                } else {
                    Role::Note
                }
            }
            _ => {
                if ow.in_notes {
                    Role::Note
                } else if semantics("footnote") || semantics("endnote") || semantics("footnotes") || semantics("endnotes") || semantics("rearnote") || semantics("rearnotes") {
                    Role::Note
                } else if headingish {
                    Role::Heading
                } else if leading_marker.as_ref().is_some_and(|m| target_blocks.contains(&pos) || st.markers_seen.contains(m)) {
                    ow.in_notes = true;
                    Role::Note
                } else if semantics("marginalia") || semantics("sidebar") || (block.tag == "aside" && !semantics("footnote")) || ((block.tag == "blockquote" || block.ancestors.iter().any(|a| a == "blockquote")) && text_len <= 160) {
                    Role::Sidenote
                } else {
                    Role::Body
                }
            }
        };
        let seq = paragraphs.len();
        let mut number = None;
        let mut attached = None;
        let mut marker = if role == Role::Note { leading_marker.clone() } else { None };
        match role {
            Role::Body => {
                st.body += 1;
                ow.has_body = true;
                number = Some(st.body);
                st.last_body = Some(seq);
                for r in &block.noterefs {
                    st.markers_seen.push(r.marker.clone());
                }
            }
            Role::Sidenote => attached = st.last_body,
            Role::Note => {
                // 先找带同一个引用标记的正文段
                // 没有标记的接着上一条注释（「译者按」）
                let m = marker.clone().or_else(|| paragraphs.iter().rev().take_while(|p: &&Paragraph| p.unit == nu && p.role == Role::Note).find_map(|p| p.marker.clone()));
                marker = m.clone();
                attached = m.as_ref().and_then(|m| paragraphs.iter().rev().filter(|p| p.unit == nu && p.role == Role::Body).find(|p| p.noterefs.iter().any(|(_, x)| x == m)).map(|p| p.seq)).or(st.last_body);
            }
            _ => {}
        }
        paragraphs.push(Paragraph {
            seq,
            unit: if matches!(role, Role::Guide | Role::Matter) || matches!(kind, UnitKind::Guide | UnitKind::Notes | UnitKind::Matter) { owner } else { nu },
            role,
            number,
            key: String::new(),
            attached,
            text: block.text.clone(),
            page: start_page,
            breaks,
            marker,
            noterefs: block.noterefs.iter().map(|r| (r.at, r.marker.clone())).collect(),
        });
    }
    let has_pages = paragraphs.iter().any(|p| p.page.is_some());
    let language = normalize_language(&book.language, &paragraphs);
    let mut text = Text {
        title: book.title.clone(),
        language,
        creators: book.creators.clone(),
        publisher: book.publisher.clone(),
        date: book.date.clone(),
        identifiers: book.identifiers.clone(),
        format: "epub".into(),
        parser: PARSER_VERSION.into(),
        numbering: Some(numbering),
        page_source: if has_pages { book.page_source.clone() } else { "none".into() },
        units,
        paragraphs,
        warnings,
    };
    text.renumber(numbering);
    text
}

fn leading_note_marker(text: &str) -> Option<String> {
    let t = text.trim_start();
    let first: String = t.chars().take(8).collect();
    // 只认方括号：「（1）熟练程度的提高」是列举，不是注释
    let close = first.find(['］', ']', '〕'])?;
    let head = &first[..close + first[close..].chars().next()?.len_utf8()];
    if !head.starts_with(['［', '[', '〔']) {
        return None;
    }
    epub::note_marker(head)
}

fn ancestors(units: &[Unit], i: usize) -> impl Iterator<Item = usize> + '_ {
    std::iter::successors(units[i].parent, move |p| units[*p].parent)
}

fn normalize_language(declared: &str, paragraphs: &[Paragraph]) -> String {
    let d = declared.trim().to_ascii_lowercase();
    let d = d.split(['-', '_']).next().unwrap_or("");
    match d {
        "zh" | "chi" | "zho" => return "zh".into(),
        "en" | "eng" => return "en".into(),
        "" | "und" => {}
        other => return other.to_string(),
    }
    let (mut cjk, mut all) = (0usize, 0usize);
    for p in paragraphs.iter().filter(|p| p.role == Role::Body).take(50) {
        for c in p.text.chars().filter(|c| c.is_alphabetic()) {
            all += 1;
            if is_cjk(c) {
                cjk += 1;
            }
        }
    }
    if all > 0 && cjk * 3 > all { "zh".into() } else { "en".into() }
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

impl Text {
    /// 按某种写法重新编段落号（解析后在预览里可以换）
    pub fn renumber(&mut self, numbering: Numbering) {
        self.numbering = Some(numbering);
        // 单元的引用名
        let mut part_no: HashMap<Option<u32>, u32> = HashMap::new();
        let keys: Vec<String> = (0..self.units.len())
            .map(|i| {
                let u = &self.units[i];
                match u.kind {
                    UnitKind::Chapter => match (numbering, u.book, u.chapter) {
                        (Numbering::Glasgow, Some(b), Some(c)) => format!("{}.{}", roman(b), roman(c).to_lowercase()),
                        (_, _, Some(c)) => c.to_string(),
                        _ => format!("U{}", i + 1),
                    },
                    UnitKind::Part | UnitKind::Book | UnitKind::Container => {
                        let base = if intro_like(&u.label) {
                            "Intro".to_string()
                        } else {
                            let n = part_no.entry(u.book).or_insert(0);
                            *n += 1;
                            format!("U{n}")
                        };
                        match (numbering, u.book, u.kind) {
                            (_, Some(b), UnitKind::Book) => roman(b),
                            (Numbering::Glasgow, Some(b), _) => format!("{}.{base}", roman(b)),
                            _ => base,
                        }
                    }
                    _ => short_label(&u.label),
                }
            })
            .collect();
        for (u, k) in self.units.iter_mut().zip(keys) {
            u.key = k;
        }
        let mut used: HashMap<String, u32> = HashMap::new();
        let mut unique = |k: String| -> String {
            let n = used.entry(k.clone()).or_insert(0);
            *n += 1;
            if *n == 1 { k } else { format!("{k} #{n}") }
        };
        let mut sidenotes: HashMap<usize, u32> = HashMap::new();
        let mut headings: HashMap<usize, u32> = HashMap::new();
        let mut others: HashMap<usize, u32> = HashMap::new();
        let mut note_parts: BTreeMap<(usize, String), u32> = BTreeMap::new();
        let mut keys = Vec::with_capacity(self.paragraphs.len());
        for i in 0..self.paragraphs.len() {
            let p = &self.paragraphs[i];
            let unit_key = &self.units[p.unit].key;
            let key = match p.role {
                Role::Body => match numbering {
                    Numbering::Plain => p.number.unwrap_or(0).to_string(),
                    _ => format!("{unit_key}.{}", p.number.unwrap_or(0)),
                },
                Role::Sidenote => {
                    let base = p.attached.map(|a| keys.get(a).cloned().unwrap_or_default()).unwrap_or_else(|| unit_key.clone());
                    let n = sidenotes.entry(p.attached.unwrap_or(usize::MAX - p.unit)).or_insert(0);
                    *n += 1;
                    if *n == 1 { format!("{base} 旁注") } else { format!("{base} 旁注 {n}") }
                }
                Role::Note if matches!(self.units[p.unit].kind, UnitKind::Chapter | UnitKind::Part | UnitKind::Book | UnitKind::Container | UnitKind::Section) => {
                    let marker = p.marker.clone().unwrap_or_else(|| "?".into());
                    let n = note_parts.entry((p.unit, marker.clone())).or_insert(0);
                    *n += 1;
                    let prefix = if numbering == Numbering::Plain { String::new() } else { format!("{unit_key} ") };
                    if *n == 1 { format!("{prefix}注 {marker}") } else { format!("{prefix}注 {marker}（{n}）") }
                }
                Role::Heading => {
                    let n = headings.entry(p.unit).or_insert(0);
                    *n += 1;
                    if *n == 1 { format!("{unit_key} 标题") } else { format!("{unit_key} 标题 {n}") }
                }
                _ => {
                    let n = others.entry(p.unit).or_insert(0);
                    *n += 1;
                    format!("{unit_key} ¶{n}")
                }
            };
            keys.push(unique(key));
        }
        for (p, k) in self.paragraphs.iter_mut().zip(keys) {
            p.key = k;
        }
    }

    pub fn paragraph(&self, key: &str) -> Option<&Paragraph> {
        let k = key.trim();
        self.paragraphs.iter().find(|p| p.key == k)
    }

    /// 编了段落号的单元（章和其他正文单元），按书里的顺序
    pub fn chapters(&self) -> Vec<&Unit> {
        let mut seen = Vec::new();
        for p in &self.paragraphs {
            if p.role == Role::Body && !seen.contains(&p.unit) {
                seen.push(p.unit);
            }
        }
        seen.into_iter().map(|u| &self.units[u]).collect()
    }

    pub fn body_of(&self, unit: usize) -> impl Iterator<Item = &Paragraph> {
        self.paragraphs.iter().filter(move |p| p.unit == unit && p.role == Role::Body)
    }

    /// 段内第 `at` 个字在哪一页
    pub fn page_at(&self, seq: usize, at: usize) -> Option<String> {
        let p = self.paragraphs.get(seq)?;
        let mut page = p.page.clone();
        for b in &p.breaks {
            if b.at <= at {
                page = Some(b.page.clone());
            }
        }
        page
    }

    /// 段内 `start..end` 这段文字的页码：同一页「110」，跨页「110–111」
    pub fn pages_of(&self, seq: usize, start: usize, end: usize) -> Option<String> {
        let a = self.page_at(seq, start)?;
        let b = self.page_at(seq, end.saturating_sub(1).max(start)).unwrap_or_else(|| a.clone());
        Some(if a == b { a } else { format!("{a}–{b}") })
    }

    /// 整段的页码
    pub fn paragraph_pages(&self, seq: usize) -> Option<String> {
        let len = self.paragraphs.get(seq)?.text.chars().count();
        self.pages_of(seq, 0, len)
    }
}

// ---------- 结构预览 ----------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCounts {
    pub body: usize,
    pub heading: usize,
    pub sidenote: usize,
    pub note: usize,
    pub guide: usize,
    pub matter: usize,
}

impl RoleCounts {
    fn add(&mut self, r: Role) {
        match r {
            Role::Body => self.body += 1,
            Role::Heading => self.heading += 1,
            Role::Sidenote => self.sidenote += 1,
            Role::Note => self.note += 1,
            Role::Guide => self.guide += 1,
            Role::Matter => self.matter += 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitPreview {
    pub index: usize,
    pub key: String,
    pub label: String,
    pub kind: UnitKind,
    pub depth: u32,
    pub counts: RoleCounts,
    /// 第一页、最后一页
    pub pages: Option<(String, String)>,
    /// 第一段正文的开头几个字（看认得对不对）
    pub first: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub title: String,
    pub language: String,
    pub format: String,
    pub numbering: Option<Numbering>,
    pub page_source: String,
    pub has_pages: bool,
    pub totals: RoleCounts,
    pub paragraphs: usize,
    pub chars: usize,
    pub units: Vec<UnitPreview>,
    pub warnings: Vec<String>,
}

pub fn preview(t: &Text) -> Preview {
    let mut totals = RoleCounts::default();
    let mut per: BTreeMap<usize, (RoleCounts, Option<String>, Option<String>, String)> = BTreeMap::new();
    for p in &t.paragraphs {
        totals.add(p.role);
        let e = per.entry(p.unit).or_default();
        e.0.add(p.role);
        if p.role == Role::Body {
            if e.1.is_none() {
                e.1 = p.page.clone();
            }
            if let Some(last) = t.paragraph_pages(p.seq) {
                e.2 = Some(last.rsplit('–').next().unwrap_or(&last).to_string());
            }
            if e.3.is_empty() {
                e.3 = p.text.chars().take(24).collect();
            }
        }
    }
    let units = per
        .into_iter()
        .map(|(u, (counts, first_page, last_page, first))| {
            let unit = &t.units[u];
            UnitPreview { index: u, key: unit.key.clone(), label: unit.label.clone(), kind: unit.kind, depth: unit.depth, counts, pages: first_page.zip(last_page), first }
        })
        .collect();
    Preview {
        title: t.title.clone(),
        language: t.language.clone(),
        format: t.format.clone(),
        numbering: t.numbering,
        page_source: t.page_source.clone(),
        has_pages: t.paragraphs.iter().any(|p| p.page.is_some()),
        totals,
        paragraphs: t.paragraphs.len(),
        chars: t.paragraphs.iter().map(|p| p.text.chars().count()).sum(),
        units,
        warnings: t.warnings.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_labels() {
        assert_eq!(roman(3), "III");
        assert_eq!(roman(14), "XIV");
        assert_eq!(parse_roman("xiv"), Some(14));
        assert_eq!(parse_roman("IIII"), None);
        assert_eq!(parse_english_number("twenty-one"), Some(21));
        assert_eq!(parse_chinese_number("十一"), Some(11));
        assert_eq!(parse_chinese_number("二十三"), Some(23));
        assert_eq!(parse_chinese_number("一百零五"), Some(105));
        assert_eq!(classify("Book One"), Label::Book(Some(1)));
        assert_eq!(classify("BOOK IV"), Label::Book(Some(4)));
        assert_eq!(classify("CHAPTER III"), Label::Chapter(Some(3)));
        assert_eq!(classify("Chapter 2. Of money"), Label::Chapter(Some(2)));
        assert_eq!(classify("第一编\u{3000}论劳动生产力"), Label::Book(Some(1)));
        assert_eq!(classify("第十一章\u{3000}论地租"), Label::Chapter(Some(11)));
        assert_eq!(classify("第一部分\u{3000}由于用途"), Label::Section);
        assert_eq!(classify("PART I"), Label::Section);
        assert_eq!(classify("〔前三章导读〕"), Label::Guide);
        assert_eq!(classify("〔第四章导读〕"), Label::Guide);
        assert_eq!(classify("译者导言"), Label::Guide);
        assert_eq!(classify("Analytical Introduction"), Label::Guide);
        assert_eq!(classify("Notes to the Introduction"), Label::Notes);
        assert_eq!(classify("Footnotes"), Label::Notes);
        assert_eq!(classify("Contents"), Label::Matter);
        assert_eq!(classify("丛书总序"), Label::Matter);
        assert_eq!(classify("关于译者"), Label::Matter);
        // 斯密自己的引言是正文
        assert_eq!(classify("Introduction and Plan of the Work"), Label::Other);
        assert_eq!(classify("引言和本书计划"), Label::Other);
        assert_eq!(leading_note_marker("［1］劳动分工一词").as_deref(), Some("1"));
        assert_eq!(leading_note_marker("[12] note").as_deref(), Some("12"));
        assert_eq!(leading_note_marker("第一，工人"), None);
        assert_eq!(leading_note_marker("（1）熟练程度的提高"), None);
    }
}
