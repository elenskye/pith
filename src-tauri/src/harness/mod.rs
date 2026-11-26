//! 对话的 Agent 循环（1.1.0 第 2 期，`decisions/0035`、`0037`）：一个 Rust 写的循环，模型自己决定查什么、算什么、问不问、交不交付。
//!
//! 一轮：调模型（流式）→ 跑工具（只读的并行跑）→ 结果接回去 → 下一轮。停：
//! - 没有工具调用（写完了）；
//! - 问了站主（问题卡）：等他回答，这一回到此为止；
//! - 轮数上限：最后一轮不许调工具，让模型收尾；
//! - 打转：同一个调用、同一个结果第 3 次提醒、第 5 次让它收尾；
//! - 月上限、模型拒答、写到上限被截断、点了停、出错。
//!
//! 每一步一条事件（状态行的一格：Agent 图标 + 一句话），子代理的步骤带它的 id；正文一边写一边流出来。
//! 给模型看的对话只往后追加（缓存才命中）：这一回新加的消息（模型的、工具结果）原样存进回答的 body（`transcript`）。

pub mod brain;
pub mod calc;
pub mod prompt;
pub mod tools;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::gateway::meter::{self, Meter, Use};
use crate::gateway::{Delta, ErrorKind, Message, Model, Part, Request, Role, Stop, SystemBlock, Tokens};
use tools::{Toolbox, ToolOut};

/// 状态行的一格
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Agent 图标（marks/agents/<id>）：think、search、read、write、lib、compute、ask、save、bg……
    pub agent: String,
    pub text: String,
    /// 子代理的步骤：它的 id（收在派它的那一格下面）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

impl Step {
    pub fn new(agent: &str, text: impl Into<String>) -> Step {
        Step { agent: agent.into(), text: text.into(), sub: None }
    }
}

/// 问题卡：一句问话、几个选项（点了就当站主这样回答）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskCard {
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// 交付卡：站主点「存进」才种进知识库
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    /// 知识库材料的种类（answer / note / concept / excerpt）
    pub kind: String,
    pub title: String,
    pub desc: String,
    /// 存进去的正文（空着就存这一回的回答）
    #[serde(default)]
    pub body: String,
}

/// 实时的事件（界面轮询 `chat_live` 看到的就是这些攒起来的样子）
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Step(Step),
    /// 正文的增量
    Text(String),
    /// 这一轮写的是「我查一下」这类过程话、接着调了工具：正文清掉重来
    Reset,
}

/// 这一回怎么结束的
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    #[default]
    Done,
    /// 问了站主，等他回答
    Asked,
    /// 轮数到了，最后一轮收的尾
    Limit,
    /// 打转，让它收的尾
    Looping,
    Stopped,
    Budget,
    Refusal,
    /// 写到输出上限被截断
    Truncated,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// 最多几轮（到了再给一轮不许调工具的，让模型收尾）
    pub rounds: usize,
    pub max_output: u32,
    /// 一次回答最多派几个子代理
    pub subagents: usize,
    /// 同时跑几个
    pub parallel: usize,
    /// 子代理自己的轮数
    pub sub_rounds: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { rounds: 16, max_output: 16_000, subagents: 12, parallel: 3, sub_rounds: 8 }
    }
}

/// 循环要用的东西（不碰 Tauri，测试直接调）
pub struct Ctx<'a> {
    pub model: &'a dyn Model,
    pub effort: Option<&'a str>,
    /// 强一档（子代理用）；没有就用同一个
    pub strong: Option<(&'a dyn Model, Option<&'a str>)>,
    pub meter: &'a Meter,
    pub conversation: Option<&'a str>,
    pub env: &'a dyn tools::Env,
    pub cancel: &'a AtomicBool,
    pub sink: &'a (dyn Fn(Event) + Sync),
    pub limits: Limits,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// 这一回新加的消息（模型的、工具结果），原样存下来，下一回接在后面
    pub turns: Vec<Message>,
    /// 给站主看的回答（最后一条模型消息的文字）
    pub text: String,
    pub steps: Vec<Step>,
    pub ask: Option<AskCard>,
    pub delivery: Option<Delivery>,
    pub ending: Ending,
    /// 结束原因的一句话（停了、超了月上限、出错……）
    pub note: Option<String>,
    pub cost_usd: f64,
    pub tokens: Tokens,
    /// 实际回答的模型
    pub model: String,
    /// 派了几个子代理
    pub subagents: usize,
}

impl Outcome {
    fn add_tokens(&mut self, t: &Tokens) {
        self.tokens = Tokens { input: self.tokens.input + t.input, output: self.tokens.output + t.output, cache_read: self.tokens.cache_read + t.cache_read, cache_write: self.tokens.cache_write + t.cache_write };
    }
}

fn ending_of(kind: ErrorKind) -> Ending {
    match kind {
        ErrorKind::Cancelled => Ending::Stopped,
        ErrorKind::Budget => Ending::Budget,
        _ => Ending::Error,
    }
}

const STOPPED: &str = "已经写出来的留着。";

/// 打转的提醒：同一个调用、同一个结果第几次
pub const LOOP_WARN: u32 = 3;
pub const LOOP_STOP: u32 = 5;

/// 跑一回。`history` 是给模型看的整段对话（最后一条是站主这一问）。
pub fn run(ctx: &Ctx, system: &[SystemBlock], toolbox: &Toolbox, history: Vec<Message>, purpose: &str) -> Outcome {
    let specs = toolbox.specs();
    let mut msgs = history;
    let mut out = Outcome { model: ctx.model.spec().api_model.into(), ..Default::default() };
    let step = |out: &mut Outcome, s: Step| {
        (ctx.sink)(Event::Step(s.clone()));
        out.steps.push(s);
    };
    step(&mut out, Step::new("think", "想了想"));
    let mut seen: HashMap<String, (String, u32)> = HashMap::new();
    let mut wrap_up = false;
    // 界面上已经写出来、下一轮接着写时要另起一段的
    let mut carry = false;
    let u = Use { purpose, conversation: ctx.conversation };
    let mut round = 0;
    loop {
        if ctx.cancel.load(Ordering::SeqCst) {
            out.ending = Ending::Stopped;
            out.note = Some(STOPPED.into());
            break;
        }
        let last = round >= ctx.limits.rounds || wrap_up;
        let req = Request {
            system: system.to_vec(),
            tools: specs.clone(),
            messages: msgs.clone(),
            max_output_tokens: ctx.limits.max_output,
            effort: ctx.effort.map(str::to_string),
            no_tools: last,
            cache_key: ctx.conversation.map(str::to_string),
        };
        let mut streamed = false;
        let sent = meter::send(
            ctx.model,
            ctx.meter,
            &u,
            &req,
            &mut |d| {
                if let Delta::Text(t) = d {
                    // 上一轮写的留着（跟着交付卡写的回答）：这一轮另起一段
                    if !streamed && carry {
                        (ctx.sink)(Event::Text("\n\n".into()));
                    }
                    streamed = true;
                    (ctx.sink)(Event::Text(t));
                }
            },
            ctx.cancel,
        );
        let (reply, cost) = match sent {
            Ok(x) => x,
            Err(e) => {
                if let Some(t) = &e.spent {
                    out.add_tokens(t);
                    out.cost_usd += ctx.meter.row(ctx.model.spec().api_model).usd(t);
                }
                out.ending = ending_of(e.kind);
                out.note = Some(match (e.kind, &e.spent) {
                    (ErrorKind::Cancelled, Some(_)) => format!("{STOPPED}停之前生成的部分照样计费，按估计记进了用量。"),
                    (ErrorKind::Cancelled, None) => STOPPED.into(),
                    // 写到一半断了：已经生成的照样计费（1.1.0 真实验收里 Gemini 中途 503，提示却说没有产生费用）
                    (_, Some(_)) => format!("{}中途断了，已经生成的部分按估计记进了用量。", e.message.replace("这次没有产生费用。", "")),
                    _ => e.message,
                });
                break;
            }
        };
        out.cost_usd += cost;
        out.add_tokens(&reply.tokens);
        out.model = reply.model.clone();
        let calls: Vec<(String, String, serde_json::Value)> = reply.message.tool_calls().into_iter().map(|(i, n, v)| (i.to_string(), n.to_string(), v.clone())).collect();
        msgs.push(reply.message.clone());
        out.turns.push(reply.message.clone());
        round += 1;
        match reply.stop {
            Stop::Paused => continue,
            Stop::Refusal => {
                out.ending = Ending::Refusal;
                out.note = Some(match reply.refusal {
                    Some(r) if !r.is_empty() => format!("模型拒绝回答这一问（{r}）。多半是误判，换个说法或换一家再问。"),
                    _ => "模型拒绝回答这一问。多半是误判，换个说法或换一家再问。".into(),
                });
            }
            Stop::MaxTokens => {
                out.ending = Ending::Truncated;
                out.note = Some("回答太长，写到上限被截断了。".into());
            }
            _ => {}
        }
        if calls.is_empty() {
            if out.ending == Ending::Done && last {
                out.ending = if wrap_up { Ending::Looping } else { Ending::Limit };
            }
            break;
        }
        // 跟着查、读、算写的是「我查一下」这类过程话：清掉重来；跟着交付卡、问题卡写的是回答本身，留着
        // （1.1.0 真实验收：DeepSeek 把整篇回答和交付卡写在同一轮，以前被当成过程话丢了）
        let gathering = calls.iter().any(|(_, n, _)| toolbox.gathers(n));
        if streamed && gathering {
            (ctx.sink)(Event::Reset);
        }
        carry = (streamed && !gathering) || (carry && !streamed && !gathering);
        // 工具调用一定配上结果（不然下一回的对话不成立）；被截断、拒答、最后一轮的调用回一句不执行
        let refuse = if out.ending != Ending::Done {
            Some("这一回已经结束，这个调用没有执行。")
        } else if last {
            Some("这一轮不能再调工具了，直接把已经知道的写成回答。")
        } else {
            None
        };
        let results: Vec<ToolOut> = match refuse {
            Some(why) => calls.iter().map(|_| ToolOut::error(why)).collect(),
            None => toolbox.run_all(ctx, &calls, &mut out),
        };
        let mut parts = Vec::new();
        for ((call_id, name, input), mut r) in calls.into_iter().zip(results) {
            if let Some(s) = r.step.take() {
                if r.announced {
                    out.steps.push(s);
                } else {
                    step(&mut out, s);
                }
            }
            out.steps.append(&mut r.sub_steps);
            if let Some(a) = r.ask.take() {
                out.ask = Some(a);
            }
            if let Some(d) = r.delivery.take() {
                out.delivery = Some(d);
            }
            out.cost_usd += r.cost_usd;
            out.add_tokens(&r.tokens);
            // 打转：同一个调用、同一个结果
            let key = format!("{name}:{input}");
            let entry = seen.entry(key).or_insert((String::new(), 0));
            if entry.0 == r.output {
                entry.1 += 1;
            } else {
                *entry = (r.output.clone(), 1);
            }
            if entry.1 >= LOOP_STOP {
                wrap_up = true;
            } else if entry.1 >= LOOP_WARN {
                r.output.push_str(&format!("\n\n（提醒：这个调用和结果已经重复 {} 次了。换个办法，或者直接回答。）", entry.1));
            }
            parts.push(Part::ToolResult { call_id, output: r.output, is_error: r.is_error });
        }
        let results = Message { role: Role::User, parts };
        msgs.push(results.clone());
        out.turns.push(results);
        if out.ending != Ending::Done {
            break;
        }
        if out.ask.is_some() {
            out.ending = Ending::Asked;
            break;
        }
    }
    out.text = answer_text(&out.turns, toolbox);
    if matches!(out.ending, Ending::Done | Ending::Limit | Ending::Looping | Ending::Asked) && !out.text.trim().is_empty() {
        step(&mut out, Step::new("write", "写好了"));
    }
    if out.ending == Ending::Limit {
        out.note = Some(format!("查了 {} 轮还没查完，先把知道的写下来了。要接着查就说「接着查」。", ctx.limits.rounds));
    } else if out.ending == Ending::Looping {
        out.note = Some("同样的查法反复了几次，先停下写了回答。".into());
    }
    out
}

/// 给站主看的回答：最后一次查、读、算以后模型写的文字（跟着交付卡、问题卡写的也算），按顺序连起来；
/// 一段都没有（比如停在查的中途）就是最后一条有字的
pub fn answer_text(turns: &[Message], toolbox: &Toolbox) -> String {
    let mut answer: Vec<String> = Vec::new();
    for m in turns.iter().filter(|m| m.role == Role::Assistant) {
        if m.tool_calls().iter().any(|(_, n, _)| toolbox.gathers(n)) {
            answer.clear();
            continue;
        }
        let t = m.text();
        if !t.trim().is_empty() {
            answer.push(t.trim().to_string());
        }
    }
    if answer.is_empty() {
        return turns.iter().rev().filter(|m| m.role == Role::Assistant).map(Message::text).find(|t| !t.trim().is_empty()).unwrap_or_default();
    }
    answer.join("\n\n")
}

#[cfg(test)]
mod tests;
