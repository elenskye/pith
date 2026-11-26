# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.1.0 第 1 期「模型和用量」做完（2026-11-26）

你 11-26 说开工 1.1.0 第 1、2 期，`0034`、`0035` 算认了（状态改成「现行」）。这一期把模型网关立起来，对话还没接上它（第 2 期）。

### 本轮做了什么

- **版本号 1.1.0**（`package.json`、`src-tauri/Cargo.toml`、`tauri.conf.json`）。
- **模型网关** `src-tauri/src/gateway/`（`decisions/0036`）：从 Peck 抽、改成同步（ureq + 线程）。
  - 中立的消息和片段；Anthropic Messages、OpenAI Responses、DeepSeek Chat Completions、Gemini（API Key 和 Vertex AI 共用）四条线协议的编码和流解析；思考签名、密文、Gemini 的 thought signature 只回放给同一来源。
  - 重试 2 / 5 / 15 / 30 / 60 秒；读流放另一条线程，点停立刻返回；流断了按估计记账。
  - Vertex AI：项目 ID、区域存设置；gcloud 的 ADC 换令牌（不读凭据文件，45 分钟换一次）。
  - 每家三档（便宜 / 均衡 / 强），默认 Anthropic · 均衡（Sonnet 5.5）；价格表带缓存读写价和长提示加价，`agent::usage::price` 先查它（旧功能也用新价）。
  - Anthropic：adaptive 思考 + 摘要、effort 写明、缓存断点（系统两块 + 最后两条站主消息）、工具边写边流、Opus / Sonnet 开服务端拒答回退（`fallbacks: "default"`）。**和 `0018`「拒答不换模型」不同，理由写在 `0036`，要关照说。**
  - 记账：发送前按最坏情况查月上限；收完按回报的模型计价；账本一行加了 `conversation`。
  - 模拟模型（脚本化），测试用；开发版的「脑子」第 2 期加。
  - 密钥：新加 Google（`AIza…`）；Anthropic、OpenAI、DeepSeek 用以前那一把。
  - 命令：`model_status`、`model_prefs_set`、`model_key_set`、`model_key_clear`、`model_vertex_set`、`usage_summary`。
- **界面**：
  - `src/chat/Usage.tsx`：输入框下面一行，左边模型名（点开换一档、换一家），右边本月用量的小圈（点开：本月、每天的细柱、各家、这段对话、缓存命中）。首页和对话里都有。
  - `src/settings/ModelSettings.tsx`：设置 › 模型与费用照 R1 重画（大圈大字、每天的细柱、各家比例、改上限；五家卡片 + 三档；各家密钥只露「换」「清掉」；Vertex 的项目和 gcloud 登录；最近几笔）。
  - 拿掉旧的 `KeysSettings.tsx` 和事实调查、概念研究设置里没人用了的密钥行。
  - `?mock`：`src/dev/mockModel.ts`。
- **文档**：新 `0036`；`architecture.md`（目录、模型网关一节）；`ROADMAP.md`（第 1 期打勾、验收清单三行）；决策索引。

### 测试

- `cargo test --workspace` 全过（新增 gateway 18 个：四家 body、四家流、截断的参数、重试和认证错误、流断了按估计、停、月上限、按回报模型计价和设置覆盖、用量汇总、Vertex 地址）。
- `tsc --noEmit` 过。`pnpm check` 见提交前的结果（下面「提交」）。
- `?mock` 里看过：首页输入框下面一行、用量明细弹出、设置 › 模型与费用整页。控制台只有原来就有的「引文排版器没有启动」（`?mock` 里没有 Tauri 事件，和这次无关）。
- 没有用真接口跑（真实验收在第 5 期）。

### 提交

- `v1.1.0: add model gateway, usage ring, model settings`

## 等你

1. **推送**：Pith `main`（含上一轮的计划和这一期）。
2. **第 5 期要的**（不急）：四家的密钥照旧放在密钥文件夹，Google 的 API Key 一起放进去；Vertex AI 的项目 ID、区域，终端里登一次 `gcloud auth application-default login`。
3. 拒答回退（上面加粗那条）默认开着，不要就说。

## 已知的、没做的

- 对话还是演示的回答（第 2 期换成 Agent 循环）。
- 每家的联网搜索、Claude 在 Vertex 上：第 4 期 / 不接。
- 1.0.5 那些照旧：卡片滑的时候宽度一下变；洇开的圈里是静的；文献库表格没虚拟滚动；技能页、导入页、设置旧块的样子没重画。

## 下一轮

第 2 期 Agent 循环：Rust 循环（并行只读工具、停的条件、防打转、事件）、提示分层、工具按插件分组、缓存命中记账、计算 / 问题卡 / 交付卡 / 查文献库、状态行跟着事件换图标、强一档的子代理（底子）、演示的回答拿掉。
