-- 0005 联网补全（v0.1 Phase 5）：每次按 DOI / ISBN 查询的结果与改动。说明见 docs/schema.md「0005」与 decisions/0012。
-- 用过的 migration 永远不改，改结构写新编号。

CREATE TABLE metadata_lookups (
  id         INTEGER PRIMARY KEY,
  item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  identifier TEXT NOT NULL,                     -- doi:10.1038/nature14539 / isbn:9780140449136
  trigger    TEXT NOT NULL,                     -- import / edit / manual / add
  status     TEXT NOT NULL,                     -- found / not_found / failed
  source     TEXT,                              -- crossref / datacite / … / openlibrary
  message    TEXT,                              -- 没找到、失败的原因（中文）
  record     TEXT,                              -- JSON：查到的记录（已换成 Zotero 字段名）
  changes    TEXT NOT NULL DEFAULT '[]',        -- JSON：[{field, current, found, kind: fill|conflict, state: applied|pending|accepted|ignored|undone}]
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX metadata_lookups_item ON metadata_lookups(item_id, id);
