//! 合成夹具和模拟接口：虚构的「示例市」青年就业调查，整条流程不联网就能跑（测试、开发版、`?mock` 以外的模拟模式都用它）。
//!
//! 地名、机构、数字全部是编的（见 `fixtures/README.md`）。模拟模型返回录制好的回复，不看输入的内容；
//! 结论那一步按事实表里有哪些事实来拼，所以用户删改子问题后编号变了也对得上。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::model::{Hit, ModelRef, Source};
use crate::ports::{ErrorKind, Fetch, Fetched, Model, ModelReply, ModelRequest, PortError, Purpose, Search, SearchReply, Usage};

pub const QUESTION: &str = "2022–2024 年示例市 16–24 岁青年的就业情况怎么样？和样本市比呢？";
/// 模拟来源的网址前缀（.invalid 是保留域名，永远解析不到）
pub const BASE: &str = "https://fixture.example.invalid";

/// 一份合成来源
pub struct Doc {
    /// 网址里的路径，也是这份来源的名字
    pub path: &'static str,
    pub title: &'static str,
    /// 原始格式：html / csv / pdf / xlsx
    pub kind: &'static str,
    /// 取出来的正文（第 1 轮）
    pub text: &'static str,
    /// 第 2 轮（重查）时的正文：`None` = 和第 1 轮一样
    pub text_r2: Option<&'static str>,
    /// 模型从这份来源摘到的事实（录制的回复）
    pub extract: &'static str,
    pub extract_r2: Option<&'static str>,
    /// 只在某一轮存在：1 = 第 2 轮消失了，2 = 第 2 轮新出现
    pub only_round: Option<u32>,
}

pub const DOCS: &[Doc] = &[
    Doc {
        path: "/stats/bulletin-2024.html",
        title: "示例市 2024 年国民经济和社会发展统计公报",
        kind: "html",
        text: include_str!("../fixtures/text/bulletin-2024.txt"),
        text_r2: Some(include_str!("../fixtures/text/bulletin-2024-r2.txt")),
        extract: include_str!("../fixtures/replies/extract-bulletin-2024.json"),
        extract_r2: Some(include_str!("../fixtures/replies/extract-bulletin-2024-r2.json")),
        only_round: None,
    },
    Doc {
        path: "/stats/labour-table.csv",
        title: "表 3 示例市劳动力市场主要指标",
        kind: "csv",
        text: include_str!("../fixtures/text/labour-table.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-labour-table.json"),
        extract_r2: None,
        only_round: None,
    },
    Doc {
        path: "/survey/youth-survey-2024.pdf",
        title: "示例市青年就业调查报告（2024）",
        kind: "pdf",
        text: include_str!("../fixtures/text/youth-survey-2024.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-youth-survey-2024.json"),
        extract_r2: None,
        only_round: None,
    },
    Doc {
        path: "/platform/platform-2024q4.xlsx",
        title: "示例招聘平台 2024 年第四季度岗位数据",
        kind: "xlsx",
        text: include_str!("../fixtures/text/platform-2024q4.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-platform-2024q4.json"),
        extract_r2: None,
        only_round: None,
    },
    Doc {
        path: "/news/daily-relay.html",
        title: "示例市青年就业形势总体稳定",
        kind: "html",
        text: include_str!("../fixtures/text/daily-relay.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-daily-relay.json"),
        extract_r2: None,
        only_round: None,
    },
    Doc {
        path: "/blog/blog-rumor.html",
        title: "示例市的年轻人到底好不好找工作",
        kind: "html",
        text: include_str!("../fixtures/text/blog-rumor.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-blog-rumor.json"),
        extract_r2: None,
        only_round: Some(1),
    },
    Doc {
        path: "/stats/sample-city-2024.html",
        title: "样本市 2024 年国民经济和社会发展统计公报",
        kind: "html",
        text: include_str!("../fixtures/text/sample-city-2024.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-sample-city-2024.json"),
        extract_r2: None,
        only_round: None,
    },
    Doc {
        path: "/stats/h1-2025.html",
        title: "2025 年上半年示例市就业情况",
        kind: "html",
        text: include_str!("../fixtures/text/h1-2025.txt"),
        text_r2: None,
        extract: include_str!("../fixtures/replies/extract-h1-2025.json"),
        extract_r2: None,
        only_round: Some(2),
    },
];

/// 搜得到、但下载会失败的网址（404）
pub const MISSING: &str = "/stats/report-2023-missing.html";
/// 下载得到、但取不出文字的文件（扫描版 PDF）
pub const SCANNED: &str = "/scan/yearbook-scan.pdf";

pub const PLAN_REPLY: &str = include_str!("../fixtures/replies/plan.json");

impl Doc {
    pub fn exists_in(&self, round: u32) -> bool {
        match self.only_round {
            None => true,
            Some(1) => round <= 1,
            Some(_) => round >= 2,
        }
    }

    pub fn text_in(&self, round: u32) -> &'static str {
        if round >= 2 { self.text_r2.unwrap_or(self.text) } else { self.text }
    }

    pub fn extract_in(&self, round: u32) -> &'static str {
        if round >= 2 { self.extract_r2.unwrap_or(self.extract) } else { self.extract }
    }
}

pub fn doc_for(url: &str) -> Option<&'static Doc> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    DOCS.iter().find(|d| path.ends_with(d.path))
}

/// 每个搜索词搜到哪些来源（按路径；第 2 轮有一处不同）
pub fn hits_for(query: &str, round: u32) -> Vec<&'static str> {
    let q: String = query.chars().filter(|c| !c.is_whitespace()).collect();
    let has = |words: &[&str]| words.iter().all(|w| q.contains(w));
    let mut paths: Vec<&'static str> = if has(&["样本市"]) {
        vec!["/stats/sample-city-2024.html"]
    } else if has(&["招聘"]) || has(&["岗位"]) {
        vec!["/platform/platform-2024q4.xlsx"]
    } else if has(&["调查", "大学"]) || has(&["独立"]) || has(&["学术"]) {
        vec!["/survey/youth-survey-2024.pdf"]
    } else if has(&["求人倍率"]) {
        vec!["/stats/labour-table.csv"]
    } else if has(&["工资"]) {
        vec!["/stats/bulletin-2024.html", SCANNED]
    } else if has(&["16-24岁"]) || has(&["调查失业率"]) {
        vec!["/stats/labour-table.csv", "/stats/bulletin-2024.html"]
    } else if has(&["失业率"]) {
        vec!["/stats/bulletin-2024.html", "/news/daily-relay.html", "/blog/blog-rumor.html", "/stats/h1-2025.html", MISSING]
    } else {
        // 别的搜索词（用户自己改的）：给主要的几份
        vec!["/stats/bulletin-2024.html", "/survey/youth-survey-2024.pdf", "/platform/platform-2024q4.xlsx"]
    };
    paths.retain(|p| doc_for(p).is_none_or(|d| d.exists_in(round)));
    paths
}

fn pause(delay: Duration, cancel: &AtomicBool) -> Result<(), PortError> {
    let steps = (delay.as_millis() / 50).max(1) as u32;
    for _ in 0..steps {
        if cancel.load(Ordering::SeqCst) {
            return Err(PortError::cancelled());
        }
        if !delay.is_zero() {
            std::thread::sleep(delay / steps);
        }
    }
    Ok(())
}

// ---------- 模拟模型 ----------

type Tamper = Box<dyn Fn(&ModelRequest, String) -> String + Send + Sync>;

/// 返回录制好的回复。`round` = 同一问题的第几次调查（重查时来源和数字有变化）。
pub struct MockModel {
    pub role: ModelRef,
    pub round: u32,
    pub delay: Duration,
    /// 测试用：把回复改坏（改一个字、改一位数……）再交出去
    pub tamper: Option<Tamper>,
    /// 测试用：这些用途的调用直接失败
    pub fail: Option<(Purpose, PortError)>,
    /// 收到过的请求的标签（测试看调了几次、调了什么）
    pub calls: Mutex<Vec<String>>,
}

impl MockModel {
    pub fn new(model: &str, round: u32) -> Self {
        MockModel { role: ModelRef { provider: "mock".into(), model: model.into() }, round, delay: Duration::ZERO, tamper: None, fail: None, calls: Mutex::new(Vec::new()) }
    }

    pub fn tampering(mut self, f: impl Fn(&ModelRequest, String) -> String + Send + Sync + 'static) -> Self {
        self.tamper = Some(Box::new(f));
        self
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Model for MockModel {
    fn info(&self) -> ModelRef {
        self.role.clone()
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn complete(&self, req: &ModelRequest, cancel: &AtomicBool) -> Result<ModelReply, PortError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(format!("{}:{}", req.purpose.key(), req.tag));
        }
        pause(self.delay, cancel)?;
        if let Some((purpose, error)) = &self.fail
            && *purpose == req.purpose
        {
            return Err(error.clone());
        }
        let text = match req.purpose {
            Purpose::Plan => PLAN_REPLY.to_string(),
            Purpose::Extract => {
                // 请求里写着这份来源的网址
                let url = req.user.iter().flat_map(|u| u.lines()).find_map(|l| l.strip_prefix("网址：")).unwrap_or("");
                // 只有第一段有事实（夹具的正文都很短）；后面的段落返回空
                match doc_for(url).filter(|_| req.tag.ends_with("#1")) {
                    Some(d) => d.extract_in(self.round).to_string(),
                    None => json!({ "publisher": "", "published": "", "facts": [] }).to_string(),
                }
            }
            Purpose::Conclude => conclude_reply(&req.user.join("\n")),
        };
        let text = match &self.tamper {
            Some(f) => f(req, text),
            None => text,
        };
        let usage = Usage { input_tokens: req.sent_chars() as u64, output_tokens: text.chars().count() as u64, searches: 0, cost_usd: 0.0 };
        let (hits, searched) = if req.purpose == Purpose::Plan && req.web_searches > 0 {
            (vec![hit("/stats/bulletin-2024.html")], vec!["示例市 青年就业 数据来源".to_string()])
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(ModelReply { text, model: self.role.model.clone(), usage, hits, searched })
    }
}

/// 事实表里的一行（模拟模型自己读，只为挑编号）
struct Row {
    id: String,
    metric: String,
    region: String,
    period: String,
    producer: String,
}

fn rows(table: &str) -> Vec<Row> {
    table
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.split(" | ").map(str::trim).collect();
            (cells.len() >= 11 && cells[0].starts_with('F') && cells[0][1..].bytes().all(|b| b.is_ascii_digit())).then(|| Row { id: cells[0].into(), metric: cells[2].into(), region: cells[4].into(), period: cells[6].into(), producer: cells[8].into() })
        })
        .collect()
}

/// 按事实表里有哪些事实拼出结论（JSON）。没有对应事实的结论不写。
pub fn conclude_reply(request_text: &str) -> String {
    let all = rows(request_text);
    let find = |metric: &str, region: &str, period: &str, producer: &str| all.iter().find(|r| r.metric.contains(metric) && r.region.contains(region) && r.period.contains(period) && r.producer.contains(producer)).map(|r| r.id.clone());
    let mut calcs: Vec<Value> = Vec::new();
    let mut conclusions: Vec<Value> = Vec::new();
    let mut calc = |op: &str, inputs: [&String; 2], label: &str, decimals: u32| {
        let id = format!("C{}", calcs.len() + 1);
        calcs.push(json!({ "id": id, "op": op, "inputs": inputs, "label": label, "decimals": decimals }));
        id
    };
    let official_now = find("城镇青年调查失业率", "示例市", "2024", "示例市统计局（中国政府官方统计）");
    let official_before = find("城镇青年调查失业率", "示例市", "2023", "");
    let independent = find("独立估计", "示例市", "2024", "");
    if let (Some(now), Some(before)) = (&official_now, &official_before) {
        let c = calc("diff", [before, now], "官方青年失业率 2023 年比 2024 年高多少", 1);
        conclusions.push(json!({ "text": format!("按官方口径，示例市 16–24 岁城镇青年调查失业率 2024 年为 {{{now}}}，比 2023 年的 {{{before}}} 低 {{{c}}}。数字由示例市统计局发布。"), "facts": [now, before], "calcs": [c], "uncertainty": [], "note": "调查失业率不含在校学生" }));
    } else if let Some(now) = &official_now {
        conclusions.push(json!({ "text": format!("按官方口径，示例市 16–24 岁城镇青年调查失业率 2024 年为 {{{now}}}。数字由示例市统计局发布。"), "facts": [now], "calcs": [], "uncertainty": [], "note": "" }));
    }
    if let (Some(ind), Some(now)) = (&independent, &official_now) {
        let c = calc("diff", [ind, now], "独立估计比官方数字高多少", 1);
        conclusions.push(json!({ "text": format!("独立调查给出的数字更高：示例大学劳动经济研究所按宽口径估计 2024 年为 {{{ind}}}，比官方数字高 {{{c}}}。宽口径把放弃找工作的人也算作失业，两个数不能直接比大小。"), "facts": [ind, now], "calcs": [c], "uncertainty": ["conflicting", "unclear_definition"], "note": "口径不同" }));
    } else if let Some(ind) = &independent {
        conclusions.push(json!({ "text": format!("示例大学劳动经济研究所按宽口径估计，2024 年示例市 16–24 岁青年失业率为 {{{ind}}}。"), "facts": [ind], "calcs": [], "uncertainty": ["unclear_definition"], "note": "" }));
    }
    if let (Some(public), Some(private)) = (find("非私营单位", "示例市", "2024", ""), find("城镇私营单位", "示例市", "2024", "")) {
        let c = calc("ratio", [&public, &private], "非私营单位工资是私营单位的几倍", 2);
        conclusions.push(json!({ "text": format!("工资方面，2024 年示例市城镇非私营单位就业人员年平均工资为 {{{public}}}，私营单位为 {{{private}}}，前者是后者的 {{{c}}}。"), "facts": [public, private], "calcs": [c], "uncertainty": [], "note": "" }));
    }
    if let (Some(jobs), Some(pay)) = (find("新发布岗位数", "示例市", "", ""), find("平均招聘月薪", "示例市", "", "")) {
        conclusions.push(json!({ "text": format!("招聘平台的数据显示，2024 年第四季度示例市新发布岗位 {{{jobs}}}，平均招聘月薪 {{{pay}}}。这是示例招聘平台自己的业务数据，只覆盖在该平台招聘的企业。"), "facts": [jobs, pay], "calcs": [], "uncertainty": [], "note": "" }));
    }
    if let (Some(here), Some(there)) = (&official_now, find("城镇青年调查失业率", "样本市", "2024", "")) {
        let c = calc("diff", [here, &there], "示例市比样本市高多少", 1);
        conclusions.push(json!({ "text": format!("和样本市比，2024 年示例市的官方青年失业率高 {{{c}}}（样本市为 {{{there}}}）。"), "facts": [here, there], "calcs": [c], "uncertainty": [], "note": "同为城镇调查失业率" }));
    }
    if conclusions.is_empty() {
        conclusions.push(json!({ "text": "事实表里没有能回答这个问题的数字。", "facts": [], "calcs": [], "uncertainty": ["not_found"], "note": "" }));
    }
    json!({
        "calculations": calcs,
        "conclusions": conclusions,
        "gaps": ["没有找到示例市社保参保人数、灵活就业和平台就业的可靠数据。", "2022 年以前和 2025 年全年的数据不在这次查到的来源里。"],
    })
    .to_string()
}

// ---------- 模拟搜索 ----------

fn hit(path: &str) -> Hit {
    let title = doc_for(path).map(|d| d.title).unwrap_or(if path == SCANNED { "示例市统计年鉴（扫描件）" } else { "示例市 2023 年统计公报" });
    Hit { url: format!("{BASE}{path}"), title: title.to_string(), snippet: format!("（合成的搜索结果）{title}"), published: None }
}

pub struct MockSearch {
    pub round: u32,
    /// 来源网址的前缀：测试里换成本地假服务器的地址
    pub base: String,
    pub delay: Duration,
    pub fail: Option<PortError>,
}

impl MockSearch {
    pub fn new(round: u32) -> Self {
        MockSearch { round, base: BASE.into(), delay: Duration::ZERO, fail: None }
    }
}

impl Search for MockSearch {
    fn engine(&self) -> String {
        "mock".into()
    }

    fn is_mock(&self) -> bool {
        true
    }

    fn search(&self, query: &str, max_results: usize, cancel: &AtomicBool) -> Result<SearchReply, PortError> {
        pause(self.delay, cancel)?;
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        let hits = hits_for(query, self.round).into_iter().take(max_results).map(|p| Hit { url: format!("{}{p}", self.base), ..hit(p) }).collect();
        Ok(SearchReply { hits, usage: Usage { searches: 1, ..Default::default() } })
    }
}

// ---------- 模拟下载 ----------

/// 不联网：按网址给合成来源的正文（不经过真的下载和取正文；那一套在 `src-tauri` 用原始格式的夹具测）
pub struct MockFetch {
    pub round: u32,
    pub delay: Duration,
}

impl MockFetch {
    pub fn new(round: u32) -> Self {
        MockFetch { round, delay: Duration::ZERO }
    }
}

/// 假的 SHA-256：只要同样的内容给同样的值（引擎不依赖哈希库）
fn fake_sha(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}").repeat(4)
}

impl Fetch for MockFetch {
    fn fetch(&self, url: &str, cancel: &AtomicBool) -> Result<Fetched, String> {
        pause(self.delay, cancel).map_err(|e| e.message)?;
        if url.ends_with(SCANNED) {
            return Ok(Fetched { final_url: url.into(), fetched_at: "2026-11-12T09:00:00".into(), sha256: fake_sha(url), kind: "pdf".into(), content_type: Some("application/pdf".into()), ext: "pdf".into(), bytes: 20_480, title: None, text: Err("PDF 里没有可以提取的文字（可能是扫描件）".into()) });
        }
        let doc = doc_for(url).filter(|d| d.exists_in(self.round)).ok_or_else(|| "服务器说没有这个网页（404）".to_string())?;
        let text = doc.text_in(self.round);
        Ok(Fetched {
            final_url: url.into(),
            fetched_at: "2026-11-12T09:00:00".into(),
            sha256: fake_sha(text),
            kind: doc.kind.into(),
            content_type: None,
            ext: doc.kind.into(),
            bytes: text.len() as u64,
            title: Some(doc.title.into()),
            text: Ok(text.to_string()),
        })
    }

    fn text(&self, source: &Source) -> Result<String, String> {
        doc_for(&source.url).map(|d| d.text_in(self.round).to_string()).ok_or_else(|| "没有保存这份来源的正文".to_string())
    }
}

/// 模拟一次失败（测试用）
pub fn failure(kind: ErrorKind, message: &str) -> PortError {
    PortError::new(kind, message)
}
