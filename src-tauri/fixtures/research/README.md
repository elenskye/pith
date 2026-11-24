# 事实调查的合成来源（原始格式）

全部是虚构的（示例市、样本市、示例市统计局、示例大学劳动经济研究所、示例招聘平台、示例日报），数字不对应任何真实统计。

和 `crates/research-engine/fixtures/text/` 是同一批来源：那边是取出来的正文，这边是下载到的原件（HTML、CSV；XLSX 和 PDF 由 `src-tauri/src/research/fixtures.rs` 用代码生成）。
`fixtures.rs` 的测试会核对两边的文字一致。模拟模式（开发版、测试）下「下载」到的就是这些文件，走真实的保存快照和取正文。

- `*-r2.html`：重查（同一问题的第 2 次调查）时同一个网址的新内容。
- `recorded/`：录制的接口回复（Anthropic 的 SSE、OpenAI 兼容接口和 Tavily 的 JSON），接口解析的测试用。内容同样是合成的。
