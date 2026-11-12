//! 事实调查（v0.3 Fact Research，`decisions/0020`）。流程、核对、计算在 `research-engine`（纯逻辑）；
//! 这里是它用到的真实接口和 App 这一侧的事：主库存取、快照、下载与取正文、模型与搜索接口、记账、导出、加入文献库、命令。

pub mod store;
