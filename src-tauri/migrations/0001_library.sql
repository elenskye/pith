-- 0001 文献库主表（v0.1）。结构说明见 docs/schema.md。用过的 migration 永远不改，改结构写新编号。
-- 约定：INTEGER 主键 + 文本 uid（ULID，对外稳定 ID）；STRICT 表；时间为 UTC ISO 字符串；回收站 = deleted_at。

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
