//! Mathpix `v3/pdf`（`decisions/0024`「PDF」）：上传 → 查 `status` 直到 `completed` → 下载 `lines.json` 和 `mmd` → `DELETE`
//! 删掉 Mathpix 上的副本。由行拼段落在 `concept_engine::mathpix`。
//!
//! - 头：`app_id`、`app_key`（存成一个条目 `app_id:app_key`）；上传是 multipart（`file` + `options_json`）；
//! - 费用：每页 $0.005，上传前按页数查月上限（和别的接口共用），做完按 Mathpix 说的页数记账（没说就按本机数的页数），
//!   上传后失败的也按页数记（宁多勿少）；
//! - 查状态每 2 秒一次，最多 10 分钟，可以取消；取消或出错后照样尽量 `DELETE`。
//!
//! 接口细节照录制样例写，C4 用真实 PDF 核对（列在 `HANDOFF-cloud.md`）。

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use research_engine::ports::{ErrorKind, PortError};
use serde_json::{Value, json};

use crate::agent::usage::Tokens;
use crate::research::meter::{self, Meter};

pub const PROVIDER: &str = "mathpix";
pub const DEFAULT_BASE: &str = "https://api.mathpix.com";
pub const PAGE_USD: f64 = 0.005;
pub const KIND_CONVERT: &str = "convert";

pub struct Mathpix {
    agent: ureq::Agent,
    base: String,
    app_id: String,
    app_key: String,
    meter: Arc<Meter>,
    /// 查状态的间隔（测试里调小）
    pub poll: Duration,
    pub max_wait: Duration,
}

#[derive(Debug)]
pub struct Converted {
    pub pdf_id: String,
    pub lines: Value,
    pub mmd: Option<String>,
    pub pages: u32,
    pub cost_usd: f64,
    /// DELETE 成功了没有（失败只提示，不算整个失败）
    pub deleted: bool,
}

fn error_message(status: u16, message: &str) -> String {
    let detail = meter::detail(message);
    match status {
        401 | 403 => "Mathpix 的 app_id 或 app_key 不对（或没有开通 PDF 接口）。到「设置 › 密钥与费用」重新填写。".into(),
        402 => "Mathpix 账户余额不足。充值后再试。".into(),
        413 => "PDF 太大，Mathpix 不收。".into(),
        429 => "Mathpix 说调用太频繁，被限流了。等一会儿再试。".into(),
        s if s >= 500 => format!("Mathpix 的服务出错（{s}）。等一会儿再试。"),
        s => format!("Mathpix 返回了 {s}{detail}。"),
    }
}

impl Mathpix {
    /// `pair` 是 `app_id:app_key`
    pub fn new(base: Option<&str>, pair: &str, meter: Arc<Meter>) -> Result<Self, String> {
        let (id, key) = pair.split_once(':').ok_or("Mathpix 的密钥要有 app_id 和 app_key 两项。")?;
        let base = base.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(DEFAULT_BASE).trim_end_matches('/').to_string();
        Ok(Mathpix { agent: meter::agent(Duration::from_secs(300)), base, app_id: id.trim().into(), app_key: key.trim().into(), meter, poll: Duration::from_secs(2), max_wait: Duration::from_secs(600) })
    }

    fn get(&self, path: &str) -> Result<ureq::http::Response<ureq::Body>, PortError> {
        self.agent.get(format!("{}{path}", self.base)).header("app_id", &self.app_id).header("app_key", &self.app_key).call().map_err(|e| PortError::new(ErrorKind::Failed, format!("连不上 Mathpix（{e}）。检查网络后再试。")))
    }

    fn read(resp: &mut ureq::http::Response<ureq::Body>) -> Result<String, PortError> {
        let mut text = String::new();
        resp.body_mut().as_reader().take(64 * 1024 * 1024).read_to_string(&mut text).map_err(|e| PortError::new(ErrorKind::Failed, format!("接收 Mathpix 的结果时连接断了：{e}")))?;
        Ok(text)
    }

    fn record(&self, pages: u32, estimated: bool, note: Option<String>, pdf_id: &str) -> Result<f64, PortError> {
        let cost = pages as f64 * PAGE_USD;
        self.meter.record(self.meter.call("concept_pdf", PROVIDER, KIND_CONVERT, "v3/pdf", Tokens::default(), pages as u64, cost, estimated, note, Some(pdf_id.to_string())))?;
        Ok(cost)
    }

    fn delete(&self, pdf_id: &str) -> bool {
        self.agent.delete(format!("{}/v3/pdf/{pdf_id}", self.base)).header("app_id", &self.app_id).header("app_key", &self.app_key).call().is_ok_and(|r| r.status().is_success())
    }

    /// 转一份 PDF。`pages` 是本机数的页数（估费用、Mathpix 没说页数时记账用）
    pub fn convert(&self, pdf: &[u8], file_name: &str, pages: u32, cancel: &AtomicBool, progress: &dyn Fn(&str)) -> Result<Converted, PortError> {
        self.meter.check(pages as f64 * PAGE_USD, &format!("这份 PDF（{pages} 页）"))?;
        // 上传
        let boundary = format!("pith-{:016x}", pdf.len() as u64 ^ 0x5eed_1234_abcd);
        let options = json!({ "conversion_formats": {}, "rm_spaces": true, "include_line_data": true });
        let mut body = Vec::with_capacity(pdf.len() + 512);
        let name = file_name.replace(['"', '\r', '\n'], "_");
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
        body.extend_from_slice(pdf);
        body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"options_json\"\r\n\r\n{options}\r\n--{boundary}--\r\n").as_bytes());
        progress("上传 PDF…");
        let mut resp = self
            .agent
            .post(format!("{}/v3/pdf", self.base))
            .header("app_id", &self.app_id)
            .header("app_key", &self.app_key)
            .header("content-type", &format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..])
            .map_err(|e| PortError::new(ErrorKind::Failed, format!("连不上 Mathpix（{e}）。检查网络后再试；这次没有产生费用。")))?;
        let status = resp.status().as_u16();
        let text = Self::read(&mut resp)?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status != 200 {
            let kind = if matches!(status, 401..=403) { ErrorKind::Fatal } else { ErrorKind::Failed };
            return Err(PortError::new(kind, format!("{}这次没有产生费用。", error_message(status, v["error"].as_str().or_else(|| v.pointer("/error_info/message").and_then(Value::as_str)).unwrap_or("")))));
        }
        let Some(pdf_id) = v["pdf_id"].as_str().map(str::to_string) else {
            return Err(PortError::new(ErrorKind::Failed, format!("Mathpix 没有给 pdf_id{}。", meter::detail(v["error"].as_str().unwrap_or("")))));
        };
        // 从这里起已经开始算钱：出错也按页数记、照样删掉
        let fail = |e: PortError, this: &Self| -> PortError {
            let cost = this.record(pages, true, Some(format!("中途停下：{}", e.message)), &pdf_id).unwrap_or(0.0);
            this.delete(&pdf_id);
            PortError { usage: research_engine::ports::Usage { searches: pages as u64, cost_usd: cost, ..Default::default() }, ..e }
        };
        let started = std::time::Instant::now();
        let mut num_pages = None;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(fail(PortError::new(ErrorKind::Cancelled, "已停止。"), self));
            }
            if started.elapsed() > self.max_wait {
                return Err(fail(PortError::new(ErrorKind::Failed, "Mathpix 十分钟还没转完，先停下。可以稍后重试。"), self));
            }
            let mut r = self.get(&format!("/v3/pdf/{pdf_id}")).map_err(|e| fail(e, self))?;
            let s = Self::read(&mut r).map_err(|e| fail(e, self))?;
            let v: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
            num_pages = v["num_pages"].as_u64().map(|n| n as u32).or(num_pages);
            match v["status"].as_str().unwrap_or("") {
                "completed" => break,
                "error" => return Err(fail(PortError::new(ErrorKind::Failed, format!("Mathpix 转换失败{}。", meter::detail(v["error"].as_str().or_else(|| v.pointer("/error_info/message").and_then(Value::as_str)).unwrap_or("")))), self)),
                other => {
                    let done = v["percent_done"].as_f64().map(|p| format!("（{p:.0}%）")).unwrap_or_default();
                    progress(&format!("Mathpix 在转换{done}：{other}"));
                }
            }
            let mut waited = Duration::ZERO;
            while waited < self.poll {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let step = Duration::from_millis(50).min(self.poll);
                std::thread::sleep(step);
                waited += step;
            }
        }
        progress("下载结果…");
        let mut r = self.get(&format!("/v3/pdf/{pdf_id}.lines.json")).map_err(|e| fail(e, self))?;
        if !r.status().is_success() {
            let s = r.status().as_u16();
            return Err(fail(PortError::new(ErrorKind::Failed, format!("下载 Mathpix 的 lines.json 失败（{s}）。")), self));
        }
        let lines: Value = serde_json::from_str(&Self::read(&mut r).map_err(|e| fail(e, self))?).map_err(|e| fail(PortError::new(ErrorKind::Failed, format!("Mathpix 的 lines.json 读不懂：{e}")), self))?;
        let mmd = match self.get(&format!("/v3/pdf/{pdf_id}.mmd")) {
            Ok(mut r) if r.status().is_success() => Self::read(&mut r).ok(),
            _ => None,
        };
        let pages_charged = num_pages.or_else(|| lines["pages"].as_array().map(|p| p.len() as u32)).unwrap_or(pages);
        let deleted = self.delete(&pdf_id);
        let cost = self.record(pages_charged, false, (!deleted).then(|| "Mathpix 上的副本没删掉".to_string()), &pdf_id)?;
        Ok(Converted { pdf_id, lines, mmd, pages: pages_charged, cost_usd: cost, deleted })
    }
}

/// 本机数 PDF 页数、按页取文字（Mathpix 前的确认框、没有 Mathpix 密钥时的后备）。和 v0.3 `research/content.rs` 用同一个库（pdf-extract）
pub fn local_pages(pdf: &[u8]) -> Result<Vec<String>, String> {
    let result = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(pdf));
    match result {
        Ok(Ok(pages)) => Ok(pages),
        Ok(Err(e)) => Err(format!("PDF 读不了：{e}")),
        Err(_) => Err("PDF 读不了：文件的结构不规范".into()),
    }
}
