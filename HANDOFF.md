# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：视觉期 V1 开工，三个 worktree 建好了，等三家做（2026-11-17）

计划（11-16 定，`decisions/0027`）：

- 视觉先做，v0.5 延后。V1 在 Atelier 里出方案，就是它 v1.0 的 Phase 5；V2 接进 Pith，做完升 0.4.2。
- App 用方案 `pith`；WonderLab 每种图形语言一套 `pith-wonder-<名字>`。用别的项目的设计系统就复制一份，原来的不动。
- 正式版 11-16 装了 0.4.1。第五轮问卷留在云端，快照在 `docs/questionnaire/round-5-answers-partial.md`。

### 本轮（11-17）

1. **简报同步进文档**：你在 Atelier 验收问卷第 1 页填的答案，记进了 Pith 的 `decisions/0027` 末尾、`docs/requirements.md`「视觉」、`ROADMAP.md`。要点：
   - Pith 是你的数字花园，沉淀通俗、真实、可视化的知识资产；
   - 多淡雅、简约、灵动，少科技感、学术、机械；
   - 参考 Maggie Appleton 的 Digital Garden；
   - 动效是亮点；
   - 交 Design System、一个动效、标志、一屏；
   - 明暗都要。
2. **Atelier 跟着改**：
   - `phase5-brief.md` 加了「一个动效」要交什么：`motion/<id>/` 零件，在 Motion Deck 里能播、能导出 GIF，`X1-Screen` 里用上。还写了参考网站只取感觉、不抄。
   - `pith-app.md`：「Pith 是什么」先写你自己的话，气质照第 1 页，删了原来的「不能花哨」。
   - `ROADMAP.md`、`CHANGELOG.md` 记了开工。
3. **在 Atelier 本地提交了一次**：`a415954 atelier 0.13.0: pith brief, phase 5 start`。只有上面那几个文件，加上你的 `db.json`。worktree 要从带简报的 `main` 起，所以先提交；没有推送。
4. **建了三个 worktree**，都从 `a415954` 起，每个里面都有 `db.json` 和 `pith-app.md`：
   - `~/Documents/GitHub/visual-atelier-claude`，分支 `agent/claude/pith`
   - `~/Documents/GitHub/visual-atelier-codex`，分支 `agent/codex/pith`
   - `~/Documents/GitHub/visual-atelier-argon`，分支 `agent/argon/pith`

## 等你

1. **开三个会话**（问卷第 2 页第 3 步）：
   - Claude Code 打开 `visual-atelier-claude`，Codex 打开 `visual-atelier-codex`，Antigravity 打开 `visual-atelier-argon`；
   - 各发一句：「你是 Claude（或 Codex、Argon）。请照 `canvases/v1-phase56-questionnaire/phase5-brief.md` 做，做完照第 3 步交付。」
   - 都交了以后，回到这里说「Phase 5 交了」。
2. **推送 Atelier**：Atelier 的 push 归你，`a415954` 还在本地。
3. **打开一次正式版 0.4.1**（11-16 装的，还没打开过）：
   - 第一次打开先备份，再把数据库升到 v9；
   - 「设置 › Word 与引文」点一次「更新」；
   - 两本《国富论》各点一次「重新解析」。
4. **v0.5 开工时**：接着填第五轮问卷（https://claude.ai/artifact/Xau2yf4Mpcu7RHYkytuKnE ）。

## 已知的、没做的

- 正式版没在真实窗口里点过：数据库升级、Word 模板更新、重新解析都要你打开以后做。
- 11-15 那四个修复留下的没做的照旧，见 `decisions/0025`：
  - 对齐还没喂向量相似度；
  - 重新解析以后语义索引要重建（约 6 分钟）；
  - PDF 的重新解析没验证；
  - 没花钱重跑真实的反方。
- 组件里写死的颜色还在：状态色 amber、emerald 约 90 处，阅读器标注色在 `src/reader/colors.ts`。现在的界面也几乎没有动效。这两样 V2 都要接上。

## 下一轮

- **V2 的准备，不用等方案**：
  - 把写死的状态色收成语义变量（`--success`、`--warning`，四类句子的记号色）；
  - 标注色和 Zotero 一致，不改值，只集中起来；
  - 用 `?mock` 预览看明暗两种都不变样。
- **三家交了以后**：
  - 在 Atelier 主工作区只读地看三条分支（`git log main..agent/<x>/pith`），并排评估。做法照 Atelier 的 `canvases/v1-phase56-questionnaire/README.md`。
  - 你挑定以后，按 V2 接进 Pith：同步进 `src/vendor/pith/`、改 `src/index.css`、接动效、换 App 图标、升 0.4.2。
- **用的时候发现 Atelier 哪里不顺**，照规则 9 去 Atelier 里改。

## 本轮改动

- Pith：
  - `docs/decisions/0027-visual-first-with-atelier.md`（加了简报一节）
  - `docs/requirements.md`、`ROADMAP.md`、本文件
- Atelier（`a415954`）：
  - `CHANGELOG.md`、`ROADMAP.md`
  - `canvases/v1-phase56-questionnaire/{phase5-brief.md, pith-app.md, db.json}`
- 三个 worktree 和三条分支 `agent/<claude|codex|argon>/pith`。
- 代码没改，没加依赖。

## 测试

- `pnpm check` 全过：`pith` 281 通过、25 项默认忽略，前端测试 29 + 17 通过。
- Atelier：只改了说明和问卷答案，没跑 `npm test`；三个 worktree 里都核对过分支名、起点提交，以及 `db.json`、`pith-app.md` 在不在。
