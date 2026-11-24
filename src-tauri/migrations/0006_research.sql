-- 0006 事实调查（v0.3 Fact Research）：调查问题、每次调查、搜索、来源、事实、计算、结论、步骤与费用。
-- 说明见 decisions/0020 与 HANDOFF-cloud.md。只建表、建索引；用过的 migration 永远不改，改结构写新编号。
-- 快照文件（原件与正文）不在库里：数据目录 research/sources/<sha256 前两位>/<sha256>.<扩展名> 和 .txt。

CREATE TABLE research_questions (
  id         INTEGER PRIMARY KEY,
  uid        TEXT NOT NULL UNIQUE,
  text       TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

-- 同一个问题可以查多次（重查），seq 从 1 起
CREATE TABLE research_runs (
  id           INTEGER PRIMARY KEY,
  uid          TEXT NOT NULL UNIQUE,
  question_id  INTEGER NOT NULL REFERENCES research_questions(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,
  previous_uid TEXT,                              -- 重查时：上一次调查的 uid
  stage        TEXT NOT NULL,                     -- plan / search / download / extract / verify / conclude / report：接下来做哪一步
  status       TEXT NOT NULL,                     -- awaiting_plan / running / cancelled / failed / done
  error        TEXT,
  mock         INTEGER NOT NULL DEFAULT 0,        -- 模拟接口给的示例
  config       TEXT NOT NULL,                     -- JSON：两个模型、检索引擎、可信度等级表、量的上限
  plan         TEXT NOT NULL,                     -- JSON：子问题、官方统计之外的维度、注意事项
  gaps         TEXT NOT NULL DEFAULT '[]',        -- JSON：没查到的
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  finished_at  TEXT,
  UNIQUE (question_id, seq)
) STRICT;

CREATE TABLE research_searches (
  id           INTEGER PRIMARY KEY,
  run_id       INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  position     INTEGER NOT NULL,
  sub_question TEXT NOT NULL,                     -- Q1…；计划时摸底搜的是空串
  query        TEXT NOT NULL,
  engine       TEXT NOT NULL,                     -- tavily / claude_web_search / mock
  hits         TEXT NOT NULL DEFAULT '[]',        -- JSON：[{url, title, snippet, published}]
  error        TEXT,
  at           TEXT NOT NULL
) STRICT;

CREATE INDEX research_searches_run ON research_searches(run_id, position);

CREATE TABLE research_sources (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  key           TEXT NOT NULL,                    -- S1、S2……一次调查里唯一
  position      INTEGER NOT NULL,
  url           TEXT NOT NULL,                    -- 网址；手动加的文件是路径
  origin        TEXT NOT NULL,                    -- search / manual
  sub_questions TEXT NOT NULL DEFAULT '[]',       -- JSON：哪些子问题搜到了它
  title         TEXT NOT NULL DEFAULT '',
  final_url     TEXT,                             -- 跳转后的网址
  fetched_at    TEXT,
  sha256        TEXT,                             -- 原件的 SHA-256，也是快照的文件名
  kind          TEXT,                             -- html / pdf / xlsx / csv / text
  content_type  TEXT,
  ext           TEXT,
  bytes         INTEGER NOT NULL DEFAULT 0,
  text_chars    INTEGER NOT NULL DEFAULT 0,
  fetch_error   TEXT,                             -- 下载失败的原因
  text_error    TEXT,                             -- 取文字失败的原因
  extracted     INTEGER NOT NULL DEFAULT 0,
  read_chars    INTEGER NOT NULL DEFAULT 0,       -- 发给模型的字数
  truncated     INTEGER NOT NULL DEFAULT 0,       -- 正文太长没读完
  extract_error TEXT,
  publisher     TEXT NOT NULL DEFAULT '',
  published     TEXT NOT NULL DEFAULT '',
  tier          TEXT,                             -- high / medium / low：已核实事实里最高的；没有就是空
  item_id       INTEGER REFERENCES items(id) ON DELETE SET NULL,  -- 加入文献库后对应的文献
  UNIQUE (run_id, key)
) STRICT;

CREATE INDEX research_sources_sha ON research_sources(sha256);
CREATE INDEX research_sources_item ON research_sources(item_id);

CREATE TABLE research_facts (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  key           TEXT NOT NULL,                    -- F1、F2……
  position      INTEGER NOT NULL,
  source_key    TEXT NOT NULL,
  sub_question  TEXT NOT NULL DEFAULT '',
  metric        TEXT NOT NULL,
  value         TEXT NOT NULL,                    -- 数值原文
  unit          TEXT NOT NULL DEFAULT '',
  region        TEXT NOT NULL DEFAULT '',
  population    TEXT NOT NULL DEFAULT '',
  period        TEXT NOT NULL DEFAULT '',
  definition    TEXT NOT NULL DEFAULT '',         -- 口径
  producer      TEXT NOT NULL DEFAULT '',         -- 数据生产者：谁统计的，不是谁转述的
  producer_kind TEXT NOT NULL,
  relayed       INTEGER NOT NULL DEFAULT 0,
  quote         TEXT NOT NULL,                    -- 原文摘录（逐字）
  unit_quote    TEXT NOT NULL DEFAULT '',
  status        TEXT NOT NULL,                    -- pending / verified / unverified（代码核对的结果）
  reason        TEXT,                             -- 没核上的原因
  span_start    INTEGER,                          -- 摘录在正文里的位置（按字符）
  span_end      INTEGER,
  occurrences   INTEGER NOT NULL DEFAULT 0,
  parsed        TEXT,                             -- JSON：数值解析的结果
  tier          TEXT NOT NULL,                    -- high / medium / low
  UNIQUE (run_id, key)
) STRICT;

CREATE TABLE research_calcs (
  id       INTEGER PRIMARY KEY,
  run_id   INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  key      TEXT NOT NULL,                         -- C1、C2……
  position INTEGER NOT NULL,
  op       TEXT NOT NULL,                         -- diff / ratio / pct_change / mean / sum
  inputs   TEXT NOT NULL,                         -- JSON：事实编号
  label    TEXT NOT NULL DEFAULT '',
  decimals INTEGER NOT NULL,
  result   TEXT,                                  -- 十进制小数的写法（不用浮点存）
  shown    TEXT NOT NULL DEFAULT '',
  unit     TEXT NOT NULL DEFAULT '',
  steps    TEXT NOT NULL DEFAULT '',              -- 算式
  error    TEXT,
  UNIQUE (run_id, key)
) STRICT;

CREATE TABLE research_conclusions (
  id          INTEGER PRIMARY KEY,
  run_id      INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  key         TEXT NOT NULL,                      -- K1、K2……
  position    INTEGER NOT NULL,
  template    TEXT NOT NULL,                      -- 模型写的原样（数用 {F1}、{C1} 占位）
  segments    TEXT NOT NULL,                      -- JSON：核对后的分段（文字 / 对上的事实 / 对上的计算 / 对不上）
  calcs       TEXT NOT NULL DEFAULT '[]',         -- JSON：用到的计算编号
  uncertainty TEXT NOT NULL DEFAULT '[]',         -- JSON：不确定的类型
  note        TEXT NOT NULL DEFAULT '',
  issues      TEXT NOT NULL DEFAULT '[]',         -- JSON：核对发现的问题
  UNIQUE (run_id, key)
) STRICT;

-- 结论与事实的对应（position：按可信度从高到低的顺序）
CREATE TABLE research_conclusion_facts (
  conclusion_id INTEGER NOT NULL REFERENCES research_conclusions(id) ON DELETE CASCADE,
  fact_id       INTEGER NOT NULL REFERENCES research_facts(id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  PRIMARY KEY (conclusion_id, fact_id)
) STRICT;

CREATE INDEX research_conclusion_facts_fact ON research_conclusion_facts(fact_id);

-- 步骤与费用（费用同时记在账本 agent/usage.jsonl 里；这里是给这次调查的报告看的）
CREATE TABLE research_steps (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES research_runs(id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  kind          TEXT NOT NULL,                    -- plan / search / download / extract / verify / conclude / report
  label         TEXT NOT NULL,
  at            TEXT NOT NULL,
  provider      TEXT NOT NULL DEFAULT '',         -- 没联网的步骤是空串
  model         TEXT NOT NULL DEFAULT '',
  input_tokens  INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  searches      INTEGER NOT NULL DEFAULT 0,
  cost_usd      REAL NOT NULL DEFAULT 0,
  mock          INTEGER NOT NULL DEFAULT 0,
  ok            INTEGER NOT NULL DEFAULT 1,
  note          TEXT
) STRICT;

CREATE INDEX research_steps_run ON research_steps(run_id, position);
