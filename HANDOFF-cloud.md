# HANDOFF-cloud — 云端交接

> 规则：`CLAUDE.md` 规则 7。开头的「任务」由本地写；云端每轮在文末「云端记录」里写结果，最新的放最上面。本地验收时把记录并进 `HANDOFF.md`。
> v0.3 的任务和云端记录已经并进 `HANDOFF.md`、`docs/decisions/0022`、`docs/architecture.md`、`docs/testing.md`，原文在 git 历史里（提交 `5bc9316`）。

## 任务：v0.4 C1–C3（本地写于 2026-11-13）

一口气做完 v0.4 里云端能做的部分（C1 文本层、C2 引擎与接口、C3 页面），推到 `cloud/v0.4`。之后本地一次性验收（C4）。计划全文在 `docs/decisions/0024-v0.4-plan.md`，**以它为准**；这里只写怎么开工、做到什么程度、哪些不碰。

### 0. 先读

1. `CLAUDE.md`：重点是规则 4、5、7。
2. `docs/decisions/0024-v0.4-plan.md`：v0.4 的全部计划，含「两本 EPUB 的检查」。
3. `docs/requirements.md`「v0.4 Concept Research」一节。
4. `docs/architecture.md`：规则、目录、「v0.3 事实调查」一节。v0.4 照 v0.3 的做法：纯逻辑 crate + App 一侧接口、发送前确认框、账本、模拟接口、录制的回复、`audit`。
5. `docs/testing.md`：升级测试、`?mock`、事实调查的测试写法。
6. 代码入口：
   - `crates/research-engine`：`fold.rs`（文字折叠）、`verify.rs`（摘录核对）、`ports.rs`（模型、搜索、下载的 trait）、`flow.rs`（流程、取消、接着做）、`audit.rs`、`render.rs`；README 有模块表。
   - `src-tauri/src/research/`：`commands.rs`（命令、设置、后台执行）、`store.rs`、`openai.rs`（OpenAI 兼容的 chat completions）、`anthropic.rs`、`tavily.rs`、`meter.rs`（发送前查上限、记账）、`download.rs`、`content.rs`（取正文）、`docx.rs`（Word 导出）、`library.rs`（加入文献库）、`tests.rs`。
   - `src-tauri/src/agent/`：`usage.rs`（账本、单价、月上限）、`secret.rs`（`ApiKey` + `Secret::for_key`）。
   - `src-tauri/src/search.rs`：FTS5 与中文二元组。
   - `src-tauri/src/word/picker.rs` 与 `src/picker/Picker.tsx`：置顶搜索窗；`src/cite/`：citeproc-js。
   - `src/app/Rail.tsx`、`SideNav.tsx`、`src/research/`：主页面与侧栏的写法。
   - `src/shared/ipc.ts`、`src/dev/mockIpc.ts`、`src/dev/mockResearch.ts`：前端调用与示例数据。

### 1. 环境与材料

- 系统库由 `scripts/cloud-setup.sh` 装（用户已粘贴到云端环境设置里）；没装上就照里面的命令自己装。Node 要能直接跑 .ts（22.18 以上或 23.6 以上）。
- 分支：先 `git fetch origin`。远端已有 `cloud/v0.4` 就切到它接着做；没有就从 `origin/main` 建。云端会话自动建的分支不推。
- 开工先跑一次 `pnpm install` 和 `pnpm check`，结果记进「云端记录」。预期 Linux 上全部编得过，只有 3 个安装 Word 模板的测试不跑（`#[cfg(target_os = "macos")]`）；还有因为 Mac 专用代码编不过的，**不改**，记下来。
- 版本号已经是 0.4.0，不用改。
- **两本真实 EPUB**（用户 2026-11-13 定：自己发给云端会话，问卷 E2）：
  - 放进 `local/wealth-of-nations/`，文件名 `en-penguin-skinner.epub`、`zh-yang-jingnian-2011.epub`。`local/` 在 `.gitignore` 里，**不提交、不推送**，也不把书里的段落抄进夹具、测试、文档或提交信息。
  - 用它们的测试默认忽略（`#[ignore]`），路径用环境变量传进去（如 `PITH_BOOK_EN`、`PITH_BOOK_ZH`），只打印聚合数字（段数、页码数、角色数）。
  - 本地已经查过的结果写在 `0024`「两本 EPUB 的检查」：第一篇第 1–3 章正文都是 11 / 5 / 8 段；英文有纸本页码（第 109–126 页）；中文没有页码，有导读、旁注、注释。
  - 用户没发过来的话，照常用夹具做完，真实 EPUB 的测试写好留给本地。
- **Cannan 版夹具**（公有领域）：找 Cannan 1904 版的文字（Online Library of Liberty 或 Project Gutenberg，核对确实是 Cannan 版），只取第一篇第 1–3 章，做成小 EPUB 夹具（目录、篇章、带几个假的分页点）。去掉 Project Gutenberg 的商标和许可说明，只留正文，在夹具旁的说明里写清来源。中文用合成的片段，故意带导读、旁注、注释和一章段数对不上的情况。

### 2. 要做的

**C1 文本层**

- 新 crate `crates/concept-engine`（加进 workspace），纯逻辑，不碰数据库和网络，不依赖 Tauri 和 macOS。折叠和模型、搜索的 trait 沿用 `research-engine`（path 依赖）。
- EPUB 解析（输入是字节）：书脊顺序、目录（EPUB 3 nav / EPUB 2 ncx）、篇章节；段落 = 块级元素的文字。
  - 角色：正文、标题、旁注、注释、导读 / 导言、前后附。靠规则认，不能靠 class 名（两本书的 class 名都没有意义）。
  - 段落号：只给正文段；「篇.章.段」（`I.iii.5`）和「章.段」（`3.5`）两种，目录有篇、章两层时默认前一种；旁注、注释挂到前面的正文段上。
  - 页码：EPUB 3 page-list、EPUB 2 `page-map.xml`、正文里的分页锚点（如 `id="page_110"`）；记段内分页点的位置，给一个字符位置能算出页码（跨页写「110–111」）。
  - 小型大写之类原样取字，不改原文。
  - 依赖先复用工作区已有的（`zip`、`roxmltree`、`quick-xml`、`scraper`）。XHTML 里有 HTML 实体时 `roxmltree` 会报错，挑能容错的。新加的登记 `THIRD_PARTY.md`。
- 结构预览：各章段数、各角色数量、有没有页码，给页面在存之前显示。
- 中英对齐：同一章正文段数相同就按顺序对齐，不同就不对齐并标出。
- PDF：
  - Mathpix `v3/pdf`（上传 → 查 `status` 直到 `completed` → 下载 `lines.json` 和 `mmd` → `DELETE`）。由 `lines.json` 的行拼段落，页码用页序。接口细节照 `0024`；全部用录制的回复和本地假服务器测。
  - 默认超过 60 页提示改用 EPUB（设置里可改）。
  - 没有 Mathpix 密钥时用 `research/content.rs` 的本机取文字，提示中文可能乱码。
- migration `0008_concept.sql`，在 `db.rs` 的 `MIGRATIONS` 登记：文本、章节、段落（含页码）、对齐等，只建表、加索引（`migrations_only_add` 会查）；v7 样本库已冻结。存取放 `src-tauri/src/concept/`，写 fresh / upgrade 两类测试。
- 书 ↔ 文献：文献库里一条文献的 EPUB / PDF 附件 → 「解析成文本」（先预览，用户确认后存）；原著和译本是两条文献，两份文本互相关联。解析前做「导入前」备份。
- 段落进 `search.db`（派生，不写 migration）：段落检索要能用中文搜到对齐的英文段。

**C2 引擎与接口**

- 技能：
  - `skills/<名字>/SKILL.md`（元数据 + 每步一节的做法）。元数据格式自己定，写进 `concept-engine` 的 README。
  - 读取并检查格式；内置的随 App 打包，用户的在 `数据目录/skills/`，同名覆盖内置的并标明。
  - 七个内置技能照 `0024` 的表，每个带录制回复的测试。
- 工作流：技能声明的几步按顺序跑，步骤只有「模型」「查文献库」「网页检索」三种；每步存、可取消、能接着做。发送前算出每步要发什么、最坏费用（`max_output_tokens` 写满）。
- 产出：概念卡、论证卡、关系、评论卡、推演卡、笔记。每张卡由「句」组成，每句有类别（原文 / 他人 / 解释 / 推演）和锚点。结构化输出的 JSON Schema 和解析写在引擎里，所有技能共用。
- 核对与审计照 `0024`「内容的分类与核对」：
  - 在 v0.3 折叠之外再容忍弯直引号、撇号样式和拉丁字母大小写。这是概念研究自己的折叠，**不改 v0.3 的 `fold`**。
  - 中译句在对齐的中文段落里核对。
  - `audit` 从存下的文本重新核一遍。
  - 改坏测试：引文改一个字、挂错段落、不存在的段落号、标成原文却没有锚点、推演混进原文类、旁注或导读被标成原文、他人观点没有来源，全部要拦下。
- 反方：先查文献库（标题、摘要、笔记、标注；有 PDF 的取正文），不够再走 v0.3 的检索 + 本机下载 + 快照（复用 `research/` 的实现，别复制一份）。
- 关系图：节点是卡片，边有类型和一句说明；布局在引擎里算（确定、可测），出一份 SVG 给页面和 HTML 导出共用。
- 渲染：Markdown、单文件 HTML（样式和脚本内联，不引外部资源，关系图能点）；Word 导出在 App 一侧，照 `research/docx.rs` 用 `docx-engine` 拼。
- OpenAI Responses API（`src-tauri/src/concept/` 里新写，**不改** v0.3 `research/openai.rs` 的 chat completions）：
  - 模型 `gpt-6.1-sol`；JSON Schema 结构化输出；`reasoning.effort` 默认 medium；`store: false`；`max_output_tokens`；用量含推理 token 和缓存命中。
  - 专用搜索是 Responses 的 `web_search` 工具（没开就 Tavily）。
  - 官方文档访问不了的细节，照录制样例写，在记录里列出「C4 真实调用时要核对的点」。
- 账本与单价：GPT-6.1 Sol 输入 $2 / 缓存 $0.10 / 输出 $10（每百万 token；单次输入超 272K 按 2 倍输入、1.5 倍输出）；OpenAI 网页搜索每千次 $10；Mathpix 每页 $0.005。和其他接口共用月上限；排版 Agent 和事实调查的测试必须保持全过。
- 密钥：
  - 加 `ApiKey::Openai`、`ApiKey::Mathpix`（`app_id` + `app_key` 存一个条目），用 `Secret::for_key`。**不改 `secret.rs` 里的 `keychain` 模块**。
  - 测试用的密钥文件改成每行 `名字=密钥`，旧写法照认（OpenAI 和 DeepSeek 都是 `sk-` 开头，按形状分不开）。

**C3 页面**

- 最左一列加第四个主页面「概念」（`Rail.tsx`），侧栏：书 → 章 → 卡片（`SideNav`）。
- 正文：
  - 原文段落带段落号，中英并排，可以切换；旁注、注释、导读的样子和正文区分开；有页码的在边上标页码。
  - 划选 → 选技能；顶上有「问这本书」「追踪概念」。
  - 确认框：段落数、字数、语言、步骤、预计费用。进度可以取消。
  - 结果：卡片按种类筛，每句标类别（原文 / 他人 / 解释 / 推演），「没挂上原文」的要醒目；点锚点跳回段落并高亮引文。
  - 关系图；导出三种；评论来源加入文献库。
  - 文献库的附件上加「解析成文本」，先显示结构预览，确认后存。
- 设置：
  - 「密钥与费用」加 OpenAI、Mathpix；
  - 「模型与联网」组加一页「概念研究」：模型（默认 GPT-6.1 Sol，可选 Opus 5.5）、推理深浅、专用搜索、PDF 页数上限、技能列表（在访达中打开技能文件夹）。
- Word 搜索窗：
  - 选中解析过的书时多一栏「卡片里的原文」；定位可选「页 / 段」。
  - 引文数据：英文 locator 是页码、后面接段落号；中文 locator 是段落号。GB/T 7714 和 APA 7 各排一遍，结果写进记录。
  - 宏和桥接的另一头本地做（见第 3 节）；搜索窗返回的数据里带上原文，格式写进记录。
- `?mock`：`src/dev/` 加概念页的示例数据（由引擎从 Cannan 夹具生成），全流程能点通。
- 样式照现有的简约做法，不做视觉设计。

### 3. 不做 / 不碰

- 不调用真实模型、搜索、Mathpix 或任何收费接口，不用任何真实密钥。
- 不改 macOS 部分：规则 7 列的那些（钥匙串、`osacompile` / AppleScript、Word 桥接与模板安装、`.app` 打包），加上 `secret.rs` 的 `keychain` 模块和 `src-tauri/word/` 下的宏（`Pith.bas`）、`customUI.xml`、脚本。把原文插进 Word 的宏改动留给本地 C4。
- 两本真实 EPUB 不进 Git，书里的文字不进夹具、测试断言、文档和提交信息。
- 文档只写 `HANDOFF-cloud.md`、`THIRD_PARTY.md`、新 crate 的 README 和 `skills/` 下的文件。不改 `HANDOFF.md`、`ROADMAP.md`、`docs/` 下已有的文件。
- 不改 v0.2 排版 Agent 和 v0.3 事实调查的行为；复用它们的代码时只加新函数或新文件，在记录里写清。

### 4. 做完的标准（云端自查）

- Linux 上 `pnpm check` 全过（Mac 专用、跳过的测试列出来），`cargo clippy --workspace --all-targets` 0 警告。
- Cannan 夹具 + 合成中文：段落号、页码、角色、对齐都对；两本真实 EPUB 的解析测试（有文件时）第一篇第 1–3 章是 11 / 5 / 8 段、英文页码在 109–126。
- 录制的回复跑通四种入口（整章、划选、问题、追踪）和反方，一直到核对、审计、三种导出。
- 第 2 节的改坏测试全部拦下。
- 浏览器 `?mock` 里概念页全流程能点通；能起 `pnpm dev` 就自己点一遍，起不了就写明。
- 新依赖登记了 `THIRD_PARTY.md`。

### 5. 提交与推送

- 小步提交。提交信息只写 `v0.4.0: <几个英文词> improve`，例如 `v0.4.0: epub parser improve`。不写正文，不署名，不用 `finished`。
- 只推 `cloud/v0.4`。不开 PR，不合并，不推 `main`，不打标签，不 force push，不删分支。

### 6. 交接怎么写

在下面「云端记录」里写一节，标日期：

- 做了什么、改了哪些文件；
- 工程选择（技能元数据格式、表结构、折叠规则、布局算法等，本地会并进 decisions / architecture）；
- 新命令；
- 测试：命令 + 结果；有没有拿到两本真实 EPUB、跑了哪些；
- Linux 上没编、没测的部分；
- 留给本地的事：钥匙串接线、宏插入原文、真实调用要核对的接口细节（OpenAI Responses、Mathpix）、单价核对；
- 已知问题。

## 云端记录

（云端从这里往下写，最新的放最上面。）
