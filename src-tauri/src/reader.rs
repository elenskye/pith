//! 阅读（Phase 4）：PDF 标注（高亮 / 下划线 / 便签）与阅读位置。
//! 规则：
//! - 位置与 Zotero 同格式：`{"pageIndex":0,"rects":[[x1,y1,x2,y2]]}`，PDF 用户空间坐标（pt，原点在页面左下）；
//!   排序键 `页|字符偏移|距页顶`（`00000|000123|00079`）。Zotero 导入的标注原样显示，不用转换。
//! - 界面只传选区的原始矩形与文字；合并成行、取整、算排序键、整理摘录文字都在这里做。
//! - 回收站里的附件（或它所属的文献在回收站）只能看，不能加改标注；来自 PDF 文件本身的标注（Zotero 的 external）只读。
//! - 删除标注 = 写 `deleted_at`（界面可撤销）；附件永久删除时随之删除。
//! - 返回受影响的文档（`search::DocRef`），命令层据此更新搜索索引（标注原文与评论在索引里）。

use crate::common::{Tx, err, new_uid};
use crate::library::Parent;
use crate::search::DocRef;
use crate::text;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// 能在 App 里阅读的附件（SQL 片段，别名 a = attachments、f = files）：有文件、文件在、是 PDF
pub const READABLE: &str =
    "(a.sha256 IS NOT NULL AND a.status = 'ok' AND (IFNULL(f.ext, '') = 'pdf' OR IFNULL(a.content_type, '') = 'application/pdf'))";

/// 默认颜色（Zotero 黄）
pub const DEFAULT_COLOR: &str = "#ffd400";

/// 选中文字的上限（再长就请用户分段）
const MAX_TEXT: usize = 20_000;
const MAX_COMMENT: usize = 50_000;
const MAX_RECTS: usize = 2_000;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderDoc {
    pub id: i64,
    pub title: String,
    pub parent: Option<Parent>,
    pub size: Option<i64>,
    /// 不为空 = 只能看、不能加改标注，值是原因
    pub readonly: Option<String>,
    /// 上次读到哪（Zotero 阅读状态同格式），没有为 None
    pub state: Option<Value>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: i64,
    pub uid: String,
    /// highlight / underline / note / image / ink / text
    pub kind: String,
    pub page_index: i64,
    pub page_label: Option<String>,
    /// 原样的位置 JSON；坏了为 null（界面不画，列表照常显示）
    pub position: Value,
    pub sort_key: String,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub color: Option<String>,
    pub tags: Vec<String>,
    pub author_name: Option<String>,
    /// 来自 PDF 文件本身（只读）
    pub is_external: bool,
    pub editable: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 界面新建标注时传来的内容
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAnnotation {
    pub attachment_id: i64,
    /// highlight / underline / note
    pub kind: String,
    pub page_index: i64,
    pub page_label: Option<String>,
    /// 页面框（PDF 坐标的 viewBox：[x1, y1, x2, y2]），用来检查矩形、算「距页顶」
    pub page_box: [f64; 4],
    /// 选区的矩形（PDF 坐标，一段文字一个，未合并）；便签为一个图标框
    pub rects: Vec<[f64; 4]>,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub color: Option<String>,
    /// 选区开头在本页文字里的位置（排序用），不知道就不填
    pub char_offset: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationPatch {
    pub color: Option<String>,
    pub comment: Option<String>,
    pub text: Option<String>,
}

// ---------------- 附件 ----------------

/// 附件不在回收站、所属文献也不在：None；否则返回原因
fn trash_reason(c: &Connection, id: i64) -> Result<Option<String>, String> {
    let row: Option<(Option<String>, Option<String>)> = c
        .query_row(
            "SELECT a.deleted_at, i.deleted_at FROM attachments a LEFT JOIN items i ON i.id = a.item_id WHERE a.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    Ok(match row.ok_or("找不到这个附件，可能已被删除")? {
        (Some(_), _) => Some("这个附件在回收站里，只能阅读；恢复后才能加标注".into()),
        (None, Some(_)) => Some("所属文献在回收站里，只能阅读；恢复后才能加标注".into()),
        _ => None,
    })
}

/// 打开阅读器要的信息。不是 PDF、文件缺失时报错（界面改用系统程序打开）。
pub fn reader_get(c: &Connection, id: i64) -> Result<ReaderDoc, String> {
    type Row = (bool, String, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>);
    let row: Option<Row> = c
        .query_row(
            &format!(
                "SELECT {READABLE}, a.status, COALESCE(a.title, a.original_name), f.size, a.item_id, i.title,
                        (SELECT state FROM reader_states s WHERE s.attachment_id = a.id)
                   FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 LEFT JOIN items i ON i.id = a.item_id
                  WHERE a.id = ?1"
            ),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()
        .map_err(err)?;
    let (readable, status, title, size, item, item_title, state) = row.ok_or("找不到这个附件，可能已被删除")?;
    if !readable {
        return Err(if status == "missing" {
            "这个附件的文件缺失，导入时就没有找到".into()
        } else {
            "这个附件不是 PDF，App 里还不能阅读；请用「打开」交给系统程序".into()
        });
    }
    Ok(ReaderDoc {
        id,
        title: title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "PDF".into()),
        parent: item.map(|i| Parent { kind: "item".into(), id: i, title: item_title }),
        size,
        readonly: trash_reason(c, id)?,
        state: state.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

// ---------------- 阅读位置 ----------------

fn finite(v: &Value, lo: f64, hi: f64) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && (lo..=hi).contains(x))
}

/// 只留认识的键，值不合理的丢掉；pageIndex 必须有。
pub fn clean_state(state: &Value) -> Result<Value, String> {
    let o = state.as_object().ok_or("阅读位置格式不对")?;
    let page = o.get("pageIndex").and_then(Value::as_i64).filter(|p| (0..1_000_000).contains(p)).ok_or("阅读位置缺少页码")?;
    let mut out = Map::new();
    out.insert("pageIndex".into(), json!(page));
    match o.get("scale") {
        Some(Value::String(s)) if ["page-width", "page-fit", "auto", "page-actual"].contains(&s.as_str()) => {
            out.insert("scale".into(), json!(s));
        }
        Some(v) => {
            if let Some(x) = finite(v, 0.1, 10.0) {
                out.insert("scale".into(), json!(x));
            }
        }
        None => {}
    }
    for k in ["top", "left"] {
        if let Some(x) = o.get(k).and_then(|v| finite(v, -1e6, 1e6)) {
            out.insert(k.into(), json!((x * 100.0).round() / 100.0));
        }
    }
    if let Some(r) = o.get("rotation").and_then(Value::as_i64).filter(|r| [0, 90, 180, 270].contains(r)) {
        out.insert("rotation".into(), json!(r));
    }
    Ok(Value::Object(out))
}

/// 记下读到哪（不改附件的修改时间；回收站里的也记）
pub fn reader_save_state(c: &Connection, id: i64, state: &Value) -> Result<(), String> {
    let state = clean_state(state)?;
    trash_reason(c, id)?; // 附件要存在
    c.execute(
        "INSERT INTO reader_states (attachment_id, state, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (attachment_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
        params![id, state.to_string(), text::now_iso()],
    )
    .map_err(err)?;
    Ok(())
}

// ---------------- 标注 ----------------

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// 选区矩形 → 一行一个（或一行里隔开的几段）矩形：
/// 每个先规整成 x1 < x2、y1 < y2 并裁到页面框里，太小的丢掉；纵向重叠过半的算同一行，同一行里挨着的（间隔小于 0.6 个行高）并成一个。
/// 结果从上到下、从左到右，坐标取 3 位小数（与 Zotero 相同）。
pub fn merge_rects(rects: &[[f64; 4]], page_box: [f64; 4]) -> Vec<[f64; 4]> {
    let [bx1, by1, bx2, by2] = page_box;
    let mut rs: Vec<[f64; 4]> = rects
        .iter()
        .filter(|r| r.iter().all(|v| v.is_finite()))
        .map(|r| [r[0].min(r[2]).max(bx1), r[1].min(r[3]).max(by1), r[0].max(r[2]).min(bx2), r[1].max(r[3]).min(by2)])
        .filter(|r| r[2] - r[0] >= 0.5 && r[3] - r[1] >= 0.5)
        .collect();
    // 从上到下（PDF 坐标 y 向上：上沿大的在前），同高时从左到右
    rs.sort_by(|a, b| b[3].total_cmp(&a[3]).then(a[0].total_cmp(&b[0])));
    let mut lines: Vec<Vec<[f64; 4]>> = Vec::new();
    for r in rs {
        let h = r[3] - r[1];
        let same = lines.last().is_some_and(|line| {
            let (ly1, ly2) = line.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x[1]), b.max(x[3])));
            let overlap = r[3].min(ly2) - r[1].max(ly1);
            overlap > 0.5 * h.min(ly2 - ly1)
        });
        if same {
            lines.last_mut().unwrap().push(r);
        } else {
            lines.push(vec![r]);
        }
    }
    let mut out = Vec::new();
    for mut line in lines {
        line.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut cur = line[0];
        for r in &line[1..] {
            let h = (cur[3] - cur[1]).min(r[3] - r[1]);
            if r[0] - cur[2] < 0.6 * h {
                cur = [cur[0], cur[1].min(r[1]), cur[2].max(r[2]), cur[3].max(r[3])];
            } else {
                out.push(cur);
                cur = *r;
            }
        }
        out.push(cur);
    }
    out.into_iter().map(|r| r.map(round3)).collect()
}

/// 排序键：`页|字符偏移|距页顶`，与 Zotero 的 sortIndex 相同（5|6|5 位，补零）
pub fn sort_key(page_index: i64, char_offset: i64, top: f64) -> String {
    let top = if top.is_finite() { top.max(0.0).floor() as i64 } else { 0 };
    format!("{:05}|{:06}|{:05}", page_index.clamp(0, 99_999), char_offset.clamp(0, 999_999), top.min(99_999))
}

/// `#RRGGBB` → 小写；不填用默认黄
fn clean_color(c: Option<&str>) -> Result<String, String> {
    let c = c.map(str::trim).filter(|c| !c.is_empty()).unwrap_or(DEFAULT_COLOR);
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
    if ok { Ok(c.to_ascii_lowercase()) } else { Err(format!("颜色格式不对：{c}")) }
}

/// 评论：去掉首尾空白，空的存 NULL
fn clean_comment(c: Option<&str>) -> Result<Option<String>, String> {
    let c = c.map(str::trim).filter(|c| !c.is_empty());
    if c.is_some_and(|c| c.chars().count() > MAX_COMMENT) {
        return Err("评论太长（超过 5 万字）".into());
    }
    Ok(c.map(str::to_string))
}

fn check_text_len(t: &str) -> Result<(), String> {
    if t.chars().count() > MAX_TEXT { Err("选中的文字太长（超过 2 万字），请分段高亮".into()) } else { Ok(()) }
}

const SELECT: &str = "SELECT an.id, an.uid, an.kind, an.page_index, an.page_label, an.position, an.sort_key, an.text, an.comment, an.color,
                             an.author_name, an.is_external, an.created_at, an.updated_at
                        FROM annotations an";

fn read_rows(c: &Connection, sql: &str, p: impl rusqlite::Params, editable: bool) -> Result<Vec<Annotation>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    let mut list: Vec<Annotation> = st
        .query_map(p, |r| {
            let external: bool = r.get(11)?;
            Ok(Annotation {
                id: r.get(0)?,
                uid: r.get(1)?,
                kind: r.get(2)?,
                page_index: r.get(3)?,
                page_label: r.get(4)?,
                position: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or(Value::Null),
                sort_key: r.get(6)?,
                text: r.get(7)?,
                comment: r.get(8)?,
                color: r.get(9)?,
                tags: Vec::new(),
                author_name: r.get(10)?,
                is_external: external,
                editable: editable && !external,
                created_at: r.get(12)?,
                updated_at: r.get(13)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = c
        .prepare("SELECT t.name FROM annotation_tags x JOIN tags t ON t.id = x.tag_id WHERE x.annotation_id = ?1 ORDER BY t.name")
        .map_err(err)?;
    for a in &mut list {
        a.tags = st.query_map([a.id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    }
    Ok(list)
}

/// 一个附件的标注（不含已删除的），按页内顺序
pub fn annotations_list(c: &Connection, attachment_id: i64) -> Result<Vec<Annotation>, String> {
    let editable = trash_reason(c, attachment_id)?.is_none();
    read_rows(c, &format!("{SELECT} WHERE an.attachment_id = ?1 AND an.deleted_at IS NULL ORDER BY an.sort_key, an.id"), [attachment_id], editable)
}

fn annotation_get(c: &Connection, id: i64) -> Result<Annotation, String> {
    let att: i64 = c
        .query_row("SELECT attachment_id FROM annotations WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or("找不到这条标注，可能已被删除")?;
    let editable = trash_reason(c, att)?.is_none();
    read_rows(c, &format!("{SELECT} WHERE an.id = ?1"), [id], editable)?.pop().ok_or_else(|| "找不到这条标注".into())
}

/// 新建标注。返回新标注与要更新的搜索文档。
pub fn annotation_create(c: &Connection, n: &NewAnnotation) -> Result<(Annotation, Vec<DocRef>), String> {
    let readable: Option<bool> = c
        .query_row(
            &format!("SELECT {READABLE} FROM attachments a LEFT JOIN files f ON f.sha256 = a.sha256 WHERE a.id = ?1"),
            [n.attachment_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    match readable {
        None => return Err("找不到这个附件，可能已被删除".into()),
        Some(false) => return Err("只能给 PDF 附件加标注".into()),
        Some(true) => {}
    }
    if let Some(why) = trash_reason(c, n.attachment_id)? {
        return Err(why);
    }
    if !(0..100_000).contains(&n.page_index) {
        return Err("页码不对".into());
    }
    let b = n.page_box;
    if !b.iter().all(|v| v.is_finite()) || b[2] - b[0] < 1.0 || b[3] - b[1] < 1.0 {
        return Err("页面大小不对".into());
    }
    if n.rects.is_empty() || n.rects.len() > MAX_RECTS {
        return Err("没有选中内容".into());
    }
    let (rects, text) = match n.kind.as_str() {
        "highlight" | "underline" => {
            let text = text::excerpt(n.text.as_deref().unwrap_or(""));
            if text.is_empty() {
                return Err("没有选中文字（这一页可能是扫描图片，没有文字层）".into());
            }
            check_text_len(&text)?;
            (merge_rects(&n.rects, b), Some(text))
        }
        "note" => {
            if n.rects.len() != 1 {
                return Err("便签只能放在一个位置".into());
            }
            (merge_rects(&n.rects, b), None)
        }
        other => return Err(format!("不支持新建这种标注：{other}")),
    };
    if rects.is_empty() {
        return Err("选中的位置不在页面上".into());
    }
    let color = clean_color(n.color.as_deref())?;
    let comment = clean_comment(n.comment.as_deref())?;
    let label = n.page_label.as_deref().map(str::trim).filter(|l| !l.is_empty() && l.chars().count() <= 50).map(str::to_string);
    let top_edge = rects.iter().map(|r| r[3]).fold(f64::MIN, f64::max);
    let key = sort_key(n.page_index, n.char_offset.unwrap_or(0), b[3] - top_edge);
    let position = json!({ "pageIndex": n.page_index, "rects": rects });
    let now = text::now_iso();
    let tx = Tx::begin(c)?;
    tx.execute(
        "INSERT INTO annotations (uid, attachment_id, kind, page_index, page_label, position, sort_key, text, comment, color, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![new_uid()?, n.attachment_id, n.kind, n.page_index, label, position.to_string(), key, text, comment, color, now],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok((annotation_get(c, id)?, vec![DocRef::attachment(n.attachment_id)]))
}

/// 这些标注都在、没删、能改；返回它们所在的附件（去重）
fn editable_targets(c: &Connection, ids: &[i64], deleted: bool) -> Result<Vec<i64>, String> {
    if ids.is_empty() {
        return Err("没有选中标注".into());
    }
    let mut atts = Vec::new();
    for &id in ids {
        let row: Option<(i64, bool, Option<String>)> = c
            .query_row("SELECT attachment_id, is_external, deleted_at FROM annotations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
            .map_err(err)?;
        let (att, external, del) = row.ok_or("找不到这条标注，可能已被删除，请刷新")?;
        if del.is_some() != deleted {
            return Err(if deleted { "这条标注没有被删除" } else { "这条标注已经删除了" }.into());
        }
        if external {
            return Err("这条标注来自 PDF 文件本身，不能修改".into());
        }
        if let Some(why) = trash_reason(c, att)? {
            return Err(why);
        }
        if !atts.contains(&att) {
            atts.push(att);
        }
    }
    Ok(atts)
}

/// 改颜色 / 评论 / 摘录文字（只改传了的）
pub fn annotation_update(c: &Connection, id: i64, p: &AnnotationPatch) -> Result<(Annotation, Vec<DocRef>), String> {
    let atts = editable_targets(c, &[id], false)?;
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    if let Some(color) = &p.color {
        tx.execute("UPDATE annotations SET color = ?1 WHERE id = ?2", params![clean_color(Some(color))?, id]).map_err(err)?;
    }
    if let Some(comment) = &p.comment {
        tx.execute("UPDATE annotations SET comment = ?1 WHERE id = ?2", params![clean_comment(Some(comment))?, id]).map_err(err)?;
    }
    if let Some(t) = &p.text {
        let t = t.trim();
        check_text_len(t)?;
        tx.execute("UPDATE annotations SET text = ?1 WHERE id = ?2", params![Some(t).filter(|t| !t.is_empty()), id]).map_err(err)?;
    }
    tx.execute("UPDATE annotations SET updated_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
    tx.commit()?;
    Ok((annotation_get(c, id)?, atts.into_iter().map(DocRef::attachment).collect()))
}

/// 删除（写 deleted_at，可用 `annotations_restore` 撤销）
pub fn annotations_delete(c: &Connection, ids: &[i64]) -> Result<Vec<DocRef>, String> {
    set_deleted(c, ids, false)
}

/// 撤销删除
pub fn annotations_restore(c: &Connection, ids: &[i64]) -> Result<Vec<DocRef>, String> {
    set_deleted(c, ids, true)
}

fn set_deleted(c: &Connection, ids: &[i64], restoring: bool) -> Result<Vec<DocRef>, String> {
    let atts = editable_targets(c, ids, restoring)?;
    let tx = Tx::begin(c)?;
    let now = text::now_iso();
    for &id in ids {
        if restoring {
            tx.execute("UPDATE annotations SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
        } else {
            tx.execute("UPDATE annotations SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2", params![now, id]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(atts.into_iter().map(DocRef::attachment).collect())
}

#[cfg(test)]
mod tests;
