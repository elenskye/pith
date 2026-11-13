# 架构

> 依据 `decisions/0001`–`0003`、`0006`–`0017`，沿用 Pomelo 的工程约定。数据库结构见 `docs/schema.md`。

## 目录

```
personal-research-lab/
├── src/                        前端（React 19 + TypeScript + Tailwind v4）
│   ├── main.tsx                入口；?mock 时装上浏览器内的假后端；?window=picker 是置顶搜索窗；关掉文本框的系统输入辅助（shared/plainInputs.ts）
│   ├── app/                    App.tsx（外框：最左一列切换主页面，各主页面有自己的侧栏；阅读时阅读器占满窗口）、
│   │                           Rail（文献库 / 排版 / 调查 + 设置，`decisions/0023`）、SideNav（排版、调查、设置共用的侧栏外框与条目）
│   ├── library/                Sidebar（文献库视图、集合树、标签筛选）、ItemList（搜索、筛选、排序、多选、右键菜单）、
│   │                           ItemDetail（可编辑详情、批量面板、合并）、labels（中文名）
│   ├── shared/ui.tsx           弹窗（确认 / 输入 / 选集合）、右键菜单、提示条、就地编辑、标签输入（带提示）、useMutate、useStoredState
│   ├── settings/               SettingsPage（数据目录、数据库版本、备份列表：立即备份 / 恢复）、Transfer（整库导出、从导出恢复、恢复结果）、
│   │                           WordSettings（安装 / 卸载 Word 模板、服务与排版器状态）、CiteSettings（引文样式与导入 .csl、Word 外转换 Zotero 文档）、
│   │                           KeysSettings（密钥与费用：三把密钥、共用的月度上限）、ResearchSettings（事实调查：模型、专用搜索、网页抓取后备、单价、可信度等级表）；
│   │                           SettingsPage 分两组七页（`decisions/0023`）
│   ├── picker/Picker.tsx       置顶搜索窗（v0.2 W2）：Word 里插入 / 编辑引文时选文献、填页码
│   ├── shared/ipc.ts           带类型的 api 对象，唯一的 invoke 入口
│   ├── dev/mockIpc.ts          `pnpm dev` → http://localhost:1430/?mock，合成数据，打包时剔除；mockResearch.ts / .json 是调查页的假后端（数据由引擎生成）
│   ├── research/               事实调查页（v0.3）：ResearchSidebar（问过的问题和各次调查）、ResearchPage（新调查）、Confirm（两次确认框）、RunScreen（改计划、进度、停止 / 接着做）、
│   │                           Result（短答、点数字看原文、全过程、导出、加入文献库、重查对比）、labels
│   ├── cite/                   引文排版（v0.2）：renderer（听 cite-render 事件、交回结果）、engine（citeproc-js；GB/T 页码在方括号外、前后缀）、
│   │                           runs（HTML → 带格式片段）、resources（内置样式打包、导入样式向 Rust 要）、styles.json（内置样式清单，Rust 也读）、
│   │                           csl/ 与 locales/（官方 CSL 文件，来源见 SOURCES.md）、*.test.ts（node --test）
│   ├── import/ImportPage.tsx   导入：选来源 → 预览 → 导入 → 报告；导入记录
│   ├── metadata/               联网补全的界面：首次说明与同意、后台进度与结果提示、按 DOI / ISBN 添加、详情里的待确认、设置
│   └── reader/                 阅读器（单独打包，第一次阅读时加载）：Reader（工具栏、浮动工具条、查找）、
│                               controller（pdf.js 的 PDFViewer、标注层、选区 → PDF 坐标）、AnnotationList、useRead
├── src-tauri/                  Rust 后端（唯一读写 SQLite 的地方）
│   ├── migrations/             0001_library … 0007_research_fetch；顺序编号，用过永不改
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
│       ├── library.rs          文献库只读查询（概览、列表的范围 / 搜索 / 筛选 / 排序、四种详情、集合树、标签、附件打开位置）
│       ├── edit.rs             写操作：编辑、新建、标签、集合、批量、回收站、合并、添加附件
│       ├── fields.rs           条目字段规则：由字段推出常用列（导入与编辑共用）
│       ├── dedup.rs            去重提示与「不是重复」
│       ├── search.rs           搜索索引（search.db，FTS5 + 中文二元组）
│       ├── reader.rs           阅读：标注（合并矩形、排序键、摘录文字）、阅读位置
│       ├── metadata/           联网补全：mod（设置、比较与合并、记录、撤销、待确认、后台队列、按标识新建）、
│       │                       net（doi.org / Open Library、节流与重试、换成 Zotero 字段）
│       ├── protocol.rs         rl-file:// 协议：按附件 id 给文件库里的 PDF / 图片
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
│       └── research/           事实调查（v0.3，`decisions/0020`、`0022`）：commands（`research_*` 命令、设置、组装接口、后台执行）、store（存进 research_* 表）、
│                               anthropic（流式、网页搜索、pause_turn；ClaudeSearch；ClaudeFetch：下载失败时的网页抓取）、openai（OpenAI 兼容，DeepSeek）、tavily、meter（发送前按最坏情况查月上限、记账）、
│                               download（本机下载：限速、拒内网、自己跟跳转；WithFallback：失败了改用 Claude 网页抓取）、content（HTML / PDF / XLSX / CSV / DOCX 取正文）、snapshot（快照文件）、
│                               docx（Word 导出）、library（加入文献库）、fixtures / testing（合成原件、本地假服务器）
├── crates/zotero-import/       Zotero 只读读取 + 审计 + 合成夹具（src-tauri 以 path 依赖引用）
├── crates/research-engine/     事实调查的纯逻辑（只依赖 serde）：流程、提示词与解析、核对、认数与精确十进制、计算、可信度、结论核对、
│                               重查对比、报告与渲染（Markdown / 单文件 HTML）、审计；合成夹具与模拟接口（README 有模块表）
├── crates/docx-engine/         Word 文档引擎（src-tauri 引用）：排版器 + 独立校验器 + 文档计划（0-C）、structure / verify_structure（结构类排版与它的独立核对，W4）、
│                               outline（结构摘要、表格清单、计划预览、要求文档转文字）、
│                               checks（只检查项的统计）、cite（引文写入与核对、Zotero 引文识别与转换、.csl 检查、Node 里排版）、
│                               vba（生成 vbaProject.bin）、template（生成 .dotm）
├── poc/
│   └── word-addin/             0-B：citeproc 引文引擎、Word 外直写、宏桥接、Office.js（保留待用）；独立 pnpm 包
├── scripts/                   install-app.sh（打包、签名、装进「应用程序」）、clean-build.sh（清编译缓存）、phase0-check.sh（Phase 0 端到端自检）、cite-render.ts（Node 里排版，测试与命令行用）、
│                               word-icons.mjs / .swift（生成 Word 选项卡图标）
├── local/                      本机私有（真实论文、样例、证书），gitignore
└── docs/
```

根目录是 Cargo 工作区（`src-tauri`、`crates/zotero-import`、`crates/docx-engine`、`crates/research-engine`），编译输出在根 `target/`。

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

## 当前命令（Phase 6）

数据库命令都是 `#[tauri::command(async)]`：在线程池里跑，导入的几秒钟里界面不卡。会改内容的命令在提交后更新搜索索引。

| 命令 | 作用 |
| --- | --- |
| `app_info` / `app_reveal_data_dir` | 版本、数据目录、数据库版本、启动错误 / 在访达中显示 |
| `library_overview` | 文献、回收站、集合、标签、附件、笔记、缺失文件、未分类、可能重复的数量 |
| `items_list(query)` | 列表：范围（全部 / 未分类 / 集合含子集合 / 回收站 / 文件缺失 / 可能重复）+ 搜索 + 标签 + 类型 + 排序 |
| `items_get` / `attachments_get` / `notes_get` / `collections_get(id)` | 四种详情（常用字段 + 其他字段） |
| `items_create` / `items_update(id, patch)` / `items_set_field(ids, field, value)` | 新建、编辑（字段、类型、作者）、批量改字段 |
| `items_merge(masterId, otherIds)` / `duplicates_dismiss(ids)` | 合并重复 / 标记「不是重复」 |
| `attachments_add(itemId?, path)` / `attachments_rename` / `attachments_open` | 添加附件（复制进文件库）/ 改名 / 用默认程序打开 |
| `tags_list` / `tags_add` / `tags_remove(targets, name)` / `tags_rename` / `tags_delete(id)` | 标签 |
| `collections_list` / `collections_create` / `collections_rename` / `collections_move` / `collections_add` / `collections_remove` | 集合 |
| `trash_move` / `trash_restore` / `trash_delete(targets)` / `trash_empty` | 回收站（永久删除前自动备份；文件留到没有备份用到时再删） |
| `search_rebuild` | 重建搜索索引 |
| `reader_get(id)` / `reader_save_state(id, reading)` | 打开阅读器要的信息（标题、所属文献、是否只读、上次位置）/ 记下读到哪 |
| `annotations_list(attachmentId)` / `annotations_create(input)` / `annotations_update(id, patch)` / `annotations_delete(ids)` / `annotations_restore(ids)` | 标注：列表、新建（高亮 / 下划线 / 便签）、改颜色评论原文、删除与撤销 |
| `app_open_url(url)` | PDF 里的网址链接交给系统浏览器（只认 http(s) / mailto） |
| `metadata_settings_get` / `metadata_settings_set(patch)` | 联网补全的开关、自动、联系邮箱 |
| `metadata_lookup(ids)` / `metadata_lookup_run(runId)` / `metadata_lookup_all` | 手动 / 某次导入 / 全部，放进后台队列 |
| `metadata_status` / `metadata_cancel` | 后台队列进度 / 停止 |
| `metadata_latest(itemId)` / `metadata_resolve(lookupId, field, accept)` / `metadata_undo(lookupId)` | 最近一次结果 / 采用或忽略一处不同 / 撤销自动填的 |
| `metadata_add(text, collectionId)` | 按 DOI / ISBN 新建文献 |
| `backups_list` / `backups_create` / `backups_restore(fileName)` | 备份列表 / 手动备份 / 恢复到某份备份（主库打不开时也行） |
| `export_create(dir)` / `export_reveal(path)` | 整库导出到 `dir` 下的新文件夹（含自检）/ 在访达中显示 |
| `export_inspect(dir)` / `export_restore(dir)` | 从导出恢复：预览 / 替换整个库 |
| `import_zotero_preview` / `import_zotero_run` / `import_file_preview` / `import_file_run` / `import_runs_list` / `import_runs_get` | 导入（`decisions/0009`） |
| `word_status` / `word_install` / `word_install_into(dir)` / `word_uninstall` / `word_reveal(which)` / `word_reveal_file(path)` | Word 集成：状态（模板、脚本、服务、排版器）/ 安装 / 用户在对话框里选中启动文件夹后写入模板 / 卸载 / 在访达中显示 |
| `cite_renderer_ready` / `cite_render_done(id, result, error)` | 前端排版器：挂好监听 / 交回一次排版结果 |
| `picker_session` / `picker_search(q)` / `picker_done(session, done)` | 置顶搜索窗：这一次的预填 / 搜文献库 / 选完、取消或删除这处引文 |
| `cite_styles_list` / `cite_style_import(path)` / `cite_style_remove(id)` / `cite_style_source(id)` | 引文样式：清单（内置 + 导入）/ 导入 .csl（检查、试排）/ 删除 / 前端要导入样式的 CSL |
| `word_zotero_inspect(path)` / `word_zotero_convert(path)` | Word 外转换 Zotero 文档：预览 / 转换成「原名（Pith）.docx」 |
| `agent_status` / `agent_key_set(key)` / `agent_key_clear` / `agent_budget_set(usd)` / `agent_send_mode_set(mode)` | 排版 Agent 的设置：是否模拟模型、密钥填没填与存放处、月度上限、论文发送方式、本月已花、最近的调用 |
| `format_rulesets_list` / `format_ruleset_get(id)` / `format_ruleset_rename` / `format_ruleset_delete` | 规范（要求清单 + 由它合成的规范 + 这套规范下的论文） |
| `format_extract_prepare(paths)` → `format_extract_run(ticket)` | 提炼要求：先算出要发什么、多少钱（不联网），用户确认后才发送 |
| `format_requirement_set(rulesetId, requirementId, enabled?, settings?)` | 确认清单时开关一条要求、改它的设置值 |
| `format_plan_prepare(rulesetId, path, mode?)` → `format_plan_run(ticket)` | 文档计划：同样先准备、确认后发送；发送方式用设置里的（brief 结构和片段 / full 全文） |
| `format_run_get` / `format_plan_update(rulesetId, runId, draft)` / `format_run_delete` | 一份论文的计划（含预览会改到哪些段落）/ 用户改计划 / 删存档 |
| `format_apply(rulesetId, runId)` / `format_cancel` | 按确认的规范和计划排一份副本并出报告（不联网）/ 停止进行中的模型调用 |

`targets` 是列表里的行：`{ kind: item | attachment | note | collection, id }`。

选择文件夹 / 文件用 `@tauri-apps/plugin-dialog`（封装在 `ipc.ts` 的 `pickFolder` / `pickFile`，权限 `dialog:allow-open`）。

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
- 命令（`research_*`，21 个）：status / settings_set / key_set / key_clear；questions_list / question_get；plan_prepare → plan_run；run_get；plan_update / source_add / source_remove；execute_prepare → execute；cancel；rerun；export / reveal；add_to_library；run_delete / question_delete。

## 联网（`decisions/0004`、`0012`）

- v0.1 唯一的联网功能是联网补全；只在 Rust 里联网（`metadata::net`），界面不直接发请求。没打开时 Rust 拒绝联网。v0.2 W3 多了排版 Agent 调用 Claude（`agent::claude`，见上；每次发送前确认、有月度上限）。
- v0.3 事实调查（见上）：连 `api.anthropic.com`、`api.deepseek.com`（地址可改）、`api.tavily.com`，以及检索到的网页（本机下载，只取单页）。每次发送模型和搜索前界面确认、查月度上限。
- 导入完成、改了 DOI / ISBN 后，命令层把文献放进 `AppState.lookups` 队列，后台线程一次一条地查：读库（拿锁）→ 联网（不拿锁）→ 写库（拿锁）并更新搜索索引。

## 资源约束

M3 Pro / 18 GB：不常驻解析服务或大模型；PDF 文本提取按需、可取消。
