//! 开发版的模拟模型的「脑子」：不联网、不花钱，照真模型的样子走一遍循环——先查（问题里有算式先算，再查知识库、文献库）、
//! 再做一张交付卡、最后写回答，回答里引用知识库找到的前两段（`[P123]`，程序换成 [1]），说清楚这是模拟的。
//! 正式版、`PITH_REAL_MODEL=1` 用不到它。

use serde_json::json;

use crate::gateway::mock::{Brain, Turn};
use crate::gateway::{Message, Part, Request, Role};

/// 这一问（最后一条带文字的站主消息；去掉开头那一行上下文）
fn question(req: &Request) -> String {
    let text = req.messages.iter().rev().filter(|m| m.role == Role::User).map(Message::text).find(|t| !t.trim().is_empty()).unwrap_or_default();
    text.split_once("\n\n").map(|(_, q)| q.to_string()).unwrap_or(text).trim().to_string()
}

/// 这一问以后已经调过哪些工具、拿到什么
fn since_question(req: &Request) -> Vec<(String, String)> {
    let start = req.messages.iter().rposition(|m| m.role == Role::User && !m.text().trim().is_empty()).unwrap_or(0);
    let mut names = std::collections::HashMap::new();
    let mut out = Vec::new();
    for m in &req.messages[start..] {
        for p in &m.parts {
            match p {
                Part::ToolCall { call_id, name, .. } => {
                    names.insert(call_id.clone(), name.clone());
                }
                Part::ToolResult { call_id, output, .. } => out.push((names.get(call_id).cloned().unwrap_or_default(), output.clone())),
                _ => {}
            }
        }
    }
    out
}

fn arithmetic(q: &str) -> Option<String> {
    let expr: String = q.chars().filter(|c| c.is_ascii_digit() || "+-*/().% ×÷".contains(*c)).collect();
    let expr = expr.trim().to_string();
    (expr.chars().any(|c| c.is_ascii_digit()) && expr.chars().any(|c| "+-*/×÷".contains(c))).then_some(expr)
}

pub fn brain() -> Brain {
    Box::new(|req: &Request| {
        let q = question(req);
        let done = since_question(req);
        if req.no_tools {
            return Turn::text("（模拟）先写到这里。");
        }
        let has = |name: &str| done.iter().any(|(n, _)| n == name);
        if let Some(expr) = arithmetic(&q).filter(|_| !has("calculate")) {
            return Turn { text: "我先算一下。".into(), calls: vec![("calculate".into(), json!({ "expression": expr }))], ..Default::default() };
        }
        // 要查网上的（「网上」「最新」「联网」「查一下」）：搜一次、取回第一条
        let online = ["网上", "最新", "联网", "查一下", "新闻"].iter().any(|w| q.contains(w));
        if online && !has("web_search") {
            return Turn { text: "我上网查一下。".into(), calls: vec![("web_search".into(), json!({ "query": q, "limit": 3 }))], ..Default::default() };
        }
        if online && !has("web_fetch") {
            let found = done.iter().find(|(n, _)| n == "web_search").map(|(_, r)| r.clone()).unwrap_or_default();
            if let Some(url) = found.split_whitespace().find(|w| w.starts_with("http")) {
                return Turn::call("web_fetch", json!({ "url": url }));
            }
        }
        if !has("kb_search") {
            return Turn { text: "我先在知识库里找找。".into(), reasoning: "先看看知识库里有没有相关的原文。".into(), calls: vec![("kb_search".into(), json!({ "query": q, "limit": 4 }))], ..Default::default() };
        }
        if !has("library_search") {
            // 有英文词就拿英文词搜（文献以英文为主），没有就拿整句
            let latin: Vec<&str> = q.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| w.len() >= 3).collect();
            let query: String = if latin.is_empty() { q.chars().filter(|c| !"？?。，,！!".contains(*c)).take(24).collect() } else { latin.join(" ") };
            return Turn { text: "我先在文献库里找找。".into(), reasoning: "先看看文献库里有没有相关的条目。".into(), calls: vec![("library_search".into(), json!({ "query": query }))], ..Default::default() };
        }
        let title: String = q.chars().take(24).collect();
        if !has("deliver") {
            return Turn::call("deliver", json!({ "kind": "answer", "title": title, "summary": "开发版的模拟回答：查了什么、找到了什么。" }));
        }
        let found = done.iter().find(|(n, _)| n == "library_search").map(|(_, r)| r.lines().next().unwrap_or("").to_string()).unwrap_or_default();
        let calc = done.iter().find(|(n, _)| n == "calculate").map(|(_, r)| format!("算出来 {r}。")).unwrap_or_default();
        let kb = done.iter().find(|(n, _)| n == "kb_search").map(|(_, r)| r.clone()).unwrap_or_default();
        let cited: Vec<String> = crate::kb::cite::handles(&kb).into_iter().take(2).map(|id| format!("[{id}]")).collect();
        let web = done.iter().find(|(n, _)| n == "web_fetch").map(|(_, r)| r.clone()).unwrap_or_default();
        let web_line = match crate::kb::cite::handles(&web).first() {
            Some(h) => format!("网上取回的那一页里有一段 [{h}]。"),
            None => String::new(),
        };
        let kb_line = if cited.is_empty() { "知识库里没找到相关的段落。".to_string() } else { format!("知识库里最相关的是这几段{}，点编号能看原文。", cited.join("")) };
        Turn::text(format!(
            "这是开发版的模拟回答：没有联网、不花钱，问题本身没有被回答。正式版（或设了 PITH_REAL_MODEL=1）这里是真的模型在答。\n\n刚才照真回答的样子走了一遍：{calc}{web_line}{kb_line}在文献库里也查了一下——{found}\n\n下面那张交付卡点「存进」，能看种子飞进知识库。"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_out_arithmetic_and_the_question() {
        assert_eq!(arithmetic("算一下 (120-100)/100 是多少").as_deref(), Some("(120-100)/100"));
        assert_eq!(arithmetic("1776 年的国富论"), None);
        let req = Request { messages: vec![Message::user("（今天是 2026-11-26。）\n\n斯密怎么说分工？")], ..Default::default() };
        assert_eq!(question(&req), "斯密怎么说分工？");
    }
}
