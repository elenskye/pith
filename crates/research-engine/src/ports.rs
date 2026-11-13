//! 三类对外接口：模型、搜索、下载（`decisions/0020`「接口抽象」）。
//! 这里只有 trait 和它们交换的数据；真实实现（Anthropic、OpenAI 兼容、Tavily、本机下载）在 `src-tauri`，
//! 模拟实现在 `fixture`。预算检查和记账由真实实现自己做，把这次的费用放在回复里带回来。

use std::sync::atomic::AtomicBool;

use serde_json::Value;

use crate::model::{Hit, ModelRef, Run, Source, Stage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Plan,
    Extract,
    Conclude,
}

impl Purpose {
    /// 记进账本的用途名
    pub fn key(self) -> &'static str {
        match self {
            Purpose::Plan => "research_plan",
            Purpose::Extract => "research_extract",
            Purpose::Conclude => "research_conclude",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub purpose: Purpose,
    pub system: String,
    /// 用户消息的各段文字
    pub user: Vec<String>,
    /// 输出的 JSON Schema：接口支持结构化输出就用它约束；不支持的靠提示词里的说明
    pub schema: Value,
    pub max_tokens: u32,
    /// 通常的输出量（估费用用）
    pub typical_output_tokens: u32,
    /// 允许模型用自带的网页搜索，最多几次；0 = 不搜
    pub web_searches: u32,
    /// 这次调用是为哪个对象做的（来源编号、第几段）。模拟模型按它挑回复，真实接口不用
    pub tag: String,
}

impl ModelRequest {
    /// 发出去的文字（估 token 用）
    pub fn sent_chars(&self) -> usize {
        self.system.chars().count() + self.user.iter().map(|u| u.chars().count()).sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 搜索次数；Tavily 是点数
    pub searches: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelReply {
    pub text: String,
    /// 接口说实际回答的模型
    pub model: String,
    pub usage: Usage,
    /// 模型自带的网页搜索看到的结果（计划时摸底）
    pub hits: Vec<Hit>,
    pub searched: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// 用户取消了
    Cancelled,
    /// 会超过月度上限，没有发送
    Budget,
    /// 密钥不对、余额不足这类：重试也没用，整个调查停下
    Fatal,
    /// 这一次失败了（限流、超时、模型拒答、输出被截断……）：别的调用可以照常
    Failed,
}

/// 调用失败。给用户看的中文在 `message`；失败前已经产生的费用在 `usage`
#[derive(Clone, Debug, PartialEq)]
pub struct PortError {
    pub kind: ErrorKind,
    pub message: String,
    pub usage: Usage,
}

impl PortError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        PortError { kind, message: message.into(), usage: Usage::default() }
    }

    pub fn cancelled() -> Self {
        PortError::new(ErrorKind::Cancelled, "已停止。")
    }

    /// 整个调查要不要停下
    pub fn stops_the_run(&self) -> bool {
        matches!(self.kind, ErrorKind::Cancelled | ErrorKind::Budget | ErrorKind::Fatal)
    }
}

pub trait Model: Send + Sync {
    fn info(&self) -> ModelRef;
    fn is_mock(&self) -> bool {
        false
    }
    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchReply {
    pub hits: Vec<Hit>,
    pub usage: Usage,
}

pub trait Search: Send + Sync {
    /// 引擎名：tavily / claude_web_search / mock
    fn engine(&self) -> String;
    fn is_mock(&self) -> bool {
        false
    }
    fn search(&self, query: &str, max_results: usize, cancel: &AtomicBool) -> Result<SearchReply, PortError>;
}

/// 下载并保存好的一份来源
#[derive(Clone, Debug, PartialEq)]
pub struct Fetched {
    pub final_url: String,
    pub fetched_at: String,
    pub sha256: String,
    /// html / pdf / xlsx / csv / text
    pub kind: String,
    pub content_type: Option<String>,
    pub ext: String,
    pub bytes: u64,
    /// 从文件里读到的标题
    pub title: Option<String>,
    /// 正文；取不出文字时是原因
    pub text: Result<String, String>,
    /// 不是本机直接下载到的：本机下载失败后改用 Claude 网页抓取（`decisions/0023`）。这一次调用的费用也在这里
    pub via: Option<Via>,
}

/// 来源是怎么拿到的（本机下载以外的办法）
#[derive(Clone, Debug, PartialEq)]
pub struct Via {
    /// claude_web_fetch
    pub how: String,
    /// 本机下载失败的原因
    pub download_error: String,
    pub provider: String,
    pub model: String,
    pub usage: Usage,
}

pub trait Fetch: Send + Sync {
    /// 下载一个网址（或读入用户给的本机文件），存原件和正文。失败返回原因。
    fn fetch(&self, url: &str, cancel: &AtomicBool) -> Result<Fetched, String>;
    /// 读回已经保存的正文（接着做、核对、重新审计时用）
    fn text(&self, source: &Source) -> Result<String, String>;
}

/// 每做完一步存一次
pub trait Recorder {
    fn save(&mut self, run: &Run) -> Result<(), String>;
}

/// 不存（测试、一次性运行）
pub struct NoRecorder;

impl Recorder for NoRecorder {
    fn save(&mut self, _: &Run) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
    pub message: String,
}

/// 一次调查要用到的全部接口
pub struct Ports<'a> {
    pub planner: &'a dyn Model,
    pub worker: &'a dyn Model,
    pub search: &'a dyn Search,
    pub fetch: &'a dyn Fetch,
    pub recorder: &'a mut dyn Recorder,
    pub now: &'a dyn Fn() -> String,
    pub progress: &'a dyn Fn(Progress),
    pub cancel: &'a AtomicBool,
}
