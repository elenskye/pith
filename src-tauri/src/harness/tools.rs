//! 工具，按插件分组（`decisions/0035`「插件」：1.1.0 先按插件分组，1.4.0 立格式和插件库）。
//!
//! - 基础：`calculate`（精确计算）、`ask_user`（问题卡）、`deliver`（交付卡，站主点「存进」才存）、`delegate`（派子代理）；
//! - 文献库：`library_search`、`library_item`（只读）。
//!
//! 只读的（计算、查文献库、子代理）一批里并行跑，最多 `Limits::parallel` 个一起；问题卡、交付卡按顺序。
//! 工具名用英文（各家接口都认），说明用中文。工具是 Rust 写的，只做程序该做的：给数、给条目，不规定回答怎么写（`0030`）。

use serde_json::{Value, json};

use super::{AskCard, Ctx, Delivery, Limits, Outcome, Step, calc, prompt};
use crate::gateway::{BAD_INPUT_KEY, Message, SystemBlock, Tokens, ToolSpec};

/// 工具要碰的外面的东西（App 里是数据库；测试里是假的）
pub trait Env: Send + Sync {
    fn library_search(&self, query: &str, limit: usize) -> Result<Vec<LibHit>, String>;
    fn library_item(&self, id: i64) -> Result<String, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct LibHit {
    pub id: i64,
    pub title: String,
    pub creator: String,
    pub year: Option<i64>,
    pub container: String,
    pub item_type: String,
}

pub struct ToolOut {
    pub output: String,
    pub is_error: bool,
    pub step: Option<Step>,
    pub ask: Option<AskCard>,
    pub delivery: Option<Delivery>,
    /// 子代理花的
    pub cost_usd: f64,
    pub tokens: Tokens,
    /// 子代理的步骤（收在派它的那一格下面）
    pub sub_steps: Vec<Step>,
    /// 这一格开始时已经发给界面了（子代理：先露出来，它的步骤才接得上），循环只记、不再发
    pub announced: bool,
}

impl ToolOut {
    fn ok(output: impl Into<String>, step: Step) -> ToolOut {
        ToolOut { output: output.into(), is_error: false, step: Some(step), ask: None, delivery: None, cost_usd: 0.0, tokens: Tokens::default(), sub_steps: Vec::new(), announced: false }
    }

    pub fn error(output: impl Into<String>) -> ToolOut {
        ToolOut { output: output.into(), is_error: true, step: None, ask: None, delivery: None, cost_usd: 0.0, tokens: Tokens::default(), sub_steps: Vec::new(), announced: false }
    }
}

pub struct Tool {
    pub plugin: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub read_only: bool,
}

/// 全部工具（顺序固定：缓存的前缀里不能变）
pub static TOOLS: &[Tool] = &[
    Tool {
        plugin: "基础",
        name: "calculate",
        description: "精确计算一个算式（加减乘除、括号、百分数如 12%、整数次方 ^）。凡是要算的数都用它，不要心算。",
        schema: || json!({ "type": "object", "properties": { "expression": { "type": "string", "description": "算式，例如 (1234 - 1000) / 1000 * 100" } }, "required": ["expression"] }),
        read_only: true,
    },
    Tool {
        plugin: "基础",
        name: "ask_user",
        description: "给站主一张问题卡：一句问话和两到四个选项（站主也能自己写）。只在真要他拍板时用（几个方向选一个、缺关键信息）；问了这一回就停下等他。",
        schema: || json!({ "type": "object", "properties": { "question": { "type": "string" }, "options": { "type": "array", "items": { "type": "string" }, "maxItems": 4 } }, "required": ["question"] }),
        read_only: false,
    },
    Tool {
        plugin: "基础",
        name: "deliver",
        description: "把这一回值得留下的成果做成一张交付卡（整理好的回答、概念卡、清单、摘抄）。站主点「存进」才会种进知识库，你不能自己存。body 空着就存这一回的回答。",
        schema: || {
            json!({ "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["answer", "note", "concept", "excerpt"], "description": "answer 存下的回答 / note 笔记 / concept 概念卡 / excerpt 摘抄" },
                "title": { "type": "string" },
                "summary": { "type": "string", "description": "一两句话说它是什么" },
                "body": { "type": "string" }
            }, "required": ["kind", "title", "summary"] })
        },
        read_only: false,
    },
    Tool {
        plugin: "基础",
        name: "delegate",
        description: "派一个子代理去做一件独立的事，它用强一档的模型、看不到这段对话，做完交回一份报告。适合：读很长的东西、写一段要深想的分析。task 写清楚要什么，materials 放它要用的材料原文。可以一次派几个，并行做。",
        schema: || {
            json!({ "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["read"], "description": "read：读、想、写报告" },
                "title": { "type": "string", "description": "几个字的标题，给站主看" },
                "task": { "type": "string" },
                "materials": { "type": "string" }
            }, "required": ["kind", "title", "task"] })
        },
        read_only: true,
    },
    Tool {
        plugin: "文献库",
        name: "library_search",
        description: "在站主的文献库里搜条目（标题、作者、标签、摘要、笔记）。返回条目号、第一作者、年份、标题、期刊或书名。",
        schema: || json!({ "type": "object", "properties": { "query": { "type": "string", "description": "搜索词；英文文献用英文词" }, "limit": { "type": "integer", "minimum": 1, "maximum": 20 } }, "required": ["query"] }),
        read_only: true,
    },
    Tool {
        plugin: "文献库",
        name: "library_item",
        description: "看文献库里一个条目的详情（作者、日期、出处、DOI、摘要、标签、附件和笔记的数目）。id 是 library_search 给的条目号。",
        schema: || json!({ "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] }),
        read_only: true,
    },
];

/// 一次对话能用的工具（子代理用的是去掉问题卡、交付卡、再派的一份）
pub struct Toolbox {
    names: Vec<&'static str>,
}

impl Toolbox {
    pub fn main() -> Toolbox {
        Toolbox { names: TOOLS.iter().map(|t| t.name).collect() }
    }

    pub fn subagent() -> Toolbox {
        Toolbox { names: TOOLS.iter().filter(|t| t.read_only && t.name != "delegate").map(|t| t.name).collect() }
    }

    pub fn tools(&self) -> impl Iterator<Item = &'static Tool> + '_ {
        TOOLS.iter().filter(|t| self.names.contains(&t.name))
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools().map(|t| ToolSpec { name: t.name.into(), description: t.description.into(), input_schema: (t.schema)() }).collect()
    }

    fn get(&self, name: &str) -> Option<&'static Tool> {
        self.tools().find(|t| t.name == name)
    }

    /// 一批调用：只读的并行（每次最多 `parallel` 个），别的按顺序；结果按原来的顺序
    pub fn run_all(&self, ctx: &Ctx, calls: &[(String, String, Value)], out: &mut Outcome) -> Vec<ToolOut> {
        let mut results: Vec<Option<ToolOut>> = (0..calls.len()).map(|_| None).collect();
        // （第几个调用, 子代理的序号：这一回里第几个子代理）
        let mut parallel: Vec<(usize, usize)> = Vec::new();
        for (i, (_, name, input)) in calls.iter().enumerate() {
            match self.get(name) {
                Some(t) if t.read_only => {
                    if name == "delegate" {
                        if out.subagents >= ctx.limits.subagents {
                            results[i] = Some(ToolOut::error(format!("这一回已经派了 {} 个子代理，到上限了。用已经有的结果回答。", ctx.limits.subagents)));
                            continue;
                        }
                        out.subagents += 1;
                    }
                    parallel.push((i, out.subagents));
                }
                Some(_) => results[i] = Some(self.run_one(ctx, name, input, out.subagents)),
                None => results[i] = Some(ToolOut::error(format!("没有叫 {name} 的工具。"))),
            }
        }
        for chunk in parallel.chunks(ctx.limits.parallel.max(1)) {
            let done: Vec<(usize, ToolOut)> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk.iter().map(|&(i, seq)| (i, s.spawn(move || self.run_one(ctx, &calls[i].1, &calls[i].2, seq)))).collect();
                handles.into_iter().map(|(i, h)| (i, h.join().unwrap_or_else(|_| ToolOut::error("工具出了内部错误。")))).collect()
            });
            for (i, r) in done {
                results[i] = Some(r);
            }
        }
        results.into_iter().map(|r| r.unwrap_or_else(|| ToolOut::error("工具没有执行。"))).collect()
    }

    fn run_one(&self, ctx: &Ctx, name: &str, input: &Value, seq: usize) -> ToolOut {
        if let Some(raw) = input.get(BAD_INPUT_KEY) {
            return ToolOut::error(format!("参数不是完整的 JSON（可能被截断）：{}。重新调用一次。", raw.as_str().unwrap_or("")));
        }
        let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
        match name {
            "calculate" => {
                let e = s("expression");
                match calc::eval(&e) {
                    Ok(v) => ToolOut::ok(format!("{e} = {v}"), Step::new("compute", format!("算了 {} = {v}", short(&e, 40)))),
                    Err(why) => ToolOut::error(format!("算不了：{why}")),
                }
            }
            "ask_user" => {
                let question = s("question");
                if question.is_empty() {
                    return ToolOut::error("问题卡要有一句问话。");
                }
                let options: Vec<String> = input.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).take(4).collect();
                let mut r = ToolOut::ok("问题卡已经给站主了。这一回到此为止，等他回答。", Step::new("ask", "问你一句"));
                r.ask = Some(AskCard { question, options });
                r
            }
            "deliver" => {
                let (title, summary) = (s("title"), s("summary"));
                if title.is_empty() {
                    return ToolOut::error("交付卡要有标题。");
                }
                let kind = match s("kind").as_str() {
                    k @ ("answer" | "note" | "concept" | "excerpt") => k.to_string(),
                    _ => "answer".into(),
                };
                let mut r = ToolOut::ok("交付卡已经放在回答下面了。站主点「存进」才会种进知识库。", Step::new("save", format!("做了一张交付卡：{}", short(&title, 24))));
                r.delivery = Some(Delivery { kind, title, desc: summary, body: input.get("body").and_then(Value::as_str).unwrap_or("").to_string() });
                r
            }
            "library_search" => {
                let q = s("query");
                let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(8).clamp(1, 20) as usize;
                match ctx.env.library_search(&q, limit) {
                    Ok(hits) => ToolOut::ok(format_hits(&q, &hits), Step::new("lib", if q.is_empty() { format!("翻了翻文献库：{} 条", hits.len()) } else { format!("在文献库里找「{}」：{} 条", short(&q, 20), hits.len()) })),
                    Err(e) => ToolOut::error(format!("文献库查不了：{e}")),
                }
            }
            "library_item" => match input.get("id").and_then(Value::as_i64) {
                Some(id) => match ctx.env.library_item(id) {
                    Ok(text) => ToolOut::ok(text, Step::new("lib", format!("看了条目 {id}"))),
                    Err(e) => ToolOut::error(e),
                },
                None => ToolOut::error("要给条目号 id（library_search 给的）。"),
            },
            "delegate" => delegate(ctx, &s("title"), &s("task"), &s("materials"), seq),
            other => ToolOut::error(format!("没有叫 {other} 的工具。")),
        }
    }
}

fn short(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > n { format!("{}…", s.chars().take(n).collect::<String>()) } else { s }
}

fn format_hits(q: &str, hits: &[LibHit]) -> String {
    if hits.is_empty() {
        return format!("文献库里没有找到和「{q}」有关的条目。");
    }
    let mut out = format!("文献库里和「{q}」有关的 {} 条：\n", hits.len());
    for h in hits {
        let year = h.year.map(|y| format!("（{y}）")).unwrap_or_default();
        let by = if h.creator.is_empty() { String::new() } else { format!("{} ", h.creator) };
        let container = if h.container.is_empty() { String::new() } else { format!(" · {}", h.container) };
        out.push_str(&format!("[条目 {}] {by}{year}《{}》{container} · {}\n", h.id, h.title, h.item_type));
    }
    out
}

/// 子代理交回的报告最多这么长（太长的截断，主对话的上下文不被撑爆）
pub const REPORT_CHARS: usize = 8000;

/// 派一个子代理：强一档的模型、看不到主对话、只带只读的工具；它的步骤收在派它的这一格下面，花的钱算进这一回
fn delegate(ctx: &Ctx, title: &str, task: &str, materials: &str, seq: usize) -> ToolOut {
    if task.is_empty() {
        return ToolOut::error("要写清楚 task：让子代理做什么。");
    }
    let title = if title.is_empty() { short(task, 16) } else { title.to_string() };
    let id = format!("s{seq}");
    let (model, effort) = ctx.strong.unwrap_or((ctx.model, ctx.effort));
    let parent = ctx.sink;
    // 先露出这一格，它的步骤接在下面
    let announce = Step::new("bg", format!("请{}读：{}", model.spec().label, short(&title, 20)));
    parent(super::Event::Step(announce.clone()));
    let sub_id = id.clone();
    let sink = move |e: super::Event| {
        if let super::Event::Step(mut s) = e {
            s.sub = Some(sub_id.clone());
            parent(super::Event::Step(s));
        }
    };
    let sub = Ctx {
        model,
        effort,
        strong: None,
        meter: ctx.meter,
        conversation: ctx.conversation,
        env: ctx.env,
        cancel: ctx.cancel,
        sink: &sink,
        limits: Limits { rounds: ctx.limits.sub_rounds, ..ctx.limits },
    };
    let system: Vec<SystemBlock> = prompt::subagent_system();
    let mut ask = format!("任务：{task}");
    if !materials.is_empty() {
        ask.push_str(&format!("\n\n材料：\n{materials}"));
    }
    let o = super::run(&sub, &system, &Toolbox::subagent(), vec![Message::user(ask)], "chat_subagent");
    let mut report = o.text.trim().to_string();
    if report.chars().count() > REPORT_CHARS {
        report = format!("{}……（报告太长，后面截掉了）", report.chars().take(REPORT_CHARS).collect::<String>());
    }
    let failed = report.is_empty();
    let note = o.note.clone().unwrap_or_default();
    let output = if failed { format!("子代理「{title}」没有交回报告。{note}") } else { format!("子代理「{title}」交回的报告：\n\n{report}") };
    let mut r = ToolOut::ok(output, announce);
    r.announced = true;
    r.is_error = failed;
    r.cost_usd = o.cost_usd;
    r.tokens = o.tokens;
    r.sub_steps = o.steps.into_iter().map(|mut s| {
        s.sub = Some(id.clone());
        s
    }).collect();
    r
}
