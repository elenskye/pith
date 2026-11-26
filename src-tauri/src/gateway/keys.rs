//! 模型网关用的密钥：Anthropic、OpenAI、DeepSeek 沿用以前的（排版 Agent、概念研究、事实调查各存的那一把，一家一把、几处共用），
//! 1.1.0 新加 Google 一把（Gemini 的 API Key）。Vertex AI 不用密钥，用 gcloud 的 ADC（`vertex`）。
//! 存法照 `agent::secret`：正式版钥匙串，开发版、测试在内存或 `PITH_API_KEY_FILE` 的文件里。

use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

use super::Provider;
use crate::AppState;
use crate::agent::AgentState;
use crate::agent::secret::{ApiKey, Secret};
use crate::research::commands::{ResearchState, key_saved_setting, secret_of};

static GOOGLE: OnceLock<Secret> = OnceLock::new();

/// 「填过没有」记在设置里（正式版不读钥匙串就能显示）
pub const GOOGLE_SAVED: &str = "model.key_saved.google";

pub fn google() -> &'static Secret {
    GOOGLE.get_or_init(|| Secret::for_key(ApiKey::Google, crate::agent::official()))
}

/// 这一家用哪把密钥（Vertex 没有）
pub fn key_of(p: Provider) -> Option<ApiKey> {
    match p {
        Provider::Anthropic => Some(ApiKey::Anthropic),
        Provider::Openai => Some(ApiKey::Openai),
        Provider::Deepseek => Some(ApiKey::Deepseek),
        Provider::Google => Some(ApiKey::Google),
        Provider::Vertex => None,
    }
}

pub fn secret<'a>(app: &'a AppHandle, which: ApiKey) -> &'a Secret {
    // 几把密钥都活在 Tauri 管着的状态里（排版 Agent、事实调查各一份，概念研究、Google 在进程级的静态变量里）
    let agent: tauri::State<'a, AgentState> = app.state();
    let research: tauri::State<'a, ResearchState> = app.state();
    secret_of(which, agent.inner(), research.inner())
}

/// 填过没有（不读钥匙串）
pub fn present(app: &AppHandle, which: ApiKey) -> bool {
    let state: tauri::State<AppState> = app.state();
    let saved = crate::agent::setting(&state, key_saved_setting(which)).as_deref() == Some("1");
    secret(app, which).known_present(saved)
}

pub fn load(app: &AppHandle, which: ApiKey) -> Result<String, String> {
    secret(app, which).load()?.ok_or_else(|| format!("还没有填 {} 的 API 密钥。到「设置 › 模型与费用」填写后再试。", which.label()))
}

pub fn save(app: &AppHandle, which: ApiKey, key: &str) -> Result<(), String> {
    secret(app, which).save(key)?;
    let state: tauri::State<AppState> = app.state();
    state.with_db(|c| crate::metadata::put_setting(c, key_saved_setting(which), Some("1")))
}

pub fn clear(app: &AppHandle, which: ApiKey) -> Result<(), String> {
    secret(app, which).clear()?;
    let state: tauri::State<AppState> = app.state();
    state.with_db(|c| crate::metadata::put_setting(c, key_saved_setting(which), None))
}
