//! 生成浏览器 `?mock` 模式用的示例数据（前端 `src/dev/mockResearch.json`）：合成夹具上的一次调查（刚出计划 / 做完）和一次重查（带新旧对比）。
//! 引擎改了以后重新生成：`cargo run -q -p research-engine --example mock_data > src/dev/mockResearch.json`

use std::sync::atomic::AtomicBool;

use research_engine::fixture::{MockFetch, MockModel, MockSearch, QUESTION};
use research_engine::model::{Config, ModelRef, Run};
use research_engine::ports::{NoRecorder, Ports, Progress};
use research_engine::{flow, report};
use serde_json::json;

fn config() -> Config {
    Config { planner: ModelRef { provider: "mock".into(), model: "模拟计划模型".into() }, worker: ModelRef { provider: "mock".into(), model: "模拟执行模型".into() }, search_engine: "mock".into(), ..Default::default() }
}

fn ports_run(round: u32, f: impl FnOnce(&mut Ports) -> Run) -> Run {
    let (planner, worker, search, fetch) = (MockModel::new("模拟计划模型", round), MockModel::new("模拟执行模型", round), MockSearch::new(round), MockFetch::new(round));
    let (cancel, mut recorder) = (AtomicBool::new(false), NoRecorder);
    let now = || "2026-11-12T09:00:00.000Z".to_string();
    let quiet = |_: Progress| {};
    let mut ports = Ports { planner: &planner, worker: &worker, search: &search, fetch: &fetch, recorder: &mut recorder, now: &now, progress: &quiet, cancel: &cancel };
    f(&mut ports)
}

fn main() {
    let planned = ports_run(1, |p| flow::plan(QUESTION, "MOCKRUN1", 1, None, config(), 0, p).expect("计划"));
    let done = ports_run(1, |p| {
        let mut run = planned.clone();
        flow::execute(&mut run, p).expect("执行");
        run
    });
    let rerun = ports_run(2, |p| {
        let mut run = flow::rerun_from(&done, "MOCKRUN2", config(), "2026-12-01T09:00:00.000Z".into());
        flow::execute(&mut run, p).expect("重查");
        run
    });
    let out = json!({
        "planned": { "run": planned, "report": report::build(&planned, None) },
        "done": { "run": done, "report": report::build(&done, None) },
        "rerun": { "run": rerun, "report": report::build(&rerun, Some(&done)) },
    });
    println!("{}", serde_json::to_string(&out).expect("JSON"));
}
