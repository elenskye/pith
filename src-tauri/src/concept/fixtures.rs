//! 合成的 Mathpix 回复：模拟模式下把本机取的每页文字拼成 `lines.json` 的样子（不联网、不花钱），
//! 测试里当成录制的回复用。

use serde_json::{Value, json};

/// 每页文字 → `{pages: [{page, lines: [{type, text, region, parent_id}]}]}`：空行分段，每段一个 parent_id
pub fn lines_from_pages(pages: &[String]) -> Value {
    let mut out = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let mut lines = Vec::new();
        let mut y = 100.0;
        for (p, para) in page.split("\n\n").map(str::trim).filter(|x| !x.is_empty()).enumerate() {
            let parent = format!("p{}-{}", i + 1, p + 1);
            for (j, line) in para.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
                let x = if j == 0 { 140.0 } else { 100.0 };
                lines.push(json!({ "type": "text", "text": line, "region": { "top_left_x": x, "top_left_y": y, "width": 1000, "height": 40 }, "parent_id": parent, "conversion_output": true }));
                y += 45.0;
            }
            y += 40.0;
        }
        out.push(json!({ "page": i + 1, "page_width": 2550, "page_height": 3300, "lines": lines }));
    }
    json!({ "pages": out })
}

#[cfg_attr(not(test), allow(dead_code))]
/// 录制样例：一篇两页的合成短文（中文），第一段跨页；有页码页眉要去掉
pub const SAMPLE_LINES: &str = r#"{
  "pages": [
    { "image_id": "sample-1", "page": 1, "page_width": 2550, "page_height": 3300, "lines": [
      { "id": "l1", "type": "section_header", "text": "合成短文：分工与市场", "region": { "top_left_x": 120, "top_left_y": 120, "width": 1800, "height": 60 }, "conversion_output": true },
      { "id": "l2", "type": "text", "text": "分工越细，每个人能做的活就越专，", "region": { "top_left_x": 160, "top_left_y": 260, "width": 1800, "height": 50 }, "parent_id": "b1", "conversion_output": true },
      { "id": "l3", "type": "text", "text": "但前提是市场足够大，", "region": { "top_left_x": 120, "top_left_y": 320, "width": 1800, "height": 50 }, "parent_id": "b1", "conversion_output": true },
      { "id": "l4", "type": "page_info", "text": "1", "region": { "top_left_x": 1200, "top_left_y": 3150, "width": 60, "height": 40 }, "conversion_output": true }
    ]},
    { "image_id": "sample-2", "page": 2, "page_width": 2550, "page_height": 3300, "lines": [
      { "id": "l5", "type": "text", "text": "否则多做出来的东西卖不出去。", "region": { "top_left_x": 120, "top_left_y": 160, "width": 1800, "height": 50 }, "parent_id": "b2", "conversion_output": true },
      { "id": "l6", "type": "text", "text": "第二段：水运让市场变大。", "region": { "top_left_x": 160, "top_left_y": 300, "width": 1800, "height": 50 }, "parent_id": "b3", "conversion_output": true },
      { "id": "l7", "type": "footnote", "text": "1 合成的脚注。", "region": { "top_left_x": 120, "top_left_y": 3000, "width": 1800, "height": 40 }, "conversion_output": true }
    ]}
  ]
}"#;

#[cfg_attr(not(test), allow(dead_code))]
pub const SAMPLE_MMD: &str = "# 合成短文：分工与市场\n\n分工越细，每个人能做的活就越专，但前提是市场足够大，否则多做出来的东西卖不出去。\n\n第二段：水运让市场变大。\n";
