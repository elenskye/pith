//! 中英对齐（`decisions/0024`「中英对齐」）：同一章里正文段数相同，就按顺序一段对一段；段数不同的章不对齐，标出来。
//! 章按（第几篇, 第几章）对上；不在章里的正文单元（引言）按引用名对上。v0.4 不做手动对齐。

use serde::{Deserialize, Serialize};

use crate::text::{Role, Text, UnitKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitAlignment {
    /// 原著那边的单元引用名（I.iii）
    pub key: String,
    pub label: String,
    pub other_label: String,
    pub unit: usize,
    pub other_unit: Option<usize>,
    pub count: usize,
    pub other_count: usize,
    pub aligned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alignment {
    pub units: Vec<UnitAlignment>,
    /// （原著段 seq, 译本段 seq）
    pub pairs: Vec<(usize, usize)>,
}

impl Alignment {
    pub fn other_of(&self, seq: usize) -> Option<usize> {
        self.pairs.iter().find(|(a, _)| *a == seq).map(|(_, b)| *b)
    }

    pub fn original_of(&self, other: usize) -> Option<usize> {
        self.pairs.iter().find(|(_, b)| *b == other).map(|(a, _)| *a)
    }

    /// 段数对不上的章
    pub fn mismatched(&self) -> Vec<&UnitAlignment> {
        self.units.iter().filter(|u| !u.aligned).collect()
    }
}

/// 一个单元在两个版本之间对得上的「身份」
fn identity(t: &Text, unit: usize) -> Option<String> {
    let u = &t.units[unit];
    match u.kind {
        UnitKind::Chapter => Some(format!("c:{}:{}", u.book.unwrap_or(0), u.chapter?)),
        UnitKind::Part | UnitKind::Book | UnitKind::Container => {
            let intro = u.key.rsplit('.').next().is_some_and(|k| k == "Intro");
            intro.then(|| format!("intro:{}", u.book.unwrap_or(0)))
        }
        _ => None,
    }
}

pub fn align(original: &Text, other: &Text) -> Alignment {
    let mut out = Alignment::default();
    for ch in original.chapters() {
        let Some(id) = identity(original, ch.index) else { continue };
        let a: Vec<usize> = original.body_of(ch.index).map(|p| p.seq).collect();
        let found = other.chapters().into_iter().find(|u| identity(other, u.index).as_deref() == Some(id.as_str()));
        let b: Vec<usize> = found.map(|u| other.body_of(u.index).map(|p| p.seq).collect()).unwrap_or_default();
        let aligned = found.is_some() && !a.is_empty() && a.len() == b.len();
        if aligned {
            out.pairs.extend(a.iter().copied().zip(b.iter().copied()));
        }
        out.units.push(UnitAlignment {
            key: ch.key.clone(),
            label: ch.label.clone(),
            other_label: found.map(|u| u.label.clone()).unwrap_or_default(),
            unit: ch.index,
            other_unit: found.map(|u| u.index),
            count: a.len(),
            other_count: b.len(),
            aligned,
        });
    }
    out
}

/// 对齐的中文段（`seq` 是原著的正文段）；不是正文或没对上的返回 None
pub fn counterpart<'a>(alignment: &Alignment, original: &Text, other: &'a Text, seq: usize) -> Option<&'a crate::text::Paragraph> {
    if original.paragraphs.get(seq)?.role != Role::Body {
        return None;
    }
    other.paragraphs.get(alignment.other_of(seq)?)
}
