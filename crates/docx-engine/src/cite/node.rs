//! 在 Node 里跑前端的引文排版器（`src/cite/engine.ts`，citeproc-js）：命令行与真实数据测试用，
//! 不用开 App。App 里排版走 WebView（`decisions/0016`），两边是同一份代码。
//! 只认内置样式。要求本机有 Node 22.18+（直接跑 .ts）和仓库的 node_modules。

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::plan::{Render, RenderRequest, Rendered};

pub struct NodeRender {
    /// `scripts/cite-render.ts`
    pub script: PathBuf,
}

impl NodeRender {
    /// 仓库里的脚本（由本 crate 的位置推出）
    pub fn in_repo() -> Self {
        NodeRender { script: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cite-render.ts") }
    }
}

impl Render for NodeRender {
    fn render(&self, req: &RenderRequest) -> Result<Rendered, String> {
        let mut child = Command::new("node")
            .arg(&self.script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法运行 node：{e}"))?;
        let body = serde_json::to_vec(req).map_err(|e| e.to_string())?;
        child.stdin.take().ok_or("node 没有标准输入")?.write_all(&body).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("排版失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("排版结果读不懂：{e}"))
    }
}
