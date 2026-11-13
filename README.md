# Pith

（开发名 Research Lab，`decisions/0014`）

本地优先、个人所有、以证据为核心的 macOS 研究工作台。Tauri 2 + React + TypeScript + Rust + SQLite。

它要解决三类问题：

1. **毕业论文**：Zotero 文献管理 + Word 中英文混排 / 引用格式反复调整效率低
2. **日常调查**：现成 Deep Research 漏来源、报告冗长、事实与推测混在一起
3. **阅读学习**：读书摘抄难以沉淀为有出处、可复习的知识作品

顺序：Library（v0.1）→ Word 引用与排版（v0.2）→ 事实调查（v0.3）→ 概念研究（v0.4）→ WonderLab（v0.5）→ 学术研究（v0.6）→ Adaptive Agent（v0.7），见 `docs/decisions/0005`。

## 当前状态

- 0.4.0，正在做 v0.4 Concept Research（读书：概念卡、论证卡、关系图，`docs/decisions/0024`）。
- 已经能用：v0.1 文献库（Zotero 迁移、搜索管理、PDF 阅读标注、联网补全、备份恢复）、v0.2 Word（顶栏「Pith」选项卡里的引用、排版 Agent）、v0.3 事实调查。
- 交接看 `HANDOFF.md`，计划看 `ROADMAP.md`。

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

常用：

| 文件 | 用途 |
| --- | --- |
| `CLAUDE.md` | Agent 工作规则（必读） |
| `HANDOFF.md` / `HANDOFF-cloud.md` | 最近一轮交接 / 给云端 Claude 的任务与它的记录 |
| `ROADMAP.md` | 现在 / 已完成 / 以后 |
| `docs/requirements.md` | 已定需求 |
| `docs/decisions/` | 决策记录（一事一文，`0024` 是 v0.4 计划） |
| `docs/architecture.md` | 目录、规则、各模块的流程 |
| `docs/schema.md` | 数据库结构 |
| `docs/testing.md` | 测试约定与真实验收命令 |
| `THIRD_PARTY.md` | 第三方依赖与许可证 |

参考（做完的阶段留下的）：`docs/brief-digest.md`（原始 Brief 压缩版）、`docs/questionnaire/`（三轮问卷与回答）、`docs/feasibility.md`（Phase 0 验证）、`docs/open-source-evaluation.md`、`docs/zotero-mapping.md`、`docs/zotero-copy-guide.md`、`crates/*/README.md`、`poc/word-addin/README.md`。

## 本机环境（2026-11-08）

macOS 27.2，Apple M3 Pro，18 GB；Microsoft Word 16.114（个人账号）；Zotero 9.0.3（`~/Zotero` 为真实库，只读、不触碰）；Node 24.16、pnpm 12.6、Rust 1.99。
