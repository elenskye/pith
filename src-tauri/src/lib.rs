//! Pith 后端：唯一读写 SQLite 的地方；v0.2 起还有 Word 集成（`word`：本机服务、引文排版桥、模板安装）
//! 和排版 Agent（`agent`：调用 Claude 提炼要求、出文档计划，其余交给文档引擎）。
//! Tauri 命令是界面（以及以后的 Agent 工具）共用的应用服务层，命名「模块_动作」；
//! 返回 `Result<T, String>`，错误信息是给用户看的中文。

mod agent;
mod backup;
mod common;
mod db;
mod dedup;
mod edit;
mod fields;
mod import;
mod library;
mod metadata;
mod paths;
mod protocol;
mod reader;
mod search;
mod store;
mod text;
mod transfer;
mod word;

use paths::DataPaths;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    paths: DataPaths,
    /// 主库；打不开 / 升级失败时是原因（界面显示它，而不是崩溃；这时仍可从备份恢复）
    db: Mutex<Result<Connection, String>>,
    /// 库被整个换掉（恢复）一次加一：换之前开始的后台补全不再往新库里写
    epoch: AtomicU64,
    /// 联网补全的后台队列
    lookups: metadata::Queue,
}

/// 搜索索引是派生数据：挂不上或重建失败都不挡启动，搜索时再报错
fn attach_search(conn: &Connection, paths: &DataPaths) {
    match search::attach(conn, &paths.search_db).and_then(|_| search::ensure_fresh(conn)) {
        Ok(true) => eprintln!("搜索索引已重建"),
        Ok(false) => {}
        Err(e) => eprintln!("搜索索引不可用：{e}"),
    }
}

impl AppState {
    /// 打开（必要时新建）library.db，执行升级；已有数据库时做当天的自动备份，并清理谁都不用的附件文件。
    pub fn init(paths: DataPaths) -> Self {
        let existed = paths.library_db.exists();
        let opened = paths.ensure().and_then(|_| {
            let mut conn = db::open(&paths.library_db)?;
            db::migrate(&mut conn, db::MIGRATIONS, &paths.backups)?;
            attach_search(&conn, &paths);
            if existed {
                // 尽力而为：自动备份失败不挡启动，设置页能看到备份列表
                match backup::daily(&conn, &paths.backups) {
                    // 每天一次：旧备份清理之后，只有它们用到的附件文件才删
                    Ok(Some(_)) => {
                        if let Err(e) = backup::sweep_files(&conn, &paths.backups, &paths.attachments) {
                            eprintln!("清理附件文件失败：{e}");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("自动备份失败：{e}"),
                }
            }
            Ok(conn)
        });
        Self { paths, db: Mutex::new(opened), epoch: AtomicU64::new(0), lookups: metadata::Queue::default() }
    }

    fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.db.lock().map_err(|_| "数据库连接异常，请重启 App".to_string())?;
        match guard.as_mut() {
            Ok(conn) => f(conn),
            Err(e) => Err(format!("数据库没有打开：{e}")),
        }
    }

    fn startup_error(&self) -> Option<String> {
        self.db.lock().ok().and_then(|g| g.as_ref().err().cloned())
    }

    /// 用准备好的来源替换整个库：停下联网补全 →「恢复前」备份 → 整库复制 → 重建搜索索引。
    /// 主库打不开时，把原文件挪进备份目录（记为「恢复前」），在新建的库上恢复；失败就放回去。
    /// 退出前：停下后台补全，把 WAL 日志并回 library.db / search.db。
    /// 否则退出后的内容可能还在 -wal 文件里，只拷走 library.db 会丢数据（正式版实测发现）。
    fn close(&self) {
        self.lookups.cancel();
        if let Ok(guard) = self.db.lock()
            && let Ok(c) = guard.as_ref()
        {
            let _ = c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            let _ = c.execute_batch("PRAGMA search.wal_checkpoint(TRUNCATE);");
        }
    }

    fn replace_library(&self, prepared: transfer::Prepared) -> Result<transfer::RestoreReport, String> {
        self.lookups.cancel();
        let mut guard = self.db.lock().map_err(|_| "数据库连接异常，请重启 App".to_string())?;
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let p = &self.paths;
        let report = match guard.as_mut() {
            Ok(live) => {
                let safety = backup::snapshot(live, &p.backups, backup::BEFORE_RESTORE)?;
                transfer::apply(live, &p.attachments, prepared, safety.file_name().map(|f| f.to_string_lossy().into_owned()))?
            }
            Err(_) => {
                p.ensure()?;
                let aside = backup::set_aside(&p.library_db, &p.backups)?;
                let restored = db::open(&p.library_db).and_then(|mut live| {
                    let name = aside.as_ref().and_then(|a| a.file_name()).map(|f| f.to_string_lossy().into_owned());
                    let r = transfer::apply(&mut live, &p.attachments, prepared, name)?;
                    Ok((live, r))
                });
                match restored {
                    Ok((live, r)) => {
                        if let Err(e) = search::attach(&live, &p.search_db) {
                            eprintln!("搜索索引不可用：{e}");
                        }
                        *guard = Ok(live);
                        r
                    }
                    Err(e) => {
                        if let Some(a) = &aside {
                            backup::put_back(a, &p.library_db)?;
                        }
                        return Err(e);
                    }
                }
            }
        };
        let live = guard.as_ref().map_err(|e| e.clone())?;
        if let Err(e) = backup::prune_safety(&p.backups, backup::KEEP_SAFETY) {
            eprintln!("清理旧备份失败：{e}");
        }
        // 内容整个换了：搜索索引重建
        if let Err(e) = search::rebuild(live) {
            eprintln!("重建搜索索引失败：{e}");
        }
        Ok(report)
    }
}

/// 后台补全线程看到的库：开始之后库被恢复（换掉）了，就不再读写
struct WorkerDb<'a> {
    state: &'a AppState,
    epoch: u64,
}

impl metadata::Db for WorkerDb<'_> {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.state.with_db(|c| if self.state.epoch.load(Ordering::SeqCst) == self.epoch { f(c) } else { Err("文献库已恢复成别的版本，补全停止".into()) })
    }
}

/// 放进联网补全队列；队列空闲时起一个后台线程查（一次一条，查的时候不占数据库锁）。
fn enqueue_lookups(app: &tauri::AppHandle, items: &[i64], trigger: &str) {
    let state = app.state::<AppState>();
    if state.lookups.push(items, trigger) {
        let app = app.clone();
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            let db = WorkerDb { state: &state, epoch: state.epoch.load(Ordering::SeqCst) };
            let email = state.with_db(|c| metadata::settings_get(c)).ok().and_then(|s| s.email);
            let http = metadata::net::Ureq::new(email.as_deref());
            metadata::work(&db, &http, &mut metadata::net::Pacer::default(), &state.lookups, Some(&state.paths.backups));
        });
    }
}

/// 导入完成后：打开了自动补全就把这次导入的文献排进队列
fn lookups_after_import(app: &tauri::AppHandle, state: &AppState, run_id: i64) {
    let items = state.with_db(|c| if metadata::auto_allowed(c) { metadata::run_items(c, run_id) } else { Ok(Vec::new()) });
    match items {
        Ok(items) => enqueue_lookups(app, &items, "import"),
        Err(e) => eprintln!("排队联网补全失败：{e}"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    app_version: String,
    is_dev: bool,
    data_dir: String,
    schema_version: Option<i64>,
    latest_schema: i64,
    startup_error: Option<String>,
}

// 数据库命令一律 `async`（在线程池里跑）：导入要几秒，期间界面的其他请求排队等锁，不卡主线程。

#[tauri::command(async)]
fn app_info(app: tauri::AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        app_version: app.package_info().version.to_string(),
        is_dev: paths::is_dev(),
        data_dir: state.paths.root.display().to_string(),
        schema_version: state.with_db(|c| db::user_version(c)).ok(),
        latest_schema: db::latest(),
        startup_error: state.startup_error(),
    }
}

#[tauri::command]
fn app_reveal_data_dir(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    app.opener().open_path(state.paths.root.display().to_string(), None::<&str>).map_err(|e| format!("无法打开数据目录：{e}"))
}

#[tauri::command(async)]
fn library_overview(state: State<AppState>) -> Result<library::Overview, String> {
    state.with_db(|c| library::overview(c))
}

#[tauri::command(async)]
fn items_list(state: State<AppState>, query: library::ListQuery) -> Result<Vec<library::ListRow>, String> {
    state.with_db(|c| library::items_list(c, &query))
}

#[tauri::command(async)]
fn tags_list(state: State<AppState>) -> Result<Vec<library::TagCount>, String> {
    state.with_db(|c| library::tags_list(c))
}

/// 改完库之后更新搜索索引。索引是派生数据：失败只记日志（下次启动会检查重建），不让已经成功的修改报错。
fn reindex(c: &Connection, refs: &[search::DocRef]) {
    if let Err(e) = search::update(c, refs) {
        eprintln!("更新搜索索引失败：{e}");
    }
}

/// 执行一个会改库的操作，并按它返回的文档更新索引。
fn mutate<T>(state: &AppState, f: impl FnOnce(&Connection) -> Result<(T, Vec<search::DocRef>), String>) -> Result<T, String> {
    state.with_db(|c| {
        let (out, refs) = f(c)?;
        reindex(c, &refs);
        Ok(out)
    })
}

#[tauri::command(async)]
fn items_create(state: State<AppState>, item_type: String, collection_id: Option<i64>) -> Result<i64, String> {
    mutate(&state, |c| edit::item_create(c, &item_type, collection_id).map(|id| (id, vec![search::DocRef::item(id)])))
}

#[tauri::command(async)]
fn items_update(app: tauri::AppHandle, state: State<AppState>, id: i64, patch: edit::ItemPatch) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::item_update(c, id, &patch)?)))?;
    // 填了 / 改了 DOI、ISBN（或 extra 里的「DOI: …」）：打开了自动补全就去查（标识没变的不会重复查）
    let touched = patch.fields.iter().any(|(k, v)| ["DOI", "ISBN", "extra"].contains(&k.as_str()) && !v.trim().is_empty());
    if touched && state.with_db(|c| Ok(metadata::auto_allowed(c)))? {
        enqueue_lookups(&app, &[id], "edit");
    }
    Ok(())
}

#[tauri::command(async)]
fn items_set_field(state: State<AppState>, ids: Vec<i64>, field: String, value: String) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::items_set_field(c, &ids, &field, &value)?)))
}

#[tauri::command(async)]
fn items_merge(state: State<AppState>, master_id: i64, other_ids: Vec<i64>) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::items_merge(c, master_id, &other_ids)?)))
}

#[tauri::command(async)]
fn duplicates_dismiss(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_db(|c| dedup::dismiss(c, &ids))
}

#[tauri::command(async)]
fn attachments_add(state: State<AppState>, item_id: Option<i64>, path: String) -> Result<i64, String> {
    mutate(&state, |c| edit::attachment_add(c, &state.paths.attachments, item_id, std::path::Path::new(&path)))
}

#[tauri::command(async)]
fn attachments_rename(state: State<AppState>, id: i64, title: String) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::attachment_rename(c, id, &title)?)))
}

#[tauri::command(async)]
fn tags_add(state: State<AppState>, targets: Vec<edit::Target>, name: String) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::tags_add(c, &targets, &name)?)))
}

#[tauri::command(async)]
fn tags_remove(state: State<AppState>, targets: Vec<edit::Target>, name: String) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::tags_remove(c, &targets, &name)?)))
}

#[tauri::command(async)]
fn tags_rename(state: State<AppState>, id: i64, name: String) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::tag_rename(c, id, &name)?)))
}

#[tauri::command(async)]
fn tags_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::tag_delete(c, id)?)))
}

#[tauri::command(async)]
fn collections_create(state: State<AppState>, name: String, parent_id: Option<i64>) -> Result<i64, String> {
    state.with_db(|c| edit::collection_create(c, &name, parent_id))
}

#[tauri::command(async)]
fn collections_rename(state: State<AppState>, id: i64, name: String) -> Result<(), String> {
    state.with_db(|c| edit::collection_rename(c, id, &name))
}

#[tauri::command(async)]
fn collections_move(state: State<AppState>, id: i64, parent_id: Option<i64>) -> Result<(), String> {
    state.with_db(|c| edit::collection_move(c, id, parent_id))
}

#[tauri::command(async)]
fn collections_add(state: State<AppState>, id: i64, targets: Vec<edit::Target>) -> Result<usize, String> {
    state.with_db(|c| edit::collection_add(c, id, &targets))
}

#[tauri::command(async)]
fn collections_remove(state: State<AppState>, id: i64, targets: Vec<edit::Target>) -> Result<usize, String> {
    state.with_db(|c| edit::collection_remove(c, id, &targets))
}

#[tauri::command(async)]
fn trash_move(state: State<AppState>, targets: Vec<edit::Target>) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::trash(c, &targets)?)))
}

#[tauri::command(async)]
fn trash_restore(state: State<AppState>, targets: Vec<edit::Target>) -> Result<(), String> {
    mutate(&state, |c| Ok(((), edit::restore(c, &targets)?)))
}

#[tauri::command(async)]
fn trash_delete(state: State<AppState>, targets: Vec<edit::Target>) -> Result<edit::DeleteReport, String> {
    mutate(&state, |c| edit::delete_forever(c, &state.paths.backups, &state.paths.attachments, &targets))
}

#[tauri::command(async)]
fn trash_empty(state: State<AppState>) -> Result<edit::DeleteReport, String> {
    mutate(&state, |c| edit::empty_trash(c, &state.paths.backups, &state.paths.attachments))
}

#[tauri::command(async)]
fn metadata_settings_get(state: State<AppState>) -> Result<metadata::Settings, String> {
    state.with_db(|c| metadata::settings_get(c))
}

#[tauri::command(async)]
fn metadata_settings_set(state: State<AppState>, patch: metadata::SettingsPatch) -> Result<metadata::Settings, String> {
    state.with_db(|c| metadata::settings_set(c, &patch))
}

#[tauri::command(async)]
fn metadata_status(state: State<AppState>) -> metadata::Progress {
    state.lookups.status()
}

#[tauri::command(async)]
fn metadata_cancel(state: State<AppState>) {
    state.lookups.cancel();
}

/// 手动联网补全（单条或批量）：放进队列，返回排了几条
#[tauri::command(async)]
fn metadata_lookup(app: tauri::AppHandle, state: State<AppState>, ids: Vec<i64>) -> Result<usize, String> {
    state.with_db(|c| metadata::require_enabled(c).map(|_| ()))?;
    enqueue_lookups(&app, &ids, "manual");
    Ok(ids.len())
}

/// 某次导入的文献（第一次同意联网后补查用）
#[tauri::command(async)]
fn metadata_lookup_run(app: tauri::AppHandle, state: State<AppState>, run_id: i64) -> Result<usize, String> {
    let items = state.with_db(|c| {
        metadata::require_enabled(c)?;
        metadata::run_items(c, run_id)
    })?;
    enqueue_lookups(&app, &items, "import");
    Ok(items.len())
}

/// 全部有 DOI / ISBN 的文献（已经查过的跳过），返回排了几条
#[tauri::command(async)]
fn metadata_lookup_all(app: tauri::AppHandle, state: State<AppState>) -> Result<usize, String> {
    let items = state.with_db(|c| {
        metadata::require_enabled(c)?;
        metadata::all_items(c)
    })?;
    enqueue_lookups(&app, &items, "import");
    Ok(items.len())
}

#[tauri::command(async)]
fn metadata_latest(state: State<AppState>, item_id: i64) -> Result<Option<metadata::LookupView>, String> {
    state.with_db(|c| metadata::latest(c, item_id))
}

#[tauri::command(async)]
fn metadata_resolve(state: State<AppState>, lookup_id: i64, field: String, accept: bool) -> Result<metadata::LookupView, String> {
    mutate(&state, |c| metadata::resolve(c, lookup_id, &field, accept))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UndoResult {
    lookup: metadata::LookupView,
    reverted: usize,
}

#[tauri::command(async)]
fn metadata_undo(state: State<AppState>, lookup_id: i64) -> Result<UndoResult, String> {
    mutate(&state, |c| metadata::undo(c, lookup_id).map(|(lookup, reverted, refs)| (UndoResult { lookup, reverted }, refs)))
}

/// 按 DOI / ISBN 新建文献：读库挑出已有的 → 联网逐个查（不占数据库锁）→ 逐个建。
#[tauri::command(async)]
fn metadata_add(state: State<AppState>, text: String, collection_id: Option<i64>) -> Result<metadata::AddReport, String> {
    let (todo, mut report) = state.with_db(|c| metadata::add_prepare(c, &text))?;
    let email = state.with_db(|c| metadata::settings_get(c))?.email;
    let http = metadata::net::Ureq::new(email.as_deref());
    let mut pacer = metadata::net::Pacer::default();
    for (i, id) in todo.iter().enumerate() {
        match metadata::fetch(&http, &mut pacer, std::slice::from_ref(id)) {
            Ok(metadata::Fetched::Found { found, .. }) => {
                let item = mutate(&state, |c| metadata::add_create(c, collection_id, id, &found))?;
                report.created.push(item);
            }
            Ok(metadata::Fetched::NotFound { message, .. }) => report.not_found.push(metadata::Missed { identifier: id.label(), message }),
            Err(e) => {
                let rest = todo.len() - i - 1;
                report.failed = Some(if rest > 0 { format!("{}：{}（后面 {rest} 个没有再查）", id.label(), e.message()) } else { format!("{}：{}", id.label(), e.message()) });
                break;
            }
        }
    }
    Ok(report)
}

#[tauri::command(async)]
fn search_rebuild(state: State<AppState>) -> Result<usize, String> {
    state.with_db(|c| search::rebuild(c))
}

#[tauri::command(async)]
fn items_get(state: State<AppState>, id: i64) -> Result<library::ItemDetail, String> {
    state.with_db(|c| library::item_get(c, id))
}

#[tauri::command(async)]
fn attachments_get(state: State<AppState>, id: i64) -> Result<library::AttachmentDetail, String> {
    state.with_db(|c| library::attachment_get(c, id))
}

/// 用系统默认程序打开附件（文件库里的文件只读）；网址链接在浏览器里打开。
#[tauri::command(async)]
fn attachments_open(app: tauri::AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    match state.with_db(|c| library::attachment_target(c, &state.paths.attachments, id))? {
        library::Openable::File(p) => app.opener().open_path(p.display().to_string(), None::<&str>),
        library::Openable::Url(u) => app.opener().open_url(u, None::<&str>),
    }
    .map_err(|e| format!("打不开：{e}"))
}

/// 文件里的网址链接（PDF 里的超链接等）交给系统浏览器 / 邮件程序；只认 http(s) 与 mailto。
#[tauri::command(async)]
fn app_open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) {
        return Err("只能打开网址或邮件地址".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| format!("打不开：{e}"))
}

#[tauri::command(async)]
fn reader_get(state: State<AppState>, id: i64) -> Result<reader::ReaderDoc, String> {
    state.with_db(|c| reader::reader_get(c, id))
}

#[tauri::command(async)]
fn reader_save_state(state: State<AppState>, id: i64, reading: serde_json::Value) -> Result<(), String> {
    state.with_db(|c| reader::reader_save_state(c, id, &reading))
}

#[tauri::command(async)]
fn annotations_list(state: State<AppState>, attachment_id: i64) -> Result<Vec<reader::Annotation>, String> {
    state.with_db(|c| reader::annotations_list(c, attachment_id))
}

#[tauri::command(async)]
fn annotations_create(state: State<AppState>, input: reader::NewAnnotation) -> Result<reader::Annotation, String> {
    mutate(&state, |c| reader::annotation_create(c, &input))
}

#[tauri::command(async)]
fn annotations_update(state: State<AppState>, id: i64, patch: reader::AnnotationPatch) -> Result<reader::Annotation, String> {
    mutate(&state, |c| reader::annotation_update(c, id, &patch))
}

#[tauri::command(async)]
fn annotations_delete(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    mutate(&state, |c| Ok(((), reader::annotations_delete(c, &ids)?)))
}

#[tauri::command(async)]
fn annotations_restore(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    mutate(&state, |c| Ok(((), reader::annotations_restore(c, &ids)?)))
}

/// `rl-file://localhost/<附件 id>`：查库时拿锁，读文件时不拿。
fn serve_file(app: &tauri::AppHandle, req: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Method;
    let state = app.state::<AppState>();
    let header = |k: &str| req.headers().get(k).and_then(|v| v.to_str().ok());
    let reply = match *req.method() {
        Method::OPTIONS => protocol::Reply {
            status: 204,
            headers: vec![("Access-Control-Allow-Methods", "GET, HEAD".into()), ("Access-Control-Allow-Headers", "Range".into())],
            body: Vec::new(),
        },
        Method::GET | Method::HEAD => match state.with_db(|c| Ok(protocol::locate(c, &state.paths.attachments, req.uri().path()))) {
            Err(e) => protocol::Reply::error(503, &e),
            Ok(Err(r)) => r,
            Ok(Ok(found)) => protocol::read(&found, header("Range"), req.method() == Method::HEAD),
        },
        _ => protocol::Reply::error(405, "只支持 GET / HEAD"),
    };
    let mut b = tauri::http::Response::builder().status(reply.status);
    for (k, v) in reply.headers {
        b = b.header(k, v);
    }
    if let Some(o) = header("Origin").filter(|o| protocol::allowed_origin(o, paths::is_dev())) {
        b = b.header("Access-Control-Allow-Origin", o).header("Vary", "Origin");
    }
    b.body(reply.body).unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[tauri::command(async)]
fn notes_get(state: State<AppState>, id: i64) -> Result<library::NoteDetail, String> {
    state.with_db(|c| library::note_get(c, id))
}

#[tauri::command(async)]
fn collections_list(state: State<AppState>) -> Result<Vec<library::CollectionNode>, String> {
    state.with_db(|c| library::collections_list(c))
}

#[tauri::command(async)]
fn collections_get(state: State<AppState>, id: i64) -> Result<library::CollectionDetail, String> {
    state.with_db(|c| library::collection_get(c, id))
}

#[tauri::command(async)]
fn backups_list(state: State<AppState>) -> Result<Vec<backup::BackupInfo>, String> {
    backup::list(&state.paths.backups)
}

#[tauri::command(async)]
fn backups_create(state: State<AppState>) -> Result<backup::BackupInfo, String> {
    let path = state.with_db(|c| backup::snapshot(c, &state.paths.backups, backup::MANUAL))?;
    backup::info_of(&path)
}

/// 恢复到某份备份（主库打不开时也行）。准备（复制、检查、升级）不占锁。
#[tauri::command(async)]
fn backups_restore(state: State<AppState>, file_name: String) -> Result<transfer::RestoreReport, String> {
    let prepared = transfer::prepare_backup(&state.paths.backups, &file_name)?;
    state.replace_library(prepared)
}

/// 整库导出到 `dir` 下的新文件夹：在锁里读出全部表，写文件与自检不占锁。
#[tauri::command(async)]
fn export_create(app: tauri::AppHandle, state: State<AppState>, dir: String) -> Result<transfer::ExportReport, String> {
    let snap = state.with_db(|c| transfer::snapshot(c))?;
    transfer::export(&snap, &state.paths.attachments, std::path::Path::new(&dir), &app.package_info().version.to_string())
}

/// 在访达中显示一个整库导出（只认里面有 library.json 的文件夹）
#[tauri::command(async)]
fn export_reveal(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.join(transfer::JSON_NAME).is_file() {
        return Err("这不是整库导出的文件夹".into());
    }
    app.opener().reveal_item_in_dir(p).map_err(|e| format!("无法打开访达：{e}"))
}

#[tauri::command(async)]
fn export_inspect(state: State<AppState>, dir: String) -> Result<transfer::ExportPreview, String> {
    transfer::inspect(std::path::Path::new(&dir), &state.paths.attachments)
}

/// 从整库导出恢复（替换整个库）。读、核对、重建临时库不占锁。
#[tauri::command(async)]
fn export_restore(state: State<AppState>, dir: String) -> Result<transfer::RestoreReport, String> {
    let prepared = transfer::prepare_export(std::path::Path::new(&dir), Some(&state.paths.attachments), false)?;
    state.replace_library(prepared)
}

fn zotero_options(base_dir: Option<String>) -> import::zotero::Options {
    import::zotero::Options { base_dir: base_dir.filter(|b| !b.is_empty()).map(Into::into) }
}

#[tauri::command(async)]
fn import_zotero_preview(state: State<AppState>, dir: String, base_dir: Option<String>) -> Result<import::zotero::Preview, String> {
    state.with_db(|c| import::zotero::preview(c, std::path::Path::new(&dir), &zotero_options(base_dir)))
}

#[tauri::command(async)]
fn import_zotero_run(app: tauri::AppHandle, state: State<AppState>, dir: String, base_dir: Option<String>) -> Result<import::RunDetail, String> {
    let run = state.with_db(|c| {
        let run = import::zotero::run(c, &state.paths, std::path::Path::new(&dir), &zotero_options(base_dir))?;
        if let Err(e) = search::update_run(c, run.summary.id) {
            eprintln!("更新搜索索引失败：{e}");
        }
        Ok(run)
    })?;
    lookups_after_import(&app, &state, run.summary.id);
    Ok(run)
}

#[tauri::command(async)]
fn import_file_preview(state: State<AppState>, path: String) -> Result<import::file::Preview, String> {
    state.with_db(|c| import::file::preview(c, std::path::Path::new(&path)))
}

#[tauri::command(async)]
fn import_file_run(app: tauri::AppHandle, state: State<AppState>, path: String, skip_duplicates: bool, into_collection: bool) -> Result<import::RunDetail, String> {
    let opts = import::file::Options { skip_duplicates, into_collection };
    let run = state.with_db(|c| {
        let run = import::file::run(c, &state.paths, std::path::Path::new(&path), &opts)?;
        if let Err(e) = search::update_run(c, run.summary.id) {
            eprintln!("更新搜索索引失败：{e}");
        }
        Ok(run)
    })?;
    lookups_after_import(&app, &state, run.summary.id);
    Ok(run)
}

#[tauri::command(async)]
fn import_runs_list(state: State<AppState>) -> Result<Vec<import::RunSummary>, String> {
    state.with_db(|c| import::runs_list(c))
}

#[tauri::command(async)]
fn import_runs_get(state: State<AppState>, id: i64) -> Result<import::RunDetail, String> {
    state.with_db(|c| import::runs_get(c, id))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || responder.respond(serve_file(&app, &request)));
        })
        .setup(|app| {
            let home = app.path().home_dir()?;
            let root = paths::resolve_root(&home, std::env::var_os(paths::DATA_DIR_ENV).map(Into::into));
            app.manage(AppState::init(DataPaths::new(root)));
            // Word 集成：本机服务（Word 宏经它调用 Pith）
            app.manage(word::WordState::default());
            // 排版 Agent：密钥、进行中的调用
            agent::manage(app);
            word::start_service(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            app_reveal_data_dir,
            library_overview,
            items_list,
            tags_list,
            items_get,
            items_create,
            items_update,
            items_set_field,
            items_merge,
            duplicates_dismiss,
            attachments_add,
            attachments_rename,
            tags_add,
            tags_remove,
            tags_rename,
            tags_delete,
            collections_create,
            collections_rename,
            collections_move,
            collections_add,
            collections_remove,
            trash_move,
            trash_restore,
            trash_delete,
            trash_empty,
            search_rebuild,
            metadata_settings_get,
            metadata_settings_set,
            metadata_status,
            metadata_cancel,
            metadata_lookup,
            metadata_lookup_run,
            metadata_lookup_all,
            metadata_latest,
            metadata_resolve,
            metadata_undo,
            metadata_add,
            attachments_get,
            attachments_open,
            app_open_url,
            reader_get,
            reader_save_state,
            annotations_list,
            annotations_create,
            annotations_update,
            annotations_delete,
            annotations_restore,
            notes_get,
            collections_list,
            collections_get,
            backups_list,
            backups_create,
            backups_restore,
            export_create,
            export_reveal,
            export_inspect,
            export_restore,
            import_zotero_preview,
            import_zotero_run,
            import_file_preview,
            import_file_run,
            import_runs_list,
            import_runs_get,
            word::word_status,
            word::word_install,
            word::word_install_into,
            word::word_uninstall,
            word::word_reveal,
            word::cite_renderer_ready,
            word::cite_render_done,
            word::picker_session,
            word::picker_search,
            word::picker_done,
            word::cite_styles_list,
            word::cite_style_import,
            word::cite_style_remove,
            word::cite_style_source,
            word::word_zotero_inspect,
            word::word_zotero_convert,
            word::word_reveal_file,
            agent::agent_status,
            agent::agent_key_set,
            agent::agent_key_clear,
            agent::agent_budget_set,
            agent::agent_send_mode_set,
            agent::format_rulesets_list,
            agent::format_ruleset_get,
            agent::format_ruleset_delete,
            agent::format_ruleset_rename,
            agent::format_extract_prepare,
            agent::format_extract_run,
            agent::format_requirement_set,
            agent::format_plan_prepare,
            agent::format_plan_run,
            agent::format_run_get,
            agent::format_run_delete,
            agent::format_plan_update,
            agent::format_apply,
            agent::format_cancel
        ])
        .build(tauri::generate_context!())
        .expect("Pith 启动失败")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                word::stop_service(app);
                if let Some(state) = app.try_state::<AppState>() {
                    state.close();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::tests::temp_dir;

    #[test]
    fn first_start_creates_library_without_backup() {
        let dir = temp_dir("init-fresh");
        let state = AppState::init(DataPaths::new(dir.join("data")));
        assert!(state.startup_error().is_none(), "{:?}", state.startup_error());
        assert!(dir.join("data/library.db").exists() && dir.join("data/attachments").is_dir());
        assert_eq!(state.with_db(|c| db::user_version(c)).unwrap(), db::latest());
        assert!(backup::list(&dir.join("data/backups")).unwrap().is_empty(), "新库不用备份");
    }

    #[test]
    fn later_start_takes_daily_backup_once() {
        let dir = temp_dir("init-daily");
        drop(AppState::init(DataPaths::new(dir.join("data"))));
        drop(AppState::init(DataPaths::new(dir.join("data"))));
        let state = AppState::init(DataPaths::new(dir.join("data")));
        let list = backup::list(&dir.join("data/backups")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, backup::DAILY);
        assert!(state.with_db(|c| library::overview(c)).is_ok());
    }

    fn count(state: &AppState, sql: &str) -> i64 {
        state.with_db(|c| c.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap()
    }

    fn restore(state: &AppState, file_name: &str) -> Result<transfer::RestoreReport, String> {
        state.replace_library(transfer::prepare_backup(&state.paths.backups, file_name)?)
    }

    /// 合成库 → 手动备份 → 改动（新建、永久删除带 PDF 的文献）→ 恢复备份：数据回到备份时，附件文件还在，搜索跟着变；
    /// 「恢复前」备份能把改动找回来。
    #[test]
    fn close_leaves_a_self_contained_library_db() {
        let lib = transfer::tests::rich_library("lib-close");
        let paths = lib.paths.clone();
        drop(lib.c);
        let state = AppState::init(paths.clone());
        let items = count(&state, "SELECT COUNT(*) FROM items");
        state.with_db(|c| edit::item_create(c, "book", None)).unwrap();
        let wal = |db: &std::path::Path| std::fs::metadata(db.with_extension("db-wal")).map_or(0, |m| m.len());
        assert!(wal(&paths.library_db) > 0, "改动先写进 WAL");
        state.close();
        assert_eq!(wal(&paths.library_db), 0);
        assert_eq!(wal(&paths.search_db), 0);
        // 只拿 library.db 本身（不带 -wal）也是完整的
        let copy = paths.root.join("copy.db");
        std::fs::copy(&paths.library_db, &copy).unwrap();
        let c = Connection::open(&copy).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, items + 1);
    }

    #[test]
    fn restore_backup_round_trip() {
        let lib = transfer::tests::rich_library("lib-restore");
        let paths = lib.paths.clone();
        drop(lib.c);
        let state = AppState::init(paths.clone());
        let manual = state.with_db(|c| backup::snapshot(c, &paths.backups, backup::MANUAL)).unwrap();
        let manual = manual.file_name().unwrap().to_string_lossy().into_owned();
        let items = count(&state, "SELECT COUNT(*) FROM items");

        // 改动：新建一条；永久删除 JRNL0001（带 PDF）
        let new_id = state.with_db(|c| edit::item_create(c, "book", None)).unwrap();
        let (j1, sha): (i64, String) = state
            .with_db(|c| {
                c.query_row(
                    "SELECT i.id, a.sha256 FROM items i JOIN attachments a ON a.item_id = i.id WHERE i.source_key = 'JRNL0001' AND a.sha256 IS NOT NULL",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| e.to_string())
            })
            .unwrap();
        state.with_db(|c| edit::trash(c, &[edit::Target::item(j1)])).unwrap();
        state.with_db(|c| edit::delete_forever(c, &paths.backups, &paths.attachments, &[edit::Target::item(j1)])).unwrap();
        assert_eq!(count(&state, "SELECT COUNT(*) FROM items"), items);
        let hits = |q: &str| state.with_db(|c| search::query(c, q)).unwrap().map_or(0, |m| m.len());
        let title: String = state.with_db(|c| c.query_row("SELECT title FROM items WHERE id = 1", [], |r| r.get(0)).map_err(|e| e.to_string())).unwrap();

        let r = restore(&state, &manual).unwrap();
        assert_eq!((r.from_schema, r.to_schema, r.missing_files), (db::latest(), db::latest(), 0), "删掉的 PDF 还在文件库里");
        assert_eq!(count(&state, "SELECT COUNT(*) FROM items"), items);
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {new_id}")), 0, "备份之后新建的没了");
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {j1} AND deleted_at IS NULL")), 1, "永久删除的回来了");
        let att = count(&state, &format!("SELECT id FROM attachments WHERE sha256 = '{sha}'"));
        assert!(state.with_db(|c| Ok(protocol::locate(c, &paths.attachments, &format!("/{att}")))).unwrap().is_ok(), "阅读器能打开它的 PDF");
        assert!(hits(&title) >= 1, "搜索索引重建了");
        let safety = r.safety_backup.clone().unwrap();
        assert!(safety.ends_with("-before-restore.db"));
        assert!(state.startup_error().is_none());

        // 再恢复「恢复前」那份：改动回来
        restore(&state, &safety).unwrap();
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {new_id}")), 1);
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {j1}")), 0);

        // 坏的备份：主库不变
        std::fs::write(paths.backups.join("library-20260101-080000-manual.db"), b"broken").unwrap();
        assert!(restore(&state, "library-20260101-080000-manual.db").is_err());
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {new_id}")), 1);
    }

    /// 主库打不开（损坏）时：界面报错，但可以从备份恢复；坏掉的库挪进备份目录记为「恢复前」，不删。
    #[test]
    fn restore_when_library_cannot_open() {
        let dir = temp_dir("lib-restore-broken");
        let paths = DataPaths::new(dir.join("data"));
        let state = AppState::init(paths.clone());
        let id = state.with_db(|c| edit::item_create(c, "book", None)).unwrap();
        let manual = state.with_db(|c| backup::snapshot(c, &paths.backups, backup::MANUAL)).unwrap().file_name().unwrap().to_string_lossy().into_owned();
        drop(state);
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", paths.library_db.display()));
        }
        std::fs::write(&paths.library_db, b"this is not a database").unwrap();

        let state = AppState::init(paths.clone());
        assert!(state.startup_error().is_some());
        // 恢复一份坏备份失败：原样放回，仍是打不开的状态
        std::fs::write(paths.backups.join("library-20260101-080000-manual.db"), b"broken").unwrap();
        assert!(restore(&state, "library-20260101-080000-manual.db").is_err());
        assert!(state.startup_error().is_some());
        assert_eq!(std::fs::read(&paths.library_db).unwrap(), b"this is not a database");

        let r = restore(&state, &manual).unwrap();
        assert!(state.startup_error().is_none());
        assert_eq!(count(&state, &format!("SELECT COUNT(*) FROM items WHERE id = {id}")), 1);
        let aside = paths.backups.join(r.safety_backup.unwrap());
        assert_eq!(std::fs::read(aside).unwrap(), b"this is not a database", "坏掉的库留着");
        assert!(state.with_db(|c| library::items_list(c, &library::ListQuery { q: "x".into(), ..library::ListQuery::of(library::Scope::All) })).is_ok(), "搜索能用");
    }

    /// 恢复之前开始的后台补全：恢复之后不再读写（不会把旧库的结果写进新库）
    #[test]
    fn lookup_worker_stops_touching_a_replaced_library() {
        use metadata::Db;
        let dir = temp_dir("lib-epoch");
        let state = AppState::init(DataPaths::new(dir.join("data")));
        let worker = WorkerDb { state: &state, epoch: state.epoch.load(Ordering::SeqCst) };
        assert!(worker.with(db::user_version).is_ok());
        let name = state.with_db(|c| backup::snapshot(c, &state.paths.backups, backup::MANUAL)).unwrap().file_name().unwrap().to_string_lossy().into_owned();
        restore(&state, &name).unwrap();
        assert!(worker.with(db::user_version).unwrap_err().contains("已恢复"));
        let fresh = WorkerDb { state: &state, epoch: state.epoch.load(Ordering::SeqCst) };
        assert!(fresh.with(db::user_version).is_ok());
    }

    /// 导出 → 在另一个数据目录（正式版）里从导出恢复
    #[test]
    fn export_restores_into_another_data_dir() {
        let lib = transfer::tests::rich_library("lib-export");
        let paths = lib.paths.clone();
        drop(lib.c);
        let state = AppState::init(paths.clone());
        let snap = state.with_db(|c| transfer::snapshot(c)).unwrap();
        let out = dir_of(&paths).join("exports-test");
        std::fs::create_dir_all(&out).unwrap();
        let r = transfer::export(&snap, &paths.attachments, &out, "0.1.0").unwrap();

        let target = temp_dir("lib-export-target");
        let other = AppState::init(DataPaths::new(target.join("data")));
        let p = transfer::prepare_export(std::path::Path::new(&r.path), Some(&other.paths.attachments), false).unwrap();
        let report = other.replace_library(p).unwrap();
        assert!(report.checks.iter().all(|c| c.ok));
        assert_eq!(report.counts, r.counts);
        assert_eq!(count(&other, "SELECT COUNT(*) FROM items"), count(&state, "SELECT COUNT(*) FROM items"));
        assert!(report.safety_backup.is_some_and(|b| b.ends_with("-before-restore.db")));
    }

    fn dir_of(p: &DataPaths) -> std::path::PathBuf {
        p.root.parent().unwrap().to_path_buf()
    }

    #[test]
    fn broken_database_reports_instead_of_crashing() {
        let dir = temp_dir("init-broken");
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data/library.db"), b"this is not a database").unwrap();
        let state = AppState::init(DataPaths::new(dir.join("data")));
        assert!(state.startup_error().is_some());
        let e = state.with_db(|c| library::overview(c)).unwrap_err();
        assert!(e.starts_with("数据库没有打开"), "{e}");
    }
}
