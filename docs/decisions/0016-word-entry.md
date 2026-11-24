# 0016 · v0.2 W1 Word 入口：工程上的做法

- 日期：2026-11-10
- 状态：已定（工程选择，可逆，按 `CLAUDE.md` 规则 3 直接定；用户验收时可改）
- 依据：`0006`（数据格式与接入路线）、`0007`（文档引擎）、`0015`（v0.2 开工前的选择）

## 定下的

1. **文档引擎成为工作区 crate**：`poc/docx-format` 移到 `crates/docx-engine`（包名 `docx-engine`），`src-tauri` 以 path 依赖引用。排版器、校验器原样保留；新增：
   - `cite`：0-B 引文写入的 Rust 移植——数据部件（`store`）、刷新计划（`plan`，纯函数）、Word 外改 .docx（`docx`）、独立核对（`check`，roxmltree，与写入用的不是同一套解析器）；
   - `vba`：从源码生成 vbaProject.bin；`template`：生成 Word 全局模板 .dotm。
2. **文档里的名字改成 Pith**：引文控件 tag `pith:cite:<ID>`、书目 `pith:bibliography`、数据部件命名空间 `urn:pith:citations:v1`。0-B 原型的旧名（`rl:cite:`、`rl:bibliography`、`urn:research-lab:citations:v1`）照样读，刷新时换成新名（Word 宏与 Word 外两条路都是）。目前只有原型测试文档用过旧名，改名代价最小。
3. **刷新逻辑在 Rust，排版在 WebView**：`cite::plan` 决定哪些引文要排、复制粘贴的重复 ID、缺数据、文献快照；排版请求经事件 `cite-render` 发给前端，`src/cite/` 用 citeproc-js 排好，以带格式片段经 `cite_render_done` 交回。前端挂好监听后调用 `cite_renderer_ready`；Pith 被 Word 在后台拉起时，请求最多等 40 秒就绪。
4. **本机服务**（`src-tauri/src/word/service.rs`）：只监听 127.0.0.1，端口由系统分配；每次启动生成 32 字节随机令牌；`service.json`（端口、令牌、进程号、版本）写进数据目录、权限 0600、退出时删（只删自己写的那份）。请求必须带 `Authorization: Bearer <令牌>`；带 `Origin` 的（浏览器）一律拒绝；不支持分块传输；请求体上限 64 MB。路由：`/v1/ping`、`/v1/word`（宏协议）、`/v1/docx/refresh`、`/v1/docx/verify`（Word 外刷新与核对，自动化与验收用；只写新文件）。
5. **宏协议**：沿用 0-B 的按行制表符文本（VBA 好解析），命令 ping / pick / refresh / show；格式长度按 UTF-16 计。Word 里显示的提示全部英文（`0006`），由 Rust 的 `Issue::en` / `StoreError::en` 给出。
6. **全局模板从源码生成**，不在 Word 里手工导入：`src-tauri/word/Pith.bas`（ASCII）+ `customUI.xml` → `vba::build` → `template::build_dotm`。vbaProject.bin 按 [MS-OVBA] 写：
   - 只写源码，不写性能缓存（`_VBA_PROJECT` 版本 0xFFFF，规范要求写入时如此），Word 打开时重新编译；
   - 不写任何引用；Office 库的类型（IRibbonControl、CustomXMLParts）在宏里一律用 `Object`；
   - ThisDocument 的基类写 Word Document 类的 CLSID（`0{00020906-…}`），代码页 10000、SysKind 3——对照 Word for Mac 自己存的工程（Zotero.dotm）定的；最初写成 `1Normal.ThisDocument` 时 Word 报「找不到工程或库」；
   - 生成结果确定（同样的源码得到同样的字节），设置页据此判断已装的模板是不是最新的。
   - 实测（Word 16.114.4，2026-11-10）：模板加载、选项卡、四个按钮都跑通——插入测试引文（Pith 没开时自动在后台启动）、插入书目、刷新、切到 Pith；Word 存下的文档经确定性核对一致；Pith 在 Word 外改过的文档 Word 能打开、宏能接着刷新。
7. **安装**（设置 › Word）：
   - `Pith.dotm` → Word 启动文件夹（`~/Library/Group Containers/UBF8T346G9.Office/User Content(.localized)/Startup(.localized)/Word/`）；
   - `Pith.scpt`（osacompile 编译）+ `Pith-bridge.sh` → `~/Library/Application Scripts/com.microsoft.Word/`；
   - 安装记录 `数据目录/word/installed.json`；状态只看记录与脚本，不碰 Office 组容器（免得每次打开设置页都触发系统的「访问其他 App 的数据」提示）；
   - 组容器写不进去时，模板存进 `数据目录/word/Pith.dotm`，设置页给出手动拖放的步骤；
   - 开发版与正式版装的是同一组文件名，后装的覆盖先装的；设置页显示脚本连的是哪个数据目录。
   - **实测**：本机上 macOS 不让 Pith（本地构建、未正式签名）写 Word 启动文件夹，命令行也一样（「Operation not permitted」，没有弹出授权提示）；Application Scripts 能写。所以眼下模板要用户手动拖进启动文件夹一次（每次模板更新也要再拖）。以后可改成让 Pith 请访达代为复制（要用户允许 Pith 控制访达），或给 App 正式签名后再试。
8. **Pith 没开时**：桥接脚本读不到 service.json 或连不上（curl 退出码 7）就启动 Pith：正式版 `open -g <Pith.app>`，开发版直接起可执行文件（要 `pnpm dev` 在跑）；用 `PITH_DATA_DIR` 启动的 Pith 装的桥，拉起时带同一个数据目录。最多等 60 秒服务起来。
9. **请求体走临时文件**：AppleScript 把宏的请求写进临时文件（UTF-8），桥接脚本用 `curl --data-binary @文件` 发，避免整篇数据部件挤进命令行参数。
10. **书目插在当前段落下面**：先插两个空段、把第一个包成块级控件，保证控件不包住文档最后一个段落标记（直接用 `rng.Next` 在文档末尾会拿到空值，Word 报类型不匹配——实测发现）。
11. **W1 的插入是测试引文**：选文献这一步先用「最近加入文献库的一条」，按钮叫 Test Citation；W2 换成置顶搜索窗（`0015`）。
12. **新文档默认样式**：GB/T 7714-2015 顺序编码制。内置样式清单只写一份：`src/cite/styles.json`（前端找 CSL 文件、Rust 校验样式 ID 都读它）。
13. **文献 → CSL-JSON** 在 Rust（`word/csl.rs`），按 Zotero 的 CSL 映射换类型、字段、作者角色；语言字段归一成 `zh-CN` / `en-US`（原字段为空时用按内容判定的），GB/T 7714 靠它分中英文著录。真实副本 88 条全部能换。

## 不选

| 方案 | 原因 |
| --- | --- |
| 在 Word 的 VBA 编辑器里手工导入宏、存成模板再提交二进制 | 每改一次宏都要人工操作；生成器可测、可复现 |
| 写 stdole / Office 库引用 | 引用里带 Word 安装路径，换路径就变成「缺失的引用」，宏整个编译不过 |
| 宏直接起 Node（0-B 原型） | 「文稿」文件夹权限弹窗；要求装 Node（`0006`） |
| 用 tiny_http 等现成 HTTP 库 | 只有 curl 一个客户端，最小子集手写约 150 行，少一个依赖 |
| 每次在 Rust 里内嵌 JS 引擎跑 citeproc | 重型依赖；WebView 现成（`0007`） |

## 后果

- 改宏、改选项卡：改 `src-tauri/word/` 下的源文件，重新在设置里安装（「更新」），重开 Word。
- Word 外的刷新与核对已经能用（服务的 `/v1/docx/*`），App 界面里还没有入口；W2 / W3 需要时再加。
- 已知样式问题（0-B 留下的）在 W2 处理：GB/T 顺序编码不输出引文页码、APA 下中文作者只出姓等。
