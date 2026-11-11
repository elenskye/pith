//! 引文排版在 WebView 里（citeproc-js，`decisions/0007`）：Rust 发事件请求，前端排完调用 `cite_render_done` 交回。
//! 前端挂好监听后调用 `cite_renderer_ready`；Pith 刚被 Word 在后台拉起时，先等它就绪。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Duration;

use docx_engine::cite::plan::{Render, RenderRequest, Rendered};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "cite-render";
const READY_WAIT: Duration = Duration::from_secs(40);
const RENDER_WAIT: Duration = Duration::from_secs(60);

type Reply = mpsc::Sender<Result<Rendered, String>>;

#[derive(Default)]
pub struct Hub {
    pending: Mutex<HashMap<u64, Reply>>,
    next: AtomicU64,
    ready: Mutex<bool>,
    ready_cv: Condvar,
}

impl Hub {
    pub fn set_ready(&self, ready: bool) {
        if let Ok(mut r) = self.ready.lock() {
            *r = ready;
            self.ready_cv.notify_all();
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.lock().map(|r| *r).unwrap_or(false)
    }

    fn wait_ready(&self, timeout: Duration) -> bool {
        let Ok(guard) = self.ready.lock() else { return false };
        self.ready_cv.wait_timeout_while(guard, timeout, |r| !*r).map(|(r, _)| *r).unwrap_or(false)
    }

    /// 发一个请求（`emit` 负责把编号交给前端），等前端交回结果
    pub fn request(&self, emit: impl FnOnce(u64) -> Result<(), String>, ready_wait: Duration, wait: Duration) -> Result<Rendered, String> {
        if !self.wait_ready(ready_wait) {
            return Err("引文排版器没有就绪（Pith 窗口还没加载完）".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().map_err(|_| "排版请求表异常")?.insert(id, tx);
        let done = emit(id).and_then(|_| rx.recv_timeout(wait).map_err(|_| "引文排版超时".to_string()));
        if let Ok(mut p) = self.pending.lock() {
            p.remove(&id);
        }
        done?
    }

    /// 前端交回结果；不认识的编号（已超时）忽略
    pub fn complete(&self, id: u64, result: Result<Rendered, String>) {
        let tx = self.pending.lock().ok().and_then(|mut p| p.remove(&id));
        if let Some(tx) = tx {
            let _ = tx.send(result);
        }
    }
}

#[derive(Clone, Serialize)]
struct Payload<'a> {
    id: u64,
    request: &'a RenderRequest,
}

pub struct WebRender<'a> {
    pub app: &'a AppHandle,
    pub hub: &'a Hub,
}

impl Render for WebRender<'_> {
    fn render(&self, req: &RenderRequest) -> Result<Rendered, String> {
        self.hub.request(|id| self.app.emit_to("main", EVENT, Payload { id, request: req }).map_err(|e| format!("无法联系排版器：{e}")), READY_WAIT, RENDER_WAIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn waits_for_ready_and_matches_replies() {
        let hub = Arc::new(Hub::default());
        let none = |_| Ok(());
        assert!(hub.request(none, Duration::from_millis(20), Duration::from_millis(20)).unwrap_err().contains("没有就绪"));
        // 就绪后：前端在另一个线程交回结果
        let h = hub.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            h.set_ready(true);
        });
        let h = hub.clone();
        let r = hub.request(
            move |id| {
                std::thread::spawn(move || h.complete(id, Ok(Rendered::default())));
                Ok(())
            },
            Duration::from_secs(5),
            Duration::from_secs(5),
        );
        assert!(r.is_ok());
        // 前端报错、超时、发不出去
        let h = hub.clone();
        let e = hub
            .request(
                move |id| {
                    h.complete(id, Err("样式文件坏了".into()));
                    Ok(())
                },
                Duration::ZERO,
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert_eq!(e, "样式文件坏了");
        assert!(hub.request(|_| Ok(()), Duration::ZERO, Duration::from_millis(20)).unwrap_err().contains("超时"));
        assert!(hub.request(|_| Err("x".into()), Duration::ZERO, Duration::from_millis(20)).is_err());
        assert!(hub.pending.lock().unwrap().is_empty(), "请求表清空");
        hub.complete(999, Ok(Rendered::default()));
    }
}
