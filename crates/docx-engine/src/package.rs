//! .docx（OPC zip 包）读写。未改动的部件按原压缩字节原样复制，保证图片、数据部件等逐字节不变。

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Error, Result};

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub changed: bool,
}

#[derive(Clone)]
pub struct Package {
    pub entries: Vec<Entry>,
    source: Option<Vec<u8>>,
}

impl Package {
    pub fn new() -> Self {
        Package { entries: Vec::new(), source: None }
    }

    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Zip(e.to_string()))?;
        let mut entries = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| Error::Zip(e.to_string()))?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().map_err(|e| Error::Zip(e.to_string()))?.into_owned();
            let mut data = Vec::new();
            f.read_to_end(&mut data).map_err(|e| Error::Zip(e.to_string()))?;
            entries.push(Entry { name, data, changed: false });
        }
        Ok(Package { entries, source: Some(bytes.to_vec()) })
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.data.as_slice())
    }

    pub fn get_str(&self, name: &str) -> Result<String> {
        let b = self.get(name).ok_or_else(|| Error::Missing(name.to_string()))?;
        String::from_utf8(b.to_vec()).map_err(|_| Error::Xml(format!("{name} 不是 UTF-8")))
    }

    /// 设置部件内容；内容没变就不标记改动（保持原字节复制）。
    pub fn set(&mut self, name: &str, data: Vec<u8>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.name == name) {
            if e.data != data {
                e.data = data;
                e.changed = true;
            }
        } else {
            self.entries.push(Entry { name: name.to_string(), data, changed: true });
        }
    }

    pub fn changed_parts(&self) -> Vec<&str> {
        self.entries.iter().filter(|e| e.changed).map(|e| e.name.as_str()).collect()
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        let zerr = |e: zip::result::ZipError| Error::Zip(e.to_string());
        let mut src = match &self.source {
            Some(b) => Some(ZipArchive::new(Cursor::new(b.as_slice())).map_err(zerr)?),
            None => None,
        };
        let index: BTreeMap<String, usize> = match &src {
            Some(z) => z.file_names().enumerate().filter_map(|(i, n)| n.ok().map(|n| (n.into_owned(), i))).collect(),
            None => BTreeMap::new(),
        };
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for e in &self.entries {
            match (&mut src, index.get(&e.name)) {
                (Some(z), Some(&i)) if !e.changed => {
                    let f = z.by_index_raw(i).map_err(zerr)?;
                    out.raw_copy_file(f).map_err(zerr)?;
                }
                _ => {
                    out.start_file(e.name.as_str(), opts).map_err(zerr)?;
                    out.write_all(&e.data).map_err(|e| Error::Zip(e.to_string()))?;
                }
            }
        }
        Ok(out.finish().map_err(zerr)?.into_inner())
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析 .rels：返回 (Id, Type, Target, TargetMode)。
pub fn relationships(pkg: &Package, rels_path: &str) -> Vec<(String, String, String, String)> {
    let Ok(s) = pkg.get_str(rels_path) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&s) else { return Vec::new() };
    doc.descendants()
        .filter(|n| n.has_tag_name("Relationship"))
        .map(|n| {
            (
                n.attribute("Id").unwrap_or("").to_string(),
                n.attribute("Type").unwrap_or("").to_string(),
                n.attribute("Target").unwrap_or("").to_string(),
                n.attribute("TargetMode").unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// 主文档引用的「故事」部件（正文、脚注、尾注、批注、页眉页脚），返回包内路径。
pub fn story_parts(pkg: &Package) -> Vec<String> {
    let mut parts = vec!["word/document.xml".to_string()];
    for (_, ty, target, mode) in relationships(pkg, "word/_rels/document.xml.rels") {
        if mode == "External" {
            continue;
        }
        let kind = ty.rsplit('/').next().unwrap_or("");
        if matches!(kind, "footnotes" | "endnotes" | "comments" | "header" | "footer") {
            let path = if let Some(p) = target.strip_prefix('/') { p.to_string() } else { format!("word/{target}") };
            if pkg.get(&path).is_some() {
                parts.push(path);
            }
        }
    }
    parts.sort();
    parts.dedup();
    parts
}
