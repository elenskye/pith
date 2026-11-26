# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.1.0 第 1、2 期做完（2026-11-26）

你 11-26 说开工 1.1.0 第 1、2 期，`0034`、`0035` 算认了（状态改成「现行」）。两期各一个提交。对话现在真的接上模型了：开发版用模拟模型（不花钱），正式版要等第 5 期装。

### 第 1 期：模型和用量（`decisions/0036`）

- **版本号 1.1.0**（三处一起改）。
- **模型网关** `src-tauri/src/gateway/`：从 Peck 抽、改成同步（ureq + 线程）。四条线协议（Anthropic Messages、OpenAI Responses、DeepSeek Chat Completions、Gemini——API Key 和 Vertex AI 共用）的编码和流解析；重试 2 / 5 / 15 / 30 / 60 秒；读流放另一条线程，点停立刻返回；流断了按估计记账；模拟模型。
- **每家三档**，默认 Anthropic · 均衡（Sonnet 5.5）：表在 `0036`。价格表带缓存读写价、长提示加价，`agent::usage::price` 先查它。
- **Anthropic**：adaptive 思考 + 摘要、每档写明 effort、缓存断点（系统两块 + 最后两条站主消息）、工具边写边流、Opus / Sonnet 开服务端拒答回退。**和 `0018`「拒答不换模型」不同，理由在 `0036`，不要就说。**
- **Vertex AI**：项目 ID、区域；gcloud 的 ADC 换令牌（不读凭据文件）。只接 Gemini。
- **密钥**：新加 Google（`AIza…`）；别家用以前那一把。
- **记账**：发送前按最坏情况查月上限；账本一行加了 `conversation`；子代理并行记账时一行一次写完、加了进程锁（原来并发会把两行写交错——测试里抓到的）。
- **界面**：输入框下面一行（`src/chat/Usage.tsx`：左边模型名点开换一档、换一家；右边本月用量的小圈点开看明细）；设置 › 模型与费用照 R1 重画（`src/settings/ModelSettings.tsx`）。拿掉旧的 `KeysSettings.tsx` 和没人用的密钥行。

### 第 2 期：Agent 循环（`decisions/0037`）

- **循环** `src-tauri/src/harness/`：调模型 → 跑工具（只读的并行，一次最多 3 个）→ 接回去。停：写完、问了你、16 轮（再给一轮收尾）、打转（同一调用同一结果第 3 次提醒、第 5 次收尾）、月上限、拒答、截断、停、出错——都存下来，回答下面一行说怎么结束的。
- **提示分层**：App 的规矩 + 插件清单（所有对话一样）| 角色；今天几号、勾了哪些知识库、`@` 请了谁放进你那一句的开头，存成 `sent`，原样重放。
- **工具（按插件分组）**：基础 `calculate`（精确十进制）、`ask_user`（问题卡）、`deliver`（交付卡）、`delegate`（子代理）；文献库 `library_search`、`library_item`。
- **子代理（底子）**：同一家强一档、只读的工具、看不到主对话、不带角色；一次最多 12 个、同时 3 个；步骤收在派它的那一格下面，钱算进这一回。
- **对话里**：`chat/run.rs` 后台跑，界面每 0.2 秒 `chat_live` 看进度：状态行一格格出来，正文一边写一边出来；输入框右边的方块停下；问题卡点选项就当你这样回答；交付卡「存进」存的是卡上的正文。回答下面一行写着模型和花了多少。**演示的回答（`chat/mock.rs`）拿掉了**；1.0.x 存下的演示回答还能看（标着「1.0 的演示回答」），不进给模型看的对话。
- 开发版、`?mock` 的模拟模型：有算式先算，再查文献库（有英文词拿英文词搜）、做交付卡、写一段说明这是模拟的。

### 改了哪些文件（两期合起来）

- 新：`src-tauri/src/gateway/*`、`src-tauri/src/harness/*`、`src-tauri/src/chat/run.rs`、`src/chat/Usage.tsx`、`src/settings/ModelSettings.tsx`、`src/dev/mockModel.ts`、`docs/decisions/0036`、`0037`。
- 改：`agent/usage.rs`（价格、账本）、`agent/secret.rs`（Google）、`research/commands.rs`、`research/meter.rs`、`chat/*`、`lib.rs`；`src/chat/{ChatScreen,Composer,Reply,Home}.tsx`、`src/settings/*`、`src/shared/ipc.ts`、`src/dev/{mockChat,mockIpc}.ts`、`src/styles/app.css`；文档 `architecture.md`、`schema.md`、`requirements.md`、`ROADMAP.md`、决策索引、`0034`、`0035`。
- 删：`src-tauri/src/chat/mock.rs`、`src/settings/KeysSettings.tsx`。

### 测试

- `pnpm check` 全过（`tsc --noEmit`、前端 node 测试、`cargo test --workspace`、poc 检查）。Rust 主库 321 个（新增：网关 18、循环 13、计算 1、提示 2、模拟脑子 1、对话 4——含「开发版脑子跑两句、第二句原样带着第一句」的整条路）。
- `?mock` 里走过：首页问一句（状态行、流式正文、交付卡、模型和花费）、带「还是」的问题给问题卡、点选项接着答、写到一半点停（已写的留着）、用量明细、设置 › 模型与费用整页。控制台只有原来就有的「引文排版器没有启动」（`?mock` 里 Tauri 事件装得晚，和这次无关）。
- **没有用真接口跑**：四家的线协议照 Peck 已经在真接口上跑过的写法改的，加上 Anthropic 的现行文档；真的一次都没调（真实验收是第 5 期，上限 $20）。开发版的 Tauri 窗口没有打开看（后台线程和轮询在 `?mock` 和单元测试里走过）。

### 提交

- `v1.1.0: add model gateway, usage ring, model settings`（第 1 期）
- `v1.1.0: add agent loop, live replies, question and delivery cards`（第 2 期）

## 等你

1. **推送**：Pith `main`（上一轮的计划 + 这两期）。
2. **第 5 期要的**（不急）：四家的密钥放进密钥文件夹（Google 的 API Key 一起）；Vertex AI 的项目 ID、区域，终端里登一次 `gcloud auth application-default login`。
3. 拒答回退默认开着（第 1 期加粗那条），不要就说。
4. 想先摸一下真回答：开发版设 `PITH_REAL_MODEL=1` 和 `PITH_API_KEY_FILE` 能用真接口（会花钱，记进账本、受月上限管）。

## 已知的、没做的

- 输入框下面的模型名改的是**默认**，还不能每段对话各选各的。
- 回答还不会在知识库里找、没有引用编号、不能联网（第 3、4 期）。
- 子代理只有「读」一种、不能再派；后台任务、组队、插件库在后面的版本。
- 1.0.5 那些照旧：卡片滑的时候宽度一下变；洇开的圈里是静的；文献库表格没虚拟滚动；技能页、导入页、设置旧块的样子没重画。

## 下一轮

你说「进入下一期」，开 **1.1.0 第 3 期：知识库检索**：冻结 v10 样本库；migration 0011（材料能进多个库、段落块）；本机向量和重排进 `search.db`；v0.4 解析好的书迁成全文材料；中英对齐补上向量相似度、在两本《国富论》上和只用长度的比一遍；工具：检索、读原文（上下文、整章、对照译文）、引用（[n]、页边注、一步到文献库）。读整章交给第 2 期立好的子代理。
