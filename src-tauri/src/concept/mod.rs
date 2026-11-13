//! 概念研究（v0.4 Concept Research，`decisions/0024`）。文本层、技能与工作流、核对、关系图、渲染在 `concept-engine`（纯逻辑）；
//! 这里是 App 这一侧：主库存取、段落检索、OpenAI Responses 与网页搜索、Mathpix、文献库接线、密钥、命令、Word 导出。

pub mod commands;
pub mod fixtures;
pub mod keys;
pub mod library;
pub mod mathpix;
pub mod responses;
pub mod search;
pub mod store;
#[cfg(test)]
pub(crate) mod tests;
