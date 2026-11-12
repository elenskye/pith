# 测试

## 一条命令

```bash
pnpm check
```

= `tsc --noEmit` + 前端引文排版的测试（`node --test 'src/**/*.test.ts'`，单跑：`pnpm test:web`）+ `cargo test --workspace`（App 后端、Zotero 读取、文档引擎）+ `poc/word-addin` 的检查。每轮结束前必须通过；另跑 `cargo clippy --workspace --all-targets`，0 警告。

## 约定

- 全部用合成数据：Zotero 合成夹具 `zotero_import::fixture`、各格式的合成样例、`transfer::tests::rich_library`（夹具导入 + 各种 App 里产生的数据）。
- 临时目录 `db::tests::temp_dir`（`<tmp>/researchlab-test-<pid>-<名称>`，离开作用域删除）；内存库 `db::tests::memory_db`（带内存搜索索引）。
- 联网的测试不联网：假的 HTTP 回放合成的响应。
- 浏览器预览：`pnpm dev` → http://localhost:1430/?mock（`src/dev/mockIpc.ts`）。新增 Rust 命令时同步 mock。调阅读器时把真实副本里的某个 PDF 复制到 `local/` 下，用 `?mock&pdf=/local/<文件名>.pdf` 打开（所有附件都打开它）。真实 App：`pnpm app:dev`（数据在 `~/Pith Dev/`）。开发版、测试、用 `PITH_DATA_DIR` 另指数据目录的打包版里，排版 Agent 用模拟模型（不联网、不花钱、不碰钥匙串）；试用时选 `src-tauri/fixtures/agent/示例规范.txt`，出处才核对得上。

## 各模块测什么

| 模块 | 测试 | 要点 |
| --- | --- | --- |
| 数据库、备份、启动 | `db.rs`、`backup.rs`、`lib.rs` | 新库 / 升级（升级前备份、失败整步回滚）、比 App 新就拒绝；daily 每天一次与清理规则；数据库损坏时报告不崩溃 |
| 导入 | `import/*/tests.rs` | 夹具里的每种边界（缺失文件、MD5 不符、快照、内嵌图片、回收站、群组库……）；重复导入被拒、失败整批回滚并删掉新复制的文件；写入核对的「故意写错」检验 |
| 编辑、回收站、去重 | `edit/tests.rs` | 字段 / 作者 / 换类型、批量一条失败整批不改、集合防成环；永久删除先备份、文件留到没有备份用到；合并 |
| 搜索 | `search/tests.rs` | 中文二元组、西文前缀、子笔记命中父文献、FTS 特殊字符、索引跟着改动更新 |
| 阅读 | `reader/tests.rs`、`protocol.rs` | 矩形合并、排序键与位置 JSON 与 Zotero 逐字符一致、只读的标注；协议的 Range / 拒绝路径穿越 |
| 联网补全 | `metadata/tests.rs` | 认 DOI / ISBN、Crossref / Open Library 换成 Zotero 字段、只填空字段、待确认 / 撤销、后台队列 |
| 恢复与导出 | `transfer/tests.rs`、`lib.rs` | 见下 |
| Word 集成（v0.2） | `word/*.rs` | 本机服务：没令牌 / 令牌错 401、带 Origin 403、分块 411、超大 413、`Expect: 100-continue`、service.json 0600、停止时只删自己那份；宏协议：新文档第一处引文、旧名换新、复制粘贴重复、缺数据、切换样式、比 App 新的数据、搜索窗（新插入 / 编辑预填 / 删除 / 取消）、编辑保留 ID 与创建时间、Style 菜单清单、Word 里的 Zotero 转换；搜索窗：搜索、预填（库里 / 文档快照 / 找不到 / 回收站）、一次只有一个会话；样式：内置清单与 CSL 文件一致、导入 / 替换 / 依赖样式 / 删除；Zotero 条目 key 对应；CSL 映射（含印刷品文章不出网址、「额外」字段换算）；模板含全部按钮的回调与图片；装进临时「家目录」再卸载（真跑 osacompile）、写不进时由用户选中的文件夹写入 |
| 排版 Agent（v0.2 W3） | `agent/*.rs`、`agent/tests.rs` | 整条流程用模拟模型和合成文档（提炼 → 合成规范 → 计划 → 排版 → 报告，原文件不变、不覆盖）；预算按最坏情况拦、上月不算；截断 / 拒答 / 格式不对照样记账且不存结果；中途停止按估计记账；流（SSE）解析用录好的文本；出处摘录核对、数值换算表、冲突取确定度高的；计划里不存在的样式 / 编不过的正则 / 越界的分节被丢掉；存档编号防路径穿越；测试不碰钥匙串、不联网 |
| 事实调查（v0.3） | `crates/research-engine`（各模块单元测试、`tests/flow.rs`）、`src-tauri/src/research/*.rs`、`research/tests.rs` | 见下「事实调查」 |
| W5 版面（文档引擎） | `crates/docx-engine/tests/w5_layout.rs` | 已经另起一页的段落不再加段前分页（分节符后隔空段落、上一段以分页符结尾、一个标题分两段写）；新一节第一段、前面是正文的照样分页；受保护分节不动；表格里参考文献样式的段落换成表格文字；自带悬挂缩进的样式和列表段落写明 0 字符，参考文献保持原对齐；**改坏**（去掉段前不分页、去掉 0 字符）核对都查得出 |
| 结构类排版（文档引擎，W4） | `crates/docx-engine/tests/structure.rs` | 「像论文」的合成文档上六类结构（分区与分节、页眉、页码、奇数页起、三线表、脚注编号、标题紧跟）；单面 / 双面；已有分节符后隔着空段落不多切一节；重复执行不重复加；表格可跳过、受保护分节不动；没给正文起点时如实标出；过期的分界点被拒绝；**把排好的文档逐项改坏，校验器都查得出**（含多一个部件、多一条关系、子元素顺序挪乱） |
| 结构摘要与检查（文档引擎） | `crates/docx-engine/tests/outline.rs`、`src/checks.rs` | 分节的数法与排版器一致；计划预览的命中数 = 排版器实际改的段落数；页面设置缺省的项不动；摘要字数（Word 的算法）、关键词、参考文献条数与外文占比、题注是否按章编号 |
| 引文（文档引擎） | `crates/docx-engine/tests/cite.rs`、`src/cite/`、`src/vba.rs`、`src/template.rs` | 合成文档刷新 → 引文 / 书目 / 数据部件 / 悬挂缩进；独立核对（另一套解析器）查出手改；0-B 旧格式迁移；控件以外的文字、域、其他部件不变（含 0-C 的合成论文）；中文姓名合成全名；Zotero：域代码与文档设置解析、按 key 对应、合成 Zotero 文档转换（页码、多条、库里没有的、修订里的不动、书目跨段带分页符、别的域不动）、样式对应、已有 Pith 数据的文档；.csl 检查；VBA 压缩（含规范样例）、加密、工程读回；模板确定（含图片） |
| 引文排版（前端） | `src/cite/*.test.ts` | citeproc-js：APA / GB/T / Chicago 的引文与书目、全部内置样式能排、交给 Rust 的结构；GB/T 顺序编码页码在方括号外「[2]17」（2015、2025）；前后缀、不显示作者；中文全名；HTML → 带格式片段 |

## 恢复、导出与升级（Phase 6，`decisions/0013`）

- **导出**：文件夹内容、自检、`library.json` 能被普通 JSON 工具读、同一秒不覆盖、文件库里缺的文件照常报告。
- **从导出恢复**：恢复到另一个新库后**每张表逐行一模一样**；改过的、截断的、缺表的、格式 / 数据库版本比 App 新的、不是我们的导出都拒绝；导出里缺失或改坏的附件只报告不中断。
- **从备份恢复**（`lib.rs`，走真实的 AppState）：改动后恢复 → 数据回到备份时、永久删除的 PDF 还能打开、搜索跟着变；再恢复「恢复前」→ 改动回来；坏备份不动主库；**主库损坏时也能恢复**，坏库挪进备份目录；恢复之前开始的后台补全不写新库。
- **升级流程**（每次加 migration 都靠它）：
  1. `migrations_only_add`：migration 只建表、加列、建索引。若以后有要改数据的 migration，这条测试会失败，提醒为它单独写升级测试。
  2. `every_old_version_upgrades_without_loss`：合成的完整库投影到第 1 … N-1 版 → 升到最新 → 旧数据逐表一致、App 的读取全部能用（`exercise`）。
  3. `frozen_fixtures_upgrade_to_latest`：`src-tauri/fixtures/library-vN.json`（当时的代码生成）恢复 → 升级 → 同样检查。**加新 migration 之前**先冻结当前版本（生成后提交，用过的不改）：

```bash
cargo test -p pith freeze_fixture -- --ignored
```

## 事实调查（v0.3，`decisions/0020`、`0022`）

- **引擎**（`crates/research-engine`，模拟接口）：认数（千分位、万 / 亿、% / ‰ / 百分点、负数、区间，日期和编号不当数）、文字折叠（只容忍空白和全半角）、计算（单位不一致、除数为 0、未核实的都拒绝）、结论核对（含占位符后面又写一遍单位）、可信度、重查对比（含同一格两次写法不同、不同指标 / 年龄段不误对）、渲染；`tests/flow.rs` 整条流程、取消后接着做、超预算、单个来源失败、全都读不了。夹具是虚构的「示例市」，故意带一条摘录写错的事实、一个 404、一个扫描件 PDF、一段注入文字和一段 `hidden` 文字。
- **改坏**：摘录改一个字、数字改一位、单位错、结论里多一个对不上的数、低可信来源排到前面，流程里拦一次，`audit` 对存下来的结果再查一次（另查：没核实的标成已核实、计算结果被改、可信度被抬高、正文被换）。
- **App 一侧**（`src-tauri/src/research/`）：取正文（类型判断、编码、HTML / CSV / XLSX / PDF / DOCX、扫描件）、快照、本机下载（本地 `TcpListener` 假服务器：字节一致、跳转、各种失败、内网拦截、限速与取消）、三类接口（录制的回复、请求形状、`pause_turn` 原样发回、拒答、错误不回显密钥、重试、预算、流断开按估计记账）、存取（fresh / v5 升级）、Word 导出、加入文献库；`research/tests.rs` 端到端：真实的接口实现连本地假 Anthropic / DeepSeek / Tavily / 网站，每一笔费用对上账本。
- **浏览器**：`?mock` 里调查页全流程能点通；`?mock&research=real` 假装真接口（确认框里显示缺密钥等）。mock 数据由引擎生成，改了引擎的输出结构就重新生成：

```bash
cargo run -q -p research-engine --example mock_data > src/dev/mockResearch.json
```

- **真实验收**（默认忽略，会联网花钱，事先问用户上限）：和 App 的命令走同一套接口与设置，结果存进指定的数据目录，打包版用 `PITH_DATA_DIR` 指过去就能在 App 里看。密钥文件每行一把（Anthropic、DeepSeek、Tavily 按形状认；别放 OpenAI 等其他 `sk-` 开头的密钥，会被当成 DeepSeek 的）。

```bash
PITH_REAL_MODEL=1 PITH_API_KEY_FILE=<密钥文件> PITH_ACCEPT_DIR=<数据目录> PITH_BUDGET=<月上限> PITH_STAGE=plan cargo test -p pith real_research_run -- --ignored --nocapture
```

  打印计划、uid 和下一步的预计费用；接着 `PITH_STAGE=execute PITH_RUN=<uid> PITH_EXPORT_DIR=<目录>` 做完（审计、导出三种、结论用到的来源加入文献库）；重查是 `PITH_STAGE=rerun PITH_RUN=<上一次的 uid>`，再按 execute 做新的这一次。

## 真实数据（默认忽略，只读，只打印汇总数字）

都用 `~/ResearchLab-test/zotero-copy`（副本），导入到临时目录；绝不读写 `~/Zotero/` 与 `~/Pith/`。写进 `docs/` 的只能是聚合数字。

命令格式（换掉 `<测试名>`）：

```bash
ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith <测试名> -- --ignored --nocapture
```

| 测试名 | 看什么 |
| --- | --- |
| `real_copy` | 导入 + 写入核对 |
| `real_copy_tokenizers` | 两种中文分词的对比 |
| `real_copy_lookup`（另需 `RL_CONTACT_EMAIL`，会联网） | 联网补全 |
| `real_copy_export` | 整库导出 → 恢复到另一个库，逐表一致 |
| `real_copy_csl` | 每条文献都能换成 CSL-JSON（类型、语言等聚合数字） |
| `real_thesis_zotero`（论文默认 `local/real-thesis/src/毕业论文_终稿.docx`，`PITH_THESIS=<路径>` 换） | 真实论文的 Zotero 引文 Word 外转换：对应到文献库的数量、与 Zotero 原来显示的逐处比（引文、书目）、Zotero 的 itemData 与 Pith 的 CSL-JSON 逐字段比、再切 APA；两份都独立核对。排版用 Node 里的 `src/cite`（`scripts/cite-render.ts`）。输出写到 `local/real-thesis/out/` |
| `prepare_e2e_library`（另需 `PITH_E2E_DIR=<空目录>`） | 把副本导入这个目录并留着，给端到端验收用 |

Zotero 读取层与审计：`cargo test -p zotero-import -- --ignored`（同样要 `ZOTERO_COPY_DIR`）。

另有：`bench_rebuild`（索引重建耗时）、`real_network_samples`（公开样例核对真实接口，会联网）、`node scripts/pdf-check.mjs ~/ResearchLab-test/zotero-copy/storage`（PDF 兼容性）。

## Word 端到端（v0.2，`decisions/0016`）

不碰 `~/Pith/`：先准备一个临时数据目录，再让 Pith 用它。

1. `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy PITH_E2E_DIR=<空目录> cargo test -p pith prepare_e2e_library -- --ignored --nocapture`
2. 开发版：`pnpm dev` 开着，`PITH_DATA_DIR=<目录> ./target/debug/pith`；正式版：`pnpm app:build` 后 `open --env PITH_DATA_DIR=<目录> target/release/bundle/macos/Pith.app`。
3. 不经 Word 先测服务（端口、令牌在 `<目录>/service.json`）：`curl -H "Authorization: Bearer <令牌>" http://127.0.0.1:<端口>/v1/ping`；宏协议 `POST /v1/word`（请求格式见 `src-tauri/src/word/bridge.rs` 文件头）；Word 外刷新 / 核对 `POST /v1/docx/refresh`、`/v1/docx/verify`。
4. 先备份用户自己的桥接脚本（`~/Library/Application Scripts/com.microsoft.Word/Pith-bridge.sh`、`Pith.scpt`；测试用的 Pith 安装时会改连测试数据目录）。设置 › Word › 安装到 Word。macOS 不让 Pith 写 Word 启动文件夹时，会弹出已经打开那个文件夹的对话框，点「打开」就写进去（选过一次以后「更新」直接写入）。只是测试也可以直接用 Word 打开 `<目录>/word/Pith.dotm`。
5. 用 Word 打开一份论文副本（`open -a "Microsoft Word" <副本>`）：Convert Zotero、Add/Edit Citation（插入、编辑、删除）、Style（切 APA 再切回）、Bibliography、Refresh、Open Pith；存盘后用 `/v1/docx/verify` 核对。要测「Pith 没开时自动启动」，先退出 Pith 再点按钮。用自动化点选项卡时先移动鼠标再点（Word 会忽略紧跟在别处点击之后的一下）。
6. 测完：退出 Word 与测试用的 Pith，把第 4 步备份的桥接脚本放回去（或从自己平时用的 Pith 重新安装）。

调试模板：`cargo run -p docx-engine -- dotm <输出.dotm> --bas <模块.bas> --ribbon <customUI.xml>` 生成后直接用 Word 打开（不用装）；`cargo run -p docx-engine -- vba-dump <vbaProject.bin>` 对照 Word 自己存的工程（比如 Zotero 安装包里的 Zotero.dotm）。

## 在 Word 里看排版结果（v0.2 W5，`decisions/0021`）

用 Mac Word 把排好的副本导出 PDF，再逐页渲染、列出每页页眉页脚文字、找出空白页（只用 macOS 自带的 PDFKit）：

```bash
bash scripts/word-pdf.sh <排好的.docx> <输出目录>
```

输出目录里有 `<名字>.pdf`、`<名字>-pages/pNNN.png`（每页一张图）、`<名字>-pages.txt`（每页一行：页眉区、页脚区文字；空白页标 BLANK-BODY）。文档先复制到输出目录再打开，原件不动，关闭时不保存。Word 是沙盒应用：第一次往某个目录写 PDF 时会弹「Grant File Access」，要用户点一次授权，所以固定用一个输出目录。只渲染已有的 PDF：`swift scripts/pdf-pages.swift <in.pdf> <输出目录> [倍数]`。

不花钱地复查（引擎改了以后）：用存下来的模型计划在论文副本上按 App 的同一条路径重排，结果写到 `PITH_OUT_DIR`（存下来的计划不动；同名输出不覆盖，另存「（Pith 排版 2）」）。路径用绝对路径（测试在 `src-tauri` 下运行）：

```bash
PITH_ACCEPT_DIR=<存计划的目录> PITH_RULESET=<规范 id> PITH_RUN=<计划 id> PITH_THESIS=<原论文.docx> PITH_OUT_DIR=<输出目录> cargo test -p pith real_agent_reapply -- --ignored --nocapture
```

W5 用的是 `local/agent-acceptance-w4` 里规范 `01M748Z7NH0TZ0B5ATK3A4ZK0A` 的 4 份计划，输出在 `local/real-thesis/out/agent-w5/`。排好的论文再做引文转换和切换 APA：`PITH_THESIS=<排好的.docx>` 跑上面「真实数据」里的 `real_thesis_zotero`。

## 排版 Agent 的真实模型验收（v0.2 W3，`decisions/0018`）

联网、花钱（一轮提炼约 $0.5，一份论文的计划约 $0.1–0.3），**每一期都要用户重新同意**（W3 上限 $5、实际 $1.86；W4 上限 $3、实际 $1.63），密钥放在用户给的文件里（测试代码从不读钥匙串）。账本记在 `PITH_ACCEPT_DIR` 里，合计超过 `PITH_ACCEPT_BUDGET`（默认 5 美元）就拒绝调用。

1. 不联网，先看两种方式各要发多少：`PITH_THESIS=<论文.docx> cargo test -p pith real_thesis_digest -- --ignored --nocapture`（加 `PITH_SHOW=1` 打印「结构和片段」发出的文字）。
2. 提炼：`PITH_API_KEY_FILE=<密钥文件> PITH_ACCEPT_DIR=<目录> PITH_REQ_DOCS=<文档 1>:<文档 2>… [PITH_HUMAN_SPEC=<人工规范.json>] cargo test -p pith real_agent_extract -- --ignored --nocapture`：打印清单、核对不上的摘录、合成的规范，并与人工规范逐项对比。
3. 计划：`PITH_API_KEY_FILE=… PITH_ACCEPT_DIR=… PITH_RULESET=<第 2 步打印的规范 id> PITH_THESIS=<论文.docx> PITH_MODE=brief|full [PITH_HUMAN_PLAN=<人工计划.json>] cargo test -p pith real_agent_plan -- --ignored --nocapture`：在目录里的副本上生成计划并排版，再与人工计划逐段比较最后用的样式、比较各部分的起点。
   - 不联网地试引擎（手写规范和计划）：`cargo run -p docx-engine -- apply <论文.docx> --spec <规范.json> --plan <计划.json> --out <新文件.docx> --report <新文件.md>`；W4 用的手写规范与计划在 `local/agent-acceptance-w4/manual/`。
4. 在 App 里用真模型但不碰正式数据：`pnpm app:build` 后 `open --env PITH_DATA_DIR=<临时目录> --env PITH_REAL_MODEL=1 --env PITH_API_KEY_FILE=<密钥文件> target/release/bundle/macos/Pith.app`。

## PoC 与 v0.2 素材

- `cd poc/word-addin && pnpm check`（Office.js 路线保留待用）；`cargo test -p docx-engine`；`bash scripts/phase0-check.sh`（Phase 0 端到端，仍用 0-B 原型的旧格式）。
- 真实论文：`RL_WORD_DOCX=<绝对路径> cargo test -p docx-engine --test format -- --ignored`。论文与规范在 `local/real-thesis/`，回归样例在 `local/samples/`，都不进 Git。
