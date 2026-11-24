# Zotero 数据映射（Phase 0-A）

> 依据：本机 Zotero **9.0.3**、userdata schema **125** 的真实副本（只读）+ 合成夹具。
> 代码：`crates/zotero-import/`（`reader.rs` 读取，`model.rs` 中间模型，`audit.rs` 核对）。
> 原则：先**无损**读进中间模型，再由 v0.1 导入层（`src-tauri/src/import/zotero.rs`，规则见 `decisions/0009`）落到 `library.db`。中间模型保留 Zotero 的字段名与原始值，不在读取阶段做任何"清洗"。

## 1. 读取范围

| Zotero 表 | 中间模型 | 说明 |
| --- | --- | --- |
| `items` | `ItemBase`（所有条目共有） | key、类型、库、dateAdded / dateModified / clientDateModified、version、synced |
| `itemTypesCombined`（缺失时 `itemTypes`） | `item_type` | 按名称存，不存内部 ID；含自定义类型 |
| `itemData` + `itemDataValues` + `fieldsCombined` | `fields: {字段名: 原值}` | 字段名用 Zotero 原名（`publicationTitle`、`bookTitle`……）；非文本值转字符串并记提示 |
| `creators` + `itemCreators` + `creatorTypes` | `creators[]` | 保留 firstName、lastName、fieldMode、orderIndex、角色（author / editor / translator / contributor / seriesEditor …） |
| `tags` + `itemTags` | `tags[]` | 保留 type：0 手动，1 自动 |
| `collections` + `deletedCollections` + `collectionRelations` | `Collection` | 树形（parent_key），回收站状态 |
| `collectionItems` | `collection_keys[]` | 一个条目可在多个集合；`orderIndex` 未保留（Zotero 不使用） |
| `itemRelations` + `relationPredicates` | `relations[]` | `dc:relation`（相关条目，双向各存一条）、`dc:replaces`（合并重复项历史） |
| `itemAttachments` + `charsets` | `Attachment` | 见第 3 节 |
| `itemNotes` | `Note`；附件上的旧式笔记进 `Attachment.note` | 笔记是 HTML，原样保留 |
| `itemAnnotations` | `Annotation` | 位置 JSON 原样保留，见第 4 节 |
| `deletedItems` | `deleted_at` + `in_trash` | 见第 5 节 |
| `libraries` + `groups` | `LibraryInfo` | 读 user / group 库，**跳过 feed 库** |
| `version`、`settings` | `SourceInfo` | schema 各版本号、Zotero 版本号 |

## 2. 字段值的格式

- **date**：Zotero 存为「`YYYY-MM-DD 原文`」，未知月日用 `00`。例：`2009-03-00 3/2009`、`2013-05-01 2013-05-01`。v0.1 应拆成可排序日期 + 显示原文，**两者都存**。真实库 85 个 date 值全部符合此格式。
- **accessDate**、`dateAdded` 等时间戳：UTC，`YYYY-MM-DD HH:MM:SS`。
- **extra**：自由文本，常含 CSL 变量（`original-date: 2012`、`PMID: …`），v0.2 引用引擎要解析，v0.1 原样保存。
- **语言**：真实库 64 条有 `language=en`；8 条中文标题的条目**都没有填 language**。不能靠这个字段判断中文，搜索与引用都要按内容判断。
- **类型专属字段**：Zotero 在 `itemData` 存的是类型专属字段 ID（书的章节存 `bookTitle`，不是基础字段 `publicationTitle`）。基础字段映射在 `baseFieldMappingsCombined`，v0.2 生成 CSL 时再用。

## 3. 附件

| linkMode | 名称 | path 列 | 文件位置 |
| --- | --- | --- | --- |
| 0 | imported_file | `storage:文件名` | `storage/<附件 key>/文件名` |
| 1 | imported_url | `storage:文件名` | 同上（从网页保存的 PDF / HTML 快照） |
| 2 | linked_file | 绝对路径，或 `attachments:相对路径` | Zotero 之外；相对路径的根目录在 Zotero 首选项里，**不在数据库**，需用 `--base-dir` 提供 |
| 3 | linked_url | NULL | 无文件，网址在 `fields.url` |
| 4 | embedded_image | `storage:image.png` | 笔记里的内嵌图片，父条目是**笔记** |

- **文件校验**：Zotero 的 `storageHash` 是文件 MD5（同步用）。读取器对每个存在的文件同时计算 SHA-256（v0.1 附件库的内容地址）与 MD5（与 Zotero 对比）。
- **附件目录里的其他文件**：Zotero 缓存 `.zotero-ft-cache`（全文索引文本）、`.zotero-ft-info`、`.zotero-reader-state`（阅读位置）不算附件内容；其余文件（旧式网页快照的图片等）进 `extra_files`。真实库 38 个快照均为单文件。
- **中文文件名**：按原样处理；夹具覆盖了中文文件名。
- **笔记内嵌图片**：笔记 HTML 用 `data-attachment-key="KEY"` 引用图片附件，v0.1 导入时要改写为新 ID。

## 4. 标注（Zotero 阅读器的高亮等）

`type`：1 highlight，2 note，3 image，4 ink，5 underline，6 text。`position` 是 JSON：`{"pageIndex":0,"rects":[[x1,y1,x2,y2]]}`（PDF 用户空间坐标，单位 pt），ink 为 `paths`。`sortIndex` 形如 `00000|000123|00079`（页|字符偏移|纵向位置）。

**建议**：v0.1 自己的高亮锚点（L8）直接采用同一格式（pageIndex + rects，PDF 坐标），这样 Zotero 标注无需转换即可迁入，也与 pdf.js 的坐标体系对应。

真实库**没有任何 Zotero 标注**（`itemAnnotations` 0 行），所以 v0.1 迁移这部分只需要夹具验证。image 类型标注的图片缓存在 Zotero 的 `cache/` 目录，不在 storage，迁移时只能保留区域，需要时从 PDF 重新截取。

## 5. 回收站

- 条目自己在 `deletedItems` → `deleted_at` 有值。
- **父条目在回收站时，子附件 / 子笔记不会写进 `deletedItems`**，但 Zotero 界面里它们随父条目一起在回收站。读取器沿父链判断，标为 `in_trash = true, deleted_at = null`。真实库有 4 个这样的子条目。
- 集合同理：上级集合在回收站，子集合也在。
- 中间模型**保留回收站内容**（不丢弃）。v0.1 原样导入回收站：只在直接删除的记录上写 `deleted_at`，子条目随父条目（`decisions/0009`）。

## 6. 不迁移（报告中计数）

| 内容 | 原因 |
| --- | --- |
| `fulltextItems` / `fulltextWords` | Zotero 的全文索引，可从 PDF 重建 |
| `.zotero-ft-cache` | 同上（缓存的全文文本） |
| `.zotero-reader-state`、`syncedSettings.lastPageIndex_*` | 阅读位置；v0.1.x 可选迁移 |
| `savedSearches` | 真实库为 0 |
| `settings` 中的账户信息 | Zotero 同步账户，与本项目无关 |
| feed 库 | RSS 订阅，非文献 |
| 自定义类型 / 字段 | 真实库为 0；读取器已兼容 Combined 表 |

## 7. 对 v0.1 数据模型的直接影响

1. **身份**：每条记录保存 `(来源="zotero", library_id, key)`，供重复导入时识别（增量导入冲突预览）。
2. **字段**：键值表保存全部原始字段（保证不丢），另把 title / date / DOI / ISBN / 期刊等提到列上做排序和搜索。
3. **作者**：保留 field_mode；中文单栏姓名（真实库 15 位）不要强行拆姓名。单条最多 102 位作者。
4. **附件**：按 SHA-256 存一份（真实库有 1 组内容完全相同的文件，一份在回收站），记录原始文件名、来源 key、Zotero MD5。
5. **搜索**：中文标题约占 10%。SQLite FTS5 默认分词器不切中文，Phase 3 需要 trigram 分词器或双索引，届时实测。
6. **文件缺失**：需要「缺失文件」状态，不能因为文件不在就拒绝导入整条记录。
