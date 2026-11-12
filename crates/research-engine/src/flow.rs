//! 一次调查的流程（`decisions/0020`「流程」）：计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告。
//!
//! - `plan`：模型把问题拆成子问题，停在「等用户确认计划」；
//! - `execute`：从 `run.stage` 接着往下做，每做完一步存一次（`Recorder`）。
//!   取消、超预算、出错都停在当前这一步，状态写进 `run.status`，之后可以再调 `execute` 接着做；
//!   已经做完的检索、下载、摘录不重做。
//!
//! 模型只在计划、摘证据、结论三处被调用；核对、可信度、计算都是这里的代码。

use std::sync::atomic::Ordering;

use crate::conclude::{self, Context};
use crate::fold::Folded;
use crate::model::{Config, Fact, Origin, Run, SearchRecord, Source, Stage, Status, Step, StepKind};
use crate::ports::{ErrorKind, Model, ModelReply, PortError, Ports, Progress, Usage};
use crate::{calc, prompts, verify};

fn step(kind: StepKind, label: impl Into<String>, at: String) -> Step {
    Step { kind, label: label.into(), at, provider: String::new(), model: String::new(), input_tokens: 0, output_tokens: 0, searches: 0, cost_usd: 0.0, mock: false, ok: true, note: None }
}

fn model_step(kind: StepKind, label: impl Into<String>, at: String, model: &dyn Model, usage: &Usage, served_by: &str) -> Step {
    let info = model.info();
    Step {
        provider: info.provider,
        model: if served_by.is_empty() { info.model } else { served_by.to_string() },
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        searches: usage.searches,
        cost_usd: usage.cost_usd,
        mock: model.is_mock(),
        ..step(kind, label, at)
    }
}

fn failed(mut s: Step, e: &PortError) -> Step {
    s.ok = false;
    s.note = Some(e.message.clone());
    s
}

fn cancelled(ports: &Ports) -> bool {
    ports.cancel.load(Ordering::SeqCst)
}

fn report(ports: &Ports, stage: Stage, done: usize, total: usize, message: impl Into<String>) {
    (ports.progress)(Progress { stage, done, total, message: message.into() });
}

/// 网址去重用：小写的协议和主机，去掉 #片段、utm_ 参数和结尾的斜线
pub fn normalize_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or(url);
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (host, path) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let kept: Vec<&str> = query.split('&').filter(|p| !p.is_empty() && !p.to_ascii_lowercase().starts_with("utm_")).collect();
    let mut out = format!("{}://{}{}", scheme.to_ascii_lowercase(), host.to_ascii_lowercase(), path.trim_end_matches('/'));
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out
}

fn is_web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn next_number(ids: impl Iterator<Item = impl AsRef<str>>) -> u32 {
    ids.map(|id| conclude::fact_number(id.as_ref())).filter(|n| *n != u32::MAX).max().unwrap_or(0) + 1
}

/// 用户手动加一个来源（网址或本机文件）。已经有同一个网址就不重复加。返回它的编号。
pub fn add_manual_source(run: &mut Run, url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("网址或文件路径不能空着。".into());
    }
    let key = normalize_url(url);
    if let Some(s) = run.sources.iter().find(|s| normalize_url(&s.url) == key) {
        return Ok(s.id.clone());
    }
    let id = format!("S{}", next_number(run.sources.iter().map(|s| &s.id)));
    run.sources.push(Source { id: id.clone(), url: url.to_string(), origin: Origin::Manual, ..Default::default() });
    Ok(id)
}

// ---------- 计划 ----------

/// 第 1 步：模型把问题拆成子问题。成功后停在「等用户确认计划」（`stage` = 检索，`status` = AwaitingPlan）。
/// `web_searches`：允许计划模型用自带的网页搜索摸底几次（0 = 不搜）。
pub fn plan(question: &str, uid: &str, seq: u32, previous: Option<String>, config: Config, web_searches: u32, ports: &mut Ports) -> Result<Run, PortError> {
    let question = question.trim();
    if question.is_empty() {
        return Err(PortError::new(ErrorKind::Failed, "问题不能空着。"));
    }
    let now = (ports.now)();
    report(ports, Stage::Plan, 0, 1, "模型在拆问题…");
    let req = prompts::plan_request(question, &config.limits, web_searches);
    let reply = ports.planner.complete(&req, ports.cancel)?;
    let plan = prompts::parse_plan(&reply.text, &config.limits).map_err(|e| PortError { kind: ErrorKind::Failed, message: format!("{e}。这次调用的费用已记进用量，可以重试。"), usage: reply.usage })?;
    let mut run = Run {
        uid: uid.to_string(),
        question: question.to_string(),
        seq,
        previous,
        created_at: now.clone(),
        updated_at: now.clone(),
        stage: Stage::Search,
        status: Status::AwaitingPlan,
        mock: ports.planner.is_mock(),
        config,
        plan,
        ..Default::default()
    };
    if !reply.hits.is_empty() {
        run.searches.push(SearchRecord { sub_question: String::new(), query: reply.searched.join("；"), engine: "claude_web_search".into(), hits: reply.hits.clone(), error: None, at: now.clone() });
    }
    run.steps.push(model_step(StepKind::Plan, "拆成子问题", now, ports.planner, &reply.usage, &reply.model));
    ports.recorder.save(&run).map_err(|e| PortError { kind: ErrorKind::Failed, message: e, usage: reply.usage })?;
    report(ports, Stage::Plan, 1, 1, "计划好了");
    Ok(run)
}

/// 重查：沿用上一次确认过的计划，直接从检索开始（不再花钱拆一遍问题）
pub fn rerun_from(previous: &Run, uid: &str, config: Config, now: String) -> Run {
    let mut plan = previous.plan.clone();
    plan.problems.clear();
    Run {
        uid: uid.to_string(),
        question: previous.question.clone(),
        seq: previous.seq + 1,
        previous: Some(previous.uid.clone()),
        created_at: now.clone(),
        updated_at: now,
        stage: Stage::Search,
        status: Status::AwaitingPlan,
        mock: previous.mock,
        config,
        plan,
        // 手动加的来源也沿用
        sources: previous.sources.iter().filter(|s| s.origin == Origin::Manual).enumerate().map(|(i, s)| Source { id: format!("S{}", i + 1), url: s.url.clone(), origin: Origin::Manual, ..Default::default() }).collect(),
        ..Default::default()
    }
}

// ---------- 执行 ----------

/// 从 `run.stage` 接着做到底。返回 `Err` 只有一种情况：存不进去。
/// 取消、超预算、出错都算正常返回，看 `run.status` 和 `run.error`。
pub fn execute(run: &mut Run, ports: &mut Ports) -> Result<(), String> {
    if run.status == Status::Done {
        return Ok(());
    }
    run.status = Status::Running;
    run.error = None;
    run.mock = ports.planner.is_mock() && ports.worker.is_mock();
    loop {
        run.updated_at = (ports.now)();
        ports.recorder.save(run)?;
        if cancelled(ports) {
            run.status = Status::Cancelled;
            return ports.recorder.save(run);
        }
        let result = match run.stage {
            Stage::Plan => Err(PortError::new(ErrorKind::Failed, "计划还没有生成。")),
            Stage::Search => search_step(run, ports),
            Stage::Download => download_step(run, ports),
            Stage::Extract => extract_step(run, ports),
            Stage::Verify => verify_step(run, ports),
            Stage::Conclude => conclude_step(run, ports),
            Stage::Report => {
                run.steps.push(step(StepKind::Report, "生成报告", (ports.now)()));
                run.status = Status::Done;
                run.finished_at = Some((ports.now)());
                run.updated_at = (ports.now)();
                report(ports, Stage::Report, 1, 1, "完成");
                return ports.recorder.save(run);
            }
        };
        match result {
            Ok(()) => run.stage = run.stage.next().unwrap_or(Stage::Report),
            Err(e) => {
                run.status = if e.kind == ErrorKind::Cancelled { Status::Cancelled } else { Status::Failed };
                run.error = (e.kind != ErrorKind::Cancelled).then_some(e.message);
                run.updated_at = (ports.now)();
                return ports.recorder.save(run);
            }
        }
    }
}

fn search_step(run: &mut Run, ports: &mut Ports) -> Result<(), PortError> {
    let limits = run.config.limits.clone();
    let todo: Vec<(String, String)> = run
        .plan
        .sub_questions
        .iter()
        .filter(|s| s.enabled)
        .flat_map(|s| s.queries.iter().take(limits.queries_per_sub_question).map(move |q| (s.id.clone(), q.trim().to_string())))
        .filter(|(_, q)| !q.is_empty())
        .collect();
    let engine = ports.search.engine();
    for (i, (sub, query)) in todo.iter().enumerate() {
        if cancelled(ports) {
            return Err(PortError::cancelled());
        }
        // 接着做时：搜过并且成功的不再搜
        if run.searches.iter().any(|r| &r.sub_question == sub && &r.query == query && r.error.is_none()) {
            continue;
        }
        report(ports, Stage::Search, i, todo.len(), format!("搜索：{query}"));
        let at = (ports.now)();
        let base = Step { provider: engine.clone(), mock: ports.search.is_mock(), ..step(StepKind::Search, format!("搜索「{query}」"), at.clone()) };
        run.searches.retain(|r| !(&r.sub_question == sub && &r.query == query));
        match ports.search.search(query, limits.results_per_query, ports.cancel) {
            Ok(reply) => {
                run.steps.push(Step { searches: reply.usage.searches, cost_usd: reply.usage.cost_usd, input_tokens: reply.usage.input_tokens, output_tokens: reply.usage.output_tokens, ..base });
                let hits = reply.hits.into_iter().filter(|h| is_web_url(&h.url)).take(limits.results_per_query).collect();
                run.searches.push(SearchRecord { sub_question: sub.clone(), query: query.clone(), engine: engine.clone(), hits, error: None, at });
            }
            Err(e) => {
                run.steps.push(failed(Step { searches: e.usage.searches, cost_usd: e.usage.cost_usd, ..base }, &e));
                if e.stops_the_run() {
                    return Err(e);
                }
                run.searches.push(SearchRecord { sub_question: sub.clone(), query: query.clone(), engine: engine.clone(), hits: Vec::new(), error: Some(e.message), at });
            }
        }
    }
    collect_sources(run);
    if run.sources.is_empty() {
        let errors: Vec<&str> = run.searches.iter().filter_map(|r| r.error.as_deref()).collect();
        let why = errors.first().map(|e| format!("搜索出错：{e}")).unwrap_or_else(|| "搜索没有返回任何网页。".into());
        return Err(PortError::new(ErrorKind::Failed, format!("没有找到可以读的来源。{why}可以改搜索词，或者手动加网址、文件后再试。")));
    }
    report(ports, Stage::Search, todo.len(), todo.len(), format!("找到 {} 个来源", run.sources.len()));
    Ok(())
}

/// 搜索结果 → 来源清单：按子问题轮流取（每个子问题的第 1 条、再各自的第 2 条……），网址去重，到上限为止
fn collect_sources(run: &mut Run) {
    let limit = run.config.limits.sources;
    let mut seen: Vec<String> = run.sources.iter().map(|s| normalize_url(&s.url)).collect();
    let mut number = next_number(run.sources.iter().map(|s| &s.id));
    // 每个子问题一条队列（它的几次搜索结果按顺序接起来）；计划时摸底搜到的排在最后
    let mut order: Vec<String> = run.plan.sub_questions.iter().filter(|s| s.enabled).map(|s| s.id.clone()).collect();
    order.push(String::new());
    let queues: Vec<(String, Vec<(String, String)>)> = order
        .into_iter()
        .map(|sub| {
            let hits = run.searches.iter().filter(|r| r.sub_question == sub).flat_map(|r| r.hits.iter().map(|h| (h.url.trim().to_string(), h.title.trim().to_string()))).collect();
            (sub, hits)
        })
        .collect();
    let longest = queues.iter().map(|(_, q)| q.len()).max().unwrap_or(0);
    for rank in 0..longest {
        for (sub, queue) in &queues {
            let Some((url, title)) = queue.get(rank) else { continue };
            if !is_web_url(url) {
                continue;
            }
            let key = normalize_url(url);
            if let Some(at) = seen.iter().position(|k| *k == key) {
                // 另一个子问题也搜到了它
                if let Some(s) = run.sources.get_mut(at)
                    && !sub.is_empty()
                    && !s.sub_questions.contains(sub)
                {
                    s.sub_questions.push(sub.clone());
                }
                continue;
            }
            if run.sources.len() >= limit {
                continue;
            }
            seen.push(key);
            run.sources.push(Source { id: format!("S{number}"), url: url.clone(), title: title.clone(), sub_questions: if sub.is_empty() { Vec::new() } else { vec![sub.clone()] }, ..Default::default() });
            number += 1;
        }
    }
}

fn download_step(run: &mut Run, ports: &mut Ports) -> Result<(), PortError> {
    let total = run.sources.len();
    for i in 0..total {
        if cancelled(ports) {
            return Err(PortError::cancelled());
        }
        if run.sources[i].sha256.is_some() || run.sources[i].fetch_error.is_some() {
            continue;
        }
        report(ports, Stage::Download, i, total, format!("下载：{}", run.sources[i].url));
        match ports.fetch.fetch(&run.sources[i].url, ports.cancel) {
            Ok(f) => {
                // 两个网址是同一份内容：后一个不再读
                let same = run.sources.iter().find(|s| s.sha256.as_deref() == Some(f.sha256.as_str())).map(|s| s.id.clone());
                let s = &mut run.sources[i];
                if let Some(other) = same {
                    s.fetch_error = Some(format!("和 {other} 是同一份内容，不重复读"));
                    continue;
                }
                s.final_url = Some(f.final_url);
                s.fetched_at = Some(f.fetched_at);
                s.sha256 = Some(f.sha256);
                s.kind = Some(f.kind);
                s.content_type = f.content_type;
                s.ext = Some(f.ext);
                s.bytes = f.bytes;
                if let Some(t) = f.title.filter(|t| !t.trim().is_empty())
                    && s.title.is_empty()
                {
                    s.title = t.trim().to_string();
                }
                match f.text {
                    Ok(text) if !text.trim().is_empty() => {
                        s.text_chars = text.chars().count();
                        s.text = Some(text);
                    }
                    Ok(_) => s.text_error = Some("取文字失败：文件里没有读到文字".into()),
                    Err(e) => s.text_error = Some(format!("取文字失败：{e}")),
                }
            }
            Err(e) => run.sources[i].fetch_error = Some(e),
        }
        if cancelled(ports) {
            return Err(PortError::cancelled());
        }
    }
    let ok = run.sources.iter().filter(|s| s.usable()).count();
    let no_text = run.sources.iter().filter(|s| s.text_error.is_some()).count();
    let mut s = step(StepKind::Download, format!("下载 {total} 个来源：{ok} 个可读，{} 个下载失败，{no_text} 个取不出文字", total - ok - no_text), (ports.now)());
    if ok == 0 {
        s.ok = false;
        run.steps.push(s);
        let why = run.sources.iter().find_map(|s| s.fetch_error.clone().or(s.text_error.clone())).unwrap_or_default();
        return Err(PortError::new(ErrorKind::Failed, format!("{total} 个来源都读不了（例如：{why}）。可以手动加网址或文件后再试。")));
    }
    run.steps.push(s);
    report(ports, Stage::Download, total, total, format!("{ok} 个来源可读"));
    Ok(())
}

/// 把正文切成不超过 `size` 字的几段（在换行处切；一行就超长时硬切）。返回（各段, 是否还有没读的）
pub fn chunks(text: &str, size: usize, max: usize) -> (Vec<String>, bool) {
    let size = size.max(200);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0;
    let flush = |current: &mut String, current_len: &mut usize, out: &mut Vec<String>| {
        if !current.trim().is_empty() {
            out.push(std::mem::take(current));
        }
        current.clear();
        *current_len = 0;
    };
    for line in text.split_inclusive('\n') {
        let n = line.chars().count();
        if current_len + n > size {
            flush(&mut current, &mut current_len, &mut out);
        }
        if n > size {
            let cs: Vec<char> = line.chars().collect();
            for piece in cs.chunks(size) {
                out.push(piece.iter().collect());
            }
            continue;
        }
        current.push_str(line);
        current_len += n;
    }
    flush(&mut current, &mut current_len, &mut out);
    let more = out.len() > max;
    out.truncate(max);
    (out, more)
}

fn source_text(run: &mut Run, i: usize, ports: &Ports) -> Result<String, String> {
    if let Some(t) = &run.sources[i].text {
        return Ok(t.clone());
    }
    let t = ports.fetch.text(&run.sources[i])?;
    run.sources[i].text = Some(t.clone());
    Ok(t)
}

fn extract_step(run: &mut Run, ports: &mut Ports) -> Result<(), PortError> {
    let (limits, total) = (run.config.limits.clone(), run.sources.len());
    for i in 0..total {
        if cancelled(ports) {
            return Err(PortError::cancelled());
        }
        if run.sources[i].extracted || !run.sources[i].usable() {
            continue;
        }
        let id = run.sources[i].id.clone();
        report(ports, Stage::Extract, i, total, format!("读 {id}：{}", run.sources[i].title));
        let text = match source_text(run, i, ports) {
            Ok(t) => t,
            Err(e) => {
                let s = &mut run.sources[i];
                s.text_error = Some(format!("取文字失败：{e}"));
                continue;
            }
        };
        let (parts, more) = chunks(&text, limits.chunk_chars, limits.chunks_per_source);
        let mut facts: Vec<Fact> = Vec::new();
        let mut problem: Option<String> = None;
        let (mut publisher, mut published) = (String::new(), String::new());
        for (n, part) in parts.iter().enumerate() {
            if cancelled(ports) {
                return Err(PortError::cancelled());
            }
            let req = prompts::extract_request(&run.question, &run.plan, &run.sources[i], part, (n + 1, parts.len()));
            let label = if parts.len() > 1 { format!("从 {id} 摘证据（第 {} 段）", n + 1) } else { format!("从 {id} 摘证据") };
            let at = (ports.now)();
            match ports.worker.complete(&req, ports.cancel) {
                Ok(reply) => {
                    let mut s = model_step(StepKind::Extract, label, at, ports.worker, &reply.usage, &reply.model);
                    match prompts::parse_extract(&reply.text) {
                        Ok(ex) => {
                            if publisher.is_empty() {
                                publisher = ex.publisher;
                            }
                            if published.is_empty() {
                                published = ex.published;
                            }
                            for f in ex.facts {
                                // 同一条（两段都摘到）只留一条
                                if !facts.iter().any(|x| x.metric == f.metric && x.value == f.value && x.unit == f.unit && x.quote == f.quote && x.period == f.period && x.region == f.region) {
                                    facts.push(f);
                                }
                            }
                            if !ex.problems.is_empty() {
                                s.note = Some(ex.problems.join("；"));
                            }
                        }
                        Err(e) => {
                            s.ok = false;
                            s.note = Some(e.clone());
                            problem = Some(format!("模型的回答读不懂：{e}"));
                        }
                    }
                    run.steps.push(s);
                }
                Err(e) => {
                    run.steps.push(failed(model_step(StepKind::Extract, label, at, ports.worker, &e.usage, ""), &e));
                    if e.stops_the_run() {
                        // 这个来源下次接着做时重读；已经摘到的半份不留
                        return Err(e);
                    }
                    problem = Some(e.message);
                }
            }
        }
        let mut number = next_number(run.facts.iter().map(|f| &f.id));
        for mut f in facts {
            f.id = format!("F{number}");
            f.source = id.clone();
            number += 1;
            run.facts.push(f);
        }
        let s = &mut run.sources[i];
        s.extracted = true;
        s.read_chars = parts.iter().map(|p| p.chars().count()).sum();
        s.truncated = more;
        s.extract_error = problem;
        s.publisher = publisher;
        s.published = published;
        run.updated_at = (ports.now)();
        ports.recorder.save(run).map_err(|e| PortError::new(ErrorKind::Failed, e))?;
    }
    report(ports, Stage::Extract, total, total, format!("摘到 {} 条事实", run.facts.len()));
    Ok(())
}

/// 核对全部事实，按等级表定可信度。可以单独调（审计、改了等级表之后）。`text_of` 给出某个来源保存下来的正文。
pub fn verify_all(run: &mut Run, text_of: &mut dyn FnMut(&Source) -> Result<String, String>) {
    let trust = run.config.trust.clone();
    let mut folded: Vec<(String, Result<Folded, String>)> = Vec::new();
    for f in &mut run.facts {
        if !folded.iter().any(|(id, _)| id == &f.source) {
            let text = match run.sources.iter().find(|s| s.id == f.source) {
                Some(s) => match &s.text {
                    Some(t) => Ok(Folded::new(t)),
                    None => text_of(s).map(|t| Folded::new(&t)),
                },
                None => Err("来源不存在".to_string()),
            };
            folded.push((f.source.clone(), text));
        }
        let text = folded.iter().find(|(id, _)| id == &f.source).map(|(_, t)| t);
        f.check = match text {
            Some(Ok(t)) => verify::check_fact(f, t),
            Some(Err(e)) => crate::model::Check { status: crate::model::CheckStatus::Unverified, reason: Some(format!("读不到保存的正文：{e}")), ..Default::default() },
            None => crate::model::Check::default(),
        };
        f.tier = trust.tier(f.producer_kind);
    }
    for s in &mut run.sources {
        s.tier = run.facts.iter().filter(|f| f.source == s.id && f.verified()).map(|f| f.tier).max();
    }
}

fn verify_step(run: &mut Run, ports: &mut Ports) -> Result<(), PortError> {
    report(ports, Stage::Verify, 0, run.facts.len(), "逐字核对摘录和数值…");
    let fetch = ports.fetch;
    verify_all(run, &mut |s| fetch.text(s));
    let ok = run.verified_facts().count();
    run.steps.push(step(StepKind::Verify, format!("核对 {} 条事实：{ok} 条核实，{} 条没核上", run.facts.len(), run.facts.len() - ok), (ports.now)()));
    report(ports, Stage::Verify, run.facts.len(), run.facts.len(), format!("{ok} 条核实"));
    Ok(())
}

fn question_context(run: &Run) -> String {
    let mut text = run.question.clone();
    for s in run.plan.sub_questions.iter().filter(|s| s.enabled) {
        for part in [&s.text, &s.metric, &s.definition, &s.population, &s.region, &s.period] {
            text.push('\n');
            text.push_str(part);
        }
    }
    text
}

/// 按模型的回答算出计算、核对并定稿结论
fn settle(run: &Run, reply: &ModelReply) -> Result<(Vec<crate::model::Calc>, Vec<crate::model::Conclusion>, Vec<String>), String> {
    let mut parsed = prompts::parse_conclude(&reply.text)?;
    for c in &mut parsed.calcs {
        calc::run(c, &run.facts);
    }
    let question_text = question_context(run);
    let ctx = Context { facts: &run.facts, calcs: &parsed.calcs, question_text: &question_text };
    let conclusions = parsed.drafts.iter().enumerate().map(|(i, d)| conclude::finish(&format!("K{}", i + 1), d, &ctx)).collect();
    Ok((parsed.calcs, conclusions, parsed.gaps))
}

fn conclude_step(run: &mut Run, ports: &mut Ports) -> Result<(), PortError> {
    (run.calcs, run.conclusions, run.gaps) = (Vec::new(), Vec::new(), Vec::new());
    if run.verified_facts().count() == 0 {
        run.gaps.push("没有核实到任何事实，给不出结论。可以改搜索词、手动加来源后重查。".into());
        run.steps.push(Step { note: Some("没有已核实的事实，没有调用模型".into()), ..step(StepKind::Conclude, "写结论", (ports.now)()) });
        return Ok(());
    }
    report(ports, Stage::Conclude, 0, 1, "模型在写结论…");
    let req = prompts::conclude_request(&run.question, &run.plan, &run.facts, &run.sources, &run.config.trust, None);
    let at = (ports.now)();
    let reply = match ports.planner.complete(&req, ports.cancel) {
        Ok(r) => r,
        Err(e) => {
            run.steps.push(failed(model_step(StepKind::Conclude, "写结论", at, ports.planner, &e.usage, ""), &e));
            return Err(e);
        }
    };
    let mut first_step = model_step(StepKind::Conclude, "写结论", at, ports.planner, &reply.usage, &reply.model);
    let (calcs, conclusions, gaps) = match settle(run, &reply) {
        Ok(x) => x,
        Err(e) => {
            first_step.ok = false;
            first_step.note = Some(e.clone());
            run.steps.push(first_step);
            return Err(PortError::new(ErrorKind::Failed, format!("模型写的结论读不懂：{e}。这次调用的费用已记进用量，可以点「继续」重试。")));
        }
    };
    let Some(issues) = conclude::feedback(&conclusions) else {
        run.steps.push(first_step);
        (run.calcs, run.conclusions, run.gaps) = (calcs, conclusions, gaps);
        report(ports, Stage::Conclude, 1, 1, "结论的数字都对上了");
        return Ok(());
    };
    // 有对不上的：打回重写一次
    first_step.note = Some(format!("核对发现问题，打回重写：{issues}"));
    run.steps.push(first_step);
    (run.calcs, run.conclusions, run.gaps) = (calcs, conclusions, gaps);
    if cancelled(ports) {
        return Err(PortError::cancelled());
    }
    report(ports, Stage::Conclude, 0, 1, "有数字对不上，请模型重写…");
    let retry = prompts::conclude_request(&run.question, &run.plan, &run.facts, &run.sources, &run.config.trust, Some((&reply.text, &issues)));
    let at = (ports.now)();
    match ports.planner.complete(&retry, ports.cancel) {
        Ok(second) => {
            let mut s = model_step(StepKind::Conclude, "重写结论", at, ports.planner, &second.usage, &second.model);
            match settle(run, &second) {
                Ok((calcs, conclusions, gaps)) => {
                    if let Some(left) = conclude::feedback(&conclusions) {
                        s.note = Some(format!("重写后仍有问题，已在结论旁标出：{left}"));
                    }
                    (run.calcs, run.conclusions, run.gaps) = (calcs, conclusions, gaps);
                }
                Err(e) => {
                    s.ok = false;
                    s.note = Some(format!("重写的回答读不懂（{e}），保留第一稿并标出问题"));
                }
            }
            run.steps.push(s);
        }
        Err(e) => {
            run.steps.push(failed(model_step(StepKind::Conclude, "重写结论", at, ports.planner, &e.usage, ""), &e));
            if e.stops_the_run() {
                return Err(e);
            }
            // 重写没成：保留第一稿，问题已经标在结论上
        }
    }
    report(ports, Stage::Conclude, 1, 1, "结论写好了");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_normalized_for_dedup() {
        assert_eq!(normalize_url("HTTPS://Stats.Example.invalid/Report/2024/?utm_source=x&id=3#top"), "https://stats.example.invalid/Report/2024?id=3");
        assert_eq!(normalize_url("https://a.example.invalid"), normalize_url("https://a.example.invalid/"));
        assert_ne!(normalize_url("https://a.example.invalid/x?id=1"), normalize_url("https://a.example.invalid/x?id=2"));
        assert!(is_web_url(" https://a.example.invalid") && !is_web_url("file:///etc/passwd") && !is_web_url("javascript:alert(1)"));
    }

    #[test]
    fn text_is_cut_at_line_breaks() {
        let text = format!("{}\n{}\n{}\n", "甲".repeat(150), "乙".repeat(150), "丙".repeat(450));
        let (parts, more) = chunks(&text, 200, 10);
        assert_eq!(parts.iter().map(|p| p.chars().count()).collect::<Vec<_>>(), [151, 151, 200, 200, 51]);
        assert!(!more);
        assert_eq!(parts.concat(), text);
        let (parts, more) = chunks(&text, 200, 2);
        assert!(parts.len() == 2 && more);
        assert_eq!(chunks("短短的一段", 20_000, 3), (vec!["短短的一段".to_string()], false));
        assert_eq!(chunks(" \n ", 20_000, 3).0.len(), 0);
    }

    #[test]
    fn manual_sources_get_ids_and_do_not_repeat() {
        let mut run = Run::default();
        assert_eq!(add_manual_source(&mut run, " https://a.example.invalid/x ").unwrap(), "S1");
        assert_eq!(add_manual_source(&mut run, "https://a.example.invalid/x/").unwrap(), "S1");
        assert_eq!(add_manual_source(&mut run, "/Users/someone/表.xlsx").unwrap(), "S2");
        assert!(add_manual_source(&mut run, "  ").is_err());
        assert!(run.sources.iter().all(|s| s.origin == Origin::Manual));
    }
}
