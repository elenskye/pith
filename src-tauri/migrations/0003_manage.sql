-- 0003 管理（v0.1 Phase 3）：「不是重复」的标记。说明见 docs/schema.md「0003」与 decisions/0010。
-- 用过的 migration 永远不改，改结构写新编号。

-- 用户确认「不是重复」的两条文献（item_a < item_b），去重提示不再把它们放在一起
CREATE TABLE duplicate_dismissals (
  item_a     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  item_b     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  PRIMARY KEY (item_a, item_b),
  CHECK (item_a < item_b)
) STRICT;
