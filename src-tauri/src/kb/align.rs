//! 中英对齐用上语义相似度（用户 2026-11-25 定；引擎的 `align_with` 早就留好了 `Similarity`）。
//!
//! 原著和译本都在知识库里、两边的正文段几乎都有向量（同一个本机向量模型）时，用两边段落向量的余弦重新对齐一遍，
//! 存回去并记下用的是哪个模型（`concept_alignments.similarity`）。已经是这个模型对的就不再算。
//! 只用长度和锚点的对齐还是默认：没有向量、或者向量不全（覆盖不到 95% 的正文段），都不动。

use std::collections::HashMap;

use concept_engine::align::{self, Alignment, Options};
use concept_engine::text::{Role, Text};
use rusqlite::Connection;

use super::{index, store};
use crate::common::err;

/// 两边的正文段至少这么多有向量，才用相似度对齐
const COVERAGE: f32 = 0.95;

/// 一本书每段的向量（seq → 向量），从它在知识库里那份材料的段落块来
fn vectors_of(c: &Connection, text_uid: &str, model: &str) -> Result<Option<HashMap<usize, Vec<f32>>>, String> {
    let Some(m) = store::by_source(c, "concept_text", text_uid)? else { return Ok(None) };
    let id = store::material_id(c, &m)?;
    Ok(Some(index::vectors(c, model, id)?.into_iter().map(|(seq, v)| (seq as usize, v)).collect()))
}

fn coverage(t: &Text, v: &HashMap<usize, Vec<f32>>) -> f32 {
    let bodies: Vec<usize> = t.paragraphs.iter().filter(|p| p.role == Role::Body).map(|p| p.seq).collect();
    if bodies.is_empty() {
        return 0.0;
    }
    bodies.iter().filter(|s| v.contains_key(s)).count() as f32 / bodies.len() as f32
}

/// 用向量算的对齐（不存）。哪一边向量不够就是 None
pub fn with_vectors(original: &Text, translation: &Text, a: &HashMap<usize, Vec<f32>>, b: &HashMap<usize, Vec<f32>>) -> Option<Alignment> {
    if coverage(original, a) < COVERAGE || coverage(translation, b) < COVERAGE {
        return None;
    }
    let sim = |x: usize, y: usize| -> Option<f32> { Some(crate::local::embed::dot(a.get(&x)?, b.get(&y)?)) };
    Some(align::align_with(original, translation, Some(&sim), &Options::default()))
}

/// 一对书：向量齐了、还不是用这个模型对的，就重新对齐、存回去。返回是否重算了
pub fn refine(c: &Connection, original_uid: &str, translation_uid: &str, model: &str, now: &str) -> Result<bool, String> {
    if let Some((version, Some(m))) = crate::concept::store::alignment_meta(c, original_uid, translation_uid)?
        && version == align::VERSION
        && m == model
    {
        return Ok(false);
    }
    let (Some(a), Some(b)) = (vectors_of(c, original_uid, model)?, vectors_of(c, translation_uid, model)?) else { return Ok(false) };
    let (ot, tt) = (crate::concept::store::text_load(c, original_uid)?, crate::concept::store::text_load(c, translation_uid)?);
    let Some(al) = with_vectors(&ot, &tt, &a, &b) else { return Ok(false) };
    crate::concept::store::alignment_save_with(c, original_uid, translation_uid, &al, Some(model), now)?;
    Ok(true)
}

/// 所有连着译本的原著：能用向量对齐的都对一遍。返回重算了几对
pub fn refine_all(c: &Connection, model: &str, now: &str) -> Result<usize, String> {
    let pairs: Vec<(String, String)> = {
        let mut st = c.prepare("SELECT o.uid, t.uid FROM concept_texts t JOIN concept_texts o ON o.id = t.original_id ORDER BY t.id").map_err(err)?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?
    };
    let mut n = 0;
    for (o, t) in pairs {
        if refine(c, &o, &t, model, now)? {
            n += 1;
        }
    }
    Ok(n)
}
