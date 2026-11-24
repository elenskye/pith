//! 可信度：按**数据生产者**分级（谁统计的，不是谁转述的），等级表可以由设置覆盖（`decisions/0020`「可信度」）。
//!
//! 默认表按用户的意见：中国政府官方统计、官方媒体的数字 → 低。官方数字照样列出，只是降权并标明。
//! 等级只决定排序和提示，不决定「真假」；报告里写明用的是哪张表。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 可信度等级。顺序就是高低：`Low < Medium < High`
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Low,
    Medium,
    High,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::High => "高",
            Tier::Medium => "中",
            Tier::Low => "低",
        }
    }
}

/// 数据生产者的类型（摘证据时由模型按来源正文判断，用户可以在等级表里改每类的等级）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerKind {
    /// 国际组织（国际劳工组织、经合组织、世界银行等）
    IntlOrg,
    /// 中国以外的政府统计机构
    ForeignGovStats,
    /// 学术研究（论文、高校与研究机构的调查）
    Academic,
    /// 独立调查机构、智库
    IndependentSurvey,
    /// 招聘平台、企业用自己的业务数据做的统计
    PlatformData,
    /// 企业、行业协会的报告
    IndustryReport,
    /// 市场化媒体自己采访、统计的数字
    CommercialMedia,
    /// 中国政府官方统计（统计局、人社部门等）
    CnGovStats,
    /// 中国官方媒体自己给出的数字（转述统计局的按统计局算）
    CnStateMedia,
    /// 自媒体、个人、论坛
    SelfMedia,
    /// 正文里看不出是谁统计的
    #[serde(other)]
    Unknown,
}

impl ProducerKind {
    pub const ALL: [ProducerKind; 11] = [
        ProducerKind::IntlOrg,
        ProducerKind::ForeignGovStats,
        ProducerKind::Academic,
        ProducerKind::IndependentSurvey,
        ProducerKind::PlatformData,
        ProducerKind::IndustryReport,
        ProducerKind::CommercialMedia,
        ProducerKind::CnGovStats,
        ProducerKind::CnStateMedia,
        ProducerKind::SelfMedia,
        ProducerKind::Unknown,
    ];

    /// 给模型和设置用的键
    pub fn key(self) -> &'static str {
        match self {
            ProducerKind::IntlOrg => "intl_org",
            ProducerKind::ForeignGovStats => "foreign_gov_stats",
            ProducerKind::Academic => "academic",
            ProducerKind::IndependentSurvey => "independent_survey",
            ProducerKind::PlatformData => "platform_data",
            ProducerKind::IndustryReport => "industry_report",
            ProducerKind::CommercialMedia => "commercial_media",
            ProducerKind::CnGovStats => "cn_gov_stats",
            ProducerKind::CnStateMedia => "cn_state_media",
            ProducerKind::SelfMedia => "self_media",
            ProducerKind::Unknown => "unknown",
        }
    }

    pub fn from_key(key: &str) -> ProducerKind {
        ProducerKind::ALL.into_iter().find(|k| k.key() == key.trim()).unwrap_or(ProducerKind::Unknown)
    }

    pub fn label(self) -> &'static str {
        match self {
            ProducerKind::IntlOrg => "国际组织",
            ProducerKind::ForeignGovStats => "其他国家和地区的政府统计",
            ProducerKind::Academic => "学术研究",
            ProducerKind::IndependentSurvey => "独立调查机构、智库",
            ProducerKind::PlatformData => "招聘平台、企业的业务数据",
            ProducerKind::IndustryReport => "企业、行业协会的报告",
            ProducerKind::CommercialMedia => "市场化媒体自己的调查",
            ProducerKind::CnGovStats => "中国政府官方统计",
            ProducerKind::CnStateMedia => "中国官方媒体",
            ProducerKind::SelfMedia => "自媒体、个人、论坛",
            ProducerKind::Unknown => "看不出是谁统计的",
        }
    }

    /// 给模型看的说明
    pub fn hint(self) -> &'static str {
        match self {
            ProducerKind::IntlOrg => "国际劳工组织、经合组织、世界银行、联合国机构等",
            ProducerKind::ForeignGovStats => "中国大陆以外的政府统计机构",
            ProducerKind::Academic => "同行评议的论文，高校、研究机构自己做的调查和估计",
            ProducerKind::IndependentSurvey => "不隶属政府的调查机构、智库",
            ProducerKind::PlatformData => "招聘平台、互联网平台、企业用自己的业务数据做的统计",
            ProducerKind::IndustryReport => "企业、咨询公司、行业协会发布的报告",
            ProducerKind::CommercialMedia => "市场化媒体自己采访、统计出来的数字",
            ProducerKind::CnGovStats => "国家统计局、地方统计局、人社部门等中国政府机构",
            ProducerKind::CnStateMedia => "中国官方媒体自己给出、没有说明出处的数字",
            ProducerKind::SelfMedia => "自媒体、个人博客、论坛帖子",
            ProducerKind::Unknown => "正文里看不出这个数是谁统计的",
        }
    }
}

pub const DEFAULT_TABLE_NAME: &str = "默认等级表";
pub const CUSTOM_TABLE_NAME: &str = "自定义等级表";

/// 等级表：每类数据生产者的可信度
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustTable {
    pub tiers: BTreeMap<ProducerKind, Tier>,
}

impl Default for TrustTable {
    fn default() -> Self {
        use ProducerKind::*;
        let tiers = [
            (IntlOrg, Tier::High),
            (ForeignGovStats, Tier::High),
            (Academic, Tier::High),
            (IndependentSurvey, Tier::Medium),
            (PlatformData, Tier::Medium),
            (IndustryReport, Tier::Medium),
            (CommercialMedia, Tier::Medium),
            (CnGovStats, Tier::Low),
            (CnStateMedia, Tier::Low),
            (SelfMedia, Tier::Low),
            (Unknown, Tier::Low),
        ]
        .into_iter()
        .collect();
        TrustTable { tiers }
    }
}

impl TrustTable {
    /// 表里没写的类型按默认表
    pub fn tier(&self, kind: ProducerKind) -> Tier {
        self.tiers.get(&kind).copied().unwrap_or_else(|| TrustTable::default().tiers[&kind])
    }

    /// 和默认表不同的项：（类型, 默认等级, 现在的等级）
    pub fn changes(&self) -> Vec<(ProducerKind, Tier, Tier)> {
        let default = TrustTable::default();
        ProducerKind::ALL.into_iter().filter(|k| self.tier(*k) != default.tier(*k)).map(|k| (k, default.tier(k), self.tier(k))).collect()
    }

    pub fn is_default(&self) -> bool {
        self.changes().is_empty()
    }

    pub fn name(&self) -> &'static str {
        if self.is_default() { DEFAULT_TABLE_NAME } else { CUSTOM_TABLE_NAME }
    }

    /// 补全：每类都有等级（设置里存的可能只有改过的几项）
    pub fn complete(mut self) -> TrustTable {
        let default = TrustTable::default();
        for k in ProducerKind::ALL {
            self.tiers.entry(k).or_insert(default.tiers[&k]);
        }
        self
    }

    /// 这一等级有哪些类型（报告里列表用）
    pub fn kinds_at(&self, tier: Tier) -> Vec<ProducerKind> {
        ProducerKind::ALL.into_iter().filter(|k| self.tier(*k) == tier).collect()
    }
}

/// 一串等级是不是从高到低排的（报告里来源、事实的顺序）。返回第一处排反的位置。
pub fn first_out_of_order(tiers: &[Tier]) -> Option<usize> {
    tiers.windows(2).position(|w| w[0] < w[1]).map(|i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_downweights_chinese_official_statistics() {
        let t = TrustTable::default();
        assert_eq!(t.tier(ProducerKind::CnGovStats), Tier::Low);
        assert_eq!(t.tier(ProducerKind::CnStateMedia), Tier::Low);
        assert_eq!(t.tier(ProducerKind::Academic), Tier::High);
        assert_eq!(t.tier(ProducerKind::PlatformData), Tier::Medium);
        assert!(t.is_default() && t.name() == DEFAULT_TABLE_NAME);
        assert_eq!(t.tiers.len(), ProducerKind::ALL.len());
        assert!(Tier::High > Tier::Medium && Tier::Medium > Tier::Low);
    }

    #[test]
    fn settings_can_override_and_the_table_says_so() {
        let mut t = TrustTable::default();
        t.tiers.insert(ProducerKind::CnGovStats, Tier::High);
        assert_eq!(t.tier(ProducerKind::CnGovStats), Tier::High);
        assert_eq!(t.changes(), [(ProducerKind::CnGovStats, Tier::Low, Tier::High)]);
        assert_eq!(t.name(), CUSTOM_TABLE_NAME);
        // 只存了改过的几项也行
        let partial: TrustTable = serde_json::from_str(r#"{"tiers":{"self_media":"medium"}}"#).unwrap();
        assert_eq!(partial.tier(ProducerKind::SelfMedia), Tier::Medium);
        assert_eq!(partial.tier(ProducerKind::IntlOrg), Tier::High);
        assert_eq!(partial.complete().tiers.len(), ProducerKind::ALL.len());
    }

    #[test]
    fn keys_round_trip_and_unknown_kinds_fall_back() {
        for k in ProducerKind::ALL {
            assert_eq!(ProducerKind::from_key(k.key()), k);
            assert_eq!(serde_json::to_string(&k).unwrap(), format!("\"{}\"", k.key()));
        }
        assert_eq!(ProducerKind::from_key("made_up"), ProducerKind::Unknown);
        assert_eq!(serde_json::from_str::<ProducerKind>("\"made_up\"").unwrap(), ProducerKind::Unknown);
    }

    #[test]
    fn order_check() {
        use Tier::*;
        assert_eq!(first_out_of_order(&[High, High, Medium, Low]), None);
        assert_eq!(first_out_of_order(&[Low, High]), Some(1));
        assert_eq!(first_out_of_order(&[High, Low, Medium]), Some(2));
        assert_eq!(first_out_of_order(&[]), None);
    }
}
