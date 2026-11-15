//! 重新解析之后，给旧卡片的每一句重新找锚点。
//!
//! 一份文本重新解析后段落号会变（前面多一段、一段拆成两段、单元归类变了）。存下的锚点是「段落号 + 位置」，
//! 这里按**内容**把它们搬到新文本上，再用 `verify::check` 在新文本里重新核对一遍（位置、页码、中译都由它重算），
//! 所以做完以后 `audit` 的结论和新做一次研究的一样。
//!
//! 一个锚点怎么找（`relocate`）：
//!
//! 1. 原来挂的那一段在新文本里原样还在（折叠后文字相同）：跟着那一段走。先看段落号相同的，再看同一单元，再看全书；
//!    有多处一样的，取离原位置最近的。
//! 2. 那一段的文字变了（拆开、合并、改了）：有逐字引文的按引文找，顺序同上（段落号相同的新段落 → 同一单元 → 全书，
//!    多处时取离原位置最近的）；没有引文的（解释句只挂段落）就算没挂上。
//! 3. 都找不到：去掉锚点，状态改成「没挂上原文」，`note` 写明是重新解析后找不到的、原来挂在哪一段。
//!
//! 查找只用概念研究的折叠（`fold`）做子串查找，和 `verify` 是同一套，不更宽松。能挂的角色也照 `verify` 的规则：
//! 原文句和推演的前提只能挂原著正文；他人句只能挂旁注、注释、导读；解释句不限。
//! 挂来源的他人句（S1）、本来就没有锚点的句子不动。

use std::cell::OnceCell;

use serde::{Deserialize, Serialize};

use crate::fold::fold;
use crate::model::{Card, Category, Check, Run, Sentence, Status, ZhStatus};
use crate::prompts::{Books, TRANSLATION_PREFIX};
use crate::skill::Entry;
use crate::text::{Paragraph, Role, Text};
use crate::verify::{self, SourceLookup};

/// 重新解析后找不到锚点时，`note` 的开头（下次重新解析凭它再试一次）
pub const LOST_PREFIX: &str = "重新解析后";

/// 一本书折叠后的段落（整本只折叠一次，用到才算）
struct Folds<'a> {
    text: &'a Text,
    folded: OnceCell<Vec<String>>,
}

impl<'a> Folds<'a> {
    fn new(text: &'a Text) -> Folds<'a> {
        Folds { text, folded: OnceCell::new() }
    }

    fn of(&self, seq: usize) -> &str {
        let all = self.folded.get_or_init(|| self.text.paragraphs.iter().map(|p| fold(&p.text)).collect());
        all.get(seq).map(String::as_str).unwrap_or("")
    }
}

/// 旧文本的一个单元在新文本里是哪个：引用名相同的；没有就找同一篇同一章
fn unit_in(old: &Text, new: &Text, unit: usize) -> Option<usize> {
    let u = old.units.get(unit)?;
    if let Some(n) = new.units.iter().find(|n| n.key == u.key && n.kind == u.kind) {
        return Some(n.index);
    }
    u.chapter?;
    new.units.iter().find(|n| n.kind == u.kind && n.book == u.book && n.chapter == u.chapter).map(|n| n.index)
}

/// 段落在它的单元里排第几（从 0 起）
fn rank(t: &Text, p: &Paragraph) -> usize {
    t.paragraphs.iter().filter(|x| x.unit == p.unit && x.seq < p.seq).count()
}

/// 全书只有一处命中时才算数（不知道原位置时用）
fn unique<'a>(new: &'a Text, hit: &dyn Fn(&Paragraph) -> bool) -> Option<&'a Paragraph> {
    let mut all = new.paragraphs.iter().filter(|p| hit(p));
    let first = all.next()?;
    all.next().is_none().then_some(first)
}

/// 在新文本里按顺序找：段落号相同的 → 同一单元里离原位置最近的 → 全书离原位置最近的。
/// `at` 是原来挂的那一段（旧文本里的）；不知道原位置时（`at` 为空）只认全书唯一的一处
fn search<'a>(old: &Text, new: &'a Text, key: &str, at: Option<&Paragraph>, hit: &dyn Fn(&Paragraph) -> bool) -> Option<&'a Paragraph> {
    if let Some(p) = new.paragraph(key).filter(|p| hit(p)) {
        return Some(p);
    }
    let Some(at) = at else { return unique(new, hit) };
    if let Some(unit) = unit_in(old, new, at.unit) {
        let was = rank(old, at);
        // 距离一样时取靠前的（`min_by_key` 取第一个最小的）
        if let Some(p) = new.paragraphs.iter().filter(|p| p.unit == unit && hit(p)).min_by_key(|p| rank(new, p).abs_diff(was)) {
            return Some(p);
        }
    }
    new.paragraphs.iter().filter(|p| hit(p)).min_by_key(|p| p.seq.abs_diff(at.seq))
}

/// 给定旧锚点（旧文本里的段落号 `key`，和逐字引文）和新文本，重新定位：返回新文本里该挂的那一段。
/// `allow` 是这一句能挂的角色。规则见文件头
pub fn relocate<'a>(old: &Text, new: &'a Text, key: &str, quote: Option<&str>, allow: &dyn Fn(Role) -> bool) -> Option<&'a Paragraph> {
    locate_in(old, &Folds::new(new), key, quote, allow)
}

fn locate_in<'a>(old: &Text, new: &Folds<'a>, key: &str, quote: Option<&str>, allow: &dyn Fn(Role) -> bool) -> Option<&'a Paragraph> {
    let at = old.paragraph(key);
    let needle = quote.map(fold).filter(|q| !q.is_empty());
    let has = |p: &Paragraph| needle.as_deref().is_none_or(|q| new.of(p.seq).contains(q));
    // 1. 原来那一段原样还在：跟着它走
    if let Some(at) = at {
        let same = fold(&at.text);
        if !same.is_empty()
            && let Some(p) = search(old, new.text, key, Some(at), &|p| allow(p.role) && new.of(p.seq) == same && has(p))
        {
            return Some(p);
        }
    }
    // 2. 按引文找
    needle.as_ref()?;
    search(old, new.text, key, at, &|p| allow(p.role) && has(p))
}

/// 一句重新找锚点的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// 原位跟过去了（段落号、位置、页码都没变）
    Kept,
    /// 找到了，段落号、位置或页码变了
    Moved,
    /// 没找到：没挂上原文
    Lost,
}

/// 有变化的一句（原位跟过去、中译也没变的不列）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// 卡片编号（K3）或关系编号（R2）
    pub at: String,
    /// 卡片标题；关系是「K1 → K2」
    pub title: String,
    /// 第几句（从 1 起）；关系的说明是空
    pub sentence: Option<usize>,
    pub outcome: Outcome,
    /// 原来的段落号、现在的段落号（译本里的带「译:」）
    pub from: Option<String>,
    pub to: Option<String>,
    /// 中译句原来核对通过，现在核不上了
    pub zh_lost: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kept: usize,
    pub moved: usize,
    pub lost: usize,
    pub zh_lost: usize,
    /// 研究范围里找不到了的段落（范围里的段落号也跟着换）
    pub scope_dropped: usize,
    pub changes: Vec<Change>,
}

/// 这一句能挂哪些角色（同 `verify::check`）
fn allowed(s: &Sentence) -> fn(Role) -> bool {
    match s.category {
        Category::Original => |r| r == Role::Body,
        Category::Speculation if s.quote.is_some() => |r| r == Role::Body,
        Category::Others => |r| r != Role::Body && r != Role::Heading,
        _ => |_| true,
    }
}

fn split_key(key: &str) -> (&str, bool) {
    let key = key.trim();
    match key.strip_prefix(TRANSLATION_PREFIX).or_else(|| key.strip_prefix("译：")) {
        Some(k) => (k.trim(), true),
        None => (key, false),
    }
}

/// 一次重新解析：旧的书（存下的锚点指着它）和新的书。重新解析的可能是原著，也可能是译本，没变的那一本两边是同一份
pub struct Reanchor<'a> {
    old: Books<'a>,
    new: Books<'a>,
    original: Folds<'a>,
    translation: Option<Folds<'a>>,
}

impl<'a> Reanchor<'a> {
    pub fn new(old: Books<'a>, new: Books<'a>) -> Reanchor<'a> {
        Reanchor { old, new, original: Folds::new(new.original), translation: new.translation.map(Folds::new) }
    }

    /// 一句。返回 None：这一句不挂在书上（挂来源的他人句、本来就没有锚点的），没动
    pub fn sentence(&self, s: &mut Sentence, sources: SourceLookup) -> Option<Outcome> {
        if s.category == Category::Others && s.source.as_deref().is_some_and(|x| !x.trim().is_empty()) {
            return None;
        }
        let quote = match s.category {
            Category::Original => Some(s.quote.clone().unwrap_or_else(|| s.text.clone())),
            _ => s.quote.clone(),
        };
        let allow = allowed(s);
        let found: Option<(String, bool)> = match s.paragraph.clone() {
            Some(full) => {
                let (key, in_translation) = split_key(&full);
                let books = if in_translation { self.old.translation.zip(self.translation.as_ref()) } else { Some((self.old.original, &self.original)) };
                books.and_then(|(old, new)| locate_in(old, new, key, quote.as_deref(), &allow)).map(|p| (p.key.clone(), in_translation))
            }
            // 上次重新解析没找到的：这次再试，只认唯一的一处
            None if s.check.note.as_deref().is_some_and(|n| n.starts_with(LOST_PREFIX)) && quote.is_some() => self.retry(s, quote.as_deref(), &allow),
            None => return None,
        };
        let before = s.clone();
        let was = before.paragraph.clone();
        let lose = |s: &mut Sentence, why: String| {
            // 上次就没找到、这次还是没找到的：原样留着（说明里是最早的段落号）
            if was.is_none() {
                *s = before.clone();
                return Some(Outcome::Lost);
            }
            s.paragraph = None;
            let status = if s.category == Category::Speculation && s.quote.is_none() { Status::Speculative } else { Status::Unanchored };
            s.check = Check { status, note: Some(why), ..Check::default() };
            Some(Outcome::Lost)
        };
        let from = was.as_deref().unwrap_or("");
        let Some((key, in_translation)) = found else {
            let why = if quote.is_some() { format!("{LOST_PREFIX}引文在新文本里找不到了（原来挂在「{from}」）：没挂上原文") } else { format!("{LOST_PREFIX}原来挂的段落「{from}」找不到了（文字变了或被删了）：没挂上原文") };
            return lose(s, why);
        };
        s.paragraph = Some(format!("{}{key}", if in_translation { TRANSLATION_PREFIX } else { "" }));
        match verify::check(s, &self.new, sources) {
            Err(why) => lose(s, format!("{LOST_PREFIX}核不上了（原来挂在「{from}」）：{why}")),
            Ok(()) if s.paragraph.is_none() && was.is_some() => {
                let why = s.check.note.clone().unwrap_or_default();
                lose(s, format!("{LOST_PREFIX}核不上了（原来挂在「{from}」）：{why}"))
            }
            Ok(()) => Some(if s.paragraph == before.paragraph && s.check.span == before.check.span && s.check.pages == before.check.pages && s.check.status == before.check.status { Outcome::Kept } else { Outcome::Moved }),
        }
    }

    /// 上次没找到的句子：不知道原位置，在这一类句子能挂的书里找唯一的一处
    fn retry(&self, s: &Sentence, quote: Option<&str>, allow: &dyn Fn(Role) -> bool) -> Option<(String, bool)> {
        let needle = quote.map(fold).filter(|q| !q.is_empty())?;
        let find = |f: &Folds<'a>, in_translation: bool| unique(f.text, &|p| allow(p.role) && f.of(p.seq).contains(&needle)).map(|p| (p.key.clone(), in_translation));
        let original = || find(&self.original, false);
        let translation = || self.translation.as_ref().and_then(|f| find(f, true));
        match s.category {
            Category::Others => translation().or_else(original),
            Category::Explanation => original().or_else(translation),
            _ => original(),
        }
    }

    fn record(&self, report: &mut Report, at: &str, title: &str, index: Option<usize>, before: &Sentence, after: &Sentence, outcome: Outcome) {
        match outcome {
            Outcome::Kept => report.kept += 1,
            Outcome::Moved => report.moved += 1,
            Outcome::Lost => report.lost += 1,
        }
        let zh_lost = before.check.zh_status == Some(ZhStatus::Verified) && after.check.zh_status != Some(ZhStatus::Verified) && outcome != Outcome::Lost;
        if zh_lost {
            report.zh_lost += 1;
        }
        if outcome != Outcome::Kept || zh_lost {
            report.changes.push(Change { at: at.to_string(), title: title.to_string(), sentence: index, outcome, from: before.paragraph.clone(), to: after.paragraph.clone(), zh_lost, note: after.check.note.clone() });
        }
    }

    /// 入口选的卡片是复制的一份（可能来自别的研究）：找得到的跟过去，找不到的原样留着，不计数
    fn targets(&self, cards: &mut [Card], sources: SourceLookup) {
        for s in cards.iter_mut().flat_map(|c| c.sentences.iter_mut()) {
            let before = s.clone();
            if self.sentence(s, sources) == Some(Outcome::Lost) {
                *s = before;
            }
        }
    }

    /// 研究的范围：整章入口按新文本里这一章的正文段重排；别的入口，段落原样还在的换成新段落号，不在了的去掉
    fn scope(&self, run: &mut Run, report: &mut Report) {
        let (old, new) = (self.old.original, self.new.original);
        // 原著没变（重新解析的是译本）：范围不用动
        if std::ptr::eq(old, new) {
            return;
        }
        let Some(scope) = run.scope.as_mut() else { return };
        let unit = scope.unit.as_deref().and_then(|k| old.units.iter().find(|u| u.key == k)).and_then(|u| unit_in(old, new, u.index));
        if let Some(u) = unit {
            scope.unit = Some(new.units[u].key.clone());
            if scope.entry == Entry::Chapter {
                scope.paragraphs = new.body_of(u).map(|p| p.key.clone()).collect();
                return;
            }
        }
        let mut kept: Vec<String> = Vec::new();
        for key in &scope.paragraphs {
            match locate_in(old, &self.original, key, None, &|_| true) {
                Some(p) if !kept.contains(&p.key) => kept.push(p.key.clone()),
                Some(_) => {}
                None => report.scope_dropped += 1,
            }
        }
        scope.paragraphs = kept;
    }

    /// 一次研究的每张卡片、每条关系的说明、入口选的卡片、范围
    pub fn run(&self, run: &mut Run, sources: SourceLookup) -> Report {
        let mut report = Report::default();
        for card in &mut run.cards {
            for (i, s) in card.sentences.iter_mut().enumerate() {
                let before = s.clone();
                if let Some(outcome) = self.sentence(s, sources) {
                    self.record(&mut report, &card.id, &card.title, Some(i + 1), &before, s, outcome);
                }
            }
        }
        for r in &mut run.relations {
            let before = r.note.clone();
            if let Some(outcome) = self.sentence(&mut r.note, sources) {
                self.record(&mut report, &r.id, &format!("{} → {}", r.from, r.to), None, &before, &r.note, outcome);
            }
        }
        self.targets(&mut run.targets, sources);
        self.scope(run, &mut report);
        report
    }
}

/// 给一次研究重新找锚点（`old`：重新解析之前的书；`new`：之后的）
pub fn reanchor(run: &mut Run, old: Books, new: Books, sources: SourceLookup) -> Report {
    Reanchor::new(old, new).run(run, sources)
}
