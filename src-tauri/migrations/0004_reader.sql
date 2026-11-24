-- 0004 阅读（v0.1 Phase 4）：阅读位置；标注按页内顺序取用的索引。说明见 docs/schema.md「0004」与 decisions/0011。
-- 用过的 migration 永远不改，改结构写新编号。

-- 每个附件的阅读位置。state 是 JSON，键与 Zotero 的 .zotero-reader-state 相同：
-- {"pageIndex":3,"scale":"page-width","top":612.5,"left":-8,"rotation":0}（top / left 为 PDF 坐标，pt）
CREATE TABLE reader_states (
  attachment_id INTEGER PRIMARY KEY REFERENCES attachments(id) ON DELETE CASCADE,
  state         TEXT NOT NULL,
  updated_at    TEXT NOT NULL
) STRICT;

CREATE INDEX annotations_by_order ON annotations(attachment_id, sort_key);
