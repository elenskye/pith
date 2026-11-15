//! 把文字变成向量（本机 8081，`/v1/embeddings`）。问题按模型的模板写，存进索引的段落原样送。
//! 一批 16 条；服务嫌某条太长时，把这一批逐条减半重试（书里偶尔有特别长的段落）。
//! 模拟：按字和相邻两字散列成 256 维，有共同字词的更像；含「向量失败测试」的当作服务不可用。

use std::time::Duration;

use serde_json::{Value, json};

use super::{LocalModels, ServiceModel, Svc};

pub const MOCK_FAILURE: &str = "向量失败测试";
pub const BATCH: usize = 16;
/// 一段最多送这么多字（8K 上下文的模型放得下；更长的只取开头）
pub const MAX_CHARS: usize = 6000;

pub fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn request(agent: &ureq::Agent, model: &ServiceModel, inputs: &[String]) -> Result<Vec<Vec<f32>>, Option<String>> {
    let body = json!({ "model": model.key, "input": inputs }).to_string();
    let mut resp = agent.post(format!("{}/v1/embeddings", Svc::Embedding.root())).header("content-type", "application/json").send(body).map_err(|_| Some("连不上本机的向量服务（127.0.0.1:8081）。".to_string()))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(256 * 1024 * 1024).read_to_string().unwrap_or_default();
    if status != 200 {
        let head: String = text.chars().take(2000).collect();
        // llama-server：「input (N tokens) is too large to process…」「…larger than the context size」
        return Err(if head.contains("too large") || head.contains("context size") { None } else { Some("本机的向量服务返回了错误。".to_string()) });
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| Some("本机的向量服务返回了读不懂的结果。".to_string()))?;
    let mut items: Vec<&Value> = v["data"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    if items.len() != inputs.len() {
        return Err(Some("本机的向量服务返回的数量不对。".to_string()));
    }
    items.sort_by_key(|i| i["index"].as_i64().unwrap_or(0));
    let mut out = Vec::with_capacity(items.len());
    for i in items {
        let vec: Vec<f32> = i["embedding"].as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default();
        if vec.is_empty() {
            return Err(Some("本机的向量服务返回了空向量。".to_string()));
        }
        out.push(normalized(vec));
    }
    Ok(out)
}

/// 每条文字一个向量（归一化过，余弦就是点积），顺序不变
pub fn embed(rt: &LocalModels, model: &ServiceModel, texts: &[String], question_task: Option<&str>) -> Result<Vec<Vec<f32>>, String> {
    rt.touch(Svc::Embedding);
    if rt.is_mock() {
        if texts.iter().any(|t| t.contains(MOCK_FAILURE)) {
            return Err("模拟：本机的向量服务没有就绪。".into());
        }
        return Ok(texts.iter().map(|t| normalized(mock_vector(t))).collect());
    }
    let agent = LocalModels::agent(Duration::from_secs(120));
    let mut out = Vec::with_capacity(texts.len());
    for batch in texts.chunks(BATCH) {
        let prep = |t: &String, max: usize| {
            let t = clip(t, max);
            question_task.map_or(t.clone(), |task| model.question(task, &t))
        };
        let inputs: Vec<String> = batch.iter().map(|t| prep(t, MAX_CHARS)).collect();
        match request(&agent, model, &inputs) {
            Ok(v) => out.extend(v),
            Err(Some(e)) => return Err(e),
            Err(None) => {
                // 有一条太长：逐条送，太长的减半再试
                for t in batch {
                    let mut max = MAX_CHARS;
                    loop {
                        match request(&agent, model, &[prep(t, max)]) {
                            Ok(mut v) => {
                                out.append(&mut v);
                                break;
                            }
                            Err(None) if max > 200 => max /= 2,
                            Err(None) => return Err("有一段文字太长，向量模型放不下。".into()),
                            Err(Some(e)) => return Err(e),
                        }
                    }
                }
            }
        }
        rt.touch(Svc::Embedding);
    }
    Ok(out)
}

pub fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

pub fn mock_vector(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 256];
    let chars: Vec<char> = text.to_lowercase().chars().filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation() && !"，。！？；：、（）《》「」“”".contains(*c)).collect();
    let mut add = |piece: &[char]| {
        let mut h: u32 = 2_166_136_261;
        for c in piece {
            h = (h ^ *c as u32).wrapping_mul(16_777_619);
        }
        v[(h % 256) as usize] += 1.0;
    };
    for i in 0..chars.len() {
        add(&chars[i..=i]);
        if i + 1 < chars.len() {
            add(&chars[i..=i + 1]);
        }
    }
    v
}

/// 两个归一化向量的余弦（维数不同、空的算 0）
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 存进 search.db：小端 f32 连起来
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::super::Mode;
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn mock_vectors_score_shared_words_higher() {
        let rt = LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"));
        let m = rt.ensure(Svc::Embedding).unwrap();
        let v = embed(&rt, &m, &["分工受市场范围的限制".to_string(), "市场范围限制了分工的程度".to_string(), "周末去公园散步".to_string()], None).unwrap();
        assert!(dot(&v[0], &v[1]) > dot(&v[0], &v[2]));
        assert!((dot(&v[0], &v[0]) - 1.0).abs() < 1e-5);
        assert_eq!(dot(&[1.0, 0.0], &[1.0]), 0.0);
        assert!(embed(&rt, &m, &[format!("x{MOCK_FAILURE}")], None).is_err());
    }

    #[test]
    fn blobs_round_trip() {
        let v = normalized(vec![0.5, -1.25, 3.0]);
        assert_eq!(from_blob(&to_blob(&v)), v);
        assert_eq!(clip("一二三四", 2), "一二");
    }

    /// 连真实的 8081（会经 llmctl.sh 启动向量服务；测完不停，等它闲置自己停或手动 `llmctl.sh stop embedding`）：
    /// `PITH_LOCAL_MODELS=real cargo test -p pith real_embedding -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_embedding() {
        let rt = LocalModels::detect();
        assert_eq!(rt.mode(), Mode::Real, "先设 PITH_LOCAL_MODELS=real，并确认 ~/Documents/LLM 在");
        let m = rt.ensure(Svc::Embedding).unwrap();
        let q = embed(&rt, &m, &["为什么分工受市场范围的限制？".to_string()], Some("Given a question about a book, retrieve passages from the book that help answer it")).unwrap().remove(0);
        let docs = embed(&rt, &m, &["As it is the power of exchanging that gives occasion to the division of labour, so the extent of this division must always be limited by the extent of that power, or, in other words, by the extent of the market.".to_string(), "午饭 35 元，和同事吃了牛肉面。".to_string()], None).unwrap();
        let (market, lunch) = (dot(&q, &docs[0]), dot(&q, &docs[1]));
        println!("{} 维；市场 {market:.3}，午饭 {lunch:.3}", q.len());
        assert!(market > lunch + 0.2);
    }
}
