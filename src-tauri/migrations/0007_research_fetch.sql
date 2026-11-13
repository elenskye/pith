-- v0.3：本机下载失败时改用 Claude 网页抓取（decisions/0023）。只加一列：来源是怎么拿到的
ALTER TABLE research_sources ADD COLUMN fetched_via TEXT;  -- 空 = 本机直接下载；claude_web_fetch = 改用 Claude 网页抓取
