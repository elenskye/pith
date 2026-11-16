# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：改了计划，视觉先做、v0.5 延后；正式版装了 0.4.1（2026-11-16）

### 本轮

1. **正式版装了 0.4.1**：11-15 修的四个问题（语义检索、反方读全文、全书对齐、重新解析）都在里面。版本号从 0.4.0 改成 0.4.1，跟 0.1.1 一样用补丁号。`pnpm app:install` 打包、签名、换掉了「应用程序」里的旧版。我没打开它，因为一打开就会读写 `~/Pith/`（规则 4）。
2. **计划改了**（`decisions/0027`，`0005` 加了一行）：
   - 视觉方案现在做，就是 Atelier v1.0 的 Phase 5。Claude、Codex、Argon 各做一版，你挑一版。
   - v0.5 延后，内容不变。
   - 两套设计系统：App 用 `pith`；WonderLab 每种图形语言一套 `pith-wonder-<名字>`。用别的项目的设计系统就复制一份另起名字，例：「Pith Wonder - Pomelo」，原来那份不动。
   - 视觉期分两步：V1 在 Atelier 里出方案，V2 接进 Pith。V2 做完升 0.4.2。
3. **第五轮问卷留着**：云端页没动，答案还在页面里（前 3 页填了一部分，没提交）。快照在 `docs/questionnaire/round-5-answers-partial.md`。
4. **文档更新、精简**：
   - Pith 这边：
     - `CLAUDE.md` 加了规则 9（和 Visual Atelier 联动）。规则 4 不再把 Atelier 当只读。删了重复的一条（规则 3 和规则 8 都写了问卷）和过时的说法。
     - `ROADMAP.md`「现在」换成视觉期，「已完成」压短了，v0.5 挪进「以后」。
     - `README.md`、`docs/requirements.md`、`HANDOFF-cloud.md` 跟着改。
   - Atelier 这边（照它的规矩，没提交）：
     - 新文件 `canvases/v1-phase56-questionnaire/pith-app.md`，给三家看的 Pith 背景：App 是什么、现在的界面、接进 Pith 要对上的变量名、三家都画「文献库」那一屏。
     - `phase5-brief.md` 让三家先读 `pith-app.md`，颜色角色要对得上那几个变量。
     - 验收问卷第 1 页：开头写清这一轮只做 App；「主要用在哪」换成 Pith 的三处。
     - `CLAUDE.md`「几个会话怎么分」加了 Pith 的会话。
     - `ROADMAP.md` 加了方案一行、「和 Pith 联动」的决定、授权记录。
     - `README.md`、`docs/adoption.md`、`CHANGELOG.md` 跟着改。

## 等你

1. **打开一次正式版**：第一次打开会先备份，再把数据库升到 v9。打开以后：
   - 到「设置 › Word 与引文」点一次「更新」。
   - 给两本《国富论》各点一次「重新解析」，才用上新的归类和全书对齐（怎么看在 `ROADMAP.md` 的验收清单）。
2. **提交 Atelier**：Atelier 的 `CLAUDE.md` 写着 commit 由你来做，所以我没提交。三家的 worktree 要从 `main` 建，得先提交。只加我改的文件：
   ```bash
   git -C ~/Documents/GitHub/visual-atelier add CHANGELOG.md CLAUDE.md README.md ROADMAP.md docs/adoption.md canvases/v1-phase56-questionnaire/README.md canvases/v1-phase56-questionnaire/phase5-brief.md canvases/v1-phase56-questionnaire/survey.html canvases/v1-phase56-questionnaire/pith-app.md
   ```
   `canvases/peck-mcp-pomelo/` 那两个改动不是我的，是 Peck 那边的。你要是想让我以后自己提交 Atelier 的部分，说一声，我写进它的 `CLAUDE.md`。
3. **开工 V1**：
   - 在本地 bench 打开 http://127.0.0.1:4321/#/v1-phase56-questionnaire ，填第 1 页（pith 的简报）并提交，连同 `db.json` 一起提交。
   - 再照第 2 页建三个 worktree、开三个会话，让它们读 `phase5-brief.md`。
4. **v0.5 开工时**：接着填第五轮问卷（同一个链接 https://claude.ai/artifact/Xau2yf4Mpcu7RHYkytuKnE ）。《全球通史》的材料，问卷 E0 选了它才要。

## 已知的、没做的

- 正式版装好了，但没在真实窗口里点过：数据库升级、Word 模板更新、重新解析都要你打开以后做。
- 11-15 那四个修复留下的没做的照旧，见 `decisions/0025`：
  - 对齐还没喂向量相似度。
  - 重新解析以后语义索引要重建（约 6 分钟）。
  - PDF 的重新解析没验证。
  - 没花钱重跑真实的反方。
- 组件里写死的颜色还在：状态色 amber、emerald 约 90 处，阅读器标注色在 `src/reader/colors.ts`。V2 会收进变量。

## 下一轮

- **V2 的准备，不用等方案**：把写死的状态色收成语义变量（`--success`、`--warning`，四类句子的记号色）。标注色和 Zotero 一致，不改值，只集中起来。用 `?mock` 预览看明暗两种都不变样。
- **V1 交了以后**：
  - 在 Atelier 主工作区只读地看三条分支，并排评估（照 Atelier 的 `canvases/v1-phase56-questionnaire/README.md`）。
  - 你挑定以后，按 V2 接进 Pith：同步进 `src/vendor/pith/`，改 `src/index.css`，换 App 图标，升 0.4.2。
- **用的时候发现 Atelier 哪里不顺**，就照规则 9 去 Atelier 里改。

## 本轮改动

- Pith：
  - 版本号：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`Cargo.lock`。
  - 新文件：`docs/decisions/0027-visual-first-with-atelier.md`、`docs/questionnaire/round-5-answers-partial.md`。
  - 改了：`CLAUDE.md`、`ROADMAP.md`、`README.md`、`HANDOFF-cloud.md`、`docs/requirements.md`、`docs/decisions/0005-version-order.md`、本文件。
- Atelier（没提交）：上面「等你」第 2 条列的 9 个文件。
- 没加第三方依赖，代码没改。

## 测试

- `pnpm app:install`：打包成功，签名验证通过，「应用程序」里的 Pith 是 0.4.1。
- `pnpm check` 全过：`pith` 281 通过、25 项默认忽略；其余各包全过；前端类型检查通过，前端测试 29 + 17 通过。
- Atelier：`dw check v1-phase56-questionnaire` 全过，截图看过第 1 页。没跑 `npm test`、`npm run all`：bench 和 tooling 没改。
