//! 模拟模型：不联网、不花钱。测试用脚本（一轮一轮给好回答），开发版和 `?mock` 用一个会看请求的「脑子」
//! （`harness::mock_brain`，先查、再答）。照真接口的样子流出增量、给用量（按字数估），账本记一行「模拟」。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::catalog::ModelSpec;
use super::{Delta, GateError, Message, Model, Part, Reply, Request, Role, Stop, Tokens};

/// 一轮的回答
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub text: String,
    pub reasoning: String,
    /// 工具调用：名字、参数
    pub calls: Vec<(String, Value)>,
    /// 空着：有工具调用就是 ToolUse，否则 EndTurn
    pub stop: Option<Stop>,
    /// 这一轮直接出错
    pub error: Option<GateError>,
}

impl Turn {
    pub fn text(t: impl Into<String>) -> Turn {
        Turn { text: t.into(), ..Default::default() }
    }

    pub fn call(name: &str, input: Value) -> Turn {
        Turn { calls: vec![(name.into(), input)], ..Default::default() }
    }

    pub fn calls(calls: Vec<(&str, Value)>) -> Turn {
        Turn { calls: calls.into_iter().map(|(n, v)| (n.to_string(), v)).collect(), ..Default::default() }
    }
}

pub type Brain = Box<dyn Fn(&Request) -> Turn + Send + Sync>;

pub struct MockModel {
    spec: &'static ModelSpec,
    script: Mutex<VecDeque<Turn>>,
    /// 脚本用完了以后由它出回答
    brain: Option<Brain>,
    /// 每一小段之间停多久（开发版里看得出流式）
    pub delay: Duration,
    /// 收到的请求（测试看发了什么）
    pub seen: Mutex<Vec<Request>>,
}

impl MockModel {
    pub fn scripted(spec: &'static ModelSpec, turns: Vec<Turn>) -> MockModel {
        MockModel { spec, script: Mutex::new(turns.into()), brain: None, delay: Duration::ZERO, seen: Mutex::new(Vec::new()) }
    }

    pub fn with_brain(spec: &'static ModelSpec, brain: Brain) -> MockModel {
        MockModel { spec, script: Mutex::new(VecDeque::new()), brain: Some(brain), delay: Duration::ZERO, seen: Mutex::new(Vec::new()) }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn chunks(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(6).map(|c| c.iter().collect()).collect()
}

impl Model for MockModel {
    fn spec(&self) -> &'static ModelSpec {
        self.spec
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn send(&self, req: &Request, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, GateError> {
        if let Ok(mut s) = self.seen.lock() {
            s.push(req.clone());
        }
        let next = self.script.lock().ok().and_then(|mut s| s.pop_front());
        let turn = match (next, &self.brain) {
            (Some(t), _) => t,
            (None, Some(b)) => b(req),
            (None, None) => Turn::text("（模拟模型的脚本用完了）"),
        };
        if let Some(e) = turn.error {
            return Err(e);
        }
        let pause = |cancel: &AtomicBool| -> Result<(), GateError> {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            if cancel.load(Ordering::SeqCst) { Err(GateError::cancelled()) } else { Ok(()) }
        };
        let mut parts = Vec::new();
        if !turn.reasoning.is_empty() {
            for c in chunks(&turn.reasoning) {
                pause(cancel)?;
                on(Delta::Reasoning(c));
            }
            parts.push(Part::Reasoning { text: turn.reasoning.clone(), opaque: None });
        }
        if !turn.text.is_empty() {
            for c in chunks(&turn.text) {
                pause(cancel)?;
                on(Delta::Text(c));
            }
            parts.push(Part::Text { text: turn.text.clone() });
        }
        for (i, (name, input)) in turn.calls.iter().enumerate() {
            let n = self.seen.lock().map(|s| s.len()).unwrap_or(0);
            let call_id = format!("mock-{n}-{i}");
            pause(cancel)?;
            on(Delta::ToolCall { call_id: call_id.clone(), name: name.clone() });
            parts.push(Part::ToolCall { call_id, name: name.clone(), input: input.clone(), signature: None });
        }
        let stop = turn.stop.unwrap_or(if turn.calls.is_empty() { Stop::EndTurn } else { Stop::ToolUse });
        let out = crate::agent::usage::estimate_tokens(&format!("{}{}{:?}", turn.text, turn.reasoning, turn.calls));
        let tokens = Tokens { input: req.input_estimate(), output: out, ..Default::default() };
        Ok(Reply { message: Message { role: Role::Assistant, parts }, stop, tokens, model: self.spec.api_model.into(), request_id: None, refusal: None })
    }
}
