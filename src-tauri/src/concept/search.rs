//! 段落检索（`decisions/0024`「检索」）：概念研究的段落也进 `search.db`（派生数据，删了能重建，不写 migration）。
//!
//! - 一张 FTS5 表 `search.concept_paras(body)`，rowid = `concept_paragraphs.id`；分词和查询的改写沿用文献库的
//!   （`search::index_text` / `parse_query`：中文二元组、西文前缀）。
//! - 只收正文、旁注、注释、导读（标题和前后附不收）。
//! - 表不在、或和主库里的段落对不上（恢复了备份、搜索索引整个重建过），就整个重建这张表。
//! - 中文问题搜英文原著：先搜对齐的中译段落，再按对齐映射回原著（`concept::commands::concept_search`）。

use rusqlite::{Connection, params};

use crate::common::{Tx, err};

const CREATE: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS search.concept_paras USING fts5(body, tokenize = 'unicode61 remove_diacritics 2')";
const ROLES: &str = "('body', 'sidenote', 'note', 'guide')";

fn ids(c: &Connection, sql: &str) -> Result<Vec<i64>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 表不在就建；和主库对不上就重建。返回是否重建了
pub fn ensure(c: &Connection) -> Result<bool, String> {
    c.execute_batch(CREATE).map_err(err)?;
    let want = ids(c, &format!("SELECT id FROM concept_paragraphs WHERE role IN {ROLES} ORDER BY id"))?;
    let have = ids(c, "SELECT rowid FROM search.concept_paras ORDER BY rowid").unwrap_or_default();
    if have == want {
        return Ok(false);
    }
    rebuild(c)?;
    Ok(true)
}

pub fn rebuild(c: &Connection) -> Result<usize, String> {
    let tx = Tx::begin(c)?;
    tx.execute_batch(&format!("DROP TABLE IF EXISTS search.concept_paras; {CREATE};")).map_err(err)?;
    let n = write(&tx, None)?;
    tx.commit()?;
    Ok(n)
}

fn write(c: &Connection, text_id: Option<i64>) -> Result<usize, String> {
    let sql = format!("SELECT id, body FROM concept_paragraphs WHERE role IN {ROLES}{} ORDER BY id", if text_id.is_some() { " AND text_id = ?1" } else { "" });
    let mut st = c.prepare(&sql).map_err(err)?;
    let args: Vec<i64> = text_id.into_iter().collect();
    let rows: Vec<(i64, String)> = st.query_map(rusqlite::params_from_iter(args), |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut ins = c.prepare("INSERT INTO search.concept_paras (rowid, body) VALUES (?1, ?2)").map_err(err)?;
    for (id, body) in &rows {
        ins.execute(params![id, crate::search::index_text(body)]).map_err(err)?;
    }
    Ok(rows.len())
}

/// 新存了一份文本：把它的段落加进来
pub fn add_text(c: &Connection, text_id: i64) -> Result<(), String> {
    c.execute_batch(CREATE).map_err(err)?;
    let tx = Tx::begin(c)?;
    tx.execute("DELETE FROM search.concept_paras WHERE rowid IN (SELECT id FROM concept_paragraphs WHERE text_id = ?1)", [text_id]).map_err(err)?;
    write(&tx, Some(text_id))?;
    tx.commit()
}

/// 删文本之前：拿掉它的段落
pub fn remove_text(c: &Connection, text_id: i64) -> Result<(), String> {
    c.execute_batch(CREATE).map_err(err)?;
    c.execute("DELETE FROM search.concept_paras WHERE rowid IN (SELECT id FROM concept_paragraphs WHERE text_id = ?1)", [text_id]).map_err(err)?;
    Ok(())
}

/// 在一份文本里搜：命中的段落（seq）和相关度（bm25，越小越相关），按相关度排
pub fn query(c: &Connection, text_id: i64, q: &str, limit: usize) -> Result<Vec<(usize, f64)>, String> {
    let p = crate::search::parse_query(q);
    if p.fts.is_empty() && p.literals.is_empty() {
        return Ok(Vec::new());
    }
    let mut args: Vec<rusqlite::types::Value> = vec![text_id.into()];
    let mut sql = if p.fts.is_empty() {
        "SELECT p.seq, 0.0 FROM concept_paragraphs p WHERE p.text_id = ?1".to_string()
    } else {
        args.push(p.fts.join(" ").into());
        "SELECT p.seq, bm25(concept_paras) FROM search.concept_paras f JOIN concept_paragraphs p ON p.id = f.rowid WHERE concept_paras MATCH ?2 AND p.text_id = ?1".to_string()
    };
    for lit in &p.literals {
        args.push(lit.clone().into());
        sql.push_str(&format!(" AND instr(p.body, ?{}) > 0", args.len()));
    }
    sql.push_str(&format!(" AND p.role IN {ROLES} ORDER BY 2, p.seq LIMIT {}", limit.max(1)));
    let mut st = c.prepare(&sql).map_err(err)?;
    st.query_map(rusqlite::params_from_iter(args), |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, f64>(1)?))).map_err(|e| format!("检索失败：{e}"))?.collect::<Result<_, _>>().map_err(|e| format!("检索失败：{e}"))
}
