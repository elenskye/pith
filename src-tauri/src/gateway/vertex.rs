//! Vertex AI（照 Peck）：项目 ID、区域存在设置里；认证用 gcloud 的应用默认凭据（ADC）。
//! Pith 不读 ADC 文件的内容，只看它在不在；令牌由 `gcloud auth application-default print-access-token` 换来，
//! 一小时过期，这里 45 分钟换一次，被拒（401）就扔掉、下次重换。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{ErrorKind, GateError};

pub const PROJECT_KEY: &str = "model.vertex.project";
pub const LOCATION_KEY: &str = "model.vertex.location";
pub const DEFAULT_LOCATION: &str = "global";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexConfig {
    pub project: String,
    pub location: String,
}

impl VertexConfig {
    pub fn is_set(&self) -> bool {
        !self.project.trim().is_empty()
    }

    /// 项目 ID：6–30 个小写字母、数字、连字符，字母开头；区域：global 或 us-central1 这类
    pub fn check(&self) -> Result<(), String> {
        let p = self.project.trim();
        let ok_project = p.is_empty() || ((6..=30).contains(&p.len()) && p.starts_with(|c: char| c.is_ascii_lowercase()) && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        if !ok_project {
            return Err("项目 ID 不对：6 到 30 个小写字母、数字或连字符，以字母开头（在 Google Cloud 控制台的项目选择器里能看到）。".into());
        }
        let l = self.location.trim();
        if !(l.is_empty() || (l.len() <= 30 && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))) {
            return Err("区域不对：写 global，或 us-central1、europe-west4 这类。".into());
        }
        Ok(())
    }

    fn location(&self) -> &str {
        let l = self.location.trim();
        if l.is_empty() { DEFAULT_LOCATION } else { l }
    }

    fn host(&self) -> String {
        match self.location() {
            "global" => "https://aiplatform.googleapis.com".into(),
            l => format!("https://{l}-aiplatform.googleapis.com"),
        }
    }

    /// Vertex 上的 Gemini（流式）
    pub fn gemini_url(&self, model: &str) -> String {
        format!("{}/v1/projects/{}/locations/{}/publishers/google/models/{model}:streamGenerateContent?alt=sse", self.host(), self.project.trim(), self.location())
    }
}

/// gcloud 生成的 ADC 文件在不在（只看文件，不读内容）
pub fn adc_present() -> bool {
    adc_path().is_some_and(|p| p.is_file())
}

fn adc_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/gcloud/application_default_credentials.json"))
}

/// 从「应用程序」打开的 App 的 PATH 很短，gcloud 常装在这几处
pub fn find_gcloud() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join("gcloud")).collect()).unwrap_or_default();
    candidates.extend(
        [
            "/opt/homebrew/bin/gcloud",
            "/usr/local/bin/gcloud",
            "/opt/homebrew/share/google-cloud-sdk/bin/gcloud",
            "/opt/homebrew/Caskroom/google-cloud-sdk/latest/google-cloud-sdk/bin/gcloud",
            "/usr/local/Caskroom/google-cloud-sdk/latest/google-cloud-sdk/bin/gcloud",
        ]
        .map(PathBuf::from),
    );
    candidates.push(home.join("google-cloud-sdk/bin/gcloud"));
    candidates.into_iter().find(|p| p.is_file())
}

/// 整个 App 共用一个令牌
#[derive(Default)]
pub struct AdcToken {
    cached: Mutex<Option<(String, Instant)>>,
}

const TOKEN_LIFE: Duration = Duration::from_secs(45 * 60);

impl AdcToken {
    pub fn get(&self) -> Result<String, GateError> {
        if let Ok(c) = self.cached.lock()
            && let Some((t, at)) = c.as_ref()
            && at.elapsed() < TOKEN_LIFE
        {
            return Ok(t.clone());
        }
        let token = fetch()?;
        if let Ok(mut c) = self.cached.lock() {
            *c = Some((token.clone(), Instant::now()));
        }
        Ok(token)
    }

    pub fn forget(&self) {
        if let Ok(mut c) = self.cached.lock() {
            *c = None;
        }
    }
}

pub fn token() -> &'static AdcToken {
    static T: std::sync::OnceLock<AdcToken> = std::sync::OnceLock::new();
    T.get_or_init(AdcToken::default)
}

const LOGIN_HINT: &str = "先在终端里跑一次 gcloud auth application-default login，登录以后再试。";

fn fetch() -> Result<String, GateError> {
    if !adc_present() {
        return Err(GateError::new(ErrorKind::Auth, format!("还没有 gcloud 的应用默认凭据。{LOGIN_HINT}")));
    }
    let gcloud = find_gcloud().ok_or_else(|| GateError::new(ErrorKind::Auth, "找不到 gcloud。装好 Google Cloud CLI（brew install --cask google-cloud-sdk）以后再试。"))?;
    let mut child = std::process::Command::new(gcloud)
        .args(["auth", "application-default", "print-access-token"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| GateError::new(ErrorKind::Auth, format!("运行 gcloud 失败：{e}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > Duration::from_secs(20) => {
                let _ = child.kill();
                return Err(GateError::new(ErrorKind::Auth, "gcloud 换令牌超时了。"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(GateError::new(ErrorKind::Auth, format!("gcloud 出错：{e}"))),
        }
    }
    let out = child.wait_with_output().map_err(|e| GateError::new(ErrorKind::Auth, format!("gcloud 出错：{e}")))?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() || token.contains(char::is_whitespace) {
        return Err(GateError::new(ErrorKind::Auth, format!("gcloud 换不到令牌。{LOGIN_HINT}")));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_checks() {
        let g = VertexConfig { project: "my-project-123".into(), location: "global".into() };
        assert_eq!(g.gemini_url("gemini-4-carbon"), "https://aiplatform.googleapis.com/v1/projects/my-project-123/locations/global/publishers/google/models/gemini-4-carbon:streamGenerateContent?alt=sse");
        let r = VertexConfig { project: "my-project-123".into(), location: "us-central1".into() };
        assert!(r.gemini_url("gemini-3.8-flash").starts_with("https://us-central1-aiplatform.googleapis.com/v1/projects/my-project-123/locations/us-central1/"));
        // 区域空着就是 global
        assert!(VertexConfig { project: "abcdef".into(), location: String::new() }.gemini_url("m").contains("/locations/global/"));
        assert!(g.check().is_ok() && g.is_set());
        assert!(VertexConfig { project: "Bad_Project".into(), location: "global".into() }.check().is_err());
        assert!(VertexConfig { project: "my-project-123".into(), location: "us central".into() }.check().is_err());
        assert!(!VertexConfig::default().is_set());
    }
}
