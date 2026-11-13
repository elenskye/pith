# concept-engine：Pith 的概念研究引擎

v0.4 Concept Research（`docs/decisions/0024-v0.4-plan.md`）的纯逻辑部分。不联网、不碰数据库、不依赖 Tauri 和 macOS；
模型、搜索、下载的接口沿用 `research_engine::ports`，真实实现（OpenAI Responses、Mathpix、文献库）在 `src-tauri/src/concept/`。

原则同 v0.3：模型只负责「读」和「写」（出卡片和关系），角色、段落号、页码、对齐、逐字核对、审计都由代码做，并且能从存下的文本重做一遍（`audit`）。

| 模块 | 作用 |
| --- | --- |
| `markup` | 容错的 XML / XHTML 读取（HTML 实体、自闭合、没关上的标签都认，从不失败）。不用 roxmltree（遇到 `&nbsp;` 报错）、scraper（把自闭合的 `<a id="page_110"/>` 当成没关上的 `<a>`） |
| `epub` | 容器 → OPF（元数据、清单、书脊）→ 目录（EPUB 3 nav / EPUB 2 ncx）→ 页码表（page-list / ncx pageList / `page-map.xml`，页码表里拆分前的文件名换成 `*_split_NNN`）→ 每份正文的块（文字、id、段内分页点、注释引用） |
| `text` | 单元（篇、章、节、导读、注释、前后附）、段落角色、段落号（`I.iii.5` / `3.5` / `5`）、页码（段落开头 + 段内分页点；`pages_of` 给一句引文的「110–111」）、结构预览 |
| `mathpix` | Mathpix `lines.json` → 段落（页码 = PDF 页序）；本机取文字的后备 `from_pages` |
| `align` | 中英对齐：同一章正文段数相同就按顺序对齐，不同就不对齐、标出来 |
| `fold` | 概念研究的折叠：v0.3 的折叠 + 弯直引号、撇号样式、拉丁字母大小写（v0.3 的 `fold` 不动） |
| `skill` | `SKILL.md` 的读取和格式检查；七个内置技能（仓库 `skills/`，编进来）；用户技能同名覆盖内置的 |
| `model` | 一次研究：范围、步骤记录、卡片（句：类别 + 锚点 + 核对结果）、关系、评论来源、拦下的句子 |
| `prompts` | 系统提示词（规则 + 技能总则 + 步骤做法）、所有技能共用的 JSON Schema（OpenAI 严格模式能用）、每步发什么、宽容的解析 |
| `verify` | 每句按类别核对；`admit` 把一步的输出核对后放进研究（核不上的拦下，一句不剩的卡片不要，关系两头要都在） |
| `audit` | 从存下的文本把每句重新核一遍，查存下的结果有没有被改坏 |
| `flow` | 按技能的步骤跑（模型 / 查文献库 / 网页检索），每步存，可取消，接着做；`planned` 发送前算每步发什么、最坏多少 |
| `graph` | 关系图：确定的分层布局 + 一份 SVG（App 和 HTML 导出共用） |
| `render` | 给 Markdown / Word 的 `Doc`（复用 `research_engine::render`）；单文件 HTML（卡片筛选、点节点跳卡片、点锚点跳原文并高亮） |
| `fixture` / `mock` | 夹具 EPUB（现拼，不放二进制，见 `fixtures/wealth/README.md`）；录制的模型回复（`fixtures/replies/`）、模拟的检索、下载、文献库 |

## 段落的角色怎么认

不看 class 名。先按目录项的文字分单元（`text.rs` 的 `classify`）：

- 「Book One / BOOK IV / 第一编」是篇，「CHAPTER I / 第三章」是章，「PART I / 第一部分」是节（段落号接着章往下编）；
- 「导读 / 导言 / Analytical Introduction / 译者导言」是导读（先认：「〔第四章导读〕」不是第四章）；
- 「Notes / Footnotes / 注释」是注释，「Contents / 目录 / Index / 版权页 / 序 / 关于译者」等是前后附；上一层是这三种的，下面都跟着它；
- 平铺的目录（篇和章同一层）：章跟着前面最近的同层的篇；
- 「Introduction and Plan of the Work / 引言和本书计划」这种不在章里的正文单元，段落号写 `Intro.5`。

再在正文单元里逐块认：h1–h6、单元开头全是粗体或链接（或者文字和目录一样）的是标题；短的（≤ 160 字）`blockquote`、epub:type 是 marginalia / sidebar 的是旁注；
以「［1］」「[1]」开头（或开头是回到正文的注释链接）、并且被注释引用指向或前面正文有同一个标记的段落开始「注释区」，到单元结尾都是注释（「译者按」接在上一条注释后面）；
epub:type 是 footnote / endnote 的是注释；其余是正文。注释的引用标记（上标里带链接的「［1］」、epub:type="noteref"）不进正文，记下位置。

引用名全书唯一：正文 `I.i.3`、旁注 `I.i.3 旁注`、注释 `I.i 注 1`（续写的 `I.i 注 1（2）`）、标题 `I.i 标题`、别的单元 `前三章导读 ¶2`。译本里的在锚点里写成 `译:I.i 注 1`。

## SKILL.md 的格式

```markdown
---
name: chapter            # 小写英文、数字、-；和文件夹名一样
title: 整章分析           # 中文标题，界面上显示
version: 1
description: 一句话说明
entries: [chapter]       # 能从哪些入口开始：chapter / selection / question / concept / card
produces: [concept, argument, relation]   # concept / argument / comment / counterfactual / note / relation
steps:
  - id: concepts
    kind: model          # model（调一次模型）/ library（查文献库）/ web（网页检索 + 本机下载 + 快照）
    title: 概念
    produces: [concept]  # 模型步骤必填，要在技能的 produces 里
    input: [scope, translation]   # scope / translation / cards / sources / question / instruction / terms
    max_output_tokens: 16000      # 模型步骤必填，1000–64000；最坏费用按它算
    optional: true       # 用户可以不跑这一步
  - id: web
    kind: web
    max_results: 3       # library / web：最多几条（1–10，默认 5）
    off_by_default: true # 默认不跑，用户勾选才跑
---

## 总则
每一步都带上的做法。

## 步骤 concepts
这一步的做法（模型步骤必须有这一节）。
```

元数据是 YAML 的一个小子集：「键: 值」、`[a, b]` 列表、`- ` 开头的列表项（可以是几行「键: 值」），只能用空格缩进。输出格式（卡片、句、类别、锚点）不在技能里写：所有技能共用 `prompts` 的规则和 JSON Schema，加新技能不用改代码。

## 命令

```bash
cargo test -p concept-engine
```

两本真实 EPUB（默认忽略；书不进 Git，只打印聚合数字）：

```bash
PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub cargo test -p concept-engine --test real_books -- --ignored --nocapture
```

前端 `?mock` 的示例数据由引擎从夹具生成：

```bash
cargo run -q -p concept-engine --example mock_data > src/dev/mockConcept.json
```

## 测试

- `tests/text.rs`：夹具 EPUB 3 / EPUB 2（page-map + 拆分文件）→ 段落号、页码（跨页「101–102」）、角色、中文的旁注 / 注释 / 译者按 / 导读、对齐（第三章段数不同不对齐）、「章.段」写法；
- `tests/flow.rs`：录制的回复跑通整章（三步、可跳步）、划选、问题、追踪、反方（文献库 / 再上网）、推演、自由，到核对、审计、三种导出；
  改坏测试（引文改一个字、挂错段落、不存在的段落号、标成原文却没有锚点、推演混进原文类、旁注或导读被标成原文、他人观点没有来源、他人挂到原著正文、卡片种类不对）全部拦下；
  审计查出存下的结果被改（引文、锚点、状态、页码、关系、存下的文本）；取消、出错后接着做；发送前的估算；
- `tests/real_books.rs`（忽略）：两本真实 EPUB 第一篇第 1–3 章 11 / 5 / 8 段、英文页码 109–126、中英对齐。
