//! 取回的网页（migration 0012）：一页一行，正文切成块，每块一个编号 `[W12]`。
//! 同一个网址、同样的内容只存一份（再取一次还是那些编号）；内容变了就是新的一页。

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::common::err;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: i64,
    pub url: String,
    pub final_url: String,
    pub title: String,
    /// 网站（去掉 www.）
    pub site: String,
    pub sha256: String,
    pub ext: String,
    pub kind: String,
    pub via: Option<String>,
    pub fetched_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: i64,
    pub page_id: i64,
    pub seq: i64,
    pub role: String,
    pub heading: String,
    pub text: String,
}

/// 网址里的网站：`https://www.stats.gov.cn/x` → `stats.gov.cn`
pub fn site(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_lowercase()
}

/// 要存的一页（下载好了的）
pub struct NewPage<'a> {
    pub url: &'a str,
    pub final_url: &'a str,
    pub title: &'a str,
    pub sha256: &'a str,
    pub ext: &'a str,
    pub kind: &'a str,
    pub via: Option<&'a str>,
    pub text: &'a str,
    pub fetched_at: &'a str,
}

/// 存一页、切块；同一个网址同样的内容已经有了就返回那一页
pub fn save(c: &Connection, p: &NewPage, conversation: Option<&str>) -> Result<i64, String> {
    if let Some(id) = c.query_row("SELECT id FROM web_pages WHERE final_url = ?1 AND sha256 = ?2 ORDER BY id LIMIT 1", params![p.final_url, p.sha256], |r| r.get::<_, i64>(0)).optional().map_err(err)? {
        return Ok(id);
    }
    let conv: Option<i64> = match conversation {
        Some(uid) => c.query_row("SELECT id FROM conversations WHERE uid = ?1", [uid], |r| r.get(0)).optional().map_err(err)?,
        None => None,
    };
    let tx = crate::common::Tx::begin(c)?;
    tx.execute(
        "INSERT INTO web_pages (url, final_url, title, sha256, ext, kind, via, conversation_id, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![p.url, p.final_url, p.title.trim(), p.sha256, p.ext, p.kind, p.via, conv, p.fetched_at],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    {
        let mut ins = tx.prepare("INSERT INTO web_blocks (page_id, seq, role, heading, text) VALUES (?1, ?2, ?3, ?4, ?5)").map_err(err)?;
        for (seq, b) in crate::kb::passages::chunk(p.text).iter().enumerate() {
            ins.execute(params![id, seq as i64, b.role, b.unit_label, b.text]).map_err(err)?;
        }
    }
    tx.commit()?;
    Ok(id)
}

fn page_row(r: &rusqlite::Row) -> rusqlite::Result<Page> {
    let final_url: String = r.get(2)?;
    Ok(Page { id: r.get(0)?, url: r.get(1)?, site: site(&final_url), final_url, title: r.get(3)?, sha256: r.get(4)?, ext: r.get(5)?, kind: r.get(6)?, via: r.get(7)?, fetched_at: r.get(8)? })
}

const PAGE: &str = "SELECT id, url, final_url, title, sha256, ext, kind, via, fetched_at FROM web_pages";

pub fn page(c: &Connection, id: i64) -> Result<Option<Page>, String> {
    c.query_row(&format!("{PAGE} WHERE id = ?1"), [id], page_row).optional().map_err(err)
}

fn block_row(r: &rusqlite::Row) -> rusqlite::Result<Block> {
    Ok(Block { id: r.get(0)?, page_id: r.get(1)?, seq: r.get(2)?, role: r.get(3)?, heading: r.get(4)?, text: r.get(5)? })
}

pub fn blocks(c: &Connection, page: i64) -> Result<Vec<Block>, String> {
    let mut st = c.prepare("SELECT id, page_id, seq, role, heading, text FROM web_blocks WHERE page_id = ?1 ORDER BY seq").map_err(err)?;
    st.query_map([page], block_row).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// 一块和它那一页
pub fn block(c: &Connection, id: i64) -> Result<Option<(Block, Page)>, String> {
    let Some(b) = c.query_row("SELECT id, page_id, seq, role, heading, text FROM web_blocks WHERE id = ?1", [id], block_row).optional().map_err(err)? else { return Ok(None) };
    Ok(page(c, b.page_id)?.map(|p| (b, p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_hosts_without_www() {
        assert_eq!(site("https://www.Stats.gov.cn/sj/zxfb/202401/t1.html?x=1"), "stats.gov.cn");
        assert_eq!(site("http://user@example.org:8080/a"), "example.org");
        assert_eq!(site("plato.stanford.edu/entries/smith"), "plato.stanford.edu");
    }
}
