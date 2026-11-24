//! 本机模型（v0.5）：`~/Documents/LLM` 里的向量模型和重排模型，给语义检索用（以后的内部知识库也用这一套）。
//!
//! - 那个文件夹归用户（和柚儿园共用），Pith **只读**清单 `config/models.json`、只经 `scripts/llmctl.sh` 启停，不改里面任何文件。
//! - 每类用清单里的第一个模型：向量在 `127.0.0.1:8081`（OpenAI 兼容的 `/v1/embeddings`），重排在 `127.0.0.1:8082`（`/v1/rerank`）。
//!   只连本机，不走代理，不花钱。
//! - 用到时才启动（`ensure`）：先看 `/health`，没开就 `llmctl.sh start <key> --by pith`，最多等 45 秒。
//!   只停自己启动的（`llmctl.sh status` 里有一行 `By: pith`）：闲置 15 分钟停，退出 Pith 时停；别人（柚儿园、终端）启动的不碰。
//! - 三种模式：`Real`；`Mock`（测试和 `PITH_LOCAL_MODELS=mock`：按字和相邻两字散列出向量，不起进程、不连网络）；
//!   `Off`（找不到那个文件夹，或 `PITH_LOCAL_MODELS=off`）：语义检索退回按词检索。
//!   文件夹的位置默认 `~/Documents/LLM`，`PITH_LLM_DIR` 可以另指。

pub mod embed;
pub mod rerank;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

pub const LLM_DIR_ENV: &str = "PITH_LLM_DIR";
pub const MODE_ENV: &str = "PITH_LOCAL_MODELS";
/// `llmctl.sh start … --by pith`：标出哪些是 Pith 启动的
const OWNER: &str = "pith";
const SCRIPT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const READY_TIMEOUT: Duration = Duration::from_secs(45);
pub const IDLE: Duration = Duration::from_secs(15 * 60);
const MANIFEST_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Real,
    Mock,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Svc {
    Embedding,
    Rerank,
}

impl Svc {
    pub const ALL: [Svc; 2] = [Svc::Embedding, Svc::Rerank];

    /// llmctl.sh 和清单里的类名
    pub fn arg(self) -> &'static str {
        match self {
            Svc::Embedding => "embedding",
            Svc::Rerank => "rerank",
        }
    }

    pub fn port(self) -> u16 {
        match self {
            Svc::Embedding => 8081,
            Svc::Rerank => 8082,
        }
    }

    pub fn root(self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }

    pub fn label(self) -> &'static str {
        match self {
            Svc::Embedding => "向量模型",
            Svc::Rerank => "重排模型",
        }
    }

    fn idx(self) -> usize {
        self as usize
    }
}

/// 清单里的一个向量 / 重排模型
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceModel {
    /// llmctl.sh 的 key，也是接口里的模型名；向量按它分开存（换了模型，旧向量不拿来比）
    pub key: String,
    pub name: String,
    pub context: i64,
    /// 向量模型：问题怎么写（`{task}` 任务说明、`{text}` 问题）
    pub query: Option<String>,
}

impl ServiceModel {
    /// 检索用的问题按模型的模板写；存进索引的段落原样送
    pub fn question(&self, task: &str, text: &str) -> String {
        match &self.query {
            Some(q) => q.replace("{task}", task).replace("{text}", text),
            None => text.into(),
        }
    }

    fn mock(svc: Svc) -> Self {
        ServiceModel { key: format!("mock-{}", svc.arg()), name: format!("模拟{}", svc.label()), context: 8192, query: None }
    }
}

fn key_ok(key: &str) -> bool {
    !key.is_empty() && key.len() <= 80 && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) && !["chat", "embedding", "rerank"].contains(&key)
}

/// 读清单里某一类的第一个模型
pub fn first_model(manifest: &str, svc: Svc) -> Result<ServiceModel, String> {
    let v: Value = serde_json::from_str(manifest).map_err(|_| "本机模型的清单（config/models.json）读不懂。".to_string())?;
    if v["version"].as_i64() != Some(MANIFEST_VERSION) {
        return Err(format!("本机模型的清单不是第 {MANIFEST_VERSION} 版，Pith 不认识。"));
    }
    let m = v[svc.arg()].as_array().and_then(|a| a.first()).ok_or_else(|| format!("本机模型的清单里没有{}。", svc.label()))?;
    let key = m["key"].as_str().unwrap_or_default();
    if !key_ok(key) {
        return Err(format!("本机模型的清单里，{}的 key 写得不对。", svc.label()));
    }
    Ok(ServiceModel { key: key.into(), name: m["name"].as_str().unwrap_or(key).into(), context: m["context"].as_i64().unwrap_or(0), query: m["query"].as_str().map(str::to_string) })
}

#[derive(Default)]
struct SvcState {
    last_used: Option<Instant>,
    /// 这个服务现在是 Pith 启动的
    ours: bool,
}

pub struct LocalModels {
    mode: Mode,
    root: PathBuf,
    svcs: Mutex<[SvcState; 2]>,
    /// 同一类同时只做一次启动
    starting: [Mutex<()>; 2],
}

/// 给界面看的状态（设置页、提问框）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub mode: Mode,
    pub dir: String,
    pub embedding: Option<ServiceModel>,
    pub rerank: Option<ServiceModel>,
    pub embedding_running: bool,
    pub rerank_running: bool,
    /// 用不了的原因（找不到文件夹、清单有误）
    pub issue: Option<String>,
}

struct ScriptOut {
    code: i32,
    text: String,
}

impl LocalModels {
    pub fn new(mode: Mode, root: PathBuf) -> Self {
        LocalModels { mode, root, svcs: Mutex::new([SvcState::default(), SvcState::default()]), starting: [Mutex::new(()), Mutex::new(())] }
    }

    /// 测试默认模拟；别的时候找到 `llmctl.sh` 就用真的
    pub fn detect() -> Self {
        let root = std::env::var_os(LLM_DIR_ENV).filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Documents/LLM"))).unwrap_or_default();
        let wanted = std::env::var(MODE_ENV).unwrap_or_default();
        let mode = match wanted.as_str() {
            "mock" => Mode::Mock,
            "off" => Mode::Off,
            "real" => Mode::Real,
            _ if cfg!(test) => Mode::Mock,
            _ => Mode::Real,
        };
        let mode = if mode == Mode::Real && !root.join("scripts/llmctl.sh").is_file() { Mode::Off } else { mode };
        LocalModels::new(mode, root)
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_mock(&self) -> bool {
        self.mode == Mode::Mock
    }

    fn llmctl(&self) -> PathBuf {
        self.root.join("scripts/llmctl.sh")
    }

    /// 清单里这一类用哪个模型（不启动）
    pub fn model(&self, svc: Svc) -> Result<ServiceModel, String> {
        match self.mode {
            Mode::Mock => Ok(ServiceModel::mock(svc)),
            Mode::Off => Err(format!("没有找到本机模型（{}）。", self.root.display())),
            Mode::Real => {
                let text = std::fs::read_to_string(self.root.join("config/models.json")).map_err(|_| "读不了本机模型的清单（config/models.json）。".to_string())?;
                first_model(&text, svc)
            }
        }
    }

    pub fn touch(&self, svc: Svc) {
        if let Ok(mut s) = self.svcs.lock() {
            s[svc.idx()].last_used = Some(Instant::now());
        }
    }

    fn set_ours(&self, svc: Svc, ours: bool) {
        if let Ok(mut s) = self.svcs.lock() {
            s[svc.idx()].ours = ours;
        }
    }

    fn agent(timeout: Duration) -> ureq::Agent {
        ureq::Agent::config_builder().proxy(None).timeout_global(Some(timeout)).http_status_as_error(false).max_redirects(0).build().into()
    }

    /// `/health` 是 200，并且（读得到时）`/v1/models` 的第一个就是清单里的模型
    fn serves(&self, svc: Svc, model: &ServiceModel) -> Option<Result<(), String>> {
        let a = Self::agent(Duration::from_secs(2));
        let ok = a.get(format!("{}/health", svc.root())).call().map(|r| r.status().as_u16() == 200).unwrap_or(false);
        if !ok {
            return None;
        }
        let alias = a.get(format!("{}/v1/models", svc.root())).call().ok().and_then(|mut r| r.body_mut().read_to_string().ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["data"][0]["id"].as_str().map(str::to_string));
        match alias {
            Some(a) if a != model.key => Some(Err(format!("{}（127.0.0.1:{}）现在运行的是 {a}，不是清单里的 {}。", svc.label(), svc.port(), model.key))),
            _ => Some(Ok(())),
        }
    }

    fn script(&self, args: &[&str]) -> Result<ScriptOut, String> {
        let out = Command::new("/bin/sh").arg(self.llmctl()).args(args).current_dir(&self.root).env("PATH", SCRIPT_PATH).stdin(Stdio::null()).output().map_err(|_| "没能运行本机模型的控制脚本 llmctl.sh。".to_string())?;
        let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let err = String::from_utf8_lossy(&out.stderr);
        if !err.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(err.trim());
        }
        Ok(ScriptOut { code: out.status.code().unwrap_or(-1), text })
    }

    /// 让这个服务能回答（没开就启动，最多等 45 秒）。返回用的模型
    pub fn ensure(&self, svc: Svc) -> Result<ServiceModel, String> {
        let model = self.model(svc)?;
        self.touch(svc);
        if self.mode == Mode::Mock {
            return Ok(model);
        }
        if let Some(r) = self.serves(svc, &model) {
            return r.map(|_| model);
        }
        let _one = self.starting[svc.idx()].lock().map_err(|_| "内部错误".to_string())?;
        if let Some(r) = self.serves(svc, &model) {
            return r.map(|_| model);
        }
        let out = self.script(&["start", &model.key, "--by", OWNER])?;
        if out.code != 0 {
            let head: String = out.text.chars().take(300).collect();
            return Err(format!("{}没能启动：{head}", svc.label()));
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(r) = self.serves(svc, &model) {
                let ours = self.script(&["status", svc.arg()]).map(|s| s.text.lines().any(|l| l.trim() == format!("By: {OWNER}"))).unwrap_or(false);
                self.set_ours(svc, ours);
                self.touch(svc);
                return r.map(|_| model);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!("{}（127.0.0.1:{}）没有在 45 秒内就绪。", svc.label(), svc.port()))
    }

    /// Pith 启动的、闲置够久的服务
    fn idle(&self, now: Instant) -> Vec<Svc> {
        let Ok(s) = self.svcs.lock() else { return Vec::new() };
        Svc::ALL.into_iter().filter(|svc| s[svc.idx()].ours && s[svc.idx()].last_used.is_none_or(|t| now.saturating_duration_since(t) >= IDLE)).collect()
    }

    pub fn stop_idle(&self) {
        if self.mode != Mode::Real {
            return;
        }
        for svc in self.idle(Instant::now()) {
            if self.script(&["stop", svc.arg()]).is_ok() {
                self.set_ours(svc, false);
            }
        }
    }

    /// 退出 Pith：停掉自己启动的，不等它们结束
    pub fn stop_ours_on_exit(&self) {
        if self.mode != Mode::Real {
            return;
        }
        let ours: Vec<Svc> = self.svcs.lock().map(|s| Svc::ALL.into_iter().filter(|svc| s[svc.idx()].ours).collect()).unwrap_or_default();
        for svc in ours {
            let _ = Command::new("/bin/sh").arg(self.llmctl()).args(["stop", svc.arg()]).current_dir(&self.root).env("PATH", SCRIPT_PATH).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        }
    }

    pub fn status(&self) -> LocalStatus {
        let (e, r) = (self.model(Svc::Embedding), self.model(Svc::Rerank));
        let running = |svc: Svc, m: &Result<ServiceModel, String>| match (self.mode, m) {
            (Mode::Real, Ok(m)) => matches!(self.serves(svc, m), Some(Ok(()))),
            (Mode::Mock, _) => true,
            _ => false,
        };
        let issue = match self.mode {
            Mode::Off => Some(format!("没有找到本机模型（{}）。语义检索用不了，按词检索照常。", self.root.display())),
            _ => e.as_ref().err().or(r.as_ref().err()).cloned(),
        };
        LocalStatus { mode: self.mode, dir: self.root.display().to_string(), embedding_running: running(Svc::Embedding, &e), rerank_running: running(Svc::Rerank, &r), embedding: e.ok(), rerank: r.ok(), issue }
    }
}

/// 每分钟看一次有没有闲置够久的服务；`LocalModels` 没了就结束
pub fn watch(models: &Arc<LocalModels>) {
    if models.mode != Mode::Real {
        return;
    }
    let weak: Weak<LocalModels> = Arc::downgrade(models);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            match weak.upgrade() {
                Some(m) => m.stop_idle(),
                None => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{"version":2,"runtime":"runtimes/x","chat":[],
        "embedding":[{"key":"emb-a","name":"Emb A","context":8192,"pooling":"last","query":"Instruct: {task}\nQuery:{text}"},{"key":"emb-b","name":"B","context":512}],
        "rerank":[{"key":"rr-a","name":"RR A","context":8192}]}"#;

    #[test]
    fn manifest_gives_the_first_model_of_a_kind() {
        let e = first_model(MANIFEST, Svc::Embedding).unwrap();
        assert_eq!((e.key.as_str(), e.name.as_str(), e.context), ("emb-a", "Emb A", 8192));
        assert_eq!(e.question("找段落", "分工"), "Instruct: 找段落\nQuery:分工");
        let r = first_model(MANIFEST, Svc::Rerank).unwrap();
        assert_eq!((r.key.as_str(), r.question("x", "分工").as_str()), ("rr-a", "分工"));
    }

    #[test]
    fn broken_manifests_are_refused() {
        assert!(first_model("{", Svc::Embedding).is_err());
        assert!(first_model(r#"{"version":1,"embedding":[{"key":"a"}]}"#, Svc::Embedding).unwrap_err().contains("第 2 版"));
        assert!(first_model(r#"{"version":2,"embedding":[]}"#, Svc::Embedding).unwrap_err().contains("没有向量模型"));
        assert!(first_model(r#"{"version":2,"embedding":[{"key":"a b; rm"}]}"#, Svc::Embedding).is_err());
        assert!(first_model(r#"{"version":2,"rerank":[{"key":"rerank"}]}"#, Svc::Rerank).is_err());
    }

    #[test]
    fn mock_and_off_never_run_anything() {
        let mock = LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"));
        assert_eq!(mock.ensure(Svc::Embedding).unwrap().key, "mock-embedding");
        assert!(mock.status().embedding_running && mock.status().issue.is_none());
        let off = LocalModels::new(Mode::Off, PathBuf::from("/nonexistent"));
        assert!(off.ensure(Svc::Rerank).is_err());
        let st = off.status();
        assert!(!st.embedding_running && st.issue.unwrap().contains("没有找到本机模型"));
        off.stop_idle();
        off.stop_ours_on_exit();
    }

    #[test]
    fn only_idle_services_pith_started_are_stopped() {
        let m = LocalModels::new(Mode::Mock, PathBuf::from("/nonexistent"));
        let now = Instant::now();
        assert!(m.idle(now + IDLE * 2).is_empty(), "不是 Pith 启动的不停");
        m.set_ours(Svc::Embedding, true);
        m.touch(Svc::Embedding);
        assert!(m.idle(now + Duration::from_secs(60)).is_empty(), "刚用过的不停");
        assert_eq!(m.idle(now + IDLE + Duration::from_secs(5)), vec![Svc::Embedding]);
    }
}
