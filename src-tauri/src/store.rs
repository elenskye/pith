//! 附件文件库（decisions/0003）：`attachments/<sha256 前 2 位>/<sha256>.<ext>`，同样的内容只存一份。
//! 写进来的文件设为只读：内容地址的文件被外部程序改了，哈希就对不上了。

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 相对文件库根目录的路径。
pub fn rel_path(sha256: &str, ext: &str) -> PathBuf {
    PathBuf::from(&sha256[..2]).join(format!("{sha256}.{ext}"))
}

/// 扩展名：优先用原文件名的（小写、只留字母数字、最长 10 位），没有就按 MIME 类型猜，再没有用 bin。
pub fn ext_for(name: Option<&str>, content_type: Option<&str>) -> String {
    let from_name = name
        .and_then(|n| Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty() && e.len() <= 10 && e.bytes().all(|b| b.is_ascii_alphanumeric()));
    if let Some(e) = from_name {
        return e;
    }
    match content_type.unwrap_or("") {
        "application/pdf" => "pdf",
        "text/html" | "application/xhtml+xml" => "html",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "application/epub+zip" => "epub",
        "text/plain" => "txt",
        _ => "bin",
    }
    .to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Put {
    pub sha256: String,
    pub md5: String,
    pub size: u64,
    /// 文件库里的绝对路径
    pub path: PathBuf,
    /// true = 这次新写入；false = 库里已有同样内容
    pub created: bool,
}

/// 把 `src` 复制进文件库（只读打开源文件）。边复制边算哈希，与 `expected_sha256` 不符就放弃（文件在读取后被改过）。
pub fn put(root: &Path, src: &Path, expected_sha256: &str, ext: &str) -> Result<Put, String> {
    let final_path = root.join(rel_path(expected_sha256, ext));
    let size_of = |p: &Path| std::fs::metadata(p).map(|m| m.len());
    if let (Ok(have), Ok(want)) = (size_of(&final_path), size_of(src))
        && have == want
    {
        let md5 = zotero_import::util::hash_file(&final_path).map_err(|e| format!("读不了文件库里的文件：{e}"))?.1;
        return Ok(Put {
            sha256: expected_sha256.into(),
            md5,
            size: have,
            path: final_path,
            created: false,
        });
    }
    std::fs::create_dir_all(root).map_err(|e| format!("无法创建附件目录：{e}"))?;
    let tmp = root.join(format!(".incoming-{}", crate::common::new_uid()?));
    let result = copy_hashing(src, &tmp);
    let (sha256, md5, size) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if sha256 != expected_sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("文件在读取之后被改动过，已放弃复制：{}", src.display()));
    }
    let finish = || -> Result<(), String> {
        let dir = final_path.parent().ok_or("附件路径不对")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("无法创建附件目录：{e}"))?;
        if final_path.exists() {
            // 同名旧文件大小不对（损坏）：换成这份
            let mut perm = std::fs::metadata(&final_path).map_err(|e| e.to_string())?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            std::fs::set_permissions(&final_path, perm).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &final_path).map_err(|e| format!("写入附件失败：{e}"))?;
        let mut perm = std::fs::metadata(&final_path).map_err(|e| e.to_string())?.permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&final_path, perm).map_err(|e| e.to_string())
    };
    if let Err(e) = finish() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(Put {
        sha256,
        md5,
        size,
        path: final_path,
        created: true,
    })
}

/// 边复制边算 SHA-256 / MD5，返回 (sha256, md5, 大小)。
pub(crate) fn copy_hashing(src: &Path, dst: &Path) -> Result<(String, String, u64), String> {
    let mut input = std::fs::File::open(src).map_err(|e| format!("读不了文件 {}：{e}", src.display()))?;
    let mut out = std::fs::File::create(dst).map_err(|e| format!("无法写入附件目录：{e}"))?;
    let mut sha = Sha256::new();
    let mut md5 = md5::Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf).map_err(|e| format!("读文件出错 {}：{e}", src.display()))?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        md5::Digest::update(&mut md5, &buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("写入附件失败：{e}"))?;
        size += n as u64;
    }
    out.sync_all().map_err(|e| format!("写入附件失败：{e}"))?;
    let hex = zotero_import::util::hex;
    Ok((hex(&sha.finalize()), hex(&md5::Digest::finalize(md5)), size))
}

/// 删除这次导入新写入的文件（导入失败回滚时用）。只删文件库里的。
pub fn remove_created(root: &Path, paths: &[PathBuf]) {
    for p in paths.iter().filter(|p| p.starts_with(root)) {
        remove_read_only(p);
    }
}

fn remove_read_only(p: &Path) -> bool {
    if let Ok(meta) = std::fs::metadata(p) {
        let mut perm = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        let _ = std::fs::set_permissions(p, perm);
    }
    std::fs::remove_file(p).is_ok()
}

/// 文件库里的一个文件：`<前 2 位>/<64 位 sha256>.<ext>`，返回 sha256。别的文件（临时文件、外人放进来的）不认。
fn stored_sha(dir_name: &str, file_name: &str) -> Option<String> {
    let (sha, ext) = file_name.split_once('.')?;
    let hex = |s: &str| s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (sha.len() == 64 && hex(sha) && sha.starts_with(dir_name) && dir_name.len() == 2 && !ext.is_empty()).then(|| sha.to_string())
}

/// 删掉文件库里 sha256 不在 `keep` 里的文件，返回删掉的个数。
pub fn sweep(root: &Path, keep: &std::collections::HashSet<String>) -> usize {
    let mut removed = 0;
    let Ok(dirs) = std::fs::read_dir(root) else { return 0 };
    for d in dirs.flatten().filter(|d| d.path().is_dir()) {
        let dir_name = d.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.flatten() {
            let Some(sha) = stored_sha(&dir_name, &f.file_name().to_string_lossy()) else { continue };
            if !keep.contains(&sha) && remove_read_only(&f.path()) {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use zotero_import::util::sha256_hex;

    #[test]
    fn extensions() {
        assert_eq!(ext_for(Some("Smith - 2019.PDF"), None), "pdf");
        assert_eq!(ext_for(Some("硕士学位论文.pdf"), Some("application/pdf")), "pdf");
        assert_eq!(ext_for(Some("noext"), Some("text/html")), "html");
        assert_eq!(ext_for(None, Some("image/png")), "png");
        assert_eq!(ext_for(Some("weird.ex t"), None), "bin");
        assert_eq!(ext_for(None, None), "bin");
    }

    #[test]
    fn put_dedups_verifies_and_is_read_only() {
        let dir = temp_dir("store");
        let root = dir.join("attachments");
        let src = dir.join("a.pdf");
        std::fs::write(&src, b"%PDF-1.4 hello").unwrap();
        let sha = sha256_hex(b"%PDF-1.4 hello");
        let first = put(&root, &src, &sha, "pdf").unwrap();
        assert!(first.created);
        assert_eq!(first.path, root.join(&sha[..2]).join(format!("{sha}.pdf")));
        assert_eq!(std::fs::read(&first.path).unwrap(), b"%PDF-1.4 hello");
        assert!(std::fs::metadata(&first.path).unwrap().permissions().readonly());
        assert_eq!(first.md5, zotero_import::util::md5_hex(b"%PDF-1.4 hello"));

        let copy = dir.join("b.pdf");
        std::fs::write(&copy, b"%PDF-1.4 hello").unwrap();
        let second = put(&root, &copy, &sha, "pdf").unwrap();
        assert!(!second.created, "同样内容只存一份");

        std::fs::write(&src, b"changed after reading").unwrap();
        let e = put(&root, &src, &sha256_hex(b"something else"), "pdf").unwrap_err();
        assert!(e.contains("被改动过"), "{e}");
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".incoming"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件已清理");

        remove_created(&root, std::slice::from_ref(&first.path));
        assert!(!first.path.exists());
    }

    #[test]
    fn sweep_removes_only_unused_store_files() {
        let dir = temp_dir("store-sweep");
        let root = dir.join("attachments");
        let mut shas = Vec::new();
        for body in [b"one".as_slice(), b"two", b"three"] {
            let src = dir.join("src");
            std::fs::write(&src, body).unwrap();
            let sha = sha256_hex(body);
            put(&root, &src, &sha, "pdf").unwrap();
            shas.push(sha);
        }
        std::fs::write(root.join(".incoming-x"), b"tmp").unwrap();
        std::fs::write(root.join(&shas[0][..2]).join("notes.txt"), b"x").unwrap();
        let keep: std::collections::HashSet<String> = [shas[0].clone(), shas[2].clone()].into();
        assert_eq!(sweep(&root, &keep), 1);
        assert!(!root.join(rel_path(&shas[1], "pdf")).exists());
        assert!(root.join(rel_path(&shas[0], "pdf")).exists() && root.join(rel_path(&shas[2], "pdf")).exists());
        assert!(root.join(".incoming-x").exists() && root.join(&shas[0][..2]).join("notes.txt").exists(), "不认识的文件不动");
        assert_eq!(sweep(&root, &keep), 0);
    }
}
