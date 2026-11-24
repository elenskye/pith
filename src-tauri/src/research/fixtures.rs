//! 合成来源的原件（`src-tauri/fixtures/research/`）：模拟模式下「下载」到的就是它们，走真实的保存快照和取正文。
//! 和 `research_engine::fixture` 是同一批虚构的来源（示例市）；HTML、CSV 是文件，XLSX 和 PDF 在这里用代码生成。

use docx_engine::package::Package;
use research_engine::fixture::{self, MISSING, SCANNED};

/// 假服务器 / 模拟下载给出的一份回复
#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Raw {
    Raw { status: 200, content_type, body: body.into() }
}

fn not_found() -> Raw {
    Raw { status: 404, content_type: "text/html; charset=utf-8", body: "<html><body><h1>404 Not Found</h1></body></html>".into() }
}

/// 某个路径在第 `round` 轮（1 = 第一次调查，2 = 重查）的内容
pub fn page(path: &str, round: u32) -> Raw {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.ends_with(SCANNED) {
        return ok("application/pdf", scanned_pdf());
    }
    if path.ends_with(MISSING) {
        return not_found();
    }
    let Some(doc) = fixture::doc_for(path).filter(|d| d.exists_in(round)) else { return not_found() };
    let html = "text/html; charset=utf-8";
    match doc.path {
        "/stats/bulletin-2024.html" if round >= 2 => ok(html, include_str!("../../fixtures/research/bulletin-2024-r2.html")),
        "/stats/bulletin-2024.html" => ok(html, include_str!("../../fixtures/research/bulletin-2024.html")),
        "/stats/labour-table.csv" => ok("text/csv", include_str!("../../fixtures/research/labour-table.csv")),
        "/survey/youth-survey-2024.pdf" => ok("application/pdf", survey_pdf()),
        // 故意给一个不对的类型：要按内容认出是表格
        "/platform/platform-2024q4.xlsx" => ok("application/octet-stream", platform_xlsx()),
        "/news/daily-relay.html" => ok(html, include_str!("../../fixtures/research/daily-relay.html")),
        "/blog/blog-rumor.html" => ok(html, include_str!("../../fixtures/research/blog-rumor.html")),
        "/stats/sample-city-2024.html" => ok(html, include_str!("../../fixtures/research/sample-city-2024.html")),
        "/stats/h1-2025.html" => ok(html, include_str!("../../fixtures/research/h1-2025.html")),
        _ => not_found(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 一张工作表的 XLSX：字符串用内联写法，能当数的写成数
pub fn xlsx(sheet: &str, rows: &[&[&str]]) -> Vec<u8> {
    let mut data = String::new();
    for (r, row) in rows.iter().enumerate() {
        data.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{}", (b'A' + c as u8) as char, r + 1);
            if !cell.is_empty() && cell.parse::<f64>().is_ok() {
                data.push_str(&format!("<c r=\"{at}\"><v>{cell}</v></c>"));
            } else {
                data.push_str(&format!("<c r=\"{at}\" t=\"inlineStr\"><is><t>{}</t></is></c>", xml_escape(cell)));
            }
        }
        data.push_str("</row>");
    }
    let decl = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let mut pkg = Package::new();
    pkg.set(
        "[Content_Types].xml",
        format!("{decl}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>").into_bytes(),
    );
    pkg.set(
        "_rels/.rels",
        format!("{decl}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>").into_bytes(),
    );
    pkg.set(
        "xl/workbook.xml",
        format!("{decl}<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>", xml_escape(sheet)).into_bytes(),
    );
    pkg.set(
        "xl/_rels/workbook.xml.rels",
        format!("{decl}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>").into_bytes(),
    );
    pkg.set("xl/worksheets/sheet1.xml", format!("{decl}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{data}</sheetData></worksheet>").into_bytes());
    pkg.write().unwrap_or_default()
}

pub fn platform_xlsx() -> Vec<u8> {
    xlsx("岗位与薪资", &[&["示例招聘平台 2024 年第四季度岗位数据"], &["城市", "季度", "新发布岗位数（个）", "平均招聘月薪（元）"], &["示例市", "2024Q4", "48200", "8520"], &["样本市", "2024Q4", "61500", "9140"]])
}

/// 把若干对象拼成一个 PDF（算好交叉引用表）
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 带文字的 PDF：一页，每行一段。字体不嵌入，文字靠 ToUnicode 对照表（提取文字够用；阅读器里看不到字形）。
pub fn text_pdf(lines: &[&str]) -> Vec<u8> {
    // 每个不同的字符给一个编号（从 1 起），内容流里写编号，对照表里写它是哪个字
    let mut chars: Vec<char> = Vec::new();
    let mut code = |c: char| -> usize {
        match chars.iter().position(|x| *x == c) {
            Some(i) => i + 1,
            None => {
                chars.push(c);
                chars.len()
            }
        }
    };
    let mut content = String::from("BT\n/F1 12 Tf\n72 770 Td\n18 TL\n");
    for line in lines {
        let hex: String = line.chars().map(|c| format!("{:04X}", code(c))).collect();
        content.push_str(&format!("<{hex}> Tj\nT*\n"));
    }
    content.push_str("ET");
    let mut cmap = String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for block in chars.chunks(100).enumerate().map(|(n, block)| (n * 100, block)) {
        cmap.push_str(&format!("{} beginbfchar\n", block.1.len()));
        for (i, c) in block.1.iter().enumerate() {
            let mut units = [0u16; 2];
            let utf16: String = c.encode_utf16(&mut units).iter().map(|u| format!("{u:04X}")).collect();
            cmap.push_str(&format!("<{:04X}> <{utf16}>\n", block.0 + i + 1));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 8 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /FixtureSans /Encoding /Identity-H /DescendantFonts [5 0 R] /ToUnicode 7 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /FixtureSans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 6 0 R /DW 1000 /CIDToGIDMap /Identity >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /FixtureSans /Flags 4 /FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 700 /StemV 80 >>".to_vec(),
        stream("", cmap.as_bytes()),
        stream("", content.as_bytes()),
    ])
}

pub fn survey_pdf() -> Vec<u8> {
    let text = fixture::doc_for("/survey/youth-survey-2024.pdf").map(|d| d.text).unwrap_or_default();
    text_pdf(&text.lines().collect::<Vec<_>>())
}

/// 「扫描版」PDF：一页，只画了一个灰色的方块，没有任何文字
pub fn scanned_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> /Contents 4 0 R >>".to_vec(),
        stream("", b"0.8 g\n72 200 451 500 re\nf"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::content;
    use research_engine::fold::fold;

    /// 原件取出来的正文，和引擎那边手写的正文是同一批文字（录制的模型回复里的摘录两边都要找得到）
    #[test]
    fn raw_fixtures_match_the_engine_text_fixtures() {
        for doc in fixture::DOCS {
            for round in [1, 2] {
                if !doc.exists_in(round) {
                    assert_eq!(page(doc.path, round).status, 404, "{} 第 {round} 轮应当不存在", doc.path);
                    continue;
                }
                let raw = page(doc.path, round);
                assert_eq!(raw.status, 200);
                let ex = content::extract(&raw.body, Some(raw.content_type), doc.path).unwrap();
                assert_eq!(ex.kind.key(), doc.kind, "{}", doc.path);
                let text = fold(&ex.text.unwrap_or_else(|e| panic!("{}：{e}", doc.path)));
                for line in doc.text_in(round).lines().filter(|l| !l.trim().is_empty()) {
                    assert!(text.contains(&fold(line)), "{}（第 {round} 轮）取出来的正文里没有「{line}」", doc.path);
                }
                // 录制的摘录在原件的正文里逐字找得到（除了故意写错的那一条）
                let reply: serde_json::Value = serde_json::from_str(doc.extract_in(round)).unwrap();
                for f in reply["facts"].as_array().unwrap() {
                    let quote = f["quote"].as_str().unwrap();
                    assert_eq!(text.contains(&fold(quote)), !quote.contains("35%"), "{}：「{quote}」", doc.path);
                }
            }
        }
        assert_eq!(page(MISSING, 1).status, 404);
        assert_eq!(page("/nowhere", 1).status, 404);
        // 隐藏的文字不进正文
        let raw = page("/stats/bulletin-2024.html", 1);
        assert!(!content::extract(&raw.body, Some(raw.content_type), "").unwrap().text.unwrap().contains("乘以二"));
    }
}
