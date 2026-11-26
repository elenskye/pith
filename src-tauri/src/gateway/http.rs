//! 真接口：地址、认证、重试、读流。
//!
//! - 可重试的（连不上、408 / 409 / 429 / 5xx / 529）照 2、5、15、30、60 秒重发，有 `retry-after` 听它的（最多 60 秒）；
//!   流开始以后断了不重发（已经在计费），按估计记账、报错。
//! - 读流放在另一条线程里，这边每 0.1 秒看一次「停」：点停立刻返回，读流的线程收到下一行时自己丢掉连接。
//! - 密钥只放在请求头里，不进日志、报错。

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;

use super::catalog::ModelSpec;
use super::vertex::{self, VertexConfig};
use super::{Delta, ErrorKind, GateError, Model, Origin, Provider, Reply, Request, Tokens, Wire, WireStream, anthropic, deepseek, gemini, openai};

pub const WAITS: [u64; 5] = [2, 5, 15, 30, 60];

pub enum Credential {
    Key(String),
    /// Vertex：gcloud 的 ADC 换来的令牌
    Adc,
}

pub struct HttpModel {
    spec: &'static ModelSpec,
    credential: Credential,
    url: String,
    /// Vertex 的配额项目（`x-goog-user-project`）
    quota_project: Option<String>,
    agent: ureq::Agent,
    /// 测试里把重试的等待缩短
    waits: Vec<u64>,
}

impl HttpModel {
    pub fn new(spec: &'static ModelSpec, credential: Credential, vertex: Option<&VertexConfig>) -> Result<HttpModel, GateError> {
        let url = match spec.provider {
            Provider::Anthropic => anthropic::URL.to_string(),
            Provider::Openai => openai::URL.to_string(),
            Provider::Deepseek => deepseek::URL.to_string(),
            Provider::Google => gemini::url(spec.api_model),
            Provider::Vertex => match vertex.filter(|v| v.is_set()) {
                Some(v) => v.gemini_url(spec.api_model),
                None => return Err(GateError::new(ErrorKind::Auth, "还没填 Vertex AI 的项目 ID：在「设置 › 模型与费用」里填。")),
            },
        };
        let quota_project = (spec.provider == Provider::Vertex).then(|| vertex.map(|v| v.project.trim().to_string()).unwrap_or_default());
        Ok(HttpModel { spec, credential, url, quota_project, agent: crate::research::meter::agent(Duration::from_secs(1800)), waits: WAITS.to_vec() })
    }

    /// 测试用：换成本机假服务的地址、不真等
    #[cfg(test)]
    pub fn at(mut self, url: &str) -> HttpModel {
        self.url = url.into();
        self.waits = vec![0; WAITS.len()];
        self
    }

    pub fn origin(&self) -> Origin {
        Origin { provider: self.spec.provider, model: self.spec.api_model.into() }
    }

    /// 发出去的 body（不发请求）
    pub fn body(&self, req: &Request) -> Value {
        let o = self.origin();
        match self.spec.provider.wire() {
            Wire::Messages => anthropic::encode(req, self.spec, &o),
            Wire::Responses => openai::encode(req, self.spec, &o),
            Wire::Chat => deepseek::encode(req, self.spec, &o),
            Wire::Gemini => gemini::encode(req, self.spec, &o),
        }
    }

    fn stream(&self, request_id: Option<String>) -> Box<dyn WireStream> {
        let o = self.origin();
        match self.spec.provider.wire() {
            Wire::Messages => Box::new(anthropic::AnthropicStream::new(o, request_id)),
            Wire::Responses => Box::new(openai::ResponsesStream::new(o, request_id)),
            Wire::Chat => Box::new(deepseek::ChatStream::new(o, request_id)),
            Wire::Gemini => Box::new(gemini::GeminiStream::new(o, request_id)),
        }
    }

    fn headers(&self) -> Result<Vec<(&'static str, String)>, GateError> {
        let mut h = vec![("accept", "text/event-stream".to_string())];
        match (&self.credential, self.spec.provider) {
            (Credential::Key(k), Provider::Anthropic) => {
                h.push(("x-api-key", k.clone()));
                h.push(("anthropic-version", anthropic::API_VERSION.into()));
                if self.spec.fallback {
                    h.push(("anthropic-beta", anthropic::FALLBACK_BETA.into()));
                }
            }
            (Credential::Key(k), Provider::Google) => h.push(("x-goog-api-key", k.clone())),
            (Credential::Key(k), _) => h.push(("authorization", format!("Bearer {k}"))),
            (Credential::Adc, _) => {
                h.push(("authorization", format!("Bearer {}", vertex::token().get()?)));
                if let Some(p) = self.quota_project.as_ref().filter(|p| !p.is_empty()) {
                    h.push(("x-goog-user-project", p.clone()));
                }
            }
        }
        Ok(h)
    }

    fn error_message(&self, status: u16, body: &Value) -> String {
        match self.spec.provider {
            Provider::Anthropic => anthropic::error_message(status, body),
            Provider::Openai => openai::error_message(status, body),
            Provider::Deepseek => deepseek::error_message(status, body),
            Provider::Google => gemini::error_message(status, body, false),
            Provider::Vertex => gemini::error_message(status, body, true),
        }
    }
}

fn retryable(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
}

fn error_kind(status: u16) -> ErrorKind {
    match status {
        401 | 402 | 403 => ErrorKind::Auth,
        429 | 529 => ErrorKind::Busy,
        s if s >= 500 => ErrorKind::Busy,
        _ => ErrorKind::BadRequest,
    }
}

/// 等一会儿（可以被停）
fn wait(seconds: u64, cancel: &AtomicBool) -> Result<(), GateError> {
    for _ in 0..seconds * 10 {
        if cancel.load(Ordering::SeqCst) {
            return Err(GateError::cancelled());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn read_error_body(resp: &mut ureq::http::Response<ureq::Body>) -> Value {
    let mut text = String::new();
    let _ = resp.body_mut().as_reader().take(256 * 1024).read_to_string(&mut text);
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// 中途断了、停了：按估计算已经花掉的（输入按估计，输出按收到的字数，往高里算）
fn spent(input_estimate: u64, chars: usize) -> Tokens {
    Tokens { input: input_estimate, output: chars as u64 * 3 / 2 + 200, ..Default::default() }
}

impl Model for HttpModel {
    fn spec(&self) -> &'static ModelSpec {
        self.spec
    }

    fn send(&self, req: &Request, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, GateError> {
        let body = self.body(req).to_string();
        let input_estimate = req.input_estimate();
        let who = self.spec.provider.label();
        let mut attempt = 0;
        let resp = loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(GateError::cancelled());
            }
            let mut r = self.agent.post(&self.url).header("content-type", "application/json");
            for (k, v) in self.headers()? {
                r = r.header(k, v);
            }
            let sent = r.send(body.as_str());
            let mut resp = match sent {
                Ok(resp) => resp,
                Err(e) => {
                    if attempt < self.waits.len() {
                        wait(self.waits[attempt], cancel)?;
                        attempt += 1;
                        continue;
                    }
                    return Err(GateError::new(ErrorKind::Network, format!("连不上 {who}（{e}）。检查网络后再试，这次没有产生费用。")));
                }
            };
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<u64>().ok());
            let v = read_error_body(&mut resp);
            if status == 401 && matches!(self.credential, Credential::Adc) {
                vertex::token().forget();
            }
            if retryable(status) && attempt < self.waits.len() {
                let w = self.waits[attempt];
                wait(after.map(|a| a.min(60)).filter(|_| w > 0).unwrap_or(w), cancel)?;
                attempt += 1;
                continue;
            }
            return Err(GateError::new(error_kind(status), format!("{}这次没有产生费用。", self.error_message(status, &v))));
        };
        let request_id = ["request-id", "x-request-id"].iter().find_map(|h| resp.headers().get(*h).and_then(|v| v.to_str().ok()).map(String::from));
        let sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|s| s.starts_with("text/event-stream"));
        let mut stream = self.stream(request_id);
        if !sse {
            return Err(GateError { kind: ErrorKind::Upstream, message: format!("{who} 没有按流式回答。"), spent: Some(spent(input_estimate, 0)) });
        }
        // 读流的线程：一行一行送过来；这边停了，它在下一行时丢掉连接
        let (tx, rx) = mpsc::sync_channel::<Result<String, String>>(512);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let reader = resp.into_body().into_reader();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                let msg = line.map_err(|e| e.to_string());
                let failed = msg.is_err();
                if tx.send(msg).is_err() || failed {
                    break;
                }
            }
        });
        let fail = |stream: &dyn WireStream, kind: ErrorKind, message: String| GateError { kind, message, spent: Some(spent(input_estimate, stream.chars())) };
        loop {
            if cancel.load(Ordering::SeqCst) {
                stop.store(true, Ordering::SeqCst);
                return Err(fail(stream.as_ref(), ErrorKind::Cancelled, "已停止。已经生成的部分仍会计费，按估计记进了用量。".into()));
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(line)) => {
                    let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
                    if data == "[DONE]" {
                        break;
                    }
                    let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                    for d in stream.feed(&v) {
                        on(d);
                    }
                    if stream.done() {
                        stop.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Ok(Err(e)) => return Err(fail(stream.as_ref(), ErrorKind::Network, format!("接收回答时连接断了（{e}）。已经生成的部分按估计记进了用量。"))),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let chars = stream.chars();
        stream.finish().map_err(|m| GateError { kind: ErrorKind::Upstream, message: m, spent: Some(spent(input_estimate, chars)) })
    }
}
