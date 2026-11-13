# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：v0.3 已验收；v0.4 Concept Research 出计划问卷（2026-11-12）

### 本轮（v0.4 Dev Plan）

1. **v0.3 验收通过**（你定的）：
   - 密钥在正式版里存取正常；
   - Fact Research 以后的优化不挡 v0.4；
   - `ROADMAP.md` 把 v0.3 挪进「已完成」，`decisions/0022` 记下你的验收。
2. **两次真实调查导进了正式版**（你要求的，是规则 4「不读写 `~/Pith/`」的一次性例外）：
   - 用新加的工具 `import_research_runs`，走 App 自己的代码。先在空目录里演练、对 `~/Pith` 先空跑一遍，再正式导入；导入时正式版是关着的。
   - 导入前，正式库自动做了「导入前」备份：`library-20261112-200357-before-import.db`。
   - 导进来的：一个问题、两次调查（各 40 个来源、274 / 288 条事实、5 条结论）、72 份快照（逐个核对 SHA-256）。导完重新审计，两次都是 0 个问题。
   - 账本补进 156 笔真实调用（$1.92，就是那两次调查和网页抓取试验实际花的钱），正式版「本月已花」现在对得上。
   - 来源和文献的对应没带过去（那些文献只在测试库里）。要的话，在调查结果页勾选来源、点「加入文献库」。
3. **冻结了 v7 样本库** `src-tauri/fixtures/library-v7.json`：0007 已经装进正式版，不会再改；v0.4 加新 migration 前就不用再冻。
4. **视觉设计改到 v0.7 之后**：写进 `ROADMAP.md`「以后」和 `decisions/0005`。
5. **v0.4 第三轮问卷**：
   - 已发成 Artifact：https://claude.ai/artifact/6EiFWEH3mk7qUFKZvk7FaN （源文件 `docs/questionnaire/round-3.html`）；
   - 5 部分 22 题：读哪个本子、做出什么、放在 Pith 的哪里、模型与费用、分工与验收；
   - 填完点「提交给 Claude」，回答存在 Artifact 里，我从那里取。
6. **PDF 还是 EPUB**（你问的）：建议 **EPUB**。
   - EPUB 本来就是带结构的文字（章节、段落、脚注），取字准、不用 OCR，中文不乱码，最适合逐字核对原文。
   - 文字版 PDF 的长处是有纸本页码；中文 PDF 取字容易乱码，页眉页脚和断行要清理。
   - 扫描版 PDF 不建议。
   - 引文定位建议用段落编号（Glasgow 版的 I.i.1），不依赖页码；要页码就再备一份同一版本的文字版 PDF。问卷 A1–A3 里让你定。

## 等你

1. **填第三轮问卷**（约 12 分钟），填完回对话说一声「填好了」。
2. **提交这两轮的修改**（git 命令在我的回复里）。上一轮的 v0.3 改进也还没提交，这次一起提交就行。
3. 问卷定了版本和格式以后，把那本书的文件放进 `local/`（例如 `local/wealth-of-nations/`）。只放你合法持有的；不进 Git。
4. 上一轮留下的（不急）：
   - W5 排好的终稿建议在 Word 里看一眼：`local/real-thesis/out/agent-w5/终稿/brief/毕业论文_终稿（Pith 排版）.docx`；
   - 原创性声明和使用授权书要不要排在同一页。

## 下一轮

- 收到问卷回答后：
  - 写 `decisions/0024`（v0.4 计划），在 `ROADMAP.md` 定分期，标出哪几期交给云端；
  - 版本号改 0.4.0；
  - 改写 `HANDOFF-cloud.md` 的「任务」交给云端。
- 不急的已知问题：
  - 「三成」这类中文数量词只标出、不核对；
  - 中文 PDF 取文字没用真实文件验过；
  - 调查进行中退出 App，当时那次调用不记账；
  - 执行前的费用估计偏粗；
  - v0.2 的：真实 App 里经 Tauri 命令走一遍排版 Agent，编号定义里的列表、表格首列、公式编号、四线表、表注、脚注分隔线、目录生成。

## 本轮改动

- `src-tauri/src/research/commands.rs`：新工具 `import_research_runs`（测试里，默认忽略）；`real_research_run` 的 `fetch` 一步可以只用 Anthropic 密钥、`PITH_CLAUDE_ONLY=1` 直接让 Claude 抓。
- `src-tauri/fixtures/library-v7.json`（新，冻结的样本库）。
- 文档：
  - `docs/questionnaire/round-3.html`（新）；
  - `ROADMAP.md`（v0.3 挪进「已完成」，现在是 v0.4，视觉设计改到 v0.7 之后）；
  - `docs/decisions/0005`（视觉设计的时间）、`0022`（你的验收）；
  - `docs/testing.md`（导入工具）、本文件。
- 正式数据目录 `~/Pith`：导入了两次调查和账本记录（见上），导入前已备份。

## 测试

- `cargo test -p pith --lib`：247 通过（另 18 项默认忽略，含新的导入工具）；冻结样本库升级测试覆盖 v5、v6、v7。
- `cargo clippy --workspace --all-targets`：0 警告。
- 导入：
  - 空目录演练两遍：第二遍全部跳过，账本不重复记；
  - `~/Pith` 先空跑、再正式导入；
  - 导完审计 0 个问题。
- 问卷：脚本语法检查通过；本机预览能显示，控制台没有错误。Artifact 已发布，并开了存回答用的数据库。
