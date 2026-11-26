-- 1.1.0 第 3 期：知识库检索（decisions/0038）。
-- 材料能进多个库（kb_members）；材料切成段落块（passages），检索、读原文、引用都对着段落块。
-- 只建表、建索引（transfer/tests.rs 的 migrations_only_add）。已有的材料补进 kb_members、解析好的书迁成全文材料，
-- 由 App 启动时做一次（kb::on_startup），不写在这里。

-- 一份材料在哪几个知识库里。kb_materials.kb_id 从这一版起是「家」：第一次种进的那个库，一定也在这张表里；
-- 从家里拿掉而别的库还有它时，家换到别的库（代码里做），所以删掉一个库只删只在这个库里的材料
CREATE TABLE kb_members (
  kb_id       INTEGER NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
  material_id INTEGER NOT NULL REFERENCES kb_materials(id) ON DELETE CASCADE,
  added_at    TEXT NOT NULL,
  PRIMARY KEY (kb_id, material_id)
) STRICT;

CREATE INDEX kb_members_material ON kb_members(material_id);

-- 一份材料的一块：书是一段（段落号、页码、所在的章照解析好的文本），别的材料按空行切、太短的并、太长的拆。
-- 正文是材料自己的一份（书的文本删了、重新解析之前，知识库里照样读得到）；检索的索引和向量在 search.db（派生）
CREATE TABLE passages (
  id          INTEGER PRIMARY KEY,
  material_id INTEGER NOT NULL REFERENCES kb_materials(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,                   -- 材料里第几块；书 = concept_paragraphs.seq（对齐靠它）
  role        TEXT NOT NULL DEFAULT 'body',       -- body / heading / sidenote / note / guide / matter
  key         TEXT NOT NULL DEFAULT '',           -- 段落号（I.iii.1）；没有是空
  unit_key    TEXT NOT NULL DEFAULT '',           -- 所在的章（I.iii）；没有是空
  unit_label  TEXT NOT NULL DEFAULT '',
  page        TEXT,                               -- 页码：110、110–111
  text        TEXT NOT NULL,
  UNIQUE (material_id, seq)
) STRICT;
