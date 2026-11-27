//! 模型网关的测试：四家的 body、四家的流、重试和停、记账、用量。都不联网（本机假服务器 + 合成的流）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::catalog;
use super::http::{Credential, HttpModel};
use super::meter::{self, Meter, Use};
use super::mock::{MockModel, Turn};
use super::*;
use crate::agent::usage::{Ledger, Prices};
use crate::db::tests::temp_dir;
use crate::research::testing::{FakeServer, Resp};

fn spec(id: &str) -> &'static catalog::ModelSpec {
    catalog::model(id).unwrap()
}

fn origin(id: &str) -> Origin {
    let s = spec(id);
    Origin { provider: s.provider, model: s.api_model.into() }
}

/// 一段有工具来回的对话：问 → 调工具（带思考）→ 结果 → 再问
fn history(o: &Origin) -> Vec<Message> {
    vec![
        Message::user("斯密怎么说分工？"),
        Message {
            role: Role::Assistant,
            parts: vec![
                Part::Reasoning { text: "先查文献库".into(), opaque: Some(Opaque { origin: o.clone(), data: json!({ "type": "thinking", "signature": "SIG", "encrypted_content": "ENC", "thoughtSignature": "TS" }) }) },
                Part::Text { text: "我查一下。".into() },
                Part::ToolCall { call_id: "c1".into(), name: "library_search".into(), input: json!({ "query": "division of labour" }), signature: Some(Opaque { origin: o.clone(), data: json!({ "thoughtSignature": "TS-CALL" }) }) },
            ],
        },
        Message { role: Role::User, parts: vec![Part::ToolResult { call_id: "c1".into(), output: "找到 2 条".into(), is_error: false }] },
        Message::user("再说具体一点"),
    ]
}

fn request(o: &Origin) -> Request {
    Request {
        system: vec![SystemBlock { text: "App 的规矩".into(), cache: true }, SystemBlock { text: "角色".into(), cache: true }],
        tools: vec![ToolSpec { name: "library_search".into(), description: "查文献库".into(), input_schema: json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }) }],
        messages: history(o),
        max_output_tokens: 16_000,
        effort: Some("medium".into()),
        no_tools: false,
        cache_key: Some("conv-1".into()),
    }
}

fn body(id: &str, req: &Request) -> Value {
    let cred = if id.starts_with("vertex/") { Credential::Adc } else { Credential::Key("k".into()) };
    let v = vertex::VertexConfig { project: "my-project-123".into(), location: "global".into() };
    HttpModel::new(spec(id), cred, Some(&v)).unwrap().body(req)
}

#[test]
fn anthropic_body_has_cache_breakpoints_thinking_and_fallback() {
    let id = "anthropic/claude-sonnet-5-5";
    let b = body(id, &request(&origin(id)));
    assert_eq!(b["model"], "claude-sonnet-5-5");
    assert_eq!(b["thinking"], json!({ "type": "adaptive", "display": "summarized" }));
    assert_eq!(b["output_config"]["effort"], "medium");
    assert_eq!(b["fallbacks"], "default");
    assert!(b.get("temperature").is_none());
    assert_eq!(b["tools"][0]["eager_input_streaming"], true);
    // 断点：系统两块 + 最后两条站主消息，一共 4 个
    let n = b.to_string().matches("cache_control").count();
    assert_eq!(n, 4);
    assert!(b["system"][1]["cache_control"].is_object());
    let msgs = b["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4);
    // 思考原样放回，工具结果在站主那一条
    assert_eq!(msgs[1]["content"][0], json!({ "type": "thinking", "thinking": "先查文献库", "signature": "SIG" }));
    assert_eq!(msgs[1]["content"][2]["type"], "tool_use");
    assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
    assert!(msgs[3]["content"][0]["cache_control"].is_object());
    // 换了一个 Claude 模型：思考照样放回（API 自己丢读不了的）；别家的思考不放
    let other = body("anthropic/claude-opus-5-5", &request(&origin(id)));
    assert_eq!(other["messages"][1]["content"][0]["type"], "thinking");
    assert!(other["fallbacks"] == "default");
    let from_openai = body(id, &request(&origin("openai/gpt-6.2-sol")));
    assert_eq!(from_openai["messages"][1]["content"][0]["type"], "text");
    // Haiku 没有服务端回退；收尾那一轮不许调工具
    let mut r = request(&origin(id));
    r.no_tools = true;
    let h = body("anthropic/claude-haiku-5-5", &r);
    assert!(h.get("fallbacks").is_none());
    assert_eq!(h["tool_choice"], json!({ "type": "none" }));
}

#[test]
fn openai_body_replays_encrypted_reasoning_without_storing() {
    let id = "openai/gpt-6.2-sol";
    let b = body(id, &request(&origin(id)));
    assert_eq!(b["store"], false);
    assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(b["prompt_cache_key"], "conv-1");
    assert_eq!(b["instructions"], "App 的规矩\n\n角色");
    assert_eq!(b["reasoning"], json!({ "summary": "auto", "effort": "medium" }));
    let input = b["input"].as_array().unwrap();
    let types: Vec<&str> = input.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["message", "reasoning", "message", "function_call", "function_call_output", "message"]);
    assert_eq!(input[1]["encrypted_content"], "ENC");
    assert_eq!(input[3]["arguments"], r#"{"query":"division of labour"}"#);
    // 能关思考的模型、这一档不开：effort none
    let mut r = request(&origin(id));
    r.effort = None;
    assert_eq!(body("openai/gpt-6-luna", &r)["reasoning"], json!({ "effort": "none" }));
}

#[test]
fn deepseek_body_replays_reasoning_content_and_switches_thinking() {
    let id = "deepseek/deepseek-flash";
    let b = body(id, &request(&origin(id)));
    let msgs = b["messages"].as_array().unwrap();
    assert_eq!(msgs[0], json!({ "role": "system", "content": "App 的规矩\n\n角色" }));
    assert_eq!(msgs[2]["reasoning_content"], "先查文献库");
    assert_eq!(msgs[2]["tool_calls"][0]["function"]["name"], "library_search");
    assert_eq!(msgs[3], json!({ "role": "tool", "tool_call_id": "c1", "content": "找到 2 条" }));
    assert_eq!(b["thinking"], json!({ "type": "enabled" }));
    assert_eq!(b["reasoning_effort"], "medium");
    assert_eq!(b["stream_options"]["include_usage"], true);
    let mut r = request(&origin(id));
    r.effort = None;
    assert_eq!(body(id, &r)["thinking"], json!({ "type": "disabled" }));
}

#[test]
fn gemini_body_carries_thought_signatures_for_key_and_vertex() {
    for id in ["google/gemini-3.8-flash", "vertex/gemini-3.8-flash"] {
        let b = body(id, &request(&origin(id)));
        let contents = b["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
        let call = contents[1]["parts"].as_array().unwrap().iter().find(|p| p.get("functionCall").is_some()).unwrap();
        assert_eq!(call["thoughtSignature"], "TS-CALL", "{id}");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "library_search");
        assert_eq!(b["generationConfig"]["thinkingConfig"], json!({ "includeThoughts": true, "thinkingLevel": "medium" }));
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], "App 的规矩\n\n角色");
        assert_eq!(b["tools"][0]["functionDeclarations"][0]["name"], "library_search");
    }
}

// ---------- 流 ----------

fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("data: {e}\n\n")).collect()
}

fn run(model: &HttpModel, req: &Request) -> (Result<Reply, GateError>, Vec<Delta>) {
    let mut deltas = Vec::new();
    let r = model.send(req, &mut |d| deltas.push(d), &AtomicBool::new(false));
    (r, deltas)
}

fn served(id: &str, body: String) -> (FakeServer, HttpModel) {
    let server = FakeServer::start(move |_| Resp::sse(&body));
    let cred = if id.starts_with("vertex/") { Credential::Adc } else { Credential::Key("k".into()) };
    let m = HttpModel::new(spec(id), cred, None).map(|m| m.at(&format!("{}/x", server.base))).unwrap();
    (server, m)
}

#[test]
fn anthropic_stream_rebuilds_thinking_text_and_tool_calls() {
    let id = "anthropic/claude-sonnet-5-5";
    let stream = sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_1", "model": "claude-sonnet-5-5", "usage": { "input_tokens": 120, "cache_read_input_tokens": 3000, "cache_creation_input_tokens": 500, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "想想" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "SIG" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "查一下" } }),
        json!({ "type": "content_block_start", "index": 2, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "calculate", "input": {} } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "{\"expr\":" } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "\"1+2\"}" } }),
        json!({ "type": "content_block_start", "index": 3, "content_block": { "type": "fallback", "from": { "model": "x" }, "to": { "model": "y" } } }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 80 } }),
        json!({ "type": "message_stop" }),
    ]);
    let (_s, m) = served(id, stream);
    let (reply, deltas) = run(&m, &request(&origin(id)));
    let reply = reply.unwrap();
    assert_eq!(reply.stop, Stop::ToolUse);
    assert_eq!(reply.tokens, Tokens { input: 120, output: 80, cache_read: 3000, cache_write: 500 });
    assert_eq!(reply.message.tool_calls()[0].2, &json!({ "expr": "1+2" }));
    assert_eq!(reply.message.text(), "查一下");
    assert!(matches!(&reply.message.parts[0], Part::Reasoning { text, opaque: Some(o) } if text == "想想" && o.data["signature"] == "SIG"));
    // 不认识的块原样留着、只回放给同一来源
    assert!(matches!(&reply.message.parts[3], Part::Native { opaque } if opaque.data["type"] == "fallback"));
    assert!(deltas.contains(&Delta::Text("查一下".into())));
    assert!(deltas.contains(&Delta::ToolCall { call_id: "toolu_1".into(), name: "calculate".into() }));
    assert_eq!(reply.request_id.as_deref(), Some("msg_1"));
}

#[test]
fn openai_deepseek_gemini_streams_finish_with_usage() {
    let id = "openai/gpt-6.2-sol";
    let s = sse(&[
        json!({ "type": "response.created", "response": { "id": "resp_1", "model": "gpt-6.2-sol" } }),
        json!({ "type": "response.output_text.delta", "delta": "你好" }),
        json!({ "type": "response.completed", "response": { "status": "completed", "model": "gpt-6.2-sol", "output": [
            { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "嗯" }], "encrypted_content": "ENC" },
            { "type": "message", "content": [{ "type": "output_text", "text": "你好" }] },
            { "type": "function_call", "call_id": "call_1", "name": "calculate", "arguments": "{\"expr\":\"2*3\"}" }
        ], "usage": { "input_tokens": 1000, "input_tokens_details": { "cached_tokens": 800 }, "output_tokens": 50 } } }),
    ]);
    let (_s, m) = served(id, s);
    let r = run(&m, &request(&origin(id))).0.unwrap();
    assert_eq!((r.stop.clone(), r.tokens), (Stop::ToolUse, Tokens { input: 200, output: 50, cache_read: 800, cache_write: 0 }));
    assert!(matches!(&r.message.parts[0], Part::Reasoning { opaque: Some(o), .. } if o.data["encrypted_content"] == "ENC"));

    let id = "deepseek/deepseek-flash";
    let s = format!(
        "{}data: [DONE]\n\n",
        sse(&[
            json!({ "id": "ds1", "model": "deepseek-flash", "choices": [{ "delta": { "reasoning_content": "想" } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "call_a", "function": { "name": "calculate", "arguments": "{\"ex" } }] } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "pr\":\"1\"}" } }] }, "finish_reason": "tool_calls" }] }),
            json!({ "choices": [], "usage": { "prompt_tokens": 900, "prompt_cache_hit_tokens": 600, "prompt_cache_miss_tokens": 300, "completion_tokens": 40 } }),
        ])
    );
    let (_s, m) = served(id, s);
    let r = run(&m, &request(&origin(id))).0.unwrap();
    assert_eq!(r.stop, Stop::ToolUse);
    assert_eq!(r.tokens, Tokens { input: 300, output: 40, cache_read: 600, cache_write: 0 });
    assert_eq!(r.message.tool_calls()[0].2, &json!({ "expr": "1" }));

    let id = "google/gemini-3.8-flash";
    let s = sse(&[
        json!({ "responseId": "g1", "modelVersion": "gemini-3.8-flash", "candidates": [{ "content": { "parts": [{ "text": "想", "thought": true }] } }] }),
        json!({ "candidates": [{ "content": { "parts": [{ "functionCall": { "name": "calculate", "args": { "expr": "3" } }, "thoughtSignature": "TS" }] } }] }),
        json!({ "candidates": [{ "content": { "parts": [] }, "finishReason": "STOP" }], "usageMetadata": { "promptTokenCount": 500, "cachedContentTokenCount": 100, "candidatesTokenCount": 20, "thoughtsTokenCount": 30 } }),
    ]);
    let (_s, m) = served(id, s);
    let r = run(&m, &request(&origin(id))).0.unwrap();
    assert_eq!(r.stop, Stop::ToolUse);
    assert_eq!(r.tokens, Tokens { input: 400, output: 50, cache_read: 100, cache_write: 0 });
    match &r.message.parts[1] {
        Part::ToolCall { call_id, signature: Some(sig), .. } => {
            assert!(call_id.starts_with("pith-call-"));
            assert_eq!(sig.data["thoughtSignature"], "TS");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn truncated_tool_input_is_marked_not_dropped() {
    let v = parse_tool_input("{\"expr\": \"1+");
    assert!(v.get(BAD_INPUT_KEY).is_some());
    assert_eq!(parse_tool_input(""), json!({}));
    assert_eq!(parse_tool_input("[1]").get(BAD_INPUT_KEY).is_some(), true);
}

#[test]
fn retries_busy_answers_then_reports_in_chinese() {
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let ok = sse(&[json!({ "type": "message_start", "message": { "model": "claude-haiku-5-5", "usage": { "input_tokens": 5 } } }), json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 2 } }), json!({ "type": "message_stop" })]);
    let server = FakeServer::start(move |_| if h.fetch_add(1, Ordering::SeqCst) < 2 { Resp::json(529, r#"{"error":{"type":"overloaded_error","message":"busy"}}"#) } else { Resp::sse(&ok) });
    let m = HttpModel::new(spec("anthropic/claude-haiku-5-5"), Credential::Key("sk-ant-test".into()), None).unwrap().at(&format!("{}/v1/messages", server.base));
    let r = run(&m, &Request { messages: vec![Message::user("hi")], max_output_tokens: 100, ..Default::default() }).0.unwrap();
    assert_eq!(r.stop, Stop::EndTurn);
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    // 请求头：密钥、版本；Haiku 没有回退的 beta
    let reqs = server.requests();
    assert_eq!(reqs[0].header("x-api-key"), Some("sk-ant-test"));
    assert_eq!(reqs[0].header("anthropic-version"), Some(anthropic::API_VERSION));
    assert_eq!(reqs[0].header("anthropic-beta"), None);

    // 密钥不对：不重试，说清楚、不带密钥
    let server = FakeServer::start(|_| Resp::json(401, r#"{"error":{"type":"authentication_error","message":"invalid x-api-key"}}"#));
    let m = HttpModel::new(spec("anthropic/claude-sonnet-5-5"), Credential::Key("sk-ant-secret".into()), None).unwrap().at(&format!("{}/v1/messages", server.base));
    let e = run(&m, &Request { messages: vec![Message::user("hi")], max_output_tokens: 100, ..Default::default() }).0.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Auth);
    assert!(e.message.contains("密钥") && !e.message.contains("secret"));
    assert!(e.spent.is_none());
    assert_eq!(server.requests().len(), 1);
    assert_eq!(server.requests()[0].header("anthropic-beta"), Some(anthropic::FALLBACK_BETA));
}

#[test]
fn stream_cut_short_is_charged_by_estimate() {
    let s = sse(&[json!({ "type": "message_start", "message": { "model": "claude-sonnet-5-5", "usage": { "input_tokens": 5 } } }), json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }), json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "写到一半" } })]);
    let (_s, m) = served("anthropic/claude-sonnet-5-5", s);
    let e = run(&m, &Request { messages: vec![Message::user("hi")], max_output_tokens: 100, ..Default::default() }).0.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Upstream);
    assert!(e.spent.unwrap().output > 0);
}

#[test]
fn cancel_before_sending_costs_nothing() {
    let (_s, m) = served("anthropic/claude-sonnet-5-5", String::new());
    let e = m.send(&Request::default(), &mut |_| {}, &AtomicBool::new(true)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Cancelled);
    assert!(e.spent.is_none());
}

// ---------- 记账 ----------

#[test]
fn meter_checks_the_budget_and_records_each_call() {
    let dir = temp_dir("gateway-meter");
    let mut meter = Meter::new(&dir, 1.0, Prices::default());
    meter.now = || "2026-11-26T10:00:00.000Z".into();
    let model = MockModel::scripted(spec("anthropic/claude-sonnet-5-5"), vec![Turn::text("好")]);
    let u = Use { purpose: "chat", conversation: Some("conv-1") };
    let (reply, cost) = meter::send(&model, &meter, &u, &Request { messages: vec![Message::user("hi")], max_output_tokens: 100, ..Default::default() }, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(reply.message.text(), "好");
    assert_eq!(cost, 0.0);
    let calls = meter.ledger.all().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].mock && calls[0].conversation.as_deref() == Some("conv-1") && calls[0].provider == "anthropic");

    // 真接口：最坏情况超了月上限就不发
    std::fs::write(dir.join("agent/usage.jsonl"), "{\"at\":\"2026-11-02T10:00:00\",\"purpose\":\"chat\",\"model\":\"claude-opus-5-5\",\"tokens\":{\"input\":1,\"output\":1},\"costUsd\":0.99}\n").unwrap();
    let server = FakeServer::start(|_| Resp::json(500, "{}"));
    let real = HttpModel::new(spec("anthropic/claude-opus-5-5"), Credential::Key("k".into()), None).unwrap().at(&server.base);
    let e = meter::send(&real, &meter, &u, &Request { messages: vec![Message::user("hi")], max_output_tokens: 16_000, ..Default::default() }, &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Budget);
    assert!(e.message.contains("月上限"));
    assert!(server.requests().is_empty());
}

#[test]
fn meter_prices_by_the_reporting_model_with_cache_and_overrides() {
    let dir = temp_dir("gateway-meter-price");
    let meter = Meter::new(&dir, 30.0, serde_json::from_str(r#"{"models":{"claude-sonnet-5-5":{"input":4,"output":20}}}"#).unwrap());
    let t = Tokens { input: 1_000_000, output: 0, cache_read: 1_000_000, cache_write: 0 };
    // 设置把 Sonnet 改成 4 / 20：缓存读跟着翻倍
    assert!((meter.row("claude-sonnet-5-5").usd(&t) - 4.2).abs() < 1e-9);
    assert!((meter.row("claude-opus-5-5").usd(&t) - 4.2).abs() < 1e-9);
    assert_eq!(meter.row("never-heard-of").base.input, crate::agent::usage::UNKNOWN_PRICE.input);
}

#[test]
fn usage_summary_splits_by_provider_purpose_and_cache() {
    let dir = temp_dir("gateway-usage");
    let l = Ledger::new(&dir);
    std::fs::create_dir_all(dir.join("agent")).unwrap();
    let lines = [
        r#"{"at":"2026-10-30T10:00:00","purpose":"chat","provider":"anthropic","model":"claude-sonnet-5-5","tokens":{"input":1,"output":1},"costUsd":5.0}"#,
        r#"{"at":"2026-11-02T10:00:00","purpose":"chat","provider":"anthropic","model":"claude-sonnet-5-5","tokens":{"input":100,"output":10,"cacheRead":900},"costUsd":0.5,"conversation":"c1"}"#,
        r#"{"at":"2026-11-03T10:00:00","purpose":"research_extract","provider":"openai_compatible","model":"deepseek-flash","tokens":{"input":1000,"output":10},"costUsd":0.25}"#,
        r#"{"at":"2026-11-03T11:00:00","purpose":"chat","provider":"openai","model":"gpt-6.2-sol","tokens":{"input":0,"output":0},"costUsd":0.0,"mock":true}"#,
    ];
    std::fs::write(dir.join("agent/usage.jsonl"), lines.join("\n") + "\n").unwrap();
    let s = commands::summarize(&l, "2026-11", 30.0, false, Some("c1"));
    assert!((s.spent_usd - 0.75).abs() < 1e-9);
    assert_eq!(s.by_provider.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["Anthropic", "DeepSeek"]);
    assert_eq!(s.by_purpose.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["对话", "事实调查"]);
    assert_eq!((s.cache.read, s.cache.input), (900, 1100));
    assert!((s.cache.hit_rate - 0.45).abs() < 1e-9);
    assert!((s.days[1] - 0.5).abs() < 1e-9 && (s.days[2] - 0.25).abs() < 1e-9);
    assert_eq!(s.conversation_usd, Some(0.5));
    // 最近的含模拟的，新的在前
    assert!(s.recent[0].mock);
    assert_eq!(s.recent.len(), 4);
}

#[test]
fn vertex_has_its_own_models() {
    assert_eq!(catalog::strongest(Provider::Vertex).api_model, "gemini-4-carbon");
    assert!(HttpModel::new(spec("vertex/gemini-4-carbon"), Credential::Adc, None).is_err());
}

#[test]
fn thinking_can_be_switched_off_where_the_model_allows() {
    let mut req = request(&origin("anthropic/claude-sonnet-5-5"));
    req.effort = None;
    // Sonnet 5.5 关是 between_tools、Haiku 5.5 是 disabled，都不带 effort；Opus 关不掉，照 adaptive
    let b = body("anthropic/claude-sonnet-5-5", &req);
    assert_eq!((b["thinking"].clone(), b.get("output_config").is_none()), (json!({ "type": "between_tools" }), true));
    assert_eq!(body("anthropic/claude-haiku-5-5", &req)["thinking"], json!({ "type": "disabled" }));
    assert_eq!(body("anthropic/claude-opus-5-5", &req)["thinking"]["type"], "adaptive");
    // OpenAI：能关的写 none，一直开着的不写深浅
    assert_eq!(body("openai/gpt-6.1-terra", &req)["reasoning"], json!({ "effort": "none" }));
    assert_eq!(body("openai/gpt-6.2-sol", &req)["reasoning"], json!({ "summary": "auto" }));
    assert_eq!(body("deepseek/deepseek-flash", &req)["thinking"], json!({ "type": "disabled" }));
    // 设置里定的深浅照发
    req.effort = Some("xhigh".into());
    assert_eq!(body("anthropic/claude-opus-5-5", &req)["output_config"]["effort"], "xhigh");
}

#[test]
fn free_tier_calls_cost_nothing_and_skip_the_budget() {
    let dir = temp_dir("gateway-free");
    let mut meter = Meter::new(&dir, 0.000_001, Prices::default());
    meter.free = vec![Provider::Google];
    let id = "google/gemini-3.8-flash";
    let s = sse(&[json!({ "candidates": [{ "content": { "parts": [{ "text": "好" }] }, "finishReason": "STOP" }], "usageMetadata": { "promptTokenCount": 500, "candidatesTokenCount": 20 } })]);
    let (_s, m) = served(id, s);
    let u = Use { purpose: "chat", conversation: None };
    let (_, cost) = meter::send(&m, &meter, &u, &Request { messages: vec![Message::user("hi")], max_output_tokens: 100, ..Default::default() }, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(cost, 0.0);
    let c = &meter.ledger.all().unwrap()[0];
    assert_eq!(c.cost_usd, 0.0);
    assert!(c.note.as_deref().unwrap().starts_with("免费额度（按价目约 $"), "{:?}", c.note);
}

#[test]
fn free_tier_daily_quota_is_not_retried_and_says_when_it_comes_back() {
    let body = json!({ "error": { "code": 429, "message": "You exceeded your current quota", "details": [
        { "@type": "type.googleapis.com/google.rpc.QuotaFailure", "violations": [{ "quotaMetric": "generativelanguage.googleapis.com/generate_content_free_tier_requests", "quotaId": "GenerateRequestsPerDayPerProjectPerModel-FreeTier", "quotaDimensions": { "model": "gemini-3.8-flash" }, "quotaValue": "20" }] },
        { "@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "5460s" }
    ] } })
    .to_string();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let server = FakeServer::start(move |_| {
        h.fetch_add(1, Ordering::SeqCst);
        Resp::json(429, &body)
    });
    let m = HttpModel::new(spec("google/gemini-3.8-flash"), Credential::Key("k".into()), None).unwrap().at(&format!("{}/x", server.base));
    let e = run(&m, &request(&origin("google/gemini-3.8-flash"))).0.unwrap_err();
    assert_eq!(hits.load(Ordering::SeqCst), 1, "一天的额度用完了不重试");
    assert!(e.message.contains("免费层每天只能调 20 次") && e.message.contains("约 1 小时 31 分钟后恢复"), "{}", e.message);
}
