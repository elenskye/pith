# 0036 · 模型网关：四家、每家三档、价格表和用量圈（1.1.0 第 1 期）

- 日期：2026-11-26
- 状态：现行。落实 `0034` 的 1.1.0 第 1 期、`0035`「循环」一节的网关部分
- 来源：`0035`（网关从 Peck 抽）；Peck 的 `../personal-cli-studio/peck/src-tauri/src/gateway/`（只读参考：`convert/anthropic.rs`、`responses.rs`、`chat.rs`、`gemini.rs`、`providers.rs`、`config.rs`、`defaults/prices.json`）；Anthropic 的现行文档（claude-api 技能，2026-11）

## 做法

- **代码在 `src-tauri/src/gateway/`**，同步写（ureq + 线程，和 Pith 别处一样），不引 tokio、reqwest。Peck 的四个编码器和流解析照搬思路、改写成 Pith 的类型：
  - 中立的 `Message` / `Part`（文字、思考、工具调用、工具结果、各家私有块）；思考的签名和密文、Gemini 的 thought signature、Anthropic 回退的块挂在 `Opaque` 上。OpenAI、DeepSeek、Gemini 的**只回放给同一个模型**；Anthropic 的照现行文档，换了 Claude 模型也原样放回（读不了的 API 自己丢、不计费），换到别家才丢。
  - 四条线协议：Anthropic Messages、OpenAI Responses（`store: false`，思考带密文回来）、DeepSeek Chat Completions（`reasoning_content` 放回）、Gemini（API Key 和 Vertex AI 共用）。
  - 重试：连不上、408 / 409 / 429 / 5xx / 529 照 2、5、15、30、60 秒重发，有 `retry-after` 听它的；流开始以后断了不重发（已经在计费），按估计记账。
  - 能停：读流在另一条线程里，这边每 0.1 秒看一次「停」，点了立刻返回。
  - 模拟模型：脚本化（测试），开发版和 `?mock` 用会看请求的「脑子」（第 2 期）。
- **Vertex AI 只接 Gemini**（Google 自己的模型），认证照 Peck 用 gcloud 的 ADC：Pith 不读凭据文件，只看它在不在，令牌由 `gcloud auth application-default print-access-token` 换、45 分钟换一次。Claude 在 Vertex 上价格另算，先不接。
- **密钥**沿用 `agent::secret` 的钥匙串 / 内存 / 密钥文件；Anthropic、OpenAI、DeepSeek 用以前那一把（排版、事实调查、概念研究共用），新加 Google 一把（`AIza…`）。

## 每家三档（默认均衡）

| 家 | 便宜 | 均衡（默认） | 强 |
| --- | --- | --- | --- |
| Anthropic | Haiku 5.5 · low | Sonnet 5.5 · medium | Opus 5.5 · high |
| OpenAI | GPT-6 Luna · low | GPT-6.2 Sol · medium | GPT-6.2 Astra · high |
| DeepSeek | V4.1 Flash · 不思考 | V4.1 Flash · high | V4 Pro · high |
| Google / Vertex | Gemini 3.5 Flash Lite · low | Gemini 3.8 Flash · medium | Gemini 4 Carbon · high |

- 一档 = 一个模型 + 思考深浅（effort）。Opus 5.5 的 effort 默认是 medium、Sonnet 5.5 是 high，这里每档都写明；Claude 5.5 一族思考关不掉。
- 「难的再换」：主对话一直用一档（缓存按模型分，来回换会白付），读整章、写长东西交给高一档的子代理（第 2 期起）。设置里换默认；输入框下面的模型名点开也能换。
- 价格（美元 / 百万 token，缓存读、写另有单价，长提示整条换高一档）：Anthropic 取官方价目；OpenAI、Google、DeepSeek 取 Peck 2026-11-25 联网查的价格表；DeepSeek 的人民币价按高峰 ×2、6.7 折美元（和 v0.3 账本里的 0.30 / 1.20 一样）。表在 `gateway/catalog.rs`，`agent::usage::price` 先查它，旧功能也跟着用新价。

## Anthropic 的几处（照现行文档）

- `thinking: adaptive`、显示摘要；effort 在 `output_config`；不发 `temperature`（API 版本 2026-10-21 起 400）。
- 缓存断点：系统提示里标了 `cache` 的块（最多两块）+ 最后一条站主消息 + 它前一条，共不超过 4 个；顺序 工具 → 系统 → 对话，对话只往后追加。
- 工具开 `eager_input_streaming`；参数读不成完整 JSON 的不丢，标出来交给循环回一个出错的结果。
- Opus 5.5、Sonnet 5.5 开服务端的拒答回退（`fallbacks: "default"`，beta `server-side-fallback-2026-07-01`）：分类器误拦时 API 自己换模型接着答，账本按实际回答的模型计价、备注「由 … 回答」。Haiku 5.5 没有这项。**这一点和 `0018`「模型拒答直接报错、不换模型」不同**：那是排版这类要确定结果的流程；对话里误拦让人卡住，回退由 API 做、看得见、记得清。要关掉照说。

## 记账和月上限

- 每次调用之前按最坏情况（输入估计留一成、按缓存写的价算 + 输出写满）查一次，超了月上限就不发（`ErrorKind::Budget`）；收完按回报的用量 × 价格表记进 `agent/usage.jsonl`（和排版、事实调查、概念研究共用一本账、一个上限）。中途断了、停了按估计记。
- 账本一行加了 `conversation`（哪段对话），用量明细能按对话算。
- 用量圈：输入框下面一行，左边模型名（点开换一档、换一家），右边本月花了多少的小圈（点开：本月合计和上限、每天的细柱、各家、这段对话、缓存命中）。设置 › 模型与费用重画：大圈和大数字、每天的细柱、各家的比例、缓存命中、改上限；默认模型（五家卡片 + 三档）；各家的密钥；Vertex 的项目和 gcloud 登录；最近的几笔。

## 没做的

- Claude 在 Vertex 上、Bedrock：不接。
- 每家的联网搜索（Anthropic 的 web_search、OpenAI 的 web_search、Gemini 的 googleSearch）：第 4 期。
- 价格表联网刷新：不做，跟着版本更新；设置里能覆盖单价（`usage::Prices`）。
