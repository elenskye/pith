//! 在合成夹具上跑一遍调查，把报告打印出来（Markdown；加参数 `html` 出 HTML）。不联网。
//! `cargo run -p research-engine --example report [html] [2]`：最后的 2 = 再重查一次并带上对比。

use std::sync::atomic::AtomicBool;

use research_engine::fixture::{MockFetch, MockModel, MockSearch, QUESTION};
use research_engine::model::{Config, ModelRef, Run};
use research_engine::ports::{NoRecorder, Ports, Progress};
use research_engine::{flow, render, report};

fn run(round: u32, previous: Option<&Run>) -> Run {
    let (planner, worker, search, fetch) = (MockModel::new("模拟计划模型", round), MockModel::new("模拟执行模型", round), MockSearch::new(round), MockFetch::new(round));
    let (cancel, mut recorder) = (AtomicBool::new(false), NoRecorder);
    let now = || "2026-11-12T09:00:00".to_string();
    let quiet = |_: Progress| {};
    let mut ports = Ports { planner: &planner, worker: &worker, search: &search, fetch: &fetch, recorder: &mut recorder, now: &now, progress: &quiet, cancel: &cancel };
    let config = Config { planner: ModelRef { provider: "mock".into(), model: "模拟计划模型".into() }, worker: ModelRef { provider: "mock".into(), model: "模拟执行模型".into() }, search_engine: "mock".into(), ..Default::default() };
    let mut run = match previous {
        Some(p) => flow::rerun_from(p, "示例-2", config, now()),
        None => flow::plan(QUESTION, "示例-1", 1, None, config, 0, &mut ports).expect("计划"),
    };
    flow::execute(&mut run, &mut ports).expect("执行");
    run
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = run(1, None);
    let (shown, previous) = if args.iter().any(|a| a == "2") { (run(2, Some(&first)), Some(&first)) } else { (first.clone(), None) };
    let doc = render::document(&report::build(&shown, previous));
    println!("{}", if args.iter().any(|a| a == "html") { render::html(&doc) } else { render::markdown(&doc) });
}
