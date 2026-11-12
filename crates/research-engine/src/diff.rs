//! 重查对比（`decisions/0020`「重查」）：同一问题的新旧两次调查，按（指标、地区、人群、时间）对齐已核实的事实，
//! 标出变了的数字、新增和消失的来源、变了的结论。
//!
//! 对齐靠的是模型摘录时写的指标名等文字（折叠空白和全半角后比较）。两次写法不同的会显示成一条消失、一条新增。

use serde::Serialize;

use crate::flow::normalize_url;
use crate::fold::fold;
use crate::model::{Fact, Run};
use crate::numbers::Found;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactBrief {
    pub id: String,
    pub metric: String,
    pub region: String,
    pub population: String,
    pub period: String,
    pub producer: String,
    pub value: String,
    pub source: String,
}

fn brief(f: &Fact) -> FactBrief {
    FactBrief { id: f.id.clone(), metric: f.metric.clone(), region: f.region.clone(), population: f.population.clone(), period: f.period.clone(), producer: f.producer.clone(), value: f.shown(), source: f.source.clone() }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactChange {
    pub old: FactBrief,
    pub new: FactBrief,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBrief {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub previous_uid: String,
    pub previous_at: String,
    /// 对得上、但数值变了的
    pub changed: Vec<FactChange>,
    /// 这次有、上次没有的
    pub added: Vec<FactBrief>,
    /// 上次有、这次没有的
    pub removed: Vec<FactBrief>,
    /// 对得上且数值没变的条数
    pub unchanged: usize,
    pub sources_added: Vec<SourceBrief>,
    pub sources_removed: Vec<SourceBrief>,
    /// 结论：这次新出现的、上次有这次没有的、两次一样的条数
    pub conclusions_added: Vec<String>,
    pub conclusions_removed: Vec<String>,
    pub conclusions_same: usize,
}

impl Diff {
    pub fn nothing_changed(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty() && self.sources_added.is_empty() && self.sources_removed.is_empty() && self.conclusions_added.is_empty() && self.conclusions_removed.is_empty()
    }
}

fn same_value(a: &Fact, b: &Fact) -> bool {
    let bases = |f: &Fact| -> Option<Vec<(String, String)>> {
        let found: &Found = f.check.parsed.as_ref()?;
        found.quantities().iter().map(|q| q.base().map(|d| (d.trimmed().to_string(), q.unit.clone()))).collect()
    };
    match (bases(a), bases(b)) {
        (Some(x), Some(y)) => x == y,
        _ => fold(&a.shown()) == fold(&b.shown()),
    }
}

pub fn compare(old: &Run, new: &Run) -> Diff {
    let mut d = Diff { previous_uid: old.uid.clone(), previous_at: old.finished_at.clone().unwrap_or_else(|| old.updated_at.clone()), ..Default::default() };

    let old_facts: Vec<&Fact> = old.verified_facts().collect();
    let new_facts: Vec<&Fact> = new.verified_facts().collect();
    let mut old_used = vec![false; old_facts.len()];
    let producer = |f: &Fact| fold(&f.producer).to_lowercase();
    for n in &new_facts {
        let key = n.key();
        let candidates: Vec<usize> = (0..old_facts.len()).filter(|i| !old_used[*i] && old_facts[*i].key() == key).collect();
        // 同一个键下先找同一个生产者的；找不到而且两边都只有这一条，也当成同一格
        let same_producer = candidates.iter().copied().find(|i| producer(old_facts[*i]) == producer(n));
        let alone = (candidates.len() == 1 && new_facts.iter().filter(|x| x.key() == key).count() == 1).then(|| candidates[0]);
        match same_producer.or(alone) {
            Some(i) => {
                old_used[i] = true;
                if same_value(old_facts[i], n) {
                    d.unchanged += 1;
                } else {
                    d.changed.push(FactChange { old: brief(old_facts[i]), new: brief(n) });
                }
            }
            None => d.added.push(brief(n)),
        }
    }
    d.removed = old_facts.iter().zip(&old_used).filter(|(_, used)| !**used).map(|(f, _)| brief(f)).collect();

    // 来源：只比读到了内容的
    let readable = |r: &Run| -> Vec<SourceBrief> { r.sources.iter().filter(|s| s.usable()).map(|s| SourceBrief { id: s.id.clone(), title: s.title.clone(), url: s.url.clone() }).collect() };
    let (old_sources, new_sources) = (readable(old), readable(new));
    let has = |list: &[SourceBrief], url: &str| list.iter().any(|s| normalize_url(&s.url) == normalize_url(url));
    d.sources_added = new_sources.iter().filter(|s| !has(&old_sources, &s.url)).cloned().collect();
    d.sources_removed = old_sources.iter().filter(|s| !has(&new_sources, &s.url)).cloned().collect();

    // 结论：按文字比（折叠后）
    let texts = |r: &Run| -> Vec<String> { r.conclusions.iter().map(|c| c.text()).collect() };
    let (old_k, new_k) = (texts(old), texts(new));
    let within = |list: &[String], t: &str| list.iter().any(|x| fold(x) == fold(t));
    d.conclusions_added = new_k.iter().filter(|t| !within(&old_k, t)).cloned().collect();
    d.conclusions_removed = old_k.iter().filter(|t| !within(&new_k, t)).cloned().collect();
    d.conclusions_same = new_k.iter().filter(|t| within(&old_k, t)).count();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::tests::verified;
    use crate::model::{Conclusion, Segment, Source};

    fn fact(id: &str, metric: &str, period: &str, producer: &str, value: &str) -> Fact {
        let mut f = verified(id, value, "%");
        f.metric = metric.into();
        f.period = period.into();
        f.producer = producer.into();
        f
    }

    fn source(id: &str, url: &str) -> Source {
        Source { id: id.into(), url: url.into(), title: format!("来源 {id}"), sha256: Some("00".into()), ..Default::default() }
    }

    fn conclusion(text: &str) -> Conclusion {
        Conclusion { segments: vec![Segment::Text { text: text.into() }], ..Default::default() }
    }

    #[test]
    fn aligned_by_metric_region_population_period() {
        let old = Run {
            uid: "OLD".into(),
            finished_at: Some("2026-11-01T08:00:00".into()),
            facts: vec![
                fact("F1", "青年失业率", "2024 年", "示例市统计局", "12.3"),
                fact("F2", "青年失业率", "2023 年", "示例市统计局", "13.4"),
                fact("F3", "青年失业率", "2024 年", "示例大学", "15.8"),
                fact("F4", "求人倍率", "2024 年", "示例市人社局", "1.25"),
            ],
            sources: vec![source("S1", "https://stats.example.invalid/2024"), source("S2", "https://old.example.invalid/gone")],
            conclusions: vec![conclusion("失业率为 12.3%。"), conclusion("求人倍率为 1.25。")],
            ..Default::default()
        };
        let new = Run {
            uid: "NEW".into(),
            facts: vec![
                // 修订过：数值变了
                fact("F1", "青年 失业率", "２０２４ 年", "示例市统计局", "12.6"),
                // 没变（写法不同但值相同）
                fact("F2", "青年失业率", "2023 年", "示例市统计局", "13.40"),
                fact("F3", "青年失业率", "2024 年", "示例大学", "15.8"),
                // 新增
                fact("F4", "青年失业率", "2025 年", "示例市统计局", "11.9"),
            ],
            sources: vec![source("S1", "https://stats.example.invalid/2024/"), source("S2", "https://new.example.invalid/2025")],
            conclusions: vec![conclusion("失业率为 12.6%。"), conclusion("求人倍率为 1.25。")],
            ..Default::default()
        };
        let d = compare(&old, &new);
        assert_eq!(d.previous_uid, "OLD");
        assert_eq!(d.changed.len(), 1);
        assert_eq!((d.changed[0].old.value.as_str(), d.changed[0].new.value.as_str()), ("12.3%", "12.6%"));
        assert_eq!(d.unchanged, 2);
        assert_eq!(d.added.iter().map(|f| f.period.as_str()).collect::<Vec<_>>(), ["2025 年"]);
        assert_eq!(d.removed.iter().map(|f| f.metric.as_str()).collect::<Vec<_>>(), ["求人倍率"]);
        assert_eq!(d.sources_added.iter().map(|s| s.url.as_str()).collect::<Vec<_>>(), ["https://new.example.invalid/2025"]);
        assert_eq!(d.sources_removed.iter().map(|s| s.url.as_str()).collect::<Vec<_>>(), ["https://old.example.invalid/gone"]);
        assert_eq!(d.conclusions_added, ["失业率为 12.6%。"]);
        assert_eq!(d.conclusions_removed, ["失业率为 12.3%。"]);
        assert_eq!(d.conclusions_same, 1);
        assert!(!d.nothing_changed());
        assert!(compare(&old, &old).nothing_changed());
    }

    #[test]
    fn a_single_slot_is_matched_even_if_the_producer_is_named_differently() {
        let old = Run { facts: vec![fact("F1", "青年失业率", "2024 年", "示例市统计局", "12.3")], ..Default::default() };
        let new = Run { facts: vec![fact("F1", "青年失业率", "2024 年", "示例统计局（市）", "12.3")], ..Default::default() };
        let d = compare(&old, &new);
        assert_eq!((d.unchanged, d.added.len(), d.removed.len()), (1, 0, 0));
        // 没核实的不参加对比
        let mut unverified = fact("F2", "求人倍率", "2024 年", "x", "1.2");
        unverified.check.status = crate::model::CheckStatus::Unverified;
        let new = Run { facts: vec![fact("F1", "青年失业率", "2024 年", "示例市统计局", "12.3"), unverified], ..Default::default() };
        assert!(compare(&old, &new).added.is_empty());
    }
}
