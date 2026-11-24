//! 合成「毕业论文」测试文档：中英混排、三级标题、目录域、脚注尾注、批注、修订、代码块、
//! 表格（含合并单元格）、图片、行内与独立公式、题注 SEQ 域、交叉引用、编号列表、分节（第二节横向）、
//! 页眉页脚页码、0-B 的引文内容控件与数据部件。全部为虚构内容。
//!
//! 样式 ID 仿中文版 Word（标题 1 的 ID 是 "1"、正文是 "a"），检验排版器是否按样式名而不是 ID 识别。

use crate::package::Package;

const W_NS_DECL: &str = concat!(
    r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" "#,
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" "#,
    r#"mc:Ignorable="w14""#
);
const DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const DATE: &str = "2026-11-07T00:00:00Z";

/// 中文版 Word 风格的样式 ID
pub mod sid {
    pub const NORMAL: &str = "a";
    pub const H1: &str = "1";
    pub const H2: &str = "2";
    pub const H3: &str = "3";
    pub const TITLE: &str = "a4";
    pub const CAPTION: &str = "a5";
    pub const FOOTNOTE_TEXT: &str = "a6";
    pub const FOOTNOTE_REF: &str = "a7";
    pub const HEADER: &str = "a8";
    pub const FOOTER: &str = "a9";
    pub const HYPERLINK: &str = "aa";
    pub const ENDNOTE_TEXT: &str = "ab";
    pub const ENDNOTE_REF: &str = "ac";
    pub const COMMENT_TEXT: &str = "ad";
    pub const COMMENT_REF: &str = "ae";
    pub const TABLE_GRID: &str = "af";
    pub const LIST_PARA: &str = "af0";
    pub const TOC_HEADING: &str = "TOC";
    pub const TOC1: &str = "11";
    pub const TOC2: &str = "21";
    pub const CODE: &str = "Code";
}

fn esc(s: &str) -> String {
    crate::xml::escape_text(s)
}

fn t(text: &str) -> String {
    format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(text))
}

fn r(text: &str) -> String {
    format!("<w:r>{}</w:r>", t(text))
}

fn r_pr(rpr: &str, text: &str) -> String {
    format!("<w:r><w:rPr>{rpr}</w:rPr>{}</w:r>", t(text))
}

fn p(style: Option<&str>, extra_ppr: &str, body: &str) -> String {
    let ps = style.map(|s| format!(r#"<w:pStyle w:val="{s}"/>"#)).unwrap_or_default();
    let ppr = if ps.is_empty() && extra_ppr.is_empty() { String::new() } else { format!("<w:pPr>{ps}{extra_ppr}</w:pPr>") };
    format!("<w:p>{ppr}{body}</w:p>")
}

fn fld(code: &str, result: &str) -> String {
    format!(
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">{}</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>{}<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        esc(code),
        r(result)
    )
}

/// 带冲突直接格式的 run（Calibri、10.5 磅、深蓝色）：排版器应去掉字体与字号，保留颜色以外的含义格式。
const DIRECT: &str = r#"<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="PingFang SC"/><w:color w:val="1F4E79"/><w:sz w:val="21"/><w:szCs w:val="21"/>"#;
const DIRECT_PPR: &str = r#"<w:spacing w:after="160" w:line="259" w:lineRule="auto"/><w:ind w:firstLine="420"/><w:jc w:val="left"/>"#;

fn toc_entry(style: &str, anchor: &str, text: &str, page: &str, begin: bool, end: bool) -> String {
    let mut body = String::new();
    if begin {
        body.push_str(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> TOC \o "1-3" \h \z \u </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"#);
    }
    body.push_str(&format!(
        r#"<w:hyperlink w:anchor="{anchor}" w:history="1">{}<w:r><w:tab/></w:r>{}</w:hyperlink>"#,
        r(text),
        fld(&format!(" PAGEREF {anchor} \\h "), page)
    ));
    if end {
        body.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
    }
    p(Some(style), r#"<w:tabs><w:tab w:val="right" w:leader="dot" w:pos="8296"/></w:tabs>"#, &body)
}

fn heading(style: &str, id: u32, name: &str, text: &str) -> String {
    p(Some(style), "", &format!(r#"<w:bookmarkStart w:id="{id}" w:name="{name}"/>{}<w:bookmarkEnd w:id="{id}"/>"#, r(text)))
}

fn cite_sdt(id: &str) -> String {
    format!(
        r#"<w:sdt><w:sdtPr><w:alias w:val="Research Lab 引文"/><w:tag w:val="rl:cite:{id}"/><w:id w:val="{n}"/><w:lock w:val="contentLocked"/></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>"#,
        r("[…]"),
        n = 700000 + id.len() as u32 * 7 + id.as_bytes()[id.len() - 1] as u32
    )
}

fn cell(text: &str, extra_tcpr: &str, bold: bool) -> String {
    let rpr = if bold { format!("<w:b/>{DIRECT}") } else { DIRECT.to_string() };
    format!(
        r#"<w:tc><w:tcPr><w:tcW w:w="2765" w:type="dxa"/>{extra_tcpr}</w:tcPr>{}</w:tc>"#,
        p(None, r#"<w:jc w:val="center"/>"#, &r_pr(&rpr, text))
    )
}

fn document_xml() -> String {
    use sid::*;
    let drawing = r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="2286000" cy="1285875"/><wp:docPr id="1" name="图片 1" descr="合成示意图"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="image1.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImg1"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2286000" cy="1285875"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let equation = r#"<m:oMathPara><m:oMath><m:r><m:t>y=</m:t></m:r><m:sSub><m:e><m:r><m:t>β</m:t></m:r></m:e><m:sub><m:r><m:t>0</m:t></m:r></m:sub></m:sSub><m:r><m:t>+</m:t></m:r><m:sSub><m:e><m:r><m:t>β</m:t></m:r></m:e><m:sub><m:r><m:t>1</m:t></m:r></m:sub></m:sSub><m:r><m:t>x+ε</m:t></m:r></m:oMath></m:oMathPara>"#;
    let sect1 = r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader1"/><w:footerReference w:type="default" r:id="rIdFooter1"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="851" w:footer="992" w:gutter="0"/><w:pgNumType w:start="1"/><w:cols w:space="425"/><w:docGrid w:type="lines" w:linePitch="312"/></w:sectPr>"#;
    let sect2 = r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader1"/><w:footerReference w:type="default" r:id="rIdFooter1"/><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1800" w:right="1440" w:bottom="1800" w:left="1440" w:header="851" w:footer="992" w:gutter="0"/><w:cols w:space="425"/><w:docGrid w:type="lines" w:linePitch="312"/></w:sectPr>"#;
    let body = [
        p(Some(TITLE), "", &r("合成论文：政治效能感测量方法比较（排版测试）")),
        p(Some(TOC_HEADING), "", &r("目录")),
        toc_entry(TOC1, "_Toc100001", "第一章 绪论", "1", true, false),
        toc_entry(TOC2, "_Toc100002", "1.1 研究背景", "1", false, false),
        toc_entry(TOC1, "_Toc100003", "第二章 方法", "2", false, true),
        heading(H1, 1, "_Toc100001", "第一章 绪论"),
        p(
            None,
            DIRECT_PPR,
            &format!(
                "{}{}{}{}{}<w:commentRangeStart w:id=\"0\"/>{}<w:commentRangeEnd w:id=\"0\"/><w:r><w:rPr><w:rStyle w:val=\"{COMMENT_REF}\"/></w:rPr><w:commentReference w:id=\"0\"/></w:r>{}",
                r_pr(DIRECT, "政治效能感（"),
                r_pr(&format!("<w:i/>{DIRECT}"), "political efficacy"),
                r_pr(DIRECT, "）是政治行为研究中的核心概念"),
                format_args!("<w:r><w:rPr><w:rStyle w:val=\"{FOOTNOTE_REF}\"/></w:rPr><w:footnoteReference w:id=\"1\"/></w:r>"),
                r_pr(DIRECT, "。本文用合成数据比较"),
                r("三种测量方法"),
                r("。"),
            ),
        ),
        p(
            None,
            DIRECT_PPR,
            &format!(
                "{}{}{}<w:hyperlink r:id=\"rIdLink1\" w:history=\"1\"><w:r><w:rPr><w:rStyle w:val=\"{HYPERLINK}\"/></w:rPr>{}</w:r></w:hyperlink>{}<m:oMath><m:r><m:t>α=0.05</m:t></m:r></m:oMath>{}",
                r("已有综述指出测量差异会影响结论"),
                cite_sdt("fx-a"),
                r("，数据说明见"),
                t("示例网站"),
                r("，显著性水平取 "),
                r("。"),
            ),
        ),
        heading(H2, 2, "_Toc100002", "1.1 研究背景"),
        p(
            None,
            "",
            &format!(
                "{}<w:ins w:id=\"10\" w:author=\"Synthetic Reviewer\" w:date=\"{DATE}\">{}</w:ins>{}<w:del w:id=\"11\" w:author=\"Synthetic Reviewer\" w:date=\"{DATE}\"><w:r><w:delText xml:space=\"preserve\">，且样本较小</w:delText></w:r></w:del>{}{}",
                r("已有研究"),
                r("大多"),
                r("使用问卷测量"),
                r("。"),
                r_pr("<w:b/>", "这一句的加粗必须保留。"),
            ),
        ),
        p(Some(LIST_PARA), r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#, &r("问卷量表法")),
        p(Some(LIST_PARA), r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#, &r("行为数据法")),
        heading(H3, 3, "_Toc100004", "1.1.1 数据来源"),
        p(
            None,
            "",
            &format!(
                "{}{}{}{}{}{}<w:r><w:rPr><w:rStyle w:val=\"{FOOTNOTE_REF}\"/></w:rPr><w:footnoteReference w:id=\"2\"/></w:r>",
                r("样本量 N=1000，拟合优度 R"),
                r_pr("<w:vertAlign w:val=\"superscript\"/>", "2"),
                r("=0.31；排放量单位 CO"),
                r_pr("<w:vertAlign w:val=\"subscript\"/>", "2"),
                r("。English sentence with "),
                r_pr("<w:u w:val=\"single\"/>", "underline"),
            ),
        ),
        p(Some(CODE), "", &r_pr(r#"<w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/>"#, "import pandas as pd")),
        p(Some(CODE), "", &r_pr(r#"<w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/>"#, r#"df = pd.read_csv("synthetic.csv")  # 合成数据"#)),
        p(Some(CODE), "", &r_pr(r#"<w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/>"#, "print(df.describe())")),
        p(None, "", equation),
        p(
            Some(CAPTION),
            "",
            &format!(
                "{}<w:bookmarkStart w:id=\"20\" w:name=\"_Ref100010\"/><w:fldSimple w:instr=\" SEQ 表 \\* ARABIC \">{}</w:fldSimple><w:bookmarkEnd w:id=\"20\"/>{}",
                r("表 "),
                r("1"),
                r(" 样本描述（合成）"),
            ),
        ),
        format!(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="{TABLE_GRID}"/><w:tblW w:w="0" w:type="auto"/><w:jc w:val="center"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid><w:gridCol w:w="2765"/><w:gridCol w:w="2765"/><w:gridCol w:w="2766"/></w:tblGrid><w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>"#,
            cell("变量", "", true),
            cell("均值", "", true),
            cell("标准差", "", true),
            cell("政治效能感", "", false),
            cell("3.42", "", false),
            cell("0.81", "", false),
            cell("合计（合并单元格）", r#"<w:gridSpan w:val="2"/>"#, false),
            cell("—", "", false),
        ),
        p(None, "", &format!("{}{}{}", r("如表 "), fld(" REF _Ref100010 \\h ", "1"), r(" 所示，均值处于中等水平。"))),
        p(None, r#"<w:jc w:val="center"/>"#, drawing),
        p(Some(CAPTION), "", &format!("{}<w:fldSimple w:instr=\" SEQ 图 \\* ARABIC \">{}</w:fldSimple>{}", r("图 "), r("1"), r(" 合成示意图"))),
        p(None, sect1, ""),
        heading(H1, 5, "_Toc100003", "第二章 方法"),
        p(
            None,
            DIRECT_PPR,
            &format!(
                "{}{}{}<w:r><w:rPr><w:rStyle w:val=\"{ENDNOTE_REF}\"/></w:rPr><w:endnoteReference w:id=\"1\"/></w:r>{}",
                r_pr(DIRECT, "本节为横向页面，用来检验分节与页面方向。"),
                cite_sdt("fx-b"),
                r("。"),
                r(""),
            ),
        ),
        heading(H1, 6, "_Toc100005", "参考文献"),
        r#"<w:sdt><w:sdtPr><w:alias w:val="Research Lab 参考文献"/><w:tag w:val="rl:bibliography"/><w:id w:val="700099"/><w:lock w:val="contentLocked"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>（参考文献生成中）</w:t></w:r></w:p></w:sdtContent></w:sdt>"#.to_string(),
        sect2.to_string(),
    ]
    .join("");
    format!(r#"{DECL}<w:document {W_NS_DECL}><w:body>{body}</w:body></w:document>"#)
}

fn styles_xml() -> String {
    use sid::*;
    let para = |id: &str, name: &str, based: Option<&str>, extra: &str| {
        let b = based.map(|b| format!(r#"<w:basedOn w:val="{b}"/>"#)).unwrap_or_default();
        format!(r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/>{b}{extra}</w:style>"#)
    };
    let chr = |id: &str, name: &str, extra: &str| {
        format!(r#"<w:style w:type="character" w:styleId="{id}"><w:name w:val="{name}"/><w:basedOn w:val="a0"/>{extra}</w:style>"#)
    };
    let heading = |id: &str, n: u8, sz: u32| {
        para(
            id,
            &format!("heading {n}"),
            Some(NORMAL),
            &format!(
                r#"<w:next w:val="{NORMAL}"/><w:link w:val="{id}0"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="340" w:after="330" w:line="578" w:lineRule="auto"/><w:outlineLvl w:val="{}"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:hAnsiTheme="majorHAnsi" w:cstheme="majorBidi"/><w:b/><w:bCs/><w:color w:val="2F5496" w:themeColor="accent1" w:themeShade="BF"/><w:kern w:val="44"/><w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/></w:rPr>"#,
                n - 1
            ),
        )
    };
    let styles = [
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:eastAsiaTheme="minorEastAsia" w:hAnsiTheme="minorHAnsi" w:cstheme="minorBidi"/><w:kern w:val="2"/><w:sz w:val="21"/><w:szCs w:val="24"/><w:lang w:val="en-US" w:eastAsia="zh-CN" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults>"#.to_string(),
        format!(r#"<w:style w:type="paragraph" w:default="1" w:styleId="{NORMAL}"><w:name w:val="Normal"/><w:qFormat/><w:pPr><w:widowControl w:val="0"/><w:jc w:val="both"/></w:pPr></w:style>"#),
        r#"<w:style w:type="character" w:default="1" w:styleId="a0"><w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/><w:unhideWhenUsed/></w:style>"#.to_string(),
        r#"<w:style w:type="table" w:default="1" w:styleId="a1"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#.to_string(),
        heading(H1, 1, 44),
        heading(H2, 2, 32),
        heading(H3, 3, 28),
        para(TITLE, "Title", Some(NORMAL), &format!(r#"<w:next w:val="{NORMAL}"/><w:qFormat/><w:pPr><w:spacing w:before="240" w:after="60"/><w:jc w:val="center"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>"#)),
        para(TOC_HEADING, "TOC Heading", Some(H1), &format!(r#"<w:next w:val="{NORMAL}"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:outlineLvl w:val="9"/></w:pPr>"#)),
        para(TOC1, "toc 1", Some(NORMAL), &format!(r#"<w:next w:val="{NORMAL}"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/>"#)),
        para(TOC2, "toc 2", Some(NORMAL), &format!(r#"<w:next w:val="{NORMAL}"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:pPr><w:ind w:leftChars="200" w:left="420"/></w:pPr>"#)),
        para(CAPTION, "caption", Some(NORMAL), &format!(r#"<w:next w:val="{NORMAL}"/><w:uiPriority w:val="35"/><w:unhideWhenUsed/><w:qFormat/><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:eastAsia="黑体" w:hAnsiTheme="majorHAnsi" w:cstheme="majorBidi"/><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>"#)),
        para(CODE, "代码", Some(NORMAL), r#"<w:qFormat/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F2F2F2"/><w:spacing w:line="240" w:lineRule="auto"/><w:jc w:val="left"/></w:pPr><w:rPr><w:rFonts w:ascii="Menlo" w:hAnsi="Menlo" w:eastAsia="PingFang SC"/><w:sz w:val="18"/></w:rPr>"#),
        para(LIST_PARA, "List Paragraph", Some(NORMAL), r#"<w:uiPriority w:val="34"/><w:qFormat/><w:pPr><w:ind w:firstLineChars="200" w:firstLine="420"/></w:pPr>"#),
        para(FOOTNOTE_TEXT, "footnote text", Some(NORMAL), r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:snapToGrid w:val="0"/><w:jc w:val="left"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr>"#),
        chr(FOOTNOTE_REF, "footnote reference", r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>"#),
        para(ENDNOTE_TEXT, "endnote text", Some(NORMAL), r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:snapToGrid w:val="0"/><w:jc w:val="left"/></w:pPr>"#),
        chr(ENDNOTE_REF, "endnote reference", r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>"#),
        para(COMMENT_TEXT, "annotation text", Some(NORMAL), r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:jc w:val="left"/></w:pPr>"#),
        chr(COMMENT_REF, "annotation reference", r#"<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:sz w:val="21"/><w:szCs w:val="21"/></w:rPr>"#),
        para(HEADER, "header", Some(NORMAL), r#"<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="auto"/></w:pBdr><w:tabs><w:tab w:val="center" w:pos="4153"/><w:tab w:val="right" w:pos="8306"/></w:tabs><w:snapToGrid w:val="0"/><w:jc w:val="center"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr>"#),
        para(FOOTER, "footer", Some(NORMAL), r#"<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4153"/><w:tab w:val="right" w:pos="8306"/></w:tabs><w:snapToGrid w:val="0"/><w:jc w:val="center"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr>"#),
        chr(HYPERLINK, "Hyperlink", r#"<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:rPr><w:color w:val="0563C1" w:themeColor="hyperlink"/><w:u w:val="single"/></w:rPr>"#),
        format!(r#"<w:style w:type="table" w:styleId="{TABLE_GRID}"><w:name w:val="Table Grid"/><w:basedOn w:val="a1"/><w:uiPriority w:val="39"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr></w:style>"#),
    ]
    .join("");
    format!(r#"{DECL}<w:styles {W_NS_DECL}>{styles}</w:styles>"#)
}

fn notes_xml(kind: &str, style_text: &str, style_ref: &str, notes: &[(u32, &str)]) -> String {
    let tag = if kind == "footnote" { "footnotes" } else { "endnotes" };
    let reff = if kind == "footnote" { "footnoteRef" } else { "endnoteRef" };
    let mut s = format!(
        r#"{DECL}<w:{tag} {W_NS_DECL}><w:{kind} w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:separator/></w:r></w:p></w:{kind}><w:{kind} w:type="continuationSeparator" w:id="0"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:{kind}>"#
    );
    for (id, text) in notes {
        s.push_str(&format!(
            r#"<w:{kind} w:id="{id}"><w:p><w:pPr><w:pStyle w:val="{style_text}"/></w:pPr><w:r><w:rPr><w:rStyle w:val="{style_ref}"/></w:rPr><w:{reff}/></w:r>{}</w:p></w:{kind}>"#,
            r_pr(DIRECT, text)
        ));
    }
    s.push_str(&format!("</w:{tag}>"));
    s
}

/// 一个 160×90 的 PNG（不压缩的 deflate 块），内容是简单的条纹图。
pub fn png() -> Vec<u8> {
    let (w, h) = (160u32, 90u32);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            let band = (x / 20 + y / 15) % 2 == 0;
            raw.extend_from_slice(if band { &[22, 110, 102] } else { &[210, 236, 232] });
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(if last { 1 } else { 0 });
        let len = chunk.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let crc = |data: &[u8]| -> u32 {
        let mut c = !0u32;
        for &x in data {
            c ^= x as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xedb88320 } else { c >> 1 };
            }
        }
        !c
    };
    let chunk = |ty: &[u8], data: &[u8]| -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        let mut td = ty.to_vec();
        td.extend_from_slice(data);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc(&td).to_be_bytes());
        out
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// 0-B 的引文数据部件（与 poc/word-addin/src/store.ts 同格式）。引文文字由 rl-docx refresh 填写。
const CITATION_STORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?><rl:citations xmlns:rl="urn:research-lab:citations:v1" version="1"><rl:json>{"version":1,"style":"apa","items":{},"citations":{"fx-a":{"items":[{"id":"rl-item-0001"}],"created":"2026-11-07T00:00:00Z"},"fx-b":{"items":[{"id":"rl-item-0002","locator":"12"},{"id":"rl-item-0004"}],"created":"2026-11-07T00:00:00Z"}},"updated":"2026-11-07T00:00:00Z"}</rl:json></rl:citations>"#;

pub fn build() -> Package {
    use sid::*;
    let mut pkg = Package::new();
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#;
    pkg.set("[Content_Types].xml", ct.as_bytes().to_vec());
    pkg.set("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#.to_vec());
    pkg.set("docProps/core.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>合成论文（排版测试）</dc:title><dc:creator>Research Lab fixture</dc:creator></cp:coreProperties>"#.as_bytes().to_vec());
    pkg.set("docProps/app.xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Research Lab fixture</Application></Properties>"#.to_vec());
    pkg.set("word/document.xml", document_xml().into_bytes());
    pkg.set("word/_rels/document.xml.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rIdNumbering" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rIdFootnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEndnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rIdComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="rIdHeader1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFooter1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rIdImg1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/><Relationship Id="rIdLink1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/synthetic-data" TargetMode="External"/><Relationship Id="rIdRL1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#.to_vec());
    pkg.set("word/styles.xml", styles_xml().into_bytes());
    pkg.set("word/settings.xml", format!(r#"{DECL}<w:settings {W_NS_DECL}><w:defaultTabStop w:val="420"/><w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#).into_bytes());
    pkg.set("word/numbering.xml", format!(r#"{DECL}<w:numbering {W_NS_DECL}><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="（%1）"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="840" w:hanging="420"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#).into_bytes());
    pkg.set("word/footnotes.xml", notes_xml("footnote", FOOTNOTE_TEXT, FOOTNOTE_REF, &[(1, " 合成脚注：本文所有数据均为虚构。"), (2, " 第二条脚注，含 English text。")]).into_bytes());
    pkg.set("word/endnotes.xml", notes_xml("endnote", ENDNOTE_TEXT, ENDNOTE_REF, &[(1, " 合成尾注：横向页面中的说明。")]).into_bytes());
    pkg.set("word/comments.xml", format!(r#"{DECL}<w:comments {W_NS_DECL}><w:comment w:id="0" w:author="Synthetic Reviewer" w:date="{DATE}" w:initials="SR"><w:p><w:pPr><w:pStyle w:val="{COMMENT_TEXT}"/></w:pPr><w:r><w:rPr><w:rStyle w:val="{COMMENT_REF}"/></w:rPr><w:annotationRef/></w:r>{}</w:p></w:comment></w:comments>"#, r("请确认这三种方法的名称。")).into_bytes());
    pkg.set("word/header1.xml", format!(r#"{DECL}<w:hdr {W_NS_DECL}>{}</w:hdr>"#, p(Some(HEADER), "", &r_pr(DIRECT, "合成学位论文（排版测试）"))).into_bytes());
    pkg.set("word/footer1.xml", format!(r#"{DECL}<w:ftr {W_NS_DECL}>{}</w:ftr>"#, p(Some(FOOTER), "", &format!("{}{}{}", r("第 "), fld(" PAGE ", "1"), r(" 页")))).into_bytes());
    pkg.set("word/media/image1.png", png());
    pkg.set("customXml/item1.xml", CITATION_STORE.as_bytes().to_vec());
    pkg.set("customXml/itemProps1.xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><ds:datastoreItem ds:itemID="{5B8C2A10-0C3F-4E57-9B1E-3D2F6A7C8E90}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"><ds:schemaRefs><ds:schemaRef ds:uri="urn:research-lab:citations:v1"/></ds:schemaRefs></ds:datastoreItem>"#.to_vec());
    pkg.set("customXml/_rels/item1.xml.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#.to_vec());
    pkg
}

/// 仿真实论文的合成文档：第 1 节是靠直接格式排版的封面（含一个空的标题 1），
/// 正文用粘贴来的 Normal (Web) / p1 样式，题注是手动居中的普通段落，参考文献用 Bibliography 样式。
/// 与 `build()` 共用其余部件（页眉页脚、脚注、图片等）。
pub fn build_real_like() -> Package {
    let mut pkg = build();
    let sect = r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr>"#;
    let body = [
        // 封面（第 1 节）
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="黑体"/><w:sz w:val="36"/></w:rPr><w:t>某某大学本科毕业论文</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:r><w:t xml:space="preserve">学    生：张三</w:t></w:r></w:p>"#.to_string(),
        format!(r#"<w:p><w:pPr><w:pStyle w:val="{}"/></w:pPr></w:p>"#, sid::H1),
        format!(r#"<w:p><w:pPr>{sect}</w:pPr></w:p>"#),
        // 正文（第 2 节）
        format!(r#"<w:p><w:pPr><w:pStyle w:val="{}"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="SimHei"/></w:rPr><w:t>1 引言</w:t></w:r></w:p>"#, sid::H1),
        r#"<w:p><w:pPr><w:pStyle w:val="NormalWeb"/><w:spacing w:before="240" w:after="240" w:line="400" w:lineRule="exact"/><w:ind w:firstLine="482"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/><w:sz w:val="24"/></w:rPr><w:t>从网页粘贴来的正文段落。</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:pPr><w:pStyle w:val="p1"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="SimHei" w:eastAsia="SimHei" w:hAnsi="SimHei"/><w:b/></w:rPr><w:t>关键词：</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>政治效能感；机器学习</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:pPr><w:pStyle w:val="NormalWeb"/><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:sz w:val="21"/></w:rPr><w:t xml:space="preserve">图1   示例图</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:pPr><w:pStyle w:val="NormalWeb"/></w:pPr><w:r><w:t>图 1 展示了一个很长的正文句子，它讨论图中的结果而不是图的题注，所以字数明显超过题注规则允许的上限。</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:pPr><w:pStyle w:val="Bibliography"/></w:pPr><w:r><w:t>[1] 作者. 题名[J]. 刊名, 2020, 1(1): 1-2.</w:t></w:r></w:p>"#.to_string(),
        sect.to_string(),
    ]
    .concat();
    let doc = pkg.get_str("word/document.xml").unwrap();
    let (head, _) = doc.split_once("<w:body>").unwrap();
    pkg.set("word/document.xml", format!("{head}<w:body>{body}</w:body></w:document>").into_bytes());
    let extra = format!(
        concat!(
            r#"<w:style w:type="paragraph" w:styleId="NormalWeb"><w:name w:val="Normal (Web)"/><w:basedOn w:val="{n}"/><w:pPr><w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" w:afterAutospacing="1"/></w:pPr><w:rPr><w:rFonts w:ascii="Times New Roman" w:eastAsia="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman"/></w:rPr></w:style>"#,
            r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="p1"><w:name w:val="p1"/><w:basedOn w:val="{n}"/><w:rPr><w:rFonts w:ascii="SimSun" w:eastAsia="SimSun" w:hAnsi="SimSun"/></w:rPr></w:style>"#,
            r#"<w:style w:type="paragraph" w:styleId="Bibliography"><w:name w:val="Bibliography"/><w:basedOn w:val="{n}"/><w:pPr><w:ind w:left="504" w:hanging="504"/></w:pPr></w:style>"#
        ),
        n = sid::NORMAL
    );
    let styles = pkg.get_str("word/styles.xml").unwrap().replace("</w:styles>", &format!("{extra}</w:styles>"));
    pkg.set("word/styles.xml", styles.into_bytes());
    pkg
}

/// 像一份完整论文的合成文档（结构类排版用）：封面、摘要、目录、正文（章、节）、有边框的数据表格、
/// 没有边框的排版用表格、脚注、致谢、原创性声明。只有一个分节，页眉页脚是原来那一套。
pub fn build_thesis_like() -> Package {
    use sid::*;
    let mut pkg = build();
    let sect = r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader1"/><w:footerReference w:type="default" r:id="rIdFooter1"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1418" w:bottom="1418" w:left="1418" w:header="907" w:footer="850" w:gutter="567"/><w:cols w:space="708"/><w:titlePg/><w:docGrid w:linePitch="360"/></w:sectPr>"#;
    let grid = r#"<w:tblGrid><w:gridCol w:w="2765"/><w:gridCol w:w="2765"/><w:gridCol w:w="2765"/></w:tblGrid>"#;
    let row = |cells: [&str; 3], bold: bool| format!("<w:tr>{}</w:tr>", cells.iter().map(|c| cell(c, "", bold)).collect::<String>());
    let data_table = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="{TABLE_GRID}"/><w:tblW w:w="0" w:type="auto"/><w:jc w:val="center"/></w:tblPr>{grid}{}{}{}</w:tbl>"#,
        row(["变量", "均值", "标准差"], true),
        row(["年龄", "21.3", "1.8"], false),
        row(["成绩", "84.2", "6.5"], false)
    );
    let layout_table = format!(r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>{grid}{}{}</w:tbl>"#, row(["作者", "张三", ""], false), row(["日期", "2026 年 5 月", ""], false));
    let footnote = format!("{}<w:r><w:rPr><w:rStyle w:val=\"{FOOTNOTE_REF}\"/></w:rPr><w:footnoteReference w:id=\"1\"/></w:r>", r("正文第一段，带一条脚注。"));
    let body = [
        p(None, r#"<w:jc w:val="center"/>"#, &r("某某大学本科毕业论文")),
        p(None, "", &r("题目：合成论文")),
        p(Some(H1), "", &r("摘  要")),
        p(None, "", &r("本文是用来测试排版的合成论文，没有真实内容。")),
        p(None, "", &r("关键词：合成；排版；测试")),
        p(Some(H1), "", &r("目  录")),
        p(None, "", &r("（目录略）")),
        p(Some(H1), "", &r("1 引言")),
        p(Some(H2), r#"<w:spacing w:before="240"/>"#, &r("1.1 研究背景")),
        p(None, "", &footnote),
        p(Some(H2), "", &r("1.2 研究问题")),
        p(None, r#"<w:jc w:val="center"/>"#, &r("表 1.1 变量的描述统计")),
        data_table,
        p(None, "", &r("下面是一个用来排版的表格，没有边框：")),
        layout_table,
        p(Some(H1), "", &r("2 方法")),
        p(Some(H2), "", &r("2.1 数据")),
        p(None, "", &r("正文第二段。")),
        p(Some(H1), "", &r("致  谢")),
        p(None, "", &r("感谢所有人。")),
        p(Some(H1), "", &r("原创性声明")),
        p(None, "", &r("本人声明所呈交的论文是本人独立完成的。")),
        sect.to_string(),
    ]
    .concat();
    let doc = pkg.get_str("word/document.xml").unwrap();
    let (head, _) = doc.split_once("<w:body>").unwrap();
    pkg.set("word/document.xml", format!("{head}<w:body>{body}</w:body></w:document>").into_bytes());
    pkg
}
