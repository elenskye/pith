//! 排版 Agent 的成果存在数据目录的 `format/` 下，一套规范一个文件夹：
//!   format/<规范 id>/ruleset.json          要求文档的文字、要求清单（含用户的勾选和修改）
//!   format/<规范 id>/runs/<论文 id>.json   某份论文的计划、结果、报告
//! 是 JSON 文件而不是数据库表：它们是可以重做的工作成果（重做要花钱，所以存下来），不属于文献库，不进库的备份。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::extract::{Requirement, SourceDoc};
use super::planner::{Draft, Mode};
use super::usage::Call;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ruleset {
    pub id: String,
    pub name: String,
    pub created_at: String,
    /// 模拟模型给的（不是真的提炼结果）
    pub mock: bool,
    pub docs: Vec<SourceDoc>,
    pub requirements: Vec<Requirement>,
    /// 读回模型输出时改正、丢掉的地方
    pub problems: Vec<String>,
    pub call: Option<Call>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckLine {
    pub requirement: String,
    pub text: String,
    /// Some(true) 符合、Some(false) 不符合、None 没找到 / 需人工核对
    pub ok: Option<bool>,
    pub value: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub applied_at: String,
    /// 保真校验通过并写出了文件
    pub delivered: bool,
    pub output_path: Option<String>,
    pub report_path: Option<String>,
    pub report_md: String,
    pub fidelity_problems: usize,
    pub spec_checks: usize,
    pub spec_failed: usize,
    pub checks: Vec<CheckLine>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub ruleset_id: String,
    pub created_at: String,
    pub thesis_path: String,
    pub thesis_name: String,
    pub thesis_sha256: String,
    pub mode: Mode,
    pub mock: bool,
    /// 发给模型的论文原文字数
    pub sent_chars: usize,
    pub draft: Draft,
    pub call: Option<Call>,
    pub result: Option<RunResult>,
}

/// ULID（26 位 Crockford Base32）。id 会拼进路径，先核对形状
fn check_id(id: &str) -> Result<(), String> {
    if id.len() == 26 && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()) { Ok(()) } else { Err("编号不对".into()) }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Self {
        Store { root: data_dir.join("format") }
    }

    fn dir(&self, id: &str) -> Result<PathBuf, String> {
        check_id(id)?;
        Ok(self.root.join(id))
    }

    fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
        }
        // 先写临时文件再改名：写到一半断电也不会留下半个文件
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?).map_err(|e| format!("无法写入 {}：{e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("无法写入 {}：{e}", path.display()))
    }

    fn read<T: for<'de> Deserialize<'de>>(path: &Path, what: &str) -> Result<T, String> {
        let bytes = std::fs::read(path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { format!("{what}不存在了") } else { format!("读不了{what}：{e}") })?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{what}的存档读不懂：{e}"))
    }

    pub fn save_ruleset(&self, r: &Ruleset) -> Result<(), String> {
        Self::write(&self.dir(&r.id)?.join("ruleset.json"), r)
    }

    pub fn ruleset(&self, id: &str) -> Result<Ruleset, String> {
        Self::read(&self.dir(id)?.join("ruleset.json"), "这套规范")
    }

    /// 新的在前。读不懂的存档跳过（不让一个坏文件挡住整个列表）
    pub fn rulesets(&self) -> Vec<Ruleset> {
        let Ok(rd) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut all: Vec<Ruleset> = rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).filter(|n| check_id(n).is_ok()).filter_map(|id| self.ruleset(&id).ok()).collect();
        all.sort_by(|a, b| b.id.cmp(&a.id));
        all
    }

    /// 删掉一套规范和它下面的计划、报告存档（写到论文旁边的 .docx 和报告不动）
    pub fn delete_ruleset(&self, id: &str) -> Result<(), String> {
        let dir = self.dir(id)?;
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删不掉 {}：{e}", dir.display())),
        }
    }

    pub fn save_run(&self, r: &Run) -> Result<(), String> {
        check_id(&r.id)?;
        Self::write(&self.dir(&r.ruleset_id)?.join("runs").join(format!("{}.json", r.id)), r)
    }

    pub fn run(&self, ruleset_id: &str, id: &str) -> Result<Run, String> {
        check_id(id)?;
        Self::read(&self.dir(ruleset_id)?.join("runs").join(format!("{id}.json")), "这份论文的计划")
    }

    pub fn runs(&self, ruleset_id: &str) -> Vec<Run> {
        let Ok(dir) = self.dir(ruleset_id) else { return Vec::new() };
        let Ok(rd) = std::fs::read_dir(dir.join("runs")) else { return Vec::new() };
        let mut all: Vec<Run> = rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from))
            .filter(|n| check_id(n).is_ok())
            .filter_map(|id| self.run(ruleset_id, &id).ok())
            .collect();
        all.sort_by(|a, b| b.id.cmp(&a.id));
        all
    }

    pub fn delete_run(&self, ruleset_id: &str, id: &str) -> Result<(), String> {
        check_id(id)?;
        let p = self.dir(ruleset_id)?.join("runs").join(format!("{id}.json"));
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删不掉 {}：{e}", p.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::new_uid;
    use crate::db::tests::temp_dir;

    #[test]
    fn rulesets_and_runs_round_trip() {
        let dir = temp_dir("agent-store");
        let store = Store::new(&dir);
        assert!(store.rulesets().is_empty());
        let rs = Ruleset { id: new_uid().unwrap(), name: "规范".into(), created_at: "2026-11-10T00:00:00.000Z".into(), mock: true, docs: vec![], requirements: vec![], problems: vec![], call: None };
        store.save_ruleset(&rs).unwrap();
        assert_eq!(store.ruleset(&rs.id).unwrap(), rs);
        let run = Run {
            id: new_uid().unwrap(),
            ruleset_id: rs.id.clone(),
            created_at: rs.created_at.clone(),
            thesis_path: "/x/论文.docx".into(),
            thesis_name: "论文.docx".into(),
            thesis_sha256: "00".into(),
            mode: Mode::Brief,
            mock: true,
            sent_chars: 10,
            draft: Draft::default(),
            call: None,
            result: None,
        };
        store.save_run(&run).unwrap();
        assert_eq!(store.runs(&rs.id), vec![run.clone()]);
        assert_eq!(store.run(&rs.id, &run.id).unwrap(), run);
        // 坏存档不挡列表
        std::fs::create_dir_all(dir.join("format/01ARZ3NDEKTSV4RRFFQ69G5FAV")).unwrap();
        std::fs::write(dir.join("format/01ARZ3NDEKTSV4RRFFQ69G5FAV/ruleset.json"), "{").unwrap();
        assert_eq!(store.rulesets().len(), 1);
        store.delete_run(&rs.id, &run.id).unwrap();
        assert!(store.runs(&rs.id).is_empty());
        store.delete_ruleset(&rs.id).unwrap();
        assert!(store.ruleset(&rs.id).unwrap_err().contains("不存在"));
    }

    #[test]
    fn ids_cannot_escape_the_folder() {
        let dir = temp_dir("agent-store-ids");
        let store = Store::new(&dir);
        for bad in ["../x", "", "a/b", "01arz3ndektsv4rrffq69g5fav", "..", "01ARZ3NDEKTSV4RRFFQ69G5FA/"] {
            assert!(store.ruleset(bad).is_err(), "{bad}");
            assert!(store.delete_ruleset(bad).is_err(), "{bad}");
            assert!(store.run(bad, "01ARZ3NDEKTSV4RRFFQ69G5FAV").is_err());
        }
    }
}
