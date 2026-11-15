//! 重排（本机 8082，`/v1/rerank`）：给每段文字打一个「能不能回答这个问题」的分（0…1），用来给检索结果重新排序。
//! 模型文件自带判断的提示（Qwen3-Reranker：分数是回答「是」的概率），只送问题和段落。
//! 一次最多送 24 段，每段只取前 1500 字（这台机器上约每秒 3600 token，30 段 200 token 的约 1.7 秒）。
//! 模拟：模拟向量的余弦；问题里含「重排失败测试」的当作服务不可用。

use std::time::Duration;

use serde_json::{Value, json};

use super::embed::{clip, dot, mock_vector, normalized};
use super::{LocalModels, ServiceModel, Svc};

pub const MOCK_FAILURE: &str = "重排失败测试";
/// 低于这个分数的和问题无关（无关的大约 0.0001）
pub const FLOOR: f32 = 0.01;
pub const BATCH: usize = 24;
pub const MAX_CHARS: usize = 1500;

/// 每段一个分数，顺序和 `passages` 一样
pub fn scores(rt: &LocalModels, model: &ServiceModel, question: &str, passages: &[String]) -> Result<Vec<f32>, String> {
    rt.touch(Svc::Rerank);
    if passages.is_empty() {
        return Ok(Vec::new());
    }
    if rt.is_mock() {
        if question.contains(MOCK_FAILURE) {
            return Err("模拟：本机的重排服务没有就绪。".into());
        }
        let q = normalized(mock_vector(question));
        return Ok(passages.iter().map(|p| dot(&q, &normalized(mock_vector(p))).max(0.0)).collect());
    }
    let agent = LocalModels::agent(Duration::from_secs(120));
    let mut out = Vec::with_capacity(passages.len());
    for batch in passages.chunks(BATCH) {
        let docs: Vec<String> = batch.iter().map(|p| clip(p, MAX_CHARS)).collect();
        let body = json!({ "model": model.key, "query": clip(question, MAX_CHARS), "documents": docs, "top_n": docs.len() }).to_string();
        let mut resp = agent.post(format!("{}/v1/rerank", Svc::Rerank.root())).header("content-type", "application/json").send(body).map_err(|_| "连不上本机的重排服务（127.0.0.1:8082）。".to_string())?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        rt.touch(Svc::Rerank);
        if status != 200 {
            return Err("本机的重排服务返回了错误。".into());
        }
        let v: Value = serde_json::from_str(&text).map_err(|_| "本机的重排服务返回了读不懂的结果。".to_string())?;
        let mut got = vec![f32::NAN; batch.len()];
        for r in v["results"].as_array().into_iter().flatten() {
            let (Some(i), Some(s)) = (r["index"].as_u64(), r["relevance_score"].as_f64()) else { continue };
            if let Some(slot) = got.get_mut(i as usize) {
                *slot = s as f32;
            }
        }
        if got.iter().any(|s| s.is_nan()) {
            return Err("本机的重排服务返回的数量不对。".into());
        }
        out.extend(got);
    }
    Ok(out)
}

/// 按分数从高到低排（同分保持原来的顺序），丢掉低于 `FLOOR` 的；返回下标
pub fn order(scores: &[f32], keep: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).filter(|i| scores[*i] >= FLOOR).collect();
    idx.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
    idx.truncate(keep);
    idx
}

#[cfg(test)]
mod tests {
    use super::super::Mode;
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn order_keeps_the_best_above_the_floor() {
        assert_eq!(order(&[0.2, 0.9, 0.001, 0.2], 2), [1, 0]);
        assert!(order(&[0.005, 0.0], 6).is_empty());
    }

    #[test]
    fn mock_scores_follow_shared_words() {
        let rt = LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"));
        let m = rt.ensure(Svc::Rerank).unwrap();
        let docs = ["周末去公园散步。", "分工的程度受市场范围的限制。"].map(String::from);
        let s = scores(&rt, &m, "市场范围和分工", &docs).unwrap();
        assert!(s[1] > s[0]);
        assert!(scores(&rt, &m, "x", &[]).unwrap().is_empty());
        assert!(scores(&rt, &m, MOCK_FAILURE, &docs).is_err());
    }

    /// 连真实的 8082（会经 llmctl.sh 启动重排服务）：
    /// `PITH_LOCAL_MODELS=real cargo test -p pith real_rerank -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_rerank() {
        let rt = LocalModels::detect();
        assert_eq!(rt.mode(), Mode::Real, "先设 PITH_LOCAL_MODELS=real，并确认 ~/Documents/LLM 在");
        let m = rt.ensure(Svc::Rerank).unwrap();
        let docs = ["The division of labour is limited by the extent of the market.", "今天读完了一本小说，结尾很震撼。", "在偏僻的乡村，农民必须自己兼做屠夫、面包师和酿酒师，因为市场太小。"].map(String::from);
        let s = scores(&rt, &m, "为什么分工受市场范围的限制？", &docs).unwrap();
        println!("{s:?}");
        assert!(s[0] > s[1] && s[2] > s[1] && s[0] > 0.3);
    }
}
