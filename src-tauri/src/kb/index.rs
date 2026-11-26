//! 段落块的检索索引（派生数据，在 `search.db`，删了能重建，不写 migration）。
//!
//! - **按词**：`search.kb_fts(body)`，rowid = `passages.id`；分词和查询改写沿用文献库的（中文二元组、西文前缀）。只收能读的块（正文、旁注、注释、导读）。
//!   启动时和主库的块对一遍，对不上就整张重建；切块时随手改。
//! - **按意思**：`search.kb_vecs(passage_id, model, sig, vec)`，本机向量模型（`crate::local`）算的、归一化的 f32；`sig` 是文字的散列，
//!   文字变了就当没有。书的段落以前「问这本书」算过向量的（`search.concept_vecs`，同一个模型、同样的文字）直接拿来，不重算。
//! - **建索引**在后台线程（`start`），一批 16 块，读库、写库各拿一次锁，算向量时不拿锁；种进来、改了正文、检索时发现缺就开始。
//!   建完一批书，有原著和译本都齐了的就用向量重新对齐一遍（`super::align`）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::common::{Tx, err};
use crate::local::{LocalModels, Mode, Svc, embed};

const FTS: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS search.kb_fts USING fts5(body, tokenize = 'unicode61 remove_diacritics 2')";
const VECS: &str = "CREATE TABLE IF NOT EXISTS search.kb_vecs (passage_id INTEGER PRIMARY KEY, model TEXT NOT NULL, sig INTEGER NOT NULL, vec BLOB NOT NULL)";
/// 能读的块（进检索）
pub const SEARCHABLE: &str = "('body', 'sidenote', 'note', 'guide')";

/// 文字的散列（FNV-1a，和「问这本书」的一样，书的向量才能拿过来用）
pub fn sig(text: &str) -> i64 {
    crate::concept::semantic::sig(text)
}

fn ids(c: &Connection, sql: &str) -> Result<Vec<i64>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 表不在就建；按词的索引和主库对不上就重建；主库里没有的块，向量一起删。返回是否重建了
pub fn ensure(c: &Connection) -> Result<bool, String> {
    c.execute_batch(&format!("{FTS}; {VECS};")).map_err(err)?;
    c.execute("DELETE FROM search.kb_vecs WHERE passage_id NOT IN (SELECT id FROM passages)", []).map_err(err)?;
    let want = ids(c, &format!("SELECT id FROM passages WHERE role IN {SEARCHABLE} ORDER BY id"))?;
    let have = ids(c, "SELECT rowid FROM search.kb_fts ORDER BY rowid").unwrap_or_default();
    if have == want {
        return Ok(false);
    }
    rebuild(c)?;
    Ok(true)
}

pub fn rebuild(c: &Connection) -> Result<usize, String> {
    let tx = Tx::begin(c)?;
    tx.execute_batch(&format!("DROP TABLE IF EXISTS search.kb_fts; {FTS};")).map_err(err)?;
    let rows: Vec<(i64, String)> = {
        let mut st = tx.prepare(&format!("SELECT id, text FROM passages WHERE role IN {SEARCHABLE} ORDER BY id")).map_err(err)?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    {
        let mut ins = tx.prepare("INSERT INTO search.kb_fts (rowid, body) VALUES (?1, ?2)").map_err(err)?;
        for (id, text) in &rows {
            ins.execute(params![id, crate::search::index_text(text)]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(rows.len())
}

/// 切块时：改了的块重写按词的索引（不能读的拿掉），删掉的块连向量一起拿掉
pub fn update(c: &Connection, changed: &[(i64, String, String)], gone: &[i64]) -> Result<(), String> {
    c.execute_batch(&format!("{FTS}; {VECS};")).map_err(err)?;
    let mut del = c.prepare("DELETE FROM search.kb_fts WHERE rowid = ?1").map_err(err)?;
    let mut ins = c.prepare("INSERT INTO search.kb_fts (rowid, body) VALUES (?1, ?2)").map_err(err)?;
    for (id, role, text) in changed {
        del.execute([id]).map_err(err)?;
        if super::passages::searchable(role) {
            ins.execute(params![id, crate::search::index_text(text)]).map_err(err)?;
        }
    }
    let mut vec = c.prepare("DELETE FROM search.kb_vecs WHERE passage_id = ?1").map_err(err)?;
    for id in gone {
        del.execute([id]).map_err(err)?;
        vec.execute([id]).map_err(err)?;
    }
    Ok(())
}

/// 一份材料要删了：拿掉它的块的索引和向量
pub fn forget_material(c: &Connection, material: i64) -> Result<(), String> {
    c.execute_batch(&format!("{FTS}; {VECS};")).map_err(err)?;
    c.execute("DELETE FROM search.kb_fts WHERE rowid IN (SELECT id FROM passages WHERE material_id = ?1)", [material]).map_err(err)?;
    c.execute("DELETE FROM search.kb_vecs WHERE passage_id IN (SELECT id FROM passages WHERE material_id = ?1)", [material]).map_err(err)?;
    Ok(())
}

/// 范围：哪几份材料（空 = 全部）
fn scope_sql(scope: &[i64]) -> String {
    if scope.is_empty() { String::new() } else { format!(" AND p.material_id IN ({})", scope.iter().map(i64::to_string).collect::<Vec<_>>().join(",")) }
}

/// 按词找：（块 id, bm25，越小越相关）
pub fn keyword(c: &Connection, q: &str, scope: &[i64], limit: usize) -> Result<Vec<(i64, f64)>, String> {
    c.execute_batch(FTS).map_err(err)?;
    let p = crate::search::parse_query(q);
    if p.fts.is_empty() && p.literals.is_empty() {
        return Ok(Vec::new());
    }
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let mut sql = if p.fts.is_empty() {
        format!("SELECT p.id, 0.0 FROM passages p WHERE p.role IN {SEARCHABLE}")
    } else {
        args.push(p.fts.join(" ").into());
        format!("SELECT p.id, bm25(kb_fts) FROM search.kb_fts f JOIN passages p ON p.id = f.rowid WHERE kb_fts MATCH ?1 AND p.role IN {SEARCHABLE}")
    };
    for lit in &p.literals {
        args.push(lit.clone().into());
        sql.push_str(&format!(" AND instr(p.text, ?{}) > 0", args.len()));
    }
    sql.push_str(&scope_sql(scope));
    sql.push_str(&format!(" ORDER BY 2, p.id LIMIT {}", limit.max(1)));
    let mut st = c.prepare(&sql).map_err(err)?;
    st.query_map(rusqlite::params_from_iter(args), |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| format!("检索失败：{e}"))?.collect::<Result<_, _>>().map_err(|e| format!("检索失败：{e}"))
}

/// 按意思找：和问题的向量最像的块（块 id, 余弦），从高到低
pub fn nearest(c: &Connection, model: &str, q: &[f32], scope: &[i64], keep: usize) -> Result<Vec<(i64, f32)>, String> {
    c.execute_batch(VECS).map_err(err)?;
    let sql = format!("SELECT p.id, p.text, v.sig, v.vec FROM search.kb_vecs v JOIN passages p ON p.id = v.passage_id WHERE v.model = ?1 AND p.role IN {SEARCHABLE}{}", scope_sql(scope));
    let mut st = c.prepare(&sql).map_err(err)?;
    let mut rows = st.query([model]).map_err(err)?;
    let mut scored: Vec<(i64, f32)> = Vec::new();
    while let Some(r) = rows.next().map_err(err)? {
        let text = r.get_ref(1).map_err(err)?.as_str().map_err(err)?;
        if r.get::<_, i64>(2).map_err(err)? != sig(text) {
            continue;
        }
        let blob = r.get_ref(3).map_err(err)?.as_blob().map_err(err)?;
        scored.push((r.get(0).map_err(err)?, embed::dot(q, &embed::from_blob(blob))));
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(keep);
    Ok(scored)
}

/// 一组块的向量（块 id → 向量）：对齐用
pub fn vectors(c: &Connection, model: &str, material: i64) -> Result<std::collections::HashMap<i64, Vec<f32>>, String> {
    c.execute_batch(VECS).map_err(err)?;
    let mut st = c.prepare("SELECT p.seq, p.text, v.sig, v.vec FROM search.kb_vecs v JOIN passages p ON p.id = v.passage_id WHERE v.model = ?1 AND p.material_id = ?2").map_err(err)?;
    let mut rows = st.query(params![model, material]).map_err(err)?;
    let mut out = std::collections::HashMap::new();
    while let Some(r) = rows.next().map_err(err)? {
        let text = r.get_ref(1).map_err(err)?.as_str().map_err(err)?;
        if r.get::<_, i64>(2).map_err(err)? != sig(text) {
            continue;
        }
        out.insert(r.get(0).map_err(err)?, embed::from_blob(r.get_ref(3).map_err(err)?.as_blob().map_err(err)?));
    }
    Ok(out)
}

/// 书的段落以前「问这本书」算过向量的：同一个模型、同样的文字，抄过来。返回抄了几块
fn reuse_concept_vectors(c: &Connection, model: &str) -> Result<usize, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS search.concept_vecs (para_id INTEGER PRIMARY KEY, model TEXT NOT NULL, sig INTEGER NOT NULL, vec BLOB NOT NULL)").map_err(err)?;
    let sql = format!(
        "SELECT p.id, p.text, v.sig, v.vec FROM passages p
         JOIN kb_materials m ON m.id = p.material_id AND m.source_kind = 'concept_text'
         JOIN concept_texts t ON t.uid = m.source_ref
         JOIN concept_paragraphs cp ON cp.text_id = t.id AND cp.seq = p.seq
         JOIN search.concept_vecs v ON v.para_id = cp.id AND v.model = ?1
         LEFT JOIN search.kb_vecs k ON k.passage_id = p.id AND k.model = ?1
         WHERE p.role IN {SEARCHABLE} AND k.passage_id IS NULL"
    );
    let rows: Vec<(i64, String, i64, Vec<u8>)> = {
        let mut st = c.prepare(&sql).map_err(err)?;
        st.query_map([model], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    let tx = Tx::begin(c)?;
    let mut n = 0;
    {
        let mut ins = tx.prepare("INSERT OR REPLACE INTO search.kb_vecs (passage_id, model, sig, vec) VALUES (?1, ?2, ?3, ?4)").map_err(err)?;
        for (id, text, s, vec) in rows {
            if s == sig(&text) {
                ins.execute(params![id, model, s, vec]).map_err(err)?;
                n += 1;
            }
        }
    }
    tx.commit()?;
    Ok(n)
}

/// 还没有向量的块（id, 文字）和该有向量的总数
pub fn missing(c: &Connection, model: &str) -> Result<(Vec<(i64, String)>, usize), String> {
    c.execute_batch(VECS).map_err(err)?;
    let sql = format!("SELECT p.id, p.text, v.sig FROM passages p LEFT JOIN search.kb_vecs v ON v.passage_id = p.id AND v.model = ?1 WHERE p.role IN {SEARCHABLE} ORDER BY p.material_id, p.seq");
    let mut st = c.prepare(&sql).map_err(err)?;
    let rows: Vec<(i64, String, Option<i64>)> = st.query_map([model], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let total = rows.len();
    Ok((rows.into_iter().filter(|(_, t, s)| *s != Some(sig(t))).map(|(id, t, _)| (id, t)).collect(), total))
}

fn store(c: &Connection, model: &str, rows: &[(i64, String)], vecs: &[Vec<f32>]) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    {
        let mut ins = tx.prepare("INSERT OR REPLACE INTO search.kb_vecs (passage_id, model, sig, vec) VALUES (?1, ?2, ?3, ?4)").map_err(err)?;
        for ((id, text), v) in rows.iter().zip(vecs) {
            ins.execute(params![id, model, sig(text), embed::to_blob(v)]).map_err(err)?;
        }
    }
    tx.commit()
}

/// 补齐向量（同步；后台线程和测试都走它）。`progress(已有, 总数)`。返回新算了几块
pub fn index_all(db: &impl super::Db, local: &LocalModels, cancel: &AtomicBool, progress: &dyn Fn(usize, usize)) -> Result<usize, String> {
    let model = local.ensure(Svc::Embedding)?;
    let (todo, total) = db.with(|c| {
        reuse_concept_vectors(c, &model.key)?;
        missing(c, &model.key)
    })?;
    let mut done = total - todo.len();
    progress(done, total);
    let mut made = 0;
    for batch in todo.chunks(embed::BATCH) {
        if cancel.load(Ordering::SeqCst) {
            return Err("已停止。".into());
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let vecs = embed::embed(local, &model, &texts, None)?;
        db.with(|c| store(c, &model.key, batch, &vecs))?;
        made += batch.len();
        done += batch.len();
        progress(done, total);
    }
    Ok(made)
}

// ---------- 后台 ----------

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    /// 能读的块一共多少、已经有向量的多少
    pub total: usize,
    pub done: usize,
    pub running: bool,
    pub error: Option<String>,
    /// 向量模型的名字；本机模型用不了时是空，`issue` 说原因
    pub model: Option<String>,
    pub issue: Option<String>,
    /// 材料数
    pub materials: usize,
}

#[derive(Default)]
pub struct Indexer {
    running: AtomicBool,
    error: Mutex<Option<String>>,
    /// 正在建时的进度（done, total）
    progress: Mutex<(usize, usize)>,
}

pub fn status(state: &AppState, local: &LocalModels, ix: &Indexer) -> Result<IndexStatus, String> {
    let running = ix.running.load(Ordering::SeqCst);
    let mut st = IndexStatus { running, error: ix.error.lock().ok().and_then(|e| e.clone()), ..Default::default() };
    st.materials = state.with_db(|c| c.query_row("SELECT COUNT(*) FROM kb_materials", [], |r| r.get::<_, i64>(0)).map_err(err))? as usize;
    match local.model(Svc::Embedding) {
        Ok(m) => {
            if running && let Ok(p) = ix.progress.lock() {
                (st.done, st.total) = *p;
            } else {
                let (todo, total) = state.with_db(|c| missing(c, &m.key))?;
                st.total = total;
                st.done = total - todo.len();
            }
            st.model = Some(m.name);
        }
        Err(e) => {
            st.issue = Some(e);
            st.total = state.with_db(|c| c.query_row(&format!("SELECT COUNT(*) FROM passages WHERE role IN {SEARCHABLE}"), [], |r| r.get::<_, i64>(0)).map_err(err))? as usize;
        }
    }
    Ok(st)
}

/// 在后台补齐向量；已经在建、本机模型用不了、或者没有缺的就什么都不做
pub fn start(app: &AppHandle) {
    let (Some(state), Some(ix)) = (app.try_state::<AppState>(), app.try_state::<Arc<Indexer>>()) else { return };
    let local = state.local.clone();
    if local.mode() == Mode::Off || local.model(Svc::Embedding).is_err() {
        return;
    }
    if ix.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, ix) = (app.clone(), ix.inner().clone());
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let never = AtomicBool::new(false);
        if let Ok(mut e) = ix.error.lock() {
            *e = None;
        }
        let r = index_all(state.inner(), &local, &never, &|done, total| {
            if let Ok(mut p) = ix.progress.lock() {
                *p = (done, total);
            }
        });
        match r {
            Ok(_) => {
                if let Ok(m) = local.model(Svc::Embedding)
                    && let Err(e) = state.with_db(|c| super::align::refine_all(c, &m.key, &crate::text::now_iso()))
                {
                    eprintln!("用向量重新对齐失败：{e}");
                }
            }
            Err(e) => {
                if let Ok(mut x) = ix.error.lock() {
                    *x = Some(e);
                }
            }
        }
        ix.running.store(false, Ordering::SeqCst);
    });
}

pub fn manage(app: &tauri::App) {
    app.manage(Arc::new(Indexer::default()));
}
