//! `rl-file://` 协议：界面按附件 id 取文件库里的文件（`rl-file://localhost/<附件 id>`），不暴露本机路径。
//! 只给 PDF 和常见图片（HTML 快照等不在 App 里显示，避免在界面里跑网页脚本）；支持单段 Range，HEAD 只回头部。
//! 查库（`locate`）要拿数据库锁，读文件（`read`）不拿：大文件读的时候界面别的请求不用等。

use crate::common::err;
use rusqlite::{Connection, OptionalExtension};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const SCHEME: &str = "rl-file";

#[derive(Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn error(status: u16, msg: &str) -> Self {
        Reply { status, headers: vec![("Content-Type", "text/plain; charset=utf-8".into())], body: msg.as_bytes().to_vec() }
    }
}

/// 找到的文件
#[derive(Debug, PartialEq)]
pub struct Located {
    pub path: PathBuf,
    pub mime: &'static str,
}

fn mime_of(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// URI 路径 `/123`（Windows 上是 `http://rl-file.localhost/123`，路径相同）→ 附件 id → 文件。
pub fn locate(c: &Connection, attachments_dir: &Path, uri_path: &str) -> Result<Located, Reply> {
    let id: i64 = uri_path
        .trim_start_matches('/')
        .split(['/', '?', '#'])
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Reply::error(400, "地址不对：应为 rl-file://localhost/<附件 id>"))?;
    let row: Option<(Option<String>, Option<String>)> = c
        .query_row("SELECT a.sha256, f.ext FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 WHERE a.id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(|e| Reply::error(500, &err(e)))?;
    let (sha, ext) = match row {
        None => return Err(Reply::error(404, "找不到这个附件")),
        Some((Some(sha), Some(ext))) => (sha, ext),
        Some(_) => return Err(Reply::error(404, "这个附件没有文件")),
    };
    let mime = mime_of(&ext).ok_or_else(|| Reply::error(415, "这种文件不能在 App 里显示，请用系统程序打开"))?;
    let path = attachments_dir.join(crate::store::rel_path(&sha, &ext));
    if !path.is_file() {
        return Err(Reply::error(404, "文件库里找不到这个文件（可能被手动删除了）"));
    }
    Ok(Located { path, mime })
}

/// `bytes=a-b` / `bytes=a-` / `bytes=-n` → [start, end]（含两端）。多段、格式不对返回 None（按整个文件回）；越界返回 Err。
fn parse_range(h: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let r = match (a.is_empty(), b.is_empty()) {
        (true, false) => {
            let n: u64 = b.parse().ok()?;
            if n == 0 || len == 0 { Err(()) } else { Ok((len.saturating_sub(n), len - 1)) }
        }
        (false, _) => {
            let start: u64 = a.parse().ok()?;
            let end: u64 = if b.is_empty() { len.saturating_sub(1) } else { b.parse().ok()? };
            if start >= len || end < start { Err(()) } else { Ok((start, end.min(len - 1))) }
        }
        (true, true) => return None,
    };
    Some(r)
}

/// 读文件。`head` = 只要头部。
pub fn read(found: &Located, range: Option<&str>, head: bool) -> Reply {
    let mut f = match std::fs::File::open(&found.path) {
        Ok(f) => f,
        Err(e) => return Reply::error(500, &format!("读不了这个文件：{e}")),
    };
    let len = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => return Reply::error(500, &format!("读不了这个文件：{e}")),
    };
    let mut headers = vec![
        ("Content-Type", found.mime.to_string()),
        ("Accept-Ranges", "bytes".into()),
        ("Cache-Control", "no-cache".into()),
        ("X-Content-Type-Options", "nosniff".into()),
    ];
    let (status, start, end) = match range.and_then(|h| parse_range(h, len)) {
        None => (200, 0, len.saturating_sub(1)),
        Some(Ok((s, e))) => {
            headers.push(("Content-Range", format!("bytes {s}-{e}/{len}")));
            (206, s, e)
        }
        Some(Err(())) => {
            let mut r = Reply::error(416, "请求的范围超出文件大小");
            r.headers.push(("Content-Range", format!("bytes */{len}")));
            return r;
        }
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    headers.push(("Content-Length", count.to_string()));
    if head {
        return Reply { status, headers, body: Vec::new() };
    }
    let mut body = Vec::with_capacity(count as usize);
    if let Err(e) = f.seek(SeekFrom::Start(start)).and_then(|_| f.take(count).read_to_end(&mut body)) {
        return Reply::error(500, &format!("读不了这个文件：{e}"));
    }
    Reply { status, headers, body }
}

/// 允许读这些文件的页面来源：打包后的 App 页面；开发时还有 Vite 开发服务器。
pub fn allowed_origin(origin: &str, dev: bool) -> bool {
    matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") || (dev && origin == "http://localhost:1430")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{TempDir, memory_db, temp_dir};

    fn setup(name: &str) -> (Connection, TempDir) {
        let c = memory_db();
        let dir = temp_dir(name);
        let pdf = b"%PDF-1.4 0123456789";
        let sha = "ab".repeat(32);
        let p = dir.join(crate::store::rel_path(&sha, "pdf"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, pdf).unwrap();
        let html_sha = "cd".repeat(32);
        c.execute_batch(&format!(
            "INSERT INTO files (sha256, size, ext, created_at) VALUES ('{sha}', 19, 'pdf', 't'), ('{html_sha}', 5, 'html', 't'), ('{}', 3, 'png', 't');
             INSERT INTO attachments (id, uid, kind, sha256, status, created_at, updated_at) VALUES
               (1, 'A1', 'stored', '{sha}', 'ok', 't', 't'),
               (2, 'A2', 'stored', '{html_sha}', 'ok', 't', 't'),
               (3, 'A3', 'stored', NULL, 'missing', 't', 't'),
               (4, 'A4', 'stored', '{}', 'ok', 't', 't');",
            "ef".repeat(32),
            "ef".repeat(32)
        ))
        .unwrap();
        (c, dir)
    }

    fn header<'a>(r: &'a Reply, k: &str) -> Option<&'a str> {
        r.headers.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn serves_pdf_by_attachment_id() {
        let (c, dir) = setup("protocol-serve");
        let found = locate(&c, &dir, "/1").unwrap();
        assert_eq!(found.mime, "application/pdf");
        assert!(found.path.starts_with(&*dir));
        let r = read(&found, None, false);
        assert_eq!((r.status, r.body.as_slice()), (200, b"%PDF-1.4 0123456789".as_slice()));
        assert_eq!(header(&r, "Content-Length"), Some("19"));
        assert_eq!(header(&r, "Accept-Ranges"), Some("bytes"));
        let h = read(&found, None, true);
        assert!(h.body.is_empty() && h.status == 200 && header(&h, "Content-Length") == Some("19"));
        // 查询串、尾斜杠不影响
        assert_eq!(locate(&c, &dir, "/1?x=1").unwrap(), found);
    }

    #[test]
    fn ranges() {
        let (c, dir) = setup("protocol-range");
        let found = locate(&c, &dir, "/1").unwrap();
        let r = read(&found, Some("bytes=0-3"), false);
        assert_eq!((r.status, r.body.as_slice()), (206, b"%PDF".as_slice()));
        assert_eq!(header(&r, "Content-Range"), Some("bytes 0-3/19"));
        let r = read(&found, Some("bytes=15-"), false);
        assert_eq!((r.status, r.body.as_slice()), (206, b"6789".as_slice()));
        let r = read(&found, Some("bytes=-2"), false);
        assert_eq!((r.status, r.body.as_slice()), (206, b"89".as_slice()));
        let r = read(&found, Some("bytes=10-999"), false);
        assert_eq!((r.status, header(&r, "Content-Range")), (206, Some("bytes 10-18/19")));
        let r = read(&found, Some("bytes=19-"), false);
        assert_eq!((r.status, header(&r, "Content-Range")), (416, Some("bytes */19")));
        // 多段、看不懂的按整个文件
        assert_eq!(read(&found, Some("bytes=0-1,4-5"), false).status, 200);
        assert_eq!(read(&found, Some("items=0-1"), false).status, 200);
    }

    #[test]
    fn refuses_what_it_should_not_serve() {
        let (c, dir) = setup("protocol-refuse");
        let status = |p: &str| locate(&c, &dir, p).unwrap_err().status;
        assert_eq!(status("/2"), 415, "HTML 快照不在 App 里显示");
        assert_eq!(status("/3"), 404, "文件缺失");
        assert_eq!(status("/4"), 404, "库里有记录但文件被删了");
        assert_eq!(status("/999"), 404);
        assert_eq!(status("/abc"), 400);
        assert_eq!(status("/../../etc/passwd"), 400, "路径只认附件 id");
        assert_eq!(status("/"), 400);
    }

    #[test]
    fn origins() {
        assert!(allowed_origin("tauri://localhost", false));
        assert!(allowed_origin("http://tauri.localhost", false));
        assert!(!allowed_origin("http://localhost:1430", false));
        assert!(allowed_origin("http://localhost:1430", true));
        assert!(!allowed_origin("https://example.org", true));
    }
}
