//! 置顶搜索窗（`decisions/0015`、`0017`）：Word 里点 Add/Edit Citation 时打开，用户选文献、填页码；
//! Word 的请求一直等到选完或取消。窗口第一次用时才建，之后只是藏起来（关窗也只是藏起来、算取消）。
//! 打开时经启动服务把 Pith 切到前台（后台 App 不能自己抢焦点），选完把 Word 切回前台（`open -b com.microsoft.Word`），
//! 都不需要控制别的 App 的权限。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use docx_engine::cite::CiteItemRef;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use super::bridge::{PickOutcome, PickRequest};
use crate::common::err;
use crate::library::{self, ListQuery, Scope};

pub const LABEL: &str = "picker";
pub const OPEN_EVENT: &str = "picker-open";
/// Word 那边 curl 最多等 15 分钟；这里先放弃
const WAIT: Duration = Duration::from_secs(14 * 60);
const SEARCH_LIMIT: usize = 50;

/// 一条文献在搜索窗里的样子
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickInfo {
    pub uid: String,
    pub title: String,
    /// 「第一作者 等」
    pub creators: String,
    pub year: Option<i64>,
    pub container: Option<String>,
    pub item_type: Option<String>,
    /// 在文献库里（否则是文档里自带的数据）
    pub in_library: bool,
    pub in_trash: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PickEntry {
    pub cite: CiteItemRef,
    pub info: PickInfo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: u64,
    pub editing: bool,
    pub items: Vec<PickEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Done {
    Cancel,
    Items { items: Vec<CiteItemRef> },
    Remove,
}

#[derive(Default)]
pub struct Picker {
    current: Mutex<Option<(Session, mpsc::Sender<PickOutcome>)>>,
    next: AtomicU64,
}

impl Picker {
    pub fn session(&self) -> Option<Session> {
        self.current.lock().ok().and_then(|g| g.as_ref().map(|(s, _)| s.clone()))
    }

    /// 开始一次；之前没结束的那次算取消
    fn begin(&self, editing: bool, items: Vec<PickEntry>) -> (Session, mpsc::Receiver<PickOutcome>) {
        let (tx, rx) = mpsc::channel();
        let s = Session { id: self.next.fetch_add(1, Ordering::SeqCst) + 1, editing, items };
        if let Ok(mut g) = self.current.lock()
            && let Some((_, old)) = g.replace((s.clone(), tx))
        {
            let _ = old.send(PickOutcome::Cancel);
        }
        (s, rx)
    }

    /// 结束 `id` 这次（已经结束或不是当前这次就忽略）
    pub fn finish(&self, id: u64, out: PickOutcome) -> bool {
        let Ok(mut g) = self.current.lock() else { return false };
        if g.as_ref().is_some_and(|(s, _)| s.id == id)
            && let Some((_, tx)) = g.take()
        {
            let _ = tx.send(out);
            return true;
        }
        false
    }

    pub fn cancel_current(&self) {
        if let Some(id) = self.session().map(|s| s.id) {
            self.finish(id, PickOutcome::Cancel);
        }
    }
}

/// CSL-JSON（文档快照）里的显示信息
fn info_from_csl(uid: &str, v: &Value) -> PickInfo {
    let names: Vec<String> = v["author"]
        .as_array()
        .or_else(|| v["editor"].as_array())
        .map(|a| a.iter().filter_map(|n| n["literal"].as_str().or_else(|| n["family"].as_str()).map(str::to_string)).collect())
        .unwrap_or_default();
    PickInfo {
        uid: uid.to_string(),
        title: v["title"].as_str().unwrap_or("（无标题）").to_string(),
        creators: match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            _ => format!("{} 等", names[0]),
        },
        year: v["issued"]["date-parts"][0][0].as_i64().or_else(|| v["issued"]["date-parts"][0][0].as_str().and_then(|s| s.parse().ok())),
        container: v["container-title"].as_str().map(str::to_string),
        item_type: None,
        in_library: false,
        in_trash: false,
    }
}

fn info_from_row(r: &library::ListRow) -> PickInfo {
    PickInfo {
        uid: r.uid.clone(),
        title: r.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "（无标题）".into()),
        creators: match (&r.first_creator, r.creator_count) {
            (Some(c), n) if n > 1 => format!("{c} 等"),
            (Some(c), _) => c.clone(),
            _ => String::new(),
        },
        year: r.year,
        container: r.container_title.clone(),
        item_type: r.item_type.clone(),
        in_library: true,
        in_trash: r.deleted_at.is_some(),
    }
}

/// 一条文献库文献的显示信息（含回收站里的）
fn library_info(c: &Connection, uid: &str) -> Result<Option<PickInfo>, String> {
    let row = c
        .query_row(
            "SELECT i.uid, i.title, i.year, i.container_title, i.item_type, i.deleted_at,
                    (SELECT COALESCE(cr.literal, TRIM(COALESCE(cr.family, '') || ' ' || COALESCE(cr.given, ''))) FROM creators cr WHERE cr.item_id = i.id ORDER BY cr.position LIMIT 1),
                    (SELECT COUNT(*) FROM creators cr WHERE cr.item_id = i.id)
             FROM items i WHERE i.uid = ?1",
            [uid],
            |x| {
                Ok(library::ListRow {
                    kind: "item".into(),
                    id: 0,
                    uid: x.get(0)?,
                    title: x.get(1)?,
                    year: x.get(2)?,
                    container_title: x.get(3)?,
                    item_type: x.get(4)?,
                    deleted_at: x.get(5)?,
                    first_creator: x.get(6)?,
                    creator_count: x.get(7)?,
                    created_at: String::new(),
                    attachments: 0,
                    notes: 0,
                    missing_files: 0,
                    group: None,
                    group_reason: None,
                })
            },
        )
        .optional()
        .map_err(err)?;
    Ok(row.as_ref().map(info_from_row))
}

/// 编辑时预填：库里的用库里的信息，没有的用文档快照
pub fn entries(c: &Connection, items: &[CiteItemRef], snapshots: &BTreeMap<String, Value>) -> Result<Vec<PickEntry>, String> {
    items
        .iter()
        .map(|r| {
            let info = match library_info(c, &r.id)? {
                Some(i) => i,
                None => snapshots.get(&r.id).map(|v| info_from_csl(&r.id, v)).unwrap_or_else(|| PickInfo { uid: r.id.clone(), title: "（找不到这条文献）".into(), ..PickInfo::default() }),
            };
            Ok(PickEntry { cite: r.clone(), info })
        })
        .collect()
}

/// 搜索文献库（不含回收站）；空搜索词给最近加入的
pub fn search(c: &Connection, q: &str) -> Result<Vec<PickInfo>, String> {
    let mut query = ListQuery::of(Scope::All);
    query.q = q.trim().to_string();
    let rows = library::items_list(c, &query)?;
    Ok(rows.iter().filter(|r| r.kind == "item").take(SEARCH_LIMIT).map(info_from_row).collect())
}

fn window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(w);
    }
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?window=picker".into()))
        .title("Pith 引文")
        .inner_size(660.0, 440.0)
        .min_inner_size(520.0, 320.0)
        .always_on_top(true)
        .visible(false)
        .center()
        .build()
        .map_err(|e| format!("无法打开搜索窗：{e}"))?;
    let handle = app.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window(LABEL) {
                let _ = w.hide();
            }
            handle.state::<super::WordState>().picker.cancel_current();
        }
    });
    Ok(w)
}

/// 把 Word 切回前台
fn activate_word() {
    let _ = std::process::Command::new("/usr/bin/open").args(["-b", "com.microsoft.Word"]).status();
}

/// 把 Pith 自己切到前台，搜索窗才拿得到键盘。macOS 14 起后台 App 不能自己抢焦点（`set_focus` 不够），
/// 经启动服务打开自己的 .app 可以（与切回 Word 同一个办法）。开发版不在 .app 里，只能靠 `set_focus`。
fn activate_self() {
    let Ok(exe) = std::env::current_exe() else { return };
    if let super::install::Launch::App(app) = super::install::Launch::from_exe(&exe) {
        let _ = std::process::Command::new("/usr/bin/open").arg(&app).status();
    }
}

/// 打开搜索窗等用户选（在本机服务的连接线程里调用，阻塞）
pub fn pick(app: &AppHandle, picker: &Picker, req: PickRequest, entries: Vec<PickEntry>) -> Result<PickOutcome, String> {
    let (session, rx) = picker.begin(req.editing.is_some(), entries);
    let w = window(app)?;
    let _ = w.emit_to(LABEL, OPEN_EVENT, &session);
    let _ = w.set_title(if session.editing { "Pith · 编辑引文" } else { "Pith · 插入引文" });
    w.show().map_err(|e| format!("无法显示搜索窗：{e}"))?;
    activate_self();
    let _ = w.set_focus();
    let out = rx.recv_timeout(WAIT).unwrap_or(PickOutcome::Cancel);
    picker.finish(session.id, PickOutcome::Cancel);
    let _ = w.hide();
    activate_word();
    Ok(out)
}

/// 用到的文献 uid（测试用）
#[cfg(test)]
fn uids(e: &[PickEntry]) -> std::collections::BTreeSet<String> {
    e.iter().map(|x| x.info.uid.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::edit::{self, CreatorInput, ItemPatch};

    fn make(c: &Connection, title: &str, creators: &[&str]) -> String {
        let id = edit::item_create(c, "book", None).unwrap();
        let patch = ItemPatch {
            fields: [("title".to_string(), title.to_string()), ("date".to_string(), "2020".to_string())].into_iter().collect(),
            creators: Some(creators.iter().map(|n| CreatorInput { role: "author".into(), family: None, given: None, literal: Some(n.to_string()) }).collect()),
            ..Default::default()
        };
        edit::item_update(c, id, &patch).unwrap();
        c.query_row("SELECT uid FROM items WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn search_and_prefill() {
        let c = memory_db();
        let a = make(&c, "政治效能感测量", &["王晓明", "李华"]);
        let b = make(&c, "Civic attitudes", &["Lindqvist"]);
        crate::search::rebuild(&c).unwrap();
        let all = search(&c, "").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].uid, b, "空搜索：最近加入的在前");
        let hit = search(&c, "效能").unwrap();
        assert_eq!(hit.iter().map(|x| x.uid.as_str()).collect::<Vec<_>>(), [a.as_str()]);
        assert_eq!((hit[0].creators.as_str(), hit[0].year), ("王晓明 等", Some(2020)));
        // 预填：库里的、文档快照里的、哪里都没有的
        let mut snaps = BTreeMap::new();
        snaps.insert("zotero-u-X".to_string(), serde_json::json!({ "id": "zotero-u-X", "title": "Only in doc", "author": [{ "family": "Halvorsen", "given": "I" }], "issued": { "date-parts": [["2018"]] } }));
        let e = entries(&c, &[CiteItemRef::new(&a).with_locator("17"), CiteItemRef::new("zotero-u-X"), CiteItemRef::new("gone")], &snaps).unwrap();
        assert_eq!(uids(&e).len(), 3);
        assert!(e[0].info.in_library && e[0].cite.locator.as_deref() == Some("17"));
        assert_eq!((e[1].info.title.as_str(), e[1].info.creators.as_str(), e[1].info.year, e[1].info.in_library), ("Only in doc", "Halvorsen", Some(2018), false));
        assert_eq!(e[2].info.title, "（找不到这条文献）");
        // 回收站里的：搜索不到，预填时标出来
        let id: i64 = c.query_row("SELECT id FROM items WHERE uid = ?1", [&a], |r| r.get(0)).unwrap();
        edit::trash(&c, &[edit::Target::item(id)]).unwrap();
        crate::search::rebuild(&c).unwrap();
        assert!(search(&c, "效能").unwrap().is_empty());
        assert!(entries(&c, &[CiteItemRef::new(&a)], &snaps).unwrap()[0].info.in_trash);
    }

    #[test]
    fn sessions_replace_and_finish_once() {
        let p = Picker::default();
        let (s1, rx1) = p.begin(false, vec![]);
        let (s2, rx2) = p.begin(true, vec![]);
        assert_eq!(rx1.recv().unwrap(), PickOutcome::Cancel, "新的一次开始，旧的算取消");
        assert_eq!(p.session().unwrap().id, s2.id);
        assert!(!p.finish(s1.id, PickOutcome::Remove), "旧的编号不认");
        assert!(p.finish(s2.id, PickOutcome::Items(vec![CiteItemRef::new("u")])));
        assert_eq!(rx2.recv().unwrap(), PickOutcome::Items(vec![CiteItemRef::new("u")]));
        assert!(p.session().is_none());
        assert!(!p.finish(s2.id, PickOutcome::Cancel), "只结束一次");
    }
}
