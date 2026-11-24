//! 测试用的本地假服务器（`TcpListener`，只听 127.0.0.1 的随机端口）：下载和各接口的测试都连它，不连外网。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct Req {
    pub method: String,
    /// 路径和查询串
    pub path: String,
    /// 头的名字都是小写
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Resp {
        Resp { status, headers: vec![("Content-Type".into(), content_type.into())], body: body.into() }
    }

    pub fn json(status: u16, body: &str) -> Resp {
        Resp::new(status, "application/json", body)
    }

    pub fn sse(body: &str) -> Resp {
        Resp::new(200, "text/event-stream", body)
    }

    pub fn redirect(status: u16, to: &str) -> Resp {
        Resp { status, headers: vec![("Location".into(), to.into())], body: Vec::new() }
    }

    pub fn with(mut self, name: &str, value: &str) -> Resp {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub struct FakeServer {
    /// `http://127.0.0.1:<端口>`
    pub base: String,
    requests: Arc<Mutex<Vec<Req>>>,
    stop: Arc<AtomicBool>,
    port: u16,
}

fn serve(mut stream: TcpStream, handler: &(dyn Fn(&Req) -> Resp + Send + Sync), log: &Mutex<Vec<Req>>) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() || first.trim().is_empty() {
        return;
    }
    let mut parts = first.split_whitespace();
    let mut req = Req { method: parts.next().unwrap_or("").to_string(), path: parts.next().unwrap_or("").to_string(), ..Default::default() };
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            req.headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; len];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    req.body = String::from_utf8_lossy(&body).into_owned();
    let resp = handler(&req);
    if let Ok(mut l) = log.lock() {
        l.push(req);
    }
    let reason = match resp.status {
        200 => "OK",
        301 | 302 | 303 | 307 | 308 => "Redirect",
        404 => "Not Found",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n", resp.status, resp.body.len());
    for (k, v) in &resp.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

impl FakeServer {
    pub fn start(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let port = listener.local_addr().expect("本地地址").port();
        let (requests, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (log, stopping, handler) = (requests.clone(), stop.clone(), Arc::new(handler));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    let (handler, log) = (handler.clone(), log.clone());
                    std::thread::spawn(move || serve(stream, handler.as_ref(), &log));
                }
            }
        });
        FakeServer { base: format!("http://127.0.0.1:{port}"), requests, stop, port }
    }

    /// 收到过的请求，按先后
    pub fn requests(&self) -> Vec<Req> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 让 accept 醒过来看到停止标记
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}
