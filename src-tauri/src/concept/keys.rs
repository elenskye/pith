//! 概念研究的两把新密钥（`decisions/0024`「密钥」）：OpenAI、Mathpix（`app_id:app_key` 一对存在一个条目里）。
//! 和别的密钥一样用 `Secret::for_key`：正式版存钥匙串，开发版、测试在内存或 `PITH_API_KEY_FILE` 的文件里。
//! 放在进程级的静态变量里，事实调查的「密钥与费用」命令（`research_key_set` 等）也能找到它们。

use std::sync::OnceLock;

use crate::agent::secret::{ApiKey, Secret};

static OPENAI: OnceLock<Secret> = OnceLock::new();
static MATHPIX: OnceLock<Secret> = OnceLock::new();

pub fn secret(which: ApiKey) -> &'static Secret {
    let cell = if which == ApiKey::Mathpix { &MATHPIX } else { &OPENAI };
    cell.get_or_init(|| Secret::for_key(if which == ApiKey::Mathpix { ApiKey::Mathpix } else { ApiKey::Openai }, crate::agent::official()))
}

/// 「填过没有」记在设置里（正式版不读钥匙串就能显示）
pub fn saved_setting(which: ApiKey) -> &'static str {
    if which == ApiKey::Mathpix { "concept.key_saved.mathpix" } else { "concept.key_saved.openai" }
}

/// 界面上分两个框填的 Mathpix：拼成一个条目
pub fn mathpix_pair(app_id: &str, app_key: &str) -> String {
    format!("{}:{}", app_id.trim(), app_key.trim())
}
