-- 0002 导入（v0.1 Phase 2）：补上 0001 装不下的来源数据。说明见 docs/schema.md「0002」与 decisions/0009。
-- 用过的 migration 永远不改，改结构写新编号。

-- Zotero 允许附件、笔记、标注也带标签（0001 只给普通条目建了关联表）
CREATE TABLE attachment_tags (
  attachment_id INTEGER NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  tag_id        INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL DEFAULT 'manual',  -- manual / automatic
  PRIMARY KEY (attachment_id, tag_id)
) STRICT;

CREATE TABLE note_tags (
  note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind    TEXT NOT NULL DEFAULT 'manual',
  PRIMARY KEY (note_id, tag_id)
) STRICT;

CREATE TABLE annotation_tags (
  annotation_id INTEGER NOT NULL REFERENCES annotations(id) ON DELETE CASCADE,
  tag_id        INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL DEFAULT 'manual',
  PRIMARY KEY (annotation_id, tag_id)
) STRICT;

-- 独立附件、独立笔记可以直接放进集合
CREATE TABLE collection_attachments (
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  attachment_id INTEGER NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, attachment_id)
) STRICT;

CREATE TABLE collection_notes (
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  note_id       INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, note_id)
) STRICT;

-- 笔记内嵌图片（Zotero embedded_image）的上级是笔记，不是条目
ALTER TABLE attachments ADD COLUMN note_id INTEGER REFERENCES notes(id) ON DELETE CASCADE;
-- 旧版 Zotero 附件自带的一段笔记（HTML）
ALTER TABLE attachments ADD COLUMN note TEXT;
-- 标注作者（群组库里有）；是否读自 PDF 文件本身（Zotero isExternal）
ALTER TABLE annotations ADD COLUMN author_name TEXT;
ALTER TABLE annotations ADD COLUMN is_external INTEGER NOT NULL DEFAULT 0;

-- 来源里没有对应列的其余原始数据（JSON），保证一个不丢：
-- Zotero：库 ID、version / synced / dateModified 等同步信息、附件的全部字段与 linkMode、关联、快照资源文件……
-- BibTeX / RIS / CSL JSON：整条原始记录
ALTER TABLE items ADD COLUMN source_meta TEXT;
ALTER TABLE collections ADD COLUMN source_meta TEXT;
ALTER TABLE attachments ADD COLUMN source_meta TEXT;
ALTER TABLE notes ADD COLUMN source_meta TEXT;
ALTER TABLE annotations ADD COLUMN source_meta TEXT;

-- 由哪一次导入带来（核对、将来撤销某次导入用）；手动新建的为空
ALTER TABLE items ADD COLUMN import_run_id INTEGER REFERENCES import_runs(id) ON DELETE SET NULL;
ALTER TABLE collections ADD COLUMN import_run_id INTEGER REFERENCES import_runs(id) ON DELETE SET NULL;
ALTER TABLE attachments ADD COLUMN import_run_id INTEGER REFERENCES import_runs(id) ON DELETE SET NULL;
ALTER TABLE notes ADD COLUMN import_run_id INTEGER REFERENCES import_runs(id) ON DELETE SET NULL;
ALTER TABLE annotations ADD COLUMN import_run_id INTEGER REFERENCES import_runs(id) ON DELETE SET NULL;

-- 外键子列的索引（详情、集合、标签查询与级联删除）
CREATE INDEX attachments_item ON attachments(item_id);
CREATE INDEX attachments_note ON attachments(note_id);
CREATE INDEX attachments_sha256 ON attachments(sha256);
CREATE INDEX notes_item ON notes(item_id);
CREATE INDEX annotations_attachment ON annotations(attachment_id);
CREATE INDEX collections_parent ON collections(parent_id);
CREATE INDEX collection_items_item ON collection_items(item_id);
CREATE INDEX item_tags_tag ON item_tags(tag_id);
