//! v0.2 Word Studio 的 App 一侧（W1 入口 `decisions/0016`、W2 引用 `0017`）：
//! - `service`：本机服务（127.0.0.1 + 随机端口 + 令牌，写 service.json）
//! - `bridge`：Word 宏的协议（ping / pick / refresh / styles / zotero / show）
//! - `picker`：置顶搜索窗（插入 / 编辑引文）
//! - `render`：引文排版桥（Rust ↔ WebView 里的 citeproc-js）
//! - `csl`：文献库条目 → CSL-JSON、Zotero 条目 key → 文献库
//! - `install`：Word 全局模板与 AppleScript 的安装 / 卸载
//! - `styles`：引文样式（内置 + 导入的 .csl）
//!
//! 服务的路由：
//!   GET  /v1/ping                 → `OK\tPith <版本>`
//!   POST /v1/word                 → Word 宏协议（`bridge`）
//!   POST /v1/docx/refresh         → Word 外刷新一份 .docx：{path, out, style?}，只写新文件（自动化与验收用）
//!   POST /v1/docx/verify          → Word 外核对一份 .docx：{path}
//!   POST /v1/docx/convert-zotero  → Word 外把 Zotero 引文换成 Pith 引文：{path, out}，只写新文件

pub mod bridge;
pub mod csl;
pub mod install;
pub mod picker;
pub mod render;
pub mod service;
pub mod styles;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use docx_engine::cite::plan::{Render, RenderCitation, RenderRequest, Rendered};
use docx_engine::cite::zotero::ItemKey;
use docx_engine::cite::{CiteItemRef, check, convert, docx, fixture};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, common, paths, text};

#[derive(Default)]
pub struct WordState {
    pub hub: render::Hub,
    pub picker: picker::Picker,
    service: Mutex<Option<service::Service>>,
    service_error: Mutex<Option<String>>,
}

fn version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 启动本机服务（App 启动时）。起不来不挡 App，设置页显示原因。
pub fn start_service(app: &AppHandle) {
    let state = app.state::<AppState>();
    let word = app.state::<WordState>();
    let handle = app.clone();
    let handler: service::Handler = Arc::new(move |req| route(&handle, req));
    let started = state.paths.ensure().and_then(|_| service::Service::start(&state.paths.root, &version(app), handler));
    match started {
        Ok(s) => {
            if let Ok(mut g) = word.service.lock() {
                *g = Some(s);
            }
        }
        Err(e) => {
            eprintln!("本机服务没有启动：{e}");
            if let Ok(mut g) = word.service_error.lock() {
                *g = Some(e);
            }
        }
    }
}

pub fn stop_service(app: &AppHandle) {
    if let Some(word) = app.try_state::<WordState>()
        && let Ok(mut g) = word.service.lock()
        && let Some(s) = g.take()
    {
        s.stop();
    }
}

struct AppHost<'a> {
    app: &'a AppHandle,
    word: &'a WordState,
    render: render::WebRender<'a>,
}

impl<'a> AppHost<'a> {
    fn new(app: &'a AppHandle, word: &'a WordState) -> Self {
        AppHost { app, word, render: render::WebRender { app, hub: &word.hub } }
    }

    fn library(&self, uids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String> {
        self.app.state::<AppState>().with_db(|c| csl::items_csl(c, uids))
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.app.state::<AppState>().paths.root.clone()
    }

    fn resolve(&self, key: &ItemKey) -> Option<String> {
        self.app.state::<AppState>().with_db(|c| csl::resolve_zotero(c, key)).ok().flatten()
    }
}

impl bridge::Host for AppHost<'_> {
    fn version(&self) -> String {
        version(self.app)
    }
    fn library(&self, uids: &BTreeSet<String>) -> Result<BTreeMap<String, Value>, String> {
        AppHost::library(self, uids)
    }
    fn renderer(&self) -> &dyn Render {
        &self.render
    }
    fn pick(&self, req: bridge::PickRequest) -> Result<bridge::PickOutcome, String> {
        let items = req.editing.clone().unwrap_or_default();
        let entries = self.app.state::<AppState>().with_db(|c| picker::entries(c, &items, &req.snapshots))?;
        picker::pick(self.app, &self.word.picker, req, entries)
    }
    fn styles(&self) -> Vec<styles::Style> {
        styles::all(&self.data_dir())
    }
    fn resolve_zotero(&self, key: &ItemKey) -> Option<String> {
        self.resolve(key)
    }
    fn show(&self) {
        show_main(self.app);
    }
    fn now(&self) -> String {
        text::now_iso()
    }
    fn new_id(&self) -> String {
        common::new_uid().unwrap_or_else(|_| text::now_iso())
    }
}

fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[derive(Deserialize)]
struct DocxRequest {
    path: String,
    #[serde(default)]
    out: Option<String>,
    #[serde(default)]
    style: Option<String>,
}

fn docx_call(app: &AppHandle, body: &[u8], refresh: bool) -> Result<Value, String> {
    let word = app.state::<WordState>();
    let host = AppHost::new(app, &word);
    let req: DocxRequest = serde_json::from_slice(body).map_err(|e| format!("请求不对：{e}"))?;
    let bytes = std::fs::read(&req.path).map_err(|e| format!("读不了 {}：{e}", req.path))?;
    let lib = |uids: &BTreeSet<String>| host.library(uids);
    if !refresh {
        return serde_json::to_value(check::verify(&bytes, &lib, &host.render)?).map_err(|e| e.to_string());
    }
    let out = req.out.ok_or("缺少 out（输出文件）")?;
    if Path::new(&out).exists() {
        return Err(format!("拒绝覆盖已存在的文件：{out}"));
    }
    if let Some(s) = req.style.as_deref().filter(|s| !styles::exists(&host.data_dir(), s)) {
        return Err(format!("没有这个样式：{s}"));
    }
    let r = docx::refresh(&bytes, req.style.as_deref(), &lib, &host.render, &mut || common::new_uid().unwrap_or_default(), &text::now_iso())?;
    std::fs::write(&out, &r.bytes).map_err(|e| format!("写不了 {out}：{e}"))?;
    Ok(json!({ "out": out, "style": r.plan.store.style, "citations": r.plan.citations.len(), "changed": r.plan.citations.iter().filter(|c| c.changed).count(), "issues": r.plan.issues.iter().map(|i| i.zh()).collect::<Vec<_>>() }))
}

/// Word 外转换 Zotero 引文（不改原文件，写到 out）
fn zotero_convert_file(app: &AppHandle, path: &Path, out: &Path) -> Result<convert::Report, String> {
    if out.exists() {
        return Err(format!("拒绝覆盖已存在的文件：{}", out.display()));
    }
    let word = app.state::<WordState>();
    let host = AppHost::new(app, &word);
    let bytes = std::fs::read(path).map_err(|e| format!("读不了 {}：{e}", path.display()))?;
    let dir = host.data_dir();
    let lib = |uids: &BTreeSet<String>| host.library(uids);
    let o = convert::Options {
        resolve: &|k| host.resolve(k),
        style_for_uri: &|u| styles::for_uri(&dir, u),
        default_style: styles::DEFAULT,
        library: &lib,
        render: &host.render,
        now: &text::now_iso(),
    };
    let r = convert::convert(&bytes, &o, &mut || common::new_uid().unwrap_or_default())?;
    std::fs::write(out, &r.bytes).map_err(|e| format!("写不了 {}：{e}", out.display()))?;
    Ok(r.report)
}

fn route(app: &AppHandle, req: service::Request) -> service::Response {
    use service::Response;
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v1/ping") => Response::text(200, format!("OK\tPith {}", version(app))),
        ("POST", "/v1/word") => {
            let word = app.state::<WordState>();
            Response::text(200, bridge::handle(&AppHost::new(app, &word), &req.body))
        }
        ("POST", "/v1/docx/refresh") | ("POST", "/v1/docx/verify") => match docx_call(app, &req.body, req.path.ends_with("refresh")) {
            Ok(v) => Response::json(&v),
            Err(e) => Response::json(&json!({ "error": e })),
        },
        ("POST", "/v1/docx/convert-zotero") => {
            let r = serde_json::from_slice::<DocxRequest>(&req.body)
                .map_err(|e| format!("请求不对：{e}"))
                .and_then(|r| zotero_convert_file(app, Path::new(&r.path), Path::new(r.out.as_deref().ok_or("缺少 out（输出文件）")?)));
            match r {
                Ok(v) => Response::json(&v),
                Err(e) => Response::json(&json!({ "error": e })),
            }
        }
        (_, "/v1/ping" | "/v1/word" | "/v1/docx/refresh" | "/v1/docx/verify" | "/v1/docx/convert-zotero") => Response::text(405, "method not allowed"),
        _ => Response::text(404, "not found"),
    }
}

// ---------- 命令 ----------

fn places(app: &AppHandle, state: &AppState) -> Result<install::Places, String> {
    let home = app.path().home_dir().map_err(|e| format!("找不到用户目录：{e}"))?;
    Ok(install::Places::for_home(&home, &state.paths.root))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordStatus {
    install: install::Status,
    word_installed: bool,
    service_port: Option<u16>,
    service_error: Option<String>,
    renderer_ready: bool,
    data_dir: String,
    styles: Vec<styles::Style>,
}

#[tauri::command(async)]
pub fn word_status(app: AppHandle, state: State<AppState>, word: State<WordState>) -> Result<WordStatus, String> {
    Ok(WordStatus {
        install: install::status(&places(&app, &state)?),
        word_installed: Path::new("/Applications/Microsoft Word.app").exists(),
        service_port: word.service.lock().ok().and_then(|g| g.as_ref().map(|s| s.port)),
        service_error: word.service_error.lock().ok().and_then(|g| g.clone()),
        renderer_ready: word.hub.is_ready(),
        data_dir: state.paths.root.display().to_string(),
        styles: styles::all(&state.paths.root),
    })
}

#[tauri::command(async)]
pub fn word_install(app: AppHandle, state: State<AppState>) -> Result<install::InstallReport, String> {
    let exe = std::env::current_exe().map_err(|e| format!("找不到 Pith 的可执行文件：{e}"))?;
    let cfg = install::BridgeConfig {
        data_dir: state.paths.root.clone(),
        launch: Some(install::Launch::from_exe(&exe)),
        data_env: std::env::var_os(paths::DATA_DIR_ENV).filter(|v| !v.is_empty()).map(|_| state.paths.root.clone()),
    };
    install::install(&places(&app, &state)?, &cfg, &version(&app), &text::now_iso())
}

/// 用户在「打开」对话框里选中了 Word 启动文件夹：把模板写进去
#[tauri::command(async)]
pub fn word_install_into(app: AppHandle, state: State<AppState>, dir: String) -> Result<install::InstallReport, String> {
    install::install_into(&places(&app, &state)?, Path::new(&dir), &version(&app), &text::now_iso())
}

#[tauri::command(async)]
pub fn word_uninstall(app: AppHandle, state: State<AppState>) -> Result<install::Status, String> {
    install::uninstall(&places(&app, &state)?)
}

/// 在访达中显示：`startup` Word 启动文件夹；`manual` 等待手动安装的模板
#[tauri::command(async)]
pub fn word_reveal(app: AppHandle, state: State<AppState>, which: String) -> Result<(), String> {
    let p = places(&app, &state)?;
    let r = match which.as_str() {
        "startup" => app.opener().open_path(p.startup.display().to_string(), None::<&str>),
        "manual" => app.opener().reveal_item_in_dir(p.own.join(install::TEMPLATE)),
        _ => return Err("不认识的位置".into()),
    };
    r.map_err(|e| format!("无法在访达中打开：{e}"))
}

// ---------- 置顶搜索窗 ----------

/// 搜索窗现在这一次（窗口刚加载好时取）
#[tauri::command]
pub fn picker_session(word: State<WordState>) -> Option<picker::Session> {
    word.picker.session()
}

#[tauri::command(async)]
pub fn picker_search(state: State<AppState>, q: String) -> Result<Vec<picker::PickInfo>, String> {
    state.with_db(|c| picker::search(c, &q))
}

/// 搜索窗选完 / 取消 / 删除这处引文
#[tauri::command]
pub fn picker_done(word: State<WordState>, session: u64, done: picker::Done) {
    let out = match done {
        picker::Done::Cancel => bridge::PickOutcome::Cancel,
        picker::Done::Remove => bridge::PickOutcome::Remove,
        picker::Done::Items { items } => bridge::PickOutcome::Items(items),
    };
    word.picker.finish(session, out);
}

// ---------- 引文样式 ----------

#[tauri::command(async)]
pub fn cite_styles_list(state: State<AppState>) -> Vec<styles::Style> {
    styles::all(&state.paths.root)
}

/// 导入 .csl：检查、复制进数据目录，再用合成文献试排一次；排不出来就删掉
#[tauri::command(async)]
pub fn cite_style_import(app: AppHandle, state: State<AppState>, word: State<WordState>, path: String) -> Result<styles::Imported, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("读不了 {path}：{e}"))?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err("文件太大，不像 CSL 样式（超过 2 MB）".into());
    }
    let xml = std::fs::read_to_string(&path).map_err(|e| format!("读不了 {path}：{e}"))?;
    let dir = state.paths.root.clone();
    let previous = styles::imported(&dir).into_iter().find(|s| docx_engine::cite::csl_style::read_meta(&xml).is_ok_and(|m| m.id == s.id));
    let old_xml = previous.as_ref().and_then(|s| std::fs::read_to_string(styles::dir(&dir).join(&s.file)).ok());
    let imported = styles::import(&dir, &xml)?;
    let lib = fixture::library();
    let ids = ["rl-item-0001", "rl-item-0003"];
    let req = RenderRequest {
        style: imported.style.id.clone(),
        items: ids.iter().map(|i| (i.to_string(), docx_engine::cite::plan::for_render(&lib[*i]))).collect(),
        citations: vec![RenderCitation { id: "test".into(), items: ids.iter().map(|i| CiteItemRef::new(*i)).collect() }],
    };
    if let Err(e) = (render::WebRender { app: &app, hub: &word.hub }).render(&req) {
        // 试排失败：恢复原样（替换的放回旧版本，新导入的删掉）
        match old_xml {
            Some(old) => {
                let _ = std::fs::write(styles::dir(&dir).join(&imported.style.file), old);
            }
            None => {
                let _ = styles::remove(&dir, &imported.style.id);
            }
        }
        return Err(format!("这个样式排不出引文，没有导入：{e}"));
    }
    Ok(imported)
}

#[tauri::command(async)]
pub fn cite_style_remove(state: State<AppState>, id: String) -> Result<(), String> {
    styles::remove(&state.paths.root, &id)
}

/// 前端排版要的导入样式（依赖样式换成母样式）
#[tauri::command(async)]
pub fn cite_style_source(state: State<AppState>, id: String) -> Result<styles::Source, String> {
    styles::source(&state.paths.root, &id)
}

// ---------- Word 外转换 Zotero 文档 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroPreview {
    found: convert::Found,
    /// 对应文献库的情况（还没排版）
    stats: docx_engine::cite::zotero::Stats,
    /// Zotero 的样式在 Pith 里对应的样式（中文名）；None：没有对应的
    style: Option<String>,
    /// 转换后写到哪里
    out: String,
}

/// 原文件旁边的新文件名：「论文（Pith）.docx」，有了就加编号
fn output_path(path: &Path) -> std::path::PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "文档".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}（Pith）.docx") } else { format!("{stem}（Pith {n}）.docx") };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

#[tauri::command(async)]
pub fn word_zotero_inspect(state: State<AppState>, path: String) -> Result<ZoteroPreview, String> {
    let p = Path::new(&path);
    let bytes = std::fs::read(p).map_err(|e| format!("读不了 {path}：{e}"))?;
    let found = convert::inspect(&bytes)?;
    let m = state.with_db(|c| Ok(docx_engine::cite::zotero::convert(&found.codes, &|k| csl::resolve_zotero(c, k).ok().flatten(), "")))?;
    let dir = &state.paths.root;
    let style = found.zotero_style.as_deref().and_then(|u| styles::for_uri(dir, u)).and_then(|id| styles::find(dir, &id)).map(|s| s.label);
    Ok(ZoteroPreview { stats: m.stats, style, out: output_path(p).display().to_string(), found })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroConverted {
    out: String,
    report: convert::Report,
}

/// 转换：原文件不动，写到旁边的新文件
#[tauri::command(async)]
pub fn word_zotero_convert(app: AppHandle, path: String) -> Result<ZoteroConverted, String> {
    let out = output_path(Path::new(&path));
    let report = zotero_convert_file(&app, Path::new(&path), &out)?;
    Ok(ZoteroConverted { out: out.display().to_string(), report })
}

/// 在访达中显示一个文件
#[tauri::command(async)]
pub fn word_reveal_file(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| format!("无法在访达中打开：{e}"))
}

/// 前端的排版监听挂好了
#[tauri::command]
pub fn cite_renderer_ready(word: State<WordState>) {
    word.hub.set_ready(true);
}

/// 前端排完一次（或出错）
#[tauri::command]
pub fn cite_render_done(word: State<WordState>, id: u64, result: Option<Rendered>, error: Option<String>) {
    word.hub.complete(id, result.ok_or_else(|| error.unwrap_or_else(|| "排版器没有返回结果".into())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::import::zotero::{self, Options};
    use crate::paths::DataPaths;
    use std::path::PathBuf;

    fn copy_dir() -> PathBuf {
        let src = PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"));
        let home = std::env::var("HOME").unwrap_or_default();
        assert!(!src.starts_with(Path::new(&home).join("Zotero")), "不能直接读 ~/Zotero，请用副本");
        src
    }

    fn import_into(root: &Path) -> rusqlite::Connection {
        let paths = DataPaths::new(root.to_path_buf());
        paths.ensure().unwrap();
        let mut c = db::open(&paths.library_db).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
        zotero::run(&c, &paths, &copy_dir(), &Options::default()).unwrap();
        c
    }

    /// 真实副本的每条文献都能换成 CSL-JSON；只打印聚合数字。默认忽略：
    /// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith real_copy_csl -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_copy_csl() {
        let dir = db::tests::temp_dir("word-real-csl");
        let c = import_into(&dir.join("data"));
        let uids: BTreeSet<String> = c.prepare("SELECT uid FROM items").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        let all = csl::items_csl(&c, &uids).unwrap();
        assert_eq!(all.len(), uids.len());
        let count = |f: &dyn Fn(&Value) -> bool| all.values().filter(|v| f(v)).count();
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        for v in all.values() {
            *types.entry(v["type"].as_str().unwrap_or("").to_string()).or_default() += 1;
        }
        println!("文献 {}；类型 {types:?}", all.len());
        println!(
            "有标题 {}，有作者 {}，有出版日期 {}，中文 {}，英文 {}，作者用单栏姓名 {}",
            count(&|v| v["title"].is_string()),
            count(&|v| v["author"].is_array()),
            count(&|v| v["issued"].is_object()),
            count(&|v| v["language"] == "zh-CN"),
            count(&|v| v["language"] == "en-US"),
            count(&|v| v["author"].as_array().is_some_and(|a| a.iter().any(|n| n["literal"].is_string()))),
        );
        assert!(all.values().all(|v| v["type"].is_string() && v["id"].is_string()));
    }

    /// 真实论文里的 Zotero 引文换成 Pith 引文（Word 外），用真实的排版器（Node 里跑 src/cite），逐处与 Zotero 原来显示的比；
    /// 再切换成 APA 刷新一次，两份结果都独立核对。只打印聚合数字与前几处不同；输出写到 `local/real-thesis/out/`。默认忽略：
    /// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith real_thesis_zotero -- --ignored --nocapture`
    /// （论文默认 `local/real-thesis/src/毕业论文_终稿.docx`，可用 `PITH_THESIS=<路径>` 换）
    #[test]
    #[ignore]
    fn real_thesis_zotero() {
        use docx_engine::cite::node::NodeRender;
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let thesis = std::env::var("PITH_THESIS").map(PathBuf::from).unwrap_or_else(|_| repo.join("local/real-thesis/src/毕业论文_终稿.docx"));
        let dir = db::tests::temp_dir("word-real-thesis");
        let data = dir.join("data");
        let c = import_into(&data);
        let bytes = std::fs::read(&thesis).unwrap();
        let render = NodeRender::in_repo();
        let lib = |uids: &BTreeSet<String>| csl::items_csl(&c, uids);
        let resolve = |k: &ItemKey| csl::resolve_zotero(&c, k).ok().flatten();
        let style_for_uri = |u: &str| styles::for_uri(&data, u);
        let o = convert::Options { resolve: &resolve, style_for_uri: &style_for_uri, default_style: styles::DEFAULT, library: &lib, render: &render, now: "2026-11-10T00:00:00Z" };
        let mut n = 0;
        let mut ids = || {
            n += 1;
            format!("real{n:03}")
        };
        let t = std::time::Instant::now();
        let r = convert::convert(&bytes, &o, &mut ids).unwrap();
        let rep = &r.report;
        println!("论文：{}（{:.1} 秒）", thesis.file_name().unwrap().to_string_lossy(), t.elapsed().as_secs_f32());
        println!("Zotero 样式 {:?} → Pith {}；{:?}", rep.zotero_style, rep.style, rep.style_note);
        println!("{:?}", rep.stats);
        println!("书目已转换 {}；留下的 Zotero 域 {}；提示 {} 条", rep.bibliography, rep.left, rep.notes.len());
        println!("引文文字与 Zotero 一致 {}/{}；书目条目一致 {}/{}（Pith {} 条）", rep.same_citations, rep.same_citations + rep.citation_diffs.len(), rep.same_entries, rep.zotero_entries, r.plan.bibliography.texts.len());
        for d in rep.citation_diffs.iter().take(5) {
            println!("  引文 {}：Zotero「{}」 Pith「{}」", d.index + 1, d.zotero, d.pith);
        }
        for d in rep.entry_diffs.iter().take(5) {
            println!("  书目 {}：\n    Zotero「{}」\n    Pith  「{}」", d.index + 1, d.zotero, d.pith);
        }
        // Zotero 写进文档的 itemData 与 Pith 换出的 CSL-JSON 逐字段比（找映射上的差别）
        let found = convert::inspect(&bytes).unwrap();
        let mut diff: BTreeMap<String, usize> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for code in &found.codes {
            for it in docx_engine::cite::zotero::parse_citation(code).unwrap().items {
                let (Some(k), Some(z)) = (it.key, it.data) else { continue };
                if !seen.insert(k.key.clone()) {
                    continue;
                }
                let uid = csl::resolve_zotero(&c, &k).unwrap().unwrap();
                let p = csl::items_csl(&c, &[uid.clone()].into_iter().collect()).unwrap().remove(&uid).unwrap();
                let (zo, po) = (z.as_object().unwrap(), p.as_object().unwrap());
                for key in zo.keys().chain(po.keys()).collect::<BTreeSet<_>>() {
                    if ["id", "source", "system_id"].contains(&key.as_str()) {
                        continue;
                    }
                    let tag = match (zo.get(key), po.get(key)) {
                        (Some(_), None) => "只有 Zotero",
                        (None, Some(_)) => "只有 Pith",
                        (Some(a), Some(b)) if a.to_string().replace('"', "") != b.to_string().replace('"', "") => "不同",
                        _ => continue,
                    };
                    *diff.entry(format!("{key}：{tag}")).or_default() += 1;
                }
            }
        }
        println!("文献 {} 条，字段差别：{diff:?}", seen.len());
        let lib2 = |uids: &BTreeSet<String>| csl::items_csl(&c, uids);
        let v = check::verify(&r.bytes, &lib2, &render).unwrap();
        assert!(v.ok, "{:?}", v.problems);
        let out_dir = repo.join("local/real-thesis/out");
        let stem = thesis.file_stem().unwrap().to_string_lossy().to_string();
        std::fs::write(out_dir.join(format!("{stem}-pith.docx")), &r.bytes).unwrap();
        // 切换成 APA
        let apa = docx::refresh(&r.bytes, Some("apa"), &lib2, &render, &mut || "x".into(), "2026-11-10T00:00:01Z").unwrap();
        let v = check::verify(&apa.bytes, &lib2, &render).unwrap();
        assert!(v.ok, "{:?}", v.problems);
        println!("切换 APA：改写 {} 处，书目 {} 条；第 1 处「{}」", apa.plan.citations.iter().filter(|u| u.changed).count(), apa.plan.bibliography.texts.len(), apa.plan.citations[0].text);
        std::fs::write(out_dir.join(format!("{stem}-pith-apa.docx")), &apa.bytes).unwrap();
        assert_eq!(rep.stats.converted, rep.stats.citations);
        assert!(rep.bibliography);
    }

    /// 准备端到端验收用的数据目录：把真实副本导入 `PITH_E2E_DIR`（必须是不存在或空的目录），留着不删。
    /// 然后 `PITH_DATA_DIR=<同一目录>` 启动 Pith。默认忽略：
    /// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy PITH_E2E_DIR=<目录> cargo test -p pith prepare_e2e_library -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn prepare_e2e_library() {
        let root = PathBuf::from(std::env::var("PITH_E2E_DIR").expect("请设置 PITH_E2E_DIR"));
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        assert!(root != home.join("Pith") && !root.starts_with(home.join("Pith/")), "不能用正式数据目录");
        assert!(!root.exists() || std::fs::read_dir(&root).unwrap().next().is_none(), "目录必须是空的");
        let c = import_into(&root);
        let n: i64 = c.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap();
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        println!("已导入 {n} 条文献到 {}", root.display());
    }
}
