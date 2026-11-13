//! 文本层：夹具 EPUB → 单元、角色、段落号、页码、对齐、预览。

use concept_engine::align;
use concept_engine::fixture::{self, Flavor};
use concept_engine::text::{self, Numbering, Role, UnitKind};

fn chapters(t: &text::Text) -> Vec<(String, usize)> {
    t.chapters().iter().filter(|u| u.kind == UnitKind::Chapter).map(|u| (u.key.clone(), t.body_of(u.index).count())).collect()
}

#[test]
fn english_epub3_paragraph_numbers_pages_and_roles() {
    let t = fixture::english();
    assert_eq!(t.language, "en");
    assert_eq!(t.numbering, Some(Numbering::Glasgow));
    assert_eq!(t.page_source, "page-list");
    assert_eq!(chapters(&t), [("I.i".to_string(), 11), ("I.ii".to_string(), 5), ("I.iii".to_string(), 8)]);
    // 段落号：篇.章.段
    let p = t.paragraph("I.iii.5").unwrap();
    assert!(p.text.starts_with("The nations that"));
    assert_eq!(p.number, Some(5));
    // 小型大写照原样取字
    assert!(t.paragraph("I.i.1").unwrap().text.starts_with("THE greatest improvements"));
    // 合成的导言是导读、注释是注释、目录是前后附，都没有段落号
    let role_of = |needle: &str| t.paragraphs.iter().find(|p| p.text.contains(needle)).map(|p| (p.role, p.number));
    assert_eq!(role_of("synthetic introduction"), Some((Role::Guide, None)));
    assert_eq!(role_of("synthetic editor’s note"), Some((Role::Note, None)));
    assert_eq!(role_of("Editor’s Introduction").map(|r| r.0), Some(Role::Matter));
    assert_eq!(t.paragraph("I.i 标题").unwrap().text, "CHAPTER I");
    // 页码：每段都有；单调不减；段内有分页点
    let body: Vec<&text::Paragraph> = t.paragraphs.iter().filter(|p| p.role == Role::Body).collect();
    assert!(body.iter().all(|p| p.page.is_some()));
    assert_eq!(body[0].page.as_deref(), Some("101"));
    let mut last = 0u32;
    for p in &body {
        let first: u32 = p.page.as_ref().unwrap().parse().unwrap();
        assert!(first >= last);
        last = p.breaks.last().map(|b| b.page.parse().unwrap()).unwrap_or(first);
    }
    let total_chars: usize = body.iter().map(|p| p.text.chars().count()).sum();
    assert!(last as usize >= fixture::FIRST_PAGE as usize + total_chars / fixture::PAGE_EVERY - 1);
    // 一段跨页：引文的页码按它在段里的位置算
    let crossing = body.iter().find(|p| !p.breaks.is_empty()).unwrap();
    let b = &crossing.breaks[0];
    let before = crossing.page.clone().unwrap();
    assert_eq!(t.page_at(crossing.seq, b.at - 1).unwrap(), before);
    assert_eq!(t.page_at(crossing.seq, b.at).unwrap(), b.page);
    assert_eq!(t.pages_of(crossing.seq, b.at.saturating_sub(5), b.at + 5).unwrap(), format!("{before}–{}", b.page));
    assert_eq!(t.pages_of(crossing.seq, 0, 3).unwrap(), before);
}

#[test]
fn epub2_with_page_map_and_split_files_gives_the_same_text() {
    let a = fixture::english();
    let b = text::from_epub(&fixture::english_epub(Flavor::Epub2Split), None).unwrap();
    assert_eq!(b.page_source, "page-map");
    let body = |t: &text::Text| t.paragraphs.iter().filter(|p| p.role == Role::Body).map(|p| (p.key.clone(), p.text.clone(), p.page.clone(), p.breaks.clone())).collect::<Vec<_>>();
    assert_eq!(body(&a), body(&b));
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
}

#[test]
fn chinese_roles_notes_and_sidenotes() {
    let t = fixture::chinese();
    assert_eq!(t.language, "zh");
    assert_eq!(t.page_source, "none");
    assert!(t.paragraphs.iter().all(|p| p.page.is_none()));
    assert_eq!(chapters(&t), [("I.i".to_string(), 11), ("I.ii".to_string(), 5), ("I.iii".to_string(), 7)]);
    // 章名是粗体链接的 <p>：认成标题，不算正文
    assert_eq!(t.paragraph("I.i 标题").unwrap().text, "第一章\n论劳动分工");
    // 旁注挂到前面的正文段
    let s = t.paragraph("I.i.1 旁注").unwrap();
    assert_eq!(s.role, Role::Sidenote);
    assert_eq!(s.attached, Some(t.paragraph("I.i.1").unwrap().seq));
    assert_eq!(t.paragraph("I.i.5 旁注").unwrap().text, "这种好处来自三种情况");
    // 注释：引用标记不进正文；注释挂到带同一个标记的正文段；译者按接在注释 1 后面
    let p4 = t.paragraph("I.i.4").unwrap();
    assert!(!p4.text.contains('［'));
    assert_eq!(p4.noterefs.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(), ["1"]);
    let n1 = t.paragraph("I.i 注 1").unwrap();
    assert_eq!((n1.role, n1.attached), (Role::Note, Some(p4.seq)));
    let n1b = t.paragraph("I.i 注 1（2）").unwrap();
    assert!(n1b.text.starts_with("译者按"));
    assert_eq!(n1b.attached, Some(p4.seq));
    assert_eq!(t.paragraph("I.i 注 2").unwrap().attached, Some(t.paragraph("I.i.8").unwrap().seq));
    // 导读不是正文；导读里的注释也算导读
    let guide: Vec<&text::Paragraph> = t.paragraphs.iter().filter(|p| p.role == Role::Guide).collect();
    assert!(guide.len() >= 4);
    assert!(guide.iter().all(|p| p.number.is_none()));
    assert!(t.paragraphs.iter().filter(|p| p.text.contains("合成导读的注释")).all(|p| p.role == Role::Guide));
    // 结构预览
    let pv = text::preview(&t);
    assert_eq!((pv.totals.body, pv.totals.sidenote, pv.totals.note), (23, 6, 3));
    assert!(!pv.has_pages);
    let ch1 = pv.units.iter().find(|u| u.key == "I.i").unwrap();
    assert_eq!((ch1.counts.body, ch1.counts.sidenote, ch1.counts.note, ch1.counts.heading), (11, 4, 3, 1));
}

#[test]
fn chapter_style_numbering_on_request() {
    let mut t = fixture::english();
    t.renumber(Numbering::Chapter);
    assert!(t.paragraph("3.5").unwrap().text.starts_with("The nations that"));
    assert!(t.paragraph("I.iii.5").is_none());
    let auto = text::from_epub(&fixture::english_epub(Flavor::Epub3), Some(Numbering::Chapter)).unwrap();
    assert_eq!(auto.numbering, Some(Numbering::Chapter));
    assert!(auto.paragraph("1.11").is_some());
}

#[test]
fn chinese_and_english_align_where_counts_match() {
    let en = fixture::english();
    let zh = fixture::chinese();
    let a = align::align(&en, &zh);
    let summary: Vec<(&str, usize, usize, bool)> = a.units.iter().map(|u| (u.key.as_str(), u.count, u.other_count, u.aligned)).collect();
    assert_eq!(summary, [("I.i", 11, 11, true), ("I.ii", 5, 5, true), ("I.iii", 8, 7, false)]);
    assert_eq!(a.pairs.len(), 16);
    let en3 = en.paragraph("I.i.3").unwrap();
    let zh3 = align::counterpart(&a, &en, &zh, en3.seq).unwrap();
    assert_eq!(zh3.key, "I.i.3");
    assert!(zh3.text.starts_with("以制针业为例"));
    // 段数不同的章不对齐
    assert!(align::counterpart(&a, &en, &zh, en.paragraph("I.iii.1").unwrap().seq).is_none());
    assert_eq!(a.mismatched().len(), 1);
}

#[test]
fn not_an_epub_is_refused_with_a_reason() {
    assert!(text::from_epub(b"not a zip", None).unwrap_err().contains("EPUB"));
}
