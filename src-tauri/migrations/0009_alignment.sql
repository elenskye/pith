-- v0.5 全书中英对齐：段数不同的章做单调对齐，一段可以对几段、几段可以对一段。
--
-- 0008 的 concept_aligned 主键是 (alignment_id, original_seq)，一段原文只能对一段译文，存不了一对多。
-- 用过的 migration 不改，所以新建一张表；concept_aligned 留着，从这一版起不再读、不再写。
-- 对齐是派生数据：读到 algorithm 比 App 的算法版本旧的那一行，就用两份文本重新算一遍、存回去，不丢东西；
-- 旧表里属于那份对齐的行这时跟着旧的那一行一起删掉（外键级联），本文件不动任何数据。

CREATE TABLE concept_aligned_pairs (
  alignment_id    INTEGER NOT NULL REFERENCES concept_alignments(id) ON DELETE CASCADE,
  original_seq    INTEGER NOT NULL,               -- 原著的正文段（concept_paragraphs.seq）
  translation_seq INTEGER NOT NULL,               -- 译本的正文段；一对多、多对一写成多行
  PRIMARY KEY (alignment_id, original_seq, translation_seq)
) STRICT;

CREATE INDEX idx_concept_aligned_pairs_translation ON concept_aligned_pairs(alignment_id, translation_seq);

-- 对齐算法的版本（concept_engine::align::VERSION）：1 = v0.4 段数相同才对齐；2 = v0.5 单调对齐。旧行默认 1
ALTER TABLE concept_alignments ADD COLUMN algorithm INTEGER NOT NULL DEFAULT 1;
-- 算的时候用了哪个向量模型的语义相似度（没用是 NULL，只看长度和锚点）
ALTER TABLE concept_alignments ADD COLUMN similarity TEXT;
