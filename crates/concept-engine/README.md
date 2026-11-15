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
| `align` | 中英对齐：章按（篇, 章）对上；章内段数相同按顺序一段对一段，不同就做单调对齐（一段可以对几段），给出每章的对法和置信度（见下「中英对齐」） |
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
- 平铺的目录（篇和章同一层）：章跟着前面最近的同层的篇（同层的篇比上一层的篇近：中文版的目录把第五编挂在第四编下面，后面同层的「第三章」是第五编的）；
  「Footnotes」后面同层的「Page 60」和夹在中间的章名是注释的小目录，都算注释；
- 单元名就是书名（「《国富论》」「The Wealth of Nations Books I-III」）的是书名页：它自己名下的段落（作者简介、分册封面）算前后附；
- 光写着「Introduction」、不在任何一篇里、后面隔着目录或书名页才到正文的，是编者的导读（斯密自己的是「Introduction and Plan of the Work」，篇里面的「INTRODUCTION」是那一篇的引言）；
- 「Introduction and Plan of the Work / 引言和本书计划」这种不在章里的正文单元，段落号写 `Intro.5`。

再在正文单元里逐块认：h1–h6、单元开头全是粗体或链接（或者文字和目录一样）的是标题；短的（≤ 160 字）`blockquote`、epub:type 是 marginalia / sidebar 的是旁注；
以「［1］」「[1]」开头（或开头是回到正文的注释链接）、并且被注释引用指向或前面正文有同一个标记的段落开始「注释区」，到单元结尾都是注释（「译者按」接在上一条注释后面）；
epub:type 是 footnote / endnote 的是注释；其余是正文。篇名单元开头不成句的短行是篇的标题，它下面的正文是这一篇的引言。
数据表格（每一格不超过 300 字）整张算一段正文：一行一行，格之间是制表符（普通段落里的空白都合并成空格，所以有制表符的就是表格）；拿表格排版的长文照旧一格一段。注释的引用标记（上标里带链接的「［1］」、epub:type="noteref"）不进正文，记下位置。

引用名全书唯一：正文 `I.i.3`、旁注 `I.i.3 旁注`、注释 `I.i 注 1`（续写的 `I.i 注 1（2）`）、标题 `I.i 标题`、别的单元 `前三章导读 ¶2`。译本里的在锚点里写成 `译:I.i 注 1`。

## 中英对齐

`align::align(原著, 译本)`（只用长度和锚点）/ `align::align_with(原著, 译本, Some(&相似度), &Options::default())`。结果是确定的。

- **章怎么对**：章按（第几篇, 第几章）；引言按篇（篇名单元自己名下的正文也算这一篇的引言）；其他不在章里的正文单元（附录）按引用名。
- **章内**：正文段数相同（`exact`）按顺序一段对一段，和 v0.4 一样。段数不同（`fitted`）做一次动态规划的单调对齐：允许 1-1、1-2、2-1、1-3、3-1，
  对不上的落空（1-0 / 0-1），顺序不交叉。一组的代价 = 对法的代价（合并 3、三合一 6、落空 5）+ 长度差（两边折算成中文字数，按全书的长度比换算；
  差几个标准差的平方的一半，上限 12；标准差 = sqrt(1.5²·L + 0.08²·L²)）− 共有的锚点（阿拉伯数字、译本里保留的拉丁字母词；按稀有度折算，每个 1，最多 3）
  − 相似度（调用方给的余弦，换算成「比同一章里错开三段的两段高出几个标准差」，一组里取最不像的那一对，权重 1，最多 6）。
  长度比先用全书的总量算，再用确实一段对一段的那些（段数相同的章、第一遍里对得稳的 1-1）重算一遍。表格段不算长度，落空只要 0.5。
- **置信度**（0–1）：对得稳的段落占多少（按长度）。落空的、长度差出 2.5 个标准差又没有锚点撑着的算不稳；有相似度时看相似度（1-1 低于 1.5 个标准差、合并的低于 0.5 个）。
  低于 0.85 标 `low`（页面提示「可能有偏差」）；低于 0.4 这一章就不对（`none`：不硬对）。段数相同的章照样算一遍：对法不变，只是长短明显对不上时标 `low`（多半是段数碰巧相同、中间错开了）。
- **数据**：`Alignment.pairs` 是（原著 seq, 译本 seq），一对多、多对一写成多行；`other_of` / `original_of` 返回 `Vec<usize>`，`group_of` 给一段所在的那一组，`counterpart` 返回一组段落。
  每个 `UnitAlignment` 有 `method`（exact / fitted / none）、`confidence`、`low`、`merged`（合并的组数）、`unmatched`（落空的段数）。v0.4 存的 JSON 没有这些字段，读的时候按 `aligned` 补。
  算法或默认权重变了就把 `align::VERSION` 加一：App 读到旧版本算的对齐会重算。
- **相似度**是 `align::Similarity = &dyn Fn(原著段 seq, 译本段 seq) -> Option<f32>`（余弦，哪一边没有向量就返回 None，那一组退回只看长度和锚点）。引擎不联网、不算向量；
  App 用本机向量模型的段落向量。「不相干」的基线是引擎自己从调用方给的相似度里取样算的，换模型不用改代码。

默认权重是用两本真实的《国富论》（Penguin 英文版、杨敬年中译本，2026-11-15）定的，`tests/real_books.rs` 可以重跑：

| | v0.4 | 只用长度和锚点 | 加语义相似度（Qwen3-Embedding-0.6B） |
| --- | --- | --- | --- |
| 有正文的单元对上的 | 19 / 35 | 36 / 36（段数相同 20，算法对的 16，其中 1 个置信度低：附录） | 35 / 36（附录内容对不上，标成没对上） |
| 原著正文段有中译的 | 561 | 2096 / 2118（没有的 22 段多数是表格，中文版里是图片） | 2083 / 2118 |
| 对法 | 1-1 | 1-1 2038 组、1-2 8、2-1 22、3-1 2 | 1-1 2043、1-2 6、2-1 17 |
| 数字锚点落在对上的那一组里 | | 段数相同的章 57 / 57，算法对的章 225 / 227 | 同左 |

两种做法在算法对的 15 章里 99.1% 的段落对法一样。「改乱再找回」（把 19 个段数相同的章共 561 段当答案，按固定规则合并、删段）：

| 改动多少 | 只用长度和锚点 | 加语义相似度 |
| --- | --- | --- |
| 约每 20 段一处 | 99.2% | 100% |
| 约每 8 段一处 | 96.4% | 99.1% |
| 约每 3 段一处 | 89.2% | 98.1% |

合并和拆分两种做法都几乎全找得回（约每 8 段一处时 32 / 34 和 33 / 34）；差别在整段被删掉的情形：只看长度时，没人要的短段落常被并进旁边那一组。
这本书上锚点几乎不起作用（正文里的数字太少），留着是为了数字多的书；长度的尺度比实测（1.05、0.046）放宽了一些，免得只合这一个译本，放宽一倍则明显变差。
向量模型下，对着的两段余弦 p10 0.77 / p50 0.83，同一章里错开一段的 p50 0.60 / p90 0.71，错开三段的平均 0.53、标准差 0.10；合并的两段里较不像的那一半 p10 0.63 / p50 0.71。

## SKILL.md 的格式

```markdown
---
name: chapter            # 小写英文、数字、-；和文件夹名一样
title: 整章分析           # 中文标题，界面上显示
version: 1
description: 一句话说明
entries: [chapter]       # 能从哪些入口开始：chapter / selection / question / concept / card
produces: [concept, argument, relation]   # concept / argument / comment / counterfactual / note / relation
instruction: optional    # 要不要用户写一句要求：required（「自由」）/ optional（默认）
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

加上 `PITH_EMBED_URL=http://127.0.0.1:8081/v1/embeddings PITH_EMBED_MODEL=qwen3-embedding-0.6b`（本机向量服务，OpenAI 兼容）再比较语义相似度；向量缓存在 `target/tmp/`。

前端 `?mock` 的示例数据由引擎从夹具生成：

```bash
cargo run -q -p concept-engine --example mock_data > src/dev/mockConcept.json
```

## 测试

- `tests/text.rs`：夹具 EPUB 3 / EPUB 2（page-map + 拆分文件）→ 段落号、页码（跨页「101–102」）、角色、中文的旁注 / 注释 / 译者按 / 导读、对齐（前两章段数相同按顺序对，第三章 8 段对 7 段由算法对）、「章.段」写法；
- `text.rs` / `epub.rs` / `align.rs` 的单元测试：两种合成的目录（平铺的篇章与脚注小目录、书名页、编者的导读；挂错层的第五编）的归类，数据表格合成一段；
  对齐：合并、拆分、多出一段；段数相同照旧；**改坏**（一段挪走不硬对、差得太远不对齐）；锚点与相似度；表格落空；旧 JSON；
- `tests/flow.rs`：录制的回复跑通整章（三步、可跳步）、划选、问题、追踪、反方（文献库 / 再上网）、推演、自由，到核对、审计、三种导出；
  改坏测试（引文改一个字、挂错段落、不存在的段落号、标成原文却没有锚点、推演混进原文类、旁注或导读被标成原文、他人观点没有来源、他人挂到原著正文、卡片种类不对）全部拦下；
  审计查出存下的结果被改（引文、锚点、状态、页码、关系、存下的文本）；取消、出错后接着做；发送前的估算；
- `tests/real_books.rs`（忽略）：两本真实 EPUB 第一篇第 1–3 章 11 / 5 / 8 段、英文页码 109–126、归类（只有引言、各章、附录是正文）；全书对齐（每个单元都有结果、段数相同的章和按顺序对一样、顺序不交叉），
  给了 `PITH_EMBED_URL`（本机向量服务，`curl` 调）再比较加语义相似度的做法和一组权重。
