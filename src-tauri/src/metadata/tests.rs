//! 联网补全的测试：不联网，用录好的（合成的）响应。DOI 用 Crossref 的测试前缀 10.5555，ISBN 用 978-0-00 开头的假号。
use super::net::{self, Http, NetError, Pacer, Response};
use super::*;
use crate::db::tests::{memory_db, temp_dir};
use serde_json::json;
use std::collections::HashMap;

/// 假的 HTTP：按地址回放响应（同一地址可排几次，用于测重试），并记下每次请求
#[derive(Default)]
struct Fake {
    routes: Mutex<HashMap<String, VecDeque<Result<Response, NetError>>>>,
    calls: Mutex<Vec<(String, bool)>>,
}

impl Fake {
    fn on(self, url: &str, status: u16, body: &str, final_url: &str) -> Self {
        self.routes.lock().unwrap().entry(url.into()).or_default().push_back(Ok(Response { status, body: body.into(), url: final_url.into(), retry_after: Some(1) }));
        self
    }
    fn fail(self, url: &str, e: NetError) -> Self {
        self.routes.lock().unwrap().entry(url.into()).or_default().push_back(Err(e));
        self
    }
    fn calls(&self) -> Vec<(String, bool)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Http for Fake {
    fn get(&self, url: &str, _accept: &str, contact: bool) -> Result<Response, NetError> {
        self.calls.lock().unwrap().push((url.into(), contact));
        let mut routes = self.routes.lock().unwrap();
        let q = routes.get_mut(url).unwrap_or_else(|| panic!("没有录这个地址：{url}"));
        if q.len() > 1 { q.pop_front().unwrap() } else { q.front().cloned().unwrap() }
    }
}

impl Clone for Response {
    fn clone(&self) -> Self {
        Response { status: self.status, body: self.body.clone(), url: self.url.clone(), retry_after: self.retry_after }
    }
}

fn pacer() -> Pacer {
    Pacer::immediate()
}

const DOI: &str = "10.5555/RL.Test.1";
const CROSSREF: &str = "https://api.crossref.org/v1/works/10.5555%2Frl.test.1/transform";

/// 合成的 Crossref CSL JSON（结构仿真实返回：多出来的键、ISSN 数组、副标题数组、JATS 摘要、带 <i> 的标题）
fn csl() -> String {
    json!({
        "indexed": {"date-parts": [[2026, 11, 8]]},
        "reference-count": 12,
        "publisher": "Synthetic Society Press",
        "issue": "2",
        "type": "journal-article",
        "title": "Political <i>efficacy</i> revisited",
        "subtitle": ["A synthetic test record"],
        "container-title": "Journal of Synthetic Studies",
        "container-title-short": "J. Synth. Stud.",
        "author": [
            {"given": "Jane", "family": "Doe", "sequence": "first", "affiliation": []},
            {"given": "Richard", "family": "Roe", "sequence": "additional", "affiliation": []}
        ],
        "issued": {"date-parts": [[2021, 3, 15]]},
        "published-online": {"date-parts": [[2021, 3, 15]]},
        "published-print": {"date-parts": [[2022, 1]]},
        "volume": "36",
        "page": "45–70",
        "ISSN": ["0000-0001", "0000-0002"],
        "language": "en",
        "abstract": "<jats:title>Abstract</jats:title><jats:p>First paragraph of a <jats:italic>synthetic</jats:italic> abstract.</jats:p><jats:p>Second paragraph.</jats:p>",
        "DOI": "10.5555/rl.test.1",
        "URL": "http://dx.doi.org/10.5555/rl.test.1",
        "license": [{"URL": "https://example.org/license"}]
    })
    .to_string()
}

fn doi_fake() -> Fake {
    Fake::default().on(&net::doi_url(DOI), 200, &csl(), CROSSREF)
}

const ISBN: &str = "9780000000002";
fn ol_url(isbn: &str) -> String {
    format!("https://openlibrary.org/api/books?bibkeys=ISBN:{isbn}&format=json&jscmd=data")
}
fn ol_body() -> String {
    json!({
        format!("ISBN:{ISBN}"): {
            "title": "An Example Book on Survey Methods",
            "subtitle": "Second Edition",
            "authors": [{"name": "Jane Doe"}, {"name": "Roe, Richard"}, {"name": "王芳"}],
            "publishers": [{"name": "Synthetic Press"}],
            "publish_places": [{"name": "London"}, {"name": "New York"}],
            "publish_date": "March 2015",
            "number_of_pages": 320
        }
    })
    .to_string()
}

fn found_of(f: Result<Fetched, NetError>) -> Found {
    match f {
        Ok(Fetched::Found { found, .. }) => found,
        _ => panic!("应该查到"),
    }
}

// ---------------- 解析与联网层 ----------------

#[test]
fn identifiers_from_pasted_text() {
    let (ids, bad) = parse_identifiers("https://doi.org/10.1038/nature14539, doi:10.5555/ABC.1；ISBN 978-0-00-000000-2\n0-306-40615-2 9780000000002 10.1038/NATURE14539 2021 hello 9780000000003 10.5");
    assert_eq!(
        ids,
        vec![
            Identifier::Doi("10.1038/nature14539".into()),
            Identifier::Doi("10.5555/ABC.1".into()),
            Identifier::Isbn("9780000000002".into()),
            Identifier::Isbn("9780306406157".into()),
        ],
        "大小写不同的同一 DOI、同一 ISBN 去重；ISBN-10 转 13 位"
    );
    assert_eq!(bad, vec!["2021", "hello", "9780000000003", "10.5"], "年份、普通词、校验位错的 ISBN、残缺的 DOI 认不出");
    assert_eq!(Identifier::Doi("10.1/AB".into()).key(), "doi:10.1/ab");
}

#[test]
fn doi_goes_into_the_path_safely() {
    assert_eq!(net::doi_url("10.1002/(SICI)1097-0258<2>3.0.CO;2-#"), "https://doi.org/10.1002/%28SICI%291097-0258%3C2%3E3.0.CO%3B2-%23");
    assert_eq!(net::doi_url("10.5555/a b?c"), "https://doi.org/10.5555/a%20b%3Fc");
}

#[test]
fn crossref_record_maps_to_zotero_fields() {
    let fake = doi_fake();
    let f = found_of(fetch(&fake, &mut pacer(), &[Identifier::Doi(DOI.into())]));
    assert_eq!((f.source.as_str(), f.item_type.as_str()), ("crossref", "journalArticle"));
    let get = |k: &str| f.fields.get(k).map(String::as_str);
    assert_eq!(get("title"), Some("Political efficacy revisited: A synthetic test record"), "去掉 <i>，接上副标题");
    assert_eq!(get("publicationTitle"), Some("Journal of Synthetic Studies"));
    assert_eq!(get("journalAbbreviation"), Some("J. Synth. Stud."));
    assert_eq!((get("volume"), get("issue"), get("pages")), (Some("36"), Some("2"), Some("45–70")));
    assert_eq!(get("date"), Some("2021-03-15"));
    assert_eq!(get("ISSN"), Some("0000-0001, 0000-0002"));
    assert_eq!(get("publisher"), Some("Synthetic Society Press"));
    assert_eq!(get("abstractNote"), Some("First paragraph of a synthetic abstract.\nSecond paragraph."), "JATS 去标签、去掉「Abstract」小标题、保留段落");
    assert_eq!(get("url"), None, "不要 doi.org 的链接");
    assert!(f.fields.keys().all(|k| !k.starts_with("csl:")), "引用数、许可之类的不要");
    assert_eq!(creators_text(&f.creators), "Doe, Jane; Roe, Richard");
    // 联系邮箱只给 doi.org
    assert_eq!(fake.calls(), vec![(net::doi_url(DOI), true)]);
}

#[test]
fn doi_not_found_and_odd_answers() {
    let missing = "10.5555/missing";
    let html = "10.5555/html";
    let fake = Fake::default()
        .on(&net::doi_url(missing), 404, "DOI Not Found", &net::doi_url(missing))
        .on(&net::doi_url(html), 200, "<html>landing page</html>", "https://www.example.org/landing")
        .on(&net::doi_url("10.5555/busy"), 429, "slow down", "https://doi.org/10.5555/busy")
        .on(&net::doi_url("10.5555/busy"), 200, &csl(), CROSSREF)
        .on(&net::doi_url("10.5555/down"), 500, "oops", "https://doi.org/10.5555/down");
    let p = &mut pacer();
    match fetch(&fake, p, &[Identifier::Doi(missing.into())]) {
        Ok(Fetched::NotFound { message, .. }) => assert!(message.contains("没有这个 DOI"), "{message}"),
        _ => panic!(),
    }
    match fetch(&fake, p, &[Identifier::Doi(html.into())]) {
        Ok(Fetched::NotFound { message, .. }) => assert!(message.contains("www.example.org"), "{message}"),
        _ => panic!(),
    }
    found_of(fetch(&fake, p, &[Identifier::Doi("10.5555/busy".into())]));
    assert_eq!(fake.calls().iter().filter(|c| c.0.contains("busy")).count(), 2, "429 后再试一次");
    assert!(matches!(fetch(&fake, p, &[Identifier::Doi("10.5555/down".into())]), Err(NetError::Other(m)) if m.contains("500")));
    assert!(fetch(&fake, p, &[]).is_err());
}

#[test]
fn open_library_book_and_fallback_from_doi() {
    let fake = Fake::default()
        .on(&ol_url(ISBN), 200, &ol_body(), &ol_url(ISBN))
        .on(&ol_url("9781111111113"), 200, "{}", &ol_url("9781111111113"))
        .on(&net::doi_url("10.5555/nobook"), 404, "", &net::doi_url("10.5555/nobook"));
    let p = &mut pacer();
    // DOI 没有 → 接着用 ISBN 查
    let f = found_of(fetch(&fake, p, &[Identifier::Doi("10.5555/nobook".into()), Identifier::Isbn(ISBN.into())]));
    assert_eq!((f.source.as_str(), f.item_type.as_str()), ("openlibrary", "book"));
    assert_eq!(f.fields.get("title").unwrap(), "An Example Book on Survey Methods: Second Edition");
    assert_eq!(
        [&f.fields["publisher"], &f.fields["place"], &f.fields["date"], &f.fields["numPages"]],
        ["Synthetic Press", "London", "March 2015", "320"]
    );
    assert_eq!(creators_text(&f.creators), "Doe, Jane; Roe, Richard; 王芳", "「名 姓」「姓, 名」拆开，中文名单栏");
    assert!(fake.calls().iter().all(|(u, contact)| u.contains("doi.org") == *contact), "Open Library 不带邮箱");
    match fetch(&fake, p, &[Identifier::Isbn("9781111111113".into())]) {
        Ok(Fetched::NotFound { message, .. }) => assert!(message.contains("Open Library")),
        _ => panic!(),
    }
}

// ---------------- 比较 ----------------

fn cur(item_type: &str, fields: &[(&str, &str)], creators: &[(&str, &str)]) -> Current {
    Current {
        item_type: item_type.into(),
        fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        creators: creators
            .iter()
            .map(|(f, g)| FoundCreator { role: "author".into(), family: Some(f.to_string()), given: Some(g.to_string()), literal: None })
            .collect(),
    }
}

fn found() -> Found {
    found_of(fetch(&doi_fake(), &mut pacer(), &[Identifier::Doi(DOI.into())]))
}

fn kinds(plan: &[Change]) -> Vec<(String, String)> {
    plan.iter().map(|c| (c.field.clone(), c.kind.clone())).collect()
}

#[test]
fn plan_fills_empty_and_flags_differences() {
    let f = found();
    // 什么都没有的期刊文章：全部补上
    let p = plan(&cur("journalArticle", &[("DOI", DOI)], &[]), &f);
    assert!(p.iter().all(|c| c.kind == "fill" && c.state == "applied" && c.current.is_none()));
    let fields: Vec<&str> = p.iter().map(|c| c.field.as_str()).collect();
    for k in ["title", "date", "publicationTitle", "volume", "issue", "pages", "ISSN", "abstractNote", "language", "journalAbbreviation", "creators"] {
        assert!(fields.contains(&k), "缺 {k}：{fields:?}");
    }
    assert!(!fields.contains(&"DOI"), "DOI 本身不动");
    assert!(!fields.contains(&"publisher"), "Zotero 的期刊文章没有「出版者」字段，不补");

    // 已有的：写法不同但其实一样的不提示；真不同的待确认
    let have = cur(
        "journalArticle",
        &[
            ("title", "Political efficacy revisited: a synthetic test record"),
            ("date", "2021-03-16 2021-03-16"),
            ("publicationTitle", "JOURNAL OF SYNTHETIC STUDIES"),
            ("volume", "036"),
            ("issue", "3"),
            ("pages", "45-70"),
            ("abstractNote", "我自己写的摘要"),
            ("language", "English"),
        ],
        &[("doe", "J."), ("Roe", "R.")],
    );
    let p = plan(&have, &f);
    assert_eq!(
        kinds(&p),
        vec![("issue".into(), "conflict".into()), ("ISSN".into(), "fill".into()), ("journalAbbreviation".into(), "fill".into())],
        "标题大小写、日期差一天、期刊大写、卷号前导零、页码连字符都算一样；摘要、语言已有就不动"
    );
    let issue = p.iter().find(|c| c.field == "issue").unwrap();
    assert_eq!((issue.current.as_deref(), issue.found.as_str(), issue.state.as_str()), (Some("3"), "2", "pending"));

    // 印刷版年份（2022）也算一样
    let p = plan(&cur("journalArticle", &[("date", "2022-00-00 2022")], &[]), &f);
    assert!(!p.iter().any(|c| c.field == "date"), "{:?}", kinds(&p));
    assert_eq!(f.date_alternatives, vec!["2022-01", "2021-03-15"]);
    // 年份不同、作者不同
    let p = plan(&cur("journalArticle", &[("date", "2019-00-00 2019"), ("title", "Another title")], &[("Smith", "A.")]), &f);
    let k = kinds(&p);
    for want in [("date", "conflict"), ("title", "conflict"), ("creators", "conflict")] {
        assert!(k.contains(&(want.0.to_string(), want.1.to_string())), "{k:?}");
    }
    let cr = p.iter().find(|c| c.field == "creators").unwrap();
    assert_eq!((cr.current.as_deref(), cr.found.as_str()), (Some("Smith, A."), "Doe, Jane; Roe, Richard"));
}

#[test]
fn plan_respects_item_type() {
    let f = found();
    // 图书：出版者照补（Open Library 的记录类型就是图书）；期刊字段不补
    assert!(allowed("book", "publisher") && !allowed("journalArticle", "publisher") && !allowed("book", "issue") && allowed("thesis", "university"));
    // 章节：期刊名落到 bookTitle；卷期页等期刊字段不往别的类型上补，只补通用的
    let p = plan(&cur("bookSection", &[], &[]), &f);
    let fields: Vec<&str> = p.iter().map(|c| c.field.as_str()).collect();
    assert!(fields.contains(&"bookTitle") && fields.contains(&"title") && fields.contains(&"date") && fields.contains(&"abstractNote"));
    assert!(!fields.contains(&"volume") && !fields.contains(&"ISSN") && !fields.contains(&"publicationTitle"), "{fields:?}");
    // 图书没有「期刊 / 书名」：不补
    let p = plan(&cur("book", &[], &[]), &f);
    assert!(!p.iter().any(|c| c.field.ends_with("Title") && c.field != "title"));
    // 笼统的「文档」：建议改类型（待确认）
    let p = plan(&cur("document", &[("title", "x")], &[]), &f);
    let t = p.iter().find(|c| c.field == "itemType").unwrap();
    assert_eq!((t.found.as_str(), t.kind.as_str()), ("journalArticle", "conflict"));
    // 案例的标题字段是 caseName
    let p = plan(&cur("case", &[("caseName", "Old v. New")], &[]), &f);
    assert!(p.iter().any(|c| c.field == "caseName" && c.kind == "conflict"));
}

#[test]
fn same_values() {
    assert!(same("pages", "45 – 70", "45-70"));
    assert!(!same("pages", "45", "45-70"));
    assert!(same("date", "March 2021", "2021-03-15"));
    assert!(!same("date", "2020", "2021"));
    assert!(!same("date", "n.d.", "unknown"), "没有年份的只有原文一样才算一样");
    assert!(same("title", "Deep Learning!", "deep learning"));
}

// ---------------- 写库：自动补、撤销、待确认 ----------------

fn setup() -> Connection {
    let c = memory_db();
    c.execute_batch(&format!(
        "INSERT INTO items (id, uid, item_type, source, created_at, updated_at) VALUES
           (1, 'I1', 'journalArticle', 'zotero', 't', 't'),
           (2, 'I2', 'book', 'manual', 't', 't'),
           (3, 'I3', 'journalArticle', 'manual', 't', 't'),
           (4, 'I4', 'journalArticle', 'manual', 't', 't');
         INSERT INTO item_fields (item_id, field, value) VALUES
           (1, 'DOI', '{DOI}'), (1, 'issue', '3'), (1, 'title', 'Political efficacy revisited'),
           (2, 'ISBN', '978-0-00-000000-2'),
           (3, 'title', '没有标识的文献');
         INSERT INTO creators (item_id, role, family, given, position) VALUES (1, 'author', 'Doe', 'J.', 0);"
    ))
    .unwrap();
    for id in 1..=4 {
        crate::fields::refresh_columns(&c, id, "t").unwrap();
    }
    crate::search::rebuild(&c).unwrap();
    c
}

fn value(c: &Connection, item: i64, field: &str) -> Option<String> {
    c.query_row("SELECT value FROM item_fields WHERE item_id = ?1 AND field = ?2", params![item, field], |r| r.get(0)).optional().unwrap()
}

#[test]
fn record_fills_then_undo_and_resolve() {
    let c = setup();
    assert_eq!(identifiers_of(&c, 1).unwrap(), vec![Identifier::Doi(DOI.into())]);
    assert_eq!(identifiers_of(&c, 2).unwrap(), vec![Identifier::Isbn(ISBN.into())]);
    assert!(identifiers_of(&c, 3).unwrap().is_empty());

    let fetched = fetch(&doi_fake(), &mut pacer(), &identifiers_of(&c, 1).unwrap());
    let (res, refs) = record(&c, 1, "import", fetched.map_err(|e| e.message())).unwrap();
    assert_eq!(refs, vec![DocRef::item(1)]);
    assert!(res.found && res.filled >= 8 && res.conflicts == 3, "{res:?}");
    assert_eq!(value(&c, 1, "publicationTitle").as_deref(), Some("Journal of Synthetic Studies"));
    assert_eq!(value(&c, 1, "date").as_deref(), Some("2021-03-15 2021-03-15"), "日期写成 Zotero 两段式");
    assert_eq!(value(&c, 1, "issue").as_deref(), Some("3"), "已有的值不覆盖");
    let (container, year): (Option<String>, Option<i64>) = c.query_row("SELECT container_title, year FROM items WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((container.as_deref(), year), (Some("Journal of Synthetic Studies"), Some(2021)), "常用列跟着重算");

    let l = latest(&c, 1).unwrap().unwrap();
    assert_eq!((l.status.as_str(), l.source.as_deref(), l.trigger.as_str(), l.identifier.as_str()), ("found", Some("crossref"), "import", "doi:10.5555/rl.test.1"));
    let pending: Vec<&str> = l.changes.iter().filter(|c| c.state == "pending").map(|c| c.field.as_str()).collect();
    assert_eq!(pending, vec!["title", "creators", "issue"], "标题多了副标题、作者少一位、期号不同（按显示顺序）");
    let review: Vec<i64> = c.prepare(REVIEW_IDS).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(review, vec![1]);
    use crate::library::{ListQuery, Scope, items_list, overview};
    assert_eq!(overview(&c).unwrap().review, 1);
    let rows = items_list(&c, &ListQuery::of(Scope::Review)).unwrap();
    assert_eq!(rows.iter().map(|r| (r.kind.as_str(), r.id)).collect::<Vec<_>>(), vec![("item", 1)], "「待确认的补全」只列文献");

    // 待确认：采用期号、忽略标题
    let (v, refs) = resolve(&c, l.id, "issue", true).unwrap();
    assert_eq!((value(&c, 1, "issue").as_deref(), refs.len()), (Some("2"), 1));
    assert_eq!(v.changes.iter().find(|c| c.field == "issue").unwrap().state, "accepted");
    resolve(&c, l.id, "title", false).unwrap();
    resolve(&c, l.id, "creators", false).unwrap();
    assert_eq!(value(&c, 1, "title").as_deref(), Some("Political efficacy revisited"));
    assert!(resolve(&c, l.id, "title", true).unwrap_err().contains("处理过"));
    let review: Vec<i64> = c.prepare(REVIEW_IDS).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert!(review.is_empty(), "处理完就不在「待确认」里");

    // 撤销：自动填的清掉；用户之后改过的（卷号）保留
    edit::item_update(&c, 1, &ItemPatch { fields: BTreeMap::from([("volume".to_string(), "99".to_string())]), ..Default::default() }).unwrap();
    let (v, n, _) = undo(&c, l.id).unwrap();
    assert!(n >= 7, "{n}");
    assert_eq!(value(&c, 1, "publicationTitle"), None);
    assert_eq!(value(&c, 1, "volume").as_deref(), Some("99"));
    assert_eq!(v.changes.iter().find(|c| c.field == "volume").unwrap().state, "kept");
    assert_eq!(value(&c, 1, "issue").as_deref(), Some("2"), "采用的不算自动填，不撤销");
    assert!(undo(&c, l.id).is_ok(), "只剩保留的，再撤销也不报错");
}

#[test]
fn creators_filled_only_when_empty_and_undo_restores_empty() {
    let c = setup();
    c.execute("DELETE FROM creators", []).unwrap();
    let f = fetch(&doi_fake(), &mut pacer(), &identifiers_of(&c, 1).unwrap()).map_err(|e| e.message());
    record(&c, 1, "manual", f).unwrap();
    let n: i64 = c.query_row("SELECT COUNT(*) FROM creators WHERE item_id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    let l = latest(&c, 1).unwrap().unwrap();
    undo(&c, l.id).unwrap();
    let n: i64 = c.query_row("SELECT COUNT(*) FROM creators WHERE item_id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn not_found_and_failed_are_recorded_without_changes() {
    let c = setup();
    let fake = Fake::default().on(&ol_url(ISBN), 200, "{}", &ol_url(ISBN));
    let f = fetch(&fake, &mut pacer(), &identifiers_of(&c, 2).unwrap()).map_err(|e| e.message());
    let (res, refs) = record(&c, 2, "import", f).unwrap();
    assert_eq!((res, refs.len()), (ItemResult::default(), 0));
    let l = latest(&c, 2).unwrap().unwrap();
    assert_eq!((l.status.as_str(), l.identifier.as_str()), ("not_found", "isbn:9780000000002"));
    record(&c, 2, "manual", Err("连不上网络：超时".into())).unwrap();
    assert_eq!(latest(&c, 2).unwrap().unwrap().status, "failed");
    // 导入触发的：查过（没找到）就不再查；失败的会再查；手动的总是查
    assert_eq!(prepare(&c, 2, "import").unwrap(), None);
    assert!(prepare(&c, 2, "manual").unwrap().is_some());
    assert_eq!(prepare(&c, 3, "manual").unwrap(), None, "没有标识");
    c.execute("UPDATE items SET deleted_at = 't' WHERE id = 1", []).unwrap();
    assert_eq!(prepare(&c, 1, "manual").unwrap(), None, "回收站里的不查");
}

#[test]
fn edit_trigger_skips_unchanged_identifier() {
    let c = setup();
    let f = fetch(&doi_fake(), &mut pacer(), &identifiers_of(&c, 1).unwrap()).map_err(|e| e.message());
    record(&c, 1, "import", f).unwrap();
    assert_eq!(prepare(&c, 1, "edit").unwrap(), None, "DOI 没变");
    edit::item_update(&c, 1, &ItemPatch { fields: BTreeMap::from([("DOI".to_string(), "10.5555/other".to_string())]), ..Default::default() }).unwrap();
    assert_eq!(prepare(&c, 1, "edit").unwrap(), Some(vec![Identifier::Doi("10.5555/other".into())]));
}

// ---------------- 设置与新建 ----------------

#[test]
fn settings_and_consent() {
    let c = setup();
    assert_eq!(settings_get(&c).unwrap(), Settings { enabled: None, auto: true, email: None });
    assert!(add_prepare(&c, DOI).unwrap_err().contains("确认联网范围"), "没同意前不联网");
    let s = settings_set(&c, &SettingsPatch { enabled: Some(true), email: Some(" someone@example.org ".into()), ..Default::default() }).unwrap();
    assert_eq!(s, Settings { enabled: Some(true), auto: true, email: Some("someone@example.org".into()) });
    assert!(settings_set(&c, &SettingsPatch { email: Some("not an email".into()), ..Default::default() }).is_err());
    assert!(settings_set(&c, &SettingsPatch { email: Some("a@b".into()), ..Default::default() }).is_err());
    let s = settings_set(&c, &SettingsPatch { auto: Some(false), email: Some(String::new()), ..Default::default() }).unwrap();
    assert_eq!((s.auto, s.email), (false, None));
    settings_set(&c, &SettingsPatch { enabled: Some(false), ..Default::default() }).unwrap();
    assert!(add_prepare(&c, DOI).unwrap_err().contains("已关闭"));
}

#[test]
fn add_by_identifier() {
    let c = setup();
    settings_set(&c, &SettingsPatch { enabled: Some(true), ..Default::default() }).unwrap();
    let (todo, report) = add_prepare(&c, &format!("{} 10.5555/new.2 978-1-11-111111-3 junk", DOI.to_lowercase())).unwrap();
    assert_eq!(todo, vec![Identifier::Doi("10.5555/new.2".into()), Identifier::Isbn("9781111111113".into())]);
    assert_eq!(report.existing, vec![Existing { identifier: format!("DOI {}", DOI.to_lowercase()), item_id: 1, title: Some("Political efficacy revisited".into()) }]);
    assert_eq!(report.invalid, vec!["junk"]);
    assert!(add_prepare(&c, "junk").unwrap_err().contains("认不出"));

    let f = found();
    let (id, refs) = add_create(&c, None, &Identifier::Doi("10.5555/new.2".into()), &f).unwrap();
    assert_eq!(refs, vec![DocRef::item(id)]);
    let (ty, source, title, doi, year): (String, String, String, String, i64) = c
        .query_row("SELECT item_type, source, title, doi, year FROM items WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap();
    assert_eq!((ty.as_str(), source.as_str(), doi.as_str(), year), ("journalArticle", "lookup", "10.5555/new.2", 2021));
    assert!(title.starts_with("Political efficacy revisited"));
    let n: i64 = c.query_row("SELECT COUNT(*) FROM creators WHERE item_id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    assert_eq!(latest(&c, id).unwrap().unwrap().trigger, "add");
    // 再加同一个 DOI：认出已在库里
    let (todo, report) = add_prepare(&c, "https://doi.org/10.5555/NEW.2").unwrap();
    assert!(todo.is_empty() && report.existing[0].item_id == id);
}

// ---------------- 后台排队 ----------------

struct TestDb(Mutex<Connection>);
impl Db for TestDb {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        f(&self.0.lock().unwrap())
    }
}

fn enabled() -> Connection {
    let c = setup();
    settings_set(&c, &SettingsPatch { enabled: Some(true), ..Default::default() }).unwrap();
    c
}

#[test]
fn queue_runs_items_and_counts() {
    // 没打开联网：一条都不查
    let db = TestDb(Mutex::new(setup()));
    let q = Queue::default();
    q.push(&[1, 2], "manual");
    work(&db, &Fake::default(), &mut pacer(), &q, None);
    assert_eq!((q.status().done, q.status().stopped.as_deref()), (0, Some("联网补全已关闭，剩下的没有查")));

    let db = TestDb(Mutex::new(enabled()));
    let fake = Fake::default().on(&net::doi_url(DOI), 200, &csl(), CROSSREF).on(&ol_url(ISBN), 200, "{}", &ol_url(ISBN));
    let q = Queue::default();
    assert!(q.push(&[1, 2, 3, 99], "import"), "空闲时放进来要起线程");
    assert!(!q.push(&[1], "import"), "已在队列里的不重复放，也不再起线程");
    let dir = temp_dir("metadata-queue");
    work(&db, &fake, &mut pacer(), &q, Some(&dir));
    let p = q.status();
    assert_eq!(
        (p.running, p.total, p.done, p.filled_items, p.conflict_items, p.not_found, p.skipped, p.failed),
        (false, 4, 4, 1, 1, 1, 2, 0),
        "{p:?}"
    );
    assert!(p.finished_at.is_some() && p.stopped.is_none());
    let backups = crate::backup::list(&dir).unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].label, "before-metadata");
    // 搜索索引跟着更新：补上的期刊名能搜到
    let hits = db.with(|c| crate::search::query(c, "Synthetic Studies")).unwrap().unwrap_or_default();
    assert!(hits.contains_key(&DocRef::item(1)));
    // 下一轮：run 加一，计数重来
    assert!(q.push(&[2], "manual"));
    work(&db, &fake, &mut pacer(), &q, None);
    let p2 = q.status();
    assert_eq!((p2.run, p2.total, p2.not_found), (p.run + 1, 1, 1));
}

#[test]
fn queue_stops_when_offline_or_cancelled() {
    let c = enabled();
    for i in 10..15 {
        c.execute(&format!("INSERT INTO items (id, uid, item_type, source, created_at, updated_at) VALUES ({i}, 'X{i}', 'book', 'manual', 't', 't')"), []).unwrap();
        c.execute(&format!("INSERT INTO item_fields (item_id, field, value) VALUES ({i}, 'DOI', '10.5555/off.{i}')"), []).unwrap();
        crate::fields::refresh_columns(&c, i, "t").unwrap();
    }
    let db = TestDb(Mutex::new(c));
    let mut fake = Fake::default();
    for i in 10..15 {
        fake = fake.fail(&net::doi_url(&format!("10.5555/off.{i}")), NetError::Offline("timed out".into()));
    }
    let q = Queue::default();
    q.push(&[10, 11, 12, 13, 14], "import");
    work(&db, &fake, &mut pacer(), &q, None);
    let p = q.status();
    assert_eq!((p.done, p.failed, p.total), (3, 3, 3), "连续 3 次连不上就停，剩下的不查：{p:?}");
    assert!(p.stopped.as_deref().unwrap().contains("连不上网络"));
    let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM metadata_lookups", [], |r| r.get(0)).map_err(err)).unwrap();
    assert_eq!(n, 0, "没网时不记失败（下次还会查）");

    // 用户停止
    q.push(&[10, 11], "manual");
    q.cancel();
    work(&db, &fake, &mut pacer(), &q, None);
    let p = q.status();
    assert_eq!((p.done, p.stopped.as_deref()), (0, Some("已停止")));
    assert!(!p.running);
}

// ---------------- 真实接口（默认忽略，要联网） ----------------

/// 用公开的样例核对真实接口的返回结构（不涉及用户的库）：
/// `cargo test -p pith real_network_samples -- --ignored --nocapture`（可设 RL_CONTACT_EMAIL）
#[test]
#[ignore]
fn real_network_samples() {
    let email = std::env::var("RL_CONTACT_EMAIL").ok();
    let http = net::Ureq::new(email.as_deref());
    let p = &mut Pacer::default();
    let f = found_of(fetch(&http, p, &[Identifier::Doi("10.1038/nature14539".into())]));
    assert_eq!((f.source.as_str(), f.item_type.as_str()), ("crossref", "journalArticle"));
    assert_eq!(
        [f.fields["publicationTitle"].as_str(), f.fields["volume"].as_str(), f.fields["pages"].as_str(), f.fields["date"].as_str()],
        ["Nature", "521", "436-444", "2015-05-27"]
    );
    assert!(f.creators.len() >= 3 && f.creators[0].family.as_deref() == Some("LeCun"));
    println!("Crossref：{} 个字段，{} 位作者", f.fields.len(), f.creators.len());

    match fetch(&http, p, &[Identifier::Doi("10.48550/arXiv.1706.03762".into())]) {
        Ok(Fetched::Found { found, .. }) => println!("DataCite：来源 {}，类型 {}，{} 个字段，{} 位作者", found.source, found.item_type, found.fields.len(), found.creators.len()),
        Ok(Fetched::NotFound { message, .. }) => println!("DataCite：没找到（{message}）"),
        Err(e) => println!("DataCite：失败（{}）", e.message()),
    }
    assert!(matches!(fetch(&http, p, &[Identifier::Doi("10.5555/rl-definitely-missing-2026".into())]), Ok(Fetched::NotFound { .. })));

    let f = found_of(fetch(&http, p, &[Identifier::Isbn("9780140449136".into())]));
    assert_eq!((f.source.as_str(), f.item_type.as_str()), ("openlibrary", "book"));
    assert!(f.fields["title"].to_lowercase().contains("crime") && f.fields.contains_key("publisher") && !f.creators.is_empty());
    println!("Open Library：{} 个字段，{} 位作者", f.fields.len(), f.creators.len());
}

/// 真实 Zotero 副本：导入到临时目录，再像「导入后自动补全」那样联网查一遍。只打印汇总数字（不打印标题、作者、DOI）。
/// `ZOTERO_COPY_DIR=~/ResearchLab-test/zotero-copy RL_CONTACT_EMAIL=… cargo test -p pith real_copy_lookup -- --ignored --nocapture`
#[test]
#[ignore]
fn real_copy_lookup() {
    use crate::import::zotero;
    use crate::paths::DataPaths;
    let src = std::path::PathBuf::from(std::env::var("ZOTERO_COPY_DIR").expect("请设置 ZOTERO_COPY_DIR"));
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(!src.starts_with(std::path::Path::new(&home).join("Zotero")), "不能直接读 ~/Zotero，请用副本");
    let dir = temp_dir("metadata-real");
    let paths = DataPaths::new(dir.join("data"));
    paths.ensure().unwrap();
    let mut c = crate::db::open(&paths.library_db).unwrap();
    crate::db::migrate(&mut c, crate::db::MIGRATIONS, &paths.backups).unwrap();
    crate::search::attach_memory(&c).unwrap();
    let run = zotero::run(&c, &paths, &src, &zotero::Options::default()).unwrap();
    let email = std::env::var("RL_CONTACT_EMAIL").ok();
    settings_set(&c, &SettingsPatch { enabled: Some(true), email: email.clone(), ..Default::default() }).unwrap();
    let items = run_items(&c, run.summary.id).unwrap();
    let total: i64 = c.query_row("SELECT COUNT(*) FROM items WHERE deleted_at IS NULL", [], |r| r.get(0)).unwrap();
    let with_doi: i64 = c.query_row("SELECT COUNT(*) FROM items WHERE deleted_at IS NULL AND doi IS NOT NULL", [], |r| r.get(0)).unwrap();
    let db = TestDb(Mutex::new(c));
    let q = Queue::default();
    q.push(&items, "import");
    let t = std::time::Instant::now();
    work(&db, &net::Ureq::new(email.as_deref()), &mut Pacer::default(), &q, None);
    let secs = t.elapsed().as_secs_f64();
    let p = q.status();
    println!("文献 {total} 条，有 DOI {with_doi} 条，有 DOI 或 ISBN {} 条；用时 {secs:.1} 秒", items.len());
    println!(
        "查到 {} / 没找到 {} / 失败 {} / 跳过 {}；补上字段的 {} 条（共 {} 个字段）；有待确认的 {} 条{}",
        p.done - p.not_found - p.failed - p.skipped,
        p.not_found,
        p.failed,
        p.skipped,
        p.filled_items,
        p.filled_fields,
        p.conflict_items,
        p.stopped.map(|s| format!("；停下：{s}")).unwrap_or_default()
    );
    let c = db.0.lock().unwrap();
    let count = |sql: &str| -> Vec<(String, i64)> { c.prepare(sql).unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect() };
    println!("来源：{:?}", count("SELECT COALESCE(source, status), COUNT(*) FROM metadata_lookups GROUP BY 1 ORDER BY 2 DESC"));
    println!("没找到的原因：{:?}", count("SELECT message, COUNT(*) FROM metadata_lookups WHERE status != 'found' GROUP BY 1 ORDER BY 2 DESC"));
    let by = |kind: &str| count(&format!(
        "SELECT json_extract(j.value, '$.field'), COUNT(*) FROM metadata_lookups m, json_each(m.changes) j
          WHERE json_extract(j.value, '$.kind') = '{kind}' GROUP BY 1 ORDER BY 2 DESC"
    ));
    println!("补上的字段：{:?}", by("fill"));
    println!("与现有不同（待确认）：{:?}", by("conflict"));
}
