//! 模拟的回答（1.0.2）：不调模型、不花钱，照真回答的样子走一遍——在勾的知识库里找、读、写、编号，最后给一张交付卡。
//! 编号指向知识库里真的材料（按问题里的字粗略地挑），这样页边注、「知识库里这一段」都能点通。
//! 1.1.0 接上 Agent 循环以后由真的回答换掉（`decisions/0030`「1.1.0」）；回答的 JSON 形状不变。

use rusqlite::Connection;
use serde_json::{Value, json};

use super::store::{self, Material, Role};

/// 问题里的「词」：连续的汉字两两一组，英文按单词（≥ 3 个字母）
fn terms(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut han: Vec<char> = Vec::new();
    let mut word = String::new();
    let flush_han = |han: &mut Vec<char>, out: &mut Vec<String>| {
        for w in han.windows(2) {
            out.push(w.iter().collect());
        }
        han.clear();
    };
    for ch in q.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            han.push(ch);
        } else {
            flush_han(&mut han, &mut out);
        }
        if ch.is_ascii_alphanumeric() {
            word.push(ch.to_ascii_lowercase());
        } else {
            if word.len() >= 3 {
                out.push(std::mem::take(&mut word));
            }
            word.clear();
        }
    }
    flush_han(&mut han, &mut out);
    if word.len() >= 3 {
        out.push(word);
    }
    // 太常见的两个字不算
    const STOP: [&str; 12] = ["什么", "为什", "么说", "怎么", "这个", "一个", "是不", "不是", "我们", "他们", "可以", "没有"];
    out.retain(|t| !STOP.contains(&t.as_str()));
    out.dedup();
    out
}

fn score(m: &Material, terms: &[String]) -> usize {
    let hay = format!("{} {} {}", m.title, m.sub, m.body).to_lowercase();
    terms.iter().filter(|t| hay.contains(t.as_str())).count()
}

const KIND_NAMES: [(&str, &str); 10] = [
    ("full", "全文"),
    ("excerpt", "摘抄"),
    ("note", "笔记"),
    ("web", "网页快照"),
    ("file", "本机文件"),
    ("scan", "扫描件"),
    ("concept", "概念卡"),
    ("report", "调查报告"),
    ("wonder", "WonderLab 作品"),
    ("answer", "存下的回答"),
];

pub fn kind_name(kind: &str) -> &'static str {
    KIND_NAMES.iter().find(|k| k.0 == kind).map_or("材料", |k| k.1)
}

fn excerpt(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > n { format!("{}……", s.chars().take(n).collect::<String>()) } else { s }
}

/// 回答一句话。kbs 是勾的知识库 uid（空的：所有知识库）。
pub fn answer(c: &Connection, question: &str, role: &Role, kbs: &[String]) -> Result<Value, String> {
    let all = store::kbs_list(c)?;
    let picked: Vec<_> = if kbs.is_empty() { all.iter().collect() } else { all.iter().filter(|k| kbs.contains(&k.uid)).collect() };
    let mut pool: Vec<Material> = Vec::new();
    for k in &picked {
        pool.extend(store::materials(c, Some(&k.uid))?);
    }
    let ts = terms(question);
    let mut ranked: Vec<(usize, Material)> = pool.iter().map(|m| (score(m, &ts), m.clone())).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    let mut hits: Vec<Material> = ranked.iter().filter(|(s, _)| *s > 0).map(|(_, m)| m.clone()).take(3).collect();
    if hits.is_empty() {
        hits = pool.iter().take(2).cloned().collect();
    }

    let where_ = match picked.as_slice() {
        [] => "知识库".to_string(),
        [one] => format!("「{}」", one.name),
        many => format!("{} 个知识库", many.len()),
    };
    let mut steps = vec![json!({ "agent": "think", "text": "想了想怎么回答" })];
    if !pool.is_empty() {
        steps.push(json!({ "agent": "search", "text": format!("在{where_}里找了 {} 份", pool.len()) }));
        steps.push(json!({ "agent": "read", "text": format!("读了 {} 份", hits.len()) }));
    }
    steps.push(json!({ "agent": "write", "text": "写好了（演示）" }));

    let cites: Vec<Value> = hits
        .iter()
        .enumerate()
        .map(|(i, m)| {
            json!({
                "n": i + 1,
                "kind": kind_name(&m.kind),
                "src": m.title,
                "loc": m.sub,
                "text": excerpt(if m.body.is_empty() { &m.title } else { &m.body }, 160),
                "material": m.uid,
                "kb": m.kb,
            })
        })
        .collect();

    let mut paragraphs = vec![format!(
        "这是一段演示回答：这一版 Pith 还没接上模型对话，{}先照真回答的样子走一遍——找、读、写、编号，问题本身没有被回答。",
        if role.uid == "pith" { String::new() } else { format!("「{}」", role.name) }
    )];
    match hits.as_slice() {
        [] if picked.is_empty() => paragraphs.push("你还没有知识库。到顶栏的知识库里建一个，把读过的书、做完的调查种进去，回答就会带上编号，点编号能看到原文。".into()),
        [] => paragraphs.push(format!("{where_}里还没有材料。把读过的书、做完的调查种进去，回答就会带上编号。")),
        [one] => paragraphs.push(format!("{where_}里和这个问题最近的是「{}」[1]。", one.title)),
        [first, rest @ ..] => {
            let more: Vec<String> = rest.iter().enumerate().map(|(i, m)| format!("「{}」[{}]", m.title, i + 2)).collect();
            paragraphs.push(format!("{where_}里和这个问题最近的是「{}」[1]，其次是{}。", first.title, more.join("、")));
        }
    }
    paragraphs.push("要真的查证，用 /调查；要读一本书，用 /概念。1.1.0 接上 Agent 循环以后，这里就是真的回答。".into());

    let ask = match role.uid.as_str() {
        "companion" => Some("想一想：这个问题里，哪个词你最想先弄清楚？"),
        "reviewer" => Some("这个判断最弱的地方在哪，你心里有数吗？"),
        _ => None,
    };
    let kb_for_save = picked.first().map(|k| json!({ "uid": k.uid, "name": k.name }));
    Ok(json!({
        "mock": true,
        "steps": steps,
        "paragraphs": paragraphs,
        "cites": cites,
        "ask": ask,
        "delivery": {
            "kind": "answer",
            "title": store::title_of(question),
            "desc": excerpt(&paragraphs[1], 60),
            "meta": format!("存下的回答 • {} 处出处", cites.len()),
            "kb": kb_for_save,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_pair_han_and_words() {
        let t = terms("斯密为什么说分工受 market 范围限制？");
        assert!(t.contains(&"分工".to_string()));
        assert!(t.contains(&"market".to_string()));
        assert!(!t.contains(&"什么".to_string()));
    }
}
