# Phase 0-B：Word 动态引用 PoC

验证「本地文献 → Word 里插入引文 → 生成书目 → 切换样式 → 保存重开再刷新」能否在本机 Word for Mac 跑通。结果与结论见 `docs/feasibility.md` 0-B 一节，路线决策见 `docs/decisions/0006`。

三条路共用同一套数据格式与刷新逻辑（`src/`）：

| 路线 | 入口 | 在本机 Word 16.114 的状态 |
| --- | --- | --- |
| docx 直写（Word 外） | `docx/rl-docx.ts` | ✅ 实测通过 |
| Word 宏（Zotero 式，VBA → AppleScriptTask → Node） | `vba/` + `docx/rl-bridge.ts` | ✅ 实测通过 |
| Office.js 加载项（任务窗格） | `src/taskpane.ts`、`src/word.ts`、`manifest.xml` | ⛔ 侧载被 Word 忽略，未能实测（只过了类型检查与构建） |

## 文档里的数据格式

- 每处引文：行内内容控件，tag `rl:cite:<引文ID>`，内容锁定（不能手改文字）
- 参考文献表：块级内容控件，tag `rl:bibliography`
- 数据：一个 Custom XML Part，命名空间 `urn:research-lab:citations:v1`，内含 JSON：样式、每处引文的文献与页码、文献快照（换电脑也能刷新）。带数据版本号，版本比程序新时只读不改
- Zotero 已有的域（`ADDIN ZOTERO_ITEM …`）只读识别，不改

## 目录

```
src/        共用：types / styles / runs（HTML→带格式片段）/ engine（citeproc）/ store（数据部件）/ refresh（刷新计划）
            Office.js：word.ts（读写 Word）、taskpane.ts（界面）
docx/       Word 外：docx.ts（读写 .docx）、ops.ts（生成 / 刷新 / 校验）、rl-docx.ts（命令行）、rl-bridge.ts（给宏用的计算后端）
vba/        ResearchLab.bas（宏模块）、ResearchLab.applescript.in、install-word-bridge.sh
public/     csl/ 样式、locales/ 语言文件（来源见 csl/SOURCES.md）、refs.json（7 条合成文献）、icons/
scripts/    gen-manifest.ts、make-icons.ts
tests/      node:test 单测（29 项）
```

## 命令

```bash
pnpm install
```

```bash
pnpm check
```

`pnpm check` = `tsc --noEmit` + 全部单测。

Word 外生成、刷新、校验（不改写输入文件；输出建议放仓库外或 `local/`）：

```bash
node docx/rl-docx.ts seed ../../local/tmp/t.docx --zotero
```

```bash
node docx/rl-docx.ts refresh ../../local/tmp/t.docx --out ../../local/tmp/t-gbt.docx --style gbt7714-2015-numeric
```

```bash
node docx/rl-docx.ts verify ../../local/tmp/t-gbt.docx --zotero-baseline ../../local/tmp/t.docx
```

可选样式：`apa`、`gbt7714-2015-numeric`、`gbt7714-2015-author-date`、`gbt7714-2025-numeric`、`chicago-author-date`。

## Word 宏路线

1. 安装 AppleScript 桥（只写 `~/Library/Application Scripts/com.microsoft.Word/ResearchLab.scpt` 一个文件）：

```bash
bash vba/install-word-bridge.sh
```

2. Word 打开文档 → 工具 › 宏 › Visual Basic 编辑器 → 文件 › 导入文件 → `vba/ResearchLab.bas` → 另存为「启用宏的 Word 文档（.docm）」
3. 重新打开 .docm，选「启用宏」。工具 › 宏 › 宏… 运行：`RL_InsertCitation`（插入引文）、`RL_InsertBibliography`（插入书目）、`RL_SwitchStyle`（切换样式）、`RL_Refresh`（刷新）
4. 卸载：

```bash
bash vba/install-word-bridge.sh remove
```

已知限制：桥接脚本在 `~/Documents` 下时，macOS 会要求给 Word「文稿」文件夹权限；Mac 版 VBA 的输入框不能显示中文（显示成下划线）；每次打开 .docm 都要点「启用宏」。详见 feasibility。

## Office.js 路线

Word for Mac 要求加载项地址是 HTTPS。证书放在仓库外的 `local/certs/`（不进 Git），用下面的命令生成，**并由你自己在「钥匙串访问」里设为始终信任**（这是系统安全设置，Agent 不代做）：

```bash
mkdir -p ../../local/certs && cd ../../local/certs && openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 365 -keyout localhost.key -out localhost.crt -subj "/CN=Research Lab Dev localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" -addext "basicConstraints=critical,CA:FALSE" -addext "extendedKeyUsage=serverAuth"
```

```bash
pnpm dev
```

```bash
node scripts/gen-manifest.ts https://localhost:3127
```

然后用 Finder 把 `manifest.xml` 复制到 `~/Library/Containers/com.microsoft.Word/Data/Documents/wef/`，重启 Word。**本机 Word 16.114 目前不会读取这个文件夹**（微软的官方示例清单也一样），等微软修复或改用 Word 网页版的「上传我的加载项」。
