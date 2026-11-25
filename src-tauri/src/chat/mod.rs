//! 对话、角色、知识库（1.0.2，`decisions/0030`）。
//!
//! - `store`：存取（migration 0010）。
//! - `mock`：模拟的回答，1.1.0 由 Agent 循环换掉。
//! - `commands`：Tauri 命令（`chat_*`、`role_*`、`kb_*`）。

pub mod commands;
pub mod mock;
pub mod store;

#[cfg(test)]
pub(crate) mod tests;
