//! 模型网关（1.1.0 第 1 期，`decisions/0036`）：从 Peck 的网关抽出来、改成同步（ureq + 线程），对话 Agent 只经它调模型。
//!
//! - 中立的消息和片段（`Message`、`Part`）：各家的编码器把它变成自己的 body，流解析把回答变回它。
//!   思考、Gemini 的签名、各家私有的块挂在 `Opaque` 上，只回放给同一来源（换了模型整块丢掉）。
//! - 四条线协议：Anthropic Messages（`anthropic`）、OpenAI Responses（`openai`）、DeepSeek Chat Completions（`deepseek`）、
//!   Gemini（`gemini`，API Key 和 Vertex AI 共用）。地址、认证、重试在 `http`；Vertex 的项目和 ADC 令牌在 `vertex`。
//! - 每家三档（便宜、均衡、强），价格表、模型清单在 `catalog`；记账、月上限在 `meter`（接现有的账本 `agent/usage.jsonl`）。
//! - 模拟模型（`mock`）：脚本化，测试和开发版用，不联网、不花钱。
//! - 密钥（`keys`）：沿用 `agent::secret` 的钥匙串 / 内存 / 密钥文件，加 Google 一把。

pub mod anthropic;
pub mod catalog;
pub mod commands;
pub mod deepseek;
pub mod gemini;
pub mod http;
pub mod keys;
pub mod meter;
pub mod mock;
pub mod openai;
pub mod vertex;

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::agent::usage::Tokens;

// ---------- 供应商 ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Deepseek,
    /// Gemini（API Key）
    Google,
    /// Vertex AI 上的 Gemini（gcloud 的 ADC）
    Vertex,
}

impl Provider {
    pub const ALL: [Provider; 5] = [Provider::Anthropic, Provider::Openai, Provider::Deepseek, Provider::Google, Provider::Vertex];

    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openai => "openai",
            Provider::Deepseek => "deepseek",
            Provider::Google => "google",
            Provider::Vertex => "vertex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::Openai => "OpenAI",
            Provider::Deepseek => "DeepSeek",
            Provider::Google => "Google Gemini",
            Provider::Vertex => "Vertex AI",
        }
    }

    pub fn wire(self) -> Wire {
        match self {
            Provider::Anthropic => Wire::Messages,
            Provider::Openai => Wire::Responses,
            Provider::Deepseek => Wire::Chat,
            Provider::Google | Provider::Vertex => Wire::Gemini,
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Messages,
    Responses,
    Chat,
    Gemini,
}

/// 回放来源：哪一家、哪个模型（发给供应商的名字）。思考和私有块只回放给一模一样的来源。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub provider: Provider,
    pub model: String,
}

// ---------- 中立的消息 ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Message {
        Message { role: Role::User, parts: vec![Part::Text { text: text.into() }] }
    }

    /// 文字部分连起来（思考、工具调用不算）
    pub fn text(&self) -> String {
        self.parts.iter().filter_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("")
    }

    pub fn tool_calls(&self) -> Vec<(&str, &str, &Value)> {
        self.parts.iter().filter_map(|p| if let Part::ToolCall { call_id, name, input, .. } = p { Some((call_id.as_str(), name.as_str(), input)) } else { None }).collect()
    }
}

/// 私有数据：思考的签名和密文、Gemini 的 thought signature、Anthropic 回退的块……原样存、只回放给同一来源。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opaque {
    pub origin: Origin,
    pub data: Value,
}

impl Opaque {
    pub fn for_origin(&self, target: &Origin) -> Option<&Value> {
        (self.origin == *target).then_some(&self.data)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Part {
    Text {
        text: String,
    },
    /// 思考（摘要）。`opaque` 是回放要的私有数据
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opaque: Option<Opaque>,
    },
    /// 模型要调用的工具。`signature`：Gemini 挂在调用上的 thought signature
    ToolCall {
        call_id: String,
        name: String,
        input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<Opaque>,
    },
    ToolResult {
        call_id: String,
        output: String,
        #[serde(default)]
        is_error: bool,
    },
    /// 某一家自己的块（Anthropic 的 fallback 块这类）：只回放给同一来源
    Native {
        opaque: Opaque,
    },
}

/// 工具参数不是完整的 JSON（被截断）时，`input` 是这个形状；循环据此回一个出错的结果
pub const BAD_INPUT_KEY: &str = "__unparsed";

pub fn parse_tool_input(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str::<Value>(raw).ok().filter(Value::is_object).unwrap_or_else(|| serde_json::json!({ BAD_INPUT_KEY: raw.chars().take(400).collect::<String>() }))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemBlock {
    pub text: String,
    /// 在这一块后面打缓存断点（只有要显式打的 Anthropic 用得上）
    pub cache: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub system: Vec<SystemBlock>,
    pub tools: Vec<ToolSpec>,
    pub messages: Vec<Message>,
    pub max_output_tokens: u32,
    /// 思考强度（low / medium / high……），取值看这个模型的 `efforts`；None：这一档不开思考（能关的才关）
    pub effort: Option<String>,
    /// 这一轮不许调工具（收尾写报告的那一轮）：工具定义照旧放着，缓存不断
    pub no_tools: bool,
    /// OpenAI 的 `prompt_cache_key`：同一段对话的请求落在同一台机器上，缓存命中高
    pub cache_key: Option<String>,
}

impl Request {
    /// 发送前估计的输入 token 数（不联网，偏高）
    pub fn input_estimate(&self) -> u64 {
        let est = crate::agent::usage::estimate_tokens;
        let sys: u64 = self.system.iter().map(|b| est(&b.text)).sum();
        let tools: u64 = self.tools.iter().map(|t| est(&t.description) + est(&t.input_schema.to_string()) + 8).sum();
        let msgs: u64 = self
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .map(|p| match p {
                Part::Text { text } | Part::Reasoning { text, .. } => est(text),
                Part::ToolCall { input, .. } => est(&input.to_string()) + 8,
                Part::ToolResult { output, .. } => est(output) + 8,
                Part::Native { opaque } => est(&opaque.data.to_string()) / 2,
            })
            .sum();
        sys + tools + msgs + 16
    }
}

// ---------- 回答 ----------

/// 流式增量（界面上的状态行、一个字一个字出来的正文）
#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    Text(String),
    Reasoning(String),
    ToolCall { call_id: String, name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal,
    /// Anthropic 的 pause_turn：原样再发一次，它接着做
    Paused,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub message: Message,
    pub stop: Stop,
    /// input：没命中缓存的输入；cache_read / cache_write 另记
    pub tokens: Tokens,
    /// 供应商说实际回答的模型（Anthropic 回退时会变）
    pub model: String,
    pub request_id: Option<String>,
    /// 拒答的原因（Anthropic 的 stop_details.category 这类）
    pub refusal: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 没填密钥、密钥不对、Vertex 没登录
    Auth,
    /// 超过月上限，没发
    Budget,
    /// 连不上、断了
    Network,
    /// 限流、过载（重试完了还是这样）
    Busy,
    /// 供应商报错、回了读不懂的东西
    Upstream,
    /// 请求本身不对（400、404）
    BadRequest,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GateError {
    pub kind: ErrorKind,
    /// 给站主看的中文
    pub message: String,
    /// 已经花掉的（流到一半断了、停了：按估计，往高里算）；没发出去是 None
    pub spent: Option<Tokens>,
}

impl GateError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> GateError {
        GateError { kind, message: message.into(), spent: None }
    }

    pub fn cancelled() -> GateError {
        GateError::new(ErrorKind::Cancelled, "已停止。")
    }
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// 一个能发请求的模型（真接口或模拟的）。`send` 阻塞到回答收完；`cancel` 置位就尽快返回 `Cancelled`。
pub trait Model: Send + Sync {
    fn spec(&self) -> &'static catalog::ModelSpec;
    fn is_mock(&self) -> bool {
        false
    }
    fn send(&self, req: &Request, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, GateError>;
}

/// 一条线协议的流：一条一条喂 SSE 的 `data:` JSON，`finish` 拿整条回答。
pub trait WireStream {
    fn feed(&mut self, data: &Value) -> Vec<Delta>;
    /// 流里说结束了（不用等连接关）
    fn done(&self) -> bool;
    fn finish(self: Box<Self>) -> Result<Reply, String>;
    /// 已经收到的大概输出字数（中途断了按它估计花费）
    fn chars(&self) -> usize;
}

/// 一段话截短（报错里只留前面）
pub fn short(text: &str, max: usize) -> String {
    let one = text.split_whitespace().filter(|w| !(w.starts_with("sk-") || w.starts_with("AIza") || w.starts_with("tvly-"))).collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max).collect::<String>()) }
}

/// 系统提示的几块连成一段（不分块的几家用）
pub(crate) fn system_text(req: &Request) -> String {
    req.system.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests;
