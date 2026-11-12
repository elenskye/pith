//! 调查在主库里的存取（migration 0006，`decisions/0020`「代码与数据」）。
//!
//! 引擎的一份 `Run` 拆进 research_* 各表：每次保存在一个事务里把这次调查的行整份换掉（一次调查最多几十个来源、几百条事实）。
//! 读回来是同一份 `Run`（来源的正文不在库里，在快照的 .txt 里）。

use std::collections::{HashMap, HashSet};

use research_engine::dec::Dec;
use research_engine::fold::Span;
use research_engine::model::{Calc, Check, Conclusion, Fact, Run, SearchRecord, Segment, Source, Step};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::common::{Tx, err, new_uid};

/// 枚举 ↔ 库里的小写文字（和引擎的 JSON 写法一致）
fn word<T: Serialize>(v: &T) -> Result<String, String> {
    match serde_json::to_value(v).map_err(err)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(format!("不是一个词：{other}")),
    }
}

fn from_word<T: DeserializeOwned>(s: &str, what: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| format!("调查记录里的{what}「{s}」读不懂（可能是新版本写的）"))
}

fn json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(err)
}

fn from_json<T: DeserializeOwned>(s: &str, what: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(|e| format!("调查记录里的{what}读不懂：{e}"))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub uid: String,
    pub seq: u32,
    pub stage: String,
    pub status: String,
    pub mock: bool,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub sources: usize,
    pub verified_facts: usize,
    pub conclusions: usize,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionSummary {
    pub id: i64,
    pub uid: String,
    pub text: String,
    pub created_at: String,
    pub updated_at: String,
    /// 各次调查，新的在前
    pub runs: Vec<RunSummary>,
}

pub fn question_create(c: &Connection, text: &str, now: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("问题不能空着。".into());
    }
    c.execute("INSERT INTO research_questions (uid, text, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)", params![new_uid()?, text, now]).map_err(err)?;
    Ok(c.last_insert_rowid())
}

/// 删掉一个问题和它的全部调查。快照文件由调用方按 `snapshot_shas` 清理。
pub fn question_delete(c: &Connection, id: i64) -> Result<(), String> {
    let n = c.execute("DELETE FROM research_questions WHERE id = ?1", [id]).map_err(err)?;
    if n == 0 {
        return Err("这个问题不存在了。".into());
    }
    Ok(())
}

pub fn run_delete(c: &Connection, uid: &str) -> Result<(), String> {
    let n = c.execute("DELETE FROM research_runs WHERE uid = ?1", [uid]).map_err(err)?;
    if n == 0 {
        return Err("这次调查不存在了。".into());
    }
    Ok(())
}

fn summaries(c: &Connection, question_id: i64) -> Result<Vec<RunSummary>, String> {
    let mut st = c
        .prepare(
            "SELECT r.uid, r.seq, r.stage, r.status, r.mock, r.created_at, r.finished_at,
                    (SELECT COUNT(*) FROM research_sources s WHERE s.run_id = r.id),
                    (SELECT COUNT(*) FROM research_facts f WHERE f.run_id = r.id AND f.status = 'verified'),
                    (SELECT COUNT(*) FROM research_conclusions k WHERE k.run_id = r.id),
                    (SELECT COALESCE(SUM(p.cost_usd), 0.0) FROM research_steps p WHERE p.run_id = r.id AND p.mock = 0)
             FROM research_runs r WHERE r.question_id = ?1 ORDER BY r.seq DESC",
        )
        .map_err(err)?;
    st.query_map([question_id], |r| {
        Ok(RunSummary {
            uid: r.get(0)?,
            seq: r.get::<_, i64>(1)? as u32,
            stage: r.get(2)?,
            status: r.get(3)?,
            mock: r.get::<_, i64>(4)? != 0,
            created_at: r.get(5)?,
            finished_at: r.get(6)?,
            sources: r.get::<_, i64>(7)? as usize,
            verified_facts: r.get::<_, i64>(8)? as usize,
            conclusions: r.get::<_, i64>(9)? as usize,
            cost_usd: r.get(10)?,
        })
    })
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)
}

/// 全部问题，最近动过的在前
pub fn questions_list(c: &Connection) -> Result<Vec<QuestionSummary>, String> {
    let mut st = c.prepare("SELECT id, uid, text, created_at, updated_at FROM research_questions ORDER BY updated_at DESC, id DESC").map_err(err)?;
    let rows: Vec<(i64, String, String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    rows.into_iter().map(|(id, uid, text, created_at, updated_at)| Ok(QuestionSummary { runs: summaries(c, id)?, id, uid, text, created_at, updated_at })).collect()
}

pub fn question_get(c: &Connection, id: i64) -> Result<QuestionSummary, String> {
    let row = c.query_row("SELECT uid, text, created_at, updated_at FROM research_questions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(err)?;
    let (uid, text, created_at, updated_at) = row.ok_or_else(|| "这个问题不存在了。".to_string())?;
    Ok(QuestionSummary { runs: summaries(c, id)?, id, uid, text, created_at, updated_at })
}

/// 这个问题下一次调查的序号
pub fn next_seq(c: &Connection, question_id: i64) -> Result<u32, String> {
    let max: Option<i64> = c.query_row("SELECT MAX(seq) FROM research_runs WHERE question_id = ?1", [question_id], |r| r.get(0)).map_err(err)?;
    Ok(max.unwrap_or(0) as u32 + 1)
}

/// 一次调查属于哪个问题
pub fn question_of(c: &Connection, run_uid: &str) -> Result<i64, String> {
    c.query_row("SELECT question_id FROM research_runs WHERE uid = ?1", [run_uid], |r| r.get(0)).optional().map_err(err)?.ok_or_else(|| "这次调查不存在了。".to_string())
}

/// 存一次调查（新建或整份更新）。
pub fn run_save(c: &Connection, question_id: i64, run: &Run) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    tx.execute(
        "INSERT INTO research_runs (uid, question_id, seq, previous_uid, stage, status, error, mock, config, plan, gaps, created_at, updated_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (uid) DO UPDATE SET previous_uid = excluded.previous_uid, stage = excluded.stage, status = excluded.status, error = excluded.error, mock = excluded.mock,
           config = excluded.config, plan = excluded.plan, gaps = excluded.gaps, updated_at = excluded.updated_at, finished_at = excluded.finished_at",
        params![run.uid, question_id, run.seq, run.previous, word(&run.stage)?, word(&run.status)?, run.error, run.mock, json(&run.config)?, json(&run.plan)?, json(&run.gaps)?, run.created_at, run.updated_at, run.finished_at],
    )
    .map_err(err)?;
    let id: i64 = tx.query_row("SELECT id FROM research_runs WHERE uid = ?1", [&run.uid], |r| r.get(0)).map_err(err)?;
    // 加入文献库时直接改的是库里的行：内存里这份没有的话沿用库里的，不让一次保存把它抹掉
    let linked: HashMap<String, i64> = {
        let mut st = tx.prepare("SELECT key, item_id FROM research_sources WHERE run_id = ?1 AND item_id IS NOT NULL").map_err(err)?;
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    for table in ["research_conclusions", "research_calcs", "research_facts", "research_sources", "research_searches", "research_steps"] {
        tx.execute(&format!("DELETE FROM {table} WHERE run_id = ?1"), [id]).map_err(err)?;
    }
    for (i, s) in run.searches.iter().enumerate() {
        tx.execute(
            "INSERT INTO research_searches (run_id, position, sub_question, query, engine, hits, error, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, i as i64, s.sub_question, s.query, s.engine, json(&s.hits)?, s.error, s.at],
        )
        .map_err(err)?;
    }
    for (i, s) in run.sources.iter().enumerate() {
        let tier = s.tier.as_ref().map(word).transpose()?;
        tx.execute(
            "INSERT INTO research_sources (run_id, key, position, url, origin, sub_questions, title, final_url, fetched_at, sha256, kind, content_type, ext, bytes, text_chars,
                                           fetch_error, text_error, extracted, read_chars, truncated, extract_error, publisher, published, tier, item_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            params![
                id,
                s.id,
                i as i64,
                s.url,
                word(&s.origin)?,
                json(&s.sub_questions)?,
                s.title,
                s.final_url,
                s.fetched_at,
                s.sha256,
                s.kind,
                s.content_type,
                s.ext,
                s.bytes as i64,
                s.text_chars as i64,
                s.fetch_error,
                s.text_error,
                s.extracted,
                s.read_chars as i64,
                s.truncated,
                s.extract_error,
                s.publisher,
                s.published,
                tier,
                s.item_id.or_else(|| linked.get(&s.id).copied()),
            ],
        )
        .map_err(|e| format!("存不了来源 {}：{e}", s.id))?;
    }
    let mut fact_rows: HashMap<&str, i64> = HashMap::new();
    for (i, f) in run.facts.iter().enumerate() {
        let parsed = f.check.parsed.as_ref().map(json).transpose()?;
        tx.execute(
            "INSERT INTO research_facts (run_id, key, position, source_key, sub_question, metric, value, unit, region, population, period, definition, producer, producer_kind, relayed,
                                         quote, unit_quote, status, reason, span_start, span_end, occurrences, parsed, tier)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            params![
                id,
                f.id,
                i as i64,
                f.source,
                f.sub_question,
                f.metric,
                f.value,
                f.unit,
                f.region,
                f.population,
                f.period,
                f.definition,
                f.producer,
                word(&f.producer_kind)?,
                f.relayed,
                f.quote,
                f.unit_quote,
                word(&f.check.status)?,
                f.check.reason,
                f.check.span.map(|s| s.start as i64),
                f.check.span.map(|s| s.end as i64),
                f.check.occurrences as i64,
                parsed,
                word(&f.tier)?,
            ],
        )
        .map_err(|e| format!("存不了事实 {}：{e}", f.id))?;
        fact_rows.insert(f.id.as_str(), tx.last_insert_rowid());
    }
    for (i, k) in run.calcs.iter().enumerate() {
        tx.execute(
            "INSERT INTO research_calcs (run_id, key, position, op, inputs, label, decimals, result, shown, unit, steps, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![id, k.id, i as i64, word(&k.op)?, json(&k.inputs)?, k.label, k.decimals, k.result.map(|d| d.to_string()), k.shown, k.unit, k.steps, k.error],
        )
        .map_err(|e| format!("存不了计算 {}：{e}", k.id))?;
    }
    for (i, k) in run.conclusions.iter().enumerate() {
        tx.execute(
            "INSERT INTO research_conclusions (run_id, key, position, template, segments, calcs, uncertainty, note, issues) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, k.id, i as i64, k.template, json(&k.segments)?, json(&k.calcs)?, json(&k.uncertainty)?, k.note, json(&k.issues)?],
        )
        .map_err(|e| format!("存不了结论 {}：{e}", k.id))?;
        let conclusion = tx.last_insert_rowid();
        for (n, fact) in k.facts.iter().enumerate() {
            let row = fact_rows.get(fact.as_str()).ok_or_else(|| format!("结论 {} 引用的 {fact} 不在事实表里", k.id))?;
            tx.execute("INSERT INTO research_conclusion_facts (conclusion_id, fact_id, position) VALUES (?1, ?2, ?3)", params![conclusion, row, n as i64]).map_err(err)?;
        }
    }
    for (i, s) in run.steps.iter().enumerate() {
        tx.execute(
            "INSERT INTO research_steps (run_id, position, kind, label, at, provider, model, input_tokens, output_tokens, searches, cost_usd, mock, ok, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![id, i as i64, word(&s.kind)?, s.label, s.at, s.provider, s.model, s.input_tokens as i64, s.output_tokens as i64, s.searches as i64, s.cost_usd, s.mock, s.ok, s.note],
        )
        .map_err(err)?;
    }
    tx.execute("UPDATE research_questions SET updated_at = ?1 WHERE id = ?2", params![run.updated_at, question_id]).map_err(err)?;
    tx.commit()
}

fn rows<T>(c: &Connection, sql: &str, run_id: i64, f: impl Fn(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    st.query_map([run_id], |r| f(r)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 读回一次调查（来源的 `text` 不在库里）。
pub fn run_load(c: &Connection, uid: &str) -> Result<Run, String> {
    type Head = (i64, i64, String, i64, Option<String>, String, String, Option<String>, bool, String, String, String, String, String, Option<String>);
    let head: Option<Head> = c
        .query_row(
            "SELECT r.id, r.question_id, q.text, r.seq, r.previous_uid, r.stage, r.status, r.error, r.mock, r.config, r.plan, r.gaps, r.created_at, r.updated_at, r.finished_at
             FROM research_runs r JOIN research_questions q ON q.id = r.question_id WHERE r.uid = ?1",
            [uid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?)),
        )
        .optional()
        .map_err(err)?;
    let (id, _question_id, question, seq, previous, stage, status, error, mock, config, plan, gaps, created_at, updated_at, finished_at) = head.ok_or_else(|| "这次调查不存在了。".to_string())?;

    let searches = rows(c, "SELECT sub_question, query, engine, hits, error, at FROM research_searches WHERE run_id = ?1 ORDER BY position", id, |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?))
    })?
    .into_iter()
    .map(|(sub_question, query, engine, hits, error, at)| Ok(SearchRecord { sub_question, query, engine, hits: from_json(&hits, "搜索结果")?, error, at }))
    .collect::<Result<Vec<_>, String>>()?;

    type SourceRow = (Source, String, String, Option<String>);
    let sources = rows(
        c,
        "SELECT key, url, origin, sub_questions, title, final_url, fetched_at, sha256, kind, content_type, ext, bytes, text_chars, fetch_error, text_error, extracted, read_chars, truncated,
                extract_error, publisher, published, tier, item_id
         FROM research_sources WHERE run_id = ?1 ORDER BY position",
        id,
        |r| -> rusqlite::Result<SourceRow> {
            Ok((
                Source {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(4)?,
                    final_url: r.get(5)?,
                    fetched_at: r.get(6)?,
                    sha256: r.get(7)?,
                    kind: r.get(8)?,
                    content_type: r.get(9)?,
                    ext: r.get(10)?,
                    bytes: r.get::<_, i64>(11)? as u64,
                    text_chars: r.get::<_, i64>(12)? as usize,
                    fetch_error: r.get(13)?,
                    text_error: r.get(14)?,
                    extracted: r.get(15)?,
                    read_chars: r.get::<_, i64>(16)? as usize,
                    truncated: r.get(17)?,
                    extract_error: r.get(18)?,
                    publisher: r.get(19)?,
                    published: r.get(20)?,
                    item_id: r.get(22)?,
                    ..Default::default()
                },
                r.get(2)?,
                r.get(3)?,
                r.get(21)?,
            ))
        },
    )?
    .into_iter()
    .map(|(mut s, origin, sub_questions, tier)| {
        s.origin = from_word(&origin, "来源类型")?;
        s.sub_questions = from_json(&sub_questions, "子问题编号")?;
        s.tier = tier.map(|t| from_word(&t, "可信度")).transpose()?;
        Ok(s)
    })
    .collect::<Result<Vec<_>, String>>()?;

    type FactRow = (Fact, String, String, Option<String>, String);
    let facts = rows(
        c,
        "SELECT key, source_key, sub_question, metric, value, unit, region, population, period, definition, producer, producer_kind, relayed, quote, unit_quote,
                status, reason, span_start, span_end, occurrences, parsed, tier
         FROM research_facts WHERE run_id = ?1 ORDER BY position",
        id,
        |r| -> rusqlite::Result<FactRow> {
            let span = match (r.get::<_, Option<i64>>(17)?, r.get::<_, Option<i64>>(18)?) {
                (Some(a), Some(b)) => Some(Span { start: a as usize, end: b as usize }),
                _ => None,
            };
            Ok((
                Fact {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    sub_question: r.get(2)?,
                    metric: r.get(3)?,
                    value: r.get(4)?,
                    unit: r.get(5)?,
                    region: r.get(6)?,
                    population: r.get(7)?,
                    period: r.get(8)?,
                    definition: r.get(9)?,
                    producer: r.get(10)?,
                    producer_kind: research_engine::trust::ProducerKind::Unknown,
                    relayed: r.get(12)?,
                    quote: r.get(13)?,
                    unit_quote: r.get(14)?,
                    check: Check { reason: r.get(16)?, span, occurrences: r.get::<_, i64>(19)? as usize, ..Default::default() },
                    tier: research_engine::trust::Tier::Low,
                },
                r.get(11)?,
                r.get(15)?,
                r.get(20)?,
                r.get(21)?,
            ))
        },
    )?
    .into_iter()
    .map(|(mut f, kind, status, parsed, tier)| {
        f.producer_kind = from_word(&kind, "数据生产者类型")?;
        f.check.status = from_word(&status, "核对状态")?;
        f.check.parsed = parsed.map(|p| from_json(&p, "数值")).transpose()?;
        f.tier = from_word(&tier, "可信度")?;
        Ok(f)
    })
    .collect::<Result<Vec<_>, String>>()?;

    type CalcRow = (String, String, String, String, i64, Option<String>, String, String, String, Option<String>);
    let calcs = rows(c, "SELECT key, op, inputs, label, decimals, result, shown, unit, steps, error FROM research_calcs WHERE run_id = ?1 ORDER BY position", id, |r| -> rusqlite::Result<CalcRow> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))
    })?
    .into_iter()
    .map(|(key, op, inputs, label, decimals, result, shown, unit, steps, error)| {
        let result = result.map(|d| Dec::parse(&d).ok_or_else(|| format!("调查记录里 {key} 的结果「{d}」读不懂"))).transpose()?;
        Ok(Calc { id: key, op: from_word(&op, "计算类型")?, inputs: from_json(&inputs, "计算的输入")?, label, decimals: decimals as u32, result, shown, unit, steps, error })
    })
    .collect::<Result<Vec<_>, String>>()?;

    type ConclusionRow = (i64, String, String, String, String, String, String, String);
    let conclusions = rows(c, "SELECT id, key, template, segments, calcs, uncertainty, note, issues FROM research_conclusions WHERE run_id = ?1 ORDER BY position", id, |r| -> rusqlite::Result<ConclusionRow> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
    })?
    .into_iter()
    .map(|(row, key, template, segments, calcs, uncertainty, note, issues)| {
        let facts = rows(c, "SELECT f.key FROM research_conclusion_facts x JOIN research_facts f ON f.id = x.fact_id WHERE x.conclusion_id = ?1 ORDER BY x.position", row, |r| r.get::<_, String>(0))?;
        // 「单位写两遍」修正之前存下的结论，读出来时同样去掉重复的单位（`decisions/0022`）
        let mut segments: Vec<Segment> = from_json(&segments, "结论")?;
        research_engine::conclude::drop_repeated_units(&mut segments);
        Ok(Conclusion { id: key, template, segments, facts, calcs: from_json(&calcs, "结论用到的计算")?, uncertainty: from_json(&uncertainty, "不确定类型")?, note, issues: from_json(&issues, "结论的问题")? })
    })
    .collect::<Result<Vec<_>, String>>()?;

    type StepRow = (String, Step);
    let steps = rows(c, "SELECT kind, label, at, provider, model, input_tokens, output_tokens, searches, cost_usd, mock, ok, note FROM research_steps WHERE run_id = ?1 ORDER BY position", id, |r| -> rusqlite::Result<StepRow> {
        Ok((
            r.get(0)?,
            Step {
                kind: research_engine::model::StepKind::Report,
                label: r.get(1)?,
                at: r.get(2)?,
                provider: r.get(3)?,
                model: r.get(4)?,
                input_tokens: r.get::<_, i64>(5)? as u64,
                output_tokens: r.get::<_, i64>(6)? as u64,
                searches: r.get::<_, i64>(7)? as u64,
                cost_usd: r.get(8)?,
                mock: r.get(9)?,
                ok: r.get(10)?,
                note: r.get(11)?,
            },
        ))
    })?
    .into_iter()
    .map(|(kind, mut s)| {
        s.kind = from_word(&kind, "步骤")?;
        Ok(s)
    })
    .collect::<Result<Vec<_>, String>>()?;

    Ok(Run {
        uid: uid.to_string(),
        question,
        seq: seq as u32,
        previous,
        created_at,
        updated_at,
        finished_at,
        stage: from_word(&stage, "步骤")?,
        status: from_word(&status, "状态")?,
        error,
        mock,
        config: from_json(&config, "配置")?,
        plan: from_json(&plan, "计划")?,
        searches,
        sources,
        facts,
        calcs,
        conclusions,
        gaps: from_json(&gaps, "没查到的")?,
        steps,
    })
}

/// 把一个来源和文献库里的条目连起来（加入文献库之后）
pub fn source_set_item(c: &Connection, run_uid: &str, key: &str, item_id: i64) -> Result<(), String> {
    let n = c
        .execute("UPDATE research_sources SET item_id = ?1 WHERE key = ?2 AND run_id = (SELECT id FROM research_runs WHERE uid = ?3)", params![item_id, key, run_uid])
        .map_err(err)?;
    if n == 0 {
        return Err(format!("这次调查里没有来源 {key}"));
    }
    Ok(())
}

/// 库里还用着的快照（清理没人用的快照文件前查）
pub fn snapshot_shas(c: &Connection) -> Result<HashSet<String>, String> {
    let mut st = c.prepare("SELECT DISTINCT sha256 FROM research_sources WHERE sha256 IS NOT NULL").map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 程序异常退出时停在「进行中」的调查：下次启动标成「已停止」，可以接着做
pub fn mark_interrupted(c: &Connection) -> Result<usize, String> {
    c.execute("UPDATE research_runs SET status = 'cancelled', error = NULL WHERE status = 'running'", []).map_err(err)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::{self, tests::memory_db};
    use research_engine::fixture::{MockFetch, MockModel, MockSearch, QUESTION};
    use research_engine::flow;
    use research_engine::model::{Config, ModelRef, Status};
    use research_engine::ports::{Ports, Progress, Recorder};
    use std::sync::atomic::AtomicBool;

    pub(crate) struct DbRecorder<'a> {
        pub c: &'a Connection,
        pub question_id: i64,
        pub saves: usize,
    }

    impl Recorder for DbRecorder<'_> {
        fn save(&mut self, run: &Run) -> Result<(), String> {
            self.saves += 1;
            run_save(self.c, self.question_id, run)
        }
    }

    pub(crate) fn config() -> Config {
        Config { planner: ModelRef { provider: "mock".into(), model: "mock-planner".into() }, worker: ModelRef { provider: "mock".into(), model: "mock-worker".into() }, search_engine: "mock".into(), ..Default::default() }
    }

    /// 在合成夹具上查一次，每一步都存进库。`previous`：重查。
    pub(crate) fn investigate(c: &Connection, question_id: i64, uid: &str, round: u32, previous: Option<&Run>) -> Run {
        let (planner, worker, search, fetch) = (MockModel::new("mock-planner", round), MockModel::new("mock-worker", round), MockSearch::new(round), MockFetch::new(round));
        let cancel = AtomicBool::new(false);
        let mut recorder = DbRecorder { c, question_id, saves: 0 };
        let now = || "2026-11-12T09:00:00.000Z".to_string();
        let quiet = |_: Progress| {};
        let mut ports = Ports { planner: &planner, worker: &worker, search: &search, fetch: &fetch, recorder: &mut recorder, now: &now, progress: &quiet, cancel: &cancel };
        let mut run = match previous {
            Some(p) => flow::rerun_from(p, uid, config(), now()),
            None => flow::plan(QUESTION, uid, 1, None, config(), 0, &mut ports).unwrap(),
        };
        flow::execute(&mut run, &mut ports).unwrap();
        assert_eq!(run.status, Status::Done);
        assert!(recorder.saves >= 7);
        run
    }

    /// 读回来的没有正文：比较前把内存里那份的正文也去掉
    fn without_text(mut run: Run) -> Run {
        for s in &mut run.sources {
            s.text = None;
        }
        run
    }

    #[test]
    fn fresh_database_stores_and_reloads_a_whole_investigation() {
        let c = memory_db();
        let q = question_create(&c, &format!("  {QUESTION}  "), "2026-11-12T08:59:00.000Z").unwrap();
        assert_eq!(next_seq(&c, q).unwrap(), 1);
        let run = investigate(&c, q, "01RUNAAAAAAAAAAAAAAAAAAAAA", 1, None);
        assert_eq!(run_load(&c, &run.uid).unwrap(), without_text(run.clone()));
        assert_eq!(question_of(&c, &run.uid).unwrap(), q);
        // 结论与事实的对应进了关联表，顺序保留
        let links: i64 = c.query_row("SELECT COUNT(*) FROM research_conclusion_facts", [], |r| r.get(0)).unwrap();
        assert_eq!(links as usize, run.conclusions.iter().map(|k| k.facts.len()).sum::<usize>());
        // 摘要
        let list = questions_list(&c).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].text.as_str(), list[0].runs.len()), (QUESTION, 1));
        let s = &list[0].runs[0];
        assert_eq!((s.seq, s.status.as_str(), s.stage.as_str(), s.mock, s.sources, s.verified_facts, s.conclusions, s.cost_usd), (1, "done", "report", true, 9, 13, 5, 0.0));
        assert_eq!(snapshot_shas(&c).unwrap().len(), 8);

        // 重查：同一个问题的第 2 次
        assert_eq!(next_seq(&c, q).unwrap(), 2);
        let second = investigate(&c, q, "01RUNBBBBBBBBBBBBBBBBBBBBB", 2, Some(&run));
        assert_eq!(run_load(&c, &second.uid).unwrap(), without_text(second.clone()));
        assert_eq!(question_get(&c, q).unwrap().runs.iter().map(|r| r.seq).collect::<Vec<_>>(), [2, 1]);
        let diff = research_engine::diff::compare(&run_load(&c, &run.uid).unwrap(), &run_load(&c, &second.uid).unwrap());
        assert_eq!((diff.changed.len(), diff.added.len(), diff.removed.len()), (1, 1, 1));

        // 重复保存不出错、不重复；再存一次改过的
        let mut again = second.clone();
        again.facts.truncate(3);
        again.conclusions.clear();
        again.calcs.clear();
        run_save(&c, q, &again).unwrap();
        assert_eq!(run_load(&c, &again.uid).unwrap(), without_text(again));
        assert!(run_load(&c, "没有这个").unwrap_err().contains("不存在"));

        // 删一次调查、删整个问题：子表跟着清
        run_delete(&c, &second.uid).unwrap();
        assert_eq!(question_get(&c, q).unwrap().runs.len(), 1);
        question_delete(&c, q).unwrap();
        for t in ["research_runs", "research_searches", "research_sources", "research_facts", "research_calcs", "research_conclusions", "research_conclusion_facts", "research_steps"] {
            let n: i64 = c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "{t}");
        }
        assert!(question_delete(&c, q).is_err());
        assert!(question_create(&c, "  ", "t").is_err());
    }

    #[test]
    fn library_links_survive_saves_and_item_deletion() {
        let c = memory_db();
        let q = question_create(&c, QUESTION, "t").unwrap();
        let run = investigate(&c, q, "01RUNCCCCCCCCCCCCCCCCCCCCC", 1, None);
        let item = crate::edit::item_create(&c, "webpage", None).unwrap();
        source_set_item(&c, &run.uid, "S1", item).unwrap();
        assert!(source_set_item(&c, &run.uid, "S99", item).is_err());
        assert_eq!(run_load(&c, &run.uid).unwrap().source("S1").unwrap().item_id, Some(item));
        // 内存里那份不知道这个链接：再存一次不会把它抹掉
        run_save(&c, q, &run).unwrap();
        assert_eq!(run_load(&c, &run.uid).unwrap().source("S1").unwrap().item_id, Some(item));
        // 文献被永久删除：链接清空，调查还在
        c.execute("DELETE FROM items WHERE id = ?1", [item]).unwrap();
        assert_eq!(run_load(&c, &run.uid).unwrap().source("S1").unwrap().item_id, None);
    }

    #[test]
    fn interrupted_runs_can_be_resumed_after_a_restart() {
        let c = memory_db();
        let q = question_create(&c, QUESTION, "t").unwrap();
        let mut run = investigate(&c, q, "01RUNDDDDDDDDDDDDDDDDDDDDD", 1, None);
        run.status = Status::Running;
        run_save(&c, q, &run).unwrap();
        assert_eq!(mark_interrupted(&c).unwrap(), 1);
        assert_eq!(run_load(&c, &run.uid).unwrap().status, Status::Cancelled);
        assert_eq!(mark_interrupted(&c).unwrap(), 0);
    }

    /// 升级：第 5 版的库（已有文献）升到最新，文献不动，调查的表可用
    #[test]
    fn version_5_library_upgrades_and_can_store_investigations() {
        let dir = crate::db::tests::temp_dir("research-upgrade");
        let backups = dir.join("backups");
        let mut c = db::open(&dir.join("library.db")).unwrap();
        db::migrate(&mut c, &db::MIGRATIONS[..5], &backups).unwrap();
        assert_eq!(db::user_version(&c).unwrap(), 5);
        c.execute("INSERT INTO items (uid, item_type, title, source, created_at, updated_at) VALUES ('U1', 'report', '升级前的文献', 'manual', 't', 't')", []).unwrap();
        let none: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'research_%'", [], |r| r.get(0)).unwrap();
        assert_eq!(none, 0);
        assert_eq!(db::migrate(&mut c, db::MIGRATIONS, &backups).unwrap(), db::latest());
        assert!(db::latest() >= 6);
        // 升级前自动备份了
        let files: Vec<String> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(files.len() == 1 && files[0].ends_with("-before-v6.db"), "{files:?}");
        let title: String = c.query_row("SELECT title FROM items WHERE uid = 'U1'", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "升级前的文献");
        crate::search::attach_memory(&c).unwrap();
        let q = question_create(&c, QUESTION, "t").unwrap();
        let run = investigate(&c, q, "01RUNEEEEEEEEEEEEEEEEEEEEE", 1, None);
        // 来源可以连到升级前就有的文献
        source_set_item(&c, &run.uid, "S1", 1).unwrap();
        assert_eq!(run_load(&c, &run.uid).unwrap().source("S1").unwrap().item_id, Some(1));
        let fk: i64 = c.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 0);
    }
}
