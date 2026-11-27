//! 1.1.0 第 5 期的真实验收（默认忽略；联网、花钱，上限 $20，`decisions/0034`）。
//!
//! 和 App 走同一条路：真模型（网关）、真的本机检索（两本真《国富论》在知识库的「暂存箱」里）、真的联网，
//! 工具环境是 App 用的 `ChatEnv`，回答照 App 的样子存进对话。数据目录用 `PITH_ACCEPT_DIR`（一个测试目录，不是正式的），
//! 打包版设 `PITH_DATA_DIR` 指过去就能在 App 里看这些对话。
//!
//! ```bash
//! PITH_LOCAL_MODELS=real PITH_API_KEY_FILE=<名字=密钥 的文件> PITH_ACCEPT_DIR=local/acceptance-1.1.0/data \
//! PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub \
//! PITH_ROUNDS=anthropic,openai,deepseek,google,chapter,web,followup [PITH_VERTEX_PROJECT=<项目 ID>] cargo test -p pith real_chat_acceptance -- --ignored --nocapture
//! ```
//! 每一轮打印：模型、怎么结束的、状态行、花了多少（含缓存命中）、引用编号对上了几个、回答。整个目录的账本超过 $20 就不发（网关的月上限）。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::run::{self, ChatEnv};
use super::store;
use crate::AppState;
use crate::agent::secret::ApiKey;
use crate::gateway::catalog;
use crate::gateway::http::{Credential, HttpModel};
use crate::gateway::meter::Meter;
use crate::gateway::Model;
use crate::harness::{self, Ctx, Event, Limits, prompt, tools::Toolbox};
use crate::kb::cite;

/// 验收的上限（这个目录的账本按月算）
const BUDGET: f64 = 20.0;

fn path_of(var: &str) -> PathBuf {
    let p = PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("要设 {var}")));
    if p.is_absolute() { p } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(p) }
}

/// 两本书在不在；不在就解析、迁进暂存箱、补向量
fn shelve(state: &AppState) -> String {
    let have = state.with_db(|c| crate::concept::store::texts_list(c)).unwrap();
    if have.is_empty() {
        state
            .with_db(|c| {
                let (et, em) = crate::concept::commands::parse_epub(&std::fs::read(path_of("PITH_BOOK_EN")).unwrap())?;
                let en = crate::concept::commands::save_text(c, &state.paths.backups, &et, &em, None, None, "accept-en", None)?;
                let (zt, zm) = crate::concept::commands::parse_epub(&std::fs::read(path_of("PITH_BOOK_ZH")).unwrap())?;
                crate::concept::commands::save_text(c, &state.paths.backups, &zt, &zm, None, None, "accept-zh", Some(&en))?;
                c.execute("DELETE FROM settings WHERE key = 'kb.v11'", []).map_err(crate::common::err)?;
                crate::kb::store::upgrade(c, &crate::text::now_iso())?;
                Ok(())
            })
            .unwrap();
    }
    // 解析器换过版本的（第 5 期修了页码不能一直往下带）：从书重新解析，知识库里的那份跟着重抄
    for t in state.with_db(|c| crate::concept::store::texts_list(c)).unwrap() {
        if t.parser == concept_engine::text::PARSER_VERSION {
            continue;
        }
        let file = if t.language == "en" { "PITH_BOOK_EN" } else { "PITH_BOOK_ZH" };
        let (text, meta) = crate::concept::commands::parse_epub(&std::fs::read(path_of(file)).unwrap()).unwrap();
        let r = state.with_db(|c| crate::concept::reparse::apply(c, &state.paths.backups, &crate::concept::library::snapshots(&state.paths.root), &t.uid, &text, &meta, &t.source_sha256)).unwrap();
        println!("重新解析 {}（{} → {}）：{} 段", t.language, t.parser, concept_engine::text::PARSER_VERSION, r.paragraphs);
    }
    let started = std::time::Instant::now();
    let made = crate::kb::index::index_all(state, &state.local, &AtomicBool::new(false), &|_, _| {}).unwrap();
    if made > 0 {
        println!("补向量 {made} 块，{:.0} 秒", started.elapsed().as_secs_f32());
    }
    let m = state.local.model(crate::local::Svc::Embedding).unwrap().key;
    let n = state.with_db(|c| crate::kb::align::refine_all(c, &m, &crate::text::now_iso())).unwrap();
    if n > 0 {
        println!("用向量重新对齐：{n} 对");
    }
    state.with_db(|c| crate::kb::store::kbs_list(c)).unwrap().into_iter().find(|k| k.name == crate::kb::store::INBOX).expect("暂存箱").uid
}

struct Round {
    name: &'static str,
    /// 对话用的模型（清单 id；子代理用这一家最强的，思考照清单的默认）
    model: &'static str,
    question: &'static str,
    /// 接着哪一轮的对话问
    after: Option<&'static str>,
}

const ROUNDS: &[Round] = &[
    Round { name: "anthropic", model: "anthropic/claude-sonnet-5-5", question: "斯密为什么说分工受市场范围的限制？请用书里的原话说明，每个说法标出处。", after: None },
    Round { name: "followup", model: "anthropic/claude-sonnet-5-5", question: "那他为什么特别强调水运？", after: Some("anthropic") },
    Round { name: "openai", model: "openai/gpt-6.2-sol", question: "斯密的别针工厂例子是怎么讲的？他说分工提高效率有哪三个原因？引原文。", after: None },
    Round { name: "deepseek", model: "deepseek/deepseek-flash", question: "「看不见的手」在《国富论》里出现在哪里？原话怎么说，上下文在讲什么？", after: None },
    Round { name: "google", model: "google/gemini-3.8-flash", question: "斯密怎么区分生产性劳动和非生产性劳动？举书里的例子，标出处。", after: None },
    Round { name: "chapter", model: "anthropic/claude-sonnet-5-5", question: "把第一篇第三章整章读一遍，告诉我这一章的论证是怎么一步步展开的，每一步标出处。", after: None },
    Round { name: "web", model: "anthropic/claude-sonnet-5-5", question: "《国富论》第一版是哪一年、在哪里出版的？上网查一下，给出处。再说说它和书里斯密自己讲市场范围的那段有没有关系（不用勉强联系）。", after: None },
    Round { name: "longchapter", model: "anthropic/claude-sonnet-5-5", question: "把第四篇第二章整章读一遍，概括斯密反对限制进口的论证是怎么展开的，每一步标出处。", after: None },
    Round { name: "web_openai", model: "openai/gpt-6.2-sol", question: "亚当·斯密是哪一年去世的、葬在哪里？上网查一下，给出处。", after: None },
    Round { name: "web_google", model: "google/gemini-3.8-flash", question: "格拉斯哥大学现在有没有以亚当·斯密命名的学院或商学院？上网查一下，给出处。", after: None },
    Round { name: "google_lite", model: "google/gemini-3.5-flash-lite", question: "斯密怎么区分生产性劳动和非生产性劳动？举书里的例子，标出处。", after: None },
    Round { name: "web_google_lite", model: "google/gemini-3.5-flash-lite", question: "格拉斯哥大学现在有没有以亚当·斯密命名的学院或商学院？上网查一下，给出处。", after: None },
    Round { name: "vertex", model: "vertex/gemini-3.8-flash", question: "斯密怎么区分生产性劳动和非生产性劳动？举书里的例子，标出处。", after: None },
    Round { name: "vertex_lite", model: "vertex/gemini-3.5-flash-lite", question: "斯密怎么区分生产性劳动和非生产性劳动？举书里的例子，标出处。", after: None },
    Round { name: "web_vertex", model: "vertex/gemini-3.5-flash-lite", question: "格拉斯哥大学现在有没有以亚当·斯密命名的学院或商学院？上网查一下，给出处。", after: None },
    Round { name: "web_deepseek", model: "deepseek/deepseek-flash", question: "斯密的《道德情操论》第一版是哪一年出版的？上网查一下，给出处。", after: None },
];

#[test]
#[ignore]
fn real_chat_acceptance() {
    let root = path_of("PITH_ACCEPT_DIR");
    std::fs::create_dir_all(&root).unwrap();
    let state = AppState::init(crate::paths::DataPaths::new(root.clone()));
    assert!(state.startup_error().is_none(), "{:?}", state.startup_error());
    assert_eq!(state.local.mode(), crate::local::Mode::Real, "先设 PITH_LOCAL_MODELS=real");
    let keys = std::fs::read_to_string(path_of("PITH_API_KEY_FILE")).expect("要设 PITH_API_KEY_FILE");
    let key = |k: ApiKey| k.find_in(&keys);
    let shelf = shelve(&state);
    let wanted: Vec<String> = std::env::var("PITH_ROUNDS").unwrap_or_else(|_| "anthropic".into()).split(',').map(|s| s.trim().to_string()).collect();
    let prices = crate::research::commands::settings(&state).prices;
    // Vertex 的几轮：PITH_VERTEX_PROJECT（区域 PITH_VERTEX_LOCATION，空着是 global），认证用 gcloud 的 ADC
    let vertex = std::env::var("PITH_VERTEX_PROJECT").ok().filter(|p| !p.is_empty()).map(|project| crate::gateway::vertex::VertexConfig { project, location: std::env::var("PITH_VERTEX_LOCATION").unwrap_or_default() });
    let mut convs: Vec<(&str, String)> = Vec::new();
    let mut total = 0.0;
    for r in ROUNDS.iter().filter(|r| wanted.iter().any(|w| w == r.name)) {
        let main = catalog::default_pick(r.model);
        let provider = main.model.provider;
        let strong = catalog::strongest(provider).settle(None);
        let credential = || match crate::gateway::keys::key_of(provider) {
            Some(which) => Credential::Key(key(which).unwrap_or_else(|| panic!("密钥文件里没有 {} 的", provider.label()))),
            None => Credential::Adc,
        };
        let model = HttpModel::new(main.model, credential(), vertex.as_ref()).unwrap();
        let strong_model = HttpModel::new(strong.model, credential(), vertex.as_ref()).unwrap();
        // 接着问：这一次跑过的那一轮，或者以前跑过、标题是那一轮问题的最近一段对话
        let earlier = r.after.and_then(|a| {
            convs.iter().find(|(n, _)| *n == a).map(|(_, u)| u.clone()).or_else(|| {
                let q = ROUNDS.iter().find(|x| x.name == a)?.question;
                state.with_db(|c| store::list(c)).ok()?.into_iter().find(|s| s.title == store::title_of(q)).and_then(|s| s.uid)
            })
        });
        let uid = match earlier {
            Some(u) => u,
            None => state.with_db(|c| store::create(c, r.question, "pith", std::slice::from_ref(&shelf), None, None, &crate::text::now_iso())).unwrap(),
        };
        convs.push((r.name, uid.clone()));
        let (conv, role) = state
            .with_db(|c| {
                let conv = store::get(c, &uid)?;
                let body = run::you_body(c, &conv, r.question, "pith", &crate::text::now_iso()[..10])?;
                store::add_message(c, &uid, "you", &body, &crate::text::now_iso())?;
                Ok((store::get(c, &uid)?, store::role_get(c, "pith")?))
            })
            .unwrap();
        let meter = Meter::new(&root, BUDGET, prices.clone());
        let cancel = AtomicBool::new(false);
        let env = ChatEnv {
            state: &state,
            kbs: conv.kbs.clone(),
            conversation: Some(uid.clone()),
            cancel: &cancel,
            backfill: Box::new(|| {}),
            make_web: Box::new(|| {
                crate::web::kit(&crate::web::Setup { root: &root, real: true, provider, key: &key, vertex: vertex.clone(), budget_usd: BUDGET, prices: prices.clone(), conversation: Some(&uid) })
            }),
            web: std::sync::OnceLock::new(),
        };
        let seen = Mutex::new(0usize);
        let sink = |e: Event| {
            if let Event::Step(s) = e {
                let mut n = seen.lock().unwrap();
                *n += 1;
                println!("    {}{} {}", if s.sub.is_some() { "  └ " } else { "" }, s.agent, s.text);
            }
        };
        println!("\n== {}（{}）：{}", r.name, main.model.label, r.question);
        let started = std::time::Instant::now();
        let ctx = Ctx { model: &model, effort: main.effort, strong: Some((&strong_model as &dyn Model, strong.effort)), meter: &meter, conversation: Some(&uid), env: &env, cancel: &cancel, sink: &sink, limits: Limits::default() };
        let o = harness::run(&ctx, &prompt::system(&role), &Toolbox::main(), run::history(&conv), "chat");
        let cites = state.with_db(|c| cite::cites(c, &cite::handles(&o.text))).unwrap();
        let unresolved = cite::handles(&o.text).len() - cites.len();
        let body = run::reply_body(&o, main.model.label, None, &cites);
        state.with_db(|c| store::add_message(c, &uid, "pith", &body, &crate::text::now_iso())).unwrap();
        total += o.cost_usd;
        println!(
            "  结束：{:?}{}；{:.0} 秒；${:.4}（输入 {}，输出 {}，缓存读 {}，缓存写 {}）；子代理 {}；实际模型 {}",
            o.ending,
            o.note.as_ref().map(|n| format!("（{n}）")).unwrap_or_default(),
            started.elapsed().as_secs_f32(),
            o.cost_usd,
            o.tokens.input,
            o.tokens.output,
            o.tokens.cache_read,
            o.tokens.cache_write,
            o.subagents,
            o.model
        );
        println!("  引用：{} 个编号对上 {} 个，没对上 {unresolved} 个", cites.len() + unresolved, cites.len());
        for x in &cites {
            println!("    [{}] {} {} · {}", x.n, x.kind, x.src.chars().take(40).collect::<String>(), x.loc);
        }
        println!("  回答：\n{}\n", body["paragraphs"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("\n\n"));
        drop(env);
    }
    state.local.stop_ours_on_exit();
    let ledger = crate::agent::usage::Ledger::new(&root).all().unwrap();
    let month: f64 = ledger.iter().filter(|c| !c.mock).map(|c| c.cost_usd).sum();
    println!("这一次 ${total:.4}；这个目录的账本一共 ${month:.4}（{} 行）", ledger.len());
    for (n, u) in &convs {
        println!("对话 {n}：{u}");
    }
}
