//! 本机服务（`decisions/0015`、`0016`）：Word 宏（经 AppleScript 与 curl）调用 Pith 的入口。
//! - 只监听 127.0.0.1，端口由系统随机分配；每次启动生成新令牌
//! - 端口与令牌写进数据目录的 `service.json`（只有本人可读），宏从那里读；退出时删掉
//! - 请求必须带 `Authorization: Bearer <令牌>`；带 `Origin` 头的（浏览器发来的）一律拒绝
//! - 只有 HTTP/1.1 的最小子集：一个请求一条连接，按 Content-Length 读请求体

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "service.json";
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Response { status, content_type: "text/plain; charset=utf-8", body: body.into().into_bytes() }
    }

    pub fn json(body: &impl Serialize) -> Self {
        Response { status: 200, content_type: "application/json; charset=utf-8", body: serde_json::to_vec(body).unwrap_or_default() }
    }
}

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

pub struct Service {
    pub port: u16,
    token: String,
    file: PathBuf,
    stop: Arc<AtomicBool>,
}

fn random_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| format!("无法生成令牌：{e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// 先写临时文件再改名；只有本人可读写
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| format!("无法写 {}：{e}", tmp.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| format!("无法写 {}：{e}", path.display()))
}

pub fn read_info(data_dir: &Path) -> Option<Info> {
    serde_json::from_slice(&std::fs::read(data_dir.join(FILE)).ok()?).ok()
}

impl Service {
    pub fn start(data_dir: &Path, version: &str, handler: Handler) -> Result<Service, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("本机服务无法启动：{e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = random_token()?;
        let info = Info { port, token: token.clone(), pid: std::process::id(), version: version.to_string() };
        let file = data_dir.join(FILE);
        write_private(&file, &serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stop2, token2) = (stop.clone(), token.clone());
        std::thread::Builder::new()
            .name("pith-service".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if stop2.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = conn else { continue };
                    let (h, t) = (handler.clone(), token2.clone());
                    let _ = std::thread::Builder::new().name("pith-service-conn".into()).spawn(move || serve(stream, &t, &h));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Service { port, token, file, stop })
    }

    /// 停止接受新连接；service.json 还是我们写的那份就删掉（不删别的 Pith 进程刚写的）
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if read_info(self.file.parent().unwrap_or(Path::new("."))).is_some_and(|i| i.token == self.token) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    }
}

fn respond(stream: &mut TcpStream, r: &Response) {
    let head = format!("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", r.status, reason(r.status), r.content_type, r.body.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&r.body)).and_then(|_| stream.flush());
    let _ = stream.shutdown(Shutdown::Write);
}

fn serve(mut stream: TcpStream, token: &str, handler: &Handler) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let Ok(clone) = stream.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut lines = Vec::new();
    let mut total = 0;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
        if total > MAX_HEADER {
            return respond(&mut stream, &Response::text(400, "header too large"));
        }
        let l = line.trim_end_matches(['\r', '\n']).to_string();
        if l.is_empty() {
            break;
        }
        lines.push(l);
    }
    let mut first = lines.first().map(|l| l.split(' ')).into_iter().flatten();
    let (Some(method), Some(path)) = (first.next().map(str::to_string), first.next().map(str::to_string)) else {
        return respond(&mut stream, &Response::text(400, "bad request"));
    };
    let header = |name: &str| lines.iter().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
    if header("Origin").is_some() {
        return respond(&mut stream, &Response::text(403, "forbidden"));
    }
    let auth = header("Authorization").unwrap_or_default();
    if !same(auth.strip_prefix("Bearer ").unwrap_or("").as_bytes(), token.as_bytes()) {
        return respond(&mut stream, &Response::text(401, "unauthorized"));
    }
    if header("Transfer-Encoding").is_some() {
        return respond(&mut stream, &Response::text(411, "length required"));
    }
    let len: usize = header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return respond(&mut stream, &Response::text(413, "too large"));
    }
    if header("Expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut body = vec![0u8; len];
    if reader.read_exact(&mut body).is_err() {
        return respond(&mut stream, &Response::text(400, "incomplete body"));
    }
    // 处理可能要等（排版、以后的选文献窗口）：不再限时读
    let _ = stream.set_read_timeout(None);
    let r = handler(Request { method, path, body });
    respond(&mut stream, &r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    fn call(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn token_origin_and_body_rules() {
        let dir = temp_dir("service");
        let echo: Handler = Arc::new(|r: Request| Response::text(200, format!("{} {} {}", r.method, r.path, String::from_utf8_lossy(&r.body))));
        let svc = Service::start(&dir, "0.2.0", echo).unwrap();
        let info = read_info(&dir).unwrap();
        assert_eq!((info.port, info.version.as_str(), info.pid), (svc.port, "0.2.0", std::process::id()));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600, "只有本人可读");

        let body = "{\"cmd\":\"ping\",\"q\":\"中文\"}";
        let ok = call(svc.port, &format!("POST /v1/word HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n{body}", info.token, body.len()));
        assert!(ok.starts_with("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK"), "{ok}");
        assert!(ok.ends_with(&format!("POST /v1/word {body}")));
        let no_token = call(svc.port, "POST /v1/word HTTP/1.1\r\nContent-Length: 0\r\n\r\n");
        assert!(no_token.starts_with("HTTP/1.1 401"));
        let wrong = call(svc.port, "GET /v1/ping HTTP/1.1\r\nAuthorization: Bearer 00\r\n\r\n");
        assert!(wrong.starts_with("HTTP/1.1 401"));
        let browser = call(svc.port, &format!("GET /v1/ping HTTP/1.1\r\nOrigin: http://evil.example\r\nAuthorization: Bearer {}\r\n\r\n", info.token));
        assert!(browser.starts_with("HTTP/1.1 403"));
        let chunked = call(svc.port, &format!("POST /v1/word HTTP/1.1\r\nAuthorization: Bearer {}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", info.token));
        assert!(chunked.starts_with("HTTP/1.1 411"));
        let huge = call(svc.port, &format!("POST /v1/word HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Length: 999999999\r\n\r\n", info.token));
        assert!(huge.starts_with("HTTP/1.1 413"));

        // 停止：service.json 删掉，端口不再应答
        svc.stop();
        assert!(read_info(&dir).is_none());
        std::thread::sleep(Duration::from_millis(50));
        let after = TcpStream::connect(("127.0.0.1", svc.port)).map(|mut s| {
            let _ = s.write_all(b"GET / HTTP/1.1\r\n\r\n");
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        });
        assert!(after.map_or(true, |o| o.is_empty()));
    }

    #[test]
    fn stop_leaves_a_newer_service_file_alone() {
        let dir = temp_dir("service-stale");
        let h: Handler = Arc::new(|_| Response::text(200, "x"));
        let a = Service::start(&dir, "0.2.0", h.clone()).unwrap();
        let b = Service::start(&dir, "0.2.0", h).unwrap();
        a.stop();
        assert_eq!(read_info(&dir).unwrap().port, b.port, "后启动的那份还在");
        b.stop();
        assert!(read_info(&dir).is_none());
    }
}
