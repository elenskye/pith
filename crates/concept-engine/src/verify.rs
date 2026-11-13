//! 核对（`decisions/0024`「内容的分类与核对」）：模型给的每一句按类别查锚点和引文，核不上的拦下。
//!
//! | 类别 | 锚点 | 核不上时 |
//! | --- | --- | --- |
//! | 原文 | 必须挂原著的正文段；文字就是引文，在那一段里逐字找得到 | 整句拦下，列进核对报告 |
//! | 他人 | 必须有出处：来源（S1）或译本 / 原著里的旁注、注释、导读；引文在那里逐字找得到 | 同上 |
//! | 解释 | 尽量有；挂的段落号必须存在，带引文的引文也要找得到 | 去掉锚点照样显示，标「没挂上原文」 |
//! | 推演 | 不要求；引原文当前提的，前提那句按原文核对 | 前提核不上就拦下 |
//!
//! 逐字核对用概念研究的折叠（`fold`：v0.3 的规则 + 引号样式 + 拉丁字母大小写）。原文句的中译在对齐的中文段落里核对，
//! 核不上只显示整段译文。

use crate::align;
use crate::fold::Folded;
use crate::model::{Card, CardKind, Category, Check, Rejected, Relation, Run, Sentence, Status, ZhStatus};
use crate::prompts::{Books, CardDraft, Output, TRANSLATION_PREFIX};
use crate::skill::{Produce, Step};
use crate::text::{Paragraph, Role, Text};

/// 来源编号 → 存下的正文
pub type SourceLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Body => "原著正文",
        Role::Heading => "标题",
        Role::Sidenote => "旁注",
        Role::Note => "注释",
        Role::Guide => "导读",
        Role::Matter => "前后附",
    }
}

/// 一个引用名指向哪本书的哪一段
fn locate<'a>(books: &Books<'a>, key: &str) -> Option<(&'a Text, &'a Paragraph, bool)> {
    let key = key.trim();
    if let Some(k) = key.strip_prefix(TRANSLATION_PREFIX).or_else(|| key.strip_prefix("译：")) {
        let t = books.translation?;
        return t.paragraph(k.trim()).map(|p| (t, p, true));
    }
    books.original.paragraph(key).map(|p| (books.original, p, false))
}

/// 引文在全书正文里还出现在哪一段（挂错段落时提示）
fn found_elsewhere(text: &Text, quote: &str, except: usize) -> Option<String> {
    text.paragraphs.iter().filter(|p| p.seq != except && p.role == Role::Body).find(|p| Folded::new(&p.text).find(quote).is_some()).map(|p| p.key.clone())
}

fn same(a: &str, b: &str) -> bool {
    crate::fold::fold(a) == crate::fold::fold(b)
}

/// 原文类（和推演的前提）：挂原著正文段，引文逐字找得到。返回位置和页码
fn check_original(s: &mut Sentence, books: &Books, what: &str) -> Result<(), String> {
    let Some(key) = s.paragraph.clone() else { return Err(format!("标成{what}却没有锚点（没写段落号）")) };
    let quote = s.quote.clone().unwrap_or_else(|| s.text.clone());
    let Some((text, p, in_translation)) = locate(books, &key) else { return Err(format!("段落号「{key}」不存在")) };
    if in_translation {
        return Err(format!("「{key}」是译本里的段落：原文只能挂原著正文"));
    }
    if p.role != Role::Body {
        return Err(format!("「{key}」是{}，不是原著正文，不能标成原文", role_name(p.role)));
    }
    let Some((span, _)) = Folded::new(&p.text).find(&quote) else {
        return Err(match found_elsewhere(text, &quote, p.seq) {
            Some(other) => format!("引文不在「{key}」里（在「{other}」里找得到：挂错段落）"),
            None => format!("引文在「{key}」里逐字找不到"),
        });
    };
    s.quote = Some(quote);
    s.paragraph = Some(p.key.clone());
    s.check.span = Some(span);
    s.check.pages = text.pages_of(p.seq, span.start, span.end);
    // 中译句：在对齐的中文段落里核对
    if let (Some(zh), Some(al)) = (books.translation, books.alignment) {
        match align::counterpart(al, books.original, zh, p.seq) {
            Some(z) => {
                s.check.zh_paragraph = Some(z.key.clone());
                match s.zh.as_deref().and_then(|q| Folded::new(&z.text).find(q)) {
                    Some((zspan, _)) => {
                        s.check.zh_status = Some(ZhStatus::Verified);
                        s.check.zh_span = Some(zspan);
                    }
                    None => {
                        s.check.zh_status = Some(ZhStatus::ParagraphOnly);
                        if s.zh.is_some() {
                            s.check.note = Some("中译句在对齐的段落里找不到，只显示整段译文".into());
                        }
                    }
                }
            }
            None => s.check.zh_status = Some(ZhStatus::Unaligned),
        }
    }
    Ok(())
}

/// 核对一句。返回 Err 的整句拦下（原因给读者看）
pub fn check(s: &mut Sentence, books: &Books, sources: SourceLookup) -> Result<(), String> {
    s.check = Check::default();
    if s.text.trim().is_empty() {
        return Err("句子是空的".into());
    }
    match s.category {
        Category::Original => {
            // 原文句的文字就是引文：文字和引文不一致的（推演、概括混进原文类）拦下
            if let Some(q) = s.quote.as_deref()
                && !same(q, &s.text)
            {
                return Err("标成原文，但文字和引文不一致：原文类只能是逐字引文".into());
            }
            check_original(s, books, "原文")?;
            s.check.status = Status::Verified;
        }
        Category::Others => {
            let quote = s.quote.clone().ok_or("他人的话没有逐字引文")?;
            if let Some(id) = s.source.clone().filter(|x| !x.trim().is_empty()) {
                let text = sources(id.trim()).ok_or_else(|| format!("来源「{id}」不存在"))?;
                let (span, _) = Folded::new(&text).find(&quote).ok_or_else(|| format!("引文在来源「{id}」的正文里逐字找不到"))?;
                s.check.span = Some(span);
            } else if let Some(key) = s.paragraph.clone() {
                let (_, p, in_translation) = locate(books, &key).ok_or_else(|| format!("段落号「{key}」不存在"))?;
                if p.role == Role::Body || p.role == Role::Heading {
                    return Err(format!("「{key}」是{}{}：原著的话要标成原文，不是他人", if in_translation { "译本里的" } else { "" }, role_name(p.role)));
                }
                let (span, _) = Folded::new(&p.text).find(&quote).ok_or_else(|| format!("引文在「{key}」里逐字找不到"))?;
                s.check.span = Some(span);
                s.paragraph = Some(format!("{}{}", if in_translation { TRANSLATION_PREFIX } else { "" }, p.key));
            } else {
                return Err("他人的观点没有来源".into());
            }
            s.check.status = Status::Verified;
        }
        Category::Explanation => {
            s.check.status = Status::Verified;
            match s.paragraph.clone() {
                None => {
                    s.check.status = Status::Unanchored;
                    s.check.note = Some("没挂上原文".into());
                }
                Some(key) => match locate(books, &key) {
                    None => {
                        s.paragraph = None;
                        s.quote = None;
                        s.check.status = Status::Unanchored;
                        s.check.note = Some(format!("段落号「{key}」不存在，锚点去掉了：没挂上原文"));
                    }
                    Some((text, p, in_translation)) => {
                        s.paragraph = Some(format!("{}{}", if in_translation { TRANSLATION_PREFIX } else { "" }, p.key));
                        if let Some(q) = s.quote.clone() {
                            match Folded::new(&p.text).find(&q) {
                                Some((span, _)) => {
                                    s.check.span = Some(span);
                                    if !in_translation {
                                        s.check.pages = text.pages_of(p.seq, span.start, span.end);
                                    }
                                }
                                None => {
                                    s.quote = None;
                                    s.paragraph = None;
                                    s.check.status = Status::Unanchored;
                                    s.check.note = Some(format!("引文在「{key}」里逐字找不到，锚点去掉了：没挂上原文"));
                                }
                            }
                        } else if !in_translation {
                            s.check.pages = text.paragraph_pages(p.seq);
                        }
                    }
                },
            }
        }
        Category::Speculation => {
            if s.quote.is_some() {
                check_original(s, books, "推演的前提")?;
            } else {
                s.paragraph = s.paragraph.clone().filter(|k| locate(books, k).is_some());
            }
            s.check.status = Status::Speculative;
        }
    }
    Ok(())
}

/// 这一步能出的卡片种类
fn allowed(step: &Step, kind: CardKind) -> bool {
    step.produces.iter().any(|p| p.key() == kind.key())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Admitted {
    pub cards: usize,
    pub relations: usize,
    pub rejected: usize,
}

/// 把一步的输出核对后放进这次研究：核不上的句子拦下；一句都不剩的卡片不要；关系的两头要都在
pub fn admit(run: &mut Run, step: &Step, out: Output, books: &Books, sources: SourceLookup) -> Admitted {
    let mut result = Admitted::default();
    let mut keymap: Vec<(String, String)> = Vec::new();
    let reject = |run: &mut Run, card: &str, sentence: Sentence, reason: String| {
        run.rejected.push(Rejected { step: step.id.clone(), card: card.to_string(), sentence, reason });
    };
    for problem in &out.problems {
        reject(run, "", Sentence { category: Category::Explanation, text: String::new(), paragraph: None, quote: None, zh: None, source: None, check: Check::default() }, problem.clone());
        result.rejected += 1;
    }
    for CardDraft { key, kind, kind_text, title, about, sentences } in out.cards {
        let title = if title.trim().is_empty() { "（没写标题）".to_string() } else { title };
        let Some(kind) = kind.filter(|k| allowed(step, *k)) else {
            let why = if kind.is_none() { format!("卡片种类「{kind_text}」不认识") } else { format!("这一步不出{}", kind.map(|k| k.label()).unwrap_or("")) };
            for s in sentences {
                reject(run, &title, s, why.clone());
                result.rejected += 1;
            }
            continue;
        };
        let mut kept = Vec::new();
        for mut s in sentences {
            match check(&mut s, books, sources) {
                Ok(()) => kept.push(s),
                Err(why) => {
                    reject(run, &title, s, why);
                    result.rejected += 1;
                }
            }
        }
        if kept.is_empty() {
            continue;
        }
        let id = run.next_card_id();
        keymap.push((key, id.clone()));
        let about = about.into_iter().filter(|a| run.card(a).is_some()).collect();
        run.cards.push(Card { id, kind, title, sentences: kept, step: step.id.clone(), about });
        result.cards += 1;
    }
    if !step.produces.contains(&Produce::Relation) && !out.relations.is_empty() {
        for r in &out.relations {
            if let Some(n) = r.note.clone() {
                reject(run, &format!("关系 {} → {}", r.from, r.to), n, "这一步不画关系".into());
            }
            result.rejected += 1;
        }
        return result;
    }
    let resolve = |run: &Run, k: &str| -> Option<String> { keymap.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone()).or_else(|| run.card(k).map(|c| c.id.clone())) };
    for r in out.relations {
        let label = format!("关系 {} → {}", r.from, r.to);
        let blank = Sentence { category: Category::Explanation, text: String::new(), paragraph: None, quote: None, zh: None, source: None, check: Check::default() };
        let (Some(from), Some(to)) = (resolve(run, &r.from), resolve(run, &r.to)) else {
            reject(run, &label, r.note.unwrap_or(blank), "关系的一头不是这次研究里的卡片".into());
            result.rejected += 1;
            continue;
        };
        let Some(kind) = r.kind else {
            reject(run, &label, r.note.unwrap_or(blank), "关系的类型不认识".into());
            result.rejected += 1;
            continue;
        };
        if from == to {
            reject(run, &label, r.note.unwrap_or(blank), "关系的两头是同一张卡片".into());
            result.rejected += 1;
            continue;
        }
        let mut note = r.note.unwrap_or(Sentence { category: Category::Explanation, text: kind.label().to_string(), ..blank });
        if let Err(why) = check(&mut note, books, sources) {
            reject(run, &label, note, why);
            result.rejected += 1;
            continue;
        }
        let id = run.next_relation_id();
        run.relations.push(Relation { id, from, to, kind, note, step: step.id.clone() });
        result.relations += 1;
    }
    result
}
