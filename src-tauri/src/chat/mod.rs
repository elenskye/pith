//! 对话、角色、知识库（1.0.2，`decisions/0030`）。
//!
//! - `store`：存取（migration 0010）。
//! - `run`：一回回答（1.1.0 起是 Agent 循环，`harness`）：后台跑、界面轮询，跑完存成一条消息。
//! - `commands`：Tauri 命令（`chat_*`、`role_*`、`kb_*`）。

pub mod commands;
pub mod run;
pub mod store;

#[cfg(test)]
pub(crate) mod tests;
