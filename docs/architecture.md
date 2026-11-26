# 架构

> 代码怎么分、工程上的规矩。产品架构（四个入口、知识库、对话 Agent）在 `decisions/0030`，1.0.2 的界面和存储在 `0031`，1.0.3 的控件和文献库在 `0032`；表结构在 `docs/schema.md`，测试在 `docs/testing.md`。

## 目录

```
pith/
├── src/                        前端（React 19 + TypeScript + Tailwind v4）
│   ├── main.tsx                入口；?mock 时装上浏览器内的假后端；?window=picker 是置顶搜索窗；关掉文本框的系统输入辅助（shared/plainInputs.ts）
│   ├── index.css、fonts.css    样式入口：先引 vendor/pith/pith.css，再把 shadcn 命名的变量指到 pith 的 --color-*（对照在 vendor/pith/DESIGN.md 第 9 节）；
│   │                           fonts.css 登记随 App 打包的 Lato、Newsreader（@fontsource）；明暗跟着系统写根上的 data-theme（shared/theme.ts）
│   ├── vendor/pith/            视觉方案 pith 的已发版本（Atelier 按 tag 同步：pith.css、bundle.css、tokens.ts、DESIGN.md、marks/、App 图标 PNG），
│   │                           带 VERSION、SHA256SUMS，不手改（CLAUDE.md 规则 9）；App 图标由 scripts/app-icon.sh（pnpm app:icon）缩到 macOS 网格再出 src-tauri/icons/
│   ├── styles/app.css          外框和几屏的版面（照 Explore R1，跟着窗口伸缩）；组件的样子（含下拉框、勾选框这些控件）和动效零件在 vendor/pith/bundle.css
│   ├── app/                    外框（`0031`）：App（顶栏 + 一屏；⌘K、⌘\、⌘N、两指横滑：文献库里叫标签树、别处叫对话历史；阅读器占满窗口；存卡的种子、换配色的波纹）、nav（几屏之间跳、技能清单）、
│   │                           water（换配色的整页水面：原生截图 + WebGL 着色器，`0033`）、
│   │                           TopBar、History（对话历史的枝桠）、Palette（⌘K）、branch（枝桠的画法，历史和设置共用）、SideNav（技能左边那一列）
│   ├── chat/                   对话：Home（问候、叶子、最近照料的五份材料）、Composer（/ @ #）、ChatScreen（一段对话；技能放进对话：调查、概念、排版原来那一页）、
│   │                           Reply（状态行、编号和页边注、反问、交付卡和「存进」）、Usage（输入框下面一行：模型名、本月用量的小圈和明细，1.1.0）
│   ├── kb/KnowledgePage.tsx    知识库：两栏（知识库 | 种类、搜索；生长阶段）、卡片墙（十种记录各一种样子）、右边的面板、「种进来」
│   ├── library/                LibraryScreen（页头、两栏：范围 | 类型、搜索；集合；标签筛选；右边的面板）、Collections（集合一栏，子集合点开接在后面）、
│   │                           TagTree（标签树，两指往右滑）、ItemList（表格、排序、多选、右键菜单、「新建」）、ItemDetail（可编辑详情、批量面板、合并）、scope、labels
│   ├── settings/               SettingsScreen（枝桠 + 八页；角色、知识库、外观是新的，导入 1.0.3 从文献库挪来）、SettingsPage（原来八页的内容块：数据目录与备份、Word、密钥、三个技能……）、
│   │                           Transfer、WordSettings、CiteSettings、ResearchSettings、ConceptSettings、AgentSettings；
│   │                           ModelSettings（模型与费用：本月用量、默认模型、各家密钥、Vertex，1.1.0 重画）
│   ├── shared/                 ipc.ts（带类型的 api 对象，唯一的 invoke 入口）、ui.tsx（弹窗、右键菜单、提示条、就地编辑、标签输入）、
│   │                           controls.tsx（下拉框 Select、弹出列表 Menu、退场 usePresence、横排滚轮 useWheelX；不用系统的 <select>）、
│   │                           marks.tsx（内联 vendor/pith/marks 的图形）、masonry.ts（知识库的瀑布流：绝对定位，宽变了卡片滑过去，不用 CSS 多栏）、theme.ts（配色和明暗，记在 localStorage；换的时候用 View Transitions 洇开）、plainInputs.ts
│   ├── picker/                 置顶搜索窗（v0.2 W2）：Word 里插入 / 编辑引文时选文献、填页码；v0.4 加「卡片里的原文」和页 / 段定位（conceptLocator）
│   ├── dev/                    `pnpm dev` → http://localhost:1430/?mock 的假后端，打包时剔除：mockIpc（文献库等）、mockChat（对话、知识库，内容照 R1）、mockModel（模型和用量）、
│   │                           mockResearch / mockConcept / mockAgent（三个技能，数据由各自的引擎生成）；perfProbe（?perf 量换配色的帧）
│   ├── concept/                概念研究（v0.4，放在对话里）：ConceptPage（书架 → 一本书 → 一章 → 研究）、ConceptSidebar（书的目录）、Reader（原文段落、中英并排、划选）、
│   │                           Entries（问这本书、追踪概念）、Confirm、RunView（卡片、关系图、导出）、ParseDialog（附件解析成文本）
│   ├── research/               事实调查（v0.3，放在对话里）：Confirm（两次确认框）、RunScreen（改计划、进度、停止 / 接着做）、Result（短答、点数字看原文、全过程、导出、重查对比）
│   ├── format/                 排版（v0.2，放在对话里）：FormatPage（规范 → 一次排版）、FormatSidebar、RulesetView、RunView、SendConfirm、parts（技能页共用的 Page、Notice 等）
│   ├── cite/                   引文排版（v0.2）：renderer（听 cite-render 事件、交回结果）、engine（citeproc-js；GB/T 页码在方括号外、前后缀）、
│   │                           runs（HTML → 带格式片段）、resources（内置样式打包、导入样式向 Rust 要）、styles.json（内置样式清单，Rust 也读）、
│   │                           csl/ 与 locales/（官方 CSL 文件，来源见 SOURCES.md）、*.test.ts（node --test）
│   ├── import/ImportPage.tsx   导入：选来源 → 预览 → 导入 → 报告；导入记录
│   ├── metadata/               联网补全的界面：首次说明与同意、后台进度与结果提示、按 DOI / ISBN 添加、详情里的待确认、设置
│   └── reader/                 阅读器（单独打包，第一次阅读时加载）：Reader（工具栏、浮动工具条、查找）、
│                               controller（pdf.js 的 PDFViewer、标注层、选区 → PDF 坐标）、AnnotationList、useRead
├── src-tauri/                  Rust 后端（唯一读写 SQLite 的地方）
│   ├── migrations/             0001_library … 0010_chat；顺序编号，用过永不改
│   ├── word/                   Word 一侧的源文件：Pith.bas（宏，ASCII）、customUI.xml（选项卡）、images/（选项卡图标 PNG）、Pith.applescript、pith-bridge.sh
│   ├── fixtures/               冻结的样本库 library-vN.json（每个数据库版本一份，升级测试用，`decisions/0013`）；agent/（合成的示例规范与模拟模型的清单）；
│   │                           research/（虚构「示例市」的网页、统计表原件，录制的 Anthropic / DeepSeek / Tavily 回复）
│   └── src/
│       ├── lib.rs              AppState（启动：建目录 → 打开 → 升级 → 当天自动备份 → 清理没人用的附件文件；整库替换）、命令注册
│       ├── paths.rs            正式 ~/Pith/、开发 ~/Pith Dev/、PITH_DATA_DIR 覆盖
│       ├── db.rs               打开（WAL、外键）、migration、数据库比 App 新就拒绝
│       ├── backup.rs           VACUUM INTO 快照、daily / manual / before-*、清理规则；恢复用的检查、整库复制（backup API）；
│       │                       还有备份用到的附件文件不删（sweep_files）
│       ├── common.rs           Tx（SAVEPOINT 嵌套事务）、ULID uid
│       ├── gateway/            模型网关（1.1.0，`decisions/0036`）：mod（中立的消息和片段、Model 接口）、catalog（每家三档、价格表）、
│       │                       anthropic / openai / deepseek / gemini（四条线协议的编码和流）、http（地址、认证、重试、能停）、vertex（项目、gcloud ADC）、
│       │                       keys（几把密钥放哪）、meter（月上限、记账）、mock（模拟模型）、commands（`model_*`、`usage_summary`）
│       ├── chat/               对话、角色、知识库（1.0.2，`0031`）：store（0010 的五张表；内置四个角色；对话历史并上以前的调查和研究；能种进知识库的东西）、
│       │                       mock（模拟的回答，1.1.0 换成 Agent 循环）、commands（`chat_*`、`role_*`、`kb_*`）
│       ├── library.rs          文献库只读查询（概览、列表的范围 / 搜索 / 筛选 / 排序、四种详情、集合树、标签、附件打开位置）
│       ├── edit.rs             写操作：编辑、新建、标签、集合、批量、回收站、合并、添加附件
│       ├── fields.rs           条目字段规则：由字段推出常用列（导入与编辑共用）
│       ├── dedup.rs            去重提示与「不是重复」
│       ├── search.rs           搜索索引（search.db，FTS5 + 中文二元组）
│       ├── local/              本机的向量和重排模型（v0.5）：清单、llmctl.sh 启停、embed、rerank
│       ├── reader.rs           阅读：标注（合并矩形、排序键、摘录文字）、阅读位置
│       ├── metadata/           联网补全：mod（设置、比较与合并、记录、撤销、待确认、后台队列、按标识新建）、
│       │                       net（doi.org / Open Library、节流与重试、换成 Zotero 字段）
│       ├── protocol.rs         rl-file:// 协议：按附件 id 给文件库里的 PDF / 图片
│       ├── snapshot.rs         app_snapshot：让 WKWebView 拍一张网页当前的样子，原始像素给换配色的水面（1.0.4，`decisions/0033`；只在 macOS）
│       ├── store.rs            附件文件库：按 SHA-256 存一份，写入后只读
│       ├── text.rs             标题规范化、语言判定、日期解析、DOI / ISBN、HTML → 纯文本
│       ├── import/             mod（写入工具、导入记录、核对）、zotero（Zotero 副本）、file（文件写入层）、
│       │                       formats/（bibtex / ris / csl_json 解析 → 统一 Record）
│       ├── transfer.rs         整库导出（通用表转储 + 摘要 + 自检）、从导出 / 备份准备恢复用的临时库（`decisions/0013`）
│       ├── word/               Word 集成（v0.2，`decisions/0016`、`0017`）：service（本机服务）、bridge（宏协议）、picker（置顶搜索窗）、
│                               render（排版桥）、csl（文献 → CSL-JSON、Zotero 条目 key → 文献库）、install（模板与脚本的安装）、
│                               styles（内置 + 导入的 .csl）
│       ├── agent/              排版 Agent（v0.2 W3，`decisions/0018`）：claude（调用 Anthropic Messages API、模拟模型）、usage（用量账本、月度预算）、
│                               secret（密钥：正式版钥匙串，其余内存或文件）、extract（提炼要求、出处核对、数值换算、合成规范）、
│       │                       planner（论文 → 给模型看的文字、文档计划的核对与预览）、apply（排版、校验、只检查项、报告）、store（成果存档）
│       ├── concept/            概念研究（v0.4，`decisions/0024`、`0025`）：commands（`concept_*` 命令、设置、组装接口、后台执行）、store（concept_* 表）、
│       │                       search（段落进 search.db）、responses（OpenAI Responses + 网页搜索）、mathpix、library（文献库作反方来源、加入文献库）、keys
│       └── research/           事实调查（v0.3，`decisions/0020`、`0022`）：commands（`research_*` 命令、设置、组装接口、后台执行）、store（存进 research_* 表）、
│                               anthropic（流式、网页搜索、pause_turn；ClaudeSearch；ClaudeFetch：下载失败时的网页抓取）、openai（OpenAI 兼容，DeepSeek）、tavily、meter（发送前按最坏情况查月上限、记账）、
│                               download（本机下载：限速、拒内网、自己跟跳转；WithFallback：失败了改用 Claude 网页抓取）、content（HTML / PDF / XLSX / CSV / DOCX 取正文）、snapshot（快照文件）、
│                               docx（Word 导出）、library（加入文献库）、fixtures / testing（合成原件、本地假服务器）
├── crates/zotero-import/       Zotero 只读读取 + 审计 + 合成夹具（src-tauri 以 path 依赖引用）
├── crates/research-engine/     事实调查的纯逻辑（只依赖 serde）：流程、提示词与解析、核对、认数与精确十进制、计算、可信度、结论核对、
│                               重查对比、报告与渲染（Markdown / 单文件 HTML）、审计；合成夹具与模拟接口（README 有模块表）
├── crates/concept-engine/      概念研究的纯逻辑：EPUB 与 Mathpix 行 → 文本（角色、段落号、页码）、对齐、技能（SKILL.md）、流程、提示词与 Schema、
│                               核对与审计、关系图布局与 SVG、渲染；夹具与模拟接口（README 有模块表、角色规则和 SKILL.md 格式）
├── skills/                     内置技能（v0.4）：chapter、passage、ask、trace、critics、counterfactual、free，各一个 SKILL.md
├── crates/docx-engine/         Word 文档引擎（src-tauri 引用）：排版器 + 独立校验器 + 文档计划（0-C）、structure / verify_structure（结构类排版与它的独立核对，W4）、
│                               outline（结构摘要、表格清单、计划预览、要求文档转文字）、
│                               checks（只检查项的统计）、cite（引文写入与核对、Zotero 引文识别与转换、.csl 检查、Node 里排版）、
│                               vba（生成 vbaProject.bin）、template（生成 .dotm）
├── poc/
│   └── word-addin/             0-B：citeproc 引文引擎、Word 外直写、宏桥接、Office.js（保留待用）；独立 pnpm 包
├── scripts/                   webkit-perf.swift（WKWebView 小窗跑 ?perf 量帧，docs/testing.md）、install-app.sh（打包、签名、装进「应用程序」）、app-icon.sh（App 图标缩到 macOS 网格再出 src-tauri/icons/）、clean-build.sh（清编译缓存）、phase0-check.sh（Phase 0 端到端自检）、cite-render.ts（Node 里排版，测试与命令行用）、
│                               word-icons.mjs / .swift（生成 Word 选项卡图标）
├── local/                      本机私有（真实论文、样例、证书），gitignore
└── docs/
```

根目录是 Cargo 工作区（`src-tauri`、`crates/zotero-import`、`crates/docx-engine`、`crates/research-engine`、`crates/concept-engine`），编译输出在根 `target/`。

## 规则

- 只有 Rust 读写 SQLite；前端只经 `shared/ipc.ts` 调用，命令名 `模块_动作`（`items_list`、`backups_create`）。业务规则只写在 Rust。
- 命令返回 `Result<T, String>`，错误是给用户看的中文；数据库打不开时 App 照常启动，界面显示原因。
- 一次改多表用 `Tx::begin`；会改数据的导入类命令先做 `before-import` 备份。
- `search.db` 只存派生数据，以 `search` 名 ATTACH 到主连接；版本不对就清空重建，启动时比对文档集合，不一致就重建；不写 migration。
- 附件按 SHA-256 存放，界面通过自定义协议 `rl-file://localhost/<附件 id>` 取文件，不暴露绝对路径；只给 PDF 与图片（`decisions/0011`）。
- pdf.js 资源（字体编码表、替代字体、wasm）由 `vite.config.ts` 的 `pdfjsAssets` 在开发时直接给、打包时复制到 `dist/pdfjs/`。
- pdf.js 视图里一律 `box-sizing: content-box`（`reader.css`；Tailwind 基础样式是 border-box，会让画布比文字层小一圈，高亮错位）。开发版每页渲染后自检文字层与画布是否一样大，不一样就在控制台警告。
- 开发用 `~/Pith Dev/`（调试构建自动选），测试用临时目录，正式 `~/Pith/` 开发期间不碰。
- 三种版本（照 Pomelo，`decisions/0018` 第 13 条）：开发版 `pnpm app:dev`；打包版 `pnpm app:build`（配 `PITH_DATA_DIR` 测试）；正式版 `pnpm app:install`，用固定的自签名证书「Pith Self-Signed」签名后搬进「应用程序」。证书建一次：钥匙串访问 › 证书助理 › 创建证书（自签名根证书、代码签名）；不必设成受信任。固定证书让系统把每次更新认成同一个 App，钥匙串里的密钥更新后还能读（新版第一次读时系统会问一次登录密码，点「始终允许」）。
- 编译输出 `target/` 只增不减：超过 20 GB 时 `pnpm check` / `app:dev` / `app:build` / `app:install` 自动清（`scripts/clean-build.sh`），下次从头编。
- 退出时把 WAL 日志并回 `library.db` / `search.db`（`AppState::close`），退出后的 `library.db` 单独拷走也是完整的。
- 文本框一律关掉 macOS 的自动更正 / 自动大写 / 拼写检查 / 写作建议 / 自动填充（`index.html` + `shared/plainInputs.ts`），作者名、标题、DOI 原样保存。不用 `<datalist>`（macOS 上和输入框一样窄），标签提示用 `TagInput`。
- 只影响显示的界面偏好（标签栏开合、「含子集合」）用 `useStoredState` 记在 WebView 的 localStorage，不进数据库、不进导出。

## 命令

- 命令清单以 `src/shared/ipc.ts` 为准（前端唯一的 invoke 入口），Rust 一侧在 `lib.rs` 注册。命名 `模块_动作`：文献库 `items_*` / `attachments_*` / `tags_*` / `collections_*` / `trash_*`，导入 `import_*`，阅读 `reader_*` / `annotations_*`，联网补全 `metadata_*`，备份与导出 `backups_*` / `export_*`，Word `word_*` / `cite_*` / `picker_*`，排版 Agent `agent_*` / `format_*`，事实调查 `research_*`，概念研究 `concept_*`，对话、角色、知识库 `chat_*` / `role_*` / `kb_*`。前端用到的和 `lib.rs` 注册的一一对应（1.0.2 清掉了没人调的）。
- 数据库命令都是 `#[tauri::command(async)]`，在线程池里跑；会改内容的命令在提交后更新搜索索引。
- 要花钱的都是两段：`*_prepare` 先算出要发什么、多少钱（不联网），用户确认后再调发送的命令（如 `format_plan_run`、`research_execute`）。
- 列表里的行用 `targets`：`{ kind: item | attachment | note | collection, id }`。选择文件夹 / 文件用 `@tauri-apps/plugin-dialog`（`ipc.ts` 的 `pickFolder` / `pickFile`）。

## 对话、角色、知识库（1.0.2，`decisions/0031`）

- **存**：`chat::store`，migration 0010 的五张表（`docs/schema.md`）。消息的 body 是 JSON：你说的 `{text, role}`；Pith 的 `{steps, paragraphs, cites, ask, delivery, mock}`，前端照它画状态行、正文（`[n]` 是编号）、页边注、交付卡。1.1.0 的 Agent 循环照这个形状写消息。
- **角色**：内置四个在 `chat::store::BUILTIN`，改过的用同一个 uid 存一份，删掉就是改回默认；新建的 uid 是 ULID。
- **技能进对话**：`conversations.skill` + `ref_uid` 连到那次运行（research：调查 uid；concept：研究 uid；format：`规范 id:排版 id`）。点名了技能的对话先只存你说的那句，技能跑起来以后 `chat_link`。以前的调查和研究没有对话：`chat_list` 把每个问题最新的那次调查、每次研究也列出来（`uid` 是空的），接着问时 `chat_open_ref` 才建对话。
- **知识库**：材料按 `(库, source_kind, source_ref)` 去重，再种一次只是更新「最近照料」。`kb_candidates` 列出能种的：解析好的书（`concept_text`）、做完的调查（`research_run`）、做完的研究的卡片（`concept_card`，`研究 uid#卡片 key`）。`kb_material_add` 不给库时种进第一个，没有就建「随手存」。
- **模拟的回答**：`chat::mock::answer`，在勾的知识库（没勾就全部）里按问题里的字两两一组粗略地挑三份当出处，出状态行、段落、反问（伴读、审稿人才有）、交付卡，标 `mock: true`。
- **界面**：外框的状态在 `app/nav.tsx`（哪一屏、哪段对话、设置哪一页、文献库要看的）；配色和明暗在 `shared/theme.ts`（localStorage，跟随系统时听系统的变化）；图形只从 `shared/marks.tsx` 取（内联同步来的 SVG，类名留给 `bundle.css` 的动效零件）。

## 模型网关（1.1.0，`decisions/0036`）

```
对话（第 2 期的 Agent 循环）─ Request（系统几块、工具、对话、上限、effort）─▶ meter::send：按最坏情况查月上限
   ─▶ Model::send：HttpModel（四家编码 → POST 流式 → 读流线程 → 每家的流解析 → Reply）或 MockModel（脚本 / 脑子）
   ─▶ 记一行账（回报的模型 × 价格表，含缓存读写；对话 uid）─▶ Reply（中立的消息、停的原因、用量）
```

- 选模型：设置里一家 + 一档（`model.prefs`）；`catalog::tier(家, 档)` 给模型和 effort。Vertex 的项目、区域在 `model.vertex.*`，令牌由 gcloud 换。
- 私有数据（思考签名、密文、Gemini 的 thought signature、Anthropic 回退的块）只回放给同一来源；换了模型就丢掉。
- 开发版、测试、`?mock` 用模拟模型（不联网、不花钱，账本记「模拟」）；正式版或 `PITH_REAL_MODEL=1` 用真接口。

## 恢复的流程（`decisions/0013`）

1. 准备（不占锁）：来源（备份 / 导出）→ 临时库 → 检查、核对 → 升到最新版本。
2. 替换（在锁里）：停下联网补全、库「代号」加一 →「恢复前」备份 → 导出里的附件放进文件库 → backup API 整库复制进主库 → 重建搜索索引。
3. 主库打不开时：原文件挪进备份目录（「恢复前」），在新建的库上恢复，失败就挪回去。

## 导入的流程（`decisions/0009`）

1. 预览：读来源、数数量、列缺失文件与可能重复，不写库。
2. 导入：`before-import` 备份 → 附件复制进文件库（记下新写的文件）→ 一个事务写入 → 写入核对（来源 ↔ 库，逐类逐条）→ 写 `import_runs` → 提交。
3. 任一步失败：事务回滚，删掉这次新复制的文件，界面显示原因；导入前备份保留。

## v0.2 Word Studio（`decisions/0006`–`0008`、`0015`、`0016`）

```
Word「Pith」选项卡 ─▶ Pith.dotm 里的宏（VBA，只读写 Word；Add/Edit Citation 时 Pith 弹置顶搜索窗，请求一直等到选完）
   ─AppleScriptTask─▶ Pith.scpt ─▶ Pith-bridge.sh（读 service.json；Pith 没开就后台启动）
   ─curl + 令牌─▶ 本机服务 127.0.0.1:<随机端口>（word::service）
   ─▶ word::bridge：刷新计划（docx_engine::cite::plan）+ 文献库 CSL（word::csl）
   ─cite-render 事件─▶ WebView 里的 citeproc-js（src/cite）─cite_render_done─▶ 回到 Rust
   ─按行文本─▶ 宏照着改引文控件、书目控件、数据部件
```

- Word 外：`docx_engine::cite::docx::refresh` 直接改 .docx 副本，写完用 `cite::check`（roxmltree）独立核对；服务的 `/v1/docx/refresh`、`/v1/docx/verify` 调它。
- Zotero 引文：Word 里 Convert Zotero（宏收集域代码 → Pith 给每处新 ID 与数据 → 宏把域换成控件）；Word 外 `cite::convert`（设置页、服务的 `/v1/docx/convert-zotero`）。两条路共用 `cite::zotero` 的解析与按条目 key 对应。
- 样式：内置的在前端打包；导入的 .csl 在 `数据目录/styles/`，ID 是 CSL `<id>`；Word 里 Style 菜单（dynamicMenu）每次打开问 Pith 要清单。
- 文档里的数据：引文控件 `pith:cite:<ID>`、书目控件 `pith:bibliography`、数据部件 `urn:pith:citations:v1`（0-B 旧名照读、刷新时换新）。
- 模板与 VBA 工程由 `src-tauri/word/` 的源文件生成（`docx_engine::vba`、`template`），设置 › Word 安装。

### 排版 Agent（W3，`decisions/0008`、`0018`）

```
规范 / 模板 / 附件 ─extract::load_docs─▶ 文字 ─format_extract_prepare（估费用，不联网）─▶ 确认框
   ─format_extract_run─▶ Claude Opus 5.5（结构化输出）─▶ 要求清单 ─Rust 核对摘录、换算数值─▶ format/<id>/ruleset.json
用户逐条确认（开关、改值）─extract::assemble─▶ 规范 Spec（每个值记着来自哪条要求；没规定的不进规范）+ 排版程序的默认做法
论文 .docx ─outline::read─▶ 结构摘要 ─planner::render（结构和片段 / 全文）─format_plan_prepare─▶ 确认框
   ─format_plan_run─▶ Claude ─▶ 计划草稿 ─Rust 核对样式名 / 正则 / 分节，outline::preview 算命中─▶ 用户确认
format_apply ─▶ 排版器改副本 ─▶ 校验器（另一套解析器）─▶ checks 统计只检查项 ─▶「原名（Pith 排版）.docx」+ 报告
```

- 联网只有两次模型调用，只在 Rust 里（`agent::claude`），只连 `api.anthropic.com`；每次发送前界面确认，费用记进 `agent/usage.jsonl`，超月度上限拒绝。
- 只有「应用程序」里用正式数据目录的 Pith 用真模型和钥匙串；开发版、测试、`PITH_DATA_DIR` 的打包版用模拟模型（`PITH_REAL_MODEL=1` + `PITH_API_KEY_FILE` 可显式改用真模型）。
- 模型的输出只是数据：结构由 JSON Schema 和 Rust 的反序列化定死，内容逐项核对后才用；排版、校验、统计都是确定性的。
- 结构类排版（W4，`decisions/0019`）：规范的 `structure`（页眉、页码、奇数页起、三线表、脚注编号、标题紧跟）由要求清单的设置合成；计划的 `layout`（各部分从哪段开始、单面 / 双面、不排三线表的表格）由模型提出、Rust 核对、用户在计划页确认。排版器在格式排完后调 `structure::apply`；`verify::verify_all` 放行的结构改动由校验器自己从规范和计划推出。
- W5（`decisions/0021`）：排完结构后，已经另起一页的段落不再加段前分页（`format::keep_single_page_breaks`）；表格里参考文献样式的段落按表格文字排；自带磅值缩进的样式和段落写明 0 字符，免得被正文的字符缩进盖掉（`format::keep_own_indents`）；规范没定参考文献对齐时保持原对齐。校验器各有一项独立核对（`verify::check_page_breaks`、`check_own_indents`）。
- 模型拒答时直接报错让用户重试，不换模型；论文默认按「结构和片段」发送，设置里可换成「全文」（`agent_send_mode_set`）。

## v0.3 事实调查（`decisions/0020`、`0022`）

```
问题 ─research_plan_prepare（估费用，不联网）─▶ 确认框 ─research_plan_run─▶ Opus 5.5（可带 Claude 网页搜索摸底）─▶ 计划（子问题、维度、注意事项）
用户删改子问题、加网址或文件 ─research_execute_prepare─▶ 确认框 ─research_execute（后台线程，事件 research-progress / research-done）─▶
   检索（执行模型的专用搜索：DeepSeek → Tavily；Anthropic → Claude 网页搜索）─▶ 本机下载 → 快照 + 正文
   ─▶ 摘证据（DeepSeek，JSON；记数据生产者）─▶ 核对（代码：摘录逐字、数值在摘录里、单位）─▶ 可信度排序（代码，按等级表）
   ─▶ 结论（Opus 5.5，用 {F3}/{C1} 占位，代码填数、核对模型自己写的数）─▶ 报告 ─▶ 导出 Markdown / HTML / Word、加入文献库、重查对比
```

- 分层：`crates/research-engine` 不碰数据库和网络，接口经 `ports` 的 trait 进来（模型、搜索、下载、存档、进度、取消）；App 这一侧（`src-tauri/src/research/`）实现真实接口并存库。`Run` 整份可序列化；每次保存在一个事务里整份换掉这次调查的行。
- 每步都存，可以取消；停下或出错后「接着做」从 `run.stage` 继续，搜过的不再搜、读过的不再读。启动时把异常退出时还在「进行中」的标成「已停止」。
- 核对两层：流程里核一次（核不上的不进结论）；`research_run_get` 每次对做完的调查从快照重新审计一遍（`audit`），有问题页面顶上标红。
- 数字只由代码写进结论；计算用精确十进制（`dec`），结果存字符串。中文数量词（「三成」）只标出、不核对。
- 带网页搜索的请求不加结构化输出：网页搜索的引用总是开着，而引用和 `output_config.format` 不能同时用（官方文档）；靠提示词 + 宽容解析。不带搜索的计划、结论用 JSON Schema。
- 费用：每次调用前按最坏情况（输入估计 ×1.1 + 输出写满 + 搜索用满）查月度上限（和排版 Agent 共用，默认 $30）；记账按接口返回的用量，缓存命中也按原价，宁多勿少；模拟接口记一行「模拟」不算钱。
- 下载：外来网址不下本机和内网；跳转自己跟、每一跳都检查；类型按内容判断；同一站点间隔 1.5 秒；User-Agent 只有 `Pith/版本`。
- 下载失败的后备（`decisions/0023`）：本机下载失败、网址是外网的 http(s)、设置开着并且有 Anthropic 密钥时，用 `web_fetch_20250910`（默认 Haiku 5.5）抓一次；拿到的文字或 PDF 照常存快照、取文字。费用按最坏情况查上限、记账，并记成「下载」那一步的一行；来源标 `fetched_via = claude_web_fetch`，报告写明。
- 快照：`数据目录/research/sources/<sha256 前两位>/<sha256>.<扩展名>` + `.txt`，只读、同内容一份，删调查后清理没人用的。
- 加入文献库：来源 → 网页 / 报告条目，原件作附件（附件 SHA-256 去重），同一网址已有的连上不重建；先做「导入前」备份。
- Word 导出不改文档引擎：`research/docx.rs` 用 `docx_engine::package::Package` 拼最小的 .docx（标题、表格、列表、外链、书签与文档内跳转）。
- 密钥：Anthropic 和排版 Agent 共用一把（`agent.key_saved`）；DeepSeek、Tavily 各一个钥匙串条目（`deepseek-api-key`、`tavily-api-key`）。和排版 Agent 一样，只有正式版用钥匙串和真接口。三把密钥和月度上限都在「设置 › 密钥与费用」。
- 命令（`research_*`，20 个）：status / settings_set / key_set / key_clear；questions_list；plan_prepare → plan_run；run_get；plan_update / source_add / source_remove；execute_prepare → execute；cancel；rerun；export / reveal；add_to_library；run_delete / question_delete。

## v0.4 概念研究（`decisions/0024`、`0025`）

```
文献库里一本书的 EPUB 附件 ─concept_text_prepare─▶ 解析（目录分单元 → 逐块认角色 → 段落号 → 页码）─▶ 结构预览 ─concept_text_save─▶ 存（导入前备份）
   译本：连上原著，按章对齐（段数相同的一段对一段；不同的用章内单调对齐，允许一对多、多对一和落空，v0.5）；段落进 search.db
   PDF：concept_text_prepare 算页数和费用 ─concept_pdf_convert─▶ Mathpix v3/pdf（lines.json → 段落，页码 = 页序；用完 DELETE）或本机取文字
入口（一章 / 划选的段落 / 问题 → 本地检索勾选 / 概念 → 中英检索词勾选 / 已有卡片）+ 技能 ─concept_run_prepare（不联网）─▶ 确认框（每步发什么、最坏费用）
   ─concept_run_execute（后台，事件 concept-progress / concept-done）─▶ 按技能的步骤：模型 / 查文献库 / 网页检索（v0.3 的下载器）
   ─▶ 每步的输出核对后才进研究（核不上的拦下）─▶ 卡片 + 关系 ─▶ 关系图、导出 Markdown / HTML / Word、Word 搜索窗里的「卡片里的原文」
```

- 分层同 v0.3：`crates/concept-engine` 不碰数据库和网络，接口沿用 `research_engine::ports`；App 一侧实现 OpenAI Responses、Mathpix、文献库、快照并存库。打开一次研究就从存下的文本重新审计一遍（`audit`），有问题页面顶上标红。
- **技能**：`SKILL.md`（YAML 小子集：名字、标题、入口、产出、步骤）；内置的在仓库 `skills/` 编进 App，用户的在 `数据目录/skills/`，同名覆盖内置的。步骤只有模型、查文献库、网页检索三种，不让模型自己反复调工具，所以发送前算得出每步发什么、最多花多少。
- **文本**：XHTML 用自己写的容错读取（`markup`）；角色不看 class 名，先按目录文字分单元、再逐块认（规则在 concept-engine 的 README）。引用名全书唯一：`I.i.3`、`I.i.3 旁注`、`I.i 注 1`、`Intro.5`、PDF 只有段 `5`；锚到译本的写 `译:I.i 注 1`。页码来源依次是 EPUB 3 page-list、ncx pageList、`page-map.xml`、正文里的分页锚点。
- **重新解析**（`concept_text_reparse`，`concept/reparse.rs` + 引擎的 `reanchor`）：附件已经解析过时，解析对话框确认后不存第二份，就地换掉这份文本的单元和段落（uid、id、译本关联都不变）。先做「导入前」备份，然后一个事务里：换文本和段落检索 → 重新对齐（它是原著就对每份译本，是译本就对原著）→ 受影响的研究逐句重找锚点、存回、重新审计（这份文本上的研究；它是原著当前用的译本时，还有原著上的研究）；中途出错整个回滚。重找的规则：原来挂的那一段原样还在（折叠后文字相同）就跟着它走，否则按逐字引文找；两种都按「段落号相同的新段落 → 同一单元里离原位置最近的 → 全书离原位置最近的」的顺序；能挂的角色照核对的规则；找到后用 `verify::check` 在新文本里重算位置、页码、中译；找不到的去掉锚点、标「没挂上原文」，`note` 以「重新解析后」开头并留着原来的段落号（下次重新解析时引文全书只有一处就重新挂上）。原文、他人、推演前提没挂上原文算审计问题，解释句不算。挂来源的他人句不动；研究范围里的段落号跟着换。EPUB 重新解析默认沿用原来的段落号写法。
- **四类句子与核对**：原文（只能挂原著正文段，文字就是引文，逐字核对）、他人（有来源，或挂译本的旁注 / 注释 / 导读）、解释（挂的段落要存在、带的引文要核得上，否则去掉锚点、标「没挂上原文」）、推演（前提按原文核对）。折叠在 v0.3 的基础上再容忍弯直引号、撇号样式和拉丁字母大小写。原文句的中译在对齐的中文段落里核对。一句不剩的卡片不要，关系两头都要在。
- **模型**：默认 GPT-6.1 Sol，走 OpenAI Responses API（JSON Schema 严格模式、`reasoning.effort`、`store: false`、`max_output_tokens`）；可在设置里换 Opus 5.5（走 v0.3 的 `research/anthropic.rs`）。专用搜索是 Responses 的 `web_search`，关掉就用 Tavily。费用和排版、调查共用月上限，账本用途记 `concept`。
- **Word**：搜索窗把原文放进引文数据的 `pithQuote`（原样存进文档的数据部件）；有页码的写 `110 (I.i.3)`（label page），没有的写 `I.i.3`（label paragraph）。桥接回复里多一行 `Q`，宏在新插入的引文前面加上「“原文”」（编辑已有引文时不加）。
- **快照**：评论来源的正文在 `数据目录/concept/sources/`，和事实调查的分开，不会被调查的清理删掉。

## 本机模型与语义检索（2026-11-15 修 `0025` 的已知问题）

- **本机模型**（`src-tauri/src/local/`）：用户的 `~/Documents/LLM`（和柚儿园共用）里的向量模型（8081，`/v1/embeddings`）和重排模型（8082，`/v1/rerank`）。Pith 只读清单 `config/models.json`（每类用第一个）、只经 `scripts/llmctl.sh` 启停，不改那个文件夹。用到时才启动（`ensure`：先看 `/health`，没开就 `start <key> --by pith`，最多等 45 秒）；只停自己启动的（`status` 里有 `By: pith`）：闲置 15 分钟停，退出 Pith 时停。只连 `127.0.0.1`，不走代理，不花钱。三种模式：`Real`、`Mock`（测试和 `PITH_LOCAL_MODELS=mock`：按字散列的 256 维向量）、`Off`（找不到文件夹或 `PITH_LOCAL_MODELS=off`）。`PITH_LLM_DIR` 可以另指位置。`AppState.local` 拿着它。
- **全书对齐**（v0.5，`concept-engine/src/align.rs`，算法第 2 版）：章内动态规划的单调对齐（1-1、1-2、2-1、1-3、3-1、落空），代价看长度比、两边都有的数字和拉丁字母词，可选调用方给的语义相似度（`align_with` 的 `Similarity`，App 现在没喂，只用长度和锚点；用户 11-25 定要补上，排在 1.1.0 的知识库检索里，`requirements.md`「以后」）。每章记怎么对上的（`exact` / `fitted` / `none`）和置信度，低的在页面上标「对齐存疑」。存 `concept_aligned_pairs`（migration 0009，主键三列，能存一对多）；`concept_alignments.algorithm` 旧的读书时重算。解析器同时修了归类（编者导读、书名页、脚注小目录、挂错层的篇）并把数据表格整张算一段（`PARSER_VERSION` = `concept-text-2`）；已存的文本要「重新解析」才用上新的归类。
- **向量是派生数据**：`search.concept_vecs(para_id, model, sig, vec)`，在 `search.db`，不写 migration。按模型的 key 分开存；`sig` 是段落文字的散列，段落换了（重新解析）就当作没有，重新算。归一化的 f32 小端连起来存，检索时整本书扫一遍算点积（一本书几千段，几十毫秒）。以后的内部知识库量大了再换存法。
- **建索引**在后台线程（`concept::semantic`，命令 `concept_index_start`，事件 `concept-index`），一批 16 段，读库和写库各拿一次锁、联本机服务时不拿锁。真实的两本《国富论》9265 段约 6 分钟。
- **「问这本书」**（`concept_ask_search`）：问题的向量 → 原著里最像的 40 段 + 译本里最像的 24 段（按对齐映射回原著）+ 按词检索的命中 → 重排模型打分 → 按相关度给前几段，只给正文段（旁注、注释算到它挂的正文段，编者的导读不要）。中文问题、有对齐的中译时重排比的是中译。退路：本机模型用不了、或索引还一段都没有 → 按词检索并说明原因；重排用不了 → 按向量相似度排。「追踪概念」仍按词（要的就是这个词出现在哪）。
- **反方读长文**（`concept-engine/src/excerpt.rs`）：来源超过 1.2 万字时不再只发开头，而是切成约 900 字的块，按和目标卡片、检索词的字面重合取前 40 块，本机重排模型（`Shelf::relevance`，App 一侧 `DbShelf` 实现）按意思再排，开头一块总带上，装满 1.2 万字，按原文顺序排回去，中间写「……（中间略）……」；余量接着开头往下装。发给模型的是片段，核对引文照旧用全文。费用上限不变（每条来源仍最多 1.2 万字）。
- 向量和重排只决定「给人和模型看哪些段落」，原文引用照旧由确定的逐字核对把关。

## 联网（`decisions/0004`、`0012`）

- v0.1 唯一的联网功能是联网补全；只在 Rust 里联网（`metadata::net`），界面不直接发请求。没打开时 Rust 拒绝联网。v0.2 W3 多了排版 Agent 调用 Claude（`agent::claude`，见上；每次发送前确认、有月度上限）。
- v0.3 事实调查（见上）：连 `api.anthropic.com`、`api.deepseek.com`（地址可改）、`api.tavily.com`，以及检索到的网页（本机下载，只取单页）。每次发送模型和搜索前界面确认、查月度上限。
- v0.4 概念研究（见上）：连 `api.openai.com`（或选 Opus 时的 `api.anthropic.com`）、`api.mathpix.com`（上传前确认页数和费用，用完删除），网页检索同 v0.3。
- 本机模型（见上）：只连 `127.0.0.1:8081` / `8082`，不出这台电脑。
- 1.0.2 的对话不联网（回答是模拟的）。
- 导入完成、改了 DOI / ISBN 后，命令层把文献放进 `AppState.lookups` 队列，后台线程一次一条地查：读库（拿锁）→ 联网（不拿锁）→ 写库（拿锁）并更新搜索索引。

## 资源约束

M3 Pro / 18 GB：不常驻解析服务或大模型；PDF 文本提取按需、可取消。本机的向量和重排服务各占约 1.6 GB，用到时才启动、闲置 15 分钟停。
