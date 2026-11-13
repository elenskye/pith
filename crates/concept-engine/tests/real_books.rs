//! 两本真实 EPUB 的解析（默认忽略；书不进 Git，书里的文字不进断言和输出，只打印聚合数字）。
//!
//! ```bash
//! PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub cargo test -p concept-engine --test real_books -- --ignored --nocapture
//! ```
//! 路径相对于仓库根目录或写绝对路径。

use concept_engine::align;
use concept_engine::text::{self, Numbering, Role, Text, UnitKind};

fn load(var: &str) -> Option<Text> {
    let path = std::env::var(var).ok().filter(|p| !p.is_empty())?;
    let p = std::path::Path::new(&path);
    let p = if p.is_absolute() { p.to_path_buf() } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p) };
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("读不了 {}：{e}", p.display()));
    Some(text::from_epub(&bytes, None).expect("解析失败"))
}

/// 第一篇第 1–3 章
fn first_three(t: &Text) -> Vec<usize> {
    let mut out = Vec::new();
    for u in t.chapters() {
        if u.kind == UnitKind::Chapter && u.book == Some(1) && matches!(u.chapter, Some(1..=3)) {
            out.push(u.index);
        }
    }
    out
}

fn report(name: &str, t: &Text) {
    let p = text::preview(t);
    println!("== {name}：语言 {}，段落号 {:?}，页码来源 {}，有页码 {}", p.language, p.numbering, p.page_source, p.has_pages);
    println!("   全书：段 {}，正文 {}，标题 {}，旁注 {}，注释 {}，导读 {}，前后附 {}；单元 {}", p.paragraphs, p.totals.body, p.totals.heading, p.totals.sidenote, p.totals.note, p.totals.guide, p.totals.matter, t.units.len());
    for u in first_three(t) {
        let up = p.units.iter().find(|x| x.index == u).unwrap();
        println!("   {}：正文 {}，旁注 {}，注释 {}，标题 {}，页 {:?}", up.key, up.counts.body, up.counts.sidenote, up.counts.note, up.counts.heading, up.pages);
    }
    for w in &p.warnings {
        println!("   提示：{w}");
    }
}

#[test]
#[ignore]
fn real_epubs_first_three_chapters() {
    let en = load("PITH_BOOK_EN");
    let zh = load("PITH_BOOK_ZH");
    if en.is_none() && zh.is_none() {
        println!("没给 PITH_BOOK_EN / PITH_BOOK_ZH，跳过");
        return;
    }
    if let Some(en) = &en {
        report("英文", en);
        assert_eq!(en.numbering, Some(Numbering::Glasgow));
        let chapters = first_three(en);
        let counts: Vec<usize> = chapters.iter().map(|u| en.body_of(*u).count()).collect();
        assert_eq!(counts, [11, 5, 8]);
        let keys: Vec<String> = chapters.iter().map(|u| en.units[*u].key.clone()).collect();
        assert_eq!(keys, ["I.i", "I.ii", "I.iii"]);
        // 页码：第 1–3 章正文都在 109–126 页
        let pages: Vec<u32> = chapters.iter().flat_map(|u| en.body_of(*u)).flat_map(|p| {
            let mut v: Vec<u32> = p.page.iter().filter_map(|x| x.parse().ok()).collect();
            v.extend(p.breaks.iter().filter_map(|b| b.page.parse::<u32>().ok()));
            v
        }).collect();
        assert_eq!(pages.iter().min(), Some(&109));
        assert_eq!(pages.iter().max(), Some(&126));
        assert!(chapters.iter().flat_map(|u| en.body_of(*u)).all(|p| p.page.is_some()));
        // 小型大写照样取字：第一段以全大写的词开头
        let first = en.body_of(chapters[0]).next().unwrap();
        assert!(first.text.split_whitespace().next().unwrap().chars().all(|c| c.is_uppercase()));
        println!("   I.i.1 跨页：{:?}；I.i.2 页码 {:?}", en.paragraph_pages(first.seq), en.paragraph("I.i.2").and_then(|p| en.paragraph_pages(p.seq)));
    }
    if let Some(zh) = &zh {
        report("中文", zh);
        assert_eq!(zh.language, "zh");
        let chapters = first_three(zh);
        let counts: Vec<usize> = chapters.iter().map(|u| zh.body_of(*u).count()).collect();
        assert_eq!(counts, [11, 5, 8]);
        assert!(!zh.paragraphs.iter().any(|p| p.page.is_some()), "中文版没有页码");
        let p = text::preview(zh);
        assert!(p.totals.guide > 0 && p.totals.sidenote > 0 && p.totals.note > 0);
        // 导读、旁注、注释都不是正文
        for u in &chapters {
            let unit_paras: Vec<_> = zh.paragraphs.iter().filter(|p| p.unit == *u).collect();
            let notes = unit_paras.iter().filter(|p| p.role == Role::Note).count();
            let sides = unit_paras.iter().filter(|p| p.role == Role::Sidenote).count();
            println!("   {}：注释 {notes}，旁注 {sides}", zh.units[*u].key);
        }
    }
    if let (Some(en), Some(zh)) = (&en, &zh) {
        let a = align::align(en, zh);
        let first: Vec<_> = a.units.iter().filter(|u| ["I.i", "I.ii", "I.iii"].contains(&u.key.as_str())).collect();
        println!("== 对齐：{} 章对上，{} 章段数不同；第一篇 1–3 章：{:?}", a.units.iter().filter(|u| u.aligned).count(), a.mismatched().len(), first.iter().map(|u| (u.key.as_str(), u.count, u.other_count, u.aligned)).collect::<Vec<_>>());
        assert!(first.len() == 3 && first.iter().all(|u| u.aligned));
    }
}
