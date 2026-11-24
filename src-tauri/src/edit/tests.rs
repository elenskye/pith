//! 编辑、标签、集合、回收站、合并、添加附件的测试（合成数据）。

use super::*;
use crate::db::{self, tests::TempDir, tests::memory_db, tests::temp_dir};
use crate::library::{self, ListQuery, Scope};
use crate::paths::DataPaths;

fn one<T: rusqlite::types::FromSql>(c: &Connection, sql: &str) -> T {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn field(c: &Connection, id: i64, name: &str) -> Option<String> {
    c.query_row("SELECT value FROM item_fields WHERE item_id = ?1 AND field = ?2", params![id, name], |r| r.get(0)).optional().unwrap()
}

fn patch(kv: &[(&str, &str)]) -> ItemPatch {
    ItemPatch { fields: kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), ..Default::default() }
}

fn creator(role: &str, family: Option<&str>, given: Option<&str>, literal: Option<&str>) -> CreatorInput {
    CreatorInput { role: role.into(), family: family.map(Into::into), given: given.map(Into::into), literal: literal.map(Into::into) }
}

fn list(c: &Connection, scope: Scope) -> Vec<library::ListRow> {
    library::items_list(c, &ListQuery::of(scope)).unwrap()
}

#[test]
fn create_and_edit_fields_and_creators() {
    let c = memory_db();
    let coll = collection_create(&c, "毕业论文", None).unwrap();
    let id = item_create(&c, "journalArticle", Some(coll)).unwrap();
    assert_eq!(list(&c, Scope::Collection { id: coll, include_children: false }).len(), 1);
    let before: String = one(&c, "SELECT updated_at FROM items");

    let mut p = patch(&[
        ("title", "  互联网使用与政治效能感  "),
        ("date", "2019年3月"),
        ("container", "合成社会学研究"),
        ("DOI", "https://doi.org/10.1/ABC"),
        ("volume", "36"),
    ]);
    p.creators = Some(vec![
        creator("author", Some("Smith"), Some("Alice"), None),
        creator("author", Some("忽略"), None, Some("王小明")),
        creator("editor", Some("  "), None, None),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert_eq!(item_update(&c, id, &p).unwrap(), vec![DocRef::item(id)]);
    let (title, norm, sort, year, container, doi, lang, updated): (String, String, String, i64, String, String, String, String) = c
        .query_row("SELECT title, title_norm, date_sort, year, container_title, doi, detected_lang, updated_at FROM items WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
        })
        .unwrap();
    assert_eq!((title.as_str(), norm.as_str(), sort.as_str(), year), ("互联网使用与政治效能感", "互联网使用与政治效能感", "2019-03-00", 2019));
    assert_eq!((container.as_str(), doi.as_str(), lang.as_str()), ("合成社会学研究", "10.1/ABC", "zh"));
    assert!(updated > before);
    assert_eq!(field(&c, id, "date").as_deref(), Some("2019-03-00 2019年3月"), "日期存成 Zotero 两段式");
    assert_eq!(field(&c, id, "publicationTitle").as_deref(), Some("合成社会学研究"), "期刊文章的「期刊」是 publicationTitle");
    assert_eq!(field(&c, id, "DOI").as_deref(), Some("https://doi.org/10.1/ABC"), "字段存原值，列是规范化的");
    type Row = (String, Option<String>, Option<String>, Option<String>, i64);
    let creators: Vec<Row> = c
        .prepare("SELECT role, family, given, literal, position FROM creators WHERE item_id = ?1 ORDER BY position")
        .unwrap()
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(creators, vec![("author".into(), Some("Smith".into()), Some("Alice".into()), None, 0), ("author".into(), None, None, Some("王小明".into()), 1)], "空行不存，有单栏姓名时只存单栏");

    // 删除字段、详情
    item_update(&c, id, &patch(&[("volume", ""), ("DOI", "")])).unwrap();
    assert_eq!(field(&c, id, "volume"), None);
    assert_eq!(one::<Option<String>>(&c, "SELECT doi FROM items"), None);
    let d = library::item_get(&c, id).unwrap();
    assert_eq!((d.title.as_deref(), d.date.as_deref(), d.container_field.as_deref()), (Some("互联网使用与政治效能感"), Some("2019年3月"), Some("publicationTitle")));

    // 不合法的输入
    assert!(item_update(&c, id, &patch(&[("bad name", "x")])).is_err());
    assert!(item_update(&c, id, &ItemPatch { item_type: Some("not a type".into()), ..Default::default() }).is_err());
    assert!(item_update(&c, 999, &patch(&[("title", "x")])).is_err());
    let bad = ItemPatch { creators: Some(vec![creator("bad role", Some("x"), None, None)]), ..Default::default() };
    assert!(item_update(&c, id, &bad).is_err());
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM creators"), 2, "失败的修改整体回滚");
    assert!(item_create(&c, "", None).is_err());
}

#[test]
fn changing_type_moves_container_field() {
    let c = memory_db();
    let id = item_create(&c, "journalArticle", None).unwrap();
    item_update(&c, id, &patch(&[("container", "Some Journal")])).unwrap();
    item_update(&c, id, &ItemPatch { item_type: Some("bookSection".into()), ..Default::default() }).unwrap();
    assert_eq!((field(&c, id, "publicationTitle"), field(&c, id, "bookTitle").as_deref()), (None, Some("Some Journal")));
    assert_eq!(one::<String>(&c, "SELECT container_title FROM items"), "Some Journal");
    // 图书没有「期刊 / 书名」：值留在原字段，不丢
    item_update(&c, id, &ItemPatch { item_type: Some("book".into()), ..Default::default() }).unwrap();
    assert_eq!(field(&c, id, "bookTitle").as_deref(), Some("Some Journal"));
    let fresh = item_create(&c, "book", None).unwrap();
    assert!(item_update(&c, fresh, &patch(&[("container", "x")])).unwrap_err().contains("没有"));
}

#[test]
fn batch_set_field() {
    let c = memory_db();
    let a = item_create(&c, "journalArticle", None).unwrap();
    let b = item_create(&c, "book", None).unwrap();
    items_set_field(&c, &[a, b], "language", "zh-CN").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM items WHERE language = 'zh-CN'"), 2);
    items_set_field(&c, &[a, b], "itemType", "report").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM items WHERE item_type = 'report'"), 2);
    items_set_field(&c, &[a, b], "date", "2020").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM items WHERE year = 2020"), 2);
    assert!(items_set_field(&c, &[a, 999], "language", "en").is_err());
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM items WHERE language = 'en'"), 0, "有一条失败就整批不改");
}

#[test]
fn tags_add_remove_rename_delete() {
    let c = memory_db();
    let a = item_create(&c, "book", None).unwrap();
    let b = item_create(&c, "book", None).unwrap();
    c.execute("INSERT INTO notes (uid, html, created_at, updated_at) VALUES ('N1', '<p>独立笔记</p>', 't', 't')", []).unwrap();
    let note = Target { kind: Kind::Note, id: c.last_insert_rowid() };
    tags_add(&c, &[Target::item(a), Target::item(b), note], " 政治效能感 ").unwrap();
    tags_add(&c, &[Target::item(a)], "政治效能感").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM tags"), 1);
    assert_eq!(library::tags_list(&c).unwrap()[0].count, 3);
    assert!(tags_add(&c, &[Target { kind: Kind::Collection, id: 1 }], "x").is_err());
    assert!(tags_add(&c, &[Target::item(a)], "  ").is_err());

    tags_add(&c, &[Target::item(a)], "survey").unwrap();
    let survey: i64 = one(&c, "SELECT id FROM tags WHERE name = 'survey'");
    let main: i64 = one(&c, "SELECT id FROM tags WHERE name = '政治效能感'");
    // 改名成已有的名字 = 合并
    let users = tag_rename(&c, survey, "政治效能感").unwrap();
    assert_eq!(users, vec![DocRef::item(a)]);
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM tags"), 1);
    tag_rename(&c, main, "efficacy").unwrap();
    assert_eq!(one::<String>(&c, "SELECT name FROM tags"), "efficacy");

    tags_remove(&c, &[Target::item(a), Target::item(b)], "efficacy").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM tags"), 1, "笔记还在用");
    tags_remove(&c, &[note], "efficacy").unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM tags"), 0, "没人用的标签删掉");

    tags_add(&c, &[Target::item(a), Target::item(b)], "x").unwrap();
    let x: i64 = one(&c, "SELECT id FROM tags");
    assert_eq!(tag_delete(&c, x).unwrap().len(), 2);
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM item_tags"), 0);
}

#[test]
fn collections_manage_and_membership() {
    let c = memory_db();
    let root = collection_create(&c, "毕业论文", None).unwrap();
    let sub = collection_create(&c, "文献综述", Some(root)).unwrap();
    let deep = collection_create(&c, "理论", Some(sub)).unwrap();
    assert!(collection_create(&c, " ", None).is_err());
    collection_rename(&c, sub, "综述").unwrap();
    assert!(collection_move(&c, root, Some(deep)).unwrap_err().contains("下级"), "不能移进自己的下级");
    assert!(collection_move(&c, root, Some(root)).is_err());
    collection_move(&c, deep, None).unwrap();
    assert_eq!(one::<Option<i64>>(&c, &format!("SELECT parent_id FROM collections WHERE id = {deep}")), None);

    let item = item_create(&c, "book", None).unwrap();
    c.execute("INSERT INTO attachments (uid, kind, created_at, updated_at) VALUES ('A1', 'linked_url', 't', 't')", []).unwrap();
    let standalone = Target { kind: Kind::Attachment, id: c.last_insert_rowid() };
    c.execute("INSERT INTO attachments (uid, item_id, kind, created_at, updated_at) VALUES ('A2', ?1, 'linked_url', 't', 't')", [item]).unwrap();
    let child = Target { kind: Kind::Attachment, id: c.last_insert_rowid() };
    assert_eq!(collection_add(&c, sub, &[Target::item(item), standalone, child]).unwrap(), 2, "子附件跟着文献，不单独进集合");
    assert_eq!(collection_add(&c, sub, &[Target::item(item)]).unwrap(), 0, "重复加入不算");
    assert_eq!(list(&c, Scope::Collection { id: sub, include_children: false }).len(), 2);
    assert_eq!(list(&c, Scope::Collection { id: root, include_children: false }).len(), 0);
    assert_eq!(list(&c, Scope::Collection { id: root, include_children: true }).len(), 2, "连同下级集合");
    assert_eq!(list(&c, Scope::Unfiled).len(), 0);
    assert_eq!(collection_remove(&c, sub, &[Target::item(item)]).unwrap(), 1);
    assert_eq!(list(&c, Scope::Unfiled).len(), 1, "移出集合后算未分类");

    // 集合进回收站：下级随它，文献不动
    trash(&c, &[Target { kind: Kind::Collection, id: root }]).unwrap();
    assert!(library::collections_list(&c).unwrap().iter().all(|n| n.id != sub));
    assert!(collection_add(&c, sub, &[Target::item(item)]).unwrap_err().contains("回收站"), "上级在回收站，下级也算在回收站");
    assert!(collection_create(&c, "新子集合", Some(sub)).is_err());
    // 恢复下级集合时上级一起回来
    trash(&c, &[Target { kind: Kind::Collection, id: sub }]).unwrap();
    restore(&c, &[Target { kind: Kind::Collection, id: sub }]).unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM collections WHERE deleted_at IS NOT NULL"), 0);
}

struct Env {
    _dir: TempDir,
    paths: DataPaths,
    c: Connection,
}

fn zotero_env(name: &str) -> Env {
    let dir = temp_dir(name);
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    let fx = zotero_import::fixture::build(&dir.join("fx")).unwrap();
    crate::import::zotero::run(&c, &paths, &fx.zotero_dir, &crate::import::zotero::Options { base_dir: Some(fx.base_dir.clone()) }).unwrap();
    Env { _dir: dir, paths, c }
}

fn id_of(c: &Connection, table: &str, key: &str) -> i64 {
    c.query_row(&format!("SELECT id FROM {table} WHERE source_key = ?1"), [key], |r| r.get(0)).unwrap()
}

fn store_files(p: &DataPaths) -> usize {
    fn walk(d: &Path) -> usize {
        std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 }).sum()).unwrap_or(0)
    }
    walk(&p.attachments)
}

#[test]
fn trash_restore_and_delete_forever_with_file_cleanup() {
    let e = zotero_env("edit-trash");
    let c = &e.c;
    let files_before = store_files(&e.paths);
    assert_eq!(files_before as i64, one::<i64>(c, "SELECT COUNT(*) FROM files"));

    // 恢复子附件：父条目一起回来
    let att12 = id_of(c, "attachments", "ATT00012");
    let j5 = id_of(c, "items", "JRNL0005");
    trash(c, &[Target { kind: Kind::Attachment, id: att12 }]).unwrap();
    let refs = restore(c, &[Target { kind: Kind::Attachment, id: att12 }]).unwrap();
    assert!(refs.contains(&DocRef::item(j5)));
    assert_eq!(one::<Option<String>>(c, &format!("SELECT deleted_at FROM items WHERE id = {j5}")), None);

    // 只能永久删除回收站里的
    assert!(delete_forever(c, &e.paths.backups, &e.paths.attachments, &[Target::item(j5)]).unwrap_err().contains("回收站"));

    // 删 JRNL0005：它的 PDF（trashed.pdf）没人再用，文件记录删掉；文件本身还被「永久删除前」的备份用到，先留着
    trash(c, &[Target::item(j5)]).unwrap();
    let sha12: String = one(c, &format!("SELECT sha256 FROM attachments WHERE id = {att12}"));
    let (report, refs) = delete_forever(c, &e.paths.backups, &e.paths.attachments, &[Target::item(j5)]).unwrap();
    assert_eq!((report.records, report.files_removed), (1, 0));
    assert!(report.backup_file.as_deref().is_some_and(|f| f.ends_with("-before-delete.db")));
    assert_eq!(refs, vec![DocRef::item(j5)]);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM attachments WHERE id = {att12}")), 0, "子附件随之删除");
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM notes WHERE source_key = 'NOTE0003'"), 0, "子笔记随之删除");
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM files WHERE sha256 = '{sha12}'")), 0);
    assert_eq!(store_files(&e.paths), files_before, "备份还用到，文件留着");

    // 删带内嵌图片的笔记：图片随笔记删除，但同样内容还被快照（ATT00002 的资源文件）引用，文件保留
    let note2 = id_of(c, "notes", "NOTE0002");
    let png: String = one(c, "SELECT sha256 FROM attachments WHERE source_key = 'ATT00011'");
    trash(c, &[Target { kind: Kind::Note, id: note2 }]).unwrap();
    let (report, _) = delete_forever(c, &e.paths.backups, &e.paths.attachments, &[Target { kind: Kind::Note, id: note2 }]).unwrap();
    assert_eq!(report.files_removed, 0);
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM attachments WHERE source_key = 'ATT00011'"), 0);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM files WHERE sha256 = '{png}'")), 1, "快照资源还在用");

    // 子附件直接删除后永久删除：所属文献的索引要重写
    let j1 = id_of(c, "items", "JRNL0001");
    let att13 = id_of(c, "attachments", "ATT00013");
    let (_, refs) = delete_forever(c, &e.paths.backups, &e.paths.attachments, &[Target { kind: Kind::Attachment, id: att13 }]).unwrap();
    assert!(refs.contains(&DocRef::item(j1)));

    // 清空回收站
    let (report, _) = empty_trash(c, &e.paths.backups, &e.paths.attachments).unwrap();
    assert_eq!(report.records, 2, "NOTE0004 与集合 COLL0006（还有随它的 COLL0007）");
    assert_eq!(library::overview(c).unwrap().trash, 0);
    assert_eq!(one::<i64>(c, "SELECT COUNT(*) FROM collections WHERE source_key IN ('COLL0006', 'COLL0007')"), 0);
    assert_eq!(empty_trash(c, &e.paths.backups, &e.paths.attachments).unwrap().0.records, 0);
    let deletes = crate::backup::list(&e.paths.backups).unwrap().into_iter().filter(|b| b.label == "before-delete").count();
    assert!((1..=crate::backup::KEEP_SAFETY).contains(&deletes), "永久删除前的备份只留最近几份");

    // 备份都清理掉之后，没人用的文件才删；文件库与记录一致
    std::fs::remove_dir_all(&e.paths.backups).unwrap();
    assert!(crate::backup::sweep_files(c, &e.paths.backups, &e.paths.attachments).unwrap() >= 1);
    assert!(!e.paths.attachments.join(store::rel_path(&sha12, "pdf")).exists());
    assert_eq!(store_files(&e.paths) as i64, one::<i64>(c, "SELECT COUNT(*) FROM files"), "文件库与记录一致");
}

#[test]
fn merge_duplicates() {
    let e = zotero_env("edit-merge");
    let c = &e.c;
    let master = item_create(c, "journalArticle", None).unwrap();
    item_update(c, master, &patch(&[("title", "Synthetic Panel Study of Efficacy and Voting")])).unwrap();
    let other = id_of(c, "items", "JRNL0001");
    let other_uid: String = one(c, &format!("SELECT uid FROM items WHERE id = {other}"));
    let refs = items_merge(c, master, &[other]).unwrap();
    assert_eq!(refs, vec![DocRef::item(master), DocRef::item(other)]);
    // 字段：master 没有的从另一条补上
    assert_eq!(one::<String>(c, &format!("SELECT doi FROM items WHERE id = {master}")), "10.0000/synthetic.2019.001");
    assert_eq!(field(c, master, "title").as_deref(), Some("Synthetic Panel Study of Efficacy and Voting"));
    // 子附件、子笔记、标签、集合、作者都并过来
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM attachments WHERE item_id = {master}")), 2);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM notes WHERE item_id = {master}")), 1);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM item_tags WHERE item_id = {master}")), 2);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM collection_items WHERE item_id = {master}")), 2);
    assert_eq!(one::<i64>(c, &format!("SELECT COUNT(*) FROM creators WHERE item_id = {master}")), 2);
    assert_eq!(one::<String>(c, &format!("SELECT object FROM item_relations WHERE item_id = {master} AND predicate = 'dc:replaces'")), format!("researchlab:item:{other_uid}"));
    assert!(one::<Option<String>>(c, &format!("SELECT deleted_at FROM items WHERE id = {other}")).is_some(), "另一条进回收站");
    assert!(items_merge(c, master, &[master]).is_err());
    assert!(items_merge(c, master, &[other]).unwrap_err().contains("回收站"));
}

#[test]
fn add_and_rename_attachments() {
    let dir = temp_dir("edit-attach");
    let c = memory_db();
    let root = dir.join("attachments");
    let src = dir.join("Smith 2019.PDF");
    std::fs::write(&src, b"%PDF-1.4 synthetic").unwrap();
    let item = item_create(&c, "journalArticle", None).unwrap();
    let (a, refs) = attachment_add(&c, &root, Some(item), &src).unwrap();
    assert_eq!(refs, vec![DocRef::attachment(a)]);
    let (title, ctype, status, sha): (String, String, String, String) =
        c.query_row("SELECT title, content_type, status, sha256 FROM attachments WHERE id = ?1", [a], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert_eq!((title.as_str(), ctype.as_str(), status.as_str()), ("Smith 2019.PDF", "application/pdf", "ok"));
    assert!(root.join(&sha[..2]).join(format!("{sha}.pdf")).is_file());
    assert!(src.is_file(), "原文件不动");
    let (b, _) = attachment_add(&c, &root, None, &src).unwrap();
    assert_eq!(one::<i64>(&c, "SELECT COUNT(*) FROM files"), 1, "同样内容只存一份");
    assert_eq!(list(&c, Scope::All).iter().filter(|r| r.kind == "attachment").count(), 1, "独立附件出现在列表里");
    attachment_rename(&c, b, "新名字.pdf").unwrap();
    assert_eq!(one::<String>(&c, &format!("SELECT title FROM attachments WHERE id = {b}")), "新名字.pdf");
    assert!(attachment_rename(&c, b, "").is_err());
    assert!(attachment_add(&c, &root, None, &dir.join("missing.pdf")).is_err());
    assert!(attachment_add(&c, &root, Some(999), &src).is_err());
}
