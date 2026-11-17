# ROADMAP

> 现在 → 已完成 → 以后 → v0.7 之后：用户验收清单 → 规矩。版本顺序见 `docs/decisions/0005`。
> v0.7 之前每期「做完」= 功能走通 + 自动测试 + Agent 自己的真实验收记录；用户验收、打磨功能在 v0.7 之后（`decisions/0026`，`CLAUDE.md` 规则 8）。视觉方案现在做，由用户在 Visual Atelier 里挑（`0027`，规则 9）。

## 现在：视觉方案（2026-11-16 起，排在 v0.4 和 v0.5 之间）

用户 11-16 定：Pith 的视觉方案提前，v0.5 延后（`decisions/0027`）。方案在 Visual Atelier 里做，正好是 Atelier v1.0 的 Phase 5。

| 步 | 在哪做 | 做什么 | 状态 |
| --- | --- | --- | --- |
| V1 出方案 | `../visual-atelier` | 用户填 Atelier 验收问卷第 1 页（pith 的简报）→ Claude、Codex、Argon 各在自己的分支上做一版 `schemes/pith/` 和 `canvases/pith-explore/` → Atelier 的会话并排评估 → 用户挑一版，记 `0.1.0`，打 `pith-v0.1.0` | 简报填了，三个 worktree 建好了（11-17）；等用户开三个会话、三家交 |
| V2 接进 Pith | 本仓库 | 同步进 `src/vendor/pith/`；`src/index.css` 改用方案的变量（仍用 shadcn 命名）；组件里写死的状态色、标注色收进变量；接上方案的动效；换 App 图标；做完升 0.4.2 | 等 V1。写死的颜色可以先收成语义变量，不等方案 |

- 三家的说明在 Atelier 的 `canvases/v1-phase56-questionnaire/phase5-brief.md`，Pith 的背景在同目录的 `pith-app.md`，简报摘要在 `decisions/0027` 末尾：数字花园；淡雅、简约、灵动；动效是亮点。
- 三个 worktree：`../visual-atelier-claude`、`-codex`、`-argon`，分支 `agent/<claude|codex|argon>/pith`。
- 这一轮只做 App 的方案。WonderLab 的设计系统（`pith-wonder-*`）在 v0.5 做 WonderLab 时起（`0027`）。

## 已完成

- **v0.4 的四个已知问题**（2026-11-15，正式版 0.4.1 于 11-16 装上）：语义检索、反方读全文、全书中英对齐、重新解析后重找锚点（`0025`「2026-11-15：修掉四个已知问题」）。
- **v0.4 Concept Research**（2026-11-13，0.4.0）：EPUB / PDF（Mathpix）解析成带段落号、页码的文本，中英按段对齐。技能与工作流，四种入口。原文 / 他人 / 解释 / 推演四类句子，原文逐字核对。关系图、反方、推演、三种导出，Word 里插入卡片原文。本地真实验收花了 $0.69（`0024`、`0025`）。
- **v0.3 Fact Research**（2026-11-12，用户验收）：计划 → 检索 → 本机下载与快照 → 摘证据 → 代码核对 → 可信度 → 结论 → 报告和导出（`0020`、`0022`、`0023`）。
- **v0.2 Word Studio**（2026-11-10 至 11-12）：Word 选项卡与本机服务、引用（两份真实论文 63/63、64/64）、排版 Agent、结构类排版，在 Word 里验收（`0015`–`0019`、`0021`）。
- **v0.1 Library**（2026-11-08 至 11-10，用户验收）：Zotero 副本导入、搜索与管理、PDF 阅读与标注、DOI / ISBN 补全、备份恢复与整库导出（`0009`–`0014`）。
- **Phase 0、Start Up**（2026-11-07）：可行性验证（`docs/feasibility.md`）、Brief 压缩、问卷、决策 `0001`–`0008`。

v0.1.x 候选（v0.7 之后再排）：PDF 全文搜索、PDF 目录 / 缩略图、标注导出到笔记、笔记编辑器、拖放整理、Zotero 增量导入、撤销系统、BibTeX `file` / RIS `L1` 附件导入、合并式导入导出。

## 以后

| 版本 | 模块 | 届时需用户提供 |
| --- | --- | --- |
| v0.5 | 知识库与对话 Agent（先做）+ WonderLab（`docs/requirements.md`）。V2 做完再开工：先接着填第五轮问卷（云端页 https://claude.ai/artifact/Xau2yf4Mpcu7RHYkytuKnE ，填了前 3 页的一部分，快照在 `docs/questionnaire/round-5-answers-partial.md`），再写计划 | 《全球通史》的 EPUB 和摘抄（问卷 E0 选了它才要） |
| v0.6 | Academic Research，政治效能感课题 | 授权数据集 |
| v0.7 | Adaptive Agent（在 v0.4 的技能格式上做自我改进） | 固定研究样本 |
| v0.7 之后 | 用户真实验收、打磨功能 | — |

## v0.7 之后：用户验收清单

v0.7 之前要用户亲自看、亲自做的事攒在这里，不挡开发（`CLAUDE.md` 规则 8）。每条写清看什么、怎么看。

| 来自 | 看什么 | 怎么看 |
| --- | --- | --- |
| 0.4.1 | 给两本《国富论》各点一次「重新解析」，再看全书的中英对照 | 「概念」页打开书 → 右上「重新解析…」→ 看摘要（应当全部原位、没有没挂上的）；翻几章以前标「未对齐」的（如第一篇第 5、7 章），中英并排对不对；附录会标「对齐存疑」 |
| 0.4.1 | 「问这本书」按意思找段落 | 打开一章 →「问这本书…」→ 等语义索引建好（两本书约 6 分钟，只建一次）→ 用整句中文提问，看找到的段落对不对；「设置 › 概念研究」最下面看本机模型的状态 |
| v0.4 | 抽查 10 张概念卡、论证卡：准确吗、对复习有用吗 | 正式版「概念」页（验收数据已导入）；或 `local/concept-c4/out/` 的 HTML |
| v0.4 | Word 里插入卡片原文 | 正式版「设置 › Word」点「更新」装新模板；Word 里 Add/Edit Citation → 选书 →「卡片里的原文」选一句：英文带「p. 110 (I.i.3)」、中文带段落号；再切换样式、刷新 |
| v0.4 | 正式版里的两把新密钥 | 「设置 › 密钥与费用」填 OpenAI、Mathpix（app_id、app_key），看钥匙串存取正常 |
| v0.2 | W5 排好的终稿 | Word 打开 `local/real-thesis/out/agent-w5/终稿/brief/毕业论文_终稿（Pith 排版）.docx` |
| v0.2 | 原创性声明和使用授权书要不要排在同一页 | 定下来告诉 Agent |

## 规矩

- 只做当前期的事。v0.7 之前每期做完要能走通、有自动测试，并写 Agent 的真实验收记录（`CLAUDE.md` 规则 8）；真实验收在每版的花费上限内直接做，超了才问。
- 三种版本照 Pomelo：开发版 `pnpm app:dev`、打包版 `pnpm app:build`（配 `PITH_DATA_DIR` 测试）、正式版 `pnpm app:install`（装进「应用程序」，用户要求时才装）。
- 版本号：`package.json`、`src-tauri/Cargo.toml`、`tauri.conf.json` 一起改。现在 0.4.1；V2 做完 0.4.2；v0.5 开工时 0.5.0。
- 加新 migration 之前先冻结当前版本的样本库（`docs/testing.md`「升级流程」）。现在是 v9（`0009_alignment.sql`），加 0010 之前先冻结 v9。
- 每轮更新 `HANDOFF.md`；要用户提供资料时写在 HANDOFF「等你」，要用户亲自验收的记进上面的清单。
- 云端和本地并行：云端只推 `cloud/v0.x`，交接写 `HANDOFF-cloud.md`；本地验收通过后直接合并。Git 由本地 Agent 做（`CLAUDE.md` 规则 4、7）。Visual Atelier 里的事照规则 9。
- 前几版留下的已知问题在 v0.7 之前全部做完，尽量交给云端（`decisions/0026`）。
