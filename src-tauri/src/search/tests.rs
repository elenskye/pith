//! 搜索索引测试：合成数据（SQL 直接插入，或 `zotero_import::fixture`），不含真实数据。
//! 两个默认忽略的：5000 条合成文献的重建基准、真实副本上两种中文分词方案的召回对比（只打印聚合数字）。

use super::*;
use crate::common::new_uid;
use crate::db::tests::{memory_db, temp_dir};
use std::path::PathBuf;

const T: &str = "2026-11-08T00:00:00.000Z";

fn ins(c: &Connection, sql: &str, p: impl rusqlite::Params) -> i64 {
    c.execute(sql, p).unwrap();
    c.last_insert_rowid()
}

fn item(c: &Connection, title: &str, deleted: Option<&str>) -> i64 {
    ins(
        c,
        "INSERT INTO items (uid, item_type, title, source, created_at, updated_at, deleted_at) VALUES (?1, 'journalArticle', ?2, 'manual', ?3, ?3, ?4)",
        params![new_uid().unwrap(), title, T, deleted],
    )
}

fn note(c: &Connection, item: Option<i64>, html: &str, deleted: Option<&str>) -> i64 {
    ins(
        c,
        "INSERT INTO notes (uid, item_id, html, created_at, updated_at, deleted_at) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        params![new_uid().unwrap(), item, html, T, deleted],
    )
}

fn attachment(c: &Connection, item: Option<i64>, kind: &str, title: Option<&str>, name: &str, deleted: Option<&str>) -> i64 {
    ins(
        c,
        "INSERT INTO attachments (uid, item_id, kind, title, original_name, created_at, updated_at, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
        params![new_uid().unwrap(), item, kind, title, name, T, deleted],
    )
}

fn annotation(c: &Connection, att: i64, text: Option<&str>, comment: Option<&str>) -> i64 {
    ins(
        c,
        "INSERT INTO annotations (uid, attachment_id, kind, page_index, position, sort_key, text, comment, created_at, updated_at)
         VALUES (?1, ?2, 'highlight', 0, '{}', '00000|000000|00000', ?3, ?4, ?5, ?5)",
        params![new_uid().unwrap(), att, text, comment, T],
    )
}

/// 给 item_tags / attachment_tags / note_tags 加一个标签
fn tag(c: &Connection, table: &str, col: &str, id: i64, name: &str) {
    c.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [name]).unwrap();
    c.execute(&format!("INSERT OR IGNORE INTO {table} ({col}, tag_id) SELECT ?1, id FROM tags WHERE name = ?2"), params![id, name]).unwrap();
}

#[derive(Debug, Clone, Copy)]
struct Seed {
    /// 中文文献：标题、摘要（中英混合）、单栏作者、标签、期刊 / DOI / 出版者等字段
    zh: i64,
    /// 西文文献：带重音的作者、子笔记、子附件（带标注）、回收站里的子笔记 / 子附件
    en: i64,
    /// 回收站里的文献
    trashed: i64,
    child_note: i64,
    child_att: i64,
    alone_att: i64,
    alone_note: i64,
    /// 独立笔记里的内嵌图片
    image: i64,
}

fn seed(c: &Connection) -> Seed {
    let zh = item(c, "政治效能感研究", None);
    c.execute(
        "UPDATE items SET abstract = '本文讨论 internal efficacy 与选举参与。', year = 2020, container_title = '合成期刊', doi = '10.1234/abc.2020.01' WHERE id = ?1",
        [zh],
    )
    .unwrap();
    c.execute("INSERT INTO creators (item_id, role, literal, position) VALUES (?1, 'author', '张三', 0)", [zh]).unwrap();
    tag(c, "item_tags", "item_id", zh, "政治心理");
    for (f, v) in [
        ("title", "政治效能感研究"),
        ("publicationTitle", "合成期刊"),
        ("publisher", "合成出版社"),
        ("volume", "36"),
        ("date", "2020-00-00 2020"),
        ("filingDate", "2019-03-00 March 2019"),
        ("language", "zh-CN"),
        ("bibtex:howpublished", "rawsourcevalue"),
    ] {
        c.execute("INSERT INTO item_fields (item_id, field, value) VALUES (?1, ?2, ?3)", params![zh, f, v]).unwrap();
    }

    let en = item(c, "Political Efficacy and Voting", None);
    c.execute("UPDATE items SET abstract = 'We examine turnout.' WHERE id = ?1", [en]).unwrap();
    c.execute("INSERT INTO creators (item_id, role, family, given, position) VALUES (?1, 'author', 'Müller', 'Hans', 0)", [en]).unwrap();
    c.execute("INSERT INTO creators (item_id, role, family, given, position) VALUES (?1, 'author', 'Campbell', 'Angus', 1)", [en]).unwrap();
    tag(c, "item_tags", "item_id", en, "Survey Methods");
    let child_note = note(c, Some(en), "<h1>读书笔记</h1><p>内部效能感 zeitgeist</p>", None);
    tag(c, "note_tags", "note_id", child_note, "笔记标签");
    note(c, Some(en), "<p>回收站里的子笔记 obsoleteword</p>", Some(T));
    let child_att = attachment(c, Some(en), "stored", Some("Full Text PDF"), "campbell-1954.pdf", None);
    annotation(c, child_att, Some("civic duty matters"), Some("重要观点"));
    attachment(c, Some(en), "stored", Some("Full Text PDF"), "superseded-draft.pdf", Some(T));

    let trashed = item(c, "A Trashed Article", Some(T));

    let alone_att = attachment(c, None, "stored", None, "standalone-report.pdf", None);
    annotation(c, alone_att, Some("standalone highlight"), None);
    tag(c, "attachment_tags", "attachment_id", alone_att, "报告");
    let alone_note = note(c, None, "<p>独立笔记关于分工</p>", None);
    tag(c, "note_tags", "note_id", alone_note, "随笔");
    let image = ins(
        c,
        "INSERT INTO attachments (uid, note_id, kind, original_name, created_at, updated_at) VALUES (?1, ?2, 'embedded_image', 'image.png', ?3, ?3)",
        params![new_uid().unwrap(), alone_note, T],
    );
    Seed { zh, en, trashed, child_note, child_att, alone_att, alone_note, image }
}

fn env() -> (Connection, Seed) {
    let c = memory_db();
    attach_memory(&c).unwrap();
    let s = seed(&c);
    assert_eq!(rebuild(&c).unwrap(), 5, "3 条文献 + 独立附件 + 独立笔记");
    (c, s)
}

fn hits(c: &Connection, q: &str) -> Vec<DocRef> {
    let mut v: Vec<DocRef> = query(c, q).unwrap().expect("非空查询").into_keys().collect();
    v.sort();
    v
}

/// 测试用的确定性伪随机数（xorshift）
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn doc_count(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM search.docs", [], |r| r.get(0)).unwrap()
}

#[test]
fn bigrams_and_query_expressions() {
    assert_eq!(index_text("政治效能感"), " 政治 治效 效能 能感 感 ");
    assert_eq!(index_text("AI时代的Müller"), "AI 时代 代的 的 Müller");
    assert_eq!(index_text("ＡＢＣ　中"), "ABC  中 ");
    let p = |q: &str| parse_query(q).fts;
    assert_eq!(p("效能感"), [r#""效能 能感""#]);
    assert_eq!(p("感"), [r#""感"*"#]);
    assert_eq!(p("effic"), [r#""effic"*"#]);
    assert_eq!(p("10.1000/abc"), [r#""10 1000 abc"*"#]);
    assert_eq!(p("AI时代"), [r#""AI"*"#, r#""时代""#]);
    assert_eq!(p("\"NEAR(a b)\" -x a:b *"), [r#""NEAR a"*"#, r#""b"*"#, r#""x"*"#, r#""a b"*"#]);
    assert_eq!(parse_query("🔥 #").literals, ["🔥", "#"]);
    assert!(parse_query("🔥").fts.is_empty());
    assert_eq!(parse_query(&"词 ".repeat(100)).fts.len(), MAX_TERMS);
    for r in [DocRef::item(1), DocRef::attachment(7), DocRef::note(0), DocRef::item(-5), DocRef::note(i64::MAX / 4 - 1)] {
        assert_eq!(from_rowid(rowid(r).unwrap()), Some(r));
    }
    assert!(rowid(DocRef::item(i64::MAX / 2)).is_err());
}

#[test]
fn chinese_one_two_and_many_characters() {
    let (c, s) = env();
    let (zh, en) = (DocRef::item(s.zh), DocRef::item(s.en));
    assert_eq!(hits(&c, "效"), [zh, en], "单字：标题里的、子笔记里的都算");
    assert_eq!(hits(&c, "感"), [zh, en], "词尾的单字");
    assert_eq!(hits(&c, "效能"), [zh, en]);
    assert_eq!(hits(&c, "政治效能感"), [zh]);
    assert_eq!(hits(&c, "能感研"), [zh], "词中间的子串");
    assert_eq!(hits(&c, "感研究"), [zh]);
    assert!(hits(&c, "效感").is_empty(), "不相邻的两个字不算");
    assert!(hits(&c, "研究政治").is_empty());
    assert_eq!(hits(&c, "政治　效能"), [zh], "全角空格也是分隔");
    assert_eq!(hits(&c, "选举参与"), [zh], "摘要");
}

#[test]
fn western_prefix_case_and_diacritics() {
    let (c, s) = env();
    let (zh, en) = (DocRef::item(s.zh), DocRef::item(s.en));
    assert_eq!(hits(&c, "effic"), [zh, en], "前缀：标题与摘要");
    assert_eq!(hits(&c, "EFFICACY"), [zh, en]);
    assert_eq!(hits(&c, "Ｅｆｆｉｃ"), [zh, en], "全角字母");
    assert!(hits(&c, "ffica").is_empty(), "只做词首前缀，不找词中间");
    assert_eq!(hits(&c, "muller"), [en], "重音不敏感");
    assert_eq!(hits(&c, "MÜLL"), [en]);
    assert_eq!(hits(&c, "Mu\u{308}ller"), [en], "分解形式的重音");
    assert_eq!(hits(&c, "polit"), [en]);
}

#[test]
fn creators_tags_abstract_and_other_fields() {
    let (c, s) = env();
    let (zh, en) = (DocRef::item(s.zh), DocRef::item(s.en));
    assert_eq!(hits(&c, "张三"), [zh]);
    assert_eq!(hits(&c, "campbell"), [en]);
    assert_eq!(hits(&c, "Hans"), [en], "名也索引");
    assert_eq!(hits(&c, "政治心理"), [zh], "标签");
    assert_eq!(hits(&c, "survey meth"), [en]);
    assert_eq!(hits(&c, "turnout"), [en], "摘要");
    assert_eq!(hits(&c, "合成期刊"), [zh], "期刊名");
    assert_eq!(hits(&c, "合成出版社"), [zh], "item_fields 里的出版者");
    assert_eq!(hits(&c, "36"), [zh], "卷号");
    assert_eq!(hits(&c, "2020"), [zh], "年份");
    assert_eq!(hits(&c, "March"), [zh], "日期字段取原文");
    assert_eq!(hits(&c, "10.1234/abc"), [zh], "DOI 片段");
    assert!(hits(&c, "rawsourcevalue").is_empty(), "bibtex: 原始字段不进索引");
    assert!(hits(&c, "zh").is_empty(), "语言代码不进索引");
}

#[test]
fn children_hit_their_parent() {
    let (c, s) = env();
    let en = DocRef::item(s.en);
    assert_eq!(hits(&c, "zeitgeist"), [en], "子笔记正文");
    assert_eq!(hits(&c, "读书笔记"), [en], "子笔记标题");
    assert_eq!(hits(&c, "笔记标签"), [en], "子笔记的标签");
    assert_eq!(hits(&c, "civic duty"), [en], "标注原文");
    assert_eq!(hits(&c, "重要观点"), [en], "标注评论");
    assert_eq!(hits(&c, "campbell-1954"), [en], "子附件原文件名");
    assert!(hits(&c, "obsoleteword").is_empty(), "回收站里的子笔记不并进上级");
    assert!(hits(&c, "superseded").is_empty(), "回收站里的子附件不并进上级");
    assert!(!hits(&c, "image").contains(&DocRef::attachment(s.image)), "内嵌图片不是文档");
}

#[test]
fn standalone_rows_and_trash_are_documents() {
    let (c, s) = env();
    assert_eq!(hits(&c, "standalone"), [DocRef::attachment(s.alone_att)], "独立附件：原文件名、标注");
    assert_eq!(hits(&c, "报告"), [DocRef::attachment(s.alone_att)], "附件标签");
    assert_eq!(hits(&c, "分工"), [DocRef::note(s.alone_note)], "独立笔记");
    assert_eq!(hits(&c, "随笔"), [DocRef::note(s.alone_note)], "笔记标签");
    assert_eq!(hits(&c, "trashed"), [DocRef::item(s.trashed)], "回收站里的文献也能找到");
}

#[test]
fn words_are_anded_and_title_ranks_first() {
    let (c, s) = env();
    let (zh, en) = (DocRef::item(s.zh), DocRef::item(s.en));
    assert_eq!(hits(&c, "political voting"), [en]);
    assert_eq!(hits(&c, "efficacy 政治"), [zh]);
    assert!(hits(&c, "political 政治").is_empty());
    let r = query(&c, "efficacy").unwrap().unwrap();
    assert!(r[&en] < r[&zh], "标题命中排在摘要命中前面：{r:?}");
}

#[test]
fn syntax_characters_are_plain_text() {
    let (c, s) = env();
    assert_eq!(query(&c, "").unwrap(), None);
    assert_eq!(query(&c, " \t\n　").unwrap(), None, "全角空格也算空白");
    for q in [
        "\"", "\"\"", "*", "**", "(", ")", "()", "NEAR", "NEAR(", "NEAR(a b, 2)", "-x", "a:b", "title:政治", "^a", "a OR b", "AND", "NOT x", "{title}: x",
        "+", "-", "!", "，。！", "...", "'", "\\", "%", "_", "\0", "\u{200b}", "🔥", "Ⓐ", "\u{308}", "a\"b", "\"政治", "效能*",
    ] {
        assert!(query(&c, q).unwrap().is_some(), "{q:?}");
    }
    assert_eq!(hits(&c, "\"political\""), [DocRef::item(s.en)], "引号当普通文字");
    assert_eq!(hits(&c, "-turnout"), [DocRef::item(s.en)], "减号不是 NOT");
    assert_eq!(hits(&c, "examine:turnout"), [DocRef::item(s.en)], "冒号不是列过滤，标点连着的词是短语");
    assert!(hits(&c, "abstract:turnout").is_empty());
    assert!(hits(&c, "，。！").is_empty());
    // 随机字符串：不报错、不 panic
    let pool: Vec<char> = "\"*():^-+_.,;'!?/\\{}[]<>~@#$%&=| \t\nazZéü\u{308}政治の한Ⓐ🔥\0\u{200b}　Ａ".chars().collect();
    let words = ["NEAR", "AND", "OR", "NOT", "title:", "\"x\"", "x*", "-", "(", ")"];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2000 {
        let len = rng.next() % 12;
        let mut q = String::new();
        for _ in 0..len {
            let r = rng.next();
            if r.is_multiple_of(5) {
                q.push_str(words[(r / 5) as usize % words.len()]);
            } else {
                q.push(pool[(r / 5) as usize % pool.len()]);
            }
        }
        assert!(query(&c, &q).is_ok(), "{q:?}");
    }
    let long: String = (0..3000).map(|i| pool[i * 7 % pool.len()]).collect();
    assert!(query(&c, &long).is_ok());
    assert!(query(&c, &"政治效能感".repeat(200)).unwrap().unwrap().is_empty());
}

#[test]
fn symbol_words_match_original_text() {
    let (c, s) = env();
    c.execute("UPDATE items SET title = 'Hot 🔥 topics' WHERE id = ?1", [s.zh]).unwrap();
    update(&c, &[DocRef::item(s.zh)]).unwrap();
    assert_eq!(hits(&c, "🔥"), [DocRef::item(s.zh)]);
    assert_eq!(hits(&c, "🔥 hot"), [DocRef::item(s.zh)]);
    assert!(hits(&c, "🔥 turnout").is_empty());
}

#[test]
fn update_rewrites_changed_rows() {
    let (c, s) = env();
    c.execute("UPDATE items SET title = '全新标题' WHERE id = ?1", [s.zh]).unwrap();
    c.execute("INSERT INTO creators (item_id, role, literal, position) VALUES (?1, 'author', '李四', 1)", [s.zh]).unwrap();
    assert!(hits(&c, "全新").is_empty(), "没 update 之前索引不变");
    update(&c, &[DocRef::item(s.zh)]).unwrap();
    assert_eq!(hits(&c, "全新"), [DocRef::item(s.zh)]);
    assert_eq!(hits(&c, "李四"), [DocRef::item(s.zh)]);
    assert!(hits(&c, "政治效能感").is_empty(), "旧标题不再命中");
    assert_eq!(doc_count(&c), 5);
    // 子项移进回收站：并进上级的内容随之去掉
    c.execute("UPDATE notes SET deleted_at = ?2 WHERE id = ?1", params![s.child_note, T]).unwrap();
    update(&c, &[DocRef::note(s.child_note)]).unwrap();
    assert!(hits(&c, "zeitgeist").is_empty());
    update(&c, &[]).unwrap();
}

#[test]
fn update_deletes_missing_rows() {
    let (c, s) = env();
    c.execute("DELETE FROM items WHERE id = ?1", [s.trashed]).unwrap();
    c.execute("DELETE FROM attachments WHERE id = ?1", [s.alone_att]).unwrap();
    c.execute("DELETE FROM notes WHERE id = ?1", [s.alone_note]).unwrap();
    update(&c, &[DocRef::item(s.trashed), DocRef::attachment(s.alone_att), DocRef::note(s.alone_note)]).unwrap();
    assert!(hits(&c, "trashed").is_empty());
    assert!(hits(&c, "standalone").is_empty());
    assert!(hits(&c, "分工").is_empty());
    assert_eq!(doc_count(&c), 2);
    update(&c, &[DocRef::item(999), DocRef::note(999), DocRef::attachment(999)]).unwrap();
    assert!(!ensure_fresh(&c).unwrap(), "与库一致");
}

#[test]
fn update_redirects_children_to_parent() {
    let (c, s) = env();
    let en = DocRef::item(s.en);
    // 新的子笔记：传子笔记，重写的是文献
    let n = note(&c, Some(s.en), "<p>newnoteword</p>", None);
    update(&c, &[DocRef::note(n)]).unwrap();
    assert_eq!(hits(&c, "newnoteword"), [en]);
    // 子附件的标注
    annotation(&c, s.child_att, Some("freshannotation"), None);
    update(&c, &[DocRef::attachment(s.child_att)]).unwrap();
    assert_eq!(hits(&c, "freshannotation"), [en]);
    // 内嵌图片 → 所属的独立笔记
    c.execute("UPDATE notes SET html = '<p>改过的独立笔记</p>' WHERE id = ?1", [s.alone_note]).unwrap();
    update(&c, &[DocRef::attachment(s.image)]).unwrap();
    assert_eq!(hits(&c, "改过"), [DocRef::note(s.alone_note)]);
    assert!(hits(&c, "分工").is_empty());
    // 独立笔记挂到文献下：它自己的文档删掉，内容并进文献
    c.execute("UPDATE notes SET item_id = ?2 WHERE id = ?1", [s.alone_note, s.zh]).unwrap();
    update(&c, &[DocRef::note(s.alone_note)]).unwrap();
    assert_eq!(hits(&c, "改过"), [DocRef::item(s.zh)]);
    // 独立附件挂到文献下
    c.execute("UPDATE attachments SET item_id = ?2 WHERE id = ?1", [s.alone_att, s.zh]).unwrap();
    update(&c, &[DocRef::attachment(s.alone_att)]).unwrap();
    assert_eq!(hits(&c, "standalone"), [DocRef::item(s.zh)]);
    assert_eq!(doc_count(&c), 3);
    assert!(!ensure_fresh(&c).unwrap());
}

#[test]
fn update_run_indexes_one_import() {
    let (c, s) = env();
    let run = ins(&c, "INSERT INTO import_runs (source, started_at) VALUES ('bibtex', ?1)", [T]);
    let a = item(&c, "Imported Paper", None);
    let n = note(&c, None, "<p>importednote</p>", None);
    let child = note(&c, Some(s.zh), "<p>importedchild</p>", None);
    for (table, id) in [("items", a), ("notes", n), ("notes", child)] {
        c.execute(&format!("UPDATE {table} SET import_run_id = ?1 WHERE id = ?2"), [run, id]).unwrap();
    }
    let other = item(&c, "Not From This Run", None);
    update_run(&c, run).unwrap();
    assert_eq!(hits(&c, "imported paper"), [DocRef::item(a)]);
    assert_eq!(hits(&c, "importednote"), [DocRef::note(n)]);
    assert_eq!(hits(&c, "importedchild"), [DocRef::item(s.zh)], "子项由这次导入带来的上级也重写");
    assert!(hits(&c, "this run").is_empty(), "别的行不动");
    let _ = other;
    assert_eq!(doc_count(&c), 7);
    update_run(&c, run).unwrap();
    assert_eq!(doc_count(&c), 7, "重复调用不重复写");
}

#[test]
fn ensure_fresh_rebuilds_when_out_of_sync() {
    let c = memory_db();
    attach_memory(&c).unwrap();
    assert!(!ensure_fresh(&c).unwrap(), "空库空索引：一致");
    let s = seed(&c);
    assert!(ensure_fresh(&c).unwrap(), "从没建过");
    assert_eq!(hits(&c, "政治"), [DocRef::item(s.zh)]);
    assert!(!ensure_fresh(&c).unwrap());
    let x = item(&c, "Added Behind The Index", None);
    assert!(ensure_fresh(&c).unwrap(), "多了一行");
    assert_eq!(hits(&c, "behind"), [DocRef::item(x)]);
    // 删掉中间的一行、再加一行（新行的 id 不会和删掉的相同）
    c.execute("DELETE FROM items WHERE id = ?1", [s.trashed]).unwrap();
    item(&c, "Same Count Different Row", None);
    assert!(ensure_fresh(&c).unwrap(), "个数一样、行不一样");
    assert!(hits(&c, "trashed").is_empty());
    assert!(!ensure_fresh(&c).unwrap());
}

#[test]
fn version_change_resets_schema() {
    let (c, s) = env();
    c.execute_batch("PRAGMA search.user_version = 0; CREATE TABLE search.junk (x); CREATE VIRTUAL TABLE search.old USING fts5(a);").unwrap();
    prepare(&c).unwrap();
    let mut st = c.prepare("SELECT name FROM search.sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    let names: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(names.iter().all(|n| n == "docs" || n.starts_with("docs_")), "{names:?}");
    let v: i64 = c.query_row("PRAGMA search.user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, SEARCH_VERSION);
    assert_eq!(doc_count(&c), 0, "结构重建后内容为空");
    assert!(ensure_fresh(&c).unwrap());
    assert_eq!(hits(&c, "政治"), [DocRef::item(s.zh)]);
    prepare(&c).unwrap();
    assert_eq!(doc_count(&c), 5, "版本对时不动");
}

#[test]
fn file_index_persists_and_recovers() {
    use crate::{backup, db};
    let dir = temp_dir("search-file");
    let open = || {
        let mut c = db::open(&dir.join("library.db")).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &dir.join("backups")).unwrap();
        c
    };
    let idx = dir.join("search.db");
    let c = open();
    let s = seed(&c);
    attach(&c, &idx).unwrap();
    let mode: String = c.query_row("PRAGMA search.journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    assert!(ensure_fresh(&c).unwrap());
    attach(&c, &idx).unwrap(); // 重复 ATTACH 不出错
    // 备份只含主库
    let snap = backup::snapshot(&c, &dir.join("backups"), "manual").unwrap();
    let n: i64 = Connection::open(&snap).unwrap().query_row("SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'docs%'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    drop(c);

    // 重开：索引还在
    let c = open();
    attach(&c, &idx).unwrap();
    assert!(!ensure_fresh(&c).unwrap());
    assert_eq!(hits(&c, "政治"), [DocRef::item(s.zh)]);
    drop(c);

    // 版本号不对：结构重建
    Connection::open(&idx).unwrap().execute_batch("PRAGMA user_version = 99").unwrap();
    let c = open();
    attach(&c, &idx).unwrap();
    assert_eq!(doc_count(&c), 0);
    assert!(ensure_fresh(&c).unwrap());
    drop(c);

    assert!(!is_broken(idx.to_str().unwrap()), "好的文件不算坏");
    // 文件坏了：删掉重建
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", idx.display()));
    }
    std::fs::write(&idx, b"this is not a database at all, just junk bytes".repeat(100)).unwrap();
    assert!(is_broken(idx.to_str().unwrap()));
    let c = open();
    attach(&c, &idx).unwrap();
    assert!(ensure_fresh(&c).unwrap());
    assert_eq!(hits(&c, "政治"), [DocRef::item(s.zh)]);
}

/// 合成 Zotero 夹具导入（search 先 ATTACH，导入时的备份不受影响）后 `update_run`：子项内容并进上级、回收站、独立附件 / 笔记。
#[test]
fn zotero_fixture_import() {
    use crate::db;
    use crate::import::zotero;
    use crate::paths::DataPaths;
    let dir = temp_dir("search-zotero");
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    attach(&c, &paths.root.join("search.db")).unwrap();
    let fx = zotero_import::fixture::build(&dir.join("fx")).unwrap();
    let run = zotero::run(&c, &paths, &fx.zotero_dir, &zotero::Options { base_dir: Some(fx.base_dir.clone()) }).unwrap();
    update_run(&c, run.summary.id).unwrap();
    assert!(!ensure_fresh(&c).unwrap(), "update_run 之后与库一致");
    assert_eq!(doc_count(&c), 24, "21 条文献（含回收站）+ 独立附件 ATT00006 + 独立笔记 NOTE0002、NOTE0004");
    let id_of = |table: &str, key: &str| -> i64 {
        c.query_row(&format!("SELECT id FROM {table} WHERE source_key = ?1"), [key], |r| r.get(0)).unwrap()
    };
    let item = |key: &str| DocRef::item(id_of("items", key));

    assert_eq!(hits(&c, "内部效能感"), [item("JRNL0001")], "子笔记");
    assert_eq!(hits(&c, "核心概念"), [item("JRNL0001")], "标注评论");
    assert!(hits(&c, "internal and external").contains(&item("JRNL0001")), "摘要、标注原文");
    assert_eq!(hits(&c, "muller"), [item("JRNL0003")]);
    assert_eq!(hits(&c, "MÜLLER-lüd"), [item("JRNL0003")]);
    assert_eq!(hits(&c, "10.0000/synthetic.2016"), [item("JRNL0003")]);
    assert_eq!(hits(&c, "国富论"), [item("BOOK0001")], "子附件标题");
    assert_eq!(hits(&c, "旧版附件笔记"), [item("BOOK0001")], "旧版附件笔记");
    assert_eq!(hits(&c, "合成出版社"), [item("BOOK0001")]);
    assert_eq!(hits(&c, "978-0-00-000000-1"), [item("BOOK0001")], "ISBN 原文");
    assert_eq!(hits(&c, "9780000000001"), [item("BOOK0001")], "ISBN 规范化后");
    let mut smith = vec![item("BOOK0001"), item("BSEC0002")];
    smith.sort();
    assert_eq!(hits(&c, "亚当·斯密"), smith);
    assert_eq!(hits(&c, "standalone"), [DocRef::attachment(id_of("attachments", "ATT00006"))], "独立附件与它的标注");
    assert_eq!(hits(&c, "独立笔记"), [DocRef::note(id_of("notes", "NOTE0002"))]);
    assert_eq!(hits(&c, "直接删除的笔记"), [DocRef::note(id_of("notes", "NOTE0004"))], "回收站里的独立笔记");
    assert_eq!(hits(&c, "trashed article"), [item("JRNL0005")], "回收站里的文献");
    assert_eq!(hits(&c, "父条目已删除"), [item("JRNL0005")], "随上级进回收站的子笔记仍并进上级");
    assert!(hits(&c, "old-version").is_empty(), "直接删除的子附件 ATT00013 不并进上级");
    assert_eq!(hits(&c, "🔥"), [item("JRNL0004")], "emoji 标签按原文找");
    assert_eq!(hits(&c, "政治效能感").len(), 3, "JRNL0001（标签）、JRNL0002、学位论文");
}

// ---------- 基准与实测（默认忽略） ----------

/// 5000 条合成文献（中英混合标题、摘要、3 个作者、3 个标签、字段、子笔记、子附件、标注）的重建耗时与查询耗时。
/// `cargo test -p pith bench_rebuild -- --ignored --nocapture`（调试构建；`--release` 更快）
#[test]
#[ignore]
fn bench_rebuild_5000() {
    use std::time::Instant;
    let dir = temp_dir("search-bench");
    let c = memory_db();
    attach(&c, &dir.join("search.db")).unwrap();
    let zh = ["政治", "效能感", "选举", "参与", "互联网", "社会", "信任", "制度", "治理", "公民", "媒体", "青年", "调查", "数据", "分析", "影响"];
    let en = [
        "political", "efficacy", "voting", "turnout", "survey", "media", "trust", "institution", "governance", "citizen", "youth", "panel", "analysis",
        "internet", "participation", "democracy",
    ];
    let mut rng = Rng(42);
    let words = |rng: &mut Rng, n: usize, cjk: bool| -> String {
        (0..n)
            .map(|_| {
                let r = rng.next() as usize;
                if cjk && !r.is_multiple_of(3) { zh[r / 3 % zh.len()].to_string() } else { format!("{} ", en[r / 3 % en.len()]) }
            })
            .collect()
    };
    let t = Instant::now();
    let tx = Tx::begin(&c).unwrap();
    for i in 0..5000 {
        let cjk = i % 3 == 0;
        let id = item(&tx, &words(&mut rng, 8, cjk), None);
        let (abs, journal) = (words(&mut rng, 80, cjk), words(&mut rng, 3, cjk));
        tx.execute("UPDATE items SET abstract = ?2, year = ?3, container_title = ?4 WHERE id = ?1", params![id, abs, 1990 + i % 35, journal]).unwrap();
        for p in 0..3 {
            let family = format!("Author{}", rng.next() % 2000);
            tx.execute("INSERT INTO creators (item_id, role, family, given, position) VALUES (?1, 'author', ?2, 'A.', ?3)", params![id, family, p]).unwrap();
        }
        for _ in 0..3 {
            tag(&tx, "item_tags", "item_id", id, &format!("tag{}", rng.next() % 100));
        }
        for f in ["publisher", "place", "volume", "issue", "pages", "url", "ISSN", "extra"] {
            tx.execute("INSERT INTO item_fields (item_id, field, value) VALUES (?1, ?2, ?3)", params![id, f, words(&mut rng, 2, cjk)]).unwrap();
        }
        note(&tx, Some(id), &format!("<p>{}</p>", words(&mut rng, 60, cjk)), None);
        let a = attachment(&tx, Some(id), "stored", Some("Full Text PDF"), &format!("paper-{i}.pdf"), None);
        annotation(&tx, a, Some(&words(&mut rng, 12, cjk)), Some(&words(&mut rng, 4, cjk)));
        annotation(&tx, a, Some(&words(&mut rng, 12, cjk)), None);
    }
    tx.commit().unwrap();
    println!("合成数据写入 {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let n = rebuild(&c).unwrap();
    println!("rebuild：{n} 个文档，{:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let pages: i64 = c.query_row("PRAGMA search.page_count", [], |r| r.get(0)).unwrap();
    let size: i64 = c.query_row("PRAGMA search.page_size", [], |r| r.get(0)).unwrap();
    println!("索引大小 {:.1} MB", (pages * size) as f64 / 1e6);
    let t = Instant::now();
    assert!(!ensure_fresh(&c).unwrap());
    println!("ensure_fresh（一致时）{:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    for q in ["政", "效能", "政治效能感", "effic", "political trust", "政治 media", "tag7", "author1"] {
        let t = Instant::now();
        let n = query(&c, q).unwrap().unwrap().len();
        println!("查询 {q:?}：{n} 条，{:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    update(&c, &[DocRef::item(1), DocRef::item(2500)]).unwrap();
    println!("update 2 条 {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
}

/// 真实 Zotero 副本：导入到临时目录，分别建方案 1（unicode61 + 二元组）与方案 2（trigram + 短词 LIKE）索引，
/// 用从库里自动取的查询词比较召回与耗时。默认忽略；**只打印聚合数字**，不打印查询词、标题、作者：
/// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy cargo test -p pith real_copy_tokenizers -- --ignored --nocapture`
#[test]
#[ignore]
fn real_copy_tokenizers() {
    use crate::db;
    use crate::import::zotero;
    use crate::paths::DataPaths;
    use std::time::Instant;
    let src = PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"));
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!src.starts_with(Path::new(&home).join("Zotero")), "不能直接读 ~/Zotero，请用副本");
    let dir = temp_dir("search-real");
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = db::open(&paths.library_db).unwrap();
    db::migrate(&mut c, db::MIGRATIONS, &paths.backups).unwrap();
    zotero::run(&c, &paths, &src, &zotero::Options::default()).unwrap();

    // 方案 1
    attach(&c, &dir.join("search.db")).unwrap();
    let t = Instant::now();
    let n = rebuild(&c).unwrap();
    let build1 = t.elapsed();
    // 方案 2：trigram，原文（全角转半角）入库
    c.execute("ATTACH DATABASE ?1 AS tri", [dir.join("tri.db").to_str().unwrap()]).unwrap();
    let t = Instant::now();
    c.execute_batch(
        "PRAGMA tri.journal_mode = WAL;
         CREATE VIRTUAL TABLE tri.docs USING fts5(title, creators, tags, abstract, notes, other, tokenize = 'trigram remove_diacritics 1');",
    )
    .unwrap();
    let fold = |s: &str| -> String { s.chars().map(halfwidth).collect() };
    let mut corpus: Vec<(DocRef, [String; 6])> = Vec::new();
    for kind in KINDS {
        for (id, d) in load(&c, kind, &Scope::All).unwrap() {
            corpus.push((DocRef { kind, id }, d.texts().map(|s| fold(&s))));
        }
    }
    for (r, t) in &corpus {
        c.execute(
            "INSERT INTO tri.docs (rowid, title, creators, tags, abstract, notes, other) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![rowid(*r).unwrap(), t[0], t[1], t[2], t[3], t[4], t[5]],
        )
        .unwrap();
    }
    c.execute("INSERT INTO tri.docs (docs) VALUES ('optimize')", []).unwrap();
    let build2 = t.elapsed();
    let size = |schema: &str| -> f64 {
        let p: i64 = c.query_row(&format!("PRAGMA {schema}.page_count"), [], |r| r.get(0)).unwrap();
        let s: i64 = c.query_row(&format!("PRAGMA {schema}.page_size"), [], |r| r.get(0)).unwrap();
        (p * s) as f64 / 1024.0
    };
    let by_kind = |k: DocKind| corpus.iter().filter(|(r, _)| r.kind == k).count();
    let cjk_docs = corpus.iter().filter(|(_, t)| t.iter().any(|s| s.chars().any(is_cjk))).count();
    println!(
        "文档 {n}：文献 {}、独立附件 {}、独立笔记 {}；含中日韩文字的 {cjk_docs}",
        by_kind(DocKind::Item),
        by_kind(DocKind::Attachment),
        by_kind(DocKind::Note)
    );
    println!(
        "建索引：方案1 {:.0} ms / {:.0} KB；方案2 {:.0} ms / {:.0} KB（调试构建）",
        build1.as_secs_f64() * 1e3,
        size("search"),
        build2.as_secs_f64() * 1e3,
        size("tri")
    );

    // 真值：子串（每个词都是全文小写的子串）；西文另算「词首」真值（每个词都是某个词的开头）
    let hay: Vec<(DocRef, String, Vec<String>)> = corpus
        .iter()
        .map(|(r, t)| {
            let h = t.join("\n").to_lowercase();
            let toks = h.split(|ch: char| !ch.is_alphanumeric()).filter(|s| !s.is_empty()).map(str::to_string).collect();
            (*r, h, toks)
        })
        .collect();
    let truth_sub = |q: &str| -> HashSet<DocRef> {
        let ws: Vec<String> = q.split_whitespace().map(|w| fold(w).to_lowercase()).collect();
        hay.iter().filter(|(_, h, _)| ws.iter().all(|w| h.contains(w.as_str()))).map(|(r, _, _)| *r).collect()
    };
    let truth_prefix = |q: &str| -> HashSet<DocRef> {
        let ws: Vec<String> = q.split_whitespace().map(|w| fold(w).to_lowercase()).collect();
        hay.iter().filter(|(_, _, toks)| ws.iter().all(|w| toks.iter().any(|t| t.starts_with(w.as_str())))).map(|(r, _, _)| *r).collect()
    };
    let tri = |q: &str| -> HashSet<DocRef> {
        let mut fts = Vec::new();
        let mut likes = Vec::new();
        for w in q.split_whitespace() {
            let w = fold(w);
            if w.chars().count() >= 3 {
                fts.push(quote(&w));
            } else {
                likes.push(format!("%{}%", w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
            }
        }
        let mut args: Vec<Value> = Vec::new();
        let mut sql = "SELECT rowid FROM tri.docs WHERE 1".to_string();
        if !fts.is_empty() {
            args.push(Value::Text(fts.join(" ")));
            sql.push_str(" AND docs MATCH ?1");
        }
        for l in likes {
            args.push(Value::Text(l));
            let n = args.len();
            sql.push_str(&format!(
                " AND (title LIKE ?{n} ESCAPE '\\' OR creators LIKE ?{n} ESCAPE '\\' OR tags LIKE ?{n} ESCAPE '\\' OR abstract LIKE ?{n} ESCAPE '\\'
                       OR notes LIKE ?{n} ESCAPE '\\' OR other LIKE ?{n} ESCAPE '\\')"
            ));
        }
        let mut out = HashSet::new();
        each(&c, &sql, &args, |r| {
            out.extend(from_rowid(r.get(0)?));
            Ok(())
        })
        .unwrap();
        out
    };

    // 查询词：从库里自动取（高频的一半 + 其余里均匀取一半），不写进代码、不打印
    let pick = |freq: HashMap<String, usize>, n: usize| -> Vec<String> {
        let mut v: Vec<(String, usize)> = freq.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out: Vec<String> = v.iter().take(n / 2).map(|x| x.0.clone()).collect();
        let rest = &v[out.len()..];
        let k = n - out.len();
        if !rest.is_empty() {
            for i in 0..k.min(rest.len()) {
                out.push(rest[i * rest.len() / k.min(rest.len())].0.clone());
            }
        }
        out
    };
    let runs = |s: &str| -> Vec<Vec<char>> {
        let mut out = vec![];
        let mut cur = vec![];
        for ch in s.chars() {
            if is_cjk(ch) {
                cur.push(ch);
            } else if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    };
    let ngrams = |col: usize, n: usize| -> HashMap<String, usize> {
        let mut f = HashMap::new();
        for (_, t) in &corpus {
            for r in runs(&t[col]) {
                for w in r.windows(n) {
                    *f.entry(w.iter().collect::<String>()).or_insert(0) += 1;
                }
            }
        }
        f
    };
    const STOP: &[&str] = &["the", "and", "for", "with", "from", "that", "this", "into", "between", "among", "their", "über", "what", "when", "does"];
    let ascii_words = |s: &str| -> Vec<String> {
        s.split(|ch: char| !ch.is_ascii_alphabetic())
            .filter(|w| w.len() >= 4)
            .map(|w| w.to_ascii_lowercase())
            .filter(|w| !STOP.contains(&w.as_str()))
            .collect()
    };
    let words_of = |col: usize, min: usize, f: &dyn Fn(&str) -> Option<String>| -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for (_, t) in &corpus {
            for w in ascii_words(&t[col]).into_iter().filter(|w| w.len() >= min) {
                if let Some(x) = f(&w) {
                    *m.entry(x).or_insert(0) += 1;
                }
            }
        }
        m
    };
    let column = |sql: &str| -> Vec<String> {
        let mut st = c.prepare(sql).unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    let count = |v: Vec<String>| -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for x in v {
            *m.entry(x).or_insert(0) += 1;
        }
        m
    };

    let mut cats: Vec<(&str, Vec<String>, bool)> = Vec::new();
    cats.push(("中文 1 字", pick(ngrams(TITLE, 1), 20), false));
    cats.push(("中文 2 字", pick(ngrams(TITLE, 2), 20), false));
    let mut many = pick(ngrams(TITLE, 3), 10);
    many.extend(pick(ngrams(TITLE, 4), 10));
    cats.push(("中文 3-4 字", many, false));
    cats.push(("英文整词", pick(words_of(TITLE, 4, &|w| Some(w.to_string())), 20), true));
    cats.push(("英文前缀(5)", pick(words_of(TITLE, 7, &|w| Some(w[..5].to_string())), 20), true));
    cats.push(("英文词中子串", pick(words_of(TITLE, 8, &|w| Some(w[2..6].to_string())), 20), true));
    let fam = column("SELECT family FROM creators WHERE family IS NOT NULL AND length(family) >= 3")
        .into_iter()
        .filter(|f| f.chars().all(|ch| ch.is_ascii_alphabetic()))
        .collect();
    cats.push(("作者（西文姓）", pick(count(fam), 20), true));
    let lit = column("SELECT literal FROM creators WHERE literal IS NOT NULL").into_iter().filter(|l| l.chars().any(is_cjk)).collect();
    cats.push(("作者（中文全名）", pick(count(lit), 20), false));
    cats.push((
        "标签",
        pick(
            count(column(
                "SELECT name FROM tags WHERE id IN (SELECT tag_id FROM item_tags UNION SELECT tag_id FROM attachment_tags UNION SELECT tag_id FROM note_tags)",
            )),
            20,
        ),
        false,
    ));
    let mut nw = pick(ngrams(NOTES, 2), 10);
    nw.extend(pick(words_of(NOTES, 5, &|w| Some(w.to_string())), 10));
    cats.push(("笔记里的词", nw, false));
    let mut doi = Vec::new();
    for d in column("SELECT doi FROM items WHERE doi IS NOT NULL") {
        if let Some((pre, suf)) = d.split_once('/') {
            doi.push(pre.to_string());
            if suf.chars().count() >= 6 {
                doi.push(suf.chars().take(6).collect());
            }
        }
    }
    cats.push(("DOI 片段", pick(count(doi), 20), false));
    let mut mixed = HashMap::new();
    let mut multi = HashMap::new();
    for (_, t) in &corpus {
        let all = t.join("\n");
        let zh2 = runs(&all).into_iter().find(|r| r.len() >= 2).map(|r| r[..2].iter().collect::<String>());
        let en = ascii_words(&all).into_iter().find(|w| w.len() >= 5);
        if let (Some(a), Some(b)) = (zh2, en) {
            mixed.insert(format!("{a} {b}"), 1);
        }
        let tw = ascii_words(&t[TITLE]);
        if tw.len() >= 2 {
            multi.insert(format!("{} {}", tw[0], tw[tw.len() - 1]), 1);
        }
    }
    cats.push(("中英混合（同一文档）", pick(mixed, 20), false));
    cats.push(("英文两词 AND", pick(multi, 20), true));

    println!(
        "| 类别 | 查询 | 真值(子串) | 方案1 命中 | 方案1 漏 | 方案1 多 | 方案2 命中 | 方案2 漏 | 方案2 多 | 仅方案1 | 仅方案2 | 方案1 零命中 | 方案2 零命中 | 词首真值 | 方案1 漏(词首) | 方案1 ms 平均/最大 | 方案2 ms 平均/最大 |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let ms = |f: &dyn Fn() -> HashSet<DocRef>| -> (HashSet<DocRef>, f64) {
        let t = Instant::now();
        let mut r = HashSet::new();
        for _ in 0..3 {
            r = f();
        }
        (r, t.elapsed().as_secs_f64() * 1e3 / 3.0)
    };
    for (name, qs, latin) in &cats {
        // 查询数、真值、方案1 命中 / 漏 / 多、方案2 命中 / 漏 / 多、仅方案1、仅方案2、方案1 零命中、方案2 零命中、词首真值、方案1 漏（词首）
        let mut s = [0usize; 14];
        let (mut t1, mut m1, mut t2, mut m2) = (0.0, 0.0f64, 0.0, 0.0f64);
        for q in qs {
            let (r1, d1) = ms(&|| query(&c, q).unwrap().unwrap().into_keys().collect());
            let (r2, d2) = ms(&|| tri(q));
            let truth = truth_sub(q);
            let p = if *latin { truth_prefix(q) } else { HashSet::new() };
            for (i, v) in [
                1,
                truth.len(),
                r1.len(),
                truth.difference(&r1).count(),
                r1.difference(&truth).count(),
                r2.len(),
                truth.difference(&r2).count(),
                r2.difference(&truth).count(),
                r1.difference(&r2).count(),
                r2.difference(&r1).count(),
                r1.is_empty() as usize,
                r2.is_empty() as usize,
                p.len(),
                p.difference(&r1).count(),
            ]
            .into_iter()
            .enumerate()
            {
                s[i] += v;
            }
            t1 += d1;
            m1 = m1.max(d1);
            t2 += d2;
            m2 = m2.max(d2);
        }
        let nq = s[0].max(1) as f64;
        let prefix = if *latin { format!("{} | {}", s[12], s[13]) } else { "- | -".into() };
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {prefix} | {:.2} / {:.2} | {:.2} / {:.2} |",
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], s[9], s[10], s[11], t1 / nq, m1, t2 / nq, m2
        );
    }
}
