//! Pith 的事实调查引擎（v0.3，`decisions/0020`）。纯逻辑：不联网、不碰数据库、不依赖 Tauri 和 macOS。
//!
//! 模型只负责「读」和「写」：拆问题、从正文里摘事实、把核实过的事实写成结论。
//! 其余都由这里的代码做，并且能重做一遍来核对（`audit`）：
//!
//! - `fold` / `numbers` / `dec`：文字折叠、认数、精确小数
//! - `verify`：摘录必须在保存的正文里逐字找得到，数值和单位必须在摘录里
//! - `trust`：按数据生产者分级的可信度等级表
//! - `calc`：差、比、同比、平均……由代码算
//! - `conclude`：结论里的每个数都要对得上一条已核实的事实或一个算出来的值
//! - `prompts`：三段提示词（计划、摘证据、结论）和各自输出的解析
//! - `flow`：一次调查的流程（计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告），经 `ports` 的接口调用模型、搜索和下载
//! - `diff`：同一问题两次调查的对比
//! - `report` / `render`：报告的内容，Markdown 与单文件 HTML
//! - `audit`：对一份存下来的调查从头再核一遍
//! - `fixture`：合成夹具（虚构的「示例市」）和模拟的模型、搜索、下载

pub mod dec;
pub mod fold;
pub mod numbers;
pub mod calc;
pub mod conclude;
pub mod model;
pub mod trust;
pub mod verify;
