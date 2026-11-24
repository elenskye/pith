# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.0.0，换了新的开发准则；等定 1.1.0 开发计划（2026-11-24）

### 新架构（`decisions/0028`）

- **三个入口**：对话、文献库、设置。事实调查、概念研究、排版、Word 润色都做成技能，模型在对话里自己挑、自己调。
- **Agent 循环 Pith 自己写**（Rust）：
  - 接多家 API；
  - 做好 Prompt Cache 前缀命中；
  - 子任务用递归式的轻量 Subagent。
- **花钱只看月上限**：输入框下面一个用量圈，点开看明细；不再逐次确认，也不算每段对话花了多少。
- **组织**：研究主题只给对话分组；知识库另建。成果由模型自动存进知识库（数据）或文献库（元数据），不绑对话。
- **1.1.0 做**（原 v0.5）：对话 + 技能运行时 + 研究主题；事实调查、概念研究、排版改成技能；摘抄与笔记进知识库；Word 润色技能。WonderLab 往后放。
- **设计**：在 Atelier 里做完了，方案 `pith` 0.1.0 已并进 Atelier 的 `main`，还没同步进本仓库；本地照新架构开发。

### 本轮（2026-11-24）：合并 Atelier、换新的开发准则

1. **Atelier**：云端的 `agent/claude/pith`（19 个提交，到 `7c1350f`，Pith 的设计做完了）并进 Atelier `main`。
   - 只有 `CHANGELOG.md` 冲突，两边的条目都留了；`schemes/pith/CLAUDE.md` 里 Pith 的旧路径顺手改成 `pith`。
   - 删了本地三条 `agent/*/pith`（都已合并）和 GitHub 上的 `agent/claude/pith`。Atelier 现在只有 `main`。
2. **Pith**：GitHub 上的 `cloud/v0.3`、`cloud/v0.4` 早就并进 `main` 了，删掉。现在只有 `main`。
3. **新的开发准则**（用户给的「App & Git 开发准则」，加 ASK 里定的三点，`decisions/0029`）：
   - Pith 版本号 0.4.1 → **1.0.0**（`package.json`、`src-tauri/Cargo.toml`、`tauri.conf.json`，`Cargo.lock` 跟着变），功能不变；原 v0.5 / v0.6 / v0.7 改叫 1.1.0 / 1.2.0 / 1.3.0。
   - `CLAUDE.md`：规则 2 开工前先问开发程度；新规则 7「版本与开发程度」、规则 8「Git」（只 commit，push 由用户做）。旧规则 7（云端协作）、8（v0.7 之前的开发方式）删了：真实验收的花费上限挪进规则 4，出问卷的做法和「用户验收清单」挪进规则 3；规则 9 的「现在」改成设计做完了。
   - 删了 `HANDOFF-cloud.md`（里面没有进行中的任务）；`ROADMAP.md`、`README.md`、`docs/requirements.md`、`0026`（标成被 `0029` 取代）跟着改。
   - Atelier：`CLAUDE.md` 加「版本与 Git」，整个仓库一个版本号、从 1.0.0 起（`package.json` 原来 0.13.0）；加了 `.claude/settings.json` 关掉默认署名；`docs/sessions.md`、`ROADMAP.md`「授权记录」、`CHANGELOG.md` 跟着改。
4. **之后**：用户会删掉两个 GitHub 仓库、清掉本地历史重新 `git init`，再在 GitHub 上重建、重新连上。

### 上一轮（2026-11-23）：本地目录改名 `pith`

- `~/Documents/GitHub/personal-research-lab` 改名为 `~/Documents/GitHub/pith`；本仓库和 Atelier 里指向旧路径的地方都改了；`cargo clean -p` 清了五个工作区 crate（测试二进制里写死了旧路径）；Agent 记忆挪到新目录对应的位置。

## 等你

1. **重建 Git**：删掉两个 GitHub 仓库、清本地历史、重新 `git init`、连上新仓库、推送（命令在对话里给了）。
2. **下一步怎么走**：出新一轮问卷（按 `0028`「还没定的」列题），还是直接写 1.1.0 的开发计划。开工前我会先问是哪种程度。
3. **同步 `pith` 方案**：要先在 Atelier 给 `pith` 发版（打 tag `pith-v0.1.0`），再照 Atelier 的 `docs/adoption.md` 同步进 `src/vendor/pith/`；同步要你说一声。
4. **摘抄、笔记的导出文件夹**（1.1.0 做导入时要）：Flomo、Obsidian 导出的 Markdown，放到 `local/` 下，我只读副本。
5. 之前留下的：打开一次正式版 0.4.1：先备份、升到 v9；Word 模板点「更新」；两本《国富论》各「重新解析」一次。

## 已知的、没做的

- **改名以后，旧目录下的会话打不开文件夹了**：Claude 桌面版里开在旧路径的几个会话（「Pith v0.5」和几个 btw 讨论）工作目录不在了，要接着用就在新目录 `~/Documents/GitHub/pith` 开新会话。
  - `~/.claude.json` 里旧路径那一条没改（是 Claude Code 自己的配置），新目录第一次打开时可能要再点一次信任。
- **清掉 Git 历史以后**：
  - 文档里引用的旧提交号（`0028`「问卷」、各个决策文件里的提交号，Atelier 里的 `546725d` 等）都找不到了，只在你留的旧 `.git` 备份里。
  - Atelier 的 tag（`pomelo-v*`、`peck-v0.1.0`、`vermilion-v*`）也会没了，Pomelo、Peck 以后同步要按 tag 走，得在新仓库里重新打。
  - Atelier 的 `canvases/pomelo-fold/` 是 Pomelo 那个会话还没提交的东西，我没动；重新 `git init` 时 `git add -A` 会把它一起提交进去。
- **Atelier 的 `canvases/v1-phase56-questionnaire/` 没删**：第 4 页是 Atelier Phase 6 的冷启动验收，那边还要用。
- 第五轮问卷里填过、和新架构有关的偏好（摘抄是纯文本 Markdown、回答句后带编号、找不到时用模型自己的知识……）只在旧的 Git 历史里了（清历史以后在旧 `.git` 备份里）：删之前的 `docs/questionnaire/round-5-answers-partial.md`。出新问卷时会重新问。
- 正式版 0.4.1 还没打开过。
- 11-15 那四个修复留下的没做的照旧（`decisions/0025`）。

## 下一轮

- **先问开发程度**（`CLAUDE.md` 规则 7）。
- **定了怎么走以后**：写 1.1.0 的计划（新的决策文件、`ROADMAP.md` 分 Phase），版本号改 1.1.0，加 migration 0010 前先冻结 v9 样本库。
- **1.1.0 大致的顺序**（待计划确认）：
  1. 模型接口抽象（多家 API、缓存断点、账本记缓存命中）；
  2. Agent 循环和子任务；
  3. 技能运行时：按需加载 `SKILL.md`，工具注册，问题卡和交付卡的协议；
  4. 对话页和研究主题；
  5. 知识库：摘抄、笔记导入，检索接本机向量；
  6. 旧功能一个个改成技能，改好就删旧页面；
  7. Word 润色。
- 界面：`pith` 方案发版、同步进来以后接上；组件只用变量。

## 本轮改动

- Pith：`CLAUDE.md`、`HANDOFF.md`、`ROADMAP.md`、`README.md`、`docs/requirements.md`、`docs/decisions/0026`、`docs/decisions/0029`（新）、`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`Cargo.lock`；删了 `HANDOFF-cloud.md`。代码没改。
- Atelier：合并 `agent/claude/pith`；`CLAUDE.md`、`ROADMAP.md`、`docs/sessions.md`、`CHANGELOG.md`、`package.json`、`package-lock.json`、`.claude/settings.json`（新）、`schemes/pith/CLAUDE.md`。

## 测试

- Pith：`pnpm check` 全过：`pith` 281 通过、25 项默认忽略，其余 crate 全过，前端测试 17 + 29 通过。
- Atelier：合并以后和改完准则以后各跑一次 `npm test`，都是 37 项全过；`node tooling/pith/build.mjs --check` 一致（36 个资产）。
