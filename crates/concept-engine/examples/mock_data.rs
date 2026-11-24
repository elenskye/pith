//! 前端 `?mock` 的示例数据：引擎在夹具（公有领域英文本 + 合成中文）上跑一遍七个技能（模拟模型、录制的回复），
//! 把文本、对齐、结构预览、各次研究和关系图写成一份 JSON。
//!
//! ```bash
//! cargo run -q -p concept-engine --example mock_data > src/dev/mockConcept.json
//! ```

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use concept_engine::flow::{self, NoRecorder, Ports};
use concept_engine::mock::{MockFetch, MockModel, MockSearch, MockShelf, Store};
use concept_engine::model::{Card, Run, Scope};
use concept_engine::prompts::Books;
use concept_engine::skill::{self, Entry};
use concept_engine::text::Text;
use concept_engine::{align, audit, fixture, graph, text};
use research_engine::model::ModelRef;
use serde_json::{Value, json};

fn scope(entry: Entry, paragraphs: Vec<String>) -> Scope {
    Scope { entry, text: "MOCK-EN".into(), translation: Some("MOCK-ZH".into()), paragraphs, unit: None, question: None, terms: Vec::new(), cards: Vec::new(), instruction: None, skip: Vec::new(), with: Vec::new() }
}

fn chapter(t: &Text, unit: &str) -> Scope {
    let u = t.chapters().into_iter().find(|u| u.key == unit).unwrap().index;
    Scope { unit: Some(unit.into()), ..scope(Entry::Chapter, t.body_of(u).map(|p| p.key.clone()).collect()) }
}

fn main() {
    let en = fixture::english();
    let zh = fixture::chinese();
    let al = align::align(&en, &zh);
    let books = Books { original: &en, translation: Some(&zh), alignment: Some(&al) };
    let skills = skill::builtin();
    let store = Store::default();
    let shelf = MockShelf { store: store.clone(), has_review: true };
    let fetch = MockFetch { store: store.clone() };
    let mut runs: BTreeMap<String, Run> = BTreeMap::new();
    let mut go = |key: &str, name: &str, scope: Scope, targets: Vec<Card>| -> Run {
        let sk = skills.iter().find(|s| s.name == name).unwrap();
        let uid = format!("MOCK-{}", key.replace(':', "-"));
        let mut run = flow::new_run(&uid, sk, scope, targets, ModelRef { provider: "mock".into(), model: "模拟模型".into() }, "medium", true, "2026-11-13T10:00:00".into()).unwrap();
        let model = MockModel::default();
        let mut rec = NoRecorder;
        let now = || "2026-11-13T10:00:30".to_string();
        let progress = |_| {};
        let cancel = AtomicBool::new(false);
        let mut ports = Ports { model: &model, search: &MockSearch, fetch: &fetch, shelf: &shelf, recorder: &mut rec, now: &now, progress: &progress, cancel: &cancel };
        flow::execute(&mut run, sk, &books, &mut ports).unwrap();
        runs.insert(key.to_string(), run.clone());
        run
    };
    let base = go("chapter:I.i", "chapter", chapter(&en, "I.i"), Vec::new());
    go("chapter:I.ii", "chapter", chapter(&en, "I.ii"), Vec::new());
    go("chapter:I.iii", "chapter", chapter(&en, "I.iii"), Vec::new());
    go("passage", "passage", scope(Entry::Selection, vec!["I.i.3".into()]), Vec::new());
    go("ask", "ask", Scope { question: Some("分工为什么受市场范围的限制？".into()), ..scope(Entry::Question, vec!["I.iii.1".into(), "I.iii.2".into()]) }, Vec::new());
    go("trace", "trace", Scope { terms: vec!["division of labour".into(), "分工".into()], ..scope(Entry::Concept, vec!["I.i.1".into(), "I.ii.1".into(), "I.iii.1".into()]) }, Vec::new());
    let k1 = base.cards[0].clone();
    let k5 = base.cards[4].clone();
    go("critics", "critics", Scope { cards: vec![(base.uid.clone(), k1.id.clone())], with: vec!["web".into()], ..scope(Entry::Card, Vec::new()) }, vec![k1]);
    go("counterfactual", "counterfactual", Scope { cards: vec![(base.uid.clone(), k5.id.clone())], instruction: Some("如果市场很小".into()), ..scope(Entry::Card, vec!["I.iii.1".into()]) }, vec![k5]);
    go("free", "free", Scope { instruction: Some("做一份术语表".into()), unit: Some("I.i".into()), ..chapter(&en, "I.i") }, Vec::new());
    let mut svgs = BTreeMap::new();
    let mut findings = BTreeMap::new();
    let mut source_texts = BTreeMap::new();
    for (k, r) in &runs {
        svgs.insert(k.clone(), graph::svg(&graph::layout(r)));
        let texts: Vec<(String, String)> = r.sources.iter().filter_map(|s| s.sha256.as_deref().and_then(|sha| store.get(sha)).map(|t| (s.id.clone(), t))).collect();
        for s in &r.sources {
            if let Some(sha) = &s.sha256
                && let Some(t) = store.get(sha)
            {
                source_texts.insert(sha.clone(), t);
            }
        }
        let lookup = |id: &str| texts.iter().find(|(x, _)| x == id).map(|(_, t)| t.clone());
        findings.insert(k.clone(), audit::audit(r, &books, &lookup));
    }
    let out: Value = json!({
        "en": en,
        "zh": zh,
        "alignment": al,
        "previewEn": text::preview(&en),
        "previewZh": text::preview(&zh),
        "skills": skills,
        "runs": runs,
        "svgs": svgs,
        "findings": findings,
        "sourceTexts": source_texts,
    });
    println!("{}", serde_json::to_string(&out).unwrap());
}
