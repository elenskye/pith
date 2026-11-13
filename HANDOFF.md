# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：v0.4 C4 验收做完，正式版已更新到 0.4.0 并导入了验收数据，等你抽查（2026-11-13）

### 本轮（C4）

1. **验收并合并云端的 C1–C3**（你要我「验收并拉取」）：
   - 审了 `cloud/v0.4` 的 6 个提交：没碰 macOS 部分、提交信息合规、没有真实调用；书的文字没进 Git（中文夹具和杨敬年译本最长只重合 10 个字的常用短语，英文夹具是公有领域的 Project Gutenberg #3300）。
   - Mac 上 `pnpm check` 全过，包括云端跑不了的 3 个 Word 模板测试。
   - 快进合并进 `main`（`79ab217`），临时工作树已删。
   - 云端记录并进了 `docs/decisions/0025`、`architecture.md`、`testing.md`、`schema.md`；`HANDOFF-cloud.md` 清空成「没有任务」，原文在 git 历史里。
2. **真实验收**：用你给的密钥（OpenAI、Mathpix），数据放在 `local/concept-c4/data/`，没碰 `~/Pith`。这次自设上限 $5，**实际花了 $0.69**。结果：
   - **分工三章（中英对照）**：概念卡 21、论证卡 12、三张关系图（关系 42 条）；72 句原文和 72 句中译**全部逐字核对通过**，拦下 0 句，审计 0 个问题。
   - **其他入口都跑通了**：
     - 读一段、问这本书、追踪概念（全书 7 段）、推演；
     - 反方：文献库里没找到，用 OpenAI 网页搜索找了 3 篇文章，做出一张评论卡，引文核对通过；
     - PDF：一份 3 页的短文走 Mathpix，跨页的段落接上了，页眉页码没混进正文，Mathpix 上的副本用完删掉；卡片带 PDF 页码。
   - **接口细节都和真实的对上了**：OpenAI Responses API 第一次就通，账本和实际用量一致；Mathpix 的返回格式正好是解析器认的那种。
   - **导出**：11 次研究的 Markdown、HTML、Word 都在 `local/concept-c4/out/`。HTML 里的关系图能点，卡片里的原文带「I.i.1，第 109 页」，后面跟着核对过的中译。
3. **本地做的开发**：
   - **Word 里插入原文**：在搜索窗里选「卡片里的原文」，宏会在新引文前面插入「“原文”」。编辑已有引文时不插。
   - **修了检索**：中文词搜原著时大部分命中被丢掉了（「市场」原来只剩 1 段），现在按相关度取，再按书里的顺序排。
   - **Word 导出的标题**改成「概要」，不再沿用事实调查的「短答 / 全过程」；导出里的时间写成「09:22 UTC」。
   - **冻结 v8 样本库**，并让它带上概念研究的数据（云端做的测试库里概念的表是空的）。
   - **新加真实验收工具** `real_concept_run`，用法见 `testing.md`。
   - 修了 3 条 clippy 提示。
4. **更新正式版并导入真实测试数据**（你要求的，是规则 4「不读写 `~/Pith/`」的一次性例外，同 v0.3）：
   - `pnpm app:install` 装好 0.4.0，签名还是「Pith Self-Signed」，钥匙串里已有的密钥照样能读。
   - 新工具 `import_concept_runs` 走 App 自己的代码。先导进空目录演练两遍（第二遍全部跳过）；再导进 `~/Pith` 的完整副本，演练完删掉了。
   - 正式导入时正式版是关着的。`~/Pith` 升到 v8，升级前和导入前各自动备份一份：`backups/library-20261113-174105-before-v8.db`、`…-before-import.db`。
   - 导进来的：3 条文献（The Wealth of Nations、国富论、小镇集市上的分工，带 EPUB / PDF 附件）、3 份解析好的文本、11 次研究（56 张卡片）、评论来源快照 3 份。账本补进 19 笔 $0.69，正式版本月已花 $2.61。
   - 导完 11 次研究的审计都是 0 个问题，库的完整性、外键检查通过；原来的 84 条文献和 v0.3 的两次调查都没动。
5. **精简文档**：
   - `HANDOFF-cloud.md` 从 272 行减到 12 行；
   - `architecture.md` 加了 v0.4 一节和目录，`testing.md` 加了「概念研究」一节，`schema.md` 补上 0008；
   - `ROADMAP.md`、`requirements.md`、`README.md` 更新了状态。

## 等你

1. **提交**（git 命令在我的回复里）。
2. **抽查 10 张卡片**（`0024` 做完的标准里最后一条）：打开正式版的「概念」页就有这次的全部结果；也可以看 `local/concept-c4/out/` 里的 HTML。
3. **正式版里还要你做的**：
   - 「设置 › 密钥与费用」填 OpenAI 和 Mathpix（Mathpix 是 app_id、app_key 两个框），看钥匙串存取正常。密钥由你自己填，我不往界面里输密钥。
   - 「设置 › Word」点「更新」，装带新宏的模板。然后在 Word 里 Add/Edit Citation → 选 The Wealth of Nations 或国富论 →「卡片里的原文」选一句，看引文前面插进了原文：英文是「p. 110 (I.i.3)」这样带页码和段落号，中文只带段落号。再切换样式、刷新一次。
   - 第一次打开 0.4.0 读钥匙串时，系统可能问一次登录密码，点「始终允许」。
4. 以前留下的（不急）：W5 排好的终稿在 Word 里看一眼（`local/real-thesis/out/agent-w5/终稿/brief/毕业论文_终稿（Pith 排版）.docx`）；原创性声明和使用授权书要不要排在同一页。

## 下一轮

- 你抽查和 Word 里看过之后：v0.4 收尾，版本号不变，用 0.4.0；有问题就在这一版里改。
- 然后出 v0.5 WonderLab 的计划问卷（同一章节 ≥ 3 种知识作品，接 v0.4 的卡片、关系图和推演）。
- 不急的已知问题（详见 `0025`）：
  - 重新解析时旧卡片不会重新找锚点；
  - 全书对齐只保证了第一篇第 1–3 章（35 章里 16 章段数不同）；
  - 反方只发每个来源的前 1.2 万字；
  - 本地检索按词，不是语义检索；
  - GB/T 著者-出版年制的引文本来就不出页码；
  - v0.3、v0.2 留下的那些（中文数量词、调查中途退出不记账、排版 Agent 的几类结构）。

## 本轮改动

- 合进来的云端代码：`crates/concept-engine`、`skills/`、`src-tauri/src/concept/`、`src/concept/`、migration 0008 等，91 个文件。
- 本地：
  - `src-tauri/word/Pith.bas`、`src-tauri/src/word/bridge.rs`：Word 插入原文；
  - `src-tauri/src/concept/commands.rs`：检索排序；`run_prepare` / `run_create` 从命令里抽出来；`real_concept_run`、`import_concept_runs`；Word 导出的标题；
  - `src-tauri/src/research/docx.rs`：`write_with`；
  - `src-tauri/src/concept/{mod,tests}.rs`、`src-tauri/src/transfer/tests.rs`：测试库带上概念数据；
  - `src-tauri/fixtures/library-v8.json`（新）；
  - `crates/concept-engine/src/{epub,mock,render}.rs`：clippy、导出时间。
- 文档：
  - 新：`docs/decisions/0025-v0.4-c4-acceptance.md`；
  - 改：`0024`（指向 0025）、`architecture.md`、`testing.md`、`schema.md`、`requirements.md`、`ROADMAP.md`、`README.md`、`HANDOFF-cloud.md`、本文件。
- 正式版：`/Applications/Pith.app` 0.4.0；`~/Pith` 升到 v8 并导入了验收数据（见上）。
- 本机（不进 Git）：
  - `local/concept-c4/`：验收的数据目录和导出；
  - `local/concept-pdf/market-division.pdf`：自己写的 3 页测试短文，人名、地名、数字都是虚构的。

## 测试

- `pnpm check` 全过：tsc、前端测试 17、`cargo test --workspace`（`pith` 263 通过、20 项默认忽略，含两个真实验收 / 导入工具；`concept-engine` 36；其余原样）、`poc/word-addin` 29。
- `cargo clippy --workspace --all-targets`：0 警告。
- 云端分支在 Mac 上单独跑过一遍：`pnpm check` 全过。
- 真实验收：11 次研究、19 笔真实调用，共 $0.687；全部审计 0 个问题。命令见 `docs/testing.md`「概念研究」。
- 导入正式版：空目录演练两遍、`~/Pith` 副本演练一遍、正式导入一遍，审计都是 0 个问题；正式库完整性和外键检查通过。
- 没做的：在 Word 里点一遍插入原文（等你更新 Word 模板）；正式版界面里点一遍概念页（云端在浏览器 `?mock` 里点通过；正式版的数据已导好，你打开就能看）。
