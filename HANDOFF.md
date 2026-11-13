# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：v0.4 Concept Research 计划已定，等云端开工（2026-11-13）

### 本轮（C0 准备）

1. **第三轮问卷的回答**：存进 `docs/questionnaire/round-3-answers.md`。理清后写成计划 `docs/decisions/0024-v0.4-plan.md`。要点：
   - **技能和工作流**：你说「不用定死，原则上什么效果都能做」，所以产出由技能（`SKILL.md`）决定，内置七个：整章分析、读一段、问这本书、追踪概念、反方与后世评论、假设推演、自由。你也可以放自己的技能，加新技能不用改代码。
   - **固定步骤**：工作流是技能里写好的几步，不让模型自己反复调工具。这样你每次确认时，要发什么、最多花多少都算得出来。
   - **四类内容**：原文 / 他人 / 解释 / 推演。「他人」是有出处的评论，也包括译本的旁注、注释、导读，它们不是斯密的原文。
   - **核对**：原文逐字核对，核不上的不进卡片；解释挂不上原文照样显示，但要标出来。
   - **PDF**：走 Mathpix，上传前确认页数和费用，用完删掉 Mathpix 上的副本。
   - **模型**：GPT-6.1 Sol，走 OpenAI Responses API，不让 OpenAI 存对话。官方价每百万 token 输入 $2、输出 $10。
   - **Word**：卡片原文插成引文，英文带页码和段落号，中文只带段落号。
2. **两本 EPUB 都能用**，副本放在 `local/wealth-of-nations/`，你「下载」里的原件没动：
   - **英文**：Penguin Classics，Skinner 编，Books I–V 合成一册。第一篇第 1–3 章正文 11 / 5 / 8 段，和 Glasgow 版段落号一致，所以照样标 I.i.3。有纸本页码（第 109–126 页）。不用另下 Cannan 版。
   - **中文**：杨敬年译，陕西人民 2011。同样 11 / 5 / 8 段，能和英文逐段对齐。**没有页码**，所以中文引文只给段落号。每章还带导读、旁注、注释，要和正文分开。
3. **分期**（`ROADMAP.md`）：C1 文本层、C2 引擎与接口、C3 页面交给云端，一口气做完推到 `cloud/v0.4`；C4 本地验收。任务写进了 `HANDOFF-cloud.md`。
4. **版本号改为 0.4.0**：`package.json`、`src-tauri/Cargo.toml`、`tauri.conf.json`，`Cargo.lock` 跟着变。
5. **精简文档**：
   - `requirements.md`：做完的版本只留还约束后面的要点，加了 v0.4 一节。
   - `ROADMAP.md`：已完成的各版本缩成一段。
   - `architecture.md`：删掉过时的命令总表，命令以 `ipc.ts` 为准。
   - `schema.md`：补上 0006 以后的 migration。
   - `README.md`：更新「当前状态」，文档表分「常用」和「参考」两组。
   - `HANDOFF-cloud.md`：v0.3 的任务和云端记录早已并进来，删掉了，原文在 git 历史里。
   - `brief-digest.md`：注明现行的版本顺序。

## 等你

1. **提交并推送这一轮**（git 命令在我的回复里）。要先推送，云端才能从 `main` 建 `cloud/v0.4`。
2. **让云端开工**：
   - 在云端会话里说「读 `HANDOFF-cloud.md`，开始 v0.4 C1–C3」；
   - 把两本 EPUB 发给它（就是「下载」里那两个文件），它会放进自己的 `local/wealth-of-nations/`，不提交。
3. **C4 之前准备**（不急，云端做完前备好就行）：
   - **OpenAI API 密钥**：platform.openai.com，账号要能用 `gpt-6.1-sol`。
   - **Mathpix API 密钥**：console.mathpix.com 的 `app_id` 和 `app_key`。官方价目页写着开通第一个密钥要一次性付 $19.99，不退；每页 $0.005。
   - **一份篇幅短的 PDF**（新闻或短文，中文的更好）：放进 `local/concept-pdf/`。
   - 真实验收的花费上限：我粗估 $2–4，到时候再问你。
4. 以前留下的（不急）：
   - W5 排好的终稿建议在 Word 里看一眼：`local/real-thesis/out/agent-w5/终稿/brief/毕业论文_终稿（Pith 排版）.docx`；
   - 原创性声明和使用授权书要不要排在同一页。

## 下一轮

- **云端做完后（C4）**：
  - 抓 `cloud/v0.4`，在 `.claude/worktrees/` 里验收（`target/` 不和主目录共用，见记忆）；
  - 逐项审改动：macOS 部分、git、费用、书的文字有没有进 Git；
  - 钥匙串接 OpenAI、Mathpix 两把新密钥；
  - Word 宏：在引文前插入原文，在 Mac Word 里看；
  - 两本真实 EPUB；
  - 真实 GPT-6.1 Sol 跑分工三章（事先问上限），核对 Responses API 的细节和单价；
  - Mathpix 真实 PDF；
  - 三种导出；
  - 请你抽查 10 张卡；
  - 把云端记录并进 decisions / architecture / testing。
- **云端做的这段时间**，本地没有排定的事。可以处理下面的已知问题，但别动云端要改的文件。
- 不急的已知问题：
  - 「三成」这类中文数量词只标出、不核对；
  - 中文 PDF 取文字没用真实文件验过（v0.4 的 Mathpix 会顺带验）；
  - 调查进行中退出 App，当时那次调用不记账；
  - 执行前的费用估计偏粗；
  - v0.2 的：真实 App 里经 Tauri 命令走一遍排版 Agent，编号定义里的列表、表格首列、公式编号、四线表、表注、脚注分隔线、目录生成。

## 本轮改动

- 新：
  - `docs/decisions/0024-v0.4-plan.md`；
  - `docs/questionnaire/round-3-answers.md`。
- 改：
  - `HANDOFF-cloud.md`（重写成 v0.4 任务）、`ROADMAP.md`、`README.md`、本文件；
  - `docs/requirements.md`、`docs/architecture.md`、`docs/schema.md`、`docs/brief-digest.md`；
  - `docs/decisions/0022`：注明 v0.3 云端记录现在在 git 历史里（提交 `5bc9316`）；
  - 版本号：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`Cargo.lock`。
- 本机（不进 Git）：`local/wealth-of-nations/` 两本 EPUB 的副本，SHA-256 和原件一致。

## 测试

- `pnpm check` 全过：
  - tsc；
  - 前端 node 测试；
  - `cargo test --workspace`，其中 `pith` 247 通过、18 项默认忽略；
  - `poc/word-addin` 29 通过。
- 这一轮只改了版本号和文档，没跑 clippy。
- 两本 EPUB 的检查：解压到临时目录读目录、`page-map.xml` 和第一篇第 1–3 章的段落（脚本在临时目录，没进仓库），结果写在 `0024`。
- 问卷回答从 Artifact 数据库 `answers/round3` 取出：22 题答了 21 题（E4 没答）。
