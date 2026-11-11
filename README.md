# Pith

（开发名 Research Lab，`decisions/0014`）

本地优先、个人所有、以证据为核心的 macOS 研究工作台。Tauri 2 + React + TypeScript + Rust + SQLite。

它要解决三类问题：

1. **毕业论文**：Zotero 文献管理 + Word 中英文混排 / 引用格式反复调整效率低
2. **日常调查**：现成 Deep Research 漏来源、报告冗长、事实与推测混在一起
3. **阅读学习**：读书摘抄难以沉淀为有出处、可复习的知识作品

顺序：先做能完全替代 Zotero 的 Library（v0.1），再做 Word 引用与排版（v0.2），再做三种研究模式。

## 当前状态

v0.1 已验收（2026-11-10）；v0.2 Word Studio 进行中（当前 0.2.0；W1 Word 入口、W2 引用已完成：Word 顶栏「Pith」选项卡里用 Pith 的搜索窗插入 / 编辑 / 删除引文、插入书目、刷新、切换样式（含导入的 .csl）、把 Zotero 引文转换成 Pith 引文，`ROADMAP.md`）。v0.1 能做的：导入（Zotero 副本、BibTeX / RIS / CSL JSON）、搜索、编辑、集合与标签、批量编辑、去重合并、回收站、PDF 阅读与高亮摘录、DOI / ISBN 联网补全、备份恢复与整库导出都能用。交接见 `HANDOFF.md`，计划见 `ROADMAP.md`。

## 运行

```bash
pnpm install
```

三种版本（同一份代码，做法照 Pomelo）：

| | 命令 | 数据 | 模型与密钥 |
| --- | --- | --- | --- |
| 开发版 | `pnpm app:dev` | `~/Pith Dev/` | 模拟模型，不联网、不碰钥匙串 |
| 打包版 | `pnpm app:build`，再 `open --env PITH_DATA_DIR=<临时目录> target/release/bundle/macos/Pith.app` | 你指定的目录 | 同上（测试用） |
| 正式版 | `pnpm app:install` | `~/Pith/` | 真模型，密钥在钥匙串 |

```bash
pnpm app:dev
```

```bash
pnpm app:install
```

- **正式版**装在「应用程序」里，只用这一个（把它拖进程序坞）。`pnpm app:install` 打包、用固定的自签名证书「Pith Self-Signed」签名、换掉「应用程序」里的旧版；新版第一次打开会先备份、再升级数据库。装完到 设置 › Word 点一次「更新」（脚本里记着 Pith 的位置）。
- 编译输出在 `target/`，可以随便清：`pnpm clean:build`；超过 20 GB 时 `pnpm check` / `app:dev` / `app:build` / `app:install` 会自动清。清掉不影响「应用程序」里的正式版。
- 证书只要建一次：钥匙串访问 › 证书助理 › 创建证书，名称 `Pith Self-Signed`、自签名根证书、证书类型「代码签名」。

只看界面（浏览器，不启动 Rust）：`pnpm dev` 后打开 http://localhost:1430/?mock

检查（每轮必过）：

```bash
pnpm check
```

## 文档

| 文件 | 用途 |
| --- | --- |
| `CLAUDE.md` | Agent 工作规则（必读） |
| `HANDOFF.md` | 最近一轮交接 |
| `ROADMAP.md` | 现在 / 下一步 / 以后 / 已完成 |
| `docs/requirements.md` | 已定需求 |
| `docs/architecture.md` | 目录、规则、当前命令 |
| `docs/schema.md` | v0.1 数据库结构 |
| `docs/decisions/` | 决策记录（一事一文） |
| `docs/feasibility.md` | Phase 0 验证记录与复现命令 |
| `docs/testing.md` | 测试约定 |
| `docs/zotero-mapping.md` | Zotero 表 → 本项目模型的映射 |
| `docs/zotero-copy-guide.md` | 导出 Zotero 库副本的步骤 |
| `docs/open-source-evaluation.md` | 开源组件评估 |
| `docs/brief-digest.md` | 原始 Brief 压缩版 |
| `docs/questionnaire/` | 需求问卷与回答 |
| `THIRD_PARTY.md` | 第三方依赖与许可证 |
| `crates/docx-engine/README.md` | Word 文档引擎（排版器、校验器、引文、VBA / 模板生成）的命令 |
| `poc/word-addin/README.md` | Phase 0-B 原型（Office.js 路线保留待用） |

## 本机环境（2026-11-08）

macOS 27.2，Apple M3 Pro，18 GB；Microsoft Word 16.114（个人账号）；Zotero 9.0.3（`~/Zotero` 为真实库，只读、不触碰）；Node 24.16、pnpm 12.6、Rust 1.99。
