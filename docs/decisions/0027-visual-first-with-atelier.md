# 0027 · 视觉先做、v0.5 延后；和 Visual Atelier 联动

- 日期：2026-11-16
- 状态：已定（用户在对话里定的）
- 改了：`0005`（视觉原定 v0.7 之后）、`0026`（v0.7 之前视觉只求简约）
- 原定 `0027` 写 v0.5 计划，顺延到 v0.5 开工时的下一个编号。

## 用户定的

1. **视觉方案现在做，v0.5 延后。** Visual Atelier（`../visual-atelier`）v1.0 的 Phase 5 就是做 pith 的视觉方案：Claude、Codex、Argon 各做一版，用户挑一版。
2. **第五轮问卷留着。** 用户填到第 3 页一半；云端页不删，v0.5 开工时接着填。答案快照在 `docs/questionnaire/round-5-answers-partial.md`。
3. **两个项目互相帮**：Atelier 给 Pith 出视觉设计、Design System 和视觉资产；Pith 开发、使用中发现的问题，反过来改 Atelier 的工作流、丰富它的内容。开在 Pith 仓库、把 Atelier 加为第二个目录的会话（用户叫它「pith 环境」）两边都能做。
4. **计划各自独立。** Pith 的计划在本仓库 `ROADMAP.md`，Atelier 的在它的 `ROADMAP.md`。Atelier 做完 v1.0 以后基本没有自己的计划，主要当别的项目的工具，边用边改。
5. **别的项目照常用 Atelier。** Pith 要用别的项目的 Design System，就复制一份另起名字，原来那份不动。例：用 Pomelo 的，复制成「Pith Wonder - Pomelo」。Pith 一定会按自己的需要改 Design System，所以必须分开。
6. **WonderLab 和 App 的设计系统分开。**

## 怎么落实

**两套设计系统**

| 用在哪 | Atelier 里的方案 | Design System 的名字 | 什么时候做 |
| --- | --- | --- | --- |
| Pith App 的界面（主页面、侧栏、设置、导出的报告和卡片、App 图标） | `schemes/pith/` | Pith | 现在（Atelier Phase 5） |
| WonderLab 的作品（2D / 3D，每种图形语言一套） | `schemes/pith-wonder-<名字>/` | Pith Wonder - <名字> | v0.5 做 WonderLab 时 |

- 从别的方案复制来的（Pomelo、Vermilion、Peck……）照例子起名：`pith-wonder-pomelo`、「Pith Wonder - Pomelo」。`DESIGN.md` 第一节写清从哪个方案、哪个版本复制的，以后各改各的。原方案的许可限制照样跟着走，比如 Pomelo 的系统字体不能打包进导出的作品。
- App 和 WonderLab 不共用 token；WonderLab 的几套之间也不共用。
- 在 Atelier 里谁能动哪些文件、Git 谁做，照 Atelier 的 `CLAUDE.md`「几个会话怎么分」（Pith 的会话那一行，2026-11-16 加）。本仓库照旧由本地 Agent 提交、推送 `main`；Atelier 的提交照它自己的规矩。

**视觉期分两步**（排在 v0.4 和 v0.5 之间，不是新的功能版本）

1. **V1 出方案**（在 Atelier 里做，就是它的 Phase 5）：
   - 用户填 Atelier 验收问卷 `v1-phase56-questionnaire` 的第 1 页（pith 的简报），提交，建三个 worktree，开三个会话。
   - Claude、Codex、Argon 各交一版 `schemes/pith/` 和 `canvases/pith-explore/`。Pith 的背景写在 Atelier 的 `canvases/v1-phase56-questionnaire/pith-app.md`：从本仓库复制过去的事实，因为 Atelier 的文档不指向别的仓库。
   - Atelier 的会话把三版并排评估，用户挑一版，`scheme.json` 记 `0.1.0`。
2. **V2 接进 Pith**（本仓库，本地 Agent 做，要在 Mac 上看界面）：
   - 用户打 `pith-v0.1.0` 以后，照 Atelier 的 `docs/adoption.md` 单向同步进 `src/vendor/pith/`，里面有 `VERSION` 和 `SHA256SUMS`，同步来的文件不手改。每次同步要用户在对话里说一声。
   - `src/index.css` 改成用同步来的变量，名字仍是 shadcn 命名（`--background`、`--primary`……）。
   - 组件里写死的颜色收进变量：状态色（`amber-*`、`emerald-*`，约 90 处）、阅读器的标注色（`src/reader/colors.ts`，和 Zotero 一致，要保留）。
   - App 图标照 Pomelo 的做法：用 `tauri icon` 从 1024px 的源图生成。Word 选项卡的图标随方案定。
   - 做完 App 升 0.4.2。v0.5 开工时再升 0.5.0。
- **V2 可以先做的准备**（不等 V1）：把写死的颜色先收成语义变量（`--success`、`--warning`、`--highlight-*` 之类），换方案时只换变量的值。

**v0.5 延后以后**

- 内容不变（知识库与对话 Agent、WonderLab，`docs/requirements.md`）。V2 做完再开工：先接着填第五轮问卷，再写计划。
- 问卷第 3 页 A3、A4 选了「让模型按题材挑」，所以每种图形语言做成一套 `pith-wonder-*`，模型按题材挑。具体有哪几套、放在 App 的什么位置，v0.5 定。

## 没改的

- 真实数据目录 `~/Pith/`、`~/Zotero/` 照旧不碰。
- 界面语言不变：App 中文，Word 英文（`0006`）。
- Pith 不引入组件库，图标仍用 Lucide。方案画的界面小图标另说，到 V2 再定换不换。
- 打磨功能、用户真实验收照旧放在 v0.7 之后（`0026`）。视觉方案用户在 V1 里自己挑，所以视觉不算在「v0.7 之后再看」里。
