-- 1.0.2：对话、角色、知识库（decisions/0030）。
-- 对话的回答这一版是模拟的（src/chat/mock.rs，不调模型、不花钱）；表照新架构定，接上 Agent 循环时不用再改。
-- 只建表、建索引（transfer/tests.rs 的 migrations_only_add）。

-- 对话。skill + ref_uid 连到技能的那次运行（research：调查的 uid；concept：研究的 uid；format：排版的 id）
CREATE TABLE conversations (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  title      TEXT NOT NULL,
  role_uid   TEXT NOT NULL DEFAULT 'pith',
  kbs        TEXT NOT NULL DEFAULT '[]',          -- JSON：这次勾的知识库 uid
  skill      TEXT,
  ref_uid    TEXT,
  stage      INTEGER NOT NULL DEFAULT 0,          -- 生长阶段：0 刚种下、1 长着、2 常青
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX conversations_updated ON conversations(updated_at);
CREATE INDEX conversations_ref ON conversations(skill, ref_uid);

-- 一条消息。body 是 JSON：你说的 {text}；Pith 的 {steps, paragraphs, cites, ask, delivery, mock}
CREATE TABLE messages (
  id              INTEGER PRIMARY KEY,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  position        INTEGER NOT NULL,
  author          TEXT NOT NULL,                  -- you，或者角色的 uid
  body            TEXT NOT NULL,
  created_at      TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX messages_position ON messages(conversation_id, position);

-- 角色。内置的四个（pith / companion / reviewer / editor）写在代码里，改过的用同一个 uid 存在这里；新建的也在这里
CREATE TABLE roles (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  mark       TEXT NOT NULL,                       -- 头像上的一个字
  line       TEXT NOT NULL DEFAULT '',
  prompt     TEXT NOT NULL DEFAULT '',
  kb         TEXT NOT NULL DEFAULT '',            -- 默认知识库的说法
  model      TEXT NOT NULL DEFAULT '',
  skills     TEXT NOT NULL DEFAULT '[]',          -- JSON：常用技能
  position   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

-- 知识库：材料的分组（研究主题只给对话分组，知识库另建，decisions/0028）
CREATE TABLE knowledge_bases (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  note       TEXT NOT NULL DEFAULT '',
  position   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

-- 知识库里的一份材料。kind 是十种记录之一（full / excerpt / note / web / file / scan / concept / report / wonder / answer）；
-- source_kind + source_ref 指回原来的东西（concept_text：文本 uid；research_run：调查 uid；concept_card：研究 uid#卡片 key；item：条目 id）
CREATE TABLE kb_materials (
  id              INTEGER PRIMARY KEY,
  uid             TEXT NOT NULL UNIQUE,
  kb_id           INTEGER NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
  kind            TEXT NOT NULL,
  title           TEXT NOT NULL,
  sub             TEXT NOT NULL DEFAULT '',
  body            TEXT NOT NULL DEFAULT '',
  stage           INTEGER NOT NULL DEFAULT 0,
  source_kind     TEXT,
  source_ref      TEXT,
  conversation_id INTEGER REFERENCES conversations(id) ON DELETE SET NULL,
  created_at      TEXT NOT NULL,
  tended_at       TEXT NOT NULL
) STRICT;

CREATE INDEX kb_materials_kb ON kb_materials(kb_id, created_at);
CREATE INDEX kb_materials_source ON kb_materials(source_kind, source_ref);
