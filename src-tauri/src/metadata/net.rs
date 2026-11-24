//! 联网查询（`decisions/0012`）：
//! - DOI → `https://doi.org/<DOI>`，按内容协商要 CSL JSON（doi.org 转到 Crossref、DataCite 等登记机构）；
//! - ISBN → Open Library 的 Books API。
//!
//! 只发 DOI / ISBN 本身。联系邮箱只放在发往 doi.org 的 User-Agent 里（Crossref 的「礼貌请求」），Open Library 不带。
//! 查到的记录换成 Zotero 字段名（DOI 的 CSL JSON 复用导入用的解析器）。网络不在这里重试以外的地方处理。

use super::{Found, FoundCreator, Identifier};
use crate::import::formats::{self, Format, Record};
use crate::text;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

pub const APP_UA: &str = concat!("Pith/", env!("CARGO_PKG_VERSION"), " (personal research library app)");

pub struct Response {
    pub status: u16,
    pub body: String,
    /// 跳转后的最终地址（看是哪家登记机构）
    pub url: String,
    /// 429 / 503 时服务端要求等的秒数
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetError {
    /// 连不上、超时、DNS 失败：多半是没网
    Offline(String),
    /// 别的失败（服务端出错、限流后重试仍不行、返回看不懂）
    Other(String),
}

impl NetError {
    pub fn message(&self) -> String {
        match self {
            NetError::Offline(m) => format!("连不上网络：{m}"),
            NetError::Other(m) => m.clone(),
        }
    }
}

/// 发 GET 的接口：正式用 `Ureq`，测试用录好的响应。`contact` = 这次请求可以带联系邮箱。
pub trait Http: Send + Sync {
    fn get(&self, url: &str, accept: &str, contact: bool) -> Result<Response, NetError>;
}

/// 用 ureq + macOS 系统 TLS（信任系统证书）
pub struct Ureq {
    with_contact: ureq::Agent,
    plain: ureq::Agent,
}

impl Ureq {
    pub fn new(email: Option<&str>) -> Self {
        let agent = |ua: String| -> ureq::Agent {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(8)))
                .timeout_global(Some(Duration::from_secs(25)))
                .http_status_as_error(false)
                .max_redirects(5)
                .user_agent(ua)
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .provider(ureq::tls::TlsProvider::NativeTls)
                        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                        .build(),
                )
                .build()
                .into()
        };
        let contact = match email.map(str::trim).filter(|e| !e.is_empty()) {
            Some(e) => format!("{APP_UA}; mailto:{e}"),
            None => APP_UA.to_string(),
        };
        Ureq { with_contact: agent(contact), plain: agent(APP_UA.to_string()) }
    }
}

impl Http for Ureq {
    fn get(&self, url: &str, accept: &str, contact: bool) -> Result<Response, NetError> {
        use ureq::ResponseExt;
        let agent = if contact { &self.with_contact } else { &self.plain };
        let mut resp = agent.get(url).header("Accept", accept).call().map_err(|e| match e {
            ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => NetError::Offline(e.to_string()),
            other => NetError::Other(format!("请求失败：{other}")),
        })?;
        let status = resp.status().as_u16();
        let final_url = resp.get_uri().to_string();
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok());
        // 个别服务返回的不是 UTF-8：按字节读、坏字符替换掉，不因为编码整条失败
        let bytes = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| NetError::Other(format!("读取响应失败：{e}")))?;
        let body = String::from_utf8_lossy(&bytes).into_owned();
        Ok(Response { status, body, url: final_url, retry_after })
    }
}

/// 同一个服务两次请求之间至少隔多久（doi.org / Crossref 0.25 秒，Open Library 1 秒）
#[derive(Default)]
pub struct Pacer {
    last: HashMap<&'static str, Instant>,
    /// 测试里关掉等待
    pub disabled: bool,
}

impl Pacer {
    /// 不等待（测试用）
    #[cfg(test)]
    pub fn immediate() -> Self {
        Pacer { disabled: true, ..Default::default() }
    }

    fn wait(&mut self, host: &'static str, gap: Duration) {
        if self.disabled {
            return;
        }
        if let Some(t) = self.last.get(host) {
            let since = t.elapsed();
            if since < gap {
                std::thread::sleep(gap - since);
            }
        }
        self.last.insert(host, Instant::now());
    }
}

/// 发请求；429 / 502 / 503 / 504 按服务端要求等一会儿再试，最多再试 2 次。
fn get_polite(http: &dyn Http, pacer: &mut Pacer, host: &'static str, gap: Duration, url: &str, accept: &str, contact: bool) -> Result<Response, NetError> {
    let mut tries = 0;
    loop {
        pacer.wait(host, gap);
        let r = http.get(url, accept, contact)?;
        if matches!(r.status, 429 | 502 | 503 | 504) && tries < 2 {
            tries += 1;
            if !pacer.disabled {
                std::thread::sleep(Duration::from_secs(r.retry_after.unwrap_or(3).clamp(1, 20)));
            }
            continue;
        }
        return Ok(r);
    }
}

pub enum Outcome {
    Found(Found),
    /// 查过了，没有这条（原因，中文）
    NotFound(String),
}

/// 按一个标识查。
pub fn lookup(http: &dyn Http, pacer: &mut Pacer, id: &Identifier) -> Result<Outcome, NetError> {
    match id {
        Identifier::Doi(doi) => lookup_doi(http, pacer, doi),
        Identifier::Isbn(isbn) => lookup_isbn(http, pacer, isbn),
    }
}

/// DOI 放进路径：除了不保留字符和 `/` 以外都百分号编码（DOI 里可以有 `<`、`#`、`?`、`;` 等）
pub fn doi_url(doi: &str) -> String {
    let mut s = String::from("https://doi.org/");
    for b in doi.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

pub const CSL_ACCEPT: &str = "application/vnd.citationstyles.csl+json";

fn lookup_doi(http: &dyn Http, pacer: &mut Pacer, doi: &str) -> Result<Outcome, NetError> {
    let r = get_polite(http, pacer, "doi.org", Duration::from_millis(250), &doi_url(doi), CSL_ACCEPT, true)?;
    match r.status {
        200 => {}
        404 => return Ok(Outcome::NotFound("doi.org 上没有这个 DOI（可能写错了）".into())),
        204 | 406 | 415 => return Ok(Outcome::NotFound("这个 DOI 的登记机构不提供可读的元数据".into())),
        s => return Err(NetError::Other(format!("doi.org 返回错误（{s}）"))),
    }
    let source = registration_agency(&r.url);
    // 有的登记机构对内容协商回网页而不是 JSON
    if !r.body.trim_start().starts_with('{') {
        return Ok(Outcome::NotFound(format!("{source} 没有给出可读的元数据")));
    }
    let body = crossref_types(&r.body);
    let parsed = formats::parse(Format::CslJson, &body).map_err(|e| NetError::Other(format!("{source} 的返回看不懂：{e}")))?;
    match parsed.records.into_iter().next() {
        Some(rec) => Ok(Outcome::Found(from_csl(rec, &source))),
        None => Ok(Outcome::NotFound(format!("{source} 没有给出可读的元数据"))),
    }
}

/// Crossref 的内容协商返回的 type 是它自己的类型名（journal-article、book-chapter……），不是 CSL 的；先换成 CSL 的再解析
fn crossref_types(body: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(body) else { return body.to_string() };
    if let Some(t) = v.get("type").and_then(Value::as_str) {
        let csl = match t {
            "journal-article" => "article-journal",
            "book-chapter" | "book-section" | "book-part" => "chapter",
            "proceedings-article" => "paper-conference",
            "posted-content" => "article",
            "monograph" | "edited-book" | "reference-book" | "book-set" => "book",
            "reference-entry" => "entry-encyclopedia",
            "dissertation" => "thesis",
            "report-component" => "report",
            "database" => "dataset",
            "peer-review" => "review",
            "other" => "document",
            other => other,
        };
        v["type"] = Value::String(csl.to_string());
    }
    v.to_string()
}

/// 跳转后的地址 → 登记机构
fn registration_agency(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', ':']).next().unwrap_or("").to_ascii_lowercase();
    for (k, name) in [("crossref", "crossref"), ("datacite", "datacite"), ("crosscite", "datacite"), ("medra", "medra"), ("jalc", "jalc"), ("kisti", "kisti")] {
        if host.contains(k) {
            return name.into();
        }
    }
    if host.is_empty() || host == "doi.org" { "doi.org".into() } else { host }
}

/// CSL 字段里带的 HTML / JATS 标签（`<i>`、`<jats:p>`）去掉，空白并成一个
fn plain(s: &str) -> String {
    text::html_to_text(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 摘要：去标签，保留段落；开头单独一行的「Abstract」去掉
fn abstract_text(s: &str) -> String {
    // JATS 标签带命名空间（<jats:p>），去掉前缀才认得出段落；小标题后换行
    let s = s.replace("<jats:", "<").replace("</jats:", "</").replace("</title>", "</title><br>");
    let t = text::html_to_text(&s);
    let mut lines: Vec<String> = t.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    if lines.first().is_some_and(|l| ["abstract", "summary", "摘要"].contains(&l.to_lowercase().trim_end_matches([':', '：', '.']))) {
        lines.remove(0);
    }
    lines.join("\n")
}

/// 从记录里取的字段（Zotero 名）。其余（url、引用数、许可、`csl:` 原样字段……）不要。
const TAKE: &[&str] = &[
    "title", "journalAbbreviation", "volume", "issue", "pages", "publisher", "university", "institution", "place", "ISSN", "ISBN", "DOI",
    "edition", "numPages", "series", "seriesNumber", "abstractNote", "language", "conferenceName", "publicationTitle", "bookTitle",
    "proceedingsTitle", "websiteTitle", "encyclopediaTitle", "dictionaryTitle", "thesisType", "reportType", "reportNumber",
];

fn from_csl(r: Record, source: &str) -> Found {
    let mut fields = BTreeMap::new();
    for k in TAKE {
        if let Some(v) = r.fields.get(*k) {
            let v = if *k == "abstractNote" { abstract_text(v) } else { plain(v) };
            if !v.is_empty() {
                fields.insert(k.to_string(), v);
            }
        }
    }
    // 副标题接在标题后（与 Zotero 的 Crossref 转换器一致）
    // CSL 的 subtitle 是数组，解析器把它原样存成 JSON 文本
    let subtitle = r.fields.get("csl:subtitle").map(|s| match serde_json::from_str::<Vec<String>>(s) {
        Ok(v) => plain(v.first().map(String::as_str).unwrap_or("")),
        Err(_) => plain(s),
    });
    if let (Some(t), Some(sub)) = (fields.get("title").cloned(), subtitle)
        && !sub.is_empty()
        && !text::title_norm(&t).contains(&text::title_norm(&sub))
    {
        fields.insert("title".into(), format!("{t}: {sub}"));
    }
    if let Some(d) = &r.date {
        let raw = match d.year {
            Some(y) => match (d.month, d.day) {
                (Some(m), Some(day)) => format!("{y:04}-{m:02}-{day:02}"),
                (Some(m), None) => format!("{y:04}-{m:02}"),
                _ => format!("{y:04}"),
            },
            None => d.raw.clone(),
        };
        fields.insert("date".into(), raw);
    }
    // 线上 / 印刷 / 出版日期（比较年份用：期刊常常线上先发、印刷晚一年）
    let mut date_alternatives = Vec::new();
    for k in ["csl:published-print", "csl:published-online", "csl:published"] {
        if let Some(parts) = r.fields.get(k).and_then(|raw| serde_json::from_str::<Value>(raw).ok()).and_then(|v| v.pointer("/date-parts/0").cloned()) {
            let n = |i: usize| parts.get(i).and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
            if let Some(y) = n(0) {
                let iso = match (n(1), n(2)) {
                    (Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
                    (Some(m), None) => format!("{y:04}-{m:02}"),
                    _ => format!("{y:04}"),
                };
                if !date_alternatives.contains(&iso) {
                    date_alternatives.push(iso);
                }
            }
        }
    }
    let creators = r
        .creators
        .into_iter()
        .map(|c| FoundCreator { role: c.role, family: c.family.map(|s| plain(&s)), given: c.given.map(|s| plain(&s)), literal: c.literal.map(|s| plain(&s)) })
        .collect();
    Found { source: source.to_string(), item_type: r.item_type, fields, creators, date_alternatives }
}

fn lookup_isbn(http: &dyn Http, pacer: &mut Pacer, isbn: &str) -> Result<Outcome, NetError> {
    let url = format!("https://openlibrary.org/api/books?bibkeys=ISBN:{isbn}&format=json&jscmd=data");
    let r = get_polite(http, pacer, "openlibrary.org", Duration::from_secs(1), &url, "application/json", false)?;
    if r.status != 200 {
        return Err(NetError::Other(format!("Open Library 返回错误（{}）", r.status)));
    }
    let v: Value = serde_json::from_str(&r.body).map_err(|_| NetError::Other("Open Library 的返回看不懂".into()))?;
    match v.as_object().and_then(|o| o.values().next()).and_then(Value::as_object) {
        Some(book) => Ok(Outcome::Found(from_open_library(book))),
        None => Ok(Outcome::NotFound("Open Library 没有这本书".into())),
    }
}

fn names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).map(plain).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// 「Jane Doe」→ 姓 Doe、名 Jane；「Doe, Jane」同；含中日文的不拆（单栏）
pub fn split_name(name: &str) -> FoundCreator {
    let name = name.trim();
    let author = |family: Option<&str>, given: Option<&str>, literal: Option<&str>| FoundCreator {
        role: "author".into(),
        family: family.map(str::to_string),
        given: given.map(str::to_string),
        literal: literal.map(str::to_string),
    };
    if text::detect_lang(name).is_some_and(|l| l != "en") {
        return author(None, None, Some(name));
    }
    if let Some((f, g)) = name.split_once(',') {
        return author(Some(f.trim()), Some(g.trim()).filter(|g| !g.is_empty()), None);
    }
    match name.rsplit_once(' ') {
        Some((g, f)) => author(Some(f.trim()), Some(g.trim()), None),
        None => author(None, None, Some(name)),
    }
}

fn from_open_library(o: &serde_json::Map<String, Value>) -> Found {
    let mut fields = BTreeMap::new();
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(plain).filter(|s| !s.is_empty());
    if let Some(t) = s("title") {
        let t = match s("subtitle") {
            Some(sub) => format!("{t}: {sub}"),
            None => t,
        };
        fields.insert("title".into(), t);
    }
    if let Some(p) = names(o.get("publishers")).into_iter().next() {
        fields.insert("publisher".into(), p);
    }
    if let Some(p) = names(o.get("publish_places")).into_iter().next() {
        fields.insert("place".into(), p);
    }
    if let Some(d) = s("publish_date") {
        fields.insert("date".into(), d);
    }
    if let Some(n) = o.get("number_of_pages").and_then(Value::as_u64) {
        fields.insert("numPages".into(), n.to_string());
    }
    let creators = names(o.get("authors")).iter().map(|n| split_name(n)).collect();
    Found { source: "openlibrary".into(), item_type: "book".into(), fields, creators, date_alternatives: Vec::new() }
}
