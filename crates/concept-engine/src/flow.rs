//! 一次概念研究的流程（`decisions/0024`「一次概念研究」「工作流」）：技能声明的几步按顺序跑。
//!
//! - 步骤只有三种：模型、查文献库、网页检索（v0.3 的检索 + 本机下载 + 快照，接口沿用 `research_engine::ports`）。
//!   不让模型自己反复调工具：每步要发什么、最多花多少，发送前都算得出来（`planned`），也能用录制的回复测完整条工作流。
//! - 每做完一步存一次（`Recorder`）；取消、超预算、出错都停在当前这一步，之后再调 `execute` 从 `run.next_step` 接着做，
//!   做完的步骤不重做（网页检索里已经下载的网址也不重下）。
//! - 模型的输出由 `verify::admit` 核对后才进卡片。

use std::sync::atomic::{AtomicBool, Ordering};

use research_engine::model::ModelRef;
use research_engine::ports::{ErrorKind, Fetch, Model, ModelRequest, PortError, Search};
use serde::{Deserialize, Serialize};

use crate::model::{Card, CommentSource, Run, RunStatus, Scope, SourceOrigin, StepRecord};
use crate::prompts::{self, Books, SourceText};
use crate::skill::{Entry, Input, Skill, StepKind};
use crate::verify;

/// 文献库：按检索词找文献（标题、摘要、笔记、标注；有 PDF 的取正文），存来源正文的快照
pub trait Shelf: Send + Sync {
    fn find(&self, terms: &[String], max: usize, cancel: &AtomicBool) -> Result<Vec<ShelfHit>, String>;
    /// 存一份正文快照，返回它的 SHA-256
    fn keep(&self, text: &str) -> Result<String, String>;
    /// 读回来源的正文（核对、审计、接着做时用）
    fn text(&self, source: &CommentSource) -> Result<String, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShelfHit {
    pub item_id: i64,
    pub attachment_id: Option<i64>,
    pub title: String,
    pub text: String,
}

pub trait Recorder {
    fn save(&mut self, run: &Run) -> Result<(), String>;
}

pub struct NoRecorder;

impl Recorder for NoRecorder {
    fn save(&mut self, _: &Run) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub step: String,
    pub done: usize,
    pub total: usize,
    pub message: String,
}

pub struct Ports<'a> {
    pub model: &'a dyn Model,
    pub search: &'a dyn Search,
    pub fetch: &'a dyn Fetch,
    pub shelf: &'a dyn Shelf,
    pub recorder: &'a mut dyn Recorder,
    pub now: &'a dyn Fn() -> String,
    pub progress: &'a dyn Fn(Progress),
    pub cancel: &'a AtomicBool,
}

/// 建一次研究（还没跑）：检查入口、范围和要跑的步骤
#[allow(clippy::too_many_arguments)]
pub fn new_run(uid: &str, skill: &Skill, scope: Scope, targets: Vec<Card>, model: ModelRef, effort: &str, mock: bool, now: String) -> Result<Run, String> {
    if !skill.entries.contains(&scope.entry) {
        return Err(format!("「{}」不能从「{}」开始。", skill.title, scope.entry.label()));
    }
    let needs_scope = skill.steps.iter().any(|s| s.input.contains(&Input::Scope));
    if scope.entry == Entry::Card && targets.is_empty() {
        return Err("先选一张卡片。".into());
    }
    if needs_scope && scope.paragraphs.is_empty() && targets.is_empty() {
        return Err("范围里没有段落。".into());
    }
    if scope.entry == Entry::Question && scope.question.as_deref().is_none_or(|q| q.trim().is_empty()) {
        return Err("先写下问题。".into());
    }
    if scope.entry == Entry::Concept && scope.terms.iter().all(|t| t.trim().is_empty()) {
        return Err("先写下要追踪的概念（中英文检索词）。".into());
    }
    if skill.needs_instruction && scope.instruction.as_deref().is_none_or(|q| q.trim().is_empty()) {
        return Err(format!("「{}」要写一句要求。", skill.title));
    }
    for id in &scope.skip {
        let step = skill.steps.iter().find(|s| &s.id == id).ok_or_else(|| format!("没有步骤「{id}」。"))?;
        if !step.optional {
            return Err(format!("「{}」这一步不能不做。", if step.title.is_empty() { &step.id } else { &step.title }));
        }
    }
    let run = Run {
        uid: uid.to_string(),
        skill: skill.name.clone(),
        skill_title: skill.title.clone(),
        skill_version: skill.version,
        scope: Some(scope),
        model,
        effort: effort.to_string(),
        mock,
        created_at: now.clone(),
        updated_at: now,
        targets,
        ..Default::default()
    };
    if !active_steps(skill, &run).iter().any(|i| skill.steps[*i].kind == StepKind::Model) {
        return Err("至少要留一个模型步骤。".into());
    }
    Ok(run)
}

/// 这次要跑的步骤（下标）：去掉用户不要的可选步骤；默认不跑的要写进 `with`
pub fn active_steps(skill: &Skill, run: &Run) -> Vec<usize> {
    let scope = run.scope.as_ref();
    let skip = |id: &String| scope.is_some_and(|s| s.skip.contains(id));
    let with = |id: &String| scope.is_some_and(|s| s.with.contains(id));
    (0..skill.steps.len()).filter(|i| !skip(&skill.steps[*i].id) && (!skill.steps[*i].off_by_default || with(&skill.steps[*i].id))).collect()
}

/// 发送前算的一步：要发什么、最多多少
#[derive(Clone, Debug)]
pub struct Planned {
    pub step: String,
    pub title: String,
    pub kind: StepKind,
    /// 模型步骤的请求（按现在已有的卡片和来源组）
    pub request: Option<ModelRequest>,
    /// 前面几步还没做出来、但会放进这一步输入里的内容，按最坏情况估的 token 数
    pub extra_input_tokens: u64,
    pub max_results: u32,
}

/// 每个来源最多放多少 token 进输入（估费用用；`SOURCE_CHARS` 个字、按每字 1.3 个 token 算）
pub const SOURCE_TOKENS: u64 = (prompts::SOURCE_CHARS as u64) * 13 / 10;

pub fn planned(skill: &Skill, run: &Run, books: &Books, sources: &[SourceText]) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut extra_cards = 0u64;
    let mut extra_sources = 0u64;
    for i in active_steps(skill, run).into_iter().filter(|i| *i >= run.next_step) {
        let step = &skill.steps[i];
        match step.kind {
            StepKind::Model => {
                let req = prompts::request(skill, step, run, books, sources);
                let mut extra = 0;
                if step.input.contains(&Input::Cards) {
                    extra += extra_cards;
                }
                if step.input.contains(&Input::Sources) {
                    extra += extra_sources;
                }
                extra_cards += step.max_output_tokens as u64;
                out.push(Planned { step: step.id.clone(), title: step.title.clone(), kind: step.kind, request: Some(req), extra_input_tokens: extra, max_results: 0 });
            }
            StepKind::Library | StepKind::Web => {
                extra_sources += step.max_results as u64 * SOURCE_TOKENS;
                out.push(Planned { step: step.id.clone(), title: step.title.clone(), kind: step.kind, request: None, extra_input_tokens: 0, max_results: step.max_results });
            }
        }
    }
    out
}

/// 检索词：范围里的检索词、问题，入口卡片的标题（「division of labour / 劳动分工」拆成两个）
pub fn search_terms(run: &Run) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let add = |out: &mut Vec<String>, s: &str| {
        let s = s.trim().trim_matches(['「', '」', '"', '“', '”']).trim();
        if !s.is_empty() && s.chars().count() <= 60 && !out.iter().any(|x| x.eq_ignore_ascii_case(s)) {
            out.push(s.to_string());
        }
    };
    if let Some(scope) = &run.scope {
        for t in &scope.terms {
            add(&mut out, t);
        }
    }
    for c in &run.targets {
        let title = c.title.split(['：', ':']).next().unwrap_or(&c.title);
        for part in title.split(['/', '／']) {
            add(&mut out, part);
        }
    }
    if out.is_empty()
        && let Some(q) = run.scope.as_ref().and_then(|s| s.question.as_deref())
    {
        add(&mut out, q);
    }
    out
}

/// 来源的正文（发给模型、核对用）
pub fn source_texts(run: &Run, shelf: &dyn Shelf) -> Vec<SourceText> {
    run.sources
        .iter()
        .filter(|s| s.error.is_none() && s.sha256.is_some())
        .filter_map(|s| {
            let text = shelf.text(s).ok()?;
            let where_from = match s.origin {
                SourceOrigin::Library => "文献库".to_string(),
                SourceOrigin::Web => s.final_url.clone().or_else(|| s.url.clone()).unwrap_or_default(),
            };
            Some(SourceText { id: s.id.clone(), title: s.title.clone(), where_from, text })
        })
        .collect()
}

fn record(id: &str, label: impl Into<String>, at: String) -> StepRecord {
    StepRecord { id: id.to_string(), label: label.into(), at, ok: true, ..Default::default() }
}

fn report(ports: &Ports, step: &str, done: usize, total: usize, message: impl Into<String>) {
    (ports.progress)(Progress { step: step.to_string(), done, total, message: message.into() });
}

/// 从 `run.next_step` 接着做到底。返回 Err 只有一种情况：存不进去。取消、超预算、出错看 `run.status` 和 `run.error`
pub fn execute(run: &mut Run, skill: &Skill, books: &Books, ports: &mut Ports) -> Result<(), String> {
    if run.status == RunStatus::Done {
        return Ok(());
    }
    run.status = RunStatus::Running;
    run.error = None;
    run.mock = ports.model.is_mock();
    let steps = active_steps(skill, run);
    let total = steps.len();
    for (n, i) in steps.iter().copied().enumerate() {
        if i < run.next_step {
            continue;
        }
        run.updated_at = (ports.now)();
        ports.recorder.save(run)?;
        if ports.cancel.load(Ordering::SeqCst) {
            run.status = RunStatus::Cancelled;
            return ports.recorder.save(run);
        }
        let step = &skill.steps[i];
        let label = if step.title.is_empty() { step.id.clone() } else { step.title.clone() };
        report(ports, &step.id, n, total, format!("{label}…"));
        let result = match step.kind {
            StepKind::Model => model_step(run, skill, i, books, ports),
            StepKind::Library => library_step(run, i, skill, ports),
            StepKind::Web => web_step(run, i, skill, books, ports),
        };
        match result {
            Ok(()) => {
                run.next_step = i + 1;
            }
            Err(e) => {
                run.status = if e.kind == ErrorKind::Cancelled { RunStatus::Cancelled } else { RunStatus::Failed };
                run.error = (e.kind != ErrorKind::Cancelled).then_some(e.message);
                run.updated_at = (ports.now)();
                return ports.recorder.save(run);
            }
        }
    }
    run.next_step = skill.steps.len();
    run.status = RunStatus::Done;
    run.finished_at = Some((ports.now)());
    run.updated_at = (ports.now)();
    report(ports, "done", total, total, "完成");
    ports.recorder.save(run)
}

fn model_step(run: &mut Run, skill: &Skill, i: usize, books: &Books, ports: &mut Ports) -> Result<(), PortError> {
    let step = &skill.steps[i];
    let sources = source_texts(run, ports.shelf);
    let req = prompts::request(skill, step, run, books, &sources);
    let info = ports.model.info();
    let base = StepRecord { provider: info.provider, model: info.model, mock: ports.model.is_mock(), ..record(&step.id, if step.title.is_empty() { step.id.clone() } else { step.title.clone() }, (ports.now)()) };
    let reply = match ports.model.complete(&req, ports.cancel) {
        Ok(r) => r,
        Err(e) => {
            run.steps.push(StepRecord { ok: false, note: Some(e.message.clone()), input_tokens: e.usage.input_tokens, output_tokens: e.usage.output_tokens, cost_usd: e.usage.cost_usd, ..base });
            return Err(e);
        }
    };
    let rec = StepRecord {
        model: if reply.model.is_empty() { base.model.clone() } else { reply.model.clone() },
        input_tokens: reply.usage.input_tokens,
        output_tokens: reply.usage.output_tokens,
        searches: reply.usage.searches,
        cost_usd: reply.usage.cost_usd,
        raw: Some(reply.text.clone()),
        ..base
    };
    let out = match prompts::parse(&reply.text) {
        Ok(o) => o,
        Err(e) => {
            run.steps.push(StepRecord { ok: false, note: Some(format!("{e}，这一步的结果不能用")), ..rec });
            return Err(PortError { kind: ErrorKind::Failed, message: format!("{e}。这次调用的费用已记进用量，可以点「继续」重试这一步。"), usage: reply.usage });
        }
    };
    let lookup_texts: Vec<(String, String)> = sources.iter().map(|s| (s.id.clone(), s.text.clone())).collect();
    let lookup = |id: &str| lookup_texts.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone());
    let admitted = verify::admit(run, step, out, books, &lookup);
    let note = format!("{} 张卡片、{} 条关系；拦下 {} 句", admitted.cards, admitted.relations, admitted.rejected);
    run.steps.push(StepRecord { note: Some(note), ..rec });
    Ok(())
}

fn library_step(run: &mut Run, i: usize, skill: &Skill, ports: &mut Ports) -> Result<(), PortError> {
    let step = &skill.steps[i];
    let terms = search_terms(run);
    let mut rec = record(&step.id, format!("查文献库：{}", terms.join("、")), (ports.now)());
    let hits = ports.shelf.find(&terms, step.max_results as usize, ports.cancel).map_err(|e| PortError::new(ErrorKind::Failed, e))?;
    let mut added = 0;
    for h in hits.into_iter().take(step.max_results as usize) {
        if run.sources.iter().any(|s| s.item_id == Some(h.item_id)) || h.text.trim().is_empty() {
            continue;
        }
        let sha = ports.shelf.keep(&h.text).map_err(|e| PortError::new(ErrorKind::Failed, e))?;
        let id = run.next_source_id();
        run.sources.push(CommentSource { id, origin: SourceOrigin::Library, title: h.title, item_id: Some(h.item_id), attachment_id: h.attachment_id, url: None, final_url: None, sha256: Some(sha), ext: None, fetched_at: Some((ports.now)()), text_chars: h.text.chars().count(), error: None, added_item: None });
        added += 1;
    }
    rec.note = Some(if added == 0 { "文献库里没找到相关的文献".into() } else { format!("找到 {added} 条") });
    run.steps.push(rec);
    Ok(())
}

fn web_step(run: &mut Run, i: usize, skill: &Skill, books: &Books, ports: &mut Ports) -> Result<(), PortError> {
    let step = &skill.steps[i];
    let terms = search_terms(run);
    let main = terms.iter().find(|t| t.is_ascii()).or(terms.first()).cloned().unwrap_or_default();
    let author = books.original.creators.first().cloned().unwrap_or_default();
    let query = format!("{main} {author} criticism").split_whitespace().collect::<Vec<_>>().join(" ");
    let at = (ports.now)();
    let base = StepRecord { provider: ports.search.engine(), mock: ports.search.is_mock(), ..record(&step.id, format!("网页检索「{query}」"), at) };
    // 接着做：已经搜过、下载过的不再做
    let done: Vec<String> = run.sources.iter().filter_map(|s| s.url.clone()).collect();
    let wanted = step.max_results as usize;
    let have = run.sources.iter().filter(|s| s.origin == SourceOrigin::Web).count();
    if have >= wanted {
        return Ok(());
    }
    let reply = match ports.search.search(&query, wanted * 2, ports.cancel) {
        Ok(r) => r,
        Err(e) => {
            run.steps.push(StepRecord { ok: false, note: Some(e.message.clone()), searches: e.usage.searches, cost_usd: e.usage.cost_usd, ..base });
            if e.stops_the_run() {
                return Err(e);
            }
            return Ok(());
        }
    };
    run.steps.push(StepRecord { searches: reply.usage.searches, cost_usd: reply.usage.cost_usd, input_tokens: reply.usage.input_tokens, output_tokens: reply.usage.output_tokens, note: Some(format!("搜到 {} 条", reply.hits.len())), ..base });
    let mut fetched = have;
    for hit in reply.hits {
        if fetched >= wanted {
            break;
        }
        let url = hit.url.trim().to_string();
        if !(url.starts_with("https://") || url.starts_with("http://")) || done.contains(&url) {
            continue;
        }
        if ports.cancel.load(Ordering::SeqCst) {
            return Err(PortError::cancelled());
        }
        report(ports, &step.id, fetched, wanted, format!("下载：{url}"));
        let id = run.next_source_id();
        let mut src = CommentSource { id, origin: SourceOrigin::Web, title: hit.title.trim().to_string(), item_id: None, attachment_id: None, url: Some(url.clone()), final_url: None, sha256: None, ext: None, fetched_at: None, text_chars: 0, error: None, added_item: None };
        match ports.fetch.fetch(&url, ports.cancel) {
            Ok(f) => {
                src.final_url = Some(f.final_url);
                src.fetched_at = Some(f.fetched_at);
                src.ext = Some(f.ext);
                if src.title.is_empty() {
                    src.title = f.title.unwrap_or_default();
                }
                if let Some(via) = f.via {
                    run.steps.push(StepRecord { provider: via.provider, model: via.model, input_tokens: via.usage.input_tokens, output_tokens: via.usage.output_tokens, cost_usd: via.usage.cost_usd, ..record(&step.id, format!("本机下载失败（{}），改用 Claude 网页抓取", via.download_error), (ports.now)()) });
                }
                match f.text {
                    Ok(t) if !t.trim().is_empty() => {
                        src.text_chars = t.chars().count();
                        src.sha256 = Some(f.sha256);
                        fetched += 1;
                    }
                    Ok(_) => src.error = Some("取不出文字".into()),
                    Err(e) => src.error = Some(e),
                }
            }
            Err(e) => src.error = Some(e),
        }
        run.sources.push(src);
    }
    Ok(())
}
