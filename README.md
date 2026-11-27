# Pith

站主自己的 macOS 研究工作台：本地优先，数据归自己。打开就是对话；知识库是文献库和对话之间的一层；读过的书、做完的调查、对话里的好回答都种进知识库，回答时带编号、点开是原文。Tauri 2 + React + TypeScript + Rust + SQLite。（开发名 Research Lab，`docs/decisions/0014`。）

## 现在

- **1.0.2**（2026-11-25）：界面照 Visual Atelier 的 Explore R1 做好了——四个入口（对话、知识库、文献库、设置），事实调查、概念研究、排版在对话里用 `/` 点名。对话、角色、知识库已经真存，回答还是演示的（不调模型），1.1.0 接上 Agent 循环（`docs/decisions/0030`、`0031`）。
- **1.0.4**（2026-11-25）：换配色的整页水面回来了（App 截一张旧样子交给 GPU 扭，不卡）；集合展开长出小枝桠；衬线字在按钮里居中（`docs/decisions/0033`）。
- **1.0.3**（2026-11-25）：界面优化——下拉框、勾选框换成 Pith 的样子，文献库两栏（集合一栏、标签树两指往右滑），导入挪到 设置 › 导入，换配色不卡（`docs/decisions/0032`）。
- 以前做好的照常能用：文献库（Zotero 导入、阅读与标注、联网补全）、Word 里的引用和排版、事实调查、概念研究。
- 现在做到哪、下一步：`HANDOFF.md`；版本计划：`ROADMAP.md`。

## 运行

```bash
pnpm install
```

| | 命令 | 数据 | 模型与密钥 |
| --- | --- | --- | --- |
| 开发版 | `pnpm app:dev` | `~/Pith Dev/` | 模拟接口，不联网、不碰钥匙串 |
| 打包版 | `pnpm app:build`，再 `open --env PITH_DATA_DIR=<临时目录> target/release/bundle/macos/Pith.app` | 你指定的目录 | 同上（测试用） |
| 正式版 | `pnpm app:install` | `~/Pith/` | 真接口，密钥在钥匙串 |

```bash
pnpm app:dev
```

```bash
pnpm app:install
```

- 正式版装在「应用程序」里，用固定的自签名证书「Pith Self-Signed」签名（钥匙串访问 › 证书助理 › 创建证书：自签名根证书、代码签名，建一次）。新版第一次打开会先备份、再升级数据库；装完到 设置 › Word 与引文 点一次「更新」。
- 只看界面（浏览器，不启动 Rust）：`pnpm dev`，打开 http://localhost:1430/?mock 。
- 编译输出在 `target/`，`pnpm clean:build` 随便清；超过 20 GB 时几条命令会自动清。
- App 图标从同步来的方案生成：`pnpm app:icon`。

每轮必过的检查：

```bash
pnpm check
```

## 文档分工

一件事只写一处，别处指过去。

| 文件 | 写什么 | 谁看 |
| --- | --- | --- |
| `CLAUDE.md`（`AGENTS.md` 指向它） | Agent 的工作规则：开工前读什么、硬边界、工程约定、版本与 Git、和 Atelier 联动 | Agent，每次开工必读 |
| `HANDOFF.md` | 只写现在：上一轮做了什么、测试、等用户的、下一轮从哪开始 | 用户、接手的 Agent |
| `ROADMAP.md` | 版本计划（现在、做完的、到 2.0.0）、用户验收清单 | 用户、Agent |
| `docs/requirements.md` | 拍过板、仍然管用的需求 | 动功能前 |
| `docs/decisions/` | 为什么这么定，一事一文；`README.md` 是索引，标了哪些现行、哪些被取代 | 查理由 |
| `docs/architecture.md` | 代码怎么分、工程规矩、各模块的流程 | 动代码前 |
| `docs/schema.md` | 数据库表 | 动表前 |
| `docs/testing.md` | 测试约定、各模块测什么、真实验收的命令 | 写测试、做验收 |
| `docs/zotero-mapping.md`、`docs/zotero-copy-guide.md` | Zotero 字段怎么对过来；怎么给 Pith 一份 Zotero 副本 | 动导入时、要用户给副本时 |
| `docs/questionnaire/` | 规划问卷的源文件（答案读回后写进 ROADMAP 和决策） | 出问卷时 |
| `THIRD_PARTY.md` | 第三方依赖与许可证 | 加依赖时 |
| `src/vendor/pith/DESIGN.md` | 视觉基线（同步来的，不手改） | 动界面前 |
| `crates/*/README.md` | 各引擎的模块表和格式 | 动引擎时 |

## 本机环境

macOS 27.2，Apple M3 Pro，18 GB；Microsoft Word 16.114；Zotero 9.0.3（`~/Zotero` 是真实库，只读、不碰）；Node 24、pnpm 12、Rust 1.99。
