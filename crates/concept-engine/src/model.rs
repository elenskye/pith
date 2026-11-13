//! 一次概念研究的数据（`decisions/0024`「一次概念研究」「内容的分类与核对」）。整份可以存成 JSON。

use research_engine::fold::Span;
use research_engine::model::ModelRef;
use serde::{Deserialize, Serialize};

use crate::skill::Entry;

/// 每句的类别
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// 原著正文：引文必须在所挂段落里逐字找得到
    Original,
    /// 有出处的别人的话：后世评论、反方，译本的旁注、注释、导读
    Others,
    /// AI 的解释：尽量挂原文
    Explanation,
    /// 假设推演：不是原著观点
    Speculation,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Original, Category::Others, Category::Explanation, Category::Speculation];

    pub fn label(self) -> &'static str {
        match self {
            Category::Original => "原文",
            Category::Others => "他人",
            Category::Explanation => "解释",
            Category::Speculation => "推演",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Category::Original => "original",
            Category::Others => "others",
            Category::Explanation => "explanation",
            Category::Speculation => "speculation",
        }
    }

    pub fn from_key(s: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.key() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    Concept,
    Argument,
    Comment,
    Counterfactual,
    Note,
}

impl CardKind {
    pub const ALL: [CardKind; 5] = [CardKind::Concept, CardKind::Argument, CardKind::Comment, CardKind::Counterfactual, CardKind::Note];

    pub fn label(self) -> &'static str {
        match self {
            CardKind::Concept => "概念卡",
            CardKind::Argument => "论证卡",
            CardKind::Comment => "评论卡",
            CardKind::Counterfactual => "推演卡",
            CardKind::Note => "笔记",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CardKind::Concept => "concept",
            CardKind::Argument => "argument",
            CardKind::Comment => "comment",
            CardKind::Counterfactual => "counterfactual",
            CardKind::Note => "note",
        }
    }

    pub fn from_key(s: &str) -> Option<CardKind> {
        CardKind::ALL.into_iter().find(|c| c.key() == s)
    }
}

/// 核对的结果
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// 还没核
    #[default]
    Pending,
    /// 引文逐字找到了（原文、他人；带引文的解释）；挂的段落存在（解释）
    Verified,
    /// 解释没挂上原文（照样显示，醒目地标出）
    Unanchored,
    /// 推演：不要求锚点
    Speculative,
}

/// 中译那一边的核对
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZhStatus {
    /// 中译句在对齐的中文段落里逐字找到了
    Verified,
    /// 没给中译句，或者核不上：只显示整段译文
    ParagraphOnly,
    /// 这一段没有对齐的译文（这一章段数对不上，或没有译本）
    Unaligned,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub status: Status,
    /// 引文在所挂段落（或来源正文）里的位置
    pub span: Option<Span>,
    /// 引文的页码（有页码的版本）
    pub pages: Option<String>,
    /// 对齐的中文段落号
    pub zh_paragraph: Option<String>,
    pub zh_status: Option<ZhStatus>,
    pub zh_span: Option<Span>,
    /// 给读者看的说明（「没挂上原文」「段落号不存在，锚点去掉了」）
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub category: Category,
    /// 这一句（原文类就是引文本身）
    pub text: String,
    /// 挂的段落号：原著的「I.i.3」；译本的旁注、注释、导读写「译:I.i 注 1」
    pub paragraph: Option<String>,
    /// 逐字引文（原文类和 text 一样）
    pub quote: Option<String>,
    /// 原文句对应的中译句（逐字摘自对齐的中文段落）
    pub zh: Option<String>,
    /// 他人类：来源编号（S1）
    pub source: Option<String>,
    #[serde(default)]
    pub check: Check,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// K1、K2……一次研究里唯一
    pub id: String,
    pub kind: CardKind,
    pub title: String,
    pub sentences: Vec<Sentence>,
    /// 哪一步做的
    pub step: String,
    /// 评论、推演针对的卡片编号
    #[serde(default)]
    pub about: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Cause,
    Contains,
    Opposes,
    Example,
    Premise,
    Revises,
}

impl RelationKind {
    pub const ALL: [RelationKind; 6] = [RelationKind::Cause, RelationKind::Contains, RelationKind::Opposes, RelationKind::Example, RelationKind::Premise, RelationKind::Revises];

    pub fn label(self) -> &'static str {
        match self {
            RelationKind::Cause => "因果",
            RelationKind::Contains => "包含",
            RelationKind::Opposes => "对立",
            RelationKind::Example => "举例",
            RelationKind::Premise => "前提",
            RelationKind::Revises => "修正",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            RelationKind::Cause => "cause",
            RelationKind::Contains => "contains",
            RelationKind::Opposes => "opposes",
            RelationKind::Example => "example",
            RelationKind::Premise => "premise",
            RelationKind::Revises => "revises",
        }
    }

    pub fn from_key(s: &str) -> Option<RelationKind> {
        RelationKind::ALL.into_iter().find(|c| c.key() == s)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// R1、R2……
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
    /// 一句说明（也分类别、可以挂锚点）
    pub note: Sentence,
    pub step: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    /// 文献库里的一条文献
    Library,
    /// 网页检索 + 本机下载
    Web,
}

/// 评论的来源（S1、S2……）。正文存在快照里（`sha256`），核对和审计都从快照读
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentSource {
    pub id: String,
    pub origin: SourceOrigin,
    pub title: String,
    pub item_id: Option<i64>,
    pub attachment_id: Option<i64>,
    pub url: Option<String>,
    pub final_url: Option<String>,
    pub sha256: Option<String>,
    pub fetched_at: Option<String>,
    pub text_chars: usize,
    pub error: Option<String>,
    /// 网上的来源加入文献库后的文献
    pub added_item: Option<i64>,
}

/// 范围
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub entry: Entry,
    /// 原著文本的 uid
    pub text: String,
    /// 对齐的译本文本的 uid
    pub translation: Option<String>,
    /// 原著里的段落号（按书里的顺序）
    pub paragraphs: Vec<String>,
    /// 整章入口：章的引用名（I.iii）
    pub unit: Option<String>,
    pub question: Option<String>,
    /// 追踪概念：英文、中文检索词
    pub terms: Vec<String>,
    /// 入口选的卡片：（研究 uid, 卡片编号）
    pub cards: Vec<(String, String)>,
    /// 自由技能、推演：读者写的一句要求
    pub instruction: Option<String>,
    /// 不跑的步骤（可选的步骤）；默认不跑的步骤要写进 `with` 才跑
    pub skip: Vec<String>,
    pub with: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// 建好了，等确认
    #[default]
    Ready,
    Running,
    Cancelled,
    Failed,
    Done,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub id: String,
    pub label: String,
    pub at: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub searches: u64,
    pub cost_usd: f64,
    pub mock: bool,
    pub ok: bool,
    pub note: Option<String>,
    /// 模型这一步的原始输出（审计、出问题时看）
    pub raw: Option<String>,
}

/// 核对拦下的句子（不进卡片，列进核对报告）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub step: String,
    pub card: String,
    pub sentence: Sentence,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub uid: String,
    pub skill: String,
    pub skill_title: String,
    pub skill_version: u32,
    pub scope: Option<Scope>,
    pub status: RunStatus,
    /// 接下来做第几步（技能里的下标）
    pub next_step: usize,
    pub error: Option<String>,
    pub model: ModelRef,
    pub effort: String,
    pub mock: bool,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub steps: Vec<StepRecord>,
    pub cards: Vec<Card>,
    pub relations: Vec<Relation>,
    pub sources: Vec<CommentSource>,
    pub rejected: Vec<Rejected>,
    /// 入口选的卡片（复制一份，原来那次研究删了也看得懂）
    pub targets: Vec<Card>,
    /// 用同一技能对同一范围重跑时：上一次的 uid（旧结果保留）
    pub previous: Option<String>,
}

impl Run {
    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards.iter().chain(self.targets.iter()).find(|c| c.id == id)
    }

    /// 下一个卡片编号
    pub fn next_card_id(&self) -> String {
        let n = self.cards.iter().chain(self.targets.iter()).filter_map(|c| c.id.strip_prefix('K').and_then(|x| x.parse::<u32>().ok())).max().unwrap_or(0);
        format!("K{}", n + 1)
    }

    pub fn next_relation_id(&self) -> String {
        let n = self.relations.iter().filter_map(|r| r.id.strip_prefix('R').and_then(|x| x.parse::<u32>().ok())).max().unwrap_or(0);
        format!("R{}", n + 1)
    }

    pub fn next_source_id(&self) -> String {
        let n = self.sources.iter().filter_map(|s| s.id.strip_prefix('S').and_then(|x| x.parse::<u32>().ok())).max().unwrap_or(0);
        format!("S{}", n + 1)
    }

    pub fn cost_usd(&self) -> f64 {
        self.steps.iter().map(|s| s.cost_usd).sum()
    }
}
