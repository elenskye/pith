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

### 2026-11-13 · v0.4 C1–C3（云端，分支 `cloud/v0.4`）

**一句话**：C1 文本层、C2 引擎与接口、C3 页面都做完了，推在 `cloud/v0.4`（6 个提交）。Linux 上 `pnpm check` 全过、clippy 0 警告；两本真实 EPUB 第一篇第 1–3 章 11 / 5 / 8 段、英文第 109–126 页、中英三章全部对齐；`?mock` 里概念页全流程用无头浏览器点通（0 个控制台错误）。真实模型、Mathpix、钥匙串、Word 宏留给 C4。

#### 做了什么、改了哪些文件

新增：

- `crates/concept-engine`（新 crate，进 workspace；纯逻辑，只依赖 research-engine、serde、zip）：`markup`（容错 XHTML 读取）、`epub`、`text`（单元 / 角色 / 段落号 / 页码 / 结构预览）、`mathpix`（lines.json → 段落；本机取文字的后备）、`align`、`fold`、`skill`、`model`、`prompts`、`verify`、`audit`、`flow`、`graph`、`render`、`fixture`、`mock`；`examples/mock_data.rs`；`tests/{text,flow,real_books}.rs`；`fixtures/wealth/`（英文 PG 文字、合成中文、README）、`fixtures/replies/`（9 份录制回复）；README（模块表、角色规则、SKILL.md 格式、命令）。
- `skills/` 七个内置技能：chapter（整章分析）、passage（读一段）、ask（问这本书）、trace（追踪概念）、critics（反方与后世评论）、counterfactual（假设推演）、free（自由）。
- `src-tauri/migrations/0008_concept.sql`（13 张表，只建表、加索引）。
- `src-tauri/src/concept/`：`commands.rs`（26 个 `concept_*` 命令）、`store.rs`、`search.rs`（段落进 search.db）、`responses.rs`（OpenAI Responses + web_search）、`mathpix.rs`、`library.rs`（文献库检索作反方来源、网页快照复制、来源加入文献库、附件文件）、`keys.rs`（两把新密钥）、`fixtures.rs`（合成 lines.json）、`tests.rs`。
- 前端 `src/concept/`（ConceptPage、ConceptSidebar、Reader、RunView、Confirm、Entries、ParseDialog、labels）、`src/settings/ConceptSettings.tsx`、`src/picker/conceptLocator.ts`、`src/cite/concept-locator.test.ts`、`src/dev/mockConcept.{ts,json}`。

改动的已有文件（都是加，不改 v0.2 / v0.3 的行为）：

| 文件 | 改了什么 |
| --- | --- |
| `crates/research-engine/src/ports.rs` | `Purpose` 加 `Concept`（账本用途名 `concept`） |
| `crates/research-engine/src/fixture.rs` | 模拟模型对 `Purpose::Concept` 返回错误（多一个 match 分支） |
| `src-tauri/src/research/anthropic.rs` | effort 的 match 多一个 `Concept => "medium"`（概念研究选 Opus 5.5 时） |
| `src-tauri/src/research/commands.rs` | `key_saved_setting` / `secret_of` 多 OpenAI、Mathpix 两个分支（指向 `concept::keys`）；加 `key_known` / `key_load` 两个 `pub(crate)` 包装 |
| `src-tauri/src/research/snapshot.rs` | 加 `Snapshots::at(root)`：概念研究的快照放 `数据目录/concept/sources/`，不会被事实调查的清理删掉 |
| `src-tauri/src/agent/secret.rs` | `ApiKey` 加 `Openai`、`Mathpix`（条目 `openai-api-key`、`mathpix-api-key`）；密钥文件改成每行 `名字=密钥`，旧写法照认（见下）。**`keychain` 模块没动** |
| `src-tauri/src/agent/usage.rs` | 单价表加 `gpt-6.1-sol`（$2 / $10） |
| `src-tauri/src/search.rs` | `index_text`、`parse_query`、`Parsed` 改成 `pub(crate)`（段落检索沿用同一套分词） |
| `src-tauri/src/db.rs` | 登记 migration 8；`schema_has_all_tables_strict_with_foreign_keys` 的表清单加 13 张、总数 31 → 44 |
| `src-tauri/src/transfer.rs` | `LABELS` 加 13 张概念表（中文名和恢复时的插入顺序） |
| `src-tauri/src/lib.rs` | `mod concept`、启动时 `concept::commands::on_startup`（在挂上 search.db 之后）、`manage`、登记命令 |
| `src/format/parts.tsx` | `Page` 加可选的 `wide`（中英并排时用宽版） |
| `src/library/ItemDetail.tsx` | EPUB / PDF 附件上加「解析成文本」（解析过的显示「概念页」） |
| `src/picker/Picker.tsx` | 「卡片里的原文」一栏、页 / 段切换 |
| `src/app/{Rail,App}.tsx`、`src/settings/{SettingsPage,KeysSettings}.tsx`、`src/shared/ipc.ts`、`src/dev/mockIpc.ts` | 第四个主页面「概念」、设置页「概念研究」、密钥页两行、命令封装、mock（模拟文献库多两本书和两个 EPUB 附件） |
| `THIRD_PARTY.md` | 加 v0.4 一节（没有新的第三方依赖） |

#### 工程选择（本地验收时并进 decisions / architecture）

1. **XHTML 自己读**（`markup.rs`，只用标准库）：roxmltree 遇到 `&nbsp;` 报错；scraper 按 HTML 解析会把 XHTML 的自闭合 `<a id="page_110"/>` 当成没关上的 `<a>`，后面每段都重建一份同 id 的元素。自己读的：认自闭合、HTML 实体（常用的约 120 个，不认识的原样留着）、没关上的标签、多余的结束标签，读不懂的当文字，从不失败。
2. **页码表的文件名**：Penguin 的 `page-map.xml` 写的是 calibre 拆分之前的名字（`book1.html#page_110`，实际文件是 `book1_split_001.html`），按 `<名>_split_*.<扩展名>` 找回来。页码来源优先级：EPUB 3 page-list → ncx 的 pageList → page-map.xml；都没有时才认正文里 `id="page_N"` 这种锚点。Penguin 实际用的是 ncx 的 pageList。
3. **角色规则**（详见 concept-engine README）：先按目录文字分单元（篇 / 章 / 节 / 导读 / 注释 / 前后附，导读先认：「〔第四章导读〕」不是第四章；上一层是导读、注释、前后附的跟着它；平铺目录里章跟着前面同层的篇），再在正文单元里逐块认：h1–h6、单元开头全粗体或链接（或和目录文字一样）的是标题；≤160 字的 `blockquote` 是旁注；以「［N］」开头（或开头是回链）、并且是注释引用指向的段落开始「注释区」，到单元结尾都是注释（译者按接在上一条注释后面）；只认方括号（「（1）熟练程度的提高」是列举不是注释）。
4. **注释的引用标记不进正文**（上标里带链接的「［1］」、epub:type=noteref），记下位置；小型大写照原样（`THE greatest`）。
5. **引用名全书唯一**：正文 `I.i.3`、旁注 `I.i.3 旁注`、注释 `I.i 注 1`（续写 `I.i 注 1（2）`）、标题 `I.i 标题`、其他单元 `前三章导读 ¶2`；不在章里的正文单元（引言）写 `Intro.5`；PDF 只有段 `5`。锚点里译本的写 `译:I.i 注 1`。
6. **对齐**：章按（第几篇, 第几章）对，引言按引用名对；段数相同才一段对一段。
7. **表结构**（0008）：concept_texts（附件 SHA-256、解析器版本 `concept-text-1`、meta JSON、original_id 关联原著）、concept_units、concept_paragraphs（含 page、breaks JSON、noterefs JSON）、concept_alignments + concept_aligned、concept_runs（scope / model / targets 是 JSON）、concept_steps（含模型原始输出 raw）、concept_cards、concept_sentences、concept_anchors（一句一行：段落号、状态、位置、页码、中译核对）、concept_relations（说明句是 JSON）、concept_sources、concept_rejected。保存一次研究 = 一个事务里整份换掉子表（同 v0.3）。
8. **段落检索**：`search.concept_paras`（FTS5，rowid = 段落 id），只收正文 / 旁注 / 注释 / 导读；启动时和每次检索前比对主库，对不上就重建这张表（恢复备份后自动对上）。中文词搜英文原著：先搜对齐的中译（旁注、注释映射到它挂的正文段），再映射回原著。
9. **概念研究的折叠**（`concept_engine::fold`）：在 v0.3 折叠（去空白、全半角）之外，再容忍弯直引号、撇号样式、拉丁字母大小写；v0.3 的 `fold` 没动。
10. **核对**（`verify.rs`）：原文句的文字必须就是引文（文字和引文不一致的拦下——「推演混进原文类」靠这条）；原文只能挂原著的正文段；他人要有来源（S1）或挂旁注 / 注释 / 导读；解释挂的段落不存在或引文找不到 → 去掉锚点、标「没挂上原文」；推演带前提的，前提按原文核对。一句不剩的卡片不要；关系两头要都在。
11. **技能元数据**：YAML 小子集（README 有完整格式）；本轮加了一个键 `instruction: required | optional`（「自由」必须写要求；「推演」可写可不写）。
12. **工作流估费**：`flow::planned` 给每个模型步骤组请求；后面步骤要看前面的卡片 / 来源的，按前面步骤的 `max_output_tokens` 和每条来源 1.2 万字写满来估输入（`extra_input_tokens`）。
13. **关系图布局**：分层（深度优先找回头的边 → 最长路径分层 → 层内按邻居平均位置排四遍 → 从左往右），同样的输入同样的图；SVG 自带浅色 / 深色样式，节点 `data-card`、边 `data-relation`。
14. **渲染**：Markdown 和 Word 用同一份 `research_engine::render::Doc`（Markdown 复用 v0.3 的 `markdown`，把「短答」标题换成「概要」；Word 调 `research/docx.rs::write`，没复制代码）；HTML 是自己的单文件（卡片筛选、点节点跳卡片、点锚点跳到「引到的原文段落」并高亮）。
15. **反方的来源**：文献库（沿用文献库的搜索索引搜检索词，取标题 / 作者 / 摘要 / 笔记 / 标注 / 第一个 PDF 附件的正文）→ 正文存快照；网页走 v0.3 的下载器（`Downloader` + `WithFallback`，这里没开 Claude 网页抓取后备），下载器存在 `research/sources/` 的快照复制一份到 `concept/sources/`。
16. **密钥文件**：每行 `名字=密钥`（名字含 anthropic / claude / deepseek / tavily / openai / mathpix 就算，不分大小写，`export`、引号都认）；写了名字的只给那一把。旧写法（一行只有密钥、或名字不是这几家的）照旧按形状认，但 OpenAI 和 Mathpix 必须写名字。Mathpix 是 `mathpix=app_id:app_key`。
17. **Word 引文**（citeproc 各排一遍，写成了测试 `src/cite/concept-locator.test.ts`）：
    - 英文（有页码）：label `page`、locator `110 (I.i.3)` → APA 7「(Smith, 1999, p. 110 (I.i.3))」，跨页「pp. 110–111 (I.i.3)」；GB/T 7714-2015 顺序编码「[1]110 (I.i.3)」；
    - 中文（没有页码）或选了「段」：label `paragraph`、locator `I.i.3` → APA「(斯密, 2011, para. I.i.3)」；GB/T 顺序编码「[1]I.i.3」；
    - **GB/T 著者-出版年制这份样式的引文里不出定位**（「(Smith, 1999)」），C4 在 Word 里看要不要改；
    - 不用「110, I.i.3」：APA 会把逗号当成多页，写成「pp.」。
    - 搜索窗返回的每条引文多一个字段 `pithQuote: { text, lang, paragraph, pages, card }`（CiteItemRef 的 extra 原样带给宏），宏用它在引文前面插这句原文。

#### 新命令

`concept_status`、`concept_settings_set`、`concept_key_set`（Mathpix 两个框：`key` = app_id、`appKey`）、`concept_key_clear`、`concept_skills_reveal`；
解析：`concept_text_prepare`（EPUB 出预览；PDF 出页数和费用）→ `concept_pdf_convert`（`local` 选本机取文字）→ `concept_text_renumber` → `concept_text_save`（先做「导入前」备份；`originalUid` 连原著并对齐）；`concept_text_link`、`concept_text_delete`、`concept_texts_list`、`concept_attachment_text`、`concept_text_get`、`concept_unit_get`、`concept_search`；
研究：`concept_run_prepare`（新的或 `resume` 接着做）、`concept_rerun_prepare`、`concept_run_execute`（后台，事件 `concept-progress` / `concept-done`）、`concept_cancel`、`concept_runs_list`、`concept_run_get`（每次审计一遍）、`concept_run_delete`、`concept_export`（三种；在访达中显示复用 `research_reveal`）、`concept_add_to_library`；Word 搜索窗：`concept_quotes`。

命令行：

```bash
cargo test -p concept-engine
```

```bash
PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub cargo test -p concept-engine --test real_books -- --ignored --nocapture
```

```bash
cargo run -q -p concept-engine --example mock_data > src/dev/mockConcept.json
```

#### 测试

- 开工：`pnpm install` 后 `pnpm check` 里 tsc、node 测试、cargo test 全过（pith 244 个），只有 `poc/word-addin` 没装依赖；在那个目录 `pnpm install` 后全过。云端 pnpm 是 10.28（锁文件是 12 写的，照样装得上）。
- 收尾：`pnpm check` **全过**（exit 0）：node 17 + poc 29；cargo：pith 258 过 / 18 忽略（新增 14：概念 12、密钥 2），concept-engine 36 过 / 1 忽略（单元 20、text 6、flow 10），research-engine、docx-engine、zotero-import 原样全过。`cargo clippy --workspace --all-targets` **0 警告**。
- 没跑的：3 个 `#[cfg(target_os = "macos")]` 的 Word 模板安装测试（Linux 上不编）；默认忽略的真实数据测试。
- **两本真实 EPUB**：用户在会话里上传了，放在 `local/wealth-of-nations/`（不进 Git）。`real_books` 测试通过（只打印聚合数字）：
  - 英文：Glasgow 式；页码来源 ncx 的 pageList；第一篇第 1–3 章正文 11 / 5 / 8 段，页码 109–117、117–121、121–126；全书 5721 段（正文 3663、注释 149、导读 298、前后附 1420）。
  - 中文：没有页码；第 1–3 章正文 11 / 5 / 8，旁注 10 / 5 / 7，注释 6 / 1 / 1（含译者按）；全书导读 187 段、旁注 1983、注释 880。
  - 对齐：第一篇 1–3 章三章全部对齐；全书 35 章里 19 章对上、16 章段数不同（见「已知问题」）。
- **Cannan 夹具没做成 Cannan 版**：Project Gutenberg 上没有 Cannan 版（只有 #3300 和带 Garnier 注的 #38194），Online Library of Liberty 拒绝自动下载（CloudFront 403，没有绕）；用户说「不用下载」。改用 PG #3300（公有领域）的第一篇第 1–3 章，按 Glasgow 分段拆成 11 / 5 / 8，去掉 PG 的商标和许可说明，来源写在 `fixtures/wealth/README.md`。为了下载 PG，经用户批准新建了个人云环境「pith」（放行 gutenberg.org、oll.libertyfund.org，带 Tauri 系统库的 setup script；setup script 要等下次新会话才真正跑一遍）。
- 改坏测试（`tests/flow.rs`）：引文改一个字、挂错段落（提示在哪一段找得到）、不存在的段落号（原文拦下、解释去锚点标「没挂上原文」）、标成原文却没有锚点、推演混进原文类（文字和引文不一致；编造的引文）、旁注 / 导读被标成原文（译本旁注、英文编者导言）、他人观点没有来源、他人挂到原著正文、卡片种类不对——全部拦下；审计查出存下的结果被改（引文、锚点、状态、页码、关系、存下的文本）。
- 四种入口 + 反方（只查文献库 / 再上网，一个 404 记原因）+ 推演 + 自由，录制回复跑到核对、审计、三种导出；取消后接着做、第二步出错停下再接着做（第一步不重做）、密钥错整次停下；发送前估算。
- App 一侧（`concept/tests.rs`）：两本夹具 EPUB 作附件 → 解析 → 存（导入前备份）→ 关联对齐 → 读回逐段一致；检索（英文词、中文词映射回原著、旁注里的词）；搜索表被清空后自动重建；从 v7 升级；研究存库读回一致；Word 搜索窗的原文（英文带页码、中文只有段落号）；进行中的标成已停止；OpenAI Responses（请求形状、缓存命中计价、272K 以上加价、截断 / 拒答照样记账、401 不回显密钥、余额不足不重试、503 重试、超预算不发）；OpenAI 网页搜索（工具、引用、每次 $0.01）；Mathpix（multipart 上传、查状态、lines.json + mmd、DELETE、按页记账；出错照样 DELETE 并按页估计记账；密钥错不记钱；超预算不上传）；端到端：真实的 Responses / 网页搜索 / 下载器连本地假服务器，跑整章分析和反方（文献库 + 上网），存库、审计、三种导出、加入文献库（再加一次连上已有的），两次研究的步骤费用合计 = 账本合计。
- **浏览器 `?mock`**：`pnpm dev` 起得来，用 Playwright 无头浏览器点了一遍：书架 → 书 → 章（中英并排、旁注 / 导读样式、页码）→ 点段落号选两段 → 读一段 → 确认框 → 进度 → 结果；整章分析的关系图点节点、点锚点看原文 → 去原文（引文高亮）；卡片上「反方与后世评论」打开「上网找」；没对齐的第三章；问这本书、追踪概念的检索和勾选；整章分析去掉「关系」、跑到一半停止、继续；设置的「概念研究」「密钥与费用」；文献库 PDF 附件「解析成文本」→ Mathpix（模拟）→ 预览 → 存；Word 搜索窗选书 → 卡片里的原文 → 定位自动填「101 (I.i.1)」。控制台 0 个错误。**没起 `pnpm app:dev`**（Tauri 窗口，云端没有桌面）。

#### Linux 上没编、没测的（macOS 部分，没改）

- 钥匙串（`secret.rs` 的 `keychain` 模块，Linux 上是返回错误的桩）：两个新条目 `openai-api-key`、`mathpix-api-key` 在正式版里能不能存取，C4 在签名过的 App 里试。
- Word 模板安装、`Pith.bas` 宏、`customUI.xml`、AppleScript / 桥接脚本：一行没动。
- `.app` 打包没试。

#### 留给本地 C4 的事

1. **宏插入原文**：搜索窗交回的引文条目多一个字段 `pithQuote: { text, lang, paragraph, pages, card }`（跟着 locator / label 一起，经 `bridge.rs` 的 `OK\nP\t<json>` 交给宏）。宏要：在引文控件前面插入「“原文”」；确认文档数据部件（`urn:pith:citations:v1`）里多出来的字段保留、刷新时不丢；切换样式、刷新都正常。英文带页码和段落号、中文带段落号，在 Mac Word 里各看一遍；GB/T 著者-出版年制不出定位的问题定一下。
2. **钥匙串接线**：上面那两个条目；设置页「密钥与费用」里 Mathpix 是两个框（app_id、app_key），存成一个条目 `app_id:app_key`。
3. **OpenAI Responses 真实调用要核对的点**（官方文档云端访问不了，照记忆和录制样例写）：
   - 地址 `POST https://api.openai.com/v1/responses`、`Authorization: Bearer`；
   - 请求：`instructions` + `input`（一条 user 消息，`input_text`）、`text.format = {type: "json_schema", name: "concept_cards", schema, strict: true}`（schema 里可空字段写成 `["string", "null"]`，严格模式认不认）、`reasoning.effort`（`gpt-6.1-sol` 认不认 `xhigh` / `max`）、`store: false`、`max_output_tokens`（推理 token 算不算在内）；
   - 回复：`status`（`completed` / `incomplete`）、`incomplete_details.reason`（`max_output_tokens`）、`output[]` 里 `message` 的 `output_text` / `refusal`、`model`（带日期的快照名按设置的模型计价）、`usage.input_tokens` / `input_tokens_details.cached_tokens` / `output_tokens` / `output_tokens_details.reasoning_tokens`；错误 `error.code`（`insufficient_quota`）；
   - 网页搜索：工具名是 `web_search`（还是 `web_search_preview`）、`tool_choice: "required"`、`include: ["web_search_call.action.sources"]` 能不能拿到搜过的网址、`url_citation` 注释、每次 $0.01 和搜到的内容按输入 token 计；
   - 选 Opus 5.5 时：走 v0.3 的 `research/anthropic.rs`（effort medium、`output_config.format` 用同一份 schema），要试一次；
   - 单价核对：输入 $2、缓存 $0.10、输出 $10、272K 以上 2 倍 / 1.5 倍（`concept/responses.rs::cost`）；Mathpix 每页 $0.005。
4. **Mathpix 真实调用要核对的点**：上传是 multipart（`file` + `options_json`，现在发 `{"conversion_formats": {}, "rm_spaces": true, "include_line_data": true}`）、回 `pdf_id`；`GET /v3/pdf/{id}` 的 `status`（`split` / `processing` / `completed` / `error`）、`num_pages`、`percent_done`；`GET /v3/pdf/{id}.lines.json` 的结构（`pages[].page`、`lines[].type` 有哪些、`text`、`region`、`parent_id` 是不是按段落分组、`conversion_output`）；`.mmd`；`DELETE /v3/pdf/{id}`。由行拼段落的规则（`concept_engine::mathpix` 文件头）用真实 PDF 调一遍，特别是跨页续段和页眉页码的类型名。
5. 真实 GPT-6.1 Sol 跑分工三章（事先问上限）、短 PDF 经 Mathpix、三种导出在 Mac 上打开、用户抽查卡片；`pnpm app:dev` 里把概念页点一遍（WebKit 里划选、SVG 关系图的样式）。
6. 文档并入：本记录的「工程选择」并进 `decisions/0024`（或新开一篇）、`architecture.md`（目录、命令、v0.4 一节）、`testing.md`（概念研究的测试和命令）、`schema.md`（0008）。

#### 已知问题、没做的

- **重新解析**没做：「解析成文本」再做一次是存成第二份文本（旧文本和它的卡片照留），没有「按引文给旧卡片重新找锚点、找不到的列出来」。
- **全书对齐**只保证了第一篇第 1–3 章：35 章里 16 章中英段数不同（I.iv、I.v、I.vii、I.viii、I.x、I.xi、II.ii、II.iv、III.iii、第四、五篇多数章），原因多半是版本的分段、表格、Penguin 第二册的结构不同，没逐章查。Penguin 第二册的编者「Introduction」被认成了正文单元（`Intro`，和斯密的引言同名，段落号后面加了 `#2`）——它的目录项就叫「Introduction」，规则分不开。
- 中文版几个篇名单元（第一到第五篇）下面有少量段落被算成了正文（4 / 4 / 9 / 7 / 3 段），没逐一看是什么（可能是没有单独目录项的导读或篇的说明）；导读只认目录里写了「导读」「导言」的单元。
- 「问这本书」的本地检索是按词的 FTS（中文二元组、西文前缀），长问句要拆关键词；没有做语义检索。
- 每次读一章 / 打开一次研究都从库里读整本书（Penguin 约 6000 段），本机应该在 100 ms 以内，没测。
- 概念研究的网页下载没有开「下载失败改用 Claude 网页抓取」（v0.3 的后备）。
- `?mock` 里各技能的结果是预先生成的（`mockConcept.json`），换章、换范围时内容不跟着变（真实 App 的开发版用模拟模型现编）。

#### 等用户

- C4 之前（照 `0024`「等用户」）：OpenAI 密钥（能用 `gpt-6.1-sol`）、Mathpix 的 app_id + app_key、一份短 PDF（放 `local/concept-pdf/`）、真实验收的花费上限。
- Cannan 版：要的话请从 OLL 手动下载第一卷的 HTML / PDF 放进 `local/`，本地再换夹具（现在用的是 PG #3300，测试只依赖分段和结构，不依赖版本）。
