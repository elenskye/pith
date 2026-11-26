# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.1.0 做完（2026-11-26），正式版装好了

你说第 4、5 期一起做：两个提交。第 5 期真书真模型验收花了 **$0.63**（上限 $20），找出并修了六处问题；正式版 1.1.0 已经装进「应用程序」（没有打开它，`~/Pith` 没碰）。

### 第 5 期：真实验收和正式版（`decisions/0040`）

- **怎么验的**：`chat/acceptance.rs`（忽略的测试），和 App 走同一条路：网关的真模型、App 的工具环境、本机检索、真联网，回答存进测试目录 `local/acceptance-1.1.0/data` 的对话里。两本真《国富论》迁进书架、本机补 8112 块向量。
- **结果**（详表在 `0040`）：Anthropic、OpenAI、DeepSeek 问书都答得好，引用编号一共 93 个、库里全对得上；短章 `kb_read` 直接读完，长章（IV.ii）交给 Opus 子代理读完整章；联网三家都通（Claude 搜索、OpenAI 搜索、DeepSeek 借 Claude 的），取回原页、引用网页。**Google 没跑成**：Gemini 3.8 Flash 先 503「high demand」，后 429「exceeded your current quota」——账号那边的额度；便宜一档 3.5 Flash Lite 的工具调用和回放真的跑通了。Vertex 没跑（要你登录 gcloud、给项目 ID）。
- **修了的六处**：
  1. 回答丢了：DeepSeek 把整篇回答和交付卡写在同一轮，以前被当成过程话清掉（`harness::answer_text`）。
  2. 英文版第四、五篇的页码全是 570：Penguin 第二卷的 EPUB 没有页码标记，解析器一直往下带；现在离上一个标记约两页以外就不写页码（解析器 `concept-text-3`）。**你的正式数据要重新解析一次英文版**（验收清单 1.1.0-5）。
  3. Google 的新密钥（`AQ.` 开头，你的就是）设置里填不进：两种都认了。
  4. 中译本的段落页边注写「译文」，不再写「原文」。
  5. 模型常用的写法：单个换行隔开的小标题 / 列表 / 段落、`>` 引文、`[文字](网址)`、引用范围 `[P12–P15]`。
  6. 写到一半断了却提示「没有产生费用」。
- **正式版**：`pnpm app:install` 装好 1.1.0（固定证书签名）。第一次打开会先备份、把数据库升到 v12、书迁进书架。

改的文件（第 5 期）：新 `src-tauri/src/chat/acceptance.rs`、`src/chat/markdown.ts`（+ 测试）、`docs/decisions/0040`；改 `harness/{mod,tools,tests}.rs`、`kb/{cite,retrieve}.rs`、`agent/secret.rs`、`concept-engine/src/text.rs`（+ `tests/text.rs`）、`chat/Reply.tsx`、`styles/app.css`；文档 `architecture.md`、`testing.md`、`ROADMAP.md`、决策索引。

测试：`pnpm check` 全过（Rust 主库 337 个、前端 23 个、concept-engine 全部）；真实验收命令在 `docs/testing.md`「1.1.0 真实验收」；两本真书的解析测试（忽略的）重跑过，第一至三篇页码不变。

提交：
- `v1.1.0: add web search, page fetch, web citations`（第 4 期）
- `v1.1.0: real acceptance fixes, install`（第 5 期）

## 第 4 期（2026-11-26）

### 第 4 期：联网（`decisions/0039`）

- **冻结 v11 样本库**（`fixtures/library-v11.json`，带知识库的块），**migration 0012**：`web_pages`、`web_blocks`。
- **工具**（插件「联网」）：`web_search`（搜，返回标题、网站、网址、摘要）、`web_fetch`（取回一页，存快照，每段一个编号 `[W12]`，太长的接着读）。子代理也能用。
- **搜索跟着这一回的模型**：Anthropic → Claude 网页搜索（Haiku 搜一次）；OpenAI → Responses 网页搜索；Google、Vertex → Gemini 的 Google 搜索（新写的 `web/gemini.rs`）；DeepSeek → Tavily；用不了就用 Tavily，再不行借别家有密钥的，并说一声。密钥第一次用到联网工具时才读。
- **取回**：v0.3 的本机下载器（不下本机和内网），失败了有 Anthropic 密钥就用 Claude 网页抓取。快照在 `数据目录/web/`。
- **引用**：`[W12]` 和 `[P123]` 一起编号；网页的页边注：标题、网站、取回的日子、那一段；「打开网页」「存进知识库」（存成网页快照材料，块就是那一页的块，能检索、能引用；存过的变「知识库里」）。
- **账本**：用途 `chat_web`（用量明细里叫「联网」），带着对话。
- 开发版、`?mock`：模拟搜索和合成网页；模拟脑子遇到「网上、最新、联网、查一下、新闻」先搜、取回、引用。

改的文件：新 `src-tauri/migrations/0012_web.sql`、`src-tauri/src/web/*`、`fixtures/library-v11.json`、`docs/decisions/0039-web.md`；改 `kb/{cite,passages,retrieve,store,commands}.rs`、`chat/{run,tests}.rs`（工具环境改成不碰 Tauri 的 `ChatEnv`，真实验收也用它）、`harness/{tools,prompt,brain,tests}.rs`、`research/{meter,download}.rs`、`gateway/commands.rs`、`db.rs`、`transfer.rs`、`lib.rs`；前端 `ipc.ts`、`chat/Reply.tsx`、`dev/{mockChat,mockModel}.ts`；文档。

测试：`pnpm check` 全过（Rust 主库 336 个，新增联网 5 个、工具 1 个）。`?mock` 里走过：问一句要上网的 → 状态行「在网上搜」「取回了」→ 页边注 → 存进知识库 → 变「知识库里」。真联网在第 5 期。

提交：`v1.1.0: add web search, page fetch, web citations`

## 等你

1. **推送**：Pith `main`（第 3、4、5 期）。
2. **正式版里点一遍**：`ROADMAP.md`「用户验收清单」里 1.1.0-1 到 1.1.0-5，最要紧的三件：第一次打开（升级、书架）、**英文版《国富论》重新解析一次**（修页码）、设置 › 知识库与检索「建索引」。
3. **Google**：到 aistudio.google.com 看这把 Key 的项目开没开计费、额度多少（验收那天 3.8 Flash 一直 503 / 429）。
4. **Vertex AI**（想用的话）：终端里 `gcloud auth application-default login`，设置里填项目 ID；告诉我项目 ID，我可以补跑 Vertex 那一轮（几分钱）。
5. 两件我自己定的，不要就说：书迁进新建的「书架」；有向量时对齐默认用相似度。

## 已知的、没做的

- Gemini 的 Google 搜索（grounding）只有单元测试，没在真接口上跑成（额度）；Tavily 没有密钥，没真跑。
- 网页快照在知识库面板里还没有专门的阅读样子；没存进知识库的快照不清理。
- 输入框下面的模型名改的是默认，还不能每段对话各选各的。
- 子代理只有「读」一种、不能再派；后台任务、组队、插件库在后面的版本。
- 1.0.5 那些照旧：卡片滑的时候宽度一下变；洇开的圈里是静的；文献库表格没虚拟滚动；技能页、导入页、设置旧块的样子没重画。

## 下一轮

1.1.0 做完了。下一版是 **1.2.0 PDF 交给 engine-pdf**（`0034`）：开工前先问开发程度；那一版开工前还要问你一件——PDF 的标注写进文件、另存一份，还是照现在留在库里；engine-pdf 的 tag 要先推到 GitHub。
