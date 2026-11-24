//! 排版规范（Formatting Spec）。v0.2 由自然语言生成；PoC 里手写 JSON（见 spec/thesis-sample.json）。

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    pub font_latin: Option<String>,
    pub font_east_asia: Option<String>,
    pub size_pt: Option<f64>,
    pub bold: Option<bool>,
    pub color: Option<String>,
    /// both | center | left | right
    pub align: Option<String>,
    /// 行距倍数（1.0 单倍、1.5 倍）；与 line_spacing_exact_pt 二选一
    pub line_spacing: Option<f64>,
    /// 固定行距（磅），如「行距固定值 20 磅」
    pub line_spacing_exact_pt: Option<f64>,
    pub space_before_pt: Option<f64>,
    pub space_after_pt: Option<f64>,
    pub first_line_indent_chars: Option<f64>,
    /// 段前分页（如「一级标题换页」）
    pub page_break_before: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HeadingStyle {
    pub level: u8,
    #[serde(flatten)]
    pub style: TextStyle,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CodeStyle {
    /// 代码段落样式名（不区分大小写）；名字含 code / 代码 的样式也算
    #[serde(default)]
    pub style_names: Vec<String>,
    #[serde(flatten)]
    pub style: TextStyle,
}

/// 页边距（cm）。哪一边没写就保留原文档的值（规范没规定的不改）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Margins {
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
    pub right: Option<f64>,
}

/// 页面设置。每一项都可以不写：不写就保留原文档的值。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    /// 目前只支持 A4；不写则不改纸张
    pub paper: Option<String>,
    #[serde(default)]
    pub margins_cm: Margins,
    pub header_cm: Option<f64>,
    pub footer_cm: Option<f64>,
    /// 装订线（cm）；不写则保留原值，改了页边距而原文档没有装订线时设为 0
    pub gutter_cm: Option<f64>,
}

impl Page {
    /// 要设置的页边距各项：(pgMar 的属性名, cm)
    pub fn margin_settings(&self) -> Vec<(&'static str, f64)> {
        let m = &self.margins_cm;
        [("top", m.top), ("right", m.right), ("bottom", m.bottom), ("left", m.left), ("header", self.header_cm), ("footer", self.footer_cm), ("gutter", self.gutter_cm)]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DirectFormatting {
    /// 去掉正文 run 上的直接字体（让样式生效）
    pub strip_fonts: bool,
    pub strip_sizes: bool,
    pub strip_colors: bool,
    /// 去掉段落上的直接行距、缩进、对齐（列表、参考文献段落保留缩进）
    pub strip_paragraph_spacing: bool,
    /// 这些字体的直接设置视为有意为之（如「关键词：」用黑体），清理时保留
    #[serde(default = "default_keep_fonts")]
    pub keep_fonts: Vec<String>,
}

impl Default for DirectFormatting {
    /// 默认全部清理：让样式生效（规范由要求清单生成时用这个）
    fn default() -> Self {
        DirectFormatting { strip_fonts: true, strip_sizes: true, strip_colors: true, strip_paragraph_spacing: true, keep_fonts: default_keep_fonts() }
    }
}

fn default_keep_fonts() -> Vec<String> {
    ["黑体", "SimHei", "楷体", "KaiTi", "楷体_GB2312", "仿宋", "FangSong", "仿宋_GB2312", "Courier New", "Consolas", "Menlo", "Monaco", "Cambria Math", "Symbol", "Wingdings"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// 中文字体的中英文名视为同一字体（Word 两种写法都会出现）。
const FONT_ALIASES: &[(&str, &str)] = &[
    ("宋体", "SimSun"),
    ("黑体", "SimHei"),
    ("楷体", "KaiTi"),
    ("仿宋", "FangSong"),
    ("新宋体", "NSimSun"),
    ("微软雅黑", "Microsoft YaHei"),
    ("等线", "DengXian"),
];

pub fn same_font(a: &str, b: &str) -> bool {
    let norm = |x: &str| {
        let x = x.trim();
        FONT_ALIASES.iter().find(|(zh, en)| x == *zh || x.eq_ignore_ascii_case(en)).map(|(zh, _)| zh.to_string()).unwrap_or_else(|| x.to_lowercase())
    };
    norm(a) == norm(b)
}

// ---------- 结构类要求（v0.2 W4，`decisions/0019`）：要改文档结构才能满足的，不只是样式 ----------

/// 页眉里代表「当前章名」的占位符（排成 STYLEREF 域，随所在页的一级标题变）
pub const CHAPTER: &str = "{章名}";

/// 页眉一行：左、中、右各放什么。文字照写，章名用 `{章名}`。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HeaderLine {
    pub left: Option<String>,
    pub center: Option<String>,
    pub right: Option<String>,
}

impl HeaderLine {
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.center.is_none() && self.right.is_none()
    }
}

/// 页眉的内容与横线。从前置部分（摘要）到正文结束；封面没有，致谢之后（计划里指出了的话）没有。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HeaderSpec {
    /// 单面印制
    #[serde(default)]
    pub single: HeaderLine,
    /// 双面印制：奇数页（右页）
    #[serde(default)]
    pub odd: HeaderLine,
    /// 双面印制：偶数页（左页）
    #[serde(default)]
    pub even: HeaderLine,
    /// 页眉下的横线：true 加、false 去掉、不写则随页眉样式
    pub rule: Option<bool>,
}

impl HeaderSpec {
    /// 实际用的两行：(默认 / 奇数页, 偶数页)。双面而规范只给了单面的写法（或反过来）时互相顶替。
    pub fn lines(&self, duplex: bool) -> (HeaderLine, Option<HeaderLine>) {
        let pick = |a: &HeaderLine, b: &HeaderLine| if a.is_empty() { b.clone() } else { a.clone() };
        if duplex {
            let odd = pick(&self.odd, &self.single);
            let even = pick(&self.even, &odd);
            (odd, Some(even))
        } else {
            (pick(&self.single, &self.odd), None)
        }
    }
}

/// 页码：页脚里一个 PAGE 域；前置部分与正文各自从 1 起、各用各的数字形式。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PageNumbers {
    /// left | center | right，不写按居中
    pub align: Option<String>,
    /// 前置部分（摘要、目录）：upper_roman | lower_roman | decimal
    pub front_format: Option<String>,
    /// 正文：decimal | upper_roman | lower_roman
    pub body_format: Option<String>,
}

pub fn page_format_ooxml(f: &str) -> Option<&'static str> {
    match f {
        "upper_roman" => Some("upperRoman"),
        "lower_roman" => Some("lowerRoman"),
        "decimal" => Some("decimal"),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FootnoteNumbering {
    /// circled（①②③）| decimal（1 2 3）
    pub format: Option<String>,
    /// each_page | each_section | continuous
    pub restart: Option<String>,
}

impl FootnoteNumbering {
    pub fn format_ooxml(&self) -> Option<&'static str> {
        match self.format.as_deref()? {
            "circled" => Some("decimalEnclosedCircleChinese"),
            "decimal" => Some("decimal"),
            _ => None,
        }
    }

    pub fn restart_ooxml(&self) -> Option<&'static str> {
        match self.restart.as_deref()? {
            "each_page" => Some("eachPage"),
            "each_section" => Some("eachSect"),
            "continuous" => Some("continuous"),
            _ => None,
        }
    }
}

/// 三线表的线宽（八分之一磅）：顶线、底线 1.5 磅，栏目线 0.75 磅（出版惯例；规范一般不写线宽）
pub const RULE_HEAVY: u32 = 12;
pub const RULE_LIGHT: u32 = 6;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    pub header: Option<HeaderSpec>,
    pub page_numbers: Option<PageNumbers>,
    /// 正文（引言）从奇数页（右页）开始
    pub body_on_odd_page: Option<bool>,
    /// 数据表格排成三线表
    pub three_line_tables: Option<bool>,
    pub footnote_numbering: Option<FootnoteNumbering>,
    /// 紧跟上一级标题时段前不空的标题级别（如 [2]：二级标题紧跟一级标题时段前为 0）
    #[serde(default)]
    pub tight_headings: Vec<u8>,    /// 多少页及以上双面印制（只用来给「单面 / 双面」一个默认值，排版器不看它）
    pub duplex_from_pages: Option<u32>,
}

impl Structure {
    pub fn is_empty(&self) -> bool {
        *self == Structure::default()
    }

    /// 要按「封面 / 前置部分 / 正文」分区处理的要求（需要计划里指出各部分从哪段开始）
    pub fn needs_zones(&self) -> bool {
        self.header.is_some() || self.page_numbers.is_some() || self.body_on_odd_page == Some(true)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub page: Page,
    #[serde(default)]
    pub body: TextStyle,
    #[serde(default)]
    pub headings: Vec<HeadingStyle>,
    pub caption: Option<TextStyle>,
    pub table_text: Option<TextStyle>,
    pub figure: Option<TextStyle>,
    pub footnote: Option<TextStyle>,
    /// 参考文献段落（内置样式 Bibliography，Zotero 书目也用它）
    pub bibliography: Option<TextStyle>,
    pub header_footer: Option<TextStyle>,
    pub toc: Option<TextStyle>,
    pub code: Option<CodeStyle>,
    #[serde(default)]
    pub direct_formatting: DirectFormatting,
    /// 打开时让 Word 更新目录、页码引用等域
    #[serde(default)]
    pub update_fields_on_open: bool,
    /// 结构类要求（页眉、页码、三线表等）；没有就不动结构
    #[serde(default, skip_serializing_if = "Structure::is_empty")]
    pub structure: Structure,
}

impl Spec {
    pub fn from_json(s: &str) -> Result<Spec> {
        let spec: Spec = serde_json::from_str(s).map_err(|e| Error::Spec(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<()> {
        let spec = self;
        if let Some(p) = spec.page.paper.as_deref()
            && p != "A4"
        {
            return Err(Error::Spec(format!("目前只支持 A4 纸，收到 {p}")));
        }
        for h in &spec.headings {
            if !(1..=9).contains(&h.level) {
                return Err(Error::Spec(format!("标题级别 {} 超出 1–9", h.level)));
            }
        }
        for ts in spec.text_styles() {
            if let Some(a) = ts.align.as_deref()
                && !matches!(a, "both" | "center" | "left" | "right") {
                    return Err(Error::Spec(format!("对齐方式只能是 both / center / left / right，收到 {a}")));
                }
            if ts.line_spacing.is_some() && ts.line_spacing_exact_pt.is_some() {
                return Err(Error::Spec("line_spacing 与 line_spacing_exact_pt 只能写一个".into()));
            }
        }
        let st = &spec.structure;
        if let Some(p) = &st.page_numbers {
            for f in [&p.front_format, &p.body_format].into_iter().flatten() {
                if page_format_ooxml(f).is_none() {
                    return Err(Error::Spec(format!("页码形式只能是 upper_roman / lower_roman / decimal，收到 {f}")));
                }
            }
            if let Some(a) = p.align.as_deref()
                && !matches!(a, "left" | "center" | "right")
            {
                return Err(Error::Spec(format!("页码位置只能是 left / center / right，收到 {a}")));
            }
        }
        if let Some(f) = &st.footnote_numbering {
            if f.format.is_some() && f.format_ooxml().is_none() {
                return Err(Error::Spec(format!("脚注编号形式只能是 circled / decimal，收到 {}", f.format.clone().unwrap_or_default())));
            }
            if f.restart.is_some() && f.restart_ooxml().is_none() {
                return Err(Error::Spec(format!("脚注编号的重新开始只能是 each_page / each_section / continuous，收到 {}", f.restart.clone().unwrap_or_default())));
            }
        }
        if st.tight_headings.iter().any(|l| !(2..=9).contains(l)) {
            return Err(Error::Spec("「紧跟上一级标题时不空」只对 2–9 级标题有意义".into()));
        }
        Ok(())
    }
}

impl Spec {
    fn text_styles(&self) -> impl Iterator<Item = &TextStyle> {
        std::iter::once(&self.body)
            .chain(self.headings.iter().map(|h| &h.style))
            .chain([&self.caption, &self.table_text, &self.figure, &self.footnote, &self.bibliography, &self.header_footer, &self.toc].into_iter().flatten())
            .chain(self.code.iter().map(|c| &c.style))
    }
}

pub fn cm_to_twips(cm: f64) -> i64 {
    (cm / 2.54 * 1440.0).round() as i64
}

pub const A4_TWIPS: (i64, i64) = (11906, 16838);
