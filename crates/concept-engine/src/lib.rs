//! Pith 的概念研究引擎（v0.4，`decisions/0024`）。纯逻辑：不联网、不碰数据库、不依赖 Tauri 和 macOS。
//!
//! - 文本层：`markup`（容错的 XHTML 读取）、`epub`（容器、目录、页码表、块）、`text`（单元、角色、段落号、页码、结构预览）、
//!   `mathpix`（Mathpix 的 lines.json → 段落）、`align`（中英对齐）
//! - 技能与工作流：`skill`（SKILL.md）、`model`（一次研究的数据）、`prompts`（提示词、输出的 JSON Schema 与解析）、`flow`（按步骤跑、存、取消、接着做）
//! - 核对：`fold`（概念研究的折叠）、`verify`（每句的类别与锚点）、`audit`（从存下的文本重新核对）
//! - 产出：`graph`（关系图布局与 SVG）、`render`（Markdown、单文件 HTML、给 Word 的 `Doc`）
//! - `fixture`：夹具（公有领域英文本 + 合成中文）和录制的模型回复
//!
//! 模型、搜索、下载的接口沿用 `research_engine::ports`。

pub mod markup;
pub mod epub;
pub mod text;
pub mod align;
pub mod fold;
pub mod mathpix;
pub mod fixture;
pub mod skill;
pub mod model;
pub mod prompts;
pub mod verify;
pub mod audit;
pub mod flow;
pub mod mock;
pub mod graph;
pub mod render;
