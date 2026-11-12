//! 报告的渲染：先把 `Report` 排成一份与格式无关的文档（`Doc`），再出 Markdown 和单文件 HTML。
//! Word 导出在 `src-tauri` 里用同一份 `Doc` 交给文档引擎，三种导出的内容因此一致。
//!
//! HTML 是一个自带样式的文件：不引用任何外部资源（样式表、字体、图片、脚本地址），离线能看。

use crate::model::{Angle, Segment, Uncertainty};
use crate::report::Report;

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Strong(String),
    /// 指向文档里的一处（事实 F3、计算 C1）
    Ref { text: String, target: String },
    Link { text: String, url: String },
    /// 对不上的数：醒目地标出来
    Flag { text: String, why: String },
    /// 小标签（不确定的类型、可信度）
    Tag(String),
}

pub fn text(s: impl Into<String>) -> Inline {
    Inline::Text(s.into())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// 这一行的锚点（F3、S2、C1），别处可以指过来
    pub anchor: Option<String>,
    pub cells: Vec<Vec<Inline>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Vec<Inline>),
    Bullets(Vec<Vec<Inline>>),
    Numbered(Vec<Vec<Inline>>),
    Table { head: Vec<String>, rows: Vec<Row> },
    /// 原文摘录
    Quote(String),
    Sub(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub title: String,
    /// 标题下面的一行说明（时间、模型）
    pub byline: String,
    /// 短答：一直展开
    pub lead: Vec<Block>,
    /// 其余部分：HTML 里折叠，点开才看
    pub sections: Vec<Section>,
}

fn usd(v: f64) -> String {
    if v > 0.0 && v < 0.01 { format!("${v:.4}") } else { format!("${v:.2}") }
}

fn web_link(title: &str, url: &str) -> Inline {
    let label = if title.trim().is_empty() { url } else { title.trim() };
    if url.starts_with("https://") || url.starts_with("http://") { Inline::Link { text: label.to_string(), url: url.to_string() } } else { Inline::Text(label.to_string()) }
}

fn conclusion_inlines(segments: &[Segment]) -> Vec<Inline> {
    segments
        .iter()
        .map(|s| match s {
            Segment::Text { text } => Inline::Text(text.clone()),
            Segment::Fact { id, text } | Segment::Calc { id, text } => Inline::Ref { text: text.clone(), target: id.clone() },
            Segment::Unmatched { text, why } => Inline::Flag { text: text.clone(), why: why.clone() },
        })
        .collect()
}

pub fn document(r: &Report) -> Doc {
    let when = r.finished_at.clone().unwrap_or_else(|| r.created_at.clone());
    let byline = format!("第 {} 次调查 · {} · 计划与结论：{} · 摘证据：{} · 检索：{}", r.seq, when.replace('T', " ").chars().take(16).collect::<String>(), r.planner_model, r.worker_model, r.search_engine);

    // ---- 短答 ----
    let mut lead: Vec<Block> = Vec::new();
    if r.conclusions.is_empty() {
        lead.push(Block::Para(vec![text("这次调查没有得出结论。")]));
    } else {
        let items = r
            .conclusions
            .iter()
            .map(|k| {
                let mut line = conclusion_inlines(&k.segments);
                for u in &k.uncertainty {
                    line.push(text(" "));
                    line.push(Inline::Tag(Uncertainty::label(*u).to_string()));
                }
                if !k.note.is_empty() {
                    line.push(text(format!("（{}）", k.note)));
                }
                if !k.facts.is_empty() {
                    line.push(text(" 依据："));
                    for (i, id) in k.facts.iter().enumerate() {
                        if i > 0 {
                            line.push(text("、"));
                        }
                        line.push(Inline::Ref { text: id.clone(), target: id.clone() });
                    }
                }
                line
            })
            .collect();
        lead.push(Block::Numbered(items));
    }
    if !r.gaps.is_empty() {
        lead.push(Block::Sub("没有查到的".into()));
        lead.push(Block::Bullets(r.gaps.iter().map(|g| vec![text(g)]).collect()));
    }
    if !r.notes.is_empty() {
        lead.push(Block::Sub("读之前要知道".into()));
        lead.push(Block::Bullets(r.notes.iter().map(|n| vec![text(n)]).collect()));
    }

    let mut sections: Vec<Section> = Vec::new();

    // ---- 和上次比 ----
    if let Some(d) = &r.diff {
        let mut blocks = vec![Block::Para(vec![text(format!("上一次调查：{}。", d.previous_at.replace('T', " ").chars().take(16).collect::<String>()))])];
        if d.nothing_changed() {
            blocks.push(Block::Para(vec![text(format!("两次的数字、来源和结论都一样（{} 条事实对得上）。", d.unchanged))]));
        }
        if !d.changed.is_empty() {
            blocks.push(Block::Sub("变了的数字".into()));
            let rows = d
                .changed
                .iter()
                .map(|c| Row { anchor: None, cells: vec![vec![text(&c.new.metric)], vec![text(&c.new.region)], vec![text(&c.new.population)], vec![text(&c.new.period)], vec![text(&c.old.value)], vec![Inline::Ref { text: c.new.value.clone(), target: c.new.id.clone() }]] })
                .collect();
            blocks.push(Block::Table { head: ["指标", "地区", "人群", "时间", "上次", "这次"].map(String::from).to_vec(), rows });
        }
        let brief_rows = |list: &[crate::diff::FactBrief], link: bool| -> Vec<Row> {
            list.iter()
                .map(|f| Row {
                    anchor: None,
                    cells: vec![vec![text(&f.metric)], vec![text(&f.region)], vec![text(&f.population)], vec![text(&f.period)], vec![if link { Inline::Ref { text: f.value.clone(), target: f.id.clone() } } else { text(&f.value) }], vec![text(&f.producer)]],
                })
                .collect()
        };
        let head = || ["指标", "地区", "人群", "时间", "数值", "数据生产者"].map(String::from).to_vec();
        if !d.added.is_empty() {
            blocks.push(Block::Sub("这次新查到的".into()));
            blocks.push(Block::Table { head: head(), rows: brief_rows(&d.added, true) });
        }
        if !d.removed.is_empty() {
            blocks.push(Block::Sub("上次有、这次没查到的".into()));
            blocks.push(Block::Table { head: head(), rows: brief_rows(&d.removed, false) });
        }
        if !d.sources_added.is_empty() {
            blocks.push(Block::Sub("新增的来源".into()));
            blocks.push(Block::Bullets(d.sources_added.iter().map(|s| vec![web_link(&s.title, &s.url)]).collect()));
        }
        if !d.sources_removed.is_empty() {
            blocks.push(Block::Sub("消失的来源".into()));
            blocks.push(Block::Bullets(d.sources_removed.iter().map(|s| vec![web_link(&s.title, &s.url)]).collect()));
        }
        if !d.conclusions_added.is_empty() || !d.conclusions_removed.is_empty() {
            blocks.push(Block::Sub("变了的结论".into()));
            let mut items: Vec<Vec<Inline>> = d.conclusions_removed.iter().map(|t| vec![Inline::Strong("上次：".into()), text(t)]).collect();
            items.extend(d.conclusions_added.iter().map(|t| vec![Inline::Strong("这次：".into()), text(t)]));
            blocks.push(Block::Bullets(items));
        }
        sections.push(Section { title: "和上一次调查的对比".into(), blocks });
    }

    // ---- 子问题 ----
    let mut blocks = Vec::new();
    if !r.summary.is_empty() {
        blocks.push(Block::Para(vec![text(&r.summary)]));
    }
    let rows = r
        .sub_questions
        .iter()
        .filter(|s| s.enabled)
        .map(|s| Row {
            anchor: Some(s.id.clone()),
            cells: vec![vec![text(&s.id)], vec![text(&s.text)], vec![text(if s.angle == Angle::Alternative { "官方统计之外" } else { "官方统计" })], vec![text(&s.metric)], vec![text(&s.definition)], vec![text([s.region.as_str(), s.population.as_str(), s.period.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join("；"))]],
        })
        .collect();
    blocks.push(Block::Table { head: ["编号", "子问题", "角度", "指标", "口径", "地区；人群；时间"].map(String::from).to_vec(), rows });
    if !r.dimensions.is_empty() {
        blocks.push(Block::Sub("官方统计之外的衡量维度".into()));
        blocks.push(Block::Bullets(r.dimensions.iter().map(|d| vec![Inline::Strong(d.name.clone()), text(format!("：{}{}", d.why, if d.sources.is_empty() { String::new() } else { format!("（{}）", d.sources) }))]).collect()));
    }
    if !r.caveats.is_empty() {
        blocks.push(Block::Sub("比较时要小心的地方".into()));
        blocks.push(Block::Bullets(r.caveats.iter().map(|c| vec![text(c)]).collect()));
    }
    sections.push(Section { title: format!("子问题（{} 个）", r.totals.sub_questions), blocks });

    // ---- 搜索 ----
    let rows = r
        .searches
        .iter()
        .map(|s| Row {
            anchor: None,
            cells: vec![vec![text(if s.sub_question.is_empty() { "计划时" } else { s.sub_question.as_str() })], vec![text(&s.query)], vec![text(&s.engine)], vec![text(match &s.error {
                Some(e) => format!("出错：{e}"),
                None => format!("{} 条", s.hits.len()),
            })]],
        })
        .collect();
    sections.push(Section { title: format!("搜索（{} 次）", r.searches.len()), blocks: vec![Block::Table { head: ["子问题", "搜索词", "引擎", "结果"].map(String::from).to_vec(), rows }] });

    // ---- 来源 ----
    let mut blocks = vec![Block::Para(vec![text(format!("按可信度从高到低排，用的是「{}」。可信度看的是数据生产者（谁统计的），不是谁转述的。", r.table_name))])];
    let rows = r
        .sources
        .iter()
        .map(|s| {
            let src = &s.source;
            let status = match s.state {
                "failed" => format!("下载失败：{}", src.fetch_error.clone().unwrap_or_default()),
                "no_text" => src.text_error.clone().unwrap_or_default(),
                "pending" => "还没有下载".to_string(),
                _ => format!("{} 条事实，{} 条核实{}", s.facts, s.verified, if src.truncated { "；正文没读完" } else { "" }),
            };
            let snapshot = match (&src.fetched_at, &src.sha256) {
                (Some(at), Some(sha)) => format!("{} · SHA-256 {}…", at.replace('T', " ").chars().take(16).collect::<String>(), sha.chars().take(12).collect::<String>()),
                _ => String::new(),
            };
            Row { anchor: Some(src.id.clone()), cells: vec![vec![text(&src.id)], vec![text(s.tier_label.unwrap_or("—"))], vec![web_link(&src.title, src.final_url.as_deref().unwrap_or(&src.url))], vec![text(s.producers.join("、"))], vec![text(status)], vec![text(snapshot)]] }
        })
        .collect();
    blocks.push(Block::Table { head: ["编号", "可信度", "来源", "数据生产者", "读到的", "快照"].map(String::from).to_vec(), rows });
    blocks.push(Block::Sub(format!("可信度等级表：{}", r.table_name)));
    blocks.push(Block::Bullets(r.table.iter().map(|t| vec![Inline::Strong(format!("{}：", t.label)), text(t.kinds.join("、"))]).collect()));
    if !r.table_changes.is_empty() {
        blocks.push(Block::Para(vec![text(format!("和默认表不同：{}。", r.table_changes.join("；")))]));
    }
    sections.push(Section { title: format!("来源（{} 个，{} 个可读）", r.totals.sources, r.totals.readable), blocks });

    // ---- 事实表 ----
    let rows = r
        .facts
        .iter()
        .map(|f| {
            let x = &f.fact;
            let status = if x.verified() { "已核实".to_string() } else { format!("未核实：{}", x.check.reason.clone().unwrap_or_default()) };
            let who = format!("{}（{}{}）", x.producer, f.producer_kind_label, if x.relayed { "，转述" } else { "" });
            Row {
                anchor: Some(x.id.clone()),
                cells: vec![
                    vec![text(&x.id)],
                    vec![text(&x.metric)],
                    vec![Inline::Strong(x.shown())],
                    vec![text([x.region.as_str(), x.population.as_str(), x.period.as_str(), x.definition.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("；"))],
                    vec![text(who)],
                    vec![text(f.tier_label)],
                    vec![text(status)],
                    vec![text(format!("「{}」", x.quote)), text(" "), Inline::Ref { text: x.source.clone(), target: x.source.clone() }],
                ],
            }
        })
        .collect();
    sections.push(Section {
        title: format!("事实表（{} 条，{} 条核实）", r.totals.facts, r.totals.verified),
        blocks: vec![
            Block::Para(vec![text("每条事实的摘录都由程序在保存下来的原文里逐字找过，数值和单位也在摘录里对过。没核上的不进结论。")]),
            Block::Table { head: ["编号", "指标", "数值", "地区；人群；时间；口径", "数据生产者", "可信度", "核对", "原文摘录"].map(String::from).to_vec(), rows },
        ],
    });

    // ---- 计算 ----
    if !r.calcs.is_empty() {
        let rows = r
            .calcs
            .iter()
            .map(|c| Row { anchor: Some(c.id.clone()), cells: vec![vec![text(&c.id)], vec![text(&c.label)], vec![text(match &c.error {
                Some(e) => format!("算不了：{e}"),
                None => c.steps.clone(),
            })]] })
            .collect();
        sections.push(Section { title: format!("计算过程（{} 个）", r.calcs.len()), blocks: vec![Block::Para(vec![text("计算由程序执行，不由模型口算。")]), Block::Table { head: ["编号", "算的是什么", "算式"].map(String::from).to_vec(), rows }] });
    }

    // ---- 过程与费用 ----
    let rows = r
        .steps
        .iter()
        .map(|s| {
            let who = if s.provider.is_empty() { "本机".to_string() } else if s.model.is_empty() { s.provider.clone() } else { format!("{} · {}", s.provider, s.model) };
            let used = if s.input_tokens + s.output_tokens > 0 { format!("{} / {} token", s.input_tokens, s.output_tokens) } else if s.searches > 0 { format!("{} 次", s.searches) } else { String::new() };
            let cost = if s.provider.is_empty() { String::new() } else if s.mock { "模拟".to_string() } else { usd(s.cost_usd) };
            Row { anchor: None, cells: vec![vec![text(&s.label)], vec![text(who)], vec![text(used)], vec![text(cost)], vec![text(if s.ok { s.note.clone().unwrap_or_default() } else { format!("失败：{}", s.note.clone().unwrap_or_default()) })]] }
        })
        .collect();
    sections.push(Section {
        title: format!("过程与费用（{} 次调用，{}）", r.totals.calls, if r.mock { "模拟，没有花钱".to_string() } else { usd(r.totals.cost_usd) }),
        blocks: vec![Block::Table { head: ["步骤", "接口", "用量（输入 / 输出）", "费用", "备注"].map(String::from).to_vec(), rows }],
    });

    Doc { title: r.question.clone(), byline, lead, sections }
}

// ---------- Markdown ----------

fn md_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|").replace('\n', " ").replace('*', "\\*").replace('_', "\\_").replace('`', "\\`").replace('<', "&lt;")
}

fn md_inlines(list: &[Inline]) -> String {
    list.iter()
        .map(|i| match i {
            Inline::Text(t) => md_escape(t),
            Inline::Strong(t) => format!("**{}**", md_escape(t)),
            // 自己指自己的（「依据：F3」）不重复写编号
            Inline::Ref { text, target } if text == target => md_escape(text),
            Inline::Ref { text, target } => format!("**{}**〔{}〕", md_escape(text), target),
            Inline::Link { text, url } => format!("[{}]({})", md_escape(text).replace('[', "\\[").replace(']', "\\]"), url.replace(' ', "%20").replace(')', "%29")),
            Inline::Flag { text, why } => format!("**⚠ {}**（{}）", md_escape(text), md_escape(why)),
            Inline::Tag(t) => format!("〔{}〕", md_escape(t)),
        })
        .collect()
}

fn md_blocks(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Para(i) => out.push_str(&format!("{}\n\n", md_inlines(i))),
            Block::Bullets(items) => {
                for i in items {
                    out.push_str(&format!("- {}\n", md_inlines(i)));
                }
                out.push('\n');
            }
            Block::Numbered(items) => {
                for (n, i) in items.iter().enumerate() {
                    out.push_str(&format!("{}. {}\n", n + 1, md_inlines(i)));
                }
                out.push('\n');
            }
            Block::Table { head, rows } => {
                out.push_str(&format!("| {} |\n| {} |\n", head.iter().map(|h| md_escape(h)).collect::<Vec<_>>().join(" | "), head.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")));
                for r in rows {
                    out.push_str(&format!("| {} |\n", r.cells.iter().map(|c| md_inlines(c)).collect::<Vec<_>>().join(" | ")));
                }
                out.push('\n');
            }
            Block::Quote(q) => out.push_str(&format!("> {}\n\n", md_escape(q))),
            Block::Sub(t) => out.push_str(&format!("### {}\n\n", md_escape(t))),
        }
    }
}

pub fn markdown(doc: &Doc) -> String {
    let mut out = format!("# {}\n\n{}\n\n## 短答\n\n", md_escape(&doc.title), md_escape(&doc.byline));
    md_blocks(&doc.lead, &mut out);
    for s in &doc.sections {
        out.push_str(&format!("## {}\n\n", md_escape(&s.title)));
        md_blocks(&s.blocks, &mut out);
    }
    out.trim_end().to_string() + "\n"
}

// ---------- HTML ----------

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn html_inlines(list: &[Inline]) -> String {
    list.iter()
        .map(|i| match i {
            Inline::Text(t) => html_escape(t),
            Inline::Strong(t) => format!("<strong>{}</strong>", html_escape(t)),
            Inline::Ref { text, target } => format!("<a class=\"ref\" href=\"#{}\">{}</a>", html_escape(target), html_escape(text)),
            Inline::Link { text, url } => format!("<a href=\"{}\" rel=\"noopener noreferrer\">{}</a>", html_escape(url), html_escape(text)),
            Inline::Flag { text, why } => format!("<mark title=\"{}\">{}</mark>", html_escape(why), html_escape(text)),
            Inline::Tag(t) => format!("<span class=\"tag\">{}</span>", html_escape(t)),
        })
        .collect()
}

fn html_blocks(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Para(i) => out.push_str(&format!("<p>{}</p>\n", html_inlines(i))),
            Block::Bullets(items) => {
                out.push_str("<ul>\n");
                for i in items {
                    out.push_str(&format!("<li>{}</li>\n", html_inlines(i)));
                }
                out.push_str("</ul>\n");
            }
            Block::Numbered(items) => {
                out.push_str("<ol>\n");
                for i in items {
                    out.push_str(&format!("<li>{}</li>\n", html_inlines(i)));
                }
                out.push_str("</ol>\n");
            }
            Block::Table { head, rows } => {
                out.push_str("<div class=\"scroll\"><table>\n<thead><tr>");
                for h in head {
                    out.push_str(&format!("<th>{}</th>", html_escape(h)));
                }
                out.push_str("</tr></thead>\n<tbody>\n");
                for r in rows {
                    match &r.anchor {
                        Some(a) => out.push_str(&format!("<tr id=\"{}\">", html_escape(a))),
                        None => out.push_str("<tr>"),
                    }
                    for c in &r.cells {
                        out.push_str(&format!("<td>{}</td>", html_inlines(c)));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</tbody>\n</table></div>\n");
            }
            Block::Quote(q) => out.push_str(&format!("<blockquote>{}</blockquote>\n", html_escape(q))),
            Block::Sub(t) => out.push_str(&format!("<h3>{}</h3>\n", html_escape(t))),
        }
    }
}

const STYLE: &str = "body{font:15px/1.7 -apple-system,BlinkMacSystemFont,'PingFang SC','Hiragino Sans GB','Microsoft YaHei',sans-serif;color:#1f2328;background:#fff;max-width:60rem;margin:2rem auto;padding:0 1.25rem}\
h1{font-size:1.5rem;line-height:1.35;margin:0 0 .25rem}h2{font-size:1.1rem;margin:1.5rem 0 .5rem}h3{font-size:.95rem;margin:1.1rem 0 .4rem}\
.byline{color:#656d76;font-size:.85rem;margin:0 0 1.25rem}\
ol,ul{padding-left:1.4rem}li{margin:.35rem 0}\
details{border:1px solid #d0d7de;border-radius:6px;margin:.6rem 0;padding:0 .9rem}summary{cursor:pointer;font-weight:600;padding:.55rem 0}details[open]>summary{border-bottom:1px solid #d0d7de;margin-bottom:.6rem}\
.scroll{overflow-x:auto}table{border-collapse:collapse;width:100%;font-size:.85rem;margin:.4rem 0 .9rem}th,td{border-bottom:1px solid #d8dee4;padding:.35rem .6rem .35rem 0;text-align:left;vertical-align:top}th{color:#656d76;font-weight:500;white-space:nowrap}\
tr:target{background:#fff8c5}a{color:#0969da;text-decoration:none}a:hover{text-decoration:underline}a.ref{font-weight:600}\
mark{background:#ffebe9;color:#cf222e;padding:0 .2em;border-radius:3px}.tag{display:inline-block;font-size:.75rem;background:#fff8c5;color:#7d4e00;border-radius:3px;padding:0 .4em;white-space:nowrap}\
blockquote{margin:.5rem 0;padding:.3rem .8rem;border-left:3px solid #d0d7de;color:#57606a}\
@media (prefers-color-scheme:dark){body{background:#0d1117;color:#e6edf3}.byline,th{color:#8b949e}details,details[open]>summary{border-color:#30363d}th,td{border-color:#21262d}tr:target{background:#3b2f00}a{color:#58a6ff}mark{background:#5a1e1e;color:#ffa198}.tag{background:#3b2f00;color:#e3b341}blockquote{border-color:#30363d;color:#8b949e}}\
@media print{details{border:0;padding:0}details>summary{list-style:none}}";

/// 点文中的数字时：先把它所在的那一节展开，再滚过去（浏览器对折叠起来的锚点不一定自己展开）
const SCRIPT: &str = "document.addEventListener('click',function(e){var a=e.target.closest&&e.target.closest('a.ref');if(!a)return;var t=document.getElementById(decodeURIComponent(a.getAttribute('href').slice(1)));if(!t)return;var d=t.closest('details');if(d)d.open=true;});window.addEventListener('beforeprint',function(){document.querySelectorAll('details').forEach(function(d){d.open=true})});";

pub fn html(doc: &Doc) -> String {
    let mut out = format!(
        "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<meta name=\"generator\" content=\"Pith\">\n<title>{title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<h1>{title}</h1>\n<p class=\"byline\">{byline}</p>\n<h2>短答</h2>\n",
        title = html_escape(&doc.title),
        byline = html_escape(&doc.byline),
    );
    html_blocks(&doc.lead, &mut out);
    out.push_str("<h2>全过程</h2>\n");
    for s in &doc.sections {
        out.push_str(&format!("<details>\n<summary>{}</summary>\n", html_escape(&s.title)));
        html_blocks(&s.blocks, &mut out);
        out.push_str("</details>\n");
    }
    out.push_str(&format!("<script>{SCRIPT}</script>\n</body>\n</html>\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Doc {
        Doc {
            title: "示例市 <青年> 失业率 & 工资？".into(),
            byline: "第 1 次调查".into(),
            lead: vec![Block::Numbered(vec![vec![
                text("失业率为 "),
                Inline::Ref { text: "12.3%".into(), target: "F1".into() },
                text("，全国为 "),
                Inline::Flag { text: "17.6%".into(), why: "对不上".into() },
                text(" "),
                Inline::Tag("互相矛盾".into()),
            ]])],
            sections: vec![Section {
                title: "事实表".into(),
                blocks: vec![Block::Table {
                    head: vec!["编号".into(), "来源".into()],
                    rows: vec![Row { anchor: Some("F1".into()), cells: vec![vec![text("F1")], vec![Inline::Link { text: "统计|公报".into(), url: "https://stats.example.invalid/a b".into() }]] }],
                }],
            }],
        }
    }

    #[test]
    fn markdown_has_the_short_answer_first() {
        let md = markdown(&doc());
        assert!(md.starts_with("# 示例市 &lt;青年> 失业率 & 工资？\n\n第 1 次调查\n\n## 短答\n\n1. 失业率为 **12.3%**〔F1〕，全国为 **⚠ 17.6%**（对不上） 〔互相矛盾〕\n"), "{md}");
        assert!(md.contains("## 事实表\n\n| 编号 | 来源 |\n| --- | --- |\n| F1 | [统计\\|公报](https://stats.example.invalid/a%20b) |\n"));
    }

    #[test]
    fn html_is_one_self_contained_file() {
        let h = html(&doc());
        assert!(h.starts_with("<!doctype html>") && h.trim_end().ends_with("</html>"));
        assert!(h.contains("<title>示例市 &lt;青年&gt; 失业率 &amp; 工资？</title>"));
        // 数字可以点回事实表里的那一行
        assert!(h.contains("<a class=\"ref\" href=\"#F1\">12.3%</a>") && h.contains("<tr id=\"F1\">"));
        assert!(h.contains("<mark title=\"对不上\">17.6%</mark>"));
        assert!(h.contains("<details>\n<summary>事实表</summary>"));
        // 不引用任何外部资源：没有 link / img / iframe，script 和 style 都是内联的，唯一的外部地址是来源的超链接
        for banned in ["<link", "<img", "<iframe", "src=", "@import", "url("] {
            assert!(!h.contains(banned), "不该出现 {banned}");
        }
        let hrefs: Vec<&str> = h.match_indices("href=\"").map(|(i, _)| &h[i + 6..i + 6 + h[i + 6..].find('"').unwrap()]).collect();
        assert_eq!(hrefs, ["#F1", "https://stats.example.invalid/a b"]);
    }
}
