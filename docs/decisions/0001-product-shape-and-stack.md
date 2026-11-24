# 0001 · 产品形态与技术栈

- 日期：2026-11-07
- 状态：已定（问卷 Q6 / Q19 / Q20 + Pomelo 调研）
- 依据：`docs/questionnaire/round-1-answers.md`

## 决定

| 层 | 选择 | 理由 |
| --- | --- | --- |
| 形态 | macOS 桌面 App，Tauri 2 + React 19 + TypeScript | 用户首选；与 Pomelo 同栈，经验可复用 |
| 核心数据层 | Rust（`src-tauri/`），只有 Rust 读写 SQLite | 用户选 Rust；沿用 Pomelo 「业务规则只在 Rust」 |
| 数据库 | SQLite（rusqlite，bundled + backup 特性）+ FTS5 | Brief 首选；本地单用户足够 |
| 前端样式 | Tailwind v4 + CSS 变量（shadcn 命名），不引入组件库，图标 Lucide | 沿用 Pomelo；v0.1 只求简约 |
| PDF | pdf.js（内置阅读器），高亮锚点存 DB | 用户 Q14 明确要内置阅读器 |
| 包管理 | pnpm（前端）、cargo（Rust）；Tauri CLI 作为 devDependency 安装，不装全局 | 本机已有 pnpm / cargo |
| 布局 | 三栏：分类 / 文献列表 / 详情-阅读 | Brief 第 4 节；文献场景与 Pomelo 的侧栏+单区不同 |

## 不做

- 不做 Electron / Swift 原生 / Web 版。
- 不做 monorepo 拆包；v0.1 单个 Tauri 项目，目录 `src/`（前端）+ `src-tauri/`（Rust）+ `docs/` + `tests/fixtures/`。
- v0.1 不引入 Python Worker；PDF 解析若 Rust 生态不够，v0.1.x 再评估。

## 后果

- Rust 开发慢于 TS，v0.1 时间「不限、做扎实」可承受。
- 与 Pomelo 共用约定但**独立代码、独立数据库、独立配置**，不共享代码。
