-- 0008 概念研究（v0.4 Concept Research）：文本（书 → 单元 → 段落，含页码）、中英对齐、技能运行、卡片、句、锚点、关系、评论来源。
-- 说明见 decisions/0024 与 HANDOFF-cloud.md。只建表、建索引；用过的 migration 永远不改，改结构写新编号。
-- 来源正文的快照不在库里：数据目录 concept/sources/<sha256 前两位>/<sha256>.<扩展名> 和 .txt（和事实调查的分开放）。

-- 一份解析好的文本：文献库里一条文献的 EPUB / PDF 附件「解析成文本」得到
CREATE TABLE concept_texts (
  id            INTEGER PRIMARY KEY,
  uid           TEXT NOT NULL UNIQUE,
  item_id       INTEGER REFERENCES items(id) ON DELETE SET NULL,        -- 书是文献库里的一条文献
  attachment_id INTEGER REFERENCES attachments(id) ON DELETE SET NULL,  -- 从哪个附件解析的
  source_sha256 TEXT NOT NULL,                    -- 附件文件的 SHA-256
  format        TEXT NOT NULL,                    -- epub / pdf
  parser        TEXT NOT NULL,                    -- 解析器版本（concept-text-1）；只在用户要求时重新解析
  title         TEXT NOT NULL DEFAULT '',
  language      TEXT NOT NULL DEFAULT '',         -- en / zh / …
  numbering     TEXT NOT NULL,                    -- glasgow（I.iii.5）/ chapter（3.5）/ plain（5）
  page_source   TEXT NOT NULL DEFAULT 'none',     -- page-list / page-map / ncx-pagelist / anchors / pdf / none
  meta          TEXT NOT NULL DEFAULT '{}',       -- JSON：作者、出版者、日期、标识、解析提示、PDF 用的是 Mathpix 还是本机取文字
  original_id   INTEGER REFERENCES concept_texts(id) ON DELETE SET NULL, -- 译本：对应的原著（两份文本互相关联）
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
) STRICT;

CREATE INDEX concept_texts_item ON concept_texts(item_id);
CREATE INDEX concept_texts_original ON concept_texts(original_id);

-- 单元：篇、章、节、导读、注释、前后附（按目录）
CREATE TABLE concept_units (
  id       INTEGER PRIMARY KEY,
  text_id  INTEGER NOT NULL REFERENCES concept_texts(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,                      -- 单元下标（段落的 unit 指它）
  kind     TEXT NOT NULL,                         -- book / chapter / section / part / container / guide / notes / matter
  label    TEXT NOT NULL,                         -- 目录里的文字
  key      TEXT NOT NULL,                         -- 引用名：I.iii / 3 / Intro / 前三章导读
  depth    INTEGER NOT NULL,
  parent   INTEGER,                               -- 上一层单元的 position
  book     INTEGER,                               -- 第几篇
  chapter  INTEGER,                               -- 第几章
  UNIQUE (text_id, position)
) STRICT;

-- 段落：正文、标题、旁注、注释、导读、前后附；页码是段落开头的页 + 段内分页点
CREATE TABLE concept_paragraphs (
  id        INTEGER PRIMARY KEY,
  text_id   INTEGER NOT NULL REFERENCES concept_texts(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,                     -- 在全书里的顺序，从 0 起
  unit      INTEGER NOT NULL,                     -- 编段落号的单元（concept_units.position）
  role      TEXT NOT NULL,                        -- body / heading / sidenote / note / guide / matter
  number    INTEGER,                              -- 正文段的序号（单元内从 1 起）
  key       TEXT NOT NULL,                        -- I.i.3 / I.i.3 旁注 / I.i 注 1 / 前三章导读 ¶2，全书唯一
  attached  INTEGER,                              -- 旁注、注释挂在哪个正文段（seq）
  body      TEXT NOT NULL,                        -- 段落文字（原样）
  page      TEXT,                                 -- 段落开头在哪一页；没有页码的版本是空
  breaks    TEXT NOT NULL DEFAULT '[]',           -- JSON：段内分页点 [{at, page}]（at 按字符）
  marker    TEXT,                                 -- 注释的标记（「1」）
  noterefs  TEXT NOT NULL DEFAULT '[]',           -- JSON：正文里注释引用的位置和标记 [[at, "1"]]
  UNIQUE (text_id, seq),
  UNIQUE (text_id, key)
) STRICT;

CREATE INDEX concept_paragraphs_unit ON concept_paragraphs(text_id, unit, seq);

-- 中英对齐：同一章正文段数相同就按顺序对齐；units 记每章的段数和对没对上
CREATE TABLE concept_alignments (
  id             INTEGER PRIMARY KEY,
  original_id    INTEGER NOT NULL REFERENCES concept_texts(id) ON DELETE CASCADE,
  translation_id INTEGER NOT NULL REFERENCES concept_texts(id) ON DELETE CASCADE,
  units          TEXT NOT NULL DEFAULT '[]',      -- JSON：[{key, label, otherLabel, unit, otherUnit, count, otherCount, aligned}]
  created_at     TEXT NOT NULL,
  UNIQUE (original_id, translation_id)
) STRICT;

CREATE TABLE concept_aligned (
  alignment_id    INTEGER NOT NULL REFERENCES concept_alignments(id) ON DELETE CASCADE,
  original_seq    INTEGER NOT NULL,
  translation_seq INTEGER NOT NULL,
  PRIMARY KEY (alignment_id, original_seq)
) STRICT;

-- 一次技能运行（一次概念研究）
CREATE TABLE concept_runs (
  id            INTEGER PRIMARY KEY,
  uid           TEXT NOT NULL UNIQUE,
  text_id       INTEGER REFERENCES concept_texts(id) ON DELETE CASCADE,   -- 原著文本
  skill         TEXT NOT NULL,                    -- 技能名（chapter / passage / …）
  skill_title   TEXT NOT NULL,
  skill_version INTEGER NOT NULL,
  scope         TEXT NOT NULL,                    -- JSON：入口、段落、章、问题、检索词、选的卡片、要求、跳过 / 加上的步骤
  status        TEXT NOT NULL,                    -- ready / running / cancelled / failed / done
  next_step     INTEGER NOT NULL DEFAULT 0,       -- 接下来做技能的第几步
  error         TEXT,
  model         TEXT NOT NULL DEFAULT '{}',       -- JSON：接口和模型
  effort        TEXT NOT NULL DEFAULT '',
  mock          INTEGER NOT NULL DEFAULT 0,
  targets       TEXT NOT NULL DEFAULT '[]',       -- JSON：入口选的卡片（复制一份）
  previous_uid  TEXT,                             -- 同一技能对同一范围重跑：上一次（旧结果保留）
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  finished_at   TEXT
) STRICT;

CREATE INDEX concept_runs_text ON concept_runs(text_id, created_at);

-- 步骤与费用（费用同时记在账本 agent/usage.jsonl 里）
CREATE TABLE concept_steps (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES concept_runs(id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  step          TEXT NOT NULL,                    -- 技能里的步骤 id
  label         TEXT NOT NULL,
  at            TEXT NOT NULL,
  provider      TEXT NOT NULL DEFAULT '',
  model         TEXT NOT NULL DEFAULT '',
  input_tokens  INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  searches      INTEGER NOT NULL DEFAULT 0,
  cost_usd      REAL NOT NULL DEFAULT 0,
  mock          INTEGER NOT NULL DEFAULT 0,
  ok            INTEGER NOT NULL DEFAULT 1,
  note          TEXT,
  raw           TEXT                              -- 模型的原始输出
) STRICT;

CREATE INDEX concept_steps_run ON concept_steps(run_id, position);

CREATE TABLE concept_cards (
  id       INTEGER PRIMARY KEY,
  run_id   INTEGER NOT NULL REFERENCES concept_runs(id) ON DELETE CASCADE,
  key      TEXT NOT NULL,                         -- K1、K2……一次研究里唯一
  position INTEGER NOT NULL,
  kind     TEXT NOT NULL,                         -- concept / argument / comment / counterfactual / note
  title    TEXT NOT NULL,
  step     TEXT NOT NULL,
  about    TEXT NOT NULL DEFAULT '[]',            -- JSON：评论、推演针对的卡片编号
  UNIQUE (run_id, key)
) STRICT;

-- 句：每句一个类别
CREATE TABLE concept_sentences (
  id         INTEGER PRIMARY KEY,
  card_id    INTEGER NOT NULL REFERENCES concept_cards(id) ON DELETE CASCADE,
  position   INTEGER NOT NULL,
  category   TEXT NOT NULL,                       -- original / others / explanation / speculation
  body       TEXT NOT NULL,
  quote      TEXT,                                -- 逐字引文
  zh         TEXT,                                -- 原文句对应的中译句
  source_key TEXT,                                -- 他人：来源编号（S1）
  UNIQUE (card_id, position)
) STRICT;

-- 锚点与核对结果（一句一行）
CREATE TABLE concept_anchors (
  sentence_id  INTEGER PRIMARY KEY REFERENCES concept_sentences(id) ON DELETE CASCADE,
  paragraph    TEXT,                              -- 段落号；译本里的写「译:I.i 注 1」
  status       TEXT NOT NULL,                     -- pending / verified / unanchored / speculative
  span_start   INTEGER,                           -- 引文在段落（或来源正文）里的位置，按字符
  span_end     INTEGER,
  pages        TEXT,                              -- 引文的页码（110 / 110–111）
  zh_paragraph TEXT,
  zh_status    TEXT,                              -- verified / paragraph_only / unaligned
  zh_start     INTEGER,
  zh_end       INTEGER,
  note         TEXT                               -- 「没挂上原文」等说明
) STRICT;

CREATE TABLE concept_relations (
  id       INTEGER PRIMARY KEY,
  run_id   INTEGER NOT NULL REFERENCES concept_runs(id) ON DELETE CASCADE,
  key      TEXT NOT NULL,                         -- R1、R2……
  position INTEGER NOT NULL,
  from_key TEXT NOT NULL,                         -- 卡片编号
  to_key   TEXT NOT NULL,
  kind     TEXT NOT NULL,                         -- cause / contains / opposes / example / premise / revises
  note     TEXT NOT NULL,                         -- JSON：一句说明（类别、锚点、核对结果）
  step     TEXT NOT NULL,
  UNIQUE (run_id, key)
) STRICT;

-- 评论的来源：文献库里的文献，或网页检索 + 本机下载的快照
CREATE TABLE concept_sources (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES concept_runs(id) ON DELETE CASCADE,
  key           TEXT NOT NULL,                    -- S1、S2……
  position      INTEGER NOT NULL,
  origin        TEXT NOT NULL,                    -- library / web
  title         TEXT NOT NULL DEFAULT '',
  item_id       INTEGER REFERENCES items(id) ON DELETE SET NULL,
  attachment_id INTEGER REFERENCES attachments(id) ON DELETE SET NULL,
  url           TEXT,
  final_url     TEXT,
  sha256        TEXT,                             -- 正文快照
  ext           TEXT,                             -- 原件的扩展名（网页快照）
  fetched_at    TEXT,
  text_chars    INTEGER NOT NULL DEFAULT 0,
  error         TEXT,
  added_item_id INTEGER REFERENCES items(id) ON DELETE SET NULL,  -- 网上的来源加入文献库后的文献
  UNIQUE (run_id, key)
) STRICT;

CREATE INDEX concept_sources_sha ON concept_sources(sha256);

-- 核对拦下的句子（不进卡片，列进核对报告）
CREATE TABLE concept_rejected (
  id       INTEGER PRIMARY KEY,
  run_id   INTEGER NOT NULL REFERENCES concept_runs(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  step     TEXT NOT NULL,
  card     TEXT NOT NULL,
  sentence TEXT NOT NULL,                         -- JSON
  reason   TEXT NOT NULL
) STRICT;

CREATE INDEX concept_rejected_run ON concept_rejected(run_id, position);
