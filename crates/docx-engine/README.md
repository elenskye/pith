# docx-engine：Pith 的 Word 文档引擎

v0.2 起由 `src-tauri` 引用（原 `poc/docx-format`，`decisions/0016`）。来历：Phase 0-C 验证「按一份论文规范改 .docx 的格式，正文一个字都不变」，结果见 `docs/feasibility.md` 0-C 一节，选型见 `decisions/0007`。

v0.2 W1 新增：

| 模块 | 作用 |
| --- | --- |
| `src/cite/` 引文 | 0-B 引文写入的移植：`store` 数据部件（新旧命名空间都读）、`plan` 刷新计划（纯函数，排版经 `Render` 交给 citeproc-js；交给排版器前把分栏的中文姓名合成全名）、`docx` Word 外改 .docx、`check` 独立核对（roxmltree）、`fixture` 合成文档（含合成的 Zotero 文档）与不依赖 citeproc 的测试排版器。W2：`zotero` Zotero 引文域与文档设置的解析、按条目 key 对应；`convert` Word 外把 Zotero 引文与书目换成 Pith 控件（写完核对）；`csl_style` 导入 .csl 的检查；`node` 在 Node 里跑前端排版器（`scripts/cite-render.ts`，命令行与真实数据测试用） |
| `src/vba.rs` VBA 工程 | 从源码生成 vbaProject.bin（[MS-OVBA]：压缩、dir 流、PROJECT 流、复合文件）；`read_back` / `dump` 读回对照 |
| `src/template.rs` Word 模板 | RibbonX + VBA 工程 + 选项卡图片 → 全局模板 .dotm（结果确定） |

调试 Word 模板：`cargo run -p docx-engine -- dotm <输出.dotm> --bas <模块.bas> --ribbon <customUI.xml>`（`customUI.xml` 同目录下 `images/*.png` 一起打包）；看 Word 自己存的工程：`cargo run -p docx-engine -- vba-dump <vbaProject.bin>`。

v0.2 W3 新增（排版 Agent 用，`decisions/0018`）：

| 模块 | 作用 |
| --- | --- |
| `src/outline.rs` 结构摘要 | 只读：段落（样式、所在分节、文字、是否在表格里、含图、对齐）、样式用量、分节；`preview` 算出一份计划会改到哪些段落（与排版器的映射规则一致，测试对过数）；`doc_text` 把规范、模板读成纯文字（表格按行、文本框里的标注也读） |
| `src/checks.rs` 只检查项 | 中文摘要字数（Word「字数」的算法）、关键词个数与分隔、参考文献条数与外文占比、图表是否按章编号；找不到要统计的地方就说没找到 |

规范里的页面设置每一项都可以不写（`spec::Page`）：不写就保留原文档的值。

v0.2 W4 新增（结构类排版，`decisions/0019`）：

| 模块 | 作用 |
| --- | --- |
| `src/structure.rs` 结构类排版 | 在格式排完之后做：按计划里的分界点分区（封面 / 前置部分 / 正文 / 正文之后，没有分节符的地方加，不新增段落）、页眉（文字与章名域、横线、单面 / 双面）、页码（各区的数字形式与起始）、正文从奇数页开始、三线表、脚注编号、标题紧跟上一级时段前不空 |
| `src/verify_structure.rs` 独立核对 | 保真：结构类排版允许的改动（新页眉页脚部件及其登记、分界处的分节符）按规范和计划自己推，其余一概不许变；排版后子元素顺序不合规定的元素不能增多。生效：六类结构逐项核对 |

规范里写 `structure`（页眉、页码、`body_on_odd_page`、`three_line_tables`、`footnote_numbering`、`tight_headings`），计划里写 `layout`（`front_start` / `body_start` / `back_start` = 段落编号 + 开头文字，`duplex`，`skip_tables`）。例子见 `tests/structure.rs`。`verify::verify_all` 一次做完保真、受保护分节、规范、结构四样核对。

Phase 0-C 的部分：

| 模块 | 作用 |
| --- | --- |
| `src/format.rs` 排版器 | 改样式（按内置样式名找，去掉主题字体）、页面设置（保留横向）、清理与样式冲突的直接字体 / 字号 / 颜色 / 段落间距；代码段落、公式、列表缩进不动；修订、批注只报告不处理 |
| `src/verify.rs` 校验器 | 用另一个解析器（roxmltree）抽取正文、脚注、尾注、批注、页眉页脚的文字、删除文字、域代码、公式文字和 30 类结构计数；不该改的部件逐字节比对；另核对规范是否在样式层生效 |
| `src/plan.rs` 文档计划 | 针对某一份文档：样式映射（粘贴来的 Normal (Web)、p1 → 正文；标题层级整体升降）、按文字识别段落（题注）、受保护分节（封面：外观冻结并逐段核对） |
| `src/fixture.rs` 合成文档 | `build()`：三级标题、目录域、脚注尾注、批注、修订、代码块、合并单元格表格、图片、公式、SEQ 题注、交叉引用、编号列表、横向分节、页眉页脚、0-B 引文控件；`build_real_like()`：仿真实论文（直接格式排版的封面、粘贴样式的正文、手动题注、参考文献样式）。样式 ID 仿中文版 Word |

底层：`src/xml.rs` 是基于 quick-xml 的可编辑 XML 树，**没改过的标签和文字按原字节写回**；`src/package.rs` 读写 zip，**没改过的部件按原压缩字节复制**。

## 命令

```bash
cargo test -p docx-engine
```

```bash
cargo run -p docx-engine -- fixture ../../local/tmp/t.docx
```

```bash
cargo run -p docx-engine -- apply ../../local/tmp/t.docx --spec spec/thesis-sample.json --out ../../local/tmp/t-formatted.docx --report ../../local/tmp/t-report.md
```

```bash
cargo run -p docx-engine -- apply <论文.docx> --spec <规范.json> --plan <计划.json> --out <输出.docx> --report <报告.md>
```

```bash
cargo run -p docx-engine -- verify <排版前.docx> <排版后.docx> --spec spec/thesis-sample.json [--plan <计划.json>]
```

`apply` 写出前自动做保真校验和规范核对，报告是中文 Markdown；不通过时退出码为 1。永不改写输入文件。

真实 Word 存过的文档（不进仓库）：

```bash
RL_WORD_DOCX=<绝对路径> cargo test -p docx-engine --test format -- --ignored
```

## 排版规范

`spec/thesis-sample.json` 是一份**合成的**毕业论文规范（宋体 / Times New Roman 小四、1.5 倍行距、首行缩进 2 字符、三级黑体标题、A4 页边距 2.54 / 3.17 cm 等），不代表任何学校的真实要求。字段：`page`（含可选 `gutter_cm`）、`body`、`headings`、`caption`、`table_text`、`figure`、`footnote`、`bibliography`、`header_footer`、`toc`、`code`、`direct_formatting`（含 `keep_fonts`：黑体、楷体等直接设置视为有意保留）、`update_fields_on_open`。每个文字样式可写 `line_spacing`（倍数）或 `line_spacing_exact_pt`（固定值）、`page_break_before`。

真实学校规范与两份真实论文的计划在 `local/real-thesis/`（不进 Git）。v0.2 由 Agent 从需求文档提炼规范、分析文档生成计划（`docs/decisions/0008`）。

## 文档计划

```json
{
  "style_map": { "Normal (Web)": "Normal", "p1": "Normal", "heading 2": "heading 1" },
  "paragraph_rules": [ { "pattern": "^(图|表)\\s*\\d+(\\s|：)", "max_chars": 40, "style": "caption" } ],
  "protect_sections": [1]
}
```

样式都按名字匹配（不区分大小写）；目标样式缺失时按内置名新建，建不了就报错。受保护分节的段落不清理、不映射；样式改动会改变它的外观时，把原外观写成直接格式，`verify --plan` 逐段核对外观前后一致。

## 评估其他库

```bash
cargo run -p docx-engine --example eval_rust_libs -- <输出目录> <输入.docx>...
```

python-docx（装在仓库外的虚拟环境，不是项目依赖）：

```bash
python3 -m venv ../../local/venv-0c && ../../local/venv-0c/bin/pip install python-docx==1.2.0
```

```bash
../../local/venv-0c/bin/python -I eval/python_docx_eval.py <输入.docx> <输出目录>
```

## 已知限制

- 只支持 A4（规范不写纸张就不改纸张）；公式编号位置、四线表、表注、章节编号、目录生成等「需要改内容或结构」的要求没做
- 结构类排版的结果还没有在 Word 里目视过（W5）；页眉里的章名是 `STYLEREF 1` 域，Word 排版时才取到文字
- 目录、交叉引用的页码是 Word 缓存的结果，排版后需在 Word 里更新域（F9）或开 `update_fields_on_open`
- 文本框里的段落按正文处理；图表、SmartArt、嵌入对象只报告
- 命名空间前缀从根元素读取（OOXML 惯例）；局部重新声明前缀的文档未测
