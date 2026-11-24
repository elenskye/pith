//! 文档排版计划（Plan）：针对某一份文档的映射，与学校规范（Spec）分开。
//!
//! 真实论文里，正文常常不用「正文」样式（网页 / 其他软件粘贴来的 Normal (Web)、p1……），
//! 题注是手动居中的普通段落，封面靠直接格式排版。Spec 只描述「应该长什么样」，
//! Plan 描述「这份文档里哪些段落是什么角色、哪些地方不能动」。v0.2 由 Agent 分析文档后生成，用户确认。

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ParagraphRule {
    /// 匹配段落文字的正则（如 `^(图|表)\s*\d`）
    pub pattern: String,
    /// 文字超过这个长度的段落不算（区分「图 3 展示了……」这类正文句子）
    pub max_chars: Option<usize>,
    /// 套用的样式（内置名，如 caption）
    pub style: String,
}

/// 指向正文里的一个段落：序号（`outline` 的段落编号，从 1 起）加开头的文字。
/// 文字用来核对：序号处的段落不是以它开头就拒绝，免得文档变了以后切错地方。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub paragraph: usize,
    pub text: String,
}

/// 这份文档的分区与结构类排版的取舍（v0.2 W4）：页眉、页码要按「封面 / 前置部分 / 正文 / 正文之后」分别处理。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// 前置部分（摘要）从哪一段开始；它之前是封面、题名页
    pub front_start: Option<Anchor>,
    /// 正文（引言 / 绪论）从哪一段开始
    pub body_start: Option<Anchor>,
    /// 正文之后不要页眉的部分（如原创性声明）从哪一段开始；没有就不写
    pub back_start: Option<Anchor>,
    /// 双面印制（页眉分奇偶页）
    #[serde(default)]
    pub duplex: bool,
    /// 不排成三线表的表格（文档里第几个表格，从 1 起）
    #[serde(default)]
    pub skip_tables: Vec<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(default)]
    pub name: String,
    /// 样式名 → 目标样式名（都按名字匹配，不区分大小写），如 "Normal (Web)" → "Normal"
    #[serde(default)]
    pub style_map: BTreeMap<String, String>,
    /// 按文字识别段落角色（优先于 style_map）
    #[serde(default)]
    pub paragraph_rules: Vec<ParagraphRule>,
    /// 受保护的分节（从 1 开始，如封面所在节）：不清理、不映射，并冻结原有外观
    #[serde(default)]
    pub protect_sections: Vec<usize>,
    /// 分区与结构类排版的取舍
    #[serde(default)]
    pub layout: Layout,
    #[serde(skip)]
    compiled: Vec<Regex>,
}

impl Plan {
    pub fn from_json(s: &str) -> Result<Plan> {
        let mut plan: Plan = serde_json::from_str(s).map_err(|e| Error::Spec(format!("排版计划：{e}")))?;
        plan.compile()?;
        Ok(plan)
    }

    pub fn compile(&mut self) -> Result<()> {
        self.compiled = self.paragraph_rules.iter().map(|r| Regex::new(&r.pattern).map_err(|e| Error::Spec(format!("正则 {}：{e}", r.pattern)))).collect::<Result<_>>()?;
        if self.protect_sections.contains(&0) {
            return Err(Error::Spec("protect_sections 从 1 开始编号".into()));
        }
        let l = &self.layout;
        let order: Vec<usize> = [&l.front_start, &l.body_start, &l.back_start].into_iter().flatten().map(|a| a.paragraph).collect();
        if order.contains(&0) || order.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::Spec("分区的起点要按「前置部分 → 正文 → 正文之后」的顺序，段落编号从 1 开始、不能重复".into()));
        }
        self.style_map = self.style_map.iter().map(|(k, v)| (k.to_lowercase(), v.to_lowercase())).collect();
        Ok(())
    }

    /// 按文字匹配规则，返回目标样式名（小写）
    pub fn rule_for(&self, text: &str) -> Option<String> {
        let text = text.trim();
        self.paragraph_rules
            .iter()
            .zip(&self.compiled)
            .find(|(r, re)| r.max_chars.is_none_or(|m| text.chars().count() <= m) && re.is_match(text))
            .map(|(r, _)| r.style.to_lowercase())
    }

    pub fn is_protected(&self, section: usize) -> bool {
        self.protect_sections.contains(&section)
    }

    /// 计划里用到的目标样式名（小写，去重）
    pub fn targets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.style_map.values().cloned().chain(self.paragraph_rules.iter().map(|r| r.style.to_lowercase())).collect();
        v.sort();
        v.dedup();
        v
    }
}
