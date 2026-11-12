//! 本机下载（`decisions/0020`「下载」）：Pith 自己逐个下载来源，存原件和取出来的正文。
//!
//! - 只取这一个网址，不解析页面里的链接、不爬站；同一个站点两次请求之间至少隔一会儿。
//! - 超时、大小上限；跳转自己跟（每一跳都重新检查），记下最终网址。
//! - 只下 http / https；指向本机或内网的网址不下（搜索结果和模型给的网址是外来数据）。
//! - User-Agent 只有「Pith/版本号」，不带任何个人信息；不带 Cookie、不带来源页。
//! - 类型按内容判断（`content::sniff`）。下载失败、类型不认识的都返回原因，由调用方记下来。
//!
//! `Transport` 是「发一次 GET」的接口：正式用 `Http`，模拟模式用 `FixtureTransport`（不联网，给合成来源的原件）。

use std::collections::HashMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use research_engine::model::Source;
use research_engine::ports::{Fetch, Fetched};

use super::content;
use super::snapshot::Snapshots;

#[derive(Clone, Debug)]
pub struct Limits {
    pub timeout: Duration,
    pub max_bytes: u64,
    pub max_redirects: u32,
    /// 同一个站点两次请求之间至少隔多久
    pub host_gap: Duration,
    /// 允许本机和内网地址（只有测试连本地假服务器时打开）
    pub allow_private: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { timeout: Duration::from_secs(40), max_bytes: 25 * 1024 * 1024, max_redirects: 5, host_gap: Duration::from_millis(1500), allow_private: false }
    }
}

/// 一次 GET 的结果（不跟跳转）
#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

pub trait Transport: Send + Sync {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Raw, String>;
}

// ---------- 网址 ----------

/// （协议, 主机, 端口, 路径和查询）。只认 http / https，不认带用户名密码的
pub fn split_url(url: &str) -> Result<(String, String, u16, String), String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").ok_or_else(|| "不是网址".to_string())?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return Err(format!("只下载 http / https 的网址，不是「{scheme}」")),
    };
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err("网址里带着用户名或密码，不下载".into());
    }
    let (host, port) = if let Some(end) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
        // IPv6：[::1]:8080
        let host = &authority[1..=end];
        let port = authority[end + 2..].strip_prefix(':').map(|p| p.parse::<u16>()).transpose().map_err(|_| "网址的端口不对".to_string())?;
        (host.to_string(), port.unwrap_or(default_port))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "网址的端口不对".to_string())?),
            None => (authority.to_string(), default_port),
        }
    };
    if host.is_empty() {
        return Err("网址里没有主机名".into());
    }
    let path = path.split('#').next().unwrap_or("");
    Ok((scheme, host.to_ascii_lowercase(), port, if path.is_empty() { "/".to_string() } else if path.starts_with('?') { format!("/{path}") } else { path.to_string() }))
}

/// 跳转的目标：可以是完整网址、`//主机/路径`、`/路径` 或相对路径
pub fn join_url(base: &str, location: &str) -> Result<String, String> {
    let location = location.trim();
    if location.contains("://") {
        return Ok(location.to_string());
    }
    let (scheme, host, port, path) = split_url(base)?;
    let host = if host.contains(':') { format!("[{host}]") } else { host };
    let origin = if (scheme == "http" && port == 80) || (scheme == "https" && port == 443) { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") };
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{rest}"));
    }
    if location.starts_with('/') {
        return Ok(format!("{origin}{location}"));
    }
    let dir = path.split('?').next().unwrap_or("/").rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    Ok(format!("{origin}{dir}/{location}"))
}

fn private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || v.octets()[0] == 0 || (v.octets()[0] == 100 && (64..128).contains(&v.octets()[1])),
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return private_ip(IpAddr::V4(v4));
            }
            v.is_loopback() || v.is_unspecified() || v.is_multicast() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// 这个主机是不是公网上的（解析出来的地址里只要有一个是本机或内网的就不算）
fn check_public(host: &str, port: u16) -> Result<(), String> {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || [".localhost", ".local", ".internal", ".lan", ".home.arpa"].iter().any(|s| lower.ends_with(s)) {
        return Err("这个网址指向本机或内网，不下载".into());
    }
    let addrs: Vec<_> = (host, port).to_socket_addrs().map_err(|_| "找不到这个网站（域名解析不了）".to_string())?.collect();
    if addrs.is_empty() {
        return Err("找不到这个网站（域名解析不了）".into());
    }
    if addrs.iter().any(|a| private_ip(a.ip())) {
        return Err("这个网址指向本机或内网，不下载".into());
    }
    Ok(())
}

fn status_message(status: u16) -> String {
    match status {
        401 | 403 => format!("网站拒绝访问（{status}），可能要登录或挡住了程序"),
        404 | 410 => format!("网站说没有这个网页（{status}）"),
        429 => "网站说请求太频繁（429）".into(),
        s if s >= 500 => format!("网站出错了（{s}）"),
        s => format!("网站返回了 {s}"),
    }
}

// ---------- 正式实现 ----------

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10).min(timeout)))
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            // 跳转由 Downloader 自己跟：每一跳都要检查目标
            .max_redirects(0)
            .user_agent(crate::metadata::net::APP_UA)
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .build()
            .into();
        Http { agent }
    }
}

impl Transport for Http {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Raw, String> {
        let mut resp = self
            .agent
            .get(url)
            .header("Accept", "text/html,application/xhtml+xml,application/pdf,text/csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,text/plain;q=0.9,*/*;q=0.5")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.7")
            .call()
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => "超时了，网站没有在规定时间内回应".to_string(),
                ureq::Error::HostNotFound => "找不到这个网站（域名解析不了）".to_string(),
                ureq::Error::ConnectionFailed | ureq::Error::Io(_) => format!("连不上这个网站（{e}）"),
                other => format!("请求失败：{other}"),
            })?;
        let status = resp.status().as_u16();
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (location, content_type) = (header("location"), header("content-type"));
        if header("content-length").and_then(|v| v.parse::<u64>().ok()).is_some_and(|n| n > max_bytes) {
            return Err(too_big(max_bytes));
        }
        let body = resp.body_mut().with_config().limit(max_bytes).read_to_vec().map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => too_big(max_bytes),
            ureq::Error::Timeout(_) => "超时了，文件没有在规定时间内下载完".to_string(),
            other => format!("下载中断了：{other}"),
        })?;
        Ok(Raw { status, location, content_type, body })
    }
}

fn too_big(max_bytes: u64) -> String {
    let mb = max_bytes as f64 / 1024.0 / 1024.0;
    if mb >= 1.0 { format!("文件超过 {mb:.0} MB，没有下载") } else { format!("文件超过 {} KB，没有下载", max_bytes / 1024) }
}

/// 模拟模式：不联网，按路径给合成来源的原件（`fixtures`）。`round` = 同一问题的第几次调查。
pub struct FixtureTransport {
    pub round: u32,
}

impl Transport for FixtureTransport {
    fn get(&self, url: &str, _max_bytes: u64) -> Result<Raw, String> {
        let (_, _, _, path) = split_url(url)?;
        let page = super::fixtures::page(&path, self.round);
        Ok(Raw { status: page.status, location: None, content_type: Some(page.content_type.to_string()), body: page.body })
    }
}

// ---------- 下载器 ----------

pub struct Downloader {
    transport: Box<dyn Transport>,
    snapshots: Snapshots,
    limits: Limits,
    /// 每个站点上一次请求的时间
    last: Mutex<HashMap<String, Instant>>,
    now: fn() -> String,
}

impl Downloader {
    pub fn new(transport: Box<dyn Transport>, data_dir: &Path, limits: Limits) -> Self {
        Downloader { transport, snapshots: Snapshots::new(data_dir), limits, last: Mutex::new(HashMap::new()), now: crate::text::now_iso }
    }

    /// 真的联网下载
    pub fn real(data_dir: &Path) -> Self {
        let limits = Limits::default();
        Downloader::new(Box::new(Http::new(limits.timeout)), data_dir, limits)
    }

    /// 模拟模式：合成来源，不联网、不等待
    pub fn mock(data_dir: &Path, round: u32) -> Self {
        Downloader::new(Box::new(FixtureTransport { round }), data_dir, Limits { host_gap: Duration::ZERO, allow_private: true, ..Default::default() })
    }

    /// 同一个站点限速：离上一次请求不够久就等一等（可以被取消）
    fn pace(&self, host: &str, cancel: &AtomicBool) -> Result<(), String> {
        let wait = {
            let mut last = self.last.lock().map_err(|_| "内部错误".to_string())?;
            let now = Instant::now();
            let wait = last.get(host).map(|t| (*t + self.limits.host_gap).saturating_duration_since(now)).unwrap_or(Duration::ZERO);
            last.insert(host.to_string(), now + wait);
            wait
        };
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if cancel.load(Ordering::SeqCst) {
                return Err("已停止".into());
            }
            std::thread::sleep(Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())));
        }
        Ok(())
    }

    /// 下载一个网址，自己跟跳转。返回（最终网址, 回复）
    fn get(&self, url: &str, cancel: &AtomicBool) -> Result<(String, Raw), String> {
        let mut current = url.trim().to_string();
        for _ in 0..=self.limits.max_redirects {
            if cancel.load(Ordering::SeqCst) {
                return Err("已停止".into());
            }
            let (_, host, port, _) = split_url(&current)?;
            if !self.limits.allow_private {
                check_public(&host, port)?;
            }
            self.pace(&host, cancel)?;
            let raw = self.transport.get(&current, self.limits.max_bytes)?;
            match raw.status {
                200..=299 => return Ok((current, raw)),
                301 | 302 | 303 | 307 | 308 => {
                    let to = raw.location.as_deref().ok_or_else(|| "网站要跳转，但没说跳到哪".to_string())?;
                    current = join_url(&current, to)?;
                }
                s => return Err(status_message(s)),
            }
        }
        Err(format!("跳转超过 {} 次，放弃了", self.limits.max_redirects))
    }

    fn keep(&self, final_url: String, content_type: Option<String>, body: &[u8]) -> Result<Fetched, String> {
        if body.len() as u64 > self.limits.max_bytes {
            return Err(too_big(self.limits.max_bytes));
        }
        let ex = content::extract(body, content_type.as_deref(), &final_url)?;
        let sha256 = self.snapshots.put(body, ex.ext)?;
        if let Ok(text) = &ex.text {
            self.snapshots.put_text(&sha256, text)?;
        }
        Ok(Fetched { final_url, fetched_at: (self.now)(), sha256, kind: ex.kind.key().to_string(), content_type, ext: ex.ext.to_string(), bytes: body.len() as u64, title: ex.title, text: ex.text })
    }

    /// 快照文件在哪（加入文献库时把原件作为附件）
    pub fn original_path(&self, source: &Source) -> Result<std::path::PathBuf, String> {
        let sha = source.sha256.as_deref().ok_or_else(|| "这个来源没有下载到".to_string())?;
        let path = self.snapshots.original(sha, source.ext.as_deref().unwrap_or(""))?;
        if path.is_file() { Ok(path) } else { Err("快照文件不见了（可能是从备份恢复的库，快照没有一起恢复）".into()) }
    }
}

impl Fetch for Downloader {
    fn fetch(&self, url: &str, cancel: &AtomicBool) -> Result<Fetched, String> {
        let url = url.trim();
        if url.contains("://") {
            let (final_url, raw) = self.get(url, cancel)?;
            return self.keep(final_url, raw.content_type, &raw.body);
        }
        // 用户手动加的本机文件
        let path = Path::new(url);
        let meta = std::fs::metadata(path).map_err(|e| format!("读不了这个文件：{e}"))?;
        if !meta.is_file() {
            return Err("这不是一个文件".into());
        }
        if meta.len() > self.limits.max_bytes {
            return Err(too_big(self.limits.max_bytes));
        }
        let body = std::fs::read(path).map_err(|e| format!("读不了这个文件：{e}"))?;
        let mut fetched = self.keep(url.to_string(), None, &body)?;
        if fetched.title.is_none() {
            fetched.title = path.file_name().map(|n| n.to_string_lossy().into_owned());
        }
        Ok(fetched)
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        let sha = source.sha256.as_deref().ok_or_else(|| "这个来源没有下载到".to_string())?;
        self.snapshots.text(sha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::research::fixtures;
    use crate::research::testing::{FakeServer, Resp};
    use zotero_import::util::sha256_hex;

    fn local(dir: &Path) -> Downloader {
        let limits = Limits { timeout: Duration::from_secs(5), max_bytes: 200_000, max_redirects: 3, host_gap: Duration::ZERO, allow_private: true };
        Downloader::new(Box::new(Http::new(limits.timeout)), dir, limits)
    }

    fn quiet() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// 假网站：合成来源 + 几条专门测下载的路径
    fn site() -> FakeServer {
        FakeServer::start(|req| match req.path.as_str() {
            "/old" => Resp::redirect(301, "/moved/"),
            "/moved/" => Resp::redirect(302, "final.html?id=1"),
            "/moved/final.html?id=1" => Resp::new(200, "text/html; charset=utf-8", "<html><head><title>搬家后的页面</title></head><body><p>跳转两次之后的正文，够二十个字了吧。</p></body></html>"),
            "/loop" => Resp::redirect(302, "/loop"),
            "/nowhere" => Resp::new(302, "text/html", ""),
            "/to-file" => Resp::redirect(302, "file:///etc/passwd"),
            "/big" => Resp::new(200, "text/plain", vec![b'a'; 300_000]),
            "/image" => Resp::new(200, "text/html", b"\x89PNG\r\n\x1a\n0000".to_vec()),
            "/gbk" => Resp::new(200, "text/html; charset=gbk", encoding_rs::GBK.encode("<html><body><p>示例市统计局发布的数据，编码是 GBK。</p></body></html>").0.into_owned()),
            "/forbidden" => Resp::new(403, "text/html", "no"),
            "/error" => Resp::new(503, "text/html", "down"),
            path => {
                let page = fixtures::page(path, 1);
                Resp::new(page.status, page.content_type, page.body)
            }
        })
    }

    #[test]
    fn downloads_keep_the_original_the_text_and_where_it_came_from() {
        let (dir, server) = (temp_dir("research-download"), site());
        let d = local(&dir);
        let url = format!("{}/stats/bulletin-2024.html", server.base);
        let f = d.fetch(&url, &quiet()).unwrap();
        assert_eq!((f.final_url.as_str(), f.kind.as_str(), f.ext.as_str(), f.title.as_deref()), (url.as_str(), "html", "html", Some("示例市 2024 年国民经济和社会发展统计公报")));
        let original = fixtures::page("/stats/bulletin-2024.html", 1).body;
        assert_eq!((f.sha256.as_str(), f.bytes), (sha256_hex(&original).as_str(), original.len() as u64));
        assert!(f.fetched_at.starts_with("20") && f.content_type.as_deref() == Some("text/html; charset=utf-8"));
        let text = f.text.clone().unwrap();
        assert!(text.contains("调查失业率为 12.3%，比上年下降 1.1 个百分点。"));
        // 存下来的：原件一字节不差，正文是 .txt
        let folder = dir.join("research/sources").join(&f.sha256[..2]);
        assert_eq!(std::fs::read(folder.join(format!("{}.html", f.sha256))).unwrap(), original);
        assert_eq!(std::fs::read_to_string(folder.join(format!("{}.txt", f.sha256))).unwrap(), text);
        let source = Source { sha256: Some(f.sha256.clone()), ext: Some(f.ext.clone()), ..Default::default() };
        assert_eq!(d.text(&source).unwrap(), text);
        assert_eq!(d.original_path(&source).unwrap(), folder.join(format!("{}.html", f.sha256)));
        assert!(d.text(&Source::default()).is_err());

        // 请求头：不带个人信息、不带 Cookie
        let req = &server.requests()[0];
        assert_eq!(req.method, "GET");
        assert_eq!(req.header("user-agent"), Some(crate::metadata::net::APP_UA));
        assert!(req.header("cookie").is_none() && req.header("referer").is_none() && req.header("authorization").is_none());
        assert!(!crate::metadata::net::APP_UA.contains('@'));

        // 各种格式：按内容认，不看服务器说的类型
        let xlsx = d.fetch(&format!("{}/platform/platform-2024q4.xlsx", server.base), &quiet()).unwrap();
        assert_eq!((xlsx.kind.as_str(), xlsx.content_type.as_deref()), ("xlsx", Some("application/octet-stream")));
        assert!(xlsx.text.unwrap().contains("示例市\t2024Q4\t48200\t8520"));
        assert_eq!(d.fetch(&format!("{}/stats/labour-table.csv", server.base), &quiet()).unwrap().kind, "csv");
        let pdf = d.fetch(&format!("{}/survey/youth-survey-2024.pdf", server.base), &quiet()).unwrap();
        assert!(pdf.kind == "pdf" && pdf.text.is_ok());
        assert!(d.fetch(&format!("{}/gbk", server.base), &quiet()).unwrap().text.unwrap().contains("编码是 GBK"));
        // 取不出文字：原件照存，正文是失败原因
        let scan = d.fetch(&format!("{}{}", server.base, research_engine::fixture::SCANNED), &quiet()).unwrap();
        assert!(scan.text.clone().unwrap_err().contains("扫描件"));
        assert!(dir.join("research/sources").join(&scan.sha256[..2]).join(format!("{}.pdf", scan.sha256)).is_file());
        assert!(!dir.join("research/sources").join(&scan.sha256[..2]).join(format!("{}.txt", scan.sha256)).exists());
    }

    #[test]
    fn redirects_are_followed_and_the_final_url_is_recorded() {
        let (dir, server) = (temp_dir("research-redirect"), site());
        let d = local(&dir);
        let f = d.fetch(&format!("{}/old", server.base), &quiet()).unwrap();
        assert_eq!(f.final_url, format!("{}/moved/final.html?id=1", server.base));
        assert_eq!(f.title.as_deref(), Some("搬家后的页面"));
        assert_eq!(server.requests().iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/old", "/moved/", "/moved/final.html?id=1"]);
        assert!(d.fetch(&format!("{}/loop", server.base), &quiet()).unwrap_err().contains("跳转超过 3 次"));
        assert!(d.fetch(&format!("{}/nowhere", server.base), &quiet()).unwrap_err().contains("没说跳到哪"));
        // 跳到别的协议：不跟
        assert!(d.fetch(&format!("{}/to-file", server.base), &quiet()).unwrap_err().contains("只下载 http / https"));
    }

    #[test]
    fn failures_come_back_with_a_reason() {
        let (dir, server) = (temp_dir("research-download-fail"), site());
        let d = local(&dir);
        let err = |path: &str| d.fetch(&format!("{}{path}", server.base), &quiet()).unwrap_err();
        assert!(err(research_engine::fixture::MISSING).contains("没有这个网页（404）"));
        assert!(err("/forbidden").contains("拒绝访问（403）"));
        assert!(err("/error").contains("网站出错了（503）"));
        assert!(err("/big").contains("文件超过 195 KB"));
        assert!(err("/image").contains("图片"));
        // 连不上（端口上没有人听）
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(d.fetch(&format!("http://127.0.0.1:{closed}/"), &quiet()).unwrap_err().contains("连不上"));
        // 已经取消
        assert_eq!(d.fetch(&format!("{}/old", server.base), &AtomicBool::new(true)).unwrap_err(), "已停止");
        // 失败的不留快照
        assert!(!dir.join("research/sources").exists());
    }

    #[test]
    fn only_public_web_addresses_are_fetched() {
        let dir = temp_dir("research-download-private");
        let d = Downloader::new(Box::new(FixtureTransport { round: 1 }), &dir, Limits { host_gap: Duration::ZERO, ..Default::default() });
        for url in ["http://127.0.0.1/admin", "http://localhost:8080/", "http://10.0.0.8/", "http://192.168.1.1/", "http://169.254.169.254/latest/meta-data", "http://[::1]/", "http://[fd00::1]/x", "http://0.0.0.0/", "http://printer.local/", "http://100.64.1.1/"] {
            assert!(d.fetch(url, &quiet()).unwrap_err().contains("本机或内网"), "{url}");
        }
        assert!(d.fetch("ftp://files.example.invalid/a.csv", &quiet()).unwrap_err().contains("只下载 http / https"));
        assert!(d.fetch("https://user:pw@example.invalid/", &quiet()).unwrap_err().contains("用户名或密码"));
        assert!(d.fetch("https://fixture.example.invalid/x", &quiet()).unwrap_err().contains("域名解析不了"));
        assert!(d.fetch("javascript://alert(1)", &quiet()).is_err());
    }

    #[test]
    fn same_site_requests_are_spaced_out() {
        let dir = temp_dir("research-download-pace");
        let d = Downloader::new(Box::new(FixtureTransport { round: 1 }), &dir, Limits { host_gap: Duration::from_millis(120), allow_private: true, ..Default::default() });
        let start = Instant::now();
        for path in ["/stats/bulletin-2024.html", "/news/daily-relay.html", "/blog/blog-rumor.html"] {
            d.fetch(&format!("https://fixture.example.invalid{path}"), &quiet()).unwrap();
        }
        assert!(start.elapsed() >= Duration::from_millis(240), "{:?}", start.elapsed());
        // 别的站点不用等
        let start = Instant::now();
        d.fetch("https://other.example.invalid/stats/sample-city-2024.html", &quiet()).unwrap();
        assert!(start.elapsed() < Duration::from_millis(100));
        // 等的时候可以取消
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let d = Downloader::new(Box::new(FixtureTransport { round: 1 }), &dir, Limits { host_gap: Duration::from_secs(30), allow_private: true, ..Default::default() });
        d.fetch("https://fixture.example.invalid/stats/bulletin-2024.html", &cancel).unwrap();
        let flag = cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        assert_eq!(d.fetch("https://fixture.example.invalid/news/daily-relay.html", &cancel).unwrap_err(), "已停止");
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
        stopper.join().unwrap();
    }

    #[test]
    fn mock_mode_and_local_files_use_the_same_pipeline() {
        let dir = temp_dir("research-download-mock");
        let d = Downloader::mock(&dir, 2);
        let f = d.fetch("https://fixture.example.invalid/stats/bulletin-2024.html", &quiet()).unwrap();
        assert!(f.text.unwrap().contains("12.6%（修订数）"));
        assert!(d.fetch("https://fixture.example.invalid/blog/blog-rumor.html", &quiet()).unwrap_err().contains("404"));
        // 用户手动加的本机文件
        let file = dir.join("我的表.csv");
        std::fs::write(&file, "年份,人数\n2024,\"1,234\"\n").unwrap();
        let f = d.fetch(&file.display().to_string(), &quiet()).unwrap();
        assert_eq!((f.kind.as_str(), f.title.as_deref(), f.text.clone().unwrap().as_str()), ("csv", Some("我的表.csv"), "年份\t人数\n2024\t1,234\n"));
        assert_eq!(f.final_url, file.display().to_string());
        assert!(d.fetch(&dir.join("没有这个.pdf").display().to_string(), &quiet()).unwrap_err().contains("读不了这个文件"));
        assert!(d.fetch(&dir.display().to_string(), &quiet()).unwrap_err().contains("不是一个文件"));
    }

    #[test]
    fn url_helpers() {
        assert_eq!(split_url("HTTPS://Example.INVALID/a/b?x=1#top").unwrap(), ("https".into(), "example.invalid".into(), 443, "/a/b?x=1".into()));
        assert_eq!(split_url("http://[::1]:8080").unwrap(), ("http".into(), "::1".into(), 8080, "/".into()));
        assert_eq!(split_url("http://a.example.invalid?q=1").unwrap().3, "/?q=1");
        assert!(split_url("http://:80/").is_err() && split_url("http://a:b/").is_err() && split_url("no scheme").is_err());
        let base = "https://a.example.invalid/dir/page.html?x=1";
        assert_eq!(join_url(base, "https://b.example.invalid/z").unwrap(), "https://b.example.invalid/z");
        assert_eq!(join_url(base, "//c.example.invalid/z").unwrap(), "https://c.example.invalid/z");
        assert_eq!(join_url(base, "/root").unwrap(), "https://a.example.invalid/root");
        assert_eq!(join_url(base, "next.html").unwrap(), "https://a.example.invalid/dir/next.html");
        assert_eq!(join_url("http://127.0.0.1:8080/a", "b").unwrap(), "http://127.0.0.1:8080/b");
    }
}
