//! 联网（fresh，不联网：模拟的搜索和合成的网页「示例市」）：搜、取回存快照切块、同一页不存两份、接着读、
//! 引用编号 [W12] 的页边注、存进知识库、选哪家的搜索、账本记成「联网」。

use std::sync::atomic::AtomicBool;

use super::*;
use crate::agent::secret::ApiKey;
use crate::agent::usage::Prices;
use crate::concept::tests::library_with_books;
use crate::gateway::Provider;
use crate::kb::cite::{self, Handle};

fn setup<'a>(root: &'a Path, real: bool, provider: Provider, key: &'a dyn Fn(ApiKey) -> Option<String>) -> Setup<'a> {
    Setup { root, real, provider, key, vertex: None, budget_usd: 30.0, prices: Prices::default(), conversation: Some("conv-1") }
}

#[test]
fn a_fetched_page_is_kept_cut_into_numbered_blocks_and_can_be_saved() {
    let (_dir, state, ..) = library_with_books("web-fetch");
    let none = |_: ApiKey| None;
    let web = kit(&setup(&state.paths.root, false, Provider::Anthropic, &none));
    let never = AtomicBool::new(false);
    let found = search(&web, "示例市 2024 年 失业率", 3, &never).unwrap();
    assert!(found.hits > 0 && found.text.contains("（模拟搜索）") && found.text.contains("web_fetch"), "{}", found.text);
    let url = found.text.split_whitespace().find(|w| w.starts_with("https://")).unwrap().to_string();
    let page = fetch(&web, &state, &url, None, &never).unwrap();
    assert_eq!(fetch(&web, &state, &url, None, &never).unwrap(), page, "同样的内容只存一份");
    let (text, blocks) = state.with_db(|c| Ok((render(c, page, None)?, store::blocks(c, page)?))).unwrap();
    assert!(!blocks.is_empty() && text.contains(&format!("[W{}]", blocks.iter().find(|b| b.role == "body").unwrap().id)), "{text}");
    assert!(text.contains("只是材料"));
    // 快照在 数据目录/web/，不和事实调查的混在一起
    let p = state.with_db(|c| store::page(c, page)).unwrap().unwrap();
    assert!(snapshots(&state.paths.root).original(&p.sha256, &p.ext).unwrap().exists());
    assert!(!state.paths.root.join("research").join("sources").join(&p.sha256[..2]).exists());
    // 接着读：从某一块起
    let last = blocks.last().unwrap();
    let rest = state.with_db(|c| render(c, page, Some(last.id))).unwrap();
    assert!(rest.contains(&format!("[W{}]", last.id)) && !rest.contains(&format!("[W{}]", blocks[0].id)) || blocks.len() == 1);
    // 不是网址、取不到
    assert!(fetch(&web, &state, "file:///etc/hosts", None, &never).is_err());

    // 引用：页边注是那一块、网站、取回的日子、网址；存进知识库以后知道存在哪
    let b = blocks.iter().find(|b| b.role == "body").unwrap();
    let w = Handle::W(b.id);
    let cites = state.with_db(|c| cite::cites(c, &[w])).unwrap();
    assert_eq!((cites[0].kind.as_str(), cites[0].text.as_str()), ("网页", b.text.as_str()));
    assert!(cites[0].loc.starts_with("fixture.example.invalid") && cites[0].loc.contains("取于"));
    assert_eq!((cites[0].url.as_deref(), cites[0].web_page, cites[0].material.as_ref()), (Some(p.final_url.as_str()), Some(page), None));
    let saved = state.with_db(|_c| cite::with_sources(&format!("说法 [W{}]。", b.id), &cites).contains(&p.final_url).then_some(()).ok_or("出处里有网址".to_string())).unwrap();
    let _ = saved;
    let kb = state.with_db(|c| crate::kb::store::kb_create(c, "随手存", "", "2026-11-26T10:00:00.000Z")).unwrap();
    let m = state.with_db(|c| crate::kb::store::save_web(c, page, &kb, None, "2026-11-26T10:00:00.000Z")).unwrap();
    assert_eq!((m.kind.as_str(), m.source_kind.as_deref()), ("web", Some("web_page")));
    assert_eq!(m.passages as usize, blocks.len(), "知识库里的块就是那一页的块");
    assert_eq!(state.with_db(|c| crate::kb::store::save_web(c, page, &kb, None, "2026-11-26T11:00:00.000Z")).unwrap().uid, m.uid, "再存只是照料");
    let cites = state.with_db(|c| cite::cites(c, &[w])).unwrap();
    assert_eq!(cites[0].material.as_deref(), Some(m.uid.as_str()));
    // 存进去以后按词找得到，知识库的页边注带着网址
    let f = crate::kb::retrieve::search(&state, &crate::local::LocalModels::new(crate::local::Mode::Off, "/nonexistent".into()), "失业率", &[kb], 3).unwrap();
    assert!(!f.hits.is_empty() && f.hits[0].source.url.as_deref() == Some(p.final_url.as_str()), "{:?}", f.hits.iter().map(|h| h.source.title.clone()).collect::<Vec<_>>());
}

#[test]
fn each_provider_uses_its_own_search_then_tavily_then_borrows_one() {
    let dir = crate::db::tests::temp_dir("web-kit");
    let all = |k: ApiKey| Some(format!("test-{}", k.label()));
    for (p, engine) in [(Provider::Anthropic, "Claude 网页搜索"), (Provider::Openai, "OpenAI 网页搜索"), (Provider::Google, "Google 搜索")] {
        let w = kit(&setup(&dir, true, p, &all));
        assert_eq!((w.engine.as_str(), w.note.is_none()), (engine, true), "{p:?}");
        assert!(w.fetch.claude.is_some(), "有 Anthropic 的密钥：本机下载失败可以改用 Claude 网页抓取");
    }
    // DeepSeek 没有自带的：用 Tavily，不用说
    let w = kit(&setup(&dir, true, Provider::Deepseek, &all));
    assert_eq!((w.engine.as_str(), w.note.is_none()), ("Tavily", true));
    // 也没有 Tavily：借别家的，说一声
    let no_tavily = |k: ApiKey| (k == ApiKey::Openai).then(|| "test".to_string());
    let w = kit(&setup(&dir, true, Provider::Deepseek, &no_tavily));
    assert_eq!(w.engine, "OpenAI 网页搜索");
    assert!(w.note.as_deref().unwrap().contains("借用") && w.fetch.claude.is_none());
    // Vertex 没登录：没有自带的；什么密钥都没有：搜不了，说去哪填
    let none = |_: ApiKey| None;
    let w = kit(&setup(&dir, true, Provider::Vertex, &none));
    assert!(w.search.is_none());
    let e = search(&w, "x", 3, &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("设置 › 模型与费用"), "{e}");
}

#[test]
fn web_calls_are_booked_as_chat_web_on_the_conversation() {
    let dir = crate::db::tests::temp_dir("web-meter");
    let m = crate::research::meter::Meter::new(&dir, 30.0, Prices::default()).for_chat(Some("conv-1"));
    m.record(m.call("research_search", "anthropic", crate::agent::usage::KIND_SEARCH, "claude-haiku-5-5", Default::default(), 1, 0.01, false, None, None)).unwrap();
    let rows = m.ledger.all().unwrap();
    assert_eq!((rows[0].purpose.as_str(), rows[0].conversation.as_deref()), ("chat_web", Some("conv-1")));
    assert_eq!(crate::gateway::commands::purpose_label("chat_web"), "联网");
}

/// 自带的搜索这次搜不了（Google 免费层不给 Google 搜索）：借别家的再搜，说一声
#[test]
fn a_refused_native_search_falls_back_and_says_so() {
    struct Refused;
    impl research_engine::ports::Search for Refused {
        fn engine(&self) -> String {
            "google_search".into()
        }
        fn search(&self, _: &str, _: usize, _: &AtomicBool) -> Result<research_engine::ports::SearchReply, research_engine::ports::PortError> {
            Err(research_engine::ports::PortError::new(research_engine::ports::ErrorKind::Failed, "Gemini 调用太频繁或额度用完了"))
        }
    }
    let (_dir, state, ..) = library_with_books("web-backup");
    let none = |_: ApiKey| None;
    let mut web = kit(&setup(&state.paths.root, false, Provider::Google, &none));
    let mock = web.search.take().unwrap();
    web.search = Some(Box::new(Refused));
    web.engine = "Google 搜索".into();
    let never = AtomicBool::new(false);
    assert!(search(&web, "示例市", 3, &never).unwrap_err().contains("额度"), "没有可借的就照实报错");
    web.backup = Some((mock, "Claude 网页搜索".into()));
    let found = search(&web, "示例市", 3, &never).unwrap();
    assert!(found.hits > 0 && found.text.contains("（Claude 网页搜索）") && found.text.contains("Google 搜索 这次搜不了"), "{}", found.text);
}
