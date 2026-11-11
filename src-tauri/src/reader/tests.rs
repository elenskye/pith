use super::*;
use crate::db::tests::memory_db;
use crate::search;

/// 文献 1（PDF 子附件 10、HTML 子附件 11、缺失的 PDF 12），独立 PDF 13；文献 2 在回收站，PDF 子附件 20
fn setup() -> Connection {
    let c = memory_db();
    let pdf = "aa".repeat(32);
    let html = "bb".repeat(32);
    c.execute_batch(&format!(
        "INSERT INTO items (id, uid, item_type, title, source, created_at, updated_at, deleted_at) VALUES
           (1, 'I1', 'journalArticle', 'Voting and Efficacy', 'manual', 't', 't', NULL),
           (2, 'I2', 'book', '回收站里的书', 'manual', 't', 't', 't');
         INSERT INTO files (sha256, size, ext, created_at) VALUES ('{pdf}', 1000, 'pdf', 't'), ('{html}', 10, 'html', 't');
         INSERT INTO attachments (id, uid, item_id, kind, sha256, status, title, content_type, created_at, updated_at) VALUES
           (10, 'A10', 1, 'stored', '{pdf}', 'ok', 'Full Text PDF', 'application/pdf', 't', 't'),
           (11, 'A11', 1, 'stored', '{html}', 'ok', 'Snapshot', 'text/html', 't', 't'),
           (12, 'A12', 1, 'stored', NULL, 'missing', 'Gone', 'application/pdf', 't', 't'),
           (13, 'A13', NULL, 'stored', '{pdf}', 'ok', NULL, NULL, 't', 't'),
           (20, 'A20', 2, 'stored', '{pdf}', 'ok', '书', 'application/pdf', 't', 't');"
    ))
    .unwrap();
    search::rebuild(&c).unwrap();
    c
}

const LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

fn hl(att: i64, rects: Vec<[f64; 4]>, text: &str) -> NewAnnotation {
    NewAnnotation {
        attachment_id: att,
        kind: "highlight".into(),
        page_index: 2,
        page_label: Some("47".into()),
        page_box: LETTER,
        rects,
        text: Some(text.into()),
        comment: None,
        color: None,
        char_offset: Some(123),
    }
}

fn hits(c: &Connection, q: &str) -> Vec<search::DocRef> {
    let mut v: Vec<_> = search::query(c, q).unwrap().unwrap_or_default().into_keys().collect();
    v.sort();
    v
}

#[test]
fn reader_get_checks_the_file() {
    let c = setup();
    let d = reader_get(&c, 10).unwrap();
    assert_eq!((d.title.as_str(), d.size, d.readonly.as_deref(), d.state.as_ref()), ("Full Text PDF", Some(1000), None, None));
    assert_eq!(d.parent.as_ref().map(|p| (p.kind.as_str(), p.id)), Some(("item", 1)));
    // 没有标题、没有 MIME 类型：按扩展名认出 PDF
    let d = reader_get(&c, 13).unwrap();
    assert_eq!((d.title.as_str(), d.parent.is_none()), ("PDF", true));
    assert!(reader_get(&c, 11).unwrap_err().contains("不是 PDF"));
    assert!(reader_get(&c, 12).unwrap_err().contains("缺失"));
    assert!(reader_get(&c, 99).unwrap_err().contains("找不到"));
    assert!(reader_get(&c, 20).unwrap().readonly.unwrap().contains("所属文献在回收站"));
    c.execute("UPDATE attachments SET deleted_at = 't' WHERE id = 13", []).unwrap();
    assert!(reader_get(&c, 13).unwrap().readonly.unwrap().contains("附件在回收站"));
}

#[test]
fn reading_position_is_kept_in_zotero_shape() {
    let c = setup();
    let raw = json!({"pageIndex": 3, "scale": "page-width", "top": 612.456, "left": -8, "rotation": 0, "sidebarOpen": true, "junk": "x"});
    reader_save_state(&c, 10, &raw).unwrap();
    let s = reader_get(&c, 10).unwrap().state.unwrap();
    assert_eq!(s, json!({"pageIndex": 3, "scale": "page-width", "top": 612.46, "left": -8.0, "rotation": 0}));
    // 再存覆盖；数字缩放；不合理的值丢掉
    reader_save_state(&c, 10, &json!({"pageIndex": 0, "scale": 1.5, "top": "x", "rotation": 45})).unwrap();
    assert_eq!(reader_get(&c, 10).unwrap().state.unwrap(), json!({"pageIndex": 0, "scale": 1.5}));
    let n: i64 = c.query_row("SELECT COUNT(*) FROM reader_states", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    assert!(clean_state(&json!({"scale": 1})).is_err(), "必须有页码");
    assert!(clean_state(&json!({"pageIndex": -1})).is_err());
    assert!(clean_state(&json!([1])).is_err());
    assert_eq!(clean_state(&json!({"pageIndex": 1, "scale": 99})).unwrap(), json!({"pageIndex": 1}));
    assert!(reader_save_state(&c, 99, &json!({"pageIndex": 0})).is_err());
    // 回收站里的也能记；附件删掉，阅读位置随之删除
    reader_save_state(&c, 20, &json!({"pageIndex": 5})).unwrap();
    c.execute("DELETE FROM attachments WHERE id = 20", []).unwrap();
    let n: i64 = c.query_row("SELECT COUNT(*) FROM reader_states WHERE attachment_id = 20", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn rects_merge_into_lines() {
    // 第一行三段（span 之间有小空隙，有一段重复）、第二行两段、上下颠倒的矩形、太小的、超出页面的
    let raw = [
        [72.0, 700.0, 150.2, 712.0],
        [150.5, 700.4, 300.0, 711.6],
        [150.5, 700.4, 300.0, 711.6],
        [300.0, 712.0, 350.0, 700.0],
        [72.0, 686.0, 200.0, 698.0],
        [201.0, 686.0, 260.0, 698.0],
        [10.0, 10.0, 10.2, 20.0],
        [600.0, 500.0, 700.0, 512.0],
    ];
    assert_eq!(
        merge_rects(&raw, LETTER),
        vec![[72.0, 700.0, 350.0, 712.0], [72.0, 686.0, 260.0, 698.0], [600.0, 500.0, 612.0, 512.0]],
        "从上到下；同一行并成一个；裁到页面里"
    );
    // 两栏：同一高度、隔着栏间距的不并
    let cols = [[72.0, 700.0, 290.0, 712.0], [322.0, 700.0, 540.0, 712.0]];
    assert_eq!(merge_rects(&cols, LETTER), cols.to_vec());
    // 取 3 位小数
    assert_eq!(merge_rects(&[[1.23456, 2.0, 50.98765, 14.0]], LETTER), vec![[1.235, 2.0, 50.988, 14.0]]);
    assert!(merge_rects(&[[f64::NAN, 0.0, 1.0, 1.0], [5.0, 5.0, 5.0, 9.0]], LETTER).is_empty());
}

#[test]
fn sort_keys_match_zotero() {
    assert_eq!(sort_key(0, 123, 79.9), "00000|000123|00079");
    assert_eq!(sort_key(12, 0, -3.0), "00012|000000|00000");
    assert_eq!(sort_key(1, 5_000_000, f64::NAN), "00001|999999|00000");
}

#[test]
fn highlight_is_stored_like_zotero() {
    let c = setup();
    let n = hl(10, vec![[72.0, 700.0, 300.0, 712.0], [72.0, 686.0, 200.0, 698.0]], "The effi-\ncacy  of\nvoting");
    let (a, refs) = annotation_create(&c, &n).unwrap();
    assert_eq!(refs, vec![search::DocRef::attachment(10)]);
    assert_eq!((a.kind.as_str(), a.page_index, a.page_label.as_deref()), ("highlight", 2, Some("47")));
    assert_eq!(a.text.as_deref(), Some("The efficacy of voting"));
    assert_eq!((a.color.as_deref(), a.comment.as_deref(), a.editable, a.is_external), (Some("#ffd400"), None, true, false));
    assert_eq!(a.sort_key, "00002|000123|00080", "距页顶 = 792 - 712");
    assert_eq!(a.uid.len(), 26);
    let stored: String = c.query_row("SELECT position FROM annotations WHERE id = ?1", [a.id], |r| r.get(0)).unwrap();
    assert_eq!(stored, r#"{"pageIndex":2,"rects":[[72.0,700.0,300.0,712.0],[72.0,686.0,200.0,698.0]]}"#);
    assert_eq!(a.position, json!({"pageIndex": 2, "rects": [[72.0, 700.0, 300.0, 712.0], [72.0, 686.0, 200.0, 698.0]]}));

    // 下划线、便签；列表按页内顺序
    let mut u = hl(10, vec![[72.0, 300.0, 100.0, 312.0]], "later");
    u.kind = "underline".into();
    u.color = Some("#2EA8E5".into());
    u.comment = Some("  看这里  ".into());
    let (u, _) = annotation_create(&c, &u).unwrap();
    assert_eq!((u.color.as_deref(), u.comment.as_deref()), (Some("#2ea8e5"), Some("看这里")));
    let mut note = hl(10, vec![[500.0, 750.0, 522.0, 772.0]], "");
    note.kind = "note".into();
    note.text = None;
    note.page_index = 0;
    note.char_offset = None;
    let (note, _) = annotation_create(&c, &note).unwrap();
    assert_eq!((note.text, note.sort_key.as_str()), (None, "00000|000000|00020"));
    let ids: Vec<i64> = annotations_list(&c, 10).unwrap().iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![note.id, a.id, u.id]);
}

#[test]
fn bad_annotations_are_refused() {
    let c = setup();
    let ok = || hl(10, vec![[72.0, 700.0, 300.0, 712.0]], "text");
    let e = |n: NewAnnotation| annotation_create(&c, &n).unwrap_err();
    assert!(e(hl(11, vec![[72.0, 700.0, 300.0, 712.0]], "x")).contains("只能给 PDF"));
    assert!(e(hl(20, vec![[72.0, 700.0, 300.0, 712.0]], "x")).contains("回收站"));
    assert!(e(hl(99, vec![[72.0, 700.0, 300.0, 712.0]], "x")).contains("找不到"));
    assert!(e(hl(10, vec![[72.0, 700.0, 300.0, 712.0]], " \n ")).contains("没有选中文字"));
    assert!(e(hl(10, vec![], "x")).contains("没有选中内容"));
    assert!(e(hl(10, vec![[900.0, 900.0, 950.0, 950.0]], "x")).contains("不在页面上"));
    assert!(e(NewAnnotation { color: Some("yellow".into()), ..ok() }).contains("颜色"));
    assert!(e(NewAnnotation { kind: "ink".into(), ..ok() }).contains("不支持"));
    assert!(e(NewAnnotation { page_index: -1, ..ok() }).contains("页码"));
    assert!(e(NewAnnotation { page_box: [0.0, 0.0, 0.0, 0.0], ..ok() }).contains("页面大小"));
    assert!(e(NewAnnotation { text: Some("字".repeat(20_001)), ..ok() }).contains("太长"));
    let n: i64 = c.query_row("SELECT COUNT(*) FROM annotations", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn edit_delete_restore_and_search() {
    let c = setup();
    let (a, refs) = annotation_create(&c, &hl(10, vec![[72.0, 700.0, 300.0, 712.0]], "deliberative democracy")).unwrap();
    search::update(&c, &refs).unwrap();
    assert_eq!(hits(&c, "deliberative"), vec![search::DocRef::item(1)], "标注原文并进所属文献");

    let (b, refs) = annotation_update(&c, a.id, &AnnotationPatch { color: Some("#ff6666".into()), comment: Some("核心概念".into()), text: None }).unwrap();
    search::update(&c, &refs).unwrap();
    assert_eq!((b.color.as_deref(), b.comment.as_deref(), b.text.as_deref()), (Some("#ff6666"), Some("核心概念"), Some("deliberative democracy")));
    assert!(b.updated_at >= a.updated_at);
    assert_eq!(hits(&c, "核心概念"), vec![search::DocRef::item(1)]);
    let (b, _) = annotation_update(&c, a.id, &AnnotationPatch { comment: Some("  ".into()), text: Some("deliberative democracy (Habermas)".into()), ..Default::default() }).unwrap();
    assert_eq!((b.comment, b.text.as_deref(), b.color.as_deref()), (None, Some("deliberative democracy (Habermas)"), Some("#ff6666")));

    let refs = annotations_delete(&c, &[a.id]).unwrap();
    search::update(&c, &refs).unwrap();
    assert!(annotations_list(&c, 10).unwrap().is_empty());
    assert!(hits(&c, "deliberative").is_empty());
    assert!(annotations_delete(&c, &[a.id]).unwrap_err().contains("已经删除"));
    assert!(annotation_update(&c, a.id, &AnnotationPatch::default()).unwrap_err().contains("已经删除"));
    let refs = annotations_restore(&c, &[a.id]).unwrap();
    search::update(&c, &refs).unwrap();
    assert_eq!(annotations_list(&c, 10).unwrap().len(), 1);
    assert_eq!(hits(&c, "deliberative"), vec![search::DocRef::item(1)]);
    assert!(annotations_restore(&c, &[a.id]).unwrap_err().contains("没有被删除"));
    assert!(annotations_delete(&c, &[]).is_err());

    // 独立 PDF 的标注进它自己的文档
    let (_, refs) = annotation_create(&c, &hl(13, vec![[72.0, 700.0, 300.0, 712.0]], "standalone excerpt")).unwrap();
    search::update(&c, &refs).unwrap();
    assert_eq!(hits(&c, "standalone"), vec![search::DocRef::attachment(13)]);
}

#[test]
fn imported_and_trashed_annotations_are_read_only() {
    let c = setup();
    // 模仿 Zotero 导入：来自 PDF 文件本身（external）、带标签、带作者；另一条在回收站附件上
    c.execute_batch(
        r#"INSERT INTO annotations (id, uid, attachment_id, kind, page_index, page_label, position, sort_key, text, comment, color, author_name, is_external, created_at, updated_at)
           VALUES (1, 'Z1', 10, 'highlight', 0, '1', '{"pageIndex":0,"rects":[[10.5,20,30,40]]}', '00000|000010|00700', 'zotero text', NULL, '#5fb236', 'Someone', 1, 't', 't'),
                  (2, 'Z2', 10, 'ink', 1, '2', '{"pageIndex":1,"width":2,"paths":[[1,2,3,4]]}', '00001|000000|00010', NULL, 'ink note', '#aaaaaa', NULL, 0, 't', 't'),
                  (3, 'Z3', 10, 'image', 1, '2', 'not json', '00001|000000|00500', NULL, NULL, NULL, NULL, 0, 't', 't'),
                  (4, 'Z4', 20, 'highlight', 0, '1', '{"pageIndex":0,"rects":[[1,1,9,9]]}', '00000|000000|00000', 'in trash', NULL, NULL, NULL, 0, 't', 't');
           INSERT INTO tags (id, name) VALUES (1, '重点');
           INSERT INTO annotation_tags (annotation_id, tag_id) VALUES (1, 1);"#,
    )
    .unwrap();
    let list = annotations_list(&c, 10).unwrap();
    assert_eq!(list.iter().map(|a| a.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!((list[0].tags.clone(), list[0].author_name.as_deref(), list[0].is_external, list[0].editable), (vec!["重点".to_string()], Some("Someone"), true, false));
    assert_eq!(list[0].position["rects"][0][0], json!(10.5));
    assert!(list[1].editable && list[1].position["paths"].is_array());
    assert_eq!(list[2].position, Value::Null, "坏的位置不挡列表");
    assert!(annotation_update(&c, 1, &AnnotationPatch::default()).unwrap_err().contains("PDF 文件本身"));
    assert!(annotations_delete(&c, &[2, 1]).is_err());
    assert_eq!(annotations_list(&c, 10).unwrap().len(), 3, "一条不能删就整批不删");
    let trashed = annotations_list(&c, 20).unwrap();
    assert!(!trashed[0].editable);
    assert!(annotations_delete(&c, &[4]).unwrap_err().contains("回收站"));
}
