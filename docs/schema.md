# 数据库结构

> 状态：**已定**（用户 2026-11-08 同意下面 4 个取舍）。以 `src-tauri/migrations/` 为准，用过永不改；改结构写新编号的 migration。本文详写 v0.1 的 0001–0005，之后的见文末「0006 以后」。
> 依据：`decisions/0002`（Zotero 迁移）、`0003`（数据目录）、`0006`（Word 引文需要稳定 ID）、`docs/zotero-mapping.md` 第 7 节、Phase 0-A 真实库统计。
> 约定沿用 Pomelo：`INTEGER PRIMARY KEY`、`STRICT` 表、时间为 UTC ISO 字符串（`strftime('%Y-%m-%dT%H:%M:%fZ','now')`）、枚举值在 Rust 校验、版本号在 `PRAGMA user_version`、WAL + `foreign_keys = ON`。

## 4 个取舍（已同意）

| # | 取舍 | 定下的做法 | 后果 |
| --- | --- | --- | --- |
| S1 | 记录 ID | 整数主键 + 文本 `uid`（26 位 ULID，按时间可排序） | 表之间用整数关联；Word 引文、整库导出、将来的同步只认 `uid` |
| S2 | 附件、笔记放哪 | 独立的表（Zotero 把它们也当「条目」） | 约束清楚、查询简单；保留 Zotero key |
| S3 | 字段怎么存 | 常用字段提成列 + 全部原始字段存 `item_fields` | 排序、搜索、去重走列；Zotero 字段一个不丢 |
| S4 | 回收站 | 只在直接删除的记录上写 `deleted_at`，子附件 / 笔记随父条目进回收站 | 与 Zotero 语义一致；恢复父条目时子条目自动回来 |

**字段显示原则**（用户 2026-11-08）：存全不等于显示全。界面只显示提成列的常用字段（标题、作者、日期、期刊 / 书名、DOI、ISBN、语言、摘要、标签、集合）；`item_fields` 里的其余 Zotero 字段默认不显示，v0.2 生成 CSL 时按需映射。

## 表

```sql
-- 文献条目（不含附件、笔记）
CREATE TABLE items (
  id              INTEGER PRIMARY KEY,
  uid             TEXT NOT NULL UNIQUE,          -- 稳定外部 ID（Word 引文里的 CSL id）
  item_type       TEXT NOT NULL,                 -- journalArticle / book / …（Zotero 类型名）
  title           TEXT,
  title_norm      TEXT,                          -- 去标点、小写、全半角统一，去重用
  date_raw        TEXT,                          -- 原文日期（"3/2009"）
  date_sort       TEXT,                          -- 'YYYY-MM-DD'，未知月日为 00
  year            INTEGER,
  container_title TEXT,                          -- 期刊名 / 书名
  doi             TEXT,
  isbn            TEXT,
  language        TEXT,                          -- 原始 language 字段
  detected_lang   TEXT,                          -- 按内容判定（'zh' / 'en' / …），原字段为空时用
  abstract        TEXT,
  extra           TEXT,
  source          TEXT,                          -- 'zotero' / 'bibtex' / 'ris' / 'csl-json' / 'manual'
  source_library  INTEGER,                       -- Zotero libraryID
  source_key      TEXT,                          -- Zotero key（8 位）
  created_at      TEXT NOT NULL,                 -- 迁移时保留 Zotero dateAdded
  updated_at      TEXT NOT NULL,
  deleted_at      TEXT,                          -- 回收站
  UNIQUE (source, source_library, source_key)
) STRICT;
CREATE INDEX items_doi ON items(doi) WHERE doi IS NOT NULL;
CREATE INDEX items_isbn ON items(isbn) WHERE isbn IS NOT NULL;
CREATE INDEX items_title_norm ON items(title_norm, year);

-- 全部原始字段（保证不丢；含类型专属字段，如 bookTitle）
CREATE TABLE item_fields (
  item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  field   TEXT NOT NULL,
  value   TEXT NOT NULL,
  PRIMARY KEY (item_id, field)
) STRICT;

-- 作者、编者等；中文单栏姓名存 literal，不拆姓名
CREATE TABLE creators (
  id       INTEGER PRIMARY KEY,
  item_id  INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  role     TEXT NOT NULL,                        -- author / editor / translator / …
  family   TEXT,
  given    TEXT,
  literal  TEXT,                                 -- Zotero fieldMode=1
  position INTEGER NOT NULL,
  UNIQUE (item_id, position)
) STRICT;

-- 条目之间的关联（Zotero relations：dc:relation、owl:sameAs …）
CREATE TABLE item_relations (
  item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  predicate TEXT NOT NULL,
  object    TEXT NOT NULL,
  PRIMARY KEY (item_id, predicate, object)
) STRICT;

-- 集合（文件夹树）
CREATE TABLE collections (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  parent_id  INTEGER REFERENCES collections(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  source_key TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
) STRICT;

CREATE TABLE collection_items (
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  item_id       INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, item_id)
) STRICT;

CREATE TABLE tags (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE item_tags (
  item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind    TEXT NOT NULL DEFAULT 'manual',        -- manual / automatic（Zotero tag type 0 / 1）
  PRIMARY KEY (item_id, tag_id)
) STRICT;

-- 文件本体：按 SHA-256 存一份（attachments/<前 2 位>/<sha256>.<ext>）
CREATE TABLE files (
  sha256     TEXT PRIMARY KEY,
  size       INTEGER NOT NULL,
  ext        TEXT NOT NULL,
  md5        TEXT,                               -- 与 Zotero storageHash 对比用
  created_at TEXT NOT NULL
) STRICT;

-- 附件记录；文件缺失时 sha256 为空、status = 'missing'，条目照常导入
CREATE TABLE attachments (
  id            INTEGER PRIMARY KEY,
  uid           TEXT NOT NULL UNIQUE,
  item_id       INTEGER REFERENCES items(id) ON DELETE CASCADE,  -- 为空 = 独立附件
  kind          TEXT NOT NULL,                   -- stored / linked_file / linked_url / embedded_image
  sha256        TEXT REFERENCES files(sha256),
  status        TEXT NOT NULL DEFAULT 'ok',      -- ok / missing
  title         TEXT,
  original_name TEXT,
  original_path TEXT,                            -- linked_file 的原始路径
  url           TEXT,
  content_type  TEXT,
  source_key    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deleted_at    TEXT
) STRICT;

-- 笔记（HTML）；内嵌图片的 Zotero key 在导入时改写为附件 uid
CREATE TABLE notes (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  item_id    INTEGER REFERENCES items(id) ON DELETE CASCADE,     -- 为空 = 独立笔记
  title      TEXT,
  html       TEXT NOT NULL,
  source_key TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
) STRICT;

-- PDF 高亮 / 摘录（L8）；位置与 Zotero 同格式，迁移无需转换
CREATE TABLE annotations (
  id            INTEGER PRIMARY KEY,
  uid           TEXT NOT NULL UNIQUE,
  attachment_id INTEGER NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL,                   -- highlight / underline / note / image / ink / text
  page_index    INTEGER NOT NULL,
  page_label    TEXT,
  position      TEXT NOT NULL,                   -- JSON：{"pageIndex":0,"rects":[[x1,y1,x2,y2]]}（PDF 坐标，pt）
  sort_key      TEXT NOT NULL,                   -- 页|字符偏移|纵向位置
  text          TEXT,                            -- 被高亮的原文
  comment       TEXT,
  color         TEXT,
  source_key    TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deleted_at    TEXT
) STRICT;

-- 每次导入的记录与报告
CREATE TABLE import_runs (
  id          INTEGER PRIMARY KEY,
  source      TEXT NOT NULL,                     -- zotero / bibtex / ris / csl-json
  source_path TEXT,
  backup_file TEXT,                              -- 导入前自动备份的文件名
  started_at  TEXT NOT NULL,
  finished_at TEXT,
  stats       TEXT,                              -- JSON：各类数量
  report      TEXT                               -- JSON：缺失文件、跳过项、警告
) STRICT;

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
```

## 0002：导入补全（Phase 2，`decisions/0009`）

0001 装不下 Zotero 的几类数据，0002 只**追加**表与可空列，不改 S1–S4：

```sql
-- 附件、笔记、标注的标签（kind：manual / automatic）
CREATE TABLE attachment_tags (attachment_id → attachments, tag_id → tags, kind, PRIMARY KEY (attachment_id, tag_id)) STRICT;
CREATE TABLE note_tags       (note_id → notes,             tag_id → tags, kind, PRIMARY KEY (note_id, tag_id)) STRICT;
CREATE TABLE annotation_tags (annotation_id → annotations, tag_id → tags, kind, PRIMARY KEY (annotation_id, tag_id)) STRICT;
-- 独立附件、独立笔记直接进集合
CREATE TABLE collection_attachments (collection_id → collections, attachment_id → attachments, PRIMARY KEY (…)) STRICT;
CREATE TABLE collection_notes       (collection_id → collections, note_id → notes, PRIMARY KEY (…)) STRICT;

ALTER TABLE attachments ADD note_id   → notes      -- 笔记内嵌图片的上级是笔记
ALTER TABLE attachments ADD note      TEXT         -- 旧版 Zotero 附件自带的笔记（HTML）
ALTER TABLE annotations ADD author_name TEXT, is_external INTEGER NOT NULL DEFAULT 0
-- items / collections / attachments / notes / annotations 各加：
ALTER TABLE … ADD source_meta   TEXT               -- JSON：来源里没有对应列的其余原始数据
ALTER TABLE … ADD import_run_id → import_runs      -- 由哪次导入带来（ON DELETE SET NULL）
-- 外键子列索引：attachments(item_id / note_id / sha256)、notes(item_id)、annotations(attachment_id)、
-- collections(parent_id)、collection_items(item_id)、item_tags(tag_id)
```

`source_meta` 的内容：

| 来源 | 内容 |
| --- | --- |
| Zotero，所有类型 | `libraryID`、`version`、`synced`、`clientDateModified`（增量导入用） |
| Zotero，附件 / 笔记 / 标注 | 另含全部 `fields`、`relations`、`creators`（有就存）；上级找不到时的 `orphanParentKey` |
| Zotero，附件 | `linkMode`、`charset`、`path`、`syncState`、`storageModTime`、`storageHash`、`lastProcessedModificationTime`、`lastRead`、快照资源 `extraFiles: [[相对路径, sha256]]` |
| Zotero，标注 | `annotationType`（原始数字），`sortIndexMissing` / `positionMissing` 标记 |
| Zotero，集合 | `libraryID`、`version`、`synced`、`relations` |
| BibTeX / RIS / CSL JSON | `index`（文件里第几条）、`raw`（整条原文）、`warnings` |

**附件 `kind`**：`stored`（Zotero 的 imported_file / imported_url，原 linkMode 在 meta）、`linked_file`（已复制进文件库，`original_path` 记原位置）、`linked_url`（无文件）、`embedded_image`（`note_id` 指向笔记）。`status`：`ok` / `missing`。

**非 Zotero 来源的字段**：类型名、字段名、作者角色都映射成 Zotero 的；映射不到的以「格式:原名」进 `item_fields`（`bibtex:howpublished`、`ris:M3`、`csl:genre`）；日期统一写成 Zotero 两段式「排序值 原文」。

## 0003：管理（Phase 3，`decisions/0010`）

```sql
-- 用户确认「不是重复」的两条文献（item_a < item_b），去重提示不再把它们连在一起
CREATE TABLE duplicate_dismissals (
  item_a     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  item_b     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  PRIMARY KEY (item_a, item_b),
  CHECK (item_a < item_b)
) STRICT;
```

## 0004：阅读（Phase 4，`decisions/0011`）

```sql
-- 每个附件的阅读位置；state 的键与 Zotero 的 .zotero-reader-state 相同
-- {"pageIndex":3,"scale":"page-width","top":612.5,"left":-8,"rotation":0}（top / left 为 PDF 坐标，pt）
CREATE TABLE reader_states (
  attachment_id INTEGER PRIMARY KEY REFERENCES attachments(id) ON DELETE CASCADE,
  state         TEXT NOT NULL,
  updated_at    TEXT NOT NULL
) STRICT;
CREATE INDEX annotations_by_order ON annotations(attachment_id, sort_key);
```

## 0005：联网补全（Phase 5，`decisions/0012`）

```sql
-- 每次按 DOI / ISBN 联网查询的结果与改动
CREATE TABLE metadata_lookups (
  id         INTEGER PRIMARY KEY,
  item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  identifier TEXT NOT NULL,                -- doi:10.1038/nature14539 / isbn:9780140449136
  trigger    TEXT NOT NULL,                -- import / edit / manual / add
  status     TEXT NOT NULL,                -- found / not_found / failed
  source     TEXT,                         -- crossref / datacite / … / openlibrary
  message    TEXT,                         -- 没找到、失败的原因
  record     TEXT,                         -- JSON：查到的记录（Zotero 字段名）
  changes    TEXT NOT NULL DEFAULT '[]',   -- JSON：[{field, current, found, kind, state}]
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;
CREATE INDEX metadata_lookups_item ON metadata_lookups(item_id, id);
```

`changes` 里每处改动：`kind` = fill（原来空着，自动填）/ conflict（与现有不同）；`state` = applied / pending（待确认）/ accepted / ignored / undone / kept（要撤销时已被改过）。「待确认的补全」= 最近一次查询里还有 pending 的文献。设置存在 `settings` 表：`metadata.enabled`（1 / 0，没有 = 还没问过）、`metadata.auto`、`metadata.email`。按 DOI / ISBN 新建的文献 `items.source = 'lookup'`。

`annotations` 表 0001 就按 Zotero 格式建好了，Phase 4 不改结构：App 里新建的标注 `source_key`、`import_run_id`、`author_name` 为空，`is_external = 0`；删除写 `deleted_at`（可撤销）。

时间戳从 Phase 3 起精确到毫秒（`YYYY-MM-DDTHH:MM:SS.mmmZ`，即本文开头约定的格式）；Zotero 导入的时间保留 Zotero 原值（`.000Z`）。

## 0006 以后

| migration | 版本 | 内容 | 说明 |
| --- | --- | --- | --- |
| `0006_research.sql` | v0.3 | `research_questions`、`research_runs`、`research_searches`、`research_sources`、`research_facts`、`research_calcs`、`research_conclusions`、`research_conclusion_facts`、`research_steps` | 事实调查（`decisions/0020`）；快照文件在 `数据目录/research/sources/` |
| `0007_research_fetch.sql` | v0.3 | `research_sources.fetched_via` | 下载失败时改用 Claude 网页抓取（`0023`） |
| `0008_concept.sql` | v0.4 | `concept_texts`（一份解析好的文本：附件 SHA-256、解析器版本、关联的原著）、`concept_units`（篇章节、导读、注释、前后附）、`concept_paragraphs`（角色、引用名、页码与段内分页点、注释引用）、`concept_alignments` + `concept_aligned`（中英对齐）、`concept_runs`、`concept_steps`（含模型原始输出）、`concept_cards`、`concept_sentences`、`concept_anchors`（一句一行：段落号、核对状态、位置、页码、中译核对）、`concept_relations`、`concept_sources`、`concept_rejected` | 概念研究（`0024`、`0025`）；保存一次研究 = 一个事务里整份换掉子表；评论来源的快照在 `数据目录/concept/sources/` |

## 不在主库里的

- **搜索索引**放 `search.db`（派生数据，可删了重建，不写 migration，以 `search` 名 ATTACH 到主连接）：一张 FTS5 表 `docs(title, creators, tags, abstract, notes, other)`；中文用「重叠二元组 + 末字单字」改写后交给 unicode61（实测选定，`decisions/0010`）。
- **Word 引文数据**存在 Word 文档自己的数据部件里（`decisions/0006`），主库只提供 `items.uid` 与 CSL-JSON 导出；v0.2 再加「文档 ↔ 文献」的引用记录表。
- **备份文件**在 `backups/`，不进表。
- **整库导出**：`library.json` 是全部表的逐行转储（列与本文一致，按导出时的版本），格式见 `decisions/0013`。每个版本冻结一份合成样本库在 `src-tauri/fixtures/library-vN.json`，升级测试用。

## 与 Zotero 的对应（摘要，详见 `docs/zotero-mapping.md`）

| Zotero | 本库 |
| --- | --- |
| items（非附件、非笔记、非标注） | `items` + `item_fields` + `creators` |
| itemAttachments / itemNotes / itemAnnotations | `attachments` / `notes` / `annotations` |
| collections / collectionItems | `collections` / `collection_items` |
| tags / itemTags | `tags` / `item_tags` |
| itemRelations | `item_relations` |
| collectionRelations | `collections.source_meta.relations`（0002 起保存） |
| deletedItems / deletedCollections | 各表 `deleted_at` |
| storage/<key>/文件 | `files`（SHA-256）+ `attachments.sha256` |
