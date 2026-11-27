//! 刷新模型清单（设置 › 模型与费用 每家的「刷新」，`decisions/0041`）：问各家的接口现在有哪些模型、能不能思考、认几档深浅。
//!
//! - Anthropic：`GET /v1/models` 的 `capabilities` 直接写着（思考能不能关、关的写法、五档深浅各认不认）。
//! - OpenAI：`GET /v1/models` 只有名字。深浅的办法：对每个新模型发一个 effort 写 `minimal` 的请求——这一代都不认它，
//!   接口拒收（400，不计费）时会列出这个模型认的档位，`none` 在里面就是能关；说不认 `reasoning.effort` 的是不思考的模型。
//!   万一它认 `minimal`、真的答了（几个 token），照价记进账本，档位按同系列推断。
//! - DeepSeek：`GET /models` 只有名字，档位照自带清单的同系列推断。
//! - Google：`GET /v1beta/models` 有 `thinking`（会不会想），档位按同系列推断（Lite 多一档 minimal）。
//! - Vertex AI：`GET v1beta1/publishers/google/models`（gcloud 的令牌）；项目没开 Vertex AI API 时把原因说清楚。
//! - 只列 Pith 能用的：对话用的文字模型，Gemini 3 以上、GPT-5.5 以上；只认 `budget_tokens` 的旧 Claude 不列（Pith 只发 adaptive）。
//! - 拿到的存在设置 `model.discovered`（每家一份，带日子）；自带清单里有的照自带的（价格、档位都核过）。

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::catalog::{self, Found, Source, Thinking};
use super::{Provider, vertex};

pub const SAVED_KEY: &str = "model.discovered";

/// 一家刷新的结果
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// 上一次刷新成功的时间
    pub at: Option<String>,
    pub models: Vec<Found>,
    /// 上一次刷新没成：为什么（留着上一次的清单）
    #[serde(default)]
    pub note: Option<String>,
}

pub type Saved = BTreeMap<Provider, Listing>;

pub fn saved(c: &rusqlite::Connection) -> Saved {
    crate::metadata::setting(c, SAVED_KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// 启动后第一次用到模型时把存下的清单登记一遍
pub fn ensure_loaded(db: &impl crate::kb::Db) {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Ok(s) = db.with(|c| Ok(saved(c))) {
            for (p, l) in s {
                catalog::register(p, &l.models);
            }
        }
    });
}

/// 存下一家的结果、登记
pub fn store(c: &rusqlite::Connection, p: Provider, result: Result<Vec<Found>, String>, now: &str) -> Result<(), String> {
    let mut all = saved(c);
    let l = all.entry(p).or_default();
    match result {
        Ok(models) => {
            *l = Listing { at: Some(now.to_string()), models, note: None };
        }
        Err(e) => l.note = Some(e),
    }
    catalog::register(p, &l.models);
    let json = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    crate::metadata::put_setting(c, SAVED_KEY, Some(&json))
}

// ---------- 问接口 ----------

/// 问一家要用的东西
pub enum Auth {
    Key(String),
    /// Vertex：项目、区域，令牌现换
    Vertex(vertex::VertexConfig),
}

fn agent() -> ureq::Agent {
    crate::research::meter::agent(Duration::from_secs(60))
}

fn net_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "超时了。".into(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => format!("连不上（{e}）。检查网络后再试。"),
        other => format!("请求失败：{other}"),
    }
}

fn read(mut resp: ureq::http::Response<ureq::Body>) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn get(url: &str, headers: &[(&str, String)]) -> Result<(u16, Value), String> {
    let mut r = agent().get(url);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    r.call().map(read).map_err(net_err)
}

fn post(url: &str, headers: &[(&str, String)], body: &Value) -> Result<(u16, Value), String> {
    let mut r = agent().post(url).header("content-type", "application/json");
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    r.send(body.to_string().as_str()).map(read).map_err(net_err)
}

/// 接口回的错：给站主看的一句
fn refused(p: Provider, status: u16, v: &Value, project: Option<&str>) -> String {
    let detail = v.pointer("/error/message").and_then(Value::as_str).map(|m| super::short(m, 160)).unwrap_or_default();
    let reason = v.pointer("/error/details/0/reason").and_then(Value::as_str).unwrap_or("");
    match (p, status) {
        (Provider::Vertex, 403) if reason == "SERVICE_DISABLED" || detail.contains("has not been used") || detail.contains("disabled") => format!(
            "这个项目还没打开 Vertex AI API（Google 现在叫它 Agent Platform API）。在终端跑 gcloud services enable aiplatform.googleapis.com --project {}，或到 Google Cloud 控制台的「API 和服务」里打开；Vertex AI 一般还要项目开了计费。",
            project.unwrap_or("你的项目")
        ),
        (Provider::Vertex, 401) => "Vertex AI 不认这个登录：在终端再跑一次 gcloud auth application-default login。".into(),
        (_, 401 | 403) => format!("{} 不认这把密钥（{status}）。{detail}", p.label()),
        (_, 429) => format!("{} 说请求太多，等一会儿再刷新。{detail}", p.label()),
        _ => format!("{} 回了 {status}。{detail}", p.label()),
    }
}

/// 问一家现在有哪些模型。`ledger`：OpenAI 的探测万一真答了，照价记账
pub fn list(p: Provider, auth: &Auth, ledger: Option<&crate::agent::usage::Ledger>) -> Result<Vec<Found>, String> {
    match (p, auth) {
        (Provider::Anthropic, Auth::Key(k)) => {
            let (s, v) = get("https://api.anthropic.com/v1/models?limit=100", &[("x-api-key", k.clone()), ("anthropic-version", super::anthropic::API_VERSION.into())])?;
            if s != 200 {
                return Err(refused(p, s, &v, None));
            }
            Ok(parse_anthropic(&v))
        }
        (Provider::Openai, Auth::Key(k)) => {
            let auth = [("authorization", format!("Bearer {k}"))];
            let (s, v) = get("https://api.openai.com/v1/models", &auth)?;
            if s != 200 {
                return Err(refused(p, s, &v, None));
            }
            let mut out = Vec::new();
            for id in openai_candidates(&v).into_iter().take(24) {
                if let Some(m) = catalog::MODELS.iter().find(|m| m.provider == p && m.api_model == id) {
                    out.push(found_of_spec(m));
                    continue;
                }
                let body = json!({ "model": id, "input": "hi", "max_output_tokens": 16, "store": false, "reasoning": { "effort": "minimal" } });
                let Ok((s, v)) = post("https://api.openai.com/v1/responses", &auth, &body) else { continue };
                if s == 200 {
                    record_probe(ledger, &id, &v);
                }
                if let Some((thinking, efforts, source)) = openai_probe(s, &v) {
                    out.push(Found { label: pretty(&id), api_model: id, context: 400_000, max_output: 128_000, thinking, off: None, efforts, source });
                }
            }
            Ok(out)
        }
        (Provider::Deepseek, Auth::Key(k)) => {
            let (s, v) = get("https://api.deepseek.com/models", &[("authorization", format!("Bearer {k}"))])?;
            if s != 200 {
                return Err(refused(p, s, &v, None));
            }
            Ok(parse_deepseek(&v))
        }
        (Provider::Google, Auth::Key(k)) => {
            let (s, v) = get(&format!("{}/models?pageSize=200", super::gemini::BASE), &[("x-goog-api-key", k.clone())])?;
            if s != 200 {
                return Err(refused(p, s, &v, None));
            }
            Ok(parse_gemini(&v))
        }
        (Provider::Vertex, Auth::Vertex(cfg)) => {
            let token = vertex::token().get().map_err(|e| e.message)?;
            let project = cfg.project.trim().to_string();
            let (s, v) = get("https://aiplatform.googleapis.com/v1beta1/publishers/google/models?pageSize=200", &[("authorization", format!("Bearer {token}")), ("x-goog-user-project", project.clone())])?;
            if s == 401 {
                vertex::token().forget();
            }
            if s != 200 {
                return Err(refused(p, s, &v, Some(&project)));
            }
            Ok(parse_vertex(&v))
        }
        _ => Err("这一家要的认证不对。".into()),
    }
}

fn found_of_spec(m: &catalog::ModelSpec) -> Found {
    Found {
        api_model: m.api_model.into(),
        label: m.label.into(),
        context: m.context,
        max_output: m.max_output,
        thinking: m.thinking,
        off: m.off.map(str::to_string),
        efforts: m.efforts.iter().map(|e| e.to_string()).collect(),
        source: m.source,
    }
}

fn record_probe(ledger: Option<&crate::agent::usage::Ledger>, model: &str, v: &Value) {
    let Some(l) = ledger else { return };
    let tokens = super::Tokens { input: v.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(16), output: v.pointer("/usage/output_tokens").and_then(Value::as_u64).unwrap_or(16), ..Default::default() };
    let cost = catalog::price_of(model).unwrap_or(super::meter::UNKNOWN).usd(&tokens);
    let _ = l.append(&crate::agent::usage::Call {
        at: crate::text::now_iso(),
        purpose: "model_probe".into(),
        provider: "openai".into(),
        kind: crate::agent::usage::KIND_MODEL.into(),
        searches: 0,
        model: model.into(),
        tokens,
        cost_usd: cost,
        estimated: false,
        mock: false,
        note: Some("刷新模型清单时探测思考档位".into()),
        request_id: None,
        conversation: None,
    });
}

// ---------- 读接口的回答（不联网，测试直接调） ----------

const ALL_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub fn parse_anthropic(v: &Value) -> Vec<Found> {
    let sup = |x: &Value| x["supported"].as_bool().unwrap_or(false);
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?;
            let caps = &m["capabilities"];
            let types = &caps["thinking"]["types"];
            // Pith 只发 adaptive：只认 budget_tokens 的旧模型不列
            if !id.starts_with("claude-") || !sup(&types["adaptive"]) {
                return None;
            }
            let (thinking, off) = if sup(&types["disabled"]) {
                (Thinking::Switch, Some("disabled".to_string()))
            } else if sup(&types["between_tools"]) {
                (Thinking::Switch, Some("between_tools".to_string()))
            } else {
                (Thinking::Always, None)
            };
            let efforts = if sup(&caps["effort"]) { ALL_EFFORTS.iter().filter(|e| sup(&caps["effort"][**e])).map(|e| e.to_string()).collect() } else { Vec::new() };
            Some(Found {
                api_model: id.into(),
                label: m["display_name"].as_str().unwrap_or(id).into(),
                context: m["max_input_tokens"].as_u64().unwrap_or(200_000),
                max_output: m["max_tokens"].as_u64().unwrap_or(64_000).min(u32::MAX as u64) as u32,
                thinking,
                off,
                efforts,
                source: Source::Api,
            })
        })
        .collect()
}

/// `gpt-6.2-sol` 这类：主版本 5.5 以上、名字里只有一个系列词；音频、图像、实时、转写、搜索、行业版、Pro 这些不列
pub fn openai_candidates(v: &Value) -> Vec<String> {
    const SKIP: [&str; 13] = ["pro", "codex", "audio", "image", "realtime", "transcribe", "tts", "search", "live", "healthcare", "translate", "whisper", "chat"];
    let mut rows: Vec<(i64, String)> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?;
            let rest = id.strip_prefix("gpt-")?;
            let (ver, word) = match rest.split_once('-') {
                Some((v, w)) => (v, Some(w)),
                None => (rest, None),
            };
            let (major, minor) = match ver.split_once('.') {
                Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?),
                None => (ver.parse::<u32>().ok()?, 0),
            };
            if major * 10 + minor < 55 || major > 99 {
                return None;
            }
            if let Some(w) = word
                && (!w.chars().all(|c| c.is_ascii_lowercase()) || SKIP.contains(&w))
            {
                return None;
            }
            Some((m["created"].as_i64().unwrap_or(0), id.to_string()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    rows.into_iter().map(|(_, id)| id).collect()
}

/// 探测的回答 → 思考、档位。None：这个模型用不了（没权限、不存在）
pub fn openai_probe(status: u16, v: &Value) -> Option<(Thinking, Vec<String>, Source)> {
    if status == 200 {
        // 它认 minimal、真答了：档位按同系列推断
        return Some((Thinking::Always, ["minimal", "low", "medium", "high"].map(String::from).to_vec(), Source::Inferred));
    }
    let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
    if msg.contains("reasoning.effort' is not supported") || msg.contains("Unsupported parameter: 'reasoning") {
        return Some((Thinking::Never, Vec::new(), Source::Api));
    }
    let list = msg.split("Supported values are:").nth(1)?;
    let values: Vec<String> = list.split('\'').skip(1).step_by(2).map(str::to_string).collect();
    if values.is_empty() {
        return None;
    }
    let thinking = if values.iter().any(|x| x == "none") { Thinking::Switch } else { Thinking::Always };
    Some((thinking, values.into_iter().filter(|x| x != "none").collect(), Source::Api))
}

pub fn parse_deepseek(v: &Value) -> Vec<Found> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .filter(|id| id.starts_with("deepseek-"))
        .map(|id| match catalog::MODELS.iter().find(|m| m.provider == Provider::Deepseek && m.api_model == id) {
            Some(m) => found_of_spec(m),
            None => Found {
                api_model: id.into(),
                label: pretty(id),
                context: 1_048_576,
                max_output: 393_216,
                thinking: Thinking::Switch,
                off: None,
                efforts: ["low", "high", "max"].map(String::from).to_vec(),
                source: Source::Inferred,
            },
        })
        .collect()
}

/// Gemini 3 以上的文字模型（`gemini-3.8-flash`、`gemini-4-carbon`），语音、图像、机器人、别名这些不列
fn gemini_wanted(id: &str) -> bool {
    const SKIP: [&str; 13] = ["tts", "image", "transcribe", "robotics", "computer-use", "customtools", "latest", "omni", "embedding", "live", "audio", "native", "exp"];
    let Some(rest) = id.strip_prefix("gemini-") else { return false };
    let major = rest.split(['.', '-']).next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0);
    major >= 3 && !SKIP.iter().any(|s| id.contains(s))
}

fn gemini_efforts(id: &str) -> Vec<String> {
    if id.contains("lite") { ["minimal", "low", "medium", "high"].map(String::from).to_vec() } else { ["low", "medium", "high"].map(String::from).to_vec() }
}

pub fn parse_gemini(v: &Value) -> Vec<Found> {
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["name"].as_str()?.strip_prefix("models/")?;
            let generates = m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent"));
            if !generates || !gemini_wanted(id) {
                return None;
            }
            let thinks = m["thinking"].as_bool().unwrap_or(false);
            Some(Found {
                api_model: id.into(),
                label: m["displayName"].as_str().unwrap_or(id).into(),
                context: m["inputTokenLimit"].as_u64().unwrap_or(1_048_576),
                max_output: m["outputTokenLimit"].as_u64().unwrap_or(65_536).min(u32::MAX as u64) as u32,
                thinking: if thinks { Thinking::Always } else { Thinking::Never },
                off: None,
                efforts: if thinks { gemini_efforts(id) } else { Vec::new() },
                source: if thinks { Source::Inferred } else { Source::Api },
            })
        })
        .collect()
}

pub fn parse_vertex(v: &Value) -> Vec<Found> {
    v["publisherModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str()?.rsplit('/').next())
        .filter(|id| gemini_wanted(id))
        .map(|id| match catalog::MODELS.iter().find(|m| m.provider == Provider::Vertex && m.api_model == id) {
            Some(m) => found_of_spec(m),
            None => Found { api_model: id.into(), label: pretty(id), context: 1_048_576, max_output: 65_536, thinking: Thinking::Always, off: None, efforts: gemini_efforts(id), source: Source::Inferred },
        })
        .collect()
}

/// `gpt-6.2-sol` → `GPT-6.2 Sol`；`deepseek-v4-pro` → `DeepSeek V4 Pro`；`gemini-4-neon` → `Gemini 4 Neon`
pub fn pretty(id: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for (i, w) in id.split('-').enumerate() {
        let s = match (i, w) {
            (0, "gpt") => "GPT".to_string(),
            (0, "deepseek") => "DeepSeek".to_string(),
            (_, w) if w.starts_with(|c: char| c.is_ascii_digit()) => w.to_string(),
            (_, w) if w.len() <= 2 => w.to_uppercase(),
            (_, w) => {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        };
        words.push(s);
    }
    // GPT 和版本号连着写
    if words.first().is_some_and(|w| w == "GPT") && words.len() > 1 {
        let v = words.remove(1);
        words[0] = format!("GPT-{v}");
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_capabilities_become_thinking_and_efforts() {
        let cap = |disabled: bool, between: bool, adaptive: bool, max: bool| {
            json!({ "thinking": { "supported": true, "types": { "adaptive": { "supported": adaptive }, "disabled": { "supported": disabled }, "between_tools": { "supported": between }, "enabled": { "supported": !adaptive } } },
                    "effort": { "supported": true, "low": { "supported": true }, "medium": { "supported": true }, "high": { "supported": true }, "xhigh": { "supported": max }, "max": { "supported": max } } })
        };
        let v = json!({ "data": [
            { "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "max_input_tokens": 1000000, "max_tokens": 128000, "capabilities": cap(false, false, true, true) },
            { "id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5", "max_input_tokens": 1000000, "max_tokens": 128000, "capabilities": cap(false, true, true, true) },
            { "id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5", "max_input_tokens": 1000000, "max_tokens": 128000, "capabilities": cap(true, false, true, true) },
            { "id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6", "max_input_tokens": 1000000, "max_tokens": 128000, "capabilities": cap(true, false, true, false) },
            { "id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "capabilities": cap(true, false, false, false) }
        ] });
        let f = parse_anthropic(&v);
        assert_eq!(f.iter().map(|m| m.api_model.as_str()).collect::<Vec<_>>(), ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-sonnet-4-6"]);
        assert_eq!((f[0].thinking, f[0].off.as_deref()), (Thinking::Always, None));
        assert_eq!((f[1].thinking, f[1].off.as_deref()), (Thinking::Switch, Some("between_tools")));
        assert_eq!((f[2].thinking, f[2].off.as_deref()), (Thinking::Switch, Some("disabled")));
        assert_eq!(f[3].efforts, ["low", "medium", "high"]);
        // 自带清单和接口说的一致（2026-11-27 核过的）
        for m in &f[..3] {
            let spec = catalog::MODELS.iter().find(|s| s.api_model == m.api_model).unwrap();
            assert_eq!((spec.thinking, spec.off.map(str::to_string)), (m.thinking, m.off.clone()), "{}", m.api_model);
        }
    }

    #[test]
    fn openai_lists_new_text_models_and_reads_the_probe() {
        let v = json!({ "data": [
            { "id": "gpt-6.2-sol", "created": 30 }, { "id": "gpt-6.2-sol-healthcare", "created": 31 }, { "id": "gpt-6-luna", "created": 20 },
            { "id": "gpt-5.5", "created": 10 }, { "id": "gpt-5.5-2026-04-23", "created": 10 }, { "id": "gpt-5.4-mini", "created": 9 },
            { "id": "gpt-5.5-pro", "created": 11 }, { "id": "gpt-realtime-2", "created": 40 }, { "id": "gpt-image-2.5-flare", "created": 41 }, { "id": "o3", "created": 1 }
        ] });
        assert_eq!(openai_candidates(&v), ["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5"]);
        let err = |m: &str| json!({ "error": { "message": m } });
        let (t, e, s) = openai_probe(400, &err("Unsupported value: 'minimal' is not supported with the 'gpt-6-luna' model. Supported values are: 'none', 'low', 'medium', 'high', 'xhigh', and 'max'.")).unwrap();
        assert_eq!((t, e.as_slice(), s), (Thinking::Switch, ["low", "medium", "high", "xhigh", "max"].map(String::from).as_slice(), Source::Api));
        let (t, e, _) = openai_probe(400, &err("Unsupported value: 'minimal' is not supported with the 'gpt-6.2-sol' model. Supported values are: 'low', 'medium', 'high', 'xhigh', and 'max'.")).unwrap();
        assert_eq!((t, e.len()), (Thinking::Always, 5));
        assert_eq!(openai_probe(400, &err("Unsupported parameter: 'reasoning.effort' is not supported with this model.")).unwrap().0, Thinking::Never);
        assert_eq!(openai_probe(200, &json!({})).unwrap().2, Source::Inferred);
        assert!(openai_probe(404, &err("The model `gpt-9` does not exist")).is_none());
    }

    #[test]
    fn gemini_and_deepseek_lists() {
        let v = json!({ "models": [
            { "name": "models/gemini-4-neon", "displayName": "Gemini 4 Neon", "inputTokenLimit": 1048576, "outputTokenLimit": 65536, "thinking": true, "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-3.1-flash-lite", "displayName": "Gemini 3.1 Flash Lite", "thinking": true, "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-2.5-pro", "thinking": true, "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-3.8-flash-tts", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-flash-latest", "thinking": true, "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-embedding-2", "supportedGenerationMethods": ["embedContent"] }
        ] });
        let g = parse_gemini(&v);
        assert_eq!(g.iter().map(|m| m.api_model.as_str()).collect::<Vec<_>>(), ["gemini-4-neon", "gemini-3.1-flash-lite"]);
        assert_eq!(g[1].efforts[0], "minimal");
        let d = parse_deepseek(&json!({ "data": [{ "id": "deepseek-flash" }, { "id": "deepseek-v5" }] }));
        assert_eq!((d[0].source, d[1].source, d[1].label.as_str()), (Source::Builtin, Source::Inferred, "DeepSeek V5"));
        let vx = parse_vertex(&json!({ "publisherModels": [{ "name": "publishers/google/models/gemini-4-carbon" }, { "name": "publishers/google/models/imagen-5" }] }));
        assert_eq!(vx.len(), 1);
        assert_eq!(pretty("gpt-6.2-sol"), "GPT-6.2 Sol");
        assert_eq!(pretty("gemini-4-neon"), "Gemini 4 Neon");
    }

    #[test]
    fn vertex_says_how_to_turn_the_api_on() {
        let v = json!({ "error": { "code": 403, "message": "Agent Platform API has not been used in project p before or it is disabled.", "details": [{ "reason": "SERVICE_DISABLED" }] } });
        let m = refused(Provider::Vertex, 403, &v, Some("gen-lang-client-1"));
        assert!(m.contains("gcloud services enable aiplatform.googleapis.com --project gen-lang-client-1"), "{m}");
    }

    #[test]
    fn stored_listings_register_and_keep_the_old_list_on_failure() {
        let db = crate::db::tests::memory_db();
        let f = Found { api_model: "gemini-4-neon".into(), label: "Gemini 4 Neon".into(), context: 1, max_output: 65_536, thinking: Thinking::Always, off: None, efforts: vec!["low".into()], source: Source::Inferred };
        store(&db, Provider::Google, Ok(vec![f]), "2026-11-27T10:00:00").unwrap();
        store(&db, Provider::Google, Err("连不上".into()), "2026-11-27T11:00:00").unwrap();
        let s = saved(&db);
        let l = &s[&Provider::Google];
        assert_eq!((l.at.as_deref(), l.note.as_deref(), l.models.len()), (Some("2026-11-27T10:00:00"), Some("连不上"), 1));
        assert!(catalog::model("google/gemini-4-neon").is_some());
    }
}
