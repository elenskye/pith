//! 技能（`decisions/0024`「Skill 与工作流」）：一个技能是一个文件夹，里面一个 `SKILL.md`。
//!
//! `SKILL.md` = 开头 `---` 之间的元数据 + 正文。元数据是 YAML 的一个小子集（只有「键: 值」、`[a, b]` 列表、
//! `- ` 开头的列表项，列表项可以是几行「键: 值」），格式写在 crate 的 README 里。正文里：
//! - `## 总则`：每一步都带上的做法；
//! - `## 步骤 <id>`：这一步的做法（模型步骤必须有）。
//!
//! 内置的在仓库 `skills/`（编进 App）；用户的在 `数据目录/skills/`，同名的覆盖内置的，界面上标明。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// 整章
    Chapter,
    /// 划选的段落
    Selection,
    /// 一个问题（本地搜出候选段落，用户勾选）
    Question,
    /// 一个概念（中英检索词，按章列出命中，用户勾选）
    Concept,
    /// 已有的卡片（反方、推演）
    Card,
}

impl Entry {
    pub const ALL: [Entry; 5] = [Entry::Chapter, Entry::Selection, Entry::Question, Entry::Concept, Entry::Card];

    pub fn key(self) -> &'static str {
        match self {
            Entry::Chapter => "chapter",
            Entry::Selection => "selection",
            Entry::Question => "question",
            Entry::Concept => "concept",
            Entry::Card => "card",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Entry::Chapter => "整章",
            Entry::Selection => "划选的段落",
            Entry::Question => "一个问题",
            Entry::Concept => "一个概念",
            Entry::Card => "卡片",
        }
    }

    fn from_key(s: &str) -> Option<Entry> {
        Entry::ALL.into_iter().find(|e| e.key() == s)
    }
}

/// 一步产出什么
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Produce {
    Concept,
    Argument,
    Comment,
    Counterfactual,
    Note,
    Relation,
}

impl Produce {
    pub const ALL: [Produce; 6] = [Produce::Concept, Produce::Argument, Produce::Comment, Produce::Counterfactual, Produce::Note, Produce::Relation];

    pub fn key(self) -> &'static str {
        match self {
            Produce::Concept => "concept",
            Produce::Argument => "argument",
            Produce::Comment => "comment",
            Produce::Counterfactual => "counterfactual",
            Produce::Note => "note",
            Produce::Relation => "relation",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Produce::Concept => "概念卡",
            Produce::Argument => "论证卡",
            Produce::Comment => "评论卡",
            Produce::Counterfactual => "推演卡",
            Produce::Note => "笔记",
            Produce::Relation => "关系",
        }
    }

    pub fn from_key(s: &str) -> Option<Produce> {
        Produce::ALL.into_iter().find(|p| p.key() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// 调一次模型
    Model,
    /// 查文献库（标题、摘要、笔记、标注、PDF 正文）
    Library,
    /// 网页检索（v0.3 的检索 + 本机下载 + 快照）
    Web,
}

/// 模型那一步要看什么
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// 范围里的原文段落
    Scope,
    /// 对齐的中译段落，和译本的旁注、注释、导读（「他人」）
    Translation,
    /// 这次已经做出来的卡片 + 入口选的卡片
    Cards,
    /// 查文献库、网页检索找到的来源正文
    Sources,
    Question,
    Instruction,
    Terms,
}

impl Input {
    fn from_key(s: &str) -> Option<Input> {
        Some(match s {
            "scope" => Input::Scope,
            "translation" => Input::Translation,
            "cards" => Input::Cards,
            "sources" => Input::Sources,
            "question" => Input::Question,
            "instruction" => Input::Instruction,
            "terms" => Input::Terms,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    pub title: String,
    pub produces: Vec<Produce>,
    pub input: Vec<Input>,
    /// 模型步骤：输出上限（推理 token 也算在内），最坏费用按它算
    pub max_output_tokens: u32,
    /// 用户可以不跑这一步
    pub optional: bool,
    /// 默认不跑（例如「也上网找」）
    pub off_by_default: bool,
    /// 查文献库 / 网页检索：最多几条
    pub max_results: u32,
    /// 这一步的做法（`## 步骤 <id>` 那一节）
    pub instructions: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Builtin,
    User,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub title: String,
    pub version: u32,
    pub description: String,
    pub entries: Vec<Entry>,
    pub produces: Vec<Produce>,
    pub steps: Vec<Step>,
    /// `## 总则`
    pub general: String,
    pub origin: Origin,
    /// 用户的技能覆盖了同名的内置技能
    pub overrides_builtin: bool,
    /// 要用户写一句要求（「自由」）
    pub needs_instruction: bool,
}

// ---------- 元数据：YAML 的小子集 ----------

#[derive(Clone, Debug, PartialEq)]
enum Yaml {
    Str(String),
    List(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Yaml::Str(s) => Some(s),
            _ => None,
        }
    }

    /// 列表；单个值也当一项
    fn list(&self) -> Vec<&Yaml> {
        match self {
            Yaml::List(l) => l.iter().collect(),
            Yaml::Str(s) if s.is_empty() => Vec::new(),
            other => vec![other],
        }
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

fn scalar(v: &str) -> Yaml {
    let v = v.trim();
    if v.starts_with('[') && v.ends_with(']') {
        let inner = &v[1..v.len() - 1];
        return Yaml::List(inner.split(',').map(unquote).filter(|x| !x.is_empty()).map(Yaml::Str).collect());
    }
    Yaml::Str(unquote(v))
}

struct Lines<'a> {
    lines: Vec<(usize, usize, &'a str)>, // (行号, 缩进, 内容)
    at: usize,
}

fn parse_yaml(src: &str, first_line: usize) -> Result<Yaml, String> {
    let lines: Vec<(usize, usize, &str)> = src
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(i, l)| (i + first_line, l.len() - l.trim_start().len(), l.trim_end()))
        .collect();
    if let Some((n, _, _)) = lines.iter().find(|(_, _, l)| l.contains('\t')) {
        return Err(format!("第 {n} 行有制表符：元数据只能用空格缩进。"));
    }
    let mut p = Lines { lines, at: 0 };
    let v = parse_map(&mut p, 0)?;
    if let Some((n, _, l)) = p.lines.get(p.at) {
        return Err(format!("第 {n} 行读不懂：{}", l.trim()));
    }
    Ok(v)
}

fn parse_map(p: &mut Lines, indent: usize) -> Result<Yaml, String> {
    let mut out = Vec::new();
    while let Some(&(n, ind, line)) = p.lines.get(p.at) {
        if ind < indent {
            break;
        }
        if ind > indent {
            return Err(format!("第 {n} 行的缩进不对。"));
        }
        let t = line.trim();
        if t.starts_with("- ") || t == "-" {
            break;
        }
        let Some((k, v)) = t.split_once(':') else { return Err(format!("第 {n} 行要写成「键: 值」：{t}")) };
        p.at += 1;
        let key = k.trim().to_string();
        let value = if v.trim().is_empty() {
            match p.lines.get(p.at) {
                Some(&(_, ind2, l2)) if ind2 > indent && l2.trim_start().starts_with('-') => parse_list(p, ind2)?,
                Some(&(_, ind2, _)) if ind2 > indent => parse_map(p, ind2)?,
                _ => Yaml::Str(String::new()),
            }
        } else {
            scalar(v)
        };
        if out.iter().any(|(x, _): &(String, Yaml)| *x == key) {
            return Err(format!("第 {n} 行：「{key}」写了两次。"));
        }
        out.push((key, value));
    }
    Ok(Yaml::Map(out))
}

fn parse_list(p: &mut Lines, indent: usize) -> Result<Yaml, String> {
    let mut out = Vec::new();
    while let Some(&(n, ind, line)) = p.lines.get(p.at) {
        if ind < indent {
            break;
        }
        let t = line.trim();
        if ind > indent || !(t.starts_with("- ") || t == "-") {
            return Err(format!("第 {n} 行的缩进不对。"));
        }
        let item = t.trim_start_matches('-').trim();
        if item.contains(':') && !item.starts_with('[') && !item.starts_with('"') {
            // 一个映射：第一对写在「- 」后面，其余的在下面几行，缩进到「- 」后面的位置
            let inner = ind + (line.trim_start().len() - line.trim_start().trim_start_matches('-').trim_start().len());
            p.lines[p.at] = (n, inner, &line[line.len() - item.len()..]);
            out.push(parse_map(p, inner)?);
        } else {
            p.at += 1;
            out.push(scalar(item));
        }
    }
    Ok(Yaml::List(out))
}

// ---------- SKILL.md ----------

/// 读一个 `SKILL.md`。`folder` 是文件夹名，要和元数据里的 name 一样
pub fn parse(folder: &str, src: &str, origin: Origin) -> Result<Skill, String> {
    let src = src.trim_start_matches('\u{FEFF}');
    let mut lines = src.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("第 1 行要是「---」：SKILL.md 开头是元数据。".into());
    }
    let rest: Vec<&str> = lines.collect();
    let end = rest.iter().position(|l| l.trim() == "---").ok_or("元数据没有用「---」结束。")?;
    let meta = parse_yaml(&rest[..end].join("\n"), 2)?;
    let body = rest[end + 1..].join("\n");
    let text = |k: &str| meta.get(k).and_then(Yaml::str).map(str::trim).unwrap_or("").to_string();
    let name = text("name");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || name.len() > 40 {
        return Err("name 要写，只能有小写英文字母、数字和 -，不超过 40 个字符。".into());
    }
    if name != folder {
        return Err(format!("name「{name}」要和文件夹名「{folder}」一样。"));
    }
    let title = text("title");
    if title.is_empty() || title.chars().count() > 30 {
        return Err("title（中文标题）要写，不超过 30 个字。".into());
    }
    let version: u32 = text("version").parse().map_err(|_| "version 要是一个正整数。".to_string())?;
    let list_of = |k: &str| -> Vec<String> { meta.get(k).map(|v| v.list().into_iter().filter_map(Yaml::str).map(|s| s.trim().to_string()).collect()).unwrap_or_default() };
    let mut entries = Vec::new();
    for e in list_of("entries") {
        entries.push(Entry::from_key(&e).ok_or_else(|| format!("entries 里的「{e}」不认识：只能是 chapter / selection / question / concept / card。"))?);
    }
    if entries.is_empty() {
        return Err("entries 至少写一个入口。".into());
    }
    let mut produces = Vec::new();
    for x in list_of("produces") {
        produces.push(Produce::from_key(&x).ok_or_else(|| format!("produces 里的「{x}」不认识：只能是 concept / argument / comment / counterfactual / note / relation。"))?);
    }
    let sections = sections(&body);
    let general = sections.iter().find(|(h, _)| h == "总则").map(|(_, b)| b.clone()).unwrap_or_default();
    let steps_yaml = meta.get("steps").map(|v| v.list()).unwrap_or_default();
    if steps_yaml.is_empty() {
        return Err("steps 至少写一步。".into());
    }
    let mut steps: Vec<Step> = Vec::new();
    for (i, s) in steps_yaml.iter().enumerate() {
        let get = |k: &str| s.get(k).and_then(Yaml::str).map(str::trim).unwrap_or("").to_string();
        let id = get("id");
        let at = format!("第 {} 步", i + 1);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(format!("{at}的 id 要写，只能有小写英文字母、数字、- 和 _。"));
        }
        if steps.iter().any(|x| x.id == id) {
            return Err(format!("步骤 id「{id}」重复了。"));
        }
        let kind = match get("kind").as_str() {
            "model" => StepKind::Model,
            "library" => StepKind::Library,
            "web" => StepKind::Web,
            other => return Err(format!("步骤「{id}」的 kind「{other}」不认识：只能是 model / library / web。")),
        };
        let words = |k: &str| -> Vec<String> { s.get(k).map(|v| v.list().into_iter().filter_map(Yaml::str).map(|x| x.trim().to_string()).collect()).unwrap_or_default() };
        let mut step_produces = Vec::new();
        for x in words("produces") {
            let p = Produce::from_key(&x).ok_or_else(|| format!("步骤「{id}」的 produces「{x}」不认识。"))?;
            if !produces.contains(&p) {
                return Err(format!("步骤「{id}」产出「{x}」，但技能的 produces 里没写。"));
            }
            step_produces.push(p);
        }
        let mut input = Vec::new();
        for x in words("input") {
            input.push(Input::from_key(&x).ok_or_else(|| format!("步骤「{id}」的 input「{x}」不认识：只能是 scope / translation / cards / sources / question / instruction / terms。"))?);
        }
        let flag = |k: &str| matches!(get(k).as_str(), "true" | "yes");
        let max_output_tokens: u32 = match get("max_output_tokens").as_str() {
            "" => 0,
            v => v.parse().map_err(|_| format!("步骤「{id}」的 max_output_tokens 要是整数。"))?,
        };
        let max_results: u32 = match get("max_results").as_str() {
            "" => 5,
            v => v.parse().map_err(|_| format!("步骤「{id}」的 max_results 要是整数。"))?,
        };
        let instructions = sections.iter().find(|(h, _)| *h == format!("步骤 {id}")).map(|(_, b)| b.clone()).unwrap_or_default();
        match kind {
            StepKind::Model => {
                if step_produces.is_empty() {
                    return Err(format!("模型步骤「{id}」要写 produces。"));
                }
                if !(1000..=64_000).contains(&max_output_tokens) {
                    return Err(format!("模型步骤「{id}」的 max_output_tokens 要在 1000 到 64000 之间。"));
                }
                if instructions.trim().is_empty() {
                    return Err(format!("模型步骤「{id}」在正文里没有「## 步骤 {id}」一节。"));
                }
                if input.is_empty() {
                    return Err(format!("模型步骤「{id}」要写 input。"));
                }
            }
            StepKind::Library | StepKind::Web => {
                if !step_produces.is_empty() {
                    return Err(format!("步骤「{id}」只找来源，不产出卡片：去掉 produces。"));
                }
                if !(1..=10).contains(&max_results) {
                    return Err(format!("步骤「{id}」的 max_results 要在 1 到 10 之间。"));
                }
            }
        }
        steps.push(Step { id, kind, title: get("title"), produces: step_produces, input, max_output_tokens, optional: flag("optional"), off_by_default: flag("off_by_default"), max_results, instructions });
    }
    if !steps.iter().any(|s| s.kind == StepKind::Model) {
        return Err("至少要有一个模型步骤。".into());
    }
    // 要不要用户写一句要求：`instruction: required`（自由）；`optional` 或不写 = 可写可不写（推演）
    let needs_instruction = match text("instruction").as_str() {
        "required" => true,
        "" | "optional" => false,
        other => return Err(format!("instruction「{other}」不认识：只能是 required 或 optional。")),
    };
    if needs_instruction && !steps.iter().any(|s| s.input.contains(&Input::Instruction)) {
        return Err("instruction: required，但没有一步的 input 里有 instruction。".into());
    }
    Ok(Skill { name, title, version, description: text("description"), entries, produces, steps, general, origin, overrides_builtin: false, needs_instruction })
}

/// 正文按「## 」分节：(标题, 内容)
fn sections(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            out.push((h.trim().to_string(), String::new()));
        } else if let Some((_, b)) = out.last_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    for (_, b) in &mut out {
        *b = b.trim().to_string();
    }
    out
}

/// 内置的七个技能（`skills/` 编进来）
pub const BUILTIN: [(&str, &str); 7] = [
    ("chapter", include_str!("../../../skills/chapter/SKILL.md")),
    ("passage", include_str!("../../../skills/passage/SKILL.md")),
    ("ask", include_str!("../../../skills/ask/SKILL.md")),
    ("trace", include_str!("../../../skills/trace/SKILL.md")),
    ("critics", include_str!("../../../skills/critics/SKILL.md")),
    ("counterfactual", include_str!("../../../skills/counterfactual/SKILL.md")),
    ("free", include_str!("../../../skills/free/SKILL.md")),
];

pub fn builtin() -> Vec<Skill> {
    BUILTIN.iter().map(|(f, s)| parse(f, s, Origin::Builtin).unwrap_or_else(|e| panic!("内置技能 {f} 格式不对：{e}"))).collect()
}

/// 读不了的用户技能
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Broken {
    pub folder: String,
    pub error: String,
}

/// 内置的 + 用户的（文件夹名, SKILL.md 的内容）：同名的用户技能覆盖内置的
pub fn catalog(user: &[(String, String)]) -> (Vec<Skill>, Vec<Broken>) {
    let mut skills = builtin();
    let mut broken = Vec::new();
    for (folder, src) in user {
        match parse(folder, src, Origin::User) {
            Ok(mut s) => {
                if let Some(i) = skills.iter().position(|b| b.name == s.name) {
                    s.overrides_builtin = skills[i].origin == Origin::Builtin;
                    skills[i] = s;
                } else {
                    skills.push(s);
                }
            }
            Err(error) => broken.push(Broken { folder: folder.clone(), error }),
        }
    }
    (skills, broken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_skills_parse() {
        let all = builtin();
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["chapter", "passage", "ask", "trace", "critics", "counterfactual", "free"]);
        let chapter = &all[0];
        assert_eq!(chapter.title, "整章分析");
        assert_eq!(chapter.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["concepts", "arguments", "relations"]);
        assert!(chapter.steps.iter().all(|s| s.optional));
        assert!(!chapter.general.is_empty());
        let critics = all.iter().find(|s| s.name == "critics").unwrap();
        assert_eq!(critics.steps.iter().map(|s| s.kind).collect::<Vec<_>>(), [StepKind::Library, StepKind::Web, StepKind::Model]);
        assert!(critics.steps[1].off_by_default);
        assert!(all.iter().find(|s| s.name == "free").unwrap().needs_instruction);
        assert!(!all.iter().find(|s| s.name == "counterfactual").unwrap().needs_instruction);
    }

    const OK: &str = "---\nname: my-skill\ntitle: 术语表\nversion: 2\ndescription: \"一章的术语表\"\nentries: [chapter, selection]\nproduces: [note]\nsteps:\n  - id: glossary\n    kind: model\n    produces: [note]\n    input: [scope, translation]\n    max_output_tokens: 4000\n---\n\n## 总则\n只写术语。\n\n## 步骤 glossary\n每个术语一张笔记。\n";

    #[test]
    fn user_skill_overrides_builtin_and_errors_are_reported() {
        let s = parse("my-skill", OK, Origin::User).unwrap();
        assert_eq!((s.version, s.entries.clone(), s.steps[0].max_output_tokens), (2, vec![Entry::Chapter, Entry::Selection], 4000));
        assert_eq!(s.steps[0].instructions, "每个术语一张笔记。");
        assert_eq!(s.description, "一章的术语表");
        let over = OK.replace("my-skill", "chapter");
        let (all, broken) = catalog(&[("chapter".into(), over), ("bad".into(), "no front matter".into()), ("my-skill".into(), OK.into())]);
        assert_eq!(all.len(), 8);
        let c = all.iter().find(|s| s.name == "chapter").unwrap();
        assert!(c.overrides_builtin && c.origin == Origin::User && c.title == "术语表");
        assert_eq!(broken.len(), 1);
        assert!(broken[0].error.contains("---"));
    }

    #[test]
    fn format_checks() {
        let bad = |from: &str, to: &str| parse("my-skill", &OK.replace(from, to), Origin::User).unwrap_err();
        assert!(bad("name: my-skill", "name: other").contains("文件夹名"));
        assert!(bad("entries: [chapter, selection]", "entries: [book]").contains("book"));
        assert!(bad("    kind: model", "    kind: shell").contains("shell"));
        assert!(bad("max_output_tokens: 4000", "max_output_tokens: 999999").contains("64000"));
        assert!(bad("## 步骤 glossary", "## 步骤 other").contains("没有「## 步骤 glossary」"));
        assert!(bad("    produces: [note]", "    produces: [concept]").contains("没写"));
        assert!(bad("version: 2", "version: two").contains("version"));
        assert!(bad("  - id: glossary", "\t- id: glossary").contains("制表符"));
        assert!(parse("x", "---\nname: x\n", Origin::User).unwrap_err().contains("结束"));
    }
}
