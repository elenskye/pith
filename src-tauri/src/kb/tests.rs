//! 知识库（fresh）：一份材料进几个库、段落块、1.1.0 的升级（书迁进来、重复的并成一份）、检索、读原文、引用编号、用向量对齐。
//! 升级、导出恢复带上这几张表：`transfer/tests.rs` 的 rich_library（`chat::tests::sample`）和 `exercise`。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;

use super::store::{self, Material};
use super::{align, cite, index, passages, read, retrieve};
use crate::common::err;
use crate::concept::tests::library_with_books;
use crate::db::tests::memory_db;
use crate::local::{LocalModels, Mode};

const NOW: &str = "2026-11-26T10:00:00.000Z";

fn mock() -> LocalModels {
    LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"))
}

fn off() -> LocalModels {
    LocalModels::new(Mode::Off, PathBuf::from("/nonexistent"))
}

fn note(kb: &str, title: &str, body: &str) -> Material {
    Material { kb: kb.into(), kind: "note".into(), title: title.into(), body: body.into(), ..Default::default() }
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn a_material_lives_in_several_knowledge_bases() {
    let c = memory_db();
    assert!(store::kb_create(&c, " ", "", NOW).is_err());
    let a = store::kb_create(&c, "毕业论文", "政治效能感", NOW).unwrap();
    let b = store::kb_create(&c, "读《国富论》", "", NOW).unwrap();
    let m = Material { kb: a.clone(), kind: "report".into(), title: "斯密怎么评价东印度公司".into(), source_kind: Some("research_run".into()), source_ref: Some("RUN1".into()), ..Default::default() };
    let first = store::material_add(&c, &m, NOW).unwrap();
    assert_eq!((first.kb.as_str(), first.kbs.clone()), (a.as_str(), vec![a.clone()]));
    let again = store::material_add(&c, &m, "2026-11-26T11:00:00.000Z").unwrap();
    assert_eq!((again.uid.as_str(), again.tended_at.as_str()), (first.uid.as_str(), "2026-11-26T11:00:00.000Z"), "同一个库里再种：照料一下");
    // 种进另一个库：还是那一份，多了一个库
    let both = store::material_add(&c, &Material { kb: b.clone(), ..m.clone() }, NOW).unwrap();
    assert_eq!(both.uid, first.uid);
    assert_eq!(both.kbs, [a.clone(), b.clone()]);
    assert_eq!(store::kbs_list(&c).unwrap().iter().map(|k| k.count).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(store::materials(&c, Some(&b)).unwrap()[0].kb, b, "在一个库里列的时候，kb 是那个库");
    assert!(store::material_add(&c, &Material { kind: "poem".into(), ..m.clone() }, NOW).is_err());
    assert!(store::material_add(&c, &Material { kb: "nope".into(), ..m.clone() }, NOW).is_err());

    let up = store::material_update(&c, &first.uid, "东印度公司", "垄断", 5, NOW).unwrap();
    assert_eq!((up.title.as_str(), up.stage), ("东印度公司", 2), "阶段夹在 0–2");

    // 从家里拿掉：别的库还有，家挪过去
    store::material_remove(&c, &first.uid, Some(&a)).unwrap();
    let moved = store::material_get(&c, &first.uid).unwrap();
    assert_eq!((moved.kb.as_str(), moved.kbs.clone()), (b.as_str(), vec![b.clone()]));
    // 再放回 a；删掉 b：家挪回 a，材料还在
    store::material_link(&c, &first.uid, &a, NOW).unwrap();
    store::kb_delete(&c, &b).unwrap();
    assert_eq!(store::material_get(&c, &first.uid).unwrap().kbs, [a.clone()]);
    // 只在一个库里的：拿掉就没了，段落块一起
    let only = store::material_add(&c, &note(&a, "笔记", "一段。"), NOW).unwrap();
    assert_eq!(only.passages, 1);
    store::material_remove(&c, &only.uid, Some(&a)).unwrap();
    assert!(store::material_get(&c, &only.uid).is_err());
    assert_eq!(count(&c, "SELECT COUNT(*) FROM passages"), 1, "只剩那份报告的一块");

    // 对话删了，材料还在
    let conv = crate::chat::store::create(&c, "问", "pith", &[], None, None, NOW).unwrap();
    let saved = store::material_add(&c, &Material { kb: a.clone(), kind: "answer".into(), title: "回答".into(), conversation: Some(conv.clone()), ..Default::default() }, NOW).unwrap();
    crate::chat::store::delete(&c, &conv).unwrap();
    assert_eq!(store::material_get(&c, &saved.uid).unwrap().conversation, None);
    store::kb_delete(&c, &a).unwrap();
    assert!(store::materials(&c, None).unwrap().is_empty(), "只在这个库里的材料一起删");
    assert_eq!(count(&c, "SELECT COUNT(*) FROM passages"), 0);
}

#[test]
fn bodies_are_cut_into_passages_that_keep_their_ids() {
    let c = memory_db();
    let kb = store::kb_create(&c, "随手存", "", NOW).unwrap();
    let long = "分工受市场范围的限制。".repeat(200);
    let body = format!("# 斯密\n\n短的一句。\n\n又一句短的。\n\n{long}\n\n## 后来的人\n\nStigler 1951 说行业年轻时企业什么都自己做。");
    let m = store::material_add(&c, &note(&kb, "读书笔记", &body), NOW).unwrap();
    let ps = passages::all(&c, m.id).unwrap();
    let roles: Vec<&str> = ps.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(roles[0], "heading");
    assert_eq!(ps[1].text, "短的一句。\n\n又一句短的。", "太短的并在一起");
    assert!(ps.iter().filter(|p| p.text.starts_with("分工受市场")).count() >= 2, "太长的在句末拆开");
    assert!(ps.iter().all(|p| p.text.chars().count() <= 1600));
    assert_eq!(ps.last().unwrap().unit_label, "后来的人", "小标题下面的块记在它名下");
    // 按词找得到；改了正文：没变的块 id 不变，变了的重新进索引
    let hit = index::keyword(&c, "Stigler", &[], 5).unwrap();
    assert_eq!(hit.len(), 1);
    let stigler = hit[0].0;
    let changed = body.replace("短的一句。", "改过的一句。");
    store::material_update(&c, &m.uid, "读书笔记", &changed, 0, NOW).unwrap();
    let after = passages::all(&c, m.id).unwrap();
    assert_eq!(after.iter().map(|p| p.id).collect::<Vec<_>>(), ps.iter().map(|p| p.id).collect::<Vec<_>>());
    assert!(index::keyword(&c, "改过", &[], 5).unwrap().iter().any(|(id, _)| *id == ps[1].id));
    assert!(index::keyword(&c, "短的一句", &[], 5).unwrap().is_empty());
    assert_eq!(index::keyword(&c, "Stigler", &[], 5).unwrap()[0].0, stigler);
    // 变短：多出来的块和索引一起拿掉
    store::material_update(&c, &m.uid, "读书笔记", "只剩一句。", 0, NOW).unwrap();
    assert_eq!(passages::all(&c, m.id).unwrap().len(), 1);
    assert!(index::keyword(&c, "Stigler", &[], 5).unwrap().is_empty());
    assert!(!index::ensure(&c).unwrap(), "索引和主库对得上");
    // 索引坏了（比如 search.db 被删）：对一遍就重建
    c.execute_batch("DELETE FROM search.kb_fts").unwrap();
    assert!(index::ensure(&c).unwrap());
    assert_eq!(index::keyword(&c, "只剩", &[], 5).unwrap().len(), 1);
}

/// 第一次打开 1.1.0：0011 之后的升级还没做过
fn reset_upgrade(c: &Connection) {
    c.execute("DELETE FROM settings WHERE key = 'kb.v11'", []).unwrap();
}

#[test]
fn upgrade_moves_parsed_books_in_and_merges_duplicates() {
    let (_dir, state, en, zh, _) = library_with_books("kb-upgrade");
    state
        .with_db(|c| {
            reset_upgrade(c);
            // 0011 以前：同一份调查在两个库里各一份（直接写，绕过现在的去重）；英文书已经种在「读《国富论》」里
            let a = store::kb_create(c, "读《国富论》", "", NOW)?;
            let b = store::kb_create(c, "论文", "", NOW)?;
            for (kb, stage) in [(&a, 0), (&b, 2)] {
                c.execute(
                    "INSERT INTO kb_materials (uid, kb_id, kind, title, stage, source_kind, source_ref, created_at, tended_at) VALUES (?1, (SELECT id FROM knowledge_bases WHERE uid = ?2), 'report', '调查', ?3, 'research_run', 'RUN1', ?4, ?4)",
                    rusqlite::params![crate::common::new_uid()?, kb, stage, NOW],
                )
                .map_err(err)?;
            }
            c.execute(
                "INSERT INTO kb_materials (uid, kb_id, kind, title, source_kind, source_ref, created_at, tended_at) VALUES ('BOOK', (SELECT id FROM knowledge_bases WHERE uid = ?1), 'full', 'Wealth', 'concept_text', ?2, ?3, ?3)",
                rusqlite::params![a, en, NOW],
            )
            .map_err(err)?;
            assert_eq!(store::upgrade(c, NOW)?, 1, "只有中文书要迁");
            let all = store::materials(c, None)?;
            let reports: Vec<&Material> = all.iter().filter(|m| m.kind == "report").collect();
            assert_eq!(reports.len(), 1, "并成一份");
            assert_eq!((reports[0].stage, reports[0].kbs.len()), (2, 2));
            let books: Vec<&Material> = all.iter().filter(|m| m.kind == "full").collect();
            assert_eq!(books.len(), 2);
            let shelf = store::kbs_list(c)?.into_iter().find(|k| k.name == store::SHELF).unwrap();
            assert_eq!(shelf.count, 1);
            // 书的段落块：一段一块，段落号、页码、章照文本
            let t = crate::concept::store::text_load(c, &en)?;
            let en_m = books.iter().find(|m| m.source_ref.as_deref() == Some(en.as_str())).unwrap();
            assert_eq!(en_m.uid, "BOOK", "已经种过的不再种");
            assert_eq!(en_m.passages as usize, t.paragraphs.len());
            let p = t.paragraph("I.i.3").unwrap();
            let got = passages::at(c, en_m.id, p.seq as i64)?.unwrap();
            assert_eq!((got.key.as_str(), got.role.as_str(), got.text.as_str()), ("I.i.3", "body", p.text.as_str()));
            assert_eq!(got.page, t.paragraph_pages(p.seq));
            assert_eq!(got.unit_key, "I.i");
            // 再启动一次：什么都不多
            assert_eq!(store::upgrade(c, NOW)?, 0);
            assert_eq!(store::materials(c, None)?.len(), 3);
            assert!(store::by_source(c, "concept_text", &zh)?.is_some());
            Ok(())
        })
        .unwrap();
}

/// 两本书都迁进知识库（书架），返回（原著的材料 id, 译本的材料 id）
fn shelve(state: &crate::AppState, en: &str, zh: &str) -> (i64, i64) {
    state
        .with_db(|c| {
            reset_upgrade(c);
            store::upgrade(c, NOW)?;
            let id = |u: &str| -> Result<i64, String> { store::material_id(c, &store::by_source(c, "concept_text", u)?.unwrap()) };
            Ok((id(en)?, id(zh)?))
        })
        .unwrap()
}

#[test]
fn search_finds_passages_by_meaning_and_words_with_the_translation() {
    let (_dir, state, en, zh, _) = library_with_books("kb-search");
    let (en_m, zh_m) = shelve(&state, &en, &zh);
    let local = mock();
    // 还没有向量：只按词，并说明
    let f = retrieve::search(&state, &local, "pin-maker", &[], 6).unwrap();
    assert_eq!(f.mode, "keyword");
    assert!(f.note.as_deref().unwrap().contains("还没建好"));
    assert_eq!(f.hits[0].passage.key, "I.i.3");
    assert_eq!(f.hits[0].source.id, en_m);
    assert!(!f.hits[0].pair.is_empty() && f.hits[0].pair[0].material_id == zh_m, "带着对齐的中译");
    // 补向量：书里「问这本书」算过的直接拿来
    let never = AtomicBool::new(false);
    let made = index::index_all(&state, &local, &never, &|_, _| {}).unwrap();
    assert!(made > 0);
    assert_eq!(index::index_all(&state, &local, &never, &|_, _| {}).unwrap(), 0, "建好的不重算");
    let f = retrieve::search(&state, &local, "Why is the division of labour limited by the extent of the market?", &[], 6).unwrap();
    assert_eq!((f.mode, f.reranked, f.note.is_none()), ("semantic", true, true), "{:?}", f.note);
    assert!(f.hits.iter().take(3).any(|h| h.passage.key == "I.iii.1"), "{:?}", f.hits.iter().map(|h| h.passage.key.clone()).collect::<Vec<_>>());
    assert!(f.hits.windows(2).all(|w| w[0].score >= w[1].score));
    // 中文问题：译本里找到的映射回原著，原著在前、中译跟着；同一组不重复
    let f = retrieve::search(&state, &local, "分工为什么受市场范围的限制", &[], 6).unwrap();
    assert!(!f.hits.is_empty());
    assert!(f.hits.iter().all(|h| h.source.id == en_m || h.pair.is_empty()), "{:?}", f.hits.iter().map(|h| (h.source.id, h.passage.key.clone())).collect::<Vec<_>>());
    let keys: Vec<i64> = f.hits.iter().map(|h| h.passage.id).collect();
    assert_eq!(keys.len(), keys.iter().collect::<std::collections::HashSet<_>>().len());
    let text = retrieve::format("分工为什么受市场范围的限制", &f);
    assert!(text.contains(&format!("[P{}]", f.hits[0].passage.id)) && text.contains("对照 [P") && text.contains("[材料 "));
    // 范围：勾了一个没有这本书的库就找不到
    let other = state.with_db(|c| store::kb_create(c, "空的", "", NOW)).unwrap();
    let f = retrieve::search(&state, &local, "market", &[other], 6).unwrap();
    assert!(f.hits.is_empty() && f.note.as_deref().unwrap().contains("还没有材料"));
    // 本机模型用不了：只按词
    let f = retrieve::search(&state, &off(), "pin-maker", &[], 6).unwrap();
    assert_eq!(f.mode, "keyword");
    assert!(f.note.as_deref().unwrap().contains("只按词"));
    // 列出材料（搜索词空着）
    let list = state.with_db(|c| retrieve::list_materials(c, &[])).unwrap();
    assert!(list.contains(&format!("[材料 {en_m}] 书")));
}

#[test]
fn reading_context_chapters_outlines_and_translations() {
    let (_dir, state, en, zh, _) = library_with_books("kb-read");
    let (en_m, _) = shelve(&state, &en, &zh);
    state
        .with_db(|c| {
            let t = crate::concept::store::text_load(c, &en)?;
            let p = passages::at(c, en_m, t.paragraph("I.iii.1").unwrap().seq as i64)?.unwrap();
            let ctx = read::read(c, &read::Req { passage: Some(p.id), scope: "context".into(), around: 1, translation: true, ..Default::default() })?;
            assert!(ctx.contains(&format!("[P{}] I.iii.1", p.id)) && ctx.contains("对照 [P"), "{ctx}");
            assert!(!ctx.contains("I.ii."), "上下文不出这一章");
            let ch = read::read(c, &read::Req { passage: Some(p.id), scope: "chapter".into(), ..Default::default() })?;
            let in_unit = t.body_of(t.paragraph("I.iii.1").unwrap().unit).count();
            assert_eq!(ch.matches("[P").count(), in_unit, "{ch}");
            // 材料：目录；按章号读
            let outline = read::read(c, &read::Req { material: Some(en_m), ..Default::default() })?;
            assert!(outline.contains("I.iii") && outline.contains("从 [P"), "{outline}");
            let by_key = read::read(c, &read::Req { material: Some(en_m), chapter: Some("I.iii".into()), ..Default::default() })?;
            assert_eq!(by_key, ch);
            assert!(read::read(c, &read::Req { passage: Some(999_999), ..Default::default() }).is_err());
            assert!(read::read(c, &read::Req::default()).is_err());
            // 整章太长：只给前一部分，说怎么接着读
            let (name, sub) = read::chapter_for_subagent(c, p.id, false)?;
            assert!(!name.is_empty() && sub.contains(&format!("[P{}]", p.id)));
            // 面板：读到那一章，焦点在那一段，带对照
            let v = read::view(c, en_m, None, Some(p.id))?;
            assert_eq!(v.focus, Some(p.id));
            assert!(v.passages.iter().any(|x| x.id == p.id) && v.pairs.contains_key(&p.id));
            assert!(v.pair_title.is_some());
            let first = read::view(c, en_m, Some(0), None)?;
            assert_eq!(first.unit, 0);
            Ok(())
        })
        .unwrap();
    // 太长的一章：主对话只拿到前一部分
    let c = memory_db();
    let kb = store::kb_create(&c, "随手存", "", NOW).unwrap();
    let body = (0..80).map(|i| format!("第 {i} 段。{}", "很长的一段话，".repeat(30))).collect::<Vec<_>>().join("\n\n");
    let m = store::material_add(&c, &note(&kb, "长笔记", &body), NOW).unwrap();
    let first = passages::all(&c, m.id).unwrap()[0].id;
    let ch = read::read(&c, &read::Req { passage: Some(first), scope: "chapter".into(), ..Default::default() }).unwrap();
    assert!(ch.contains("没给出") && ch.contains("delegate") && read::weight(&ch) < read::CHAPTER_BUDGET + 400, "{}", read::weight(&ch));
}

#[test]
fn cites_turn_into_margin_notes_with_page_translation_and_item() {
    let (_dir, state, en, zh, en_item) = library_with_books("kb-cite");
    let (en_m, _) = shelve(&state, &en, &zh);
    state
        .with_db(|c| {
            let t = crate::concept::store::text_load(c, &en)?;
            let p = passages::at(c, en_m, t.paragraph("I.i.3").unwrap().seq as i64)?.unwrap();
            let kb = store::kbs_list(c)?[0].uid.clone();
            let n = store::material_add(c, &note(&kb, "我的笔记", "分工的三个好处。"), NOW)?;
            let np = passages::all(c, n.id)?[0].id;
            let text = format!("别针厂的例子 [P{}]。我记过 [P{np}]，又见 [P{}, P{np}]；[P999999] 不认识。", p.id, p.id);
            let (out, cites) = cite::resolve(c, &text)?;
            assert_eq!(out, "别针厂的例子 [1]。我记过 [2]，又见 [1][2]；[P999999] 不认识。");
            assert_eq!(cites.len(), 2);
            assert_eq!((cites[0].kind.as_str(), cites[0].text.as_str()), ("原文", p.text.as_str()));
            assert!(cites[0].loc.starts_with("I.i.3"));
            assert!(cites[0].src.contains("Wealth"));
            assert!(cites[0].translation.is_some() && cites[0].translation_loc.is_some(), "带着中译");
            assert_eq!(cites[0].item, Some(en_item), "一步到文献库");
            assert_eq!(cites[1].kind, "笔记");
            let saved = cite::with_sources(&text, &cites);
            assert!(saved.starts_with("别针厂的例子 [1]") && saved.contains("出处：\n[1] 《") && saved.contains("\n[2] 《我的笔记》"));
            Ok(())
        })
        .unwrap();
}

#[test]
fn alignment_uses_vectors_once_both_books_have_them() {
    let (_dir, state, en, zh, _) = library_with_books("kb-align");
    shelve(&state, &en, &zh);
    let local = mock();
    let model = local.model(crate::local::Svc::Embedding).unwrap().key;
    // 没有向量：不动
    assert_eq!(state.with_db(|c| align::refine_all(c, &model, NOW)).unwrap(), 0);
    let never = AtomicBool::new(false);
    index::index_all(&state, &local, &never, &|_, _| {}).unwrap();
    assert_eq!(state.with_db(|c| align::refine_all(c, &model, NOW)).unwrap(), 1);
    let meta = state.with_db(|c| crate::concept::store::alignment_meta(c, &en, &zh)).unwrap().unwrap();
    assert_eq!(meta, (concept_engine::align::VERSION, Some(model.clone())));
    let al = state.with_db(|c| crate::concept::store::alignment_load(c, &en, &zh)).unwrap().unwrap();
    // 模拟向量只看字面（中英之间没有意义），只看对齐照样算出来、每一章都有结果；真实的比较在 real_alignment
    let plain = state.with_db(|c| Ok(concept_engine::align::align(&crate::concept::store::text_load(c, &en)?, &crate::concept::store::text_load(c, &zh)?))).unwrap();
    assert!(!al.pairs.is_empty() && al.units.len() == plain.units.len());
    assert_eq!(state.with_db(|c| align::refine_all(c, &model, NOW)).unwrap(), 0, "同一个模型对过的不再算");
}

#[test]
fn a_mutex_connection_is_a_db_too() {
    let c = memory_db();
    let kb = store::kb_create(&c, "随手存", "", NOW).unwrap();
    store::material_add(&c, &note(&kb, "笔记", "Stigler 说分工受市场范围限制。"), NOW).unwrap();
    let db = Mutex::new(c);
    let f = retrieve::search(&db, &off(), "Stigler", &[], 5).unwrap();
    assert_eq!(f.hits.len(), 1);
    assert_eq!(f.hits[0].source.kind, "note");
}

/// 真实的两本《国富论》、真实的本机向量模型，走 App 自己的路（默认忽略；不联网、不花钱，会经 llmctl.sh 启动向量服务，跑完停掉）：
/// 解析两本 EPUB → 迁进知识库成段落块 → 补向量 → 只用长度和锚点对齐 vs 加上语义相似度对齐，打印聚合数字（不打印书里的文字）。
/// `PITH_LOCAL_MODELS=real PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub cargo test -p pith real_alignment -- --ignored --nocapture`
#[test]
#[ignore]
fn real_alignment() {
    use concept_engine::align::{self as ca, Method};
    use concept_engine::text::Role;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let book = |var: &str| -> Vec<u8> {
        let p = PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("要设 {var}")));
        std::fs::read(if p.is_absolute() { p } else { root.join(p) }).expect("读不了书")
    };
    let dir = crate::db::tests::temp_dir("kb-real-alignment");
    let state = crate::AppState::init(crate::paths::DataPaths::new(dir.join("data")));
    let local = LocalModels::detect();
    assert_eq!(local.mode(), Mode::Real, "先设 PITH_LOCAL_MODELS=real，并确认 ~/Documents/LLM 在");
    let (en, zh) = state
        .with_db(|c| {
            let (et, em) = crate::concept::commands::parse_epub(&book("PITH_BOOK_EN"))?;
            let en = crate::concept::commands::save_text(c, &state.paths.backups, &et, &em, None, None, "real-en", None)?;
            let (zt, zm) = crate::concept::commands::parse_epub(&book("PITH_BOOK_ZH"))?;
            let zh = crate::concept::commands::save_text(c, &state.paths.backups, &zt, &zm, None, None, "real-zh", Some(&en))?;
            Ok((en, zh))
        })
        .unwrap();
    shelve(&state, &en, &zh);
    let started = std::time::Instant::now();
    let never = AtomicBool::new(false);
    let made = index::index_all(&state, &local, &never, &|done, total| {
        if done % 800 < 16 {
            println!("  向量 {done} / {total}");
        }
    });
    local.stop_ours_on_exit();
    let made = made.unwrap();
    println!("补向量：新算 {made} 块，用时 {:.0} 秒", started.elapsed().as_secs_f32());
    let model = local.model(crate::local::Svc::Embedding).unwrap().key;
    let (et, zt, a, b) = state
        .with_db(|c| {
            let id = |u: &str| -> Result<i64, String> { store::material_id(c, &store::by_source(c, "concept_text", u)?.unwrap()) };
            let vecs = |m: i64| -> Result<std::collections::HashMap<usize, Vec<f32>>, String> { Ok(index::vectors(c, &model, m)?.into_iter().map(|(s, v)| (s as usize, v)).collect()) };
            Ok((crate::concept::store::text_load(c, &en)?, crate::concept::store::text_load(c, &zh)?, vecs(id(&en)?)?, vecs(id(&zh)?)?))
        })
        .unwrap();
    let plain = ca::align(&et, &zt);
    let started = std::time::Instant::now();
    let semantic = align::with_vectors(&et, &zt, &a, &b).expect("向量不全");
    println!("加相似度对齐用时 {:.1} 秒", started.elapsed().as_secs_f32());
    let bodies = et.paragraphs.iter().filter(|p| p.role == Role::Body).count();
    let summary = |name: &str, al: &ca::Alignment| {
        let with_body: Vec<_> = al.units.iter().filter(|u| u.count > 0).collect();
        let aligned = with_body.iter().filter(|u| u.method != Method::None).count();
        let exact = with_body.iter().filter(|u| u.method == Method::Exact).count();
        let low = with_body.iter().filter(|u| u.low).count();
        let covered = et.paragraphs.iter().filter(|p| p.role == Role::Body && !al.other_of(p.seq).is_empty()).count();
        println!("{name}：有正文的单元 {aligned} / {}（段数相同 {exact}，算法对的 {}，置信度低 {low}）；原著正文段有中译 {covered} / {bodies}；对法 {:?}", with_body.len(), aligned - exact, ca::shapes(al));
    };
    summary("只用长度和锚点", &plain);
    summary("加语义相似度", &semantic);
    // 算法对的章里，两种做法给同一段的对法一样的占多少
    let fitted: Vec<usize> = plain.units.iter().filter(|u| u.method == Method::Fitted).map(|u| u.unit).collect();
    let in_fitted: Vec<usize> = et.paragraphs.iter().filter(|p| p.role == Role::Body && fitted.contains(&p.unit)).map(|p| p.seq).collect();
    let same = in_fitted.iter().filter(|s| plain.other_of(**s) == semantic.other_of(**s)).count();
    println!("算法对的 {} 章、{} 段里，两种做法对法一样的 {same}（{:.1}%）", fitted.len(), in_fitted.len(), 100.0 * same as f32 / in_fitted.len().max(1) as f32);
    // 对着的两段的余弦（只看两边都是 1-1 的）
    let mut cos: Vec<f32> = semantic.pairs.iter().filter(|(x, y)| semantic.other_of(*x).len() == 1 && semantic.original_of(*y).len() == 1).filter_map(|(x, y)| Some(crate::local::embed::dot(a.get(x)?, b.get(y)?))).collect();
    cos.sort_by(f32::total_cmp);
    if !cos.is_empty() {
        println!("对着的 1-1 两段的余弦：p10 {:.2} / p50 {:.2}", cos[cos.len() / 10], cos[cos.len() / 2]);
    }
    // App 存下的就是加相似度的这一份
    assert_eq!(state.with_db(|c| align::refine_all(c, &model, NOW)).unwrap(), 1);
    assert_eq!(state.with_db(|c| crate::concept::store::alignment_load(c, &en, &zh)).unwrap().unwrap(), semantic);
}
