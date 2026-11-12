# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。云端 Claude 的交接在 `HANDOFF-cloud.md`。

## 现在：v0.3 计划已定，云端开工前的准备（F0）做完了；接下来本地做 v0.2 W5，云端做 v0.3 F1–F3（2026-11-12）

### 本轮

1. **问卷回答**取回并存档：`docs/questionnaire/round-2-answers.md`（24 / 25 题，E1 未答）。
2. **正式版不用再装**（问卷 A2）：「应用程序」里的 Pith 是 2026-11-11 12:36 装的 0.2.0。二进制里有 W4 的新功能，之后源代码没再改过，所以就是 W4。
3. **v0.3 计划**：`docs/decisions/0020-v0.3-plan.md`。`ROADMAP.md`「现在」改成两条线：本地 W5、云端 v0.3。`requirements.md` 加了 v0.3 一节。要点：
   - 通用事实调查，用「2022–2026 北京、天津青年就业，对比其他一线和热门二线城市」验收。
   - 不限来源、按可信度排序；中国政府官方统计默认降权，设置里可改，报告写明。
   - Opus 5.5 定计划、下结论；DeepSeek（`deepseek-flash`）读网页、摘证据。
   - 搜索用 Claude 网页搜索 + Tavily：每个模型接口配它的专用搜索，没有的用 Tavily。
   - Pith 在 Mac 上下载网页和文件，存原件 + 正文 + 哈希。摘录和数字由代码逐字核对，计算由代码做。
   - 短答在前；能导出 Markdown / Word / HTML；来源能加入文献库；能重查对比。
   - 和排版 Agent 共用月上限 $30，不设单次上限。
   - 分期：F0 本地准备（本轮）→ F1–F3 云端一口气做 → F4 本地一次性验收。
4. **F0 代码准备**（问卷 D2）：
   - 钥匙串依赖 `security-framework` 改成只在 macOS 上编译；`secret.rs` 的钥匙串模块同样只在 macOS 编译，其他系统换成「钥匙串只在 macOS 上可用」的空实现。
   - 3 个要用 `osacompile` 编译 AppleScript 的安装测试，只在 Mac 上跑。
   - 钥匙串按名字存多把密钥：`ApiKey`（Anthropic / DeepSeek / Tavily），各有形状检查和条目名。Anthropic 的条目名不变，已存的密钥不用重填；`PITH_API_KEY_FILE` 一个文件可以放几把。新增 3 项测试。
   - 这样云端以后不用碰这段 Mac 专用代码。Mac 上行为不变。
   - **没在 Linux 上实际编过**：本机 Docker 没开，我没为这个启动它。云端开工第一步就是在 Linux 上跑 `pnpm check`；还有编不过的，按规则不改、记下来。
5. **给云端的交接**：
   - `HANDOFF-cloud.md`：任务说明，包括先读什么、环境、F1–F3 做到什么程度、不碰什么、自查标准、提交格式。
   - `scripts/cloud-setup.sh`：云端环境脚本，装 Tauri 在 Linux 上要的系统库、更新 Rust、装 pnpm 12。
   - `CLAUDE.md`：规则 1、2、7 加了「云端写 `HANDOFF-cloud.md`，不改 `HANDOFF.md`、`ROADMAP.md` 和已有的 decisions」「云端只用小阶段提交格式，`finished` 留给你」。
6. **没写进去的一条**（问卷 D6）：「允许本地 Claude 自己执行 git 抓取命令来验收」。我往 `CLAUDE.md` 写这条时被安全检查拦下了（Agent 在给自己加权限），需要你自己加，见「等你」第 2 条。

## 等你

1. **提交并推送本轮**：云端从 GitHub 拉代码，要看得到 `HANDOFF-cloud.md` 和 F0 的改动。
2. **（可选）把抓取权限加进 `CLAUDE.md` 规则 4**。想让本地 Claude 自己抓云端分支验收，就在规则 4「唯一例外：云端 Claude…」那一行下面加：

   ```
   - 本地 Agent 验收云端分支时可以自己抓取（2026-11-12 用户定）：`git fetch origin`，在 `.claude/worktrees/` 下用 `git worktree add --detach` 建临时工作树跑检查，验收完 `git worktree remove` 删掉。不提交、不合并、不切换用户正在用的分支。
   ```

   临时工作树的好处：验收时不会打断你同时在用的 main（W5 在那里做）。就算加了，本地会话执行这些命令时，权限系统可能还是会弹窗问你，点允许即可。
3. **设置云端**（另一个账号）：
   1. 用另一个账号登录 claude.ai/code，连上 GitHub，让它能访问 `elenskye/pith`（私有仓库要在 GitHub 上把 Claude GitHub App 装到这个仓库）。
   2. 新建环境：网络保持默认的 Limited（已包含 crates.io、npm、Ubuntu 软件源）；Setup script 粘贴 `scripts/cloud-setup.sh` 的全部内容。
   3. 新建会话：仓库 `elenskye/pith`，起始分支 `main`，第一句话粘贴：

      ```
      先读 CLAUDE.md 和 HANDOFF-cloud.md，然后按 HANDOFF-cloud.md 的「任务」一口气做完 v0.3 的 F1–F3。只推到 cloud/v0.3，交接写在 HANDOFF-cloud.md 的「云端记录」里。
      ```

   4. 云端做完后回来说一声，我开始 F4 验收。
4. **F4 之前准备两把密钥**（云端用不上，不急）：
   - DeepSeek：platform.deepseek.com，按平台要求充值少量余额；
   - Tavily：app.tavily.com，免费额度每月 1000 点。
   F4 时在 Pith 设置里填，或者放进 `PITH_API_KEY_FILE` 指的文件做真实验收。
5. **W5 要你在 Word 里看的**（下一轮本地就做）：打开 `local/real-thesis/out/agent-w4/终稿-结构和片段.docx`，看：
   - 能否正常打开；
   - 页眉：摘要页起有页眉，奇数页是章名，偶数页是「重庆大学本科学生毕业论文（设计）」，下面有横线；封面和原创性声明起没有页眉；
   - 页码：摘要、目录是 Ⅰ、Ⅱ…，正文从 1 起；
   - 两个附录表格是三线表。
   有不对把现象发我。
6. 上一轮留下的（仍有效）：
   - 把「应用程序」里的 Pith 拖进程序坞；设置 › Word 点一次「更新」。
   - 第一次在正式版里用排版 Agent：设置 › 排版 Agent 填密钥 → 排版 › 新建规范 → 选 `local/real-thesis/src/` 的三份 Requirement（约 $0.5）。
   - 文献库里 7 位英文作者是单栏姓名；「Pith 控制访达」可以在系统设置里关掉。

## 下一轮（本地）：v0.2 W5 验收

按 `ROADMAP.md` W5：两份真实论文 + 重庆大学规范，Mac Word 目视正确，保真校验与规范核对全部通过，引文可切换刷新。先处理你在 Word 里看到的问题（「等你」第 5 条）。云端同时在改 `cloud/v0.3`；W5 尽量只动 `crates/docx-engine`、排版 Agent 和文档，不加 migration。

## 云端回来之后（本地）：v0.3 F4 验收

1. 抓取 `cloud/v0.3`：你来抓，或者「等你」第 2 条加上后由本地 Claude 在临时工作树里抓。
2. 逐项审云端改动：
   - macOS 部分有没有被改（尤其 `secret.rs` 的 `keychain` 模块）；
   - 提交信息和分支是否合规；
   - 有没有真实调用。
3. Mac 上跑 `pnpm check` 和 `cargo clippy --workspace --all-targets`。
4. 钥匙串接线；用真实接口跑北京、天津青年就业的问题（事先问你上限）；对照官方价目页核对单价；在 Mac Word 里看导出的 Word。
5. 把 `HANDOFF-cloud.md` 的记录并进本文件；云端的工程选择并进 `decisions/0020`、`architecture.md`、`testing.md`。
6. 合进 `main` 由你做。

## v0.2 状态（W1–W4 完成，W5 未做）

细节见 `ROADMAP.md` 与 `decisions/0016`–`0019`。W4 真实模型验收（Opus 5.5，6 次调用共 $1.63）：
- 三份重庆大学规范提炼 46 条，摘录全部核对上；合成规范与手写的一致 86 项、不同 0 项。
- 终稿两种发送方式：保真 0 问题，核对 78 / 78。
- 完整稿：71 / 71。
- 排出来的文档在 `local/real-thesis/out/agent-w4/`。

W4 留下的（不急）：
- 还做不了的结构类要求：公式编号的位置、四线表、表注的位置、脚注与正文之间的分隔线、目录的生成与各级格式。
- 「标题紧跟」只认紧挨着的；终稿里章标题和节标题之间隔着空段落，所以一个都没改（不删用户的空段落）。
- 双面时没有打开「对称页边距」（规范没写）。
- 三线表线宽用出版惯例（1.5 / 0.75 磅）；表头只认第一行（或第一行纵向合并到的那几行）。
- W3 留下的仍在：清单不能新增条目、PDF 不收、Word 选项卡里没有排版按钮、同一输入两次调用的边缘项有出入。

## 本轮改动

- 代码：
  - `src-tauri/Cargo.toml`：`security-framework` 移到只在 macOS 编译的依赖。
  - `src-tauri/src/agent/secret.rs`：钥匙串只在 macOS 编译、其他系统的空实现、`ApiKey` 多把密钥、3 项新测试。
  - `src-tauri/src/word/install.rs`：3 个测试只在 macOS 跑。
- 文档：
  - `docs/decisions/0020-v0.3-plan.md`（新）
  - `docs/questionnaire/round-2-answers.md`（新）
  - `ROADMAP.md`、`docs/requirements.md`
  - `CLAUDE.md`：规则 1、2、7
- 云端：`HANDOFF-cloud.md`（新）、`scripts/cloud-setup.sh`（新）。
- `HANDOFF.md`。

## 测试

- `pnpm check` 通过：前端引文排版 16；文档引擎 26 + 引文 18 + 排版器 12（另 1 项默认忽略）+ 结构摘要 5 + 计划 7 + 结构类 10；App 后端 200（原 197 + 密钥 3，另 15 项默认忽略）；Zotero 读取 9；Word PoC 29。
- `cargo clippy --workspace --all-targets`：0 警告。
- `Cargo.lock` 没变。
- `scripts/cloud-setup.sh`：`bash -n` 语法检查通过；没在 Ubuntu 上实际跑过。
- 没碰 `~/Pith/`、`~/Zotero/`、钥匙串；没运行任何改动 git 的命令。
