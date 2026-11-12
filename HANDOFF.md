# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：v0.3 计划问卷已发出，等你填；云端协作规则已写好；v0.2 W5 验收仍没做（2026-11-11）

### 本轮

1. **云端协作规则**（你定的）：`CLAUDE.md` 新增规则 7，规则 4 同步改了。要点：
   - 部分开发交给另一个账号的云端 Claude，最后一律本地验收。
   - 云端自己搭编译环境，能编的编；Linux 上编不过的 macOS 部分（钥匙串、AppleScript、Word 桥接、`.app` 打包）不许改。
   - 云端可以 commit 并 push，这是「Git 全部由用户自己做」的唯一例外；分支只能是 `cloud/v0.x`（现在是 `cloud/v0.3`）；不推 main、不开 PR、不合并、不打标签、不 force push。
   - 提交信息全英文、极简：大阶段完成 `v0.3.0 finished`，小阶段 `v0.3.0: <xxx> improve`；不写正文、不署名。
   - 新增 `.claude/settings.json`，关掉 Claude Code 默认加在提交和 PR 里的署名（`Co-Authored-By`、会话链接）。云端会话会读仓库里的这个文件。
   - `docs/requirements.md`「协作」一节同步。
2. **v0.3 计划问卷**：源文件 `docs/questionnaire/round-2.html`，已发成 Artifact：<https://claude.ai/artifact/E7vNutMkCth5dwfCaCPtGt>。5 部分 25 题：v0.2 收尾、v0.3 回答什么问题、怎么做（检索方式、模型、预算、快照、证据层）、和云端怎么分工、其他。回答点「提交给 Claude」会存进这个 Artifact 的存储（文档 `answers/round2`），下一轮 Agent 用 `db_get` 直接读；页面也能复制 Markdown 粘贴回对话。
3. **查清的云端环境**（claude.ai/code 官方文档，2026-11-11）：
   - Ubuntu 24.04 x86_64，约 4 核 / 16 GB；自带 Rust、Node 22、pnpm；setup script 以 root 运行，可以 `apt install`；setup script 5 分钟内跑完才会被缓存。
   - 网络默认「Limited」，白名单含 crates.io、npm、Ubuntu 软件源、api.anthropic.com。
   - GitHub 代理不限分支名（`cloud/v0.3` 可推），拒绝推标签和删分支；PR 不会自动开。
   - 默认在提交里加 `Co-Authored-By` 和 `Claude-Session` 两行，已用 `.claude/settings.json` 关掉。
4. **发现的阻碍**：`src-tauri` 无条件依赖 Mac 钥匙串库 `security-framework`，云端整个 App 后端编不过。问卷 D2 问你要不要本地先用条件编译把 Mac 专用代码隔开（推荐要）。

## 等你

1. **填问卷**：<https://claude.ai/artifact/E7vNutMkCth5dwfCaCPtGt>，填完点「提交给 Claude」，回对话说一声。
2. **把本轮改动推到 GitHub 的 main**：云端是从 GitHub 克隆仓库的，要看到规则 7 和 `.claude/settings.json` 才会照做。
3. **云端账号的准备**（不急，等 v0.3 计划定了再弄，到时我给具体步骤和 setup script）：用另一个账号登录 claude.ai/code，连上 GitHub 仓库 `elenskye/pith`（私有仓库要装 Claude GitHub App），新建环境、网络保持默认。
4. **v0.2 W5 相关**（上一轮留下的，仍然有效）：
   - 在 Word 里看一眼 W4 的结果：打开 `local/real-thesis/out/agent-w4/终稿-结构和片段.docx`，看能否正常打开；摘要页起有页眉（奇数页章名、偶数页「重庆大学本科学生毕业论文（设计）」，下有横线），封面和原创性声明起没有页眉；摘要、目录页码是 Ⅰ、Ⅱ…，正文从 1 起；两个附录表格是三线表。有不对把现象发我。
   - 要不要把正式版更新到 W4（`pnpm app:install`，约 2 分钟，没有数据库改动）：问卷 A2 也问了。
   - 装完之后：把「应用程序」里的 Pith 拖进程序坞；设置 › Word 点一次「更新」。
   - 第一次在正式版里用排版 Agent：设置 › 排版 Agent 填密钥 → 排版 › 新建规范 → 选 `local/real-thesis/src/` 的三份 Requirement（约 $0.5）。不想花钱先试：`pnpm app:dev`，选 `src-tauri/fixtures/agent/示例规范.txt`。
   - 文献库里 7 位英文作者是单栏姓名；「Pith 控制访达」可以在系统设置里关掉。

### 以后抓云端分支的命令（供参考）

```bash
git fetch origin
```

```bash
git switch cloud/v0.3
```

本地验收通过后合进 main：

```bash
git switch main
```

```bash
git merge cloud/v0.3
```

## 下一轮：读问卷回答，定 v0.3 计划

1. 读回答：Artifact slug `6a3e2912-0da0-44ff-969b-f0ccdf522c45`，`db_get` 路径 `answers/round2`（`markdown` 字段是整份回答）。存成 `docs/questionnaire/round-2-answers.md`。
2. 写 `docs/decisions/0020-v0.3-plan.md`；`ROADMAP.md` 把 v0.3 拆成分期，标出哪几期给云端；`requirements.md` 加 v0.3 一节。
3. 开 v0.3 时版本号改 0.3.0（`package.json`、`src-tauri/Cargo.toml`、`tauri.conf.json`）。
4. 如果 D2 选「要」：本地先做条件编译准备（Mac 上行为不变，`pnpm check` 全过），再写给云端的第一期任务说明和 setup script（装 Tauri 需要的 Linux 系统库）。
5. W5 按问卷 A1 的回答安排。

## v0.2 状态（W1–W4 完成，W5 未做）

细节见 `ROADMAP.md` 与 `decisions/0016`–`0019`。W4 真实模型验收（Opus 5.5，6 次调用共 $1.63）：三份重庆大学规范提炼 46 条、摘录全部核对上，合成规范与手写的一致 86 项、不同 0 项；终稿两种发送方式保真 0 问题、核对 78 / 78；完整稿 71 / 71。排出来的文档在 `local/real-thesis/out/agent-w4/`。

W4 留下的（不急）：
- 还做不了的结构类要求：公式编号的位置、四线表、表注的位置、脚注与正文之间的分隔线、目录的生成与各级格式。
- 「标题紧跟」只认紧挨着的；终稿里章标题和节标题之间隔着空段落，所以一个都没改（不删用户的空段落）。
- 双面时没有打开「对称页边距」（规范没写）。
- 三线表线宽用出版惯例（1.5 / 0.75 磅）；表头只认第一行（或第一行纵向合并到的那几行）。
- W3 留下的仍在：清单不能新增条目、PDF 不收、Word 选项卡里没有排版按钮、同一输入两次调用的边缘项有出入。

## 本轮改动

- `CLAUDE.md`：规则 4（Git 例外、提交模板）、新增规则 7（云端协作）。
- `.claude/settings.json`（新）：关闭提交与 PR 里的署名。
- `docs/requirements.md`：「协作」加提交信息格式和云端协作两条。
- `docs/questionnaire/round-2.html`（新）：v0.3 计划问卷。
- `ROADMAP.md`：「规矩」加云端协作一条。
- `HANDOFF.md`。

## 测试

- 只改了文档和配置，没碰代码。
- `pnpm check` 通过（与 W4 结束时相同）：前端引文排版 16；文档引擎 26 + 引文 18 + 排版器 12（另 1 项默认忽略）+ 结构摘要 5 + 计划 7 + 结构类 10；App 后端 197（另 15 项默认忽略）；Zotero 读取 9；Word PoC 29。
- 问卷：`node --check` 通过，题目结构检查 25 题；Artifact 已发布并开启存储。页面要登录 claude.ai 才能看，内置浏览器没登录，所以没有目视过渲染效果。
- 没碰 `~/Pith/`、`~/Zotero/`、钥匙串；没运行任何改动 git 的命令。
