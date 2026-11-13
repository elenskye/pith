//! 提示词、结构化输出的 JSON Schema 和解析：所有技能共用一套（`decisions/0024`「产出统一成几种内容」）。
//!
//! 模型只出「卡片 + 关系」：每张卡由若干句组成，每句写类别和锚点；核对全在 `verify`。
//! Schema 照 OpenAI 结构化输出的严格模式写（每个字段都 required，可空的写成 `["string", "null"]`，`additionalProperties: false`），
//! Anthropic 的 `output_config.format` 也认。解析是宽容的：去掉代码块围栏，取第一个 `{` 到最后一个 `}`。

use research_engine::ports::{ModelRequest, Purpose};
use serde_json::{Value, json};

use crate::align::{self, Alignment};
use crate::model::{Card, CardKind, Category, RelationKind, Run, Sentence};
use crate::skill::{Input, Produce, Skill, Step};
use crate::text::{Role, Text, UnitKind};

/// 一次研究用到的书：原著，和对齐的译本
#[derive(Clone, Copy)]
pub struct Books<'a> {
    pub original: &'a Text,
    pub translation: Option<&'a Text>,
    pub alignment: Option<&'a Alignment>,
}

/// 来源的正文（发给模型的）
#[derive(Clone, Debug, PartialEq)]
pub struct SourceText {
    pub id: String,
    pub title: String,
    pub where_from: String,
    pub text: String,
}

/// 每个来源最多发多少字
pub const SOURCE_CHARS: usize = 12_000;
/// 译本里的引用名前缀：「译:I.i 注 1」
pub const TRANSLATION_PREFIX: &str = "译:";

const RULES: &str = "你在帮一位研究者读一本经典著作。输出只能是一个 JSON 对象（格式见 JSON Schema）：若干张卡片（cards）和卡片之间的关系（relations）。

每张卡片由若干句（sentences）组成，每句必须写明类别（category）：
- original（原文）：逐字摘自原著正文的一句或几句。text 和 quote 都写这段原文，一个字不改（不改拼写、标点、大小写）；paragraph 写它所在的段落号（方括号里的，如 I.i.3）；zh 写对齐的中译段落里对应的那一句（逐字摘录），没有对齐的中译就写 null。
- others（他人）：有出处的别人的话。来自来源（S1、S2……）的：source 写来源编号，quote 逐字摘录来源原文；来自译本的旁注、注释、导读的：paragraph 写它的引用名（如「译:I.i 注 1」），quote 逐字摘录。没有出处的说法不能标成他人。
- explanation（解释）：你的解释、概括、说明。尽量挂到一个段落号上（paragraph）；带引文时 quote 逐字摘录那个段落里的话。挂不上就写 null，照样可以输出。
- speculation（推演）：假设推演，不是原著的观点。可以用 quote 引一句原文当前提（同时写 paragraph），引文会被逐字核对。

要求：
- 原文、他人两类的引文会由程序逐字核对，核不上的整句会被删掉。宁可少引，不要改字、拼接或凭记忆补。
- 旁注、注释、导读、编者导言都不是原著正文，不能标成原文。
- 卡片的 key 用 c1、c2……；关系的 from、to 写卡片的 key（这次新出的）或已有卡片的编号（K1……）。
- 卡片的 about 写这张卡针对的已有卡片编号（评论卡、推演卡用），没有就写空数组。
- 用中文写标题和解释；原文句保持原文语言。";

pub fn system(skill: &Skill, step: &Step) -> String {
    let kinds: Vec<&str> = step.produces.iter().filter(|p| **p != Produce::Relation).map(|p| p.label()).collect();
    let mut s = String::from(RULES);
    s.push_str(&format!(
        "\n\n这一步只出：{}{}。",
        if kinds.is_empty() { "（不出卡片）".to_string() } else { kinds.join("、") },
        if step.produces.contains(&Produce::Relation) { "，以及关系" } else { "；relations 写空数组" }
    ));
    s.push_str(&format!("\n\n# 技能：{}\n\n{}\n\n# 这一步：{}\n\n{}", skill.title, skill.general.trim(), if step.title.is_empty() { &step.id } else { &step.title }, step.instructions.trim()));
    s
}

fn sentence_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["category", "text", "paragraph", "quote", "zh", "source"],
        "properties": {
            "category": { "type": "string", "enum": Category::ALL.iter().map(|c| c.key()).collect::<Vec<_>>() },
            "text": { "type": "string" },
            "paragraph": { "type": ["string", "null"] },
            "quote": { "type": ["string", "null"] },
            "zh": { "type": ["string", "null"] },
            "source": { "type": ["string", "null"] }
        }
    })
}

/// 输出的 JSON Schema（所有技能共用；这一步能出的卡片种类写进 enum）
pub fn schema(step: &Step) -> Value {
    let mut kinds: Vec<&str> = step.produces.iter().filter_map(|p| CardKind::from_key(p.key())).map(|k| k.key()).collect();
    if kinds.is_empty() {
        kinds = CardKind::ALL.iter().map(|k| k.key()).collect();
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["cards", "relations"],
        "properties": {
            "cards": { "type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["key", "kind", "title", "about", "sentences"],
                "properties": {
                    "key": { "type": "string" },
                    "kind": { "type": "string", "enum": kinds },
                    "title": { "type": "string" },
                    "about": { "type": "array", "items": { "type": "string" } },
                    "sentences": { "type": "array", "items": sentence_schema() }
                }
            }},
            "relations": { "type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["from", "to", "kind", "note"],
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "kind": { "type": "string", "enum": RelationKind::ALL.iter().map(|k| k.key()).collect::<Vec<_>>() },
                    "note": sentence_schema()
                }
            }}
        }
    })
}

// ---------- 发给模型的内容 ----------

/// 范围里的原著段落（按书里的顺序）
pub fn scope_paragraphs<'a>(run: &Run, books: &Books<'a>) -> Vec<&'a crate::text::Paragraph> {
    let Some(scope) = &run.scope else { return Vec::new() };
    let mut out: Vec<&crate::text::Paragraph> = scope.paragraphs.iter().filter_map(|k| books.original.paragraph(k)).collect();
    out.sort_by_key(|p| p.seq);
    out.dedup_by_key(|p| p.seq);
    out
}

fn original_section(run: &Run, books: &Books) -> String {
    let t = books.original;
    let mut s = format!("## 原著正文（{}；{}）\n", if t.title.is_empty() { "原著" } else { &t.title }, if t.language == "zh" { "中文" } else { "英文" });
    for p in scope_paragraphs(run, books) {
        s.push_str(&format!("[{}] {}\n", p.key, p.text));
    }
    s
}

fn translation_section(run: &Run, books: &Books) -> String {
    let (Some(zh), Some(al)) = (books.translation, books.alignment) else { return "## 对齐的中译\n（没有对齐的译本）\n".into() };
    let mut body = String::new();
    let mut apparatus = String::new();
    let mut missing = Vec::new();
    let mut zh_units: Vec<usize> = Vec::new();
    for p in scope_paragraphs(run, books) {
        match align::counterpart(al, books.original, zh, p.seq) {
            Some(z) => {
                body.push_str(&format!("[{}] {}\n", p.key, z.text));
                if !zh_units.contains(&z.unit) {
                    zh_units.push(z.unit);
                }
                for a in zh.paragraphs.iter().filter(|x| x.attached == Some(z.seq) && matches!(x.role, Role::Sidenote | Role::Note)) {
                    apparatus.push_str(&format!("[{TRANSLATION_PREFIX}{}] {}\n", a.key, a.text));
                }
            }
            None => missing.push(p.key.clone()),
        }
    }
    // 章前的导读（紧挨在这一章前面的导读单元）
    for u in &zh_units {
        let mut i = *u;
        while i > 0 && zh.units[i - 1].kind == UnitKind::Guide {
            i -= 1;
            for g in zh.paragraphs.iter().filter(|p| p.unit == i && p.role == Role::Guide) {
                apparatus.push_str(&format!("[{TRANSLATION_PREFIX}{}] {}\n", g.key, g.text));
            }
        }
    }
    let mut s = format!("## 对齐的中译（{}）\n", if zh.title.is_empty() { "译本" } else { &zh.title });
    s.push_str(&body);
    if !missing.is_empty() {
        s.push_str(&format!("（{} 没有对齐的中译：这一章中英正文段数不同）\n", missing.join("、")));
    }
    if !apparatus.is_empty() {
        s.push_str("\n## 译本的旁注、注释、导读（不是原著正文；引用时标成他人）\n");
        s.push_str(&apparatus);
    }
    s
}

pub fn card_text(c: &Card) -> String {
    let mut s = format!("[{}] {}：{}\n", c.id, c.kind.label(), c.title);
    for x in &c.sentences {
        let at = x.paragraph.as_deref().map(|p| format!("（{p}）")).unwrap_or_default();
        s.push_str(&format!("  - {}{at}：{}\n", x.category.label(), x.text));
    }
    s
}

fn cards_section(run: &Run) -> String {
    let mut s = String::from("## 已有的卡片\n");
    let all: Vec<&Card> = run.targets.iter().chain(run.cards.iter()).collect();
    if all.is_empty() {
        s.push_str("（还没有）\n");
    }
    for c in all {
        s.push_str(&card_text(c));
    }
    s
}

fn sources_section(sources: &[SourceText]) -> String {
    let mut s = String::from("## 来源\n");
    if sources.is_empty() {
        s.push_str("（没有找到来源）\n");
    }
    for x in sources {
        let text: String = x.text.chars().take(SOURCE_CHARS).collect();
        let cut = if x.text.chars().count() > SOURCE_CHARS { "\n（后面略）" } else { "" };
        s.push_str(&format!("[{}] {}（{}）\n{}{}\n\n", x.id, x.title, x.where_from, text.trim(), cut));
    }
    s
}

/// 一个模型步骤的请求
pub fn request(skill: &Skill, step: &Step, run: &Run, books: &Books, sources: &[SourceText]) -> ModelRequest {
    let scope = run.scope.as_ref();
    let mut user = Vec::new();
    for input in &step.input {
        match input {
            Input::Question => {
                if let Some(q) = scope.and_then(|s| s.question.as_deref()) {
                    user.push(format!("## 问题\n{q}\n"));
                }
            }
            Input::Instruction => {
                if let Some(q) = scope.and_then(|s| s.instruction.as_deref()).filter(|q| !q.trim().is_empty()) {
                    user.push(format!("## 读者的要求\n{q}\n"));
                }
            }
            Input::Terms => {
                if let Some(s) = scope.filter(|s| !s.terms.is_empty()) {
                    user.push(format!("## 检索词\n{}\n", s.terms.join("；")));
                }
            }
            Input::Scope => user.push(original_section(run, books)),
            Input::Translation => user.push(translation_section(run, books)),
            Input::Cards => user.push(cards_section(run)),
            Input::Sources => user.push(sources_section(sources)),
        }
    }
    ModelRequest {
        purpose: Purpose::Concept,
        system: system(skill, step),
        user,
        schema: schema(step),
        max_tokens: step.max_output_tokens,
        typical_output_tokens: step.max_output_tokens / 3,
        web_searches: 0,
        tag: format!("{}/{}", skill.name, step.id),
    }
}

// ---------- 解析 ----------

#[derive(Clone, Debug, PartialEq)]
pub struct CardDraft {
    pub key: String,
    pub kind: Option<CardKind>,
    /// 模型写的种类（不认识时报给读者）
    pub kind_text: String,
    pub title: String,
    pub about: Vec<String>,
    pub sentences: Vec<Sentence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelationDraft {
    pub from: String,
    pub to: String,
    pub kind: Option<RelationKind>,
    pub note: Option<Sentence>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub cards: Vec<CardDraft>,
    pub relations: Vec<RelationDraft>,
    /// 读不懂、只能丢掉的部分
    pub problems: Vec<String>,
}

fn json_body(text: &str) -> Option<Value> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    if let Ok(v) = serde_json::from_str::<Value>(t.trim()) {
        return Some(v);
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    serde_json::from_str(&t[a..=b]).ok()
}

fn opt(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

pub fn parse_sentence(v: &Value) -> Result<Sentence, String> {
    let cat = v.get("category").and_then(Value::as_str).unwrap_or("");
    let category = Category::from_key(cat).ok_or_else(|| format!("类别「{cat}」不认识"))?;
    let text = opt(v, "text").or_else(|| opt(v, "quote")).ok_or("句子是空的")?;
    Ok(Sentence { category, text, paragraph: opt(v, "paragraph"), quote: opt(v, "quote"), zh: opt(v, "zh"), source: opt(v, "source"), check: Default::default() })
}

/// 解析模型的输出。整份读不懂返回错误（这一步重试）；个别读不懂的句子、卡片丢掉并记下来
pub fn parse(text: &str) -> Result<Output, String> {
    let v = json_body(text).ok_or("模型的输出不是 JSON")?;
    let mut out = Output::default();
    for (i, c) in v.get("cards").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let kind_text = c.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
        let mut sentences = Vec::new();
        for (j, s) in c.get("sentences").and_then(Value::as_array).into_iter().flatten().enumerate() {
            match parse_sentence(s) {
                Ok(s) => sentences.push(s),
                Err(e) => out.problems.push(format!("第 {} 张卡第 {} 句：{e}，丢掉", i + 1, j + 1)),
            }
        }
        out.cards.push(CardDraft {
            key: opt(c, "key").unwrap_or_else(|| format!("c{}", i + 1)),
            kind: CardKind::from_key(&kind_text),
            kind_text,
            title: opt(c, "title").unwrap_or_default(),
            about: c.get("about").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
            sentences,
        });
    }
    for r in v.get("relations").and_then(Value::as_array).into_iter().flatten() {
        out.relations.push(RelationDraft {
            from: opt(r, "from").unwrap_or_default(),
            to: opt(r, "to").unwrap_or_default(),
            kind: r.get("kind").and_then(Value::as_str).and_then(RelationKind::from_key),
            note: r.get("note").and_then(|n| parse_sentence(n).ok()),
        });
    }
    if v.get("cards").is_none() && v.get("relations").is_none() {
        return Err("模型的输出里没有 cards 和 relations".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_strict_and_shared() {
        let skill = crate::skill::builtin().into_iter().next().unwrap();
        let s = schema(&skill.steps[0]);
        assert_eq!(s["properties"]["cards"]["items"]["properties"]["kind"]["enum"], json!(["concept"]));
        let sentence = &s["properties"]["cards"]["items"]["properties"]["sentences"]["items"];
        assert_eq!(sentence["additionalProperties"], json!(false));
        assert_eq!(sentence["required"].as_array().unwrap().len(), sentence["properties"].as_object().unwrap().len());
        // 只出关系的一步：卡片种类不限（解析时再按这一步能出的筛）
        let rel = schema(&skill.steps[2]);
        assert_eq!(rel["properties"]["cards"]["items"]["properties"]["kind"]["enum"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn tolerant_parse() {
        let text = "```json\n{\"cards\":[{\"key\":\"c1\",\"kind\":\"concept\",\"title\":\"x\",\"about\":[],\"sentences\":[{\"category\":\"original\",\"text\":\"a\",\"paragraph\":\"I.i.1\",\"quote\":\"a\",\"zh\":null,\"source\":null},{\"category\":\"guess\",\"text\":\"b\"}]}],\"relations\":[]}\n```";
        let o = parse(text).unwrap();
        assert_eq!(o.cards.len(), 1);
        assert_eq!(o.cards[0].sentences.len(), 1);
        assert_eq!(o.problems.len(), 1);
        assert!(parse("not json").is_err());
        assert!(parse("{\"x\":1}").is_err());
    }
}
