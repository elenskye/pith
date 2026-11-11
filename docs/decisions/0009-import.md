# 0009 · 导入：Zotero 映射、核对与文件格式

- 日期：2026-11-08
- 状态：已定（v0.1 Phase 2，Agent 按已定原则自行决定，HANDOFF 汇报）
- 依据：`0002`（完全替代 Zotero、回收站原样导入）、`0003`（数据目录与备份）、`docs/schema.md`（S1–S4）、`docs/zotero-mapping.md`、用户「存全不等于显示全」（2026-11-08）

## 1. 数据库补全：migration 0002

0001 装不下 Zotero 的几类数据。按 S2 / S3 的原则补上，**不改动已同意的 4 个取舍**：

| 缺口 | 0002 的做法 | 真实库里有多少 |
| --- | --- | --- |
| 附件、笔记、标注也能带标签 | `attachment_tags` / `note_tags` / `annotation_tags` | 0（夹具覆盖） |
| 独立附件、独立笔记可以直接进集合 | `collection_attachments` / `collection_notes` | 0（有 4 个独立附件，未进集合） |
| 笔记内嵌图片的上级是笔记 | `attachments.note_id` | 0 |
| 旧版 Zotero 附件自带一段笔记 | `attachments.note` | 0 |
| 标注作者、是否读自 PDF | `annotations.author_name` / `is_external` | 0 |
| 其余没有列的原始数据 | 5 张主表各加 `source_meta`（JSON） | 附件的 accessDate 等 |
| 记录来自哪次导入 | 5 张主表各加 `import_run_id` | — |

`source_meta` 里放的：Zotero 的库 ID、version / synced / clientDateModified（v0.1.x 增量导入要用）、附件的全部字段与 linkMode / charset / path / storageHash 等、关联、快照资源文件清单、上级找不到时的原上级 key；BibTeX / RIS / CSL JSON 的整条原始记录。**界面不显示**它，但一个字段都不丢。

另加了外键子列的索引（详情、集合、级联删除用）。

## 2. Zotero 导入的做法

- **只读副本**：读取层 `crates/zotero-import`（0-A），`mode=ro&immutable=1`，不改副本任何字节（测试前后核对哈希）。目录旁有 `-journal` / `-wal` 时提示「Zotero 可能正开着」。
- **两道核对**，任一不一致整批撤销、删掉这次新复制的文件：
  1. 读取核对：Zotero 数据库 ↔ 读出的数据（0-A 的独立 SQL 审计，15 类）；
  2. 写入核对：读出的数据 ↔ 库（14 类，逐条规范化后比较；附件文件看文件库里实际的文件大小）。只读库里的列与 `source_meta`，不看写入时的中间变量。
- **字段**：全部原始字段进 `item_fields`，常用列（标题、日期排序值、年份、期刊 / 书名、DOI、ISBN、语言、摘要、extra）由字段推出；按字符判定 `detected_lang`（zh / ja / ko / en）。日期保留 Zotero 两段式「排序值 原文」。
- **作者**：fieldMode 1（单栏，中文姓名、机构）存 `literal`，不拆。
- **回收站（S4）**：只在直接删除的记录上写 `deleted_at`；子附件 / 笔记随父条目；上级集合在回收站的子集合不显示。
- **附件**：存在 storage 的、链接文件（含 `attachments:` 相对路径，需用户给出根目录）**都复制进文件库**，按 SHA-256 存一份、写入后只读；链接文件另记原路径。找不到的标 `missing` 并列入报告，条目照常导入。快照目录里的资源文件也复制并记在 `source_meta.extraFiles`。MD5 与 Zotero 记录不符的照现在的内容导入，报告里提示。
- **笔记内嵌图片**：`data-attachment-key` 的值改写为附件 uid（附件的 `source_key` 保留原 key，可逆）。
- **群组库**：条目并入本库（Research Lab 没有群组），`source_library` 记原库 ID。feed 库不导入。
- **重复导入**：同一 Zotero 库已有记录就**拒绝**，提示先恢复导入前备份。增量导入与冲突预览留给 v0.1.x。
- **不迁移**（报告里计数）：全文索引、保存的搜索、同步设置（含阅读位置）、Zotero 缓存文件、storage 里的孤儿目录等，见 `zotero-mapping.md` 第 6 节。

## 3. BibTeX / RIS / CSL JSON

- **解析库**：BibTeX 用 `biblatex`（typst，MIT / Apache-2.0；会处理 `@string`、`#` 拼接、LaTeX 重音转义；一条坏条目会让整体解析失败，所以失败时按条目切开逐条解析、跳过坏条目）。RIS 格式简单，自己写。CSL JSON 用 `serde_json`。
- **词汇统一用 Zotero 的**类型名、字段名、作者角色，v0.2 生成 CSL 只认一套；映射不到的字段以「格式:原名」进 `item_fields`（`bibtex:howpublished`、`ris:M3`、`csl:genre`），整条原文进 `source_meta.raw`。
- **查重**：与库里未删除的文献比 DOI、规范化标题 + 年份，也与本文件前面的记录比；默认跳过可能重复的。正式去重提示（L9）在 Phase 3。
- 默认放进新集合「导入：文件名」（可取消）。
- 文件里的附件路径（BibTeX `file`、RIS `L1`）v0.1 只原样保存，不复制文件。

## 4. 界面

- 列表里，独立附件、独立笔记和文献一起列出（与 Zotero 一致），子附件 / 子笔记在详情里；回收站列出直接删除的条目、附件、笔记和集合。
- 附件可以「打开」：用系统默认程序打开文件库里的只读文件，网址链接在浏览器里打开。阅读器在 Phase 4。
- 恢复与清空回收站放到 Phase 3（管理）。

## 后果

- 0001 的 4 个取舍不变；新增表与列都可为空，旧库升级不改动已有数据（有升级测试）。
- 导入在一个事务里完成，最坏情况是整批撤销；导入前备份可以恢复（恢复界面在 Phase 6）。
- 文件库里的文件只读：用别的程序直接编辑会被拒绝，这是有意的（内容地址文件被改，哈希就对不上）。
