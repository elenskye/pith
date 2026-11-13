//! 渲染（`decisions/0024`「导出」）：
//! - `document`：一次研究排成与格式无关的 `research_engine::render::Doc`；Markdown 用 v0.3 的 `markdown`，Word 在 App 一侧交给
//!   `research/docx.rs`（同一份 Doc，内容一致）；
//! - `html`：单文件 HTML（样式和脚本内联，不引外部资源）：卡片按种类筛、点关系图的节点跳到卡片、点锚点跳到原文段落并高亮引文。

use std::collections::BTreeMap;

use research_engine::render::{Block, Doc, Inline, Row, Section, html_escape as esc, text};

use crate::audit::Finding;
use crate::graph;
use crate::model::{Card, CardKind, Category, Run, Sentence, SourceOrigin, Status, ZhStatus};
use crate::prompts::{Books, TRANSLATION_PREFIX};

fn usd(v: f64) -> String {
    if v > 0.0 && v < 0.01 { format!("${v:.4}") } else { format!("${v:.2}") }
}

/// 锚点的写法：「I.i.3，第 110 页」「译:I.i 注 1」「S1」
pub fn anchor_label(s: &Sentence) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(p) = &s.paragraph {
        parts.push(p.clone());
    }
    if let Some(pages) = &s.check.pages {
        parts.push(format!("第 {pages} 页"));
    }
    if let Some(src) = &s.source {
        parts.push(src.clone());
    }
    (!parts.is_empty()).then(|| parts.join("，"))
}

/// 一句给读者看的样子
fn sentence_inlines(s: &Sentence, books: &Books) -> Vec<Inline> {
    let mut v = vec![Inline::Tag(s.category.label().into())];
    match s.category {
        Category::Original | Category::Others => v.push(text(format!("“{}”", s.quote.as_deref().unwrap_or(&s.text)))),
        _ => v.push(text(s.text.clone())),
    }
    if s.category == Category::Speculation
        && let Some(q) = &s.quote
    {
        v.push(text(format!("（前提：“{q}”）")));
    }
    if let Some(a) = anchor_label(s) {
        v.push(text(format!("（{a}）")));
    }
    if s.check.status == Status::Unanchored {
        v.push(Inline::Flag { text: "没挂上原文".into(), why: s.check.note.clone().unwrap_or_else(|| "解释没有挂到段落上".into()) });
    }
    if s.category == Category::Original {
        match s.check.zh_status {
            Some(ZhStatus::Verified) => v.push(text(format!(" 中译：“{}”（{}）", s.zh.clone().unwrap_or_default(), s.check.zh_paragraph.clone().unwrap_or_default()))),
            Some(ZhStatus::ParagraphOnly) => {
                if let (Some(zh), Some(key)) = (books.translation, &s.check.zh_paragraph)
                    && let Some(p) = zh.paragraph(key)
                {
                    v.push(text(format!(" 中译（整段，{key}）：{}", p.text)));
                }
            }
            Some(ZhStatus::Unaligned) => v.push(Inline::Tag("这一章没有对齐的中译".into())),
            None => {}
        }
    }
    v
}

fn scope_label(run: &Run) -> String {
    let Some(s) = &run.scope else { return String::new() };
    let paras = if s.paragraphs.len() > 4 { format!("{} … {}（{} 段）", s.paragraphs[0], s.paragraphs[s.paragraphs.len() - 1], s.paragraphs.len()) } else { s.paragraphs.join("、") };
    let mut parts = vec![];
    if let Some(u) = &s.unit {
        parts.push(format!("第 {u} 章"));
    } else if !paras.is_empty() {
        parts.push(paras);
    }
    if let Some(q) = &s.question {
        parts.push(format!("问题：{q}"));
    }
    if !s.terms.is_empty() {
        parts.push(format!("概念：{}", s.terms.join(" / ")));
    }
    if !run.targets.is_empty() {
        parts.push(format!("卡片：{}", run.targets.iter().map(|c| c.title.as_str()).collect::<Vec<_>>().join("、")));
    }
    if let Some(i) = &s.instruction {
        parts.push(format!("要求：{i}"));
    }
    parts.join("；")
}

pub fn title(run: &Run) -> String {
    let s = scope_label(run);
    if s.is_empty() { run.skill_title.clone() } else { format!("{}：{s}", run.skill_title) }
}

fn counts(run: &Run) -> String {
    let mut by: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &run.cards {
        *by.entry(c.kind.label()).or_insert(0) += 1;
    }
    let cards = CardKind::ALL.iter().filter_map(|k| by.get(k.label()).map(|n| format!("{} {n} 张", k.label()))).collect::<Vec<_>>().join("、");
    let sentences: Vec<&Sentence> = run.cards.iter().flat_map(|c| &c.sentences).collect();
    let n = |cat: Category| sentences.iter().filter(|s| s.category == cat).count();
    let unanchored = sentences.iter().filter(|s| s.check.status == Status::Unanchored).count();
    format!(
        "{}；关系 {} 条。原文 {} 句、他人 {} 句（都逐字核对通过）、解释 {} 句（其中 {unanchored} 句没挂上原文）、推演 {} 句；核对拦下 {} 句。",
        if cards.is_empty() { "没有卡片".to_string() } else { cards },
        run.relations.len(),
        n(Category::Original),
        n(Category::Others),
        n(Category::Explanation),
        n(Category::Speculation),
        run.rejected.len()
    )
}

/// 时间戳（UTC 的 ISO）→「2026-11-13 09:22 UTC」
fn when(t: &str) -> String {
    match t.get(..16) {
        Some(m) if t.ends_with('Z') => format!("{} UTC", m.replace('T', " ")),
        _ => t.replace('T', " "),
    }
}

/// 一次研究 → Doc（Markdown、Word 共用）
pub fn document(run: &Run, books: &Books, findings: &[Finding]) -> Doc {
    let byline = format!(
        "{} · {} · {}{}",
        if books.original.title.is_empty() { "原著" } else { &books.original.title },
        when(run.finished_at.as_deref().unwrap_or(&run.created_at)),
        if run.mock { "模拟模型".to_string() } else { run.model.model.clone() },
        if run.mock { "（示例数据）" } else { "" }
    );
    let mut lead = vec![Block::Para(vec![text(counts(run))])];
    if !findings.is_empty() {
        lead.push(Block::Bullets(findings.iter().map(|f| vec![Inline::Flag { text: format!("审计：{}{}", f.at, f.sentence.map(|n| format!(" 第 {n} 句")).unwrap_or_default()), why: f.message.clone() }]).collect()));
    }
    let mut sections = Vec::new();
    for kind in CardKind::ALL {
        let cards: Vec<&Card> = run.cards.iter().filter(|c| c.kind == kind).collect();
        if cards.is_empty() {
            continue;
        }
        let mut blocks = Vec::new();
        for c in cards {
            blocks.push(Block::Sub(format!("{} {}", c.id, c.title)));
            if !c.about.is_empty() {
                blocks.push(Block::Para(vec![text(format!("针对：{}", c.about.join("、")))]));
            }
            blocks.push(Block::Bullets(c.sentences.iter().map(|s| sentence_inlines(s, books)).collect()));
        }
        sections.push(Section { title: format!("{}（{} 张）", kind.label(), run.cards.iter().filter(|c| c.kind == kind).count()), blocks });
    }
    if !run.relations.is_empty() {
        let card_name = |id: &str| run.card(id).map(|c| format!("{id} {}", c.title)).unwrap_or_else(|| id.to_string());
        let rows = run.relations.iter().map(|r| Row { anchor: Some(r.id.clone()), cells: vec![vec![text(r.kind.label())], vec![text(card_name(&r.from))], vec![text(card_name(&r.to))], sentence_inlines(&r.note, books)] }).collect();
        sections.push(Section { title: "关系".into(), blocks: vec![Block::Table { head: ["关系", "从", "到", "说明"].map(String::from).to_vec(), rows }] });
    }
    // 卡片引到的原文段落
    let mut keys: Vec<String> = Vec::new();
    for s in run.cards.iter().flat_map(|c| &c.sentences) {
        if let Some(p) = &s.paragraph
            && !keys.contains(p)
            && matches!(s.category, Category::Original | Category::Explanation | Category::Speculation | Category::Others)
        {
            keys.push(p.clone());
        }
    }
    let mut quoted = Vec::new();
    for k in &keys {
        let (book, key) = match k.strip_prefix(TRANSLATION_PREFIX) {
            Some(rest) => (books.translation, rest),
            None => (Some(books.original), k.as_str()),
        };
        if let Some(p) = book.and_then(|b| b.paragraph(key)) {
            quoted.push(Block::Quote(format!("{k}：{}", p.text)));
        }
    }
    if !quoted.is_empty() {
        sections.push(Section { title: "引到的原文段落".into(), blocks: quoted });
    }
    if !run.sources.is_empty() {
        let rows = run
            .sources
            .iter()
            .map(|s| {
                let where_from = match (s.origin, &s.url) {
                    (SourceOrigin::Web, Some(u)) => Inline::Link { text: u.clone(), url: u.clone() },
                    _ => text("文献库"),
                };
                Row { anchor: Some(s.id.clone()), cells: vec![vec![text(s.id.clone())], vec![text(s.title.clone())], vec![where_from], vec![text(s.error.clone().unwrap_or_else(|| format!("{} 字", s.text_chars)))]] }
            })
            .collect();
        sections.push(Section { title: "评论的来源".into(), blocks: vec![Block::Table { head: ["编号", "标题", "出处", "正文"].map(String::from).to_vec(), rows }] });
    }
    if !run.rejected.is_empty() {
        let rows = run.rejected.iter().map(|r| Row { anchor: None, cells: vec![vec![text(r.card.clone())], vec![text(r.sentence.category.label())], vec![text(r.sentence.text.clone())], vec![Inline::Flag { text: "拦下".into(), why: r.reason.clone() }]] }).collect();
        sections.push(Section { title: "核对报告（拦下的句子，不在卡片里）".into(), blocks: vec![Block::Table { head: ["卡片", "类别", "句子", "原因"].map(String::from).to_vec(), rows }] });
    }
    let rows = run
        .steps
        .iter()
        .map(|s| {
            let usage = if s.input_tokens + s.output_tokens > 0 { format!("{} / {}", s.input_tokens, s.output_tokens) } else if s.searches > 0 { format!("搜索 {} 次", s.searches) } else { "—".into() };
            Row { anchor: None, cells: vec![vec![text(s.label.clone())], vec![text(if s.provider.is_empty() { "本机".to_string() } else { format!("{} {}", s.provider, s.model) })], vec![text(usage)], vec![text(if s.mock { "模拟".to_string() } else { usd(s.cost_usd) })], vec![text(s.note.clone().unwrap_or_default())]] }
        })
        .collect();
    sections.push(Section { title: format!("步骤与费用（合计 {}）", usd(run.cost_usd())), blocks: vec![Block::Table { head: ["步骤", "接口", "用量（输入 / 输出）", "费用", "备注"].map(String::from).to_vec(), rows }] });
    Doc { title: title(run), byline, lead, sections }
}

pub fn markdown(run: &Run, books: &Books, findings: &[Finding]) -> String {
    let doc = document(run, books, findings);
    let mut md = research_engine::render::markdown(&doc).replacen("\n\n## 短答\n\n", "\n\n## 概要\n\n", 1);
    let rels = &run.relations;
    if !rels.is_empty() {
        md.push_str("\n关系图见 HTML 导出。\n");
    }
    md
}

// ---------- 单文件 HTML ----------

const CSS: &str = "
:root{--bg:#fff;--fg:#1f2328;--muted:#5b6170;--line:#d9dce1;--card:#fafbfc;--accent:#2f6fd0;--flag:#c4352f;--mark:#fff1a8;--o:#2f6fd0;--t:#c98a2c;--e:#3f9654;--s:#8d5cc4}
@media (prefers-color-scheme:dark){:root{--bg:#16181d;--fg:#e6e8eb;--muted:#a9afb8;--line:#30343c;--card:#1d2026;--accent:#6ea2f0;--flag:#ef6b63;--mark:#5a4d12}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.65 -apple-system,'PingFang SC','Noto Sans CJK SC',sans-serif}
main{max-width:960px;margin:0 auto;padding:28px 16px 80px}h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:32px 0 10px;border-bottom:1px solid var(--line);padding-bottom:4px}h3{font-size:15px;margin:0 0 6px}
.byline,.muted{color:var(--muted);font-size:13px}.flag{color:var(--flag);font-weight:600}.banner{border:1px solid var(--flag);color:var(--flag);border-radius:8px;padding:8px 12px;margin:12px 0}
.filters button{font:inherit;font-size:13px;border:1px solid var(--line);background:var(--card);color:var(--fg);border-radius:999px;padding:2px 10px;margin:0 6px 6px 0;cursor:pointer}.filters button.on{border-color:var(--accent);color:var(--accent)}
.card{border:1px solid var(--line);background:var(--card);border-radius:10px;padding:12px 14px;margin:0 0 12px}.card.active{outline:2px solid var(--accent)}
.card ul{margin:0;padding-left:18px}.card li{margin:4px 0}.tag{display:inline-block;font-size:11px;border-radius:4px;padding:0 5px;margin-right:6px;color:#fff;vertical-align:1px}
.original .tag{background:var(--o)}.others .tag{background:var(--t)}.explanation .tag{background:var(--e)}.speculation .tag{background:var(--s)}.speculation{font-style:italic}
a.anchor{color:var(--accent);font-size:13px;margin-left:4px}.zh{color:var(--muted);font-size:14px}.unanchored{color:var(--flag);font-size:12px;font-weight:600;margin-left:6px}
.graph{overflow-x:auto;border:1px solid var(--line);border-radius:10px;padding:8px;background:var(--card)}
.para{padding:6px 10px;border-left:3px solid var(--line);margin:8px 0}.para.active{border-left-color:var(--accent)}.para b{color:var(--muted);font-weight:600;margin-right:6px}mark{background:var(--mark);color:inherit}mark.active{outline:2px solid var(--accent)}
table{border-collapse:collapse;width:100%;font-size:13px}th,td{border:1px solid var(--line);padding:4px 6px;text-align:left;vertical-align:top}
";

const JS: &str = "
(function(){
  function clear(sel){document.querySelectorAll(sel).forEach(function(e){e.classList.remove('active')});}
  function show(id){var el=document.getElementById(id);if(!el)return;clear('.card');clear('.node');el.classList.add('active');el.scrollIntoView({behavior:'smooth',block:'center'});
    var n=document.querySelector('.node[data-card=\"'+id.replace('card-','')+'\"]');if(n)n.classList.add('active');}
  document.querySelectorAll('.node').forEach(function(n){n.addEventListener('click',function(){show('card-'+n.getAttribute('data-card'));});
    n.addEventListener('keydown',function(e){if(e.key==='Enter')show('card-'+n.getAttribute('data-card'));});});
  document.querySelectorAll('a.anchor').forEach(function(a){a.addEventListener('click',function(ev){var p=document.getElementById(a.getAttribute('data-para'));if(!p)return;ev.preventDefault();
    clear('.para');clear('mark');p.classList.add('active');var m=a.getAttribute('data-mark')&&document.getElementById(a.getAttribute('data-mark'));if(m)m.classList.add('active');p.scrollIntoView({behavior:'smooth',block:'center'});});});
  document.querySelectorAll('.filters button').forEach(function(b){b.addEventListener('click',function(){var k=b.getAttribute('data-kind');
    document.querySelectorAll('.filters button').forEach(function(x){x.classList.toggle('on',x===b);});
    document.querySelectorAll('.card').forEach(function(c){c.style.display=(k==='all'||c.getAttribute('data-kind')===k)?'':'none';});});});
})();
";

fn para_id(key: &str) -> String {
    format!("p-{}", key.replace(' ', "_"))
}

/// 段落文字里给几段引文加 <mark>（按字符位置，重叠的合并）
fn marked(text: &str, spans: &[(usize, usize, String)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<(usize, usize, String)> = spans.iter().filter(|(a, b, _)| a < b && *b <= chars.len()).cloned().collect();
    spans.sort();
    let mut merged: Vec<(usize, usize, String)> = Vec::new();
    for s in spans {
        if let Some(last) = merged.last_mut()
            && s.0 < last.1
        {
            last.1 = last.1.max(s.1);
            continue;
        }
        merged.push(s);
    }
    let mut out = String::new();
    let mut at = 0;
    for (a, b, id) in merged {
        out.push_str(&esc(&chars[at..a].iter().collect::<String>()));
        out.push_str(&format!("<mark id=\"{}\">{}</mark>", esc(&id), esc(&chars[a..b].iter().collect::<String>())));
        at = b;
    }
    out.push_str(&esc(&chars[at..].iter().collect::<String>()));
    out
}

pub fn html(run: &Run, books: &Books, findings: &[Finding]) -> String {
    let title = title(run);
    let mut h = format!("<!doctype html>\n<html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{CSS}</style></head><body><main>", esc(&title));
    let doc = document(run, books, findings);
    h.push_str(&format!("<h1>{}</h1><p class=\"byline\">{}</p>", esc(&title), esc(&doc.byline)));
    if !findings.is_empty() {
        h.push_str("<div class=\"banner\"><b>审计发现问题：</b><ul>");
        for f in findings {
            h.push_str(&format!("<li>{}{}：{}</li>", esc(&f.at), f.sentence.map(|n| format!(" 第 {n} 句")).unwrap_or_default(), esc(&f.message)));
        }
        h.push_str("</ul></div>");
    }
    h.push_str(&format!("<p>{}</p>", esc(&counts(run))));
    let svg = graph::svg(&graph::layout(run));
    if !svg.is_empty() {
        h.push_str(&format!("<h2>关系图</h2><p class=\"muted\">点节点看卡片。</p><div class=\"graph\">{svg}</div>"));
    }
    // 卡片
    h.push_str("<h2>卡片</h2><div class=\"filters\"><button class=\"on\" data-kind=\"all\">全部</button>");
    for k in CardKind::ALL {
        let n = run.cards.iter().filter(|c| c.kind == k).count();
        if n > 0 {
            h.push_str(&format!("<button data-kind=\"{}\">{}（{n}）</button>", k.key(), k.label()));
        }
    }
    h.push_str("</div>");
    // 每段要标的引文
    let mut marks: BTreeMap<String, Vec<(usize, usize, String)>> = BTreeMap::new();
    for c in &run.cards {
        for (i, s) in c.sentences.iter().enumerate() {
            if let (Some(p), Some(span)) = (&s.paragraph, s.check.span)
                && s.source.is_none()
            {
                marks.entry(p.clone()).or_default().push((span.start, span.end, format!("m-{}-{}", c.id, i + 1)));
            }
        }
    }
    for c in &run.cards {
        h.push_str(&format!("<article class=\"card\" id=\"card-{}\" data-kind=\"{}\"><h3>{} · {} <span class=\"muted\">{}</span></h3>", esc(&c.id), c.kind.key(), esc(&c.id), esc(&c.title), c.kind.label()));
        if !c.about.is_empty() {
            h.push_str(&format!("<p class=\"muted\">针对：{}</p>", c.about.iter().map(|a| format!("<a href=\"#card-{0}\">{0}</a>", esc(a))).collect::<Vec<_>>().join("、")));
        }
        h.push_str("<ul>");
        for (i, s) in c.sentences.iter().enumerate() {
            h.push_str(&format!("<li class=\"{}\"><span class=\"tag\">{}</span>", s.category.key(), s.category.label()));
            let shown = match s.category {
                Category::Original | Category::Others => format!("“{}”", s.quote.as_deref().unwrap_or(&s.text)),
                _ => s.text.clone(),
            };
            h.push_str(&esc(&shown));
            if s.category == Category::Speculation
                && let Some(q) = &s.quote
            {
                h.push_str(&format!("<span class=\"muted\">（前提：“{}”）</span>", esc(q)));
            }
            if let Some(a) = anchor_label(s) {
                match &s.paragraph {
                    Some(p) if s.source.is_none() => h.push_str(&format!("<a class=\"anchor\" href=\"#{}\" data-para=\"{}\" data-mark=\"m-{}-{}\">{}</a>", para_id(p), para_id(p), esc(&c.id), i + 1, esc(&a))),
                    _ => h.push_str(&format!("<a class=\"anchor\" href=\"#src-{}\">{}</a>", esc(s.source.as_deref().unwrap_or("")), esc(&a))),
                }
            }
            if s.check.status == Status::Unanchored {
                h.push_str(&format!("<span class=\"unanchored\" title=\"{}\">没挂上原文</span>", esc(s.check.note.as_deref().unwrap_or(""))));
            }
            if s.category == Category::Original {
                match s.check.zh_status {
                    Some(ZhStatus::Verified) => h.push_str(&format!("<div class=\"zh\">中译：“{}”（{}）</div>", esc(s.zh.as_deref().unwrap_or("")), esc(s.check.zh_paragraph.as_deref().unwrap_or("")))),
                    Some(ZhStatus::ParagraphOnly) => {
                        if let (Some(zh), Some(key)) = (books.translation, &s.check.zh_paragraph)
                            && let Some(p) = zh.paragraph(key)
                        {
                            h.push_str(&format!("<div class=\"zh\">中译（整段，{}）：{}</div>", esc(key), esc(&p.text)));
                        }
                    }
                    Some(ZhStatus::Unaligned) => h.push_str("<div class=\"zh\">这一章中英段数不同，没有对齐的中译</div>"),
                    None => {}
                }
            }
            h.push_str("</li>");
        }
        h.push_str("</ul></article>");
    }
    if !run.relations.is_empty() {
        h.push_str("<h2>关系</h2><table><tr><th>关系</th><th>从</th><th>到</th><th>说明</th></tr>");
        for r in &run.relations {
            h.push_str(&format!(
                "<tr id=\"rel-{id}\"><td>{kind}</td><td><a href=\"#card-{from}\">{from}</a></td><td><a href=\"#card-{to}\">{to}</a></td><td class=\"{cat}\"><span class=\"tag\">{label}</span>{note}</td></tr>",
                id = esc(&r.id),
                kind = esc(r.kind.label()),
                from = esc(&r.from),
                to = esc(&r.to),
                cat = r.note.category.key(),
                label = r.note.category.label(),
                note = esc(&r.note.text)
            ));
        }
        h.push_str("</table>");
    }
    if !marks.is_empty() {
        h.push_str("<h2>引到的原文段落</h2><p class=\"muted\">点卡片里的锚点跳到这里，引文高亮。</p>");
        for (key, spans) in &marks {
            let (book, k) = match key.strip_prefix(TRANSLATION_PREFIX) {
                Some(rest) => (books.translation, rest),
                None => (Some(books.original), key.as_str()),
            };
            if let Some(p) = book.and_then(|b| b.paragraph(k)) {
                let pages = if book.map(|b| std::ptr::eq(b, books.original)).unwrap_or(false) { books.original.paragraph_pages(p.seq).map(|x| format!("（第 {x} 页）")).unwrap_or_default() } else { String::new() };
                h.push_str(&format!("<div class=\"para\" id=\"{}\"><b>{}{}</b>{}</div>", para_id(key), esc(key), esc(&pages), marked(&p.text, spans)));
            }
        }
    }
    if !run.sources.is_empty() {
        h.push_str("<h2>评论的来源</h2><ul>");
        for s in &run.sources {
            let link = match (&s.origin, &s.url) {
                (SourceOrigin::Web, Some(u)) => format!(" <a href=\"{}\">{}</a>", esc(u), esc(u)),
                _ => " <span class=\"muted\">文献库</span>".into(),
            };
            h.push_str(&format!("<li id=\"src-{}\"><b>{}</b> {}{}{}</li>", esc(&s.id), esc(&s.id), esc(&s.title), link, s.error.as_deref().map(|e| format!(" <span class=\"flag\">{}</span>", esc(e))).unwrap_or_default()));
        }
        h.push_str("</ul>");
    }
    if !run.rejected.is_empty() {
        h.push_str("<h2>核对报告</h2><p class=\"muted\">这些句子核不上，没有放进卡片。</p><table><tr><th>卡片</th><th>类别</th><th>句子</th><th>原因</th></tr>");
        for r in &run.rejected {
            h.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"flag\">{}</td></tr>", esc(&r.card), r.sentence.category.label(), esc(&r.sentence.text), esc(&r.reason)));
        }
        h.push_str("</table>");
    }
    h.push_str(&format!("<h2>步骤与费用</h2><p class=\"muted\">合计 {}{}</p><table><tr><th>步骤</th><th>接口</th><th>用量</th><th>费用</th><th>备注</th></tr>", usd(run.cost_usd()), if run.mock { "（模拟，不花钱）" } else { "" }));
    for s in &run.steps {
        h.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&s.label),
            esc(&if s.provider.is_empty() { "本机".to_string() } else { format!("{} {}", s.provider, s.model) }),
            if s.input_tokens + s.output_tokens > 0 { format!("{} / {}", s.input_tokens, s.output_tokens) } else { "—".into() },
            if s.mock { "模拟".to_string() } else { usd(s.cost_usd) },
            esc(s.note.as_deref().unwrap_or(""))
        ));
    }
    h.push_str(&format!("</table></main><script>{JS}</script></body></html>\n"));
    h
}
