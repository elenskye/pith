//! 去重提示（L9）：DOI、ISBN（统一成 13 位）、规范化标题 + 年份相同的文献连成一组（回收站里的不算）。
//! 用户标了「不是重复」的两条之间不再连线；合并在 `edit::items_merge`。

use crate::common::err;
use crate::text;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    /// 按 id 升序
    pub items: Vec<i64>,
    /// 「DOI 相同」「ISBN 相同」「标题与年份相同」
    pub reasons: Vec<String>,
}

const REASONS: [&str; 3] = ["DOI 相同", "ISBN 相同", "标题与年份相同"];

fn find(parent: &mut HashMap<i64, i64>, x: i64) -> i64 {
    let p = *parent.get(&x).unwrap_or(&x);
    if p == x {
        return x;
    }
    let root = find(parent, p);
    parent.insert(x, root);
    root
}

pub fn groups(c: &Connection) -> Result<Vec<DupGroup>, String> {
    let mut buckets: HashMap<(usize, String), Vec<i64>> = HashMap::new();
    let mut st = c.prepare("SELECT id, doi, isbn, title_norm, year FROM items WHERE deleted_at IS NULL").map_err(err)?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<i64>>(4)?)))
        .map_err(err)?;
    for row in rows {
        let (id, doi, isbn, title, year) = row.map_err(err)?;
        if let Some(d) = doi.filter(|d| !d.is_empty()) {
            buckets.entry((0, d.to_lowercase())).or_default().push(id);
        }
        if let Some(i) = isbn.as_deref().and_then(text::isbn13) {
            buckets.entry((1, i)).or_default().push(id);
        }
        if let Some(t) = title.filter(|t| t.chars().count() >= 4) {
            buckets.entry((2, format!("{t}|{}", year.map(|y| y.to_string()).unwrap_or_default()))).or_default().push(id);
        }
    }
    let mut dismissed: HashSet<(i64, i64)> = HashSet::new();
    let mut st = c.prepare("SELECT item_a, item_b FROM duplicate_dismissals").map_err(err)?;
    for r in st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)? {
        dismissed.insert(r.map_err(err)?);
    }
    let mut parent: HashMap<i64, i64> = HashMap::new();
    let mut edges: Vec<(i64, i64, usize)> = Vec::new();
    for ((reason, _), ids) in &buckets {
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                let pair = (a.min(b), a.max(b));
                if a == b || dismissed.contains(&pair) {
                    continue;
                }
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent.insert(ra, rb);
                }
                edges.push((pair.0, pair.1, *reason));
            }
        }
    }
    let mut comps: HashMap<i64, (BTreeSet<i64>, BTreeSet<usize>)> = HashMap::new();
    for (a, b, reason) in edges {
        let root = find(&mut parent, a);
        let e = comps.entry(root).or_default();
        e.0.insert(a);
        e.0.insert(b);
        e.1.insert(reason);
    }
    let mut out: Vec<DupGroup> = comps
        .into_values()
        .map(|(items, reasons)| DupGroup { items: items.into_iter().collect(), reasons: reasons.into_iter().map(|r| REASONS[r].to_string()).collect() })
        .collect();
    // 新加入的组在前
    out.sort_by(|a, b| b.items.last().cmp(&a.items.last()));
    Ok(out)
}

/// 标记「不是重复」：这几条两两之间不再提示。
pub fn dismiss(c: &Connection, ids: &[i64]) -> Result<(), String> {
    let now = text::now_iso();
    for (i, &a) in ids.iter().enumerate() {
        for &b in &ids[i + 1..] {
            if a != b {
                c.execute(
                    "INSERT OR IGNORE INTO duplicate_dismissals (item_a, item_b, created_at) VALUES (?1, ?2, ?3)",
                    params![a.min(b), a.max(b), now],
                )
                .map_err(err)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    fn item(c: &Connection, title: &str, year: Option<i64>, doi: Option<&str>, isbn: Option<&str>) -> i64 {
        c.execute(
            "INSERT INTO items (uid, item_type, title, title_norm, year, doi, isbn, source, created_at, updated_at) VALUES (?1, 'book', ?2, ?3, ?4, ?5, ?6, 'manual', 't', 't')",
            params![crate::common::new_uid().unwrap(), title, text::title_norm(title), year, doi, isbn],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    #[test]
    fn groups_by_doi_isbn_and_title() {
        let c = memory_db();
        let a = item(&c, "Synthetic Panel Study", Some(2019), Some("10.1/ABC"), None);
        let b = item(&c, "Completely different title", Some(2001), Some("10.1/abc"), None);
        let x = item(&c, "Wealth of Nations", Some(1776), None, Some("030640615X"));
        let y = item(&c, "The Wealth of Nations (reprint)", Some(1999), None, Some("9780306406157"));
        let p = item(&c, "互联网使用与政治效能感", Some(2021), None, None);
        let q = item(&c, "互联网使用与政治效能感！", Some(2021), None, None);
        item(&c, "互联网使用与政治效能感", Some(2022), None, None);
        item(&c, "Intro", Some(2020), None, None);
        item(&c, "Intro", Some(2020), None, None);
        item(&c, "Ab", None, None, None);
        item(&c, "Ab", None, None, None);
        let gone = item(&c, "Synthetic Panel Study", Some(2019), None, None);
        c.execute("UPDATE items SET deleted_at = 't' WHERE id = ?1", [gone]).unwrap();

        let g = groups(&c).unwrap();
        let find = |id: i64| g.iter().find(|x| x.items.contains(&id)).cloned();
        assert_eq!(find(a).unwrap().items, vec![a, b]);
        assert_eq!(find(a).unwrap().reasons, ["DOI 相同"], "DOI 大小写不同也算");
        assert_eq!(find(x).unwrap().items, vec![x, y], "ISBN-10 与 ISBN-13 是同一本");
        assert_eq!(find(p).unwrap().items, vec![p, q], "标点不同算同一标题；年份不同不算");
        assert!(g.iter().any(|x| x.reasons == ["标题与年份相同"] && x.items.len() == 2 && x.items != vec![p, q]), "Intro 两条");
        assert_eq!(g.len(), 4, "两个字的短标题不比较，回收站里的不算：{g:?}");

        dismiss(&c, &[a, b]).unwrap();
        assert!(groups(&c).unwrap().iter().all(|x| !x.items.contains(&a)));
        dismiss(&c, &[a, b]).unwrap();
    }
}
