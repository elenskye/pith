-- 1.1.0 第 4 期：联网（decisions/0039）。对话里取回的网页：一页一行，正文切成块（web_blocks），每块一个编号 [W12]，
-- 回答引用它时页边注是那一块原文。原件和正文是快照文件（数据目录/web/<sha256 前两位>/<sha256>.<扩展名> 和同名 .txt），只读、同内容一份。
-- 存进知识库是站主点的：建一份 web 材料（source_kind = web_page，source_ref = 这一页的 id），段落块从这里的块来。
-- 只建表、建索引（transfer/tests.rs 的 migrations_only_add）。

CREATE TABLE web_pages (
  id              INTEGER PRIMARY KEY,
  url             TEXT NOT NULL,                   -- 要取的网址
  final_url       TEXT NOT NULL,                   -- 跳转以后
  title           TEXT NOT NULL DEFAULT '',
  sha256          TEXT NOT NULL,
  ext             TEXT NOT NULL,
  kind            TEXT NOT NULL,                   -- html / pdf / xlsx / csv / text
  via             TEXT,                            -- 本机下载失败后改用的办法（claude_web_fetch）
  conversation_id INTEGER REFERENCES conversations(id) ON DELETE SET NULL,
  fetched_at      TEXT NOT NULL
) STRICT;

CREATE INDEX web_pages_url ON web_pages(final_url, sha256);

CREATE TABLE web_blocks (
  id      INTEGER PRIMARY KEY,
  page_id INTEGER NOT NULL REFERENCES web_pages(id) ON DELETE CASCADE,
  seq     INTEGER NOT NULL,
  role    TEXT NOT NULL DEFAULT 'body',            -- body / heading
  heading TEXT NOT NULL DEFAULT '',                -- 在哪个小标题下面
  text    TEXT NOT NULL,
  UNIQUE (page_id, seq)
) STRICT;
