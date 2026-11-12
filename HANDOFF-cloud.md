# HANDOFF-cloud — 云端交接

> 规则：`CLAUDE.md` 规则 7。开头的「任务」由本地写；云端每轮在文末「云端记录」里写结果，最新的放最上面。本地验收时把记录并进 `HANDOFF.md`。

## 任务：v0.3 F1–F3（本地写于 2026-11-12）

一口气做完 v0.3 里云端能做的部分（F1 证据层与流程、F2 接口、F3 页面），推到 `cloud/v0.3`。之后本地一次性验收（F4）。计划全文在 `docs/decisions/0020-v0.3-plan.md`，**以它为准**；这里只写怎么开工、做到什么程度、哪些不碰。

### 0. 先读

1. `CLAUDE.md`：重点是规则 4、5、7。
2. `docs/decisions/0020-v0.3-plan.md`：v0.3 的全部计划。
3. `docs/requirements.md`「v0.3 Fact Research」一节。
4. `docs/architecture.md`：规则、目录、「排版 Agent」一节。调查流程照排版 Agent 的做法：发送前确认框、用量账本、模拟模型。
5. `docs/testing.md`：升级测试、模拟数据模式。
6. 代码入口：
   - `src-tauri/src/agent/`：`claude.rs`（调 Anthropic、录制回复的测试写法）、`usage.rs`（账本、单价、月上限）、`secret.rs`（多把密钥，`ApiKey` + `Secret::for_key`）、`mod.rs`（命令与设置）、`tests.rs`。
   - `src-tauri/src/db.rs` 和 `src-tauri/migrations/`：migration 怎么加。
   - `src-tauri/src/import/mod.rs` 的 `insert_item`：「加入文献库」可以参考。
   - `src/format/`：排版页，调查页参考它的确认框和进度。
   - `src/shared/ipc.ts`、`src/dev/mockIpc.ts`、`src/dev/mockAgent.ts`：前端调用与示例数据模式。
   - `crates/docx-engine`：Word 导出用它。

### 1. 环境

- 系统库由 `scripts/cloud-setup.sh` 装（用户已粘贴到云端环境设置里）。如果没装上，进会话后照里面的命令自己装。
- Node：前端测试是 `node --test 'src/**/*.test.ts'`，要 Node 能直接跑 .ts（22.18 以上或 23.6 以上）。版本不够就装新的。
- 分支：
  - 先 `git fetch origin`。
  - 远端已有 `cloud/v0.3`：切到它接着做。
  - 没有：从 `origin/main` 建 `cloud/v0.3`。
  - 云端会话自动建的分支不推。
- 开工先跑一次 `pnpm install` 和 `pnpm check`，结果记进「云端记录」。
  - 预期在 Linux 上全部编得过：F0 已经把 Mac 专用代码隔开了。
  - 预期在 Linux 上不跑的：3 个安装 Word 模板的测试（`#[cfg(target_os = "macos")]`）。
  - 如果还有因为 Mac 专用代码编不过或测不过的：**不改**，记下来，接着做能做的。

### 2. 要做的

**F1 证据层与流程**

- 版本号改为 0.3.0：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`。
- 新 crate `crates/research-engine`（加进 workspace），纯逻辑，不依赖 Tauri 和 macOS：
  - 流程状态：计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告；可以取消，每步的结果能存、能读回。
  - 模型、搜索、下载三类接口的 trait，加上模拟实现。
  - 提示词：计划、摘证据、结论；结构化输出及其解析。
    - 计划提示词要主动提出官方统计之外的衡量维度（`0020`「流程」第 1 条）。
    - 摘证据要记**数据生产者**（谁统计的，不是谁转述的）。
  - 核对：
    - 摘录必须在正文里逐字找得到，只容忍空白和全半角差异；数值必须在摘录里。
    - 数值解析：万、亿、%、‰、千分位、负数、区间。
    - 计算由代码做：差、比、同比、平均。
    - 结论里的每个数字都要对得上一条已核实的事实或一个算出来的值。
  - 可信度：按数据生产者分级；默认等级表照 `0020`（中国政府官方统计降权），可以由设置覆盖；报告写明用的是哪张表。
  - 重查：新旧两次调查按（指标、地区、人群、时间）对齐，给出差异。
  - 渲染：Markdown；单文件 HTML（样式内联，不引外部资源）。
- migration `0006_research.sql`，再在 `db.rs` 的 `MIGRATIONS` 里登记。
  - 只建表、加索引（`migrations_only_add` 会查）；现有升级测试会自动覆盖新版本。
  - 存取函数放 `src-tauri/src/research/`，写 fresh / upgrade 两类测试。
- 合成夹具：假网页（HTML）、假统计表（CSV / XLSX）、带文字的假 PDF，以及录制的模型回复和搜索回复。
  - **用虚构的地名、机构和数字**（如「示例市」「示例统计局」），不要写成像真实统计的样子。
- 改坏测试：以下每一种都必须被拦下：
  - 摘录改一个字；
  - 数字改一位；
  - 单位错；
  - 结论里多一个对不上的数；
  - 低可信来源被排到前面。

**F2 接口**（全部离线开发：用录制的回复和本地起的假服务器）

- Anthropic：
  - 调查用的调用要支持 Claude 网页搜索（`web_search_20260209`）：解析搜索结果和引用、`pause_turn` 续接、`refusal`。
  - 模型 `claude-opus-5-5`。思考关不掉，用 `output_config.effort` 控制深浅；不支持强制 `tool_choice`。
  - 接口细节以官方文档为准；文档访问不了就照现有 `claude.rs` 和录制样例写，在记录里列出「F4 真实调用时要核对的点」。
- OpenAI 兼容的 chat completions（DeepSeek）：
  - 默认 `https://api.deepseek.com`，模型 `deepseek-flash`；要 JSON 输出。
  - 用量从 `usage` 读，含缓存命中的 token。
- Tavily：`POST https://api.tavily.com/search`，`Authorization: Bearer`；基础搜索记 1 点、高级记 2 点。
- 专用搜索：每个模型接口在设置里可以配；Anthropic → Claude 网页搜索，DeepSeek → Tavily。
- 本机下载：
  - 用 `ureq`；超时、大小上限、跟随跳转、按内容判断类型、同一站点限速。
  - 只取单页；User-Agent 不带个人信息。
  - 存原件和 `.txt` 正文到 `数据目录/research/sources/<sha256 前两位>/`，记网址、最终网址、抓取时间、SHA-256。
  - 测试用本地 `TcpListener` 起的假服务器，不连外网。
- 取正文：HTML、PDF、XLSX / CSV。新依赖评估许可证，登记 `THIRD_PARTY.md`。取不出字的标「取文字失败」，不静默。
- 账本（`agent/usage.rs`）：
  - 每行加上接口和种类（模型 / 搜索），月上限对所有接口合计。
  - 默认上限改 $30；设置里已存的值不动。
  - 单价表照 `0020`，可以由设置覆盖。
  - 排版 Agent 的测试必须保持全过。
- 密钥：用 `secret::Secret::for_key(ApiKey::Deepseek | ApiKey::Tavily, official())`。**不改 `secret.rs` 里的 `keychain` 模块**（Mac 专用）；其余部分需要扩展可以改。

**F3 页面**

- 侧栏加「调查」入口。调查页的流程：
  1. 输入问题；
  2. 确认框：要发的内容、预计费用；
  3. 确认计划：可以删改子问题；
  4. 进度：可以取消；
  5. 短答：3–5 条结论，数字可以点回原文，来源带可信度，结论标出不确定的类型；
  6. 展开：子问题、搜索、来源、事实表（核对状态）、计算过程、费用；
  7. 操作：导出 Markdown / HTML / Word，把来源加入文献库，重查并看新旧对比。
- 设置里加「事实调查」一节：三把密钥（Anthropic 的沿用排版 Agent 那里的）、计划模型和执行模型、各接口的专用搜索、可信度等级表、月上限（与排版共用）。
- IPC 命令按 `模块_动作` 命名（如 `research_start`）；前端只经 `src/shared/ipc.ts` 调用。
- 示例数据：`src/dev/` 加调查的 mock，让浏览器 `?mock` 模式能把全流程点通。
- 样式照现有的简约做法，不打磨视觉。

### 3. 不做 / 不碰

- 不调用真实模型、搜索或任何收费接口；不用任何真实密钥（你本来就没有）。
- 不改 macOS 部分：规则 7 列的那些，加上 `secret.rs` 的 `keychain` 模块。
- 文档只写 `HANDOFF-cloud.md`、`THIRD_PARTY.md` 和新 crate 自己的 README。不改 `HANDOFF.md`、`ROADMAP.md`、`docs/` 下已有的文件。
- 本地这段时间在做 v0.2 W5，主要会动 `crates/docx-engine` 和排版 Agent（`src-tauri/src/agent/` 里排版那部分、`src/format/`）。尽量别动这些；Word 导出要给 `docx-engine` 加东西的，只加新文件或新函数，并在记录里写清。
- 不改 v0.2 排版 Agent 的行为。

### 4. 做完的标准（云端自查）

- Linux 上 `pnpm check` 全过（Mac 专用、跳过的测试列出来），`cargo clippy --workspace --all-targets` 0 警告。
- 合成夹具上整条流程有端到端测试（模拟模型、模拟搜索、本地假服务器）：计划 → 检索 → 下载 → 摘证据 → 核对 → 结论 → 报告 → 导出 → 重查对比。
- 第 2 节的改坏测试全部拦下。
- 浏览器 `?mock` 模式下调查页全流程能点通。能起 `pnpm dev` 就自己点一遍；起不了就写明。
- 新依赖登记了 `THIRD_PARTY.md`。

### 5. 提交与推送

- 小步提交。提交信息只写 `v0.3.0: <几个英文词> improve`，例如 `v0.3.0: evidence store improve`。不写正文，不署名，不用 `finished`。
- 只推 `cloud/v0.3`。不开 PR，不合并，不推 `main`，不打标签，不 force push。

### 6. 交接怎么写

在下面「云端记录」里写一节，标日期：

- 做了什么、改了哪些文件；
- 工程选择（本地会并进 decisions / architecture）；
- 测试：命令 + 结果；
- Linux 上没编、没测的部分；
- 留给本地的事：钥匙串接线、真实调用要核对的接口细节、单价核对等；
- 已知问题。

## 云端记录

（云端从这里往下写，最新的放最上面。）
