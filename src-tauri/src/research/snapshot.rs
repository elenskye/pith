//! 来源的快照（`decisions/0020`「快照」）：`数据目录/research/sources/<sha256 前两位>/<sha256>.<扩展名>` 是下载到的原件，
//! 旁边的 `<sha256>.txt` 是取出来的正文。按内容存放，同样的内容只存一份；写进来就设成只读。
//! 网址、最终网址、抓取时间、SHA-256 记在主库的 research_sources 里。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use zotero_import::util::sha256_hex;

pub struct Snapshots {
    root: PathBuf,
}

fn is_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn write_read_only(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("快照路径不对")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建快照目录 {}：{e}", dir.display()))?;
    if std::fs::metadata(path).is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Ok(());
    }
    let tmp = dir.join(format!(".incoming-{}", crate::common::new_uid()?));
    let done = std::fs::write(&tmp, bytes).and_then(|_| {
        // 同名旧文件（大小不对 = 上次没写完）：先解除只读再换掉
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perm = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(path, perm);
        }
        std::fs::rename(&tmp, path)
    });
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("写不了快照 {}：{e}", path.display()));
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_readonly(true);
        let _ = std::fs::set_permissions(path, perm);
    }
    Ok(())
}

impl Snapshots {
    pub fn new(data_dir: &Path) -> Self {
        Snapshots { root: data_dir.join("research").join("sources") }
    }

    fn dir(&self, sha: &str) -> Result<PathBuf, String> {
        if !is_sha(sha) {
            return Err("快照的编号不对".into());
        }
        Ok(self.root.join(&sha[..2]))
    }

    /// 原件的路径（扩展名只留字母数字，防止拼出别的路径）
    pub fn original(&self, sha: &str, ext: &str) -> Result<PathBuf, String> {
        let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect();
        // 正文固定叫 .txt：原件不能占这个名字
        let ext = if ext.is_empty() || ext == "txt" { "text".to_string() } else { ext };
        Ok(self.dir(sha)?.join(format!("{sha}.{ext}")))
    }

    pub fn text_file(&self, sha: &str) -> Result<PathBuf, String> {
        Ok(self.dir(sha)?.join(format!("{sha}.txt")))
    }

    /// 存原件，返回它的 SHA-256
    pub fn put(&self, bytes: &[u8], ext: &str) -> Result<String, String> {
        let sha = sha256_hex(bytes);
        write_read_only(&self.original(&sha, ext)?, bytes)?;
        Ok(sha)
    }

    pub fn put_text(&self, sha: &str, text: &str) -> Result<(), String> {
        write_read_only(&self.text_file(sha)?, text.as_bytes())
    }

    pub fn text(&self, sha: &str) -> Result<String, String> {
        let path = self.text_file(sha)?;
        std::fs::read_to_string(&path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "快照文件不见了（可能是从备份恢复的库，快照没有一起恢复）".to_string() } else { format!("读不了快照 {}：{e}", path.display()) })
    }

    /// 删掉不在 `keep` 里的快照（删了调查之后调）。只动 `<两位>/<64 位 sha>.*` 这种名字的文件，返回删掉的个数。
    pub fn sweep(&self, keep: &HashSet<String>) -> usize {
        let mut removed = 0;
        let Ok(dirs) = std::fs::read_dir(&self.root) else { return 0 };
        for d in dirs.flatten().filter(|d| d.path().is_dir()) {
            let Ok(files) = std::fs::read_dir(d.path()) else { continue };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let Some((sha, _)) = name.split_once('.') else { continue };
                if !is_sha(sha) || keep.contains(sha) {
                    continue;
                }
                if let Ok(meta) = std::fs::metadata(f.path()) {
                    let mut perm = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(f.path(), perm);
                }
                if std::fs::remove_file(f.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    #[test]
    fn original_and_text_are_stored_once_by_content() {
        let dir = temp_dir("research-snapshots");
        let s = Snapshots::new(&dir);
        let sha = s.put(b"<html>page</html>", "html").unwrap();
        assert_eq!(sha, sha256_hex(b"<html>page</html>"));
        let original = dir.join("research/sources").join(&sha[..2]).join(format!("{sha}.html"));
        assert_eq!(s.original(&sha, "html").unwrap(), original);
        assert_eq!(std::fs::read(&original).unwrap(), b"<html>page</html>");
        assert!(std::fs::metadata(&original).unwrap().permissions().readonly());
        s.put_text(&sha, "page\n").unwrap();
        assert_eq!(s.text(&sha).unwrap(), "page\n");
        assert_eq!(s.text_file(&sha).unwrap(), original.with_extension("txt"));
        // 同样的内容再存一次：还是那一份
        assert_eq!(s.put(b"<html>page</html>", "html").unwrap(), sha);
        s.put_text(&sha, "page\n").unwrap();
        assert_eq!(std::fs::read_dir(original.parent().unwrap()).unwrap().count(), 2);
        // 纯文本的原件不占 .txt 这个名字
        let plain = s.put("只是文字".as_bytes(), "txt").unwrap();
        assert!(s.original(&plain, "txt").unwrap().to_string_lossy().ends_with(".text"));
        // 编号和扩展名拼不出别的路径
        assert!(s.text("../../etc/passwd").is_err());
        assert!(s.original(&sha, "../x").unwrap().to_string_lossy().ends_with(&format!("{sha}.x")));
        assert!(s.text(&sha256_hex(b"never stored")).unwrap_err().contains("不见了"));

        // 清理：只留还用着的
        std::fs::write(dir.join("research/sources").join(&sha[..2]).join("notes.md"), "别人的文件").unwrap();
        assert_eq!(s.sweep(&[sha.clone()].into()), 1);
        assert!(original.exists() && s.text(&sha).is_ok());
        assert_eq!(s.sweep(&HashSet::new()), 2);
        assert!(!original.exists() && dir.join("research/sources").join(&sha[..2]).join("notes.md").exists());
    }
}
