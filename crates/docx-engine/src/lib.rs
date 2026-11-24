//! Pith 的 Word 文档引擎（`decisions/0007`）：只改格式的排版器与独立保真校验器（Phase 0-C）、引文写入（0-B 移植）。
//!
//! - `format`：按规范（spec）改样式、页面设置、清理冲突的直接格式；正文文字、域、图片、公式、脚注等不碰
//! - `verify`：用另一套解析器（roxmltree）抽取各故事部件的文字与结构，比对前后差异；并核对规范是否生效
//! - `fixture`：生成覆盖各类结构的合成论文文档（仿中文版 Word 的样式 ID）
//! - `outline`：文档结构摘要（段落、样式用量、分节），排版 Agent 给模型看论文、预览计划用；`checks`：只检查项目的确定性统计
//! - `structure`：结构类排版（W4）：分区与分节、页眉、页码、奇数页起、三线表、脚注编号、标题紧跟
//! - `cite`：Pith 引文：数据部件、刷新计划、Word 外写入、独立核对（0-B 原型移植，`decisions/0016`）

pub mod checks;
pub mod cite;
pub mod fixture;
pub mod format;
pub mod outline;
pub mod package;
pub mod plan;
pub mod report;
pub mod spec;
pub mod structure;
pub mod template;
pub mod verify;
pub mod verify_structure;
pub mod vba;
pub mod xml;

use std::fmt;
use std::path::PathBuf;

pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_V: &str = "urn:schemas-microsoft-com:vml";

#[derive(Debug)]
pub enum Error {
    Zip(String),
    Xml(String),
    Missing(String),
    Spec(String),
    Io(std::io::Error, PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "zip 读写失败：{e}"),
            Error::Xml(e) => write!(f, "XML 解析失败：{e}"),
            Error::Missing(p) => write!(f, "文档缺少部件：{p}"),
            Error::Spec(e) => write!(f, "排版规范有误：{e}"),
            Error::Io(e, p) => write!(f, "{}：{e}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
