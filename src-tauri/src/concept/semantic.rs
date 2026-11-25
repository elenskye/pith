//! 语义检索（v0.5，修 `decisions/0025` 的已知问题「问这本书只能按词找」）：用本机的向量模型和重排模型（`crate::local`）按意思找段落。
//!
//! - **向量**是派生数据，放 `search.db`（删了能重建，不写 migration）：`search.concept_vecs(para_id, model, sig, vec)`。
//!   `para_id` = `concept_paragraphs.id`；`model` 是向量模型的 key（换了模型，旧向量不拿来比）；`sig` 是段落文字的散列，
//!   段落被替换（重新解析）后 id 可能被新段落占用，`sig` 对不上的当作没有向量，重新算。
//!   只收正文、旁注、注释、导读（和按词检索一样）。原著和连着的译本都建。
//! - **建索引**在后台线程里做（`concept_index_start`，事件 `concept-index`），一批 16 段；可以随时再问，已经建好的部分先用。
//! - **提问**（`concept_ask_search`）：问题的向量 → 原著里最像的 40 段 + 译本里最像的 24 段（按对齐映射回原著）+ 按词检索的命中
//!   → 重排模型打分 → 按相关度取前几条。中文问题、原著有对齐的中译时，重排看的是中译（同一种语言比得更准）。
//! - **退路**：本机模型用不了、或索引一段都还没有 → 按词检索的结果（`commands::search_hits`），并说明原因；重排用不了 → 按向量的相似度排。
//!   向量和重排只决定「给用户看哪些段落」，原文引用照旧由 `verify` 逐字核对。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use concept_engine::align::Alignment;
use concept_engine::text::{Role, Text};
use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::{commands, store};
use crate::AppState;
use crate::common::{Tx, err};
use crate::local::{LocalModels, Mode, Svc, embed, rerank};

const CREATE: &str = "CREATE TABLE IF NOT EXISTS search.concept_vecs (para_id INTEGER PRIMARY KEY, model TEXT NOT NULL, sig INTEGER NOT NULL, vec BLOB NOT NULL)";
const ROLES: &str = "('body', 'sidenote', 'note', 'guide')";
const INDEX_EVENT: &str = "concept-index";
const QUERY_TASK: &str = "Given a question about a book, retrieve passages from the book that help answer it";
/// 原著、译本各取多少个向量最像的；按词检索取多少；送去重排的上限
const TOP_ORIGINAL: usize = 40;
const TOP_TRANSLATION: usize = 24;
const TOP_KEYWORD: usize = 24;
const POOL: usize = 64;

fn sig(body: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.as_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h as i64
}

/// 表不在就建；主库里已经没有的段落，向量一起删
pub fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch(CREATE).map_err(err)?;
    c.execute("DELETE FROM search.concept_vecs WHERE para_id NOT IN (SELECT id FROM concept_paragraphs)", []).map_err(err)?;
    Ok(())
}

/// 一份文本里还没有（或已经过期的）向量的段落：(id, 文字)，以及应该有向量的段落总数
fn missing(c: &Connection, text_id: i64, model: &str) -> Result<(Vec<(i64, String)>, usize), String> {
    c.execute_batch(CREATE).map_err(err)?;
    let sql = format!("SELECT p.id, p.body, v.sig FROM concept_paragraphs p LEFT JOIN search.concept_vecs v ON v.para_id = p.id AND v.model = ?2 WHERE p.text_id = ?1 AND p.role IN {ROLES} ORDER BY p.seq");
    let mut st = c.prepare(&sql).map_err(err)?;
    let rows: Vec<(i64, String, Option<i64>)> = st.query_map(params![text_id, model], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let total = rows.len();
    Ok((rows.into_iter().filter(|(_, body, s)| *s != Some(sig(body))).map(|(id, body, _)| (id, body)).collect(), total))
}

fn store_vectors(c: &Connection, model: &str, rows: &[(i64, String)], vecs: &[Vec<f32>]) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    {
        let mut ins = tx.prepare("INSERT OR REPLACE INTO search.concept_vecs (para_id, model, sig, vec) VALUES (?1, ?2, ?3, ?4)").map_err(err)?;
        for ((id, body), v) in rows.iter().zip(vecs) {
            ins.execute(params![id, model, sig(body), embed::to_blob(v)]).map_err(err)?;
        }
    }
    tx.commit()
}

/// 给一份文本补齐向量（同步；后台线程和测试都走它）。`progress(已有, 总数)`。返回这次新算了多少段
pub fn index_text(state: &AppState, local: &LocalModels, text_id: i64, cancel: &AtomicBool, progress: &dyn Fn(usize, usize)) -> Result<usize, String> {
    let model = local.ensure(Svc::Embedding)?;
    let (todo, total) = state.with_db(|c| missing(c, text_id, &model.key))?;
    let mut done = total - todo.len();
    progress(done, total);
    let mut made = 0;
    for batch in todo.chunks(embed::BATCH) {
        if cancel.load(Ordering::SeqCst) {
            return Err("已停止。".into());
        }
        let texts: Vec<String> = batch.iter().map(|(_, b)| b.clone()).collect();
        let vecs = embed::embed(local, &model, &texts, None)?;
        state.with_db(|c| store_vectors(c, &model.key, batch, &vecs))?;
        made += batch.len();
        done += batch.len();
        progress(done, total);
    }
    Ok(made)
}

// ---------- 后台建索引 ----------

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub text_uid: String,
    /// 应该有向量的段落数、已经有的
    pub total: usize,
    pub done: usize,
    pub running: bool,
    pub error: Option<String>,
    /// 向量模型的名字；本机模型用不了时是空，`issue` 说原因
    pub model: Option<String>,
    pub issue: Option<String>,
}

#[derive(Default)]
pub struct SemanticState {
    /// 正在建索引的文本（uid）和最近一次失败的原因
    running: Mutex<HashMap<String, ()>>,
    errors: Mutex<HashMap<String, String>>,
}

impl SemanticState {
    fn is_running(&self, uid: &str) -> bool {
        self.running.lock().map(|m| m.contains_key(uid)).unwrap_or(false)
    }
}

pub fn index_status(state: &AppState, local: &LocalModels, sem: &SemanticState, uid: &str) -> Result<IndexStatus, String> {
    let mut st = IndexStatus { text_uid: uid.to_string(), running: sem.is_running(uid), error: sem.errors.lock().ok().and_then(|m| m.get(uid).cloned()), ..Default::default() };
    match local.model(Svc::Embedding) {
        Ok(m) => {
            let (todo, total) = state.with_db(|c| missing(c, store::text_id(c, uid)?, &m.key))?;
            st.total = total;
            st.done = total - todo.len();
            st.model = Some(m.name);
        }
        Err(e) => st.issue = Some(e),
    }
    Ok(st)
}

/// 在后台给这份文本（和它连着的译本 / 原著）建索引；已经在建或已经建好就什么都不做
pub fn index_start(app: &AppHandle, uid: &str) {
    let (Some(state), Some(sem)) = (app.try_state::<AppState>(), app.try_state::<SemanticState>()) else { return };
    let local = state.local.clone();
    if local.mode() == Mode::Off {
        return;
    }
    let mut uids = vec![uid.to_string()];
    if let Ok(Some((partner, _))) = state.with_db(|c| store::partner(c, uid)) {
        uids.push(partner);
    }
    for uid in uids {
        let Ok(st) = index_status(&state, &local, &sem, &uid) else { continue };
        if st.running || st.model.is_none() || st.done >= st.total {
            continue;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut m) = sem.running.lock() {
            if m.contains_key(&uid) {
                continue;
            }
            m.insert(uid.clone(), ());
        }
        if let Ok(mut e) = sem.errors.lock() {
            e.remove(&uid);
        }
        let app = app.clone();
        std::thread::spawn(move || {
            let (state, sem) = (app.state::<AppState>(), app.state::<SemanticState>());
            let local = state.local.clone();
            let emit = |done: usize, total: usize, running: bool, error: Option<&str>| {
                let _ = app.emit(INDEX_EVENT, serde_json::json!({ "textUid": uid, "done": done, "total": total, "running": running, "error": error }));
            };
            let last = Mutex::new((0usize, 0usize));
            let result = state.with_db(|c| store::text_id(c, &uid)).and_then(|id| {
                index_text(&state, &local, id, &cancel, &|done, total| {
                    if let Ok(mut l) = last.lock() {
                        *l = (done, total);
                    }
                    emit(done, total, true, None);
                })
            });
            if let Ok(mut m) = sem.running.lock() {
                m.remove(&uid);
            }
            let (done, total) = last.lock().map(|l| *l).unwrap_or((0, 0));
            match result {
                Ok(_) => emit(done, total, false, None),
                Err(e) => {
                    if let Ok(mut m) = sem.errors.lock() {
                        m.insert(uid.clone(), e.clone());
                    }
                    emit(done, total, false, Some(&e));
                }
            }
        });
    }
}

// ---------- 提问 ----------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskHit {
    pub seq: usize,
    pub key: String,
    role: Role,
    unit: usize,
    unit_key: String,
    unit_label: String,
    text: String,
    /// 是在对齐的中译里找到、映射回原著的
    via_translation: bool,
    translation_key: Option<String>,
    translation_text: Option<String>,
    /// 相关度 0…1（重排的分数；重排用不了时是向量的相似度；按词检索时没有）
    pub score: Option<f32>,
    /// 这一段是怎么找到的：semantic（按意思）/ keyword（按词）/ both
    pub via: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResult {
    pub hits: Vec<AskHit>,
    /// semantic：按意思找的；keyword：退回了按词检索
    pub mode: &'static str,
    pub reranked: bool,
    /// 给用户看的一句说明（为什么退回按词、索引建到哪了）
    pub note: Option<String>,
    pub index: IndexStatus,
}

struct Books {
    original: Text,
    original_id: i64,
    translation: Option<(i64, Text, Alignment)>,
}

fn load(c: &Connection, uid: &str) -> Result<Books, String> {
    let original = store::text_load(c, uid)?;
    let original_id = store::text_id(c, uid)?;
    let translation = match store::partner(c, uid)? {
        Some((t, true)) => match store::alignment_load(c, uid, &t)? {
            Some(al) => Some((store::text_id(c, &t)?, store::text_load(c, &t)?, al)),
            None => None,
        },
        _ => None,
    };
    Ok(Books { original, original_id, translation })
}

// 对齐以后会支持一对多：这两处收成列表，换了返回类型也不用改别处
fn originals_of(al: &Alignment, translation_seq: usize) -> Vec<usize> {
    al.original_of(translation_seq).into_iter().collect()
}

fn others_of(al: &Alignment, original_seq: usize) -> Vec<usize> {
    al.other_of(original_seq).into_iter().collect()
}

/// 正文段原样；旁注、注释算到它挂的正文段；导读、标题不算
fn body_of(t: &Text, seq: usize) -> Option<usize> {
    let p = t.paragraphs.get(seq)?;
    if p.role == Role::Body { Some(seq) } else { p.attached }
}

/// 一份文本里和 `q` 最像的段落：(seq, 相似度)，从高到低
fn nearest(c: &Connection, text_id: i64, model: &str, q: &[f32], keep: usize) -> Result<Vec<(usize, f32)>, String> {
    let sql = format!("SELECT p.seq, p.body, v.sig, v.vec FROM search.concept_vecs v JOIN concept_paragraphs p ON p.id = v.para_id WHERE p.text_id = ?1 AND v.model = ?2 AND p.role IN {ROLES}");
    let mut st = c.prepare(&sql).map_err(err)?;
    let mut scored: Vec<(usize, f32)> = Vec::new();
    let mut rows = st.query(params![text_id, model]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let body = r.get_ref(1).map_err(err)?.as_str().map_err(err)?;
        if r.get::<_, i64>(2).map_err(err)? != sig(body) {
            continue;
        }
        let blob = r.get_ref(3).map_err(err)?.as_blob().map_err(err)?;
        scored.push((r.get::<_, i64>(0).map_err(err)? as usize, embed::dot(q, &embed::from_blob(blob))));
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(keep);
    Ok(scored)
}

fn hit(b: &Books, seq: usize, score: Option<f32>, via: &'static str, via_translation: bool) -> Option<AskHit> {
    let p = b.original.paragraphs.get(seq)?;
    let u = &b.original.units[p.unit];
    let zh: Vec<&concept_engine::text::Paragraph> = b.translation.as_ref().map(|(_, t, al)| others_of(al, seq).into_iter().filter_map(|z| t.paragraphs.get(z)).collect()).unwrap_or_default();
    let join = |f: &dyn Fn(&concept_engine::text::Paragraph) -> String, sep: &str| (!zh.is_empty()).then(|| zh.iter().map(|z| f(z)).collect::<Vec<_>>().join(sep));
    Some(AskHit { seq, key: p.key.clone(), role: p.role, unit: p.unit, unit_key: u.key.clone(), unit_label: u.label.clone(), text: p.text.clone(), via_translation, translation_key: join(&|z| z.key.clone(), "、"), translation_text: join(&|z| z.text.clone(), "\n"), score, via })
}

fn keyword_result(state: &AppState, b: &Books, uid: &str, question: &str, limit: usize, note: Option<String>, index: IndexStatus) -> Result<AskResult, String> {
    let kw = state.with_db(|c| commands::search_hits(c, uid, question, limit))?;
    let mut seen = Vec::new();
    let hits = kw
        .iter()
        .filter_map(|h| {
            let body = body_of(&b.original, h.seq).filter(|s| !seen.contains(s))?;
            seen.push(body);
            hit(b, body, None, "keyword", h.via_translation)
        })
        .collect();
    Ok(AskResult { hits, mode: "keyword", reranked: false, note, index })
}

/// 按意思找段落（不依赖 Tauri，测试直接调）。`limit`：最多给几段
pub fn ask(state: &AppState, local: &LocalModels, sem: &SemanticState, uid: &str, question: &str, limit: usize) -> Result<AskResult, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("先写下问题。".into());
    }
    let limit = limit.clamp(1, 60);
    let b = state.with_db(|c| {
        ensure(c)?;
        load(c, uid)
    })?;
    let index = index_status(state, local, sem, uid)?;
    if let Some(issue) = index.issue.clone() {
        return keyword_result(state, &b, uid, question, limit, Some(format!("{issue}这次按词检索。")), index);
    }
    if index.done == 0 {
        let why = match (&index.error, index.running) {
            (Some(e), false) => format!("语义索引没建成（{e}）。这次按词检索。"),
            _ => "这本书的语义索引还没建好，这次按词检索；建好以后再找一次，就是按意思找。".to_string(),
        };
        return keyword_result(state, &b, uid, question, limit, Some(why), index);
    }
    let model = match local.ensure(Svc::Embedding) {
        Ok(m) => m,
        Err(e) => return keyword_result(state, &b, uid, question, limit, Some(format!("{e}这次按词检索。")), index),
    };
    let q = match embed::embed(local, &model, &[question.to_string()], Some(QUERY_TASK)) {
        Ok(mut v) => v.remove(0),
        Err(e) => return keyword_result(state, &b, uid, question, limit, Some(format!("{e}这次按词检索。")), index),
    };

    // 候选：向量最像的（原著 + 译本映射回来的）和按词检索的
    let mut pool: Vec<(usize, f32, bool, bool)> = Vec::new(); // (seq, 相似度, 按意思找到的, 经译本)
    // 只给正文段：旁注、注释算到它挂的正文段上，导读（编者写的）不要
    let direct = state.with_db(|c| nearest(c, b.original_id, &model.key, &q, TOP_ORIGINAL * 2))?;
    for (seq, s) in direct {
        let Some(body) = body_of(&b.original, seq) else { continue };
        match pool.iter().position(|p| p.0 == body) {
            Some(i) => pool[i].1 = pool[i].1.max(s),
            None if pool.len() < TOP_ORIGINAL => pool.push((body, s, true, false)),
            None => {}
        }
    }
    if let Some((zid, zh, al)) = &b.translation {
        for (zseq, s) in state.with_db(|c| nearest(c, *zid, &model.key, &q, TOP_TRANSLATION))? {
            let Some(body) = body_of(zh, zseq) else { continue };
            for o in originals_of(al, body) {
                match pool.iter_mut().find(|p| p.0 == o) {
                    Some(p) => p.1 = p.1.max(s),
                    None => pool.push((o, s, true, true)),
                }
            }
        }
    }
    pool.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    pool.truncate(POOL - TOP_KEYWORD / 2);
    let mut both: Vec<usize> = Vec::new();
    for h in state.with_db(|c| commands::search_hits(c, uid, question, TOP_KEYWORD)).unwrap_or_default() {
        let Some(body) = body_of(&b.original, h.seq) else { continue };
        match pool.iter().position(|p| p.0 == body) {
            Some(_) => both.push(body),
            None if pool.len() < POOL => pool.push((body, 0.0, false, h.via_translation)),
            None => {}
        }
    }

    // 重排：中文问题、有对齐的中译时比中译
    let cjk = question.chars().any(concept_engine::text::is_cjk);
    let passages: Vec<String> = pool
        .iter()
        .map(|(seq, ..)| {
            let zh: Vec<String> = match (&b.translation, cjk) {
                (Some((_, t, al)), true) => others_of(al, *seq).into_iter().filter_map(|z| t.paragraphs.get(z).map(|p| p.text.clone())).collect(),
                _ => Vec::new(),
            };
            if zh.is_empty() { b.original.paragraphs.get(*seq).map(|p| p.text.clone()).unwrap_or_default() } else { zh.join("\n") }
        })
        .collect();
    let ranked = local.ensure(Svc::Rerank).and_then(|m| rerank::scores(local, &m, question, &passages));
    let (order, scores, reranked, mut note): (Vec<usize>, Vec<f32>, bool, Option<String>) = match ranked {
        Ok(s) => {
            let mut o = rerank::order(&s, limit);
            if o.is_empty() {
                // 重排觉得都不相关：还是给出最像的几段，让用户自己判断
                o = (0..pool.len().min(limit.min(6))).collect();
            }
            (o, s, true, None)
        }
        Err(e) => ((0..pool.len().min(limit)).collect(), pool.iter().map(|p| p.1.max(0.0)).collect(), false, Some(format!("{e}这次只按向量的相似度排。"))),
    };
    if index.done < index.total {
        let more = format!("语义索引还在建（{} / {} 段），没建到的部分这次找不到。", index.done, index.total);
        note = Some(match note {
            Some(n) => format!("{n}{more}"),
            None => more,
        });
    }
    let hits = order
        .into_iter()
        .filter_map(|i| {
            let (seq, _, semantic, via_translation) = pool[i];
            let via = if !semantic { "keyword" } else if both.contains(&seq) { "both" } else { "semantic" };
            hit(&b, seq, scores.get(i).copied(), via, via_translation)
        })
        .collect();
    Ok(AskResult { hits, mode: "semantic", reranked, note, index })
}

// ---------- 命令 ----------

/// 「问这本书」的检索：按意思找；本机模型用不了时退回按词。会顺手在后台把没建完的索引建完
#[tauri::command(async)]
pub fn concept_ask_search(app: AppHandle, state: State<AppState>, sem: State<SemanticState>, uid: String, question: String, limit: Option<usize>) -> Result<AskResult, String> {
    index_start(&app, &uid);
    ask(&state, &state.local, &sem, &uid, &question, limit.unwrap_or(12))
}

#[tauri::command(async)]
pub fn concept_index_start(app: AppHandle, state: State<AppState>, sem: State<SemanticState>, uid: String) -> Result<IndexStatus, String> {
    index_start(&app, &uid);
    index_status(&state, &state.local, &sem, &uid)
}

/// 本机模型的状态（设置页）
#[tauri::command(async)]
pub fn local_status(state: State<AppState>) -> crate::local::LocalStatus {
    state.local.status()
}

pub fn manage(app: &tauri::App) {
    if let Some(state) = app.try_state::<AppState>() {
        crate::local::watch(&state.local);
    }
    app.manage(SemanticState::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concept::tests::library_with_books;
    use std::path::PathBuf;

    fn mock() -> LocalModels {
        LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"))
    }

    fn index_both(state: &AppState, local: &LocalModels, en: &str, zh: &str) -> (usize, usize) {
        let never = AtomicBool::new(false);
        let ids = state.with_db(|c| Ok((store::text_id(c, en)?, store::text_id(c, zh)?))).unwrap();
        (index_text(state, local, ids.0, &never, &|_, _| {}).unwrap(), index_text(state, local, ids.1, &never, &|_, _| {}).unwrap())
    }

    #[test]
    fn vectors_are_built_once_and_follow_the_text() {
        let (_dir, state, en, zh, _) = library_with_books("semantic-index");
        let (local, sem) = (mock(), SemanticState::default());
        let st = index_status(&state, &local, &sem, &en).unwrap();
        assert!(st.total > 20 && st.done == 0 && !st.running && st.model.is_some() && st.issue.is_none(), "{st:?}");
        let seen = Mutex::new(Vec::new());
        let never = AtomicBool::new(false);
        let id = state.with_db(|c| store::text_id(c, &en)).unwrap();
        let made = index_text(&state, &local, id, &never, &|d, t| seen.lock().unwrap().push((d, t))).unwrap();
        assert_eq!(made, st.total);
        let seen = seen.into_inner().unwrap();
        assert_eq!((seen.first().copied(), seen.last().copied()), (Some((0, st.total)), Some((st.total, st.total))));
        assert_eq!(index_text(&state, &local, id, &never, &|_, _| {}).unwrap(), 0, "建好的不重算");
        // 段落的文字变了（重新解析后 id 被新段落占用）：只重算这一段
        state.with_db(|c| c.execute("UPDATE concept_paragraphs SET body = body || ' changed' WHERE text_id = ?1 AND key = 'I.i.3'", [id]).map_err(err)).unwrap();
        assert_eq!(index_status(&state, &local, &sem, &en).unwrap().done, st.total - 1);
        assert_eq!(index_text(&state, &local, id, &never, &|_, _| {}).unwrap(), 1);
        // 停下：一批都不做
        let stop = AtomicBool::new(true);
        let zid = state.with_db(|c| store::text_id(c, &zh)).unwrap();
        assert!(index_text(&state, &local, zid, &stop, &|_, _| {}).is_err());
        // 文本删了：向量跟着清掉
        state
            .with_db(|c| {
                crate::concept::search::remove_text(c, id)?;
                store::text_delete(c, &en)?;
                ensure(c)?;
                let left: i64 = c.query_row("SELECT count(*) FROM search.concept_vecs", [], |r| r.get(0)).map_err(err)?;
                assert_eq!(left, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn questions_find_paragraphs_by_meaning_and_fall_back_to_words() {
        let (_dir, state, en, zh, _) = library_with_books("semantic-ask");
        let (local, sem) = (mock(), SemanticState::default());
        // 索引还没建：按词检索，并说明原因
        let r = ask(&state, &local, &sem, &en, "pin-maker", 8).unwrap();
        assert_eq!((r.mode, r.reranked), ("keyword", false));
        assert!(r.note.as_deref().unwrap().contains("还没建好") && r.hits.iter().all(|h| h.score.is_none() && h.via == "keyword"));
        assert_eq!(r.hits.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(), ["I.i.3"]);

        let (a, b) = index_both(&state, &local, &en, &zh);
        assert!(a > 0 && b > 0);
        // 英文问题：按相关度排，分数从高到低，最相关的在讲市场范围的那一章
        let r = ask(&state, &local, &sem, &en, "Why is the division of labour limited by the extent of the market?", 6).unwrap();
        assert_eq!((r.mode, r.reranked, r.note.is_none()), ("semantic", true, true), "{:?}", r.note);
        assert!(!r.hits.is_empty() && r.hits.len() <= 6);
        assert!(r.hits.windows(2).all(|w| w[0].score >= w[1].score), "{:?}", r.hits.iter().map(|h| (h.key.clone(), h.score)).collect::<Vec<_>>());
        // 模拟向量只看字面，不要求第一名；讲市场范围的那一段（I.iii.1）要在前三
        assert!(r.hits.iter().take(3).any(|h| h.key == "I.iii.1"), "{:?}", r.hits.iter().map(|h| (h.key.clone(), h.score)).collect::<Vec<_>>());
        assert!(r.hits.iter().all(|h| h.score.is_some()));
        // 中文问题搜英文原著：经对齐的中译找到，带着中译
        let r = ask(&state, &local, &sem, &en, "分工为什么受市场范围的限制", 6).unwrap();
        assert_eq!(r.mode, "semantic");
        assert!(!r.hits.is_empty() && r.hits.iter().all(|h| h.translation_text.is_some() && h.role == Role::Body), "{:?}", r.hits.iter().map(|h| h.key.clone()).collect::<Vec<_>>());
        // 重排用不了：按向量的相似度排，说明原因
        let r = ask(&state, &local, &sem, &en, &format!("market {}", rerank::MOCK_FAILURE), 6).unwrap();
        assert_eq!((r.mode, r.reranked), ("semantic", false));
        assert!(r.note.as_deref().unwrap().contains("相似度") && !r.hits.is_empty());
        // 向量服务用不了：退回按词
        let r = ask(&state, &local, &sem, &en, &format!("pin-maker；{}", embed::MOCK_FAILURE), 6).unwrap();
        assert_eq!(r.mode, "keyword");
        assert!(r.note.as_deref().unwrap().contains("按词检索"));
        assert!(ask(&state, &local, &sem, &en, "  ", 6).is_err());
    }

    /// 真实的本机模型和真实的书（默认忽略；不联网、不花钱，会经 llmctl.sh 启动向量和重排服务）。
    /// `PITH_ACCEPT_DIR` 是一个数据目录的**副本**（里面有解析好的原著和译本）；`PITH_QUESTIONS` 用「|」分开几个问题。
    /// 只打印段落号、分数和用时，不打印书里的文字。
    /// `PITH_LOCAL_MODELS=real PITH_ACCEPT_DIR=<副本> PITH_QUESTIONS='…|…' cargo test -p pith real_semantic -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_semantic() {
        let root = PathBuf::from(std::env::var("PITH_ACCEPT_DIR").expect("要设 PITH_ACCEPT_DIR（数据目录的副本）"));
        let state = AppState::init(crate::paths::DataPaths::new(root));
        assert!(state.startup_error().is_none(), "{:?}", state.startup_error());
        let (local, sem) = (LocalModels::detect(), SemanticState::default());
        assert_eq!(local.mode(), Mode::Real, "先设 PITH_LOCAL_MODELS=real，并确认 ~/Documents/LLM 在");
        let texts = state.with_db(|c| store::texts_list(c)).unwrap();
        let en = texts.iter().find(|t| t.original_uid.is_none() && texts.iter().any(|z| z.original_uid.as_deref() == Some(t.uid.as_str()))).expect("要有一本连着译本的原著");
        let zh = texts.iter().find(|z| z.original_uid.as_deref() == Some(en.uid.as_str())).unwrap();
        let never = AtomicBool::new(false);
        for t in [en, zh] {
            let before = index_status(&state, &local, &sem, &t.uid).unwrap();
            let started = std::time::Instant::now();
            let made = index_text(&state, &local, t.id, &never, &|_, _| {}).unwrap();
            println!("索引 {}（{}）：{} 段里新算 {made} 段，用时 {:.1} 秒", t.language, t.uid, before.total, started.elapsed().as_secs_f32());
        }
        for q in std::env::var("PITH_QUESTIONS").unwrap_or_default().split('|').map(str::trim).filter(|q| !q.is_empty()) {
            let started = std::time::Instant::now();
            let r = ask(&state, &local, &sem, &en.uid, q, 8).unwrap();
            let kw = state.with_db(|c| commands::search_hits(c, &en.uid, q, 8)).unwrap();
            println!("\n问：{q}\n  方式 {}，重排 {}，用时 {:.1} 秒{}", r.mode, r.reranked, started.elapsed().as_secs_f32(), r.note.map(|n| format!("，说明：{n}")).unwrap_or_default());
            println!("  按意思：{}", r.hits.iter().map(|h| format!("{} {:.2}{}", h.key, h.score.unwrap_or(0.0), if h.via == "semantic" { "" } else { "*" })).collect::<Vec<_>>().join("，"));
            println!("  按词：  {}", kw.iter().map(|h| h.key.clone()).collect::<Vec<_>>().join("，"));
        }
    }

    /// 真实的反方来源（默认忽略；不联网、不花钱）：`PITH_ACCEPT_DIR` 的副本里那次「反方与后世评论」存下的网页全文，
    /// 看挑出来的片段比「只取前 1.2 万字」多带了多少提到目标概念的地方。只打印字数和计数。
    /// `PITH_LOCAL_MODELS=real PITH_ACCEPT_DIR=<副本> PITH_RUN=<研究 uid> PITH_TERM='division of labour' cargo test -p pith real_excerpt -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_excerpt() {
        use concept_engine::{excerpt, flow, prompts::SOURCE_CHARS};
        let root = PathBuf::from(std::env::var("PITH_ACCEPT_DIR").expect("要设 PITH_ACCEPT_DIR（数据目录的副本）"));
        let state = AppState::init(crate::paths::DataPaths::new(root.clone()));
        let run = state.with_db(|c| store::run_load(c, &std::env::var("PITH_RUN").expect("要设 PITH_RUN"))).unwrap();
        let term = std::env::var("PITH_TERM").unwrap_or_default().to_lowercase();
        let count = |t: &str| if term.is_empty() { 0 } else { t.to_lowercase().matches(&term).count() };
        let shelf = crate::concept::library::DbShelf { state: &state, snapshots: crate::concept::library::snapshots(&root), exclude: Vec::new() };
        let sources = flow::source_texts(&run, &shelf);
        assert!(!sources.is_empty(), "这次研究没有来源");
        println!("本机模型：{:?}；目标卡片 {} 张", state.local.mode(), run.targets.len());
        let started = std::time::Instant::now();
        let shown = excerpt::for_prompt(&sources, &run, &shelf);
        println!("挑片段用时 {:.1} 秒", started.elapsed().as_secs_f32());
        for (full, cut) in sources.iter().zip(&shown) {
            let head: String = full.text.chars().take(SOURCE_CHARS).collect();
            let pieces = cut.text.matches("（中间略）").count() + 1;
            println!("{}：全文 {} 字（提到 {} 次）；只取开头 {} 字（{} 次）；挑片段 {} 字、{} 段（{} 次）", full.id, full.text.chars().count(), count(&full.text), head.chars().count(), count(&head), cut.text.chars().count(), pieces, count(&cut.text));
            assert!(cut.text.chars().count() <= SOURCE_CHARS + 60);
        }
    }

    #[test]
    fn without_local_models_search_still_works_by_words() {
        let (_dir, state, en, _, _) = library_with_books("semantic-off");
        let (local, sem) = (LocalModels::new(Mode::Off, PathBuf::from("/nonexistent")), SemanticState::default());
        let st = index_status(&state, &local, &sem, &en).unwrap();
        assert!(st.model.is_none() && st.issue.as_deref().unwrap().contains("没有找到本机模型"));
        let r = ask(&state, &local, &sem, &en, "pin-maker", 8).unwrap();
        assert_eq!(r.mode, "keyword");
        assert!(r.note.as_deref().unwrap().contains("没有找到本机模型") && r.hits.iter().any(|h| h.key == "I.i.3"));
        let never = AtomicBool::new(false);
        assert!(index_text(&state, &local, 1, &never, &|_, _| {}).is_err());
    }
}
