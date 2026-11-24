//! 中英对齐（`decisions/0024`「中英对齐」；v0.5 起全书对齐）。
//!
//! - **章**按（第几篇, 第几章）对上；不在章里的正文单元（引言、篇名下面的引言、附录）按引用名对上。
//! - **章内**：正文段数相同，按顺序一段对一段（`exact`，和 v0.4 一样）；段数不同，做一次单调对齐（`fitted`）：
//!   动态规划，允许 1-1、1-2、2-1、1-3、3-1，实在对不上的落空（1-0 / 0-1），顺序不许交叉。
//!   代价 = 这种对法本身的代价 + 两边长度差多少（Gale–Church 那一类：按全书的长度比折算）− 两边共有的锚点
//!   （阿拉伯数字、年份、保留的拉丁字母专名，越少见越值钱）− 调用方给的语义相似度（可以没有）。
//!   表格段（文字里有制表符）在另一个版本里常常是图片：不算长度，落空几乎不要代价。
//! - **置信度**：对得稳的段落占多少（按长度）。落空的、长度差得离谱又没有锚点或相似度撑着的都算不稳。
//!   低于 `Options::low_below` 标成低，页面上提示「自动对齐，可能有偏差」；低于 `Options::none_below` 就不对（不硬对）。
//! - `pairs` 用多行表示一对多：（a1, b1）（a2, b1）是两段对一段。没有手动对齐。
//!
//! 相似度由调用方给（`Similarity`：参数是两边段落的 seq，返回余弦相似度，没有向量返回 None）。这里不联网、不算向量；
//! App 一侧用本机向量模型的段落向量。没有相似度时只用长度和锚点。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::text::{Paragraph, Role, Text, UnitKind, is_cjk};

/// 对齐算法的版本：规则或默认权重变了就加一。App 把它和对齐结果存在一起，读到旧的就重算（对齐是派生数据）。
/// 1 = v0.4（段数相同才对齐）；2 = v0.5（单调对齐）
pub const VERSION: u32 = 2;

/// 两段之间的语义相似度：`(原著段 seq, 译本段 seq)` → 余弦相似度（-1..1）；哪一边没有向量就返回 None
pub type Similarity<'a> = &'a dyn Fn(usize, usize) -> Option<f32>;

/// 一章是怎么对上的
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// 正文段数相同，按顺序一段对一段
    Exact,
    /// 段数不同，算法对的（可能有偏差）
    Fitted,
    /// 没对上：译本里没有这一章、有一边没有正文，或者差得太远
    #[default]
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "UnitAlignmentIn")]
pub struct UnitAlignment {
    /// 原著那边的单元引用名（I.iii）
    pub key: String,
    pub label: String,
    pub other_label: String,
    pub unit: usize,
    pub other_unit: Option<usize>,
    pub count: usize,
    pub other_count: usize,
    /// 有对齐结果（`exact` 或 `fitted`）
    pub aligned: bool,
    pub method: Method,
    /// 0–1：对得稳的段落占多少（按长度算；`none` 是 0）。`exact` 的章照样算：一般是 1，
    /// 低了说明段数碰巧相同、中间其实错开了（照样按顺序对，只是标出来）
    pub confidence: f32,
    /// 置信度低，页面上要提示
    pub low: bool,
    /// 一对多、多对一的有几组
    pub merged: usize,
    /// 落空的有几段（两边合计；表格段在另一边是图片时也算在这里）
    pub unmatched: usize,
}

/// 读旧的 JSON（v0.4 存的只有 `aligned`）：新字段按 `aligned` 补上
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnitAlignmentIn {
    key: String,
    label: String,
    other_label: String,
    unit: usize,
    other_unit: Option<usize>,
    count: usize,
    other_count: usize,
    aligned: bool,
    method: Option<Method>,
    confidence: Option<f32>,
    #[serde(default)]
    low: bool,
    #[serde(default)]
    merged: usize,
    #[serde(default)]
    unmatched: usize,
}

impl From<UnitAlignmentIn> for UnitAlignment {
    fn from(u: UnitAlignmentIn) -> Self {
        let method = u.method.unwrap_or(if u.aligned { Method::Exact } else { Method::None });
        UnitAlignment {
            key: u.key,
            label: u.label,
            other_label: u.other_label,
            unit: u.unit,
            other_unit: u.other_unit,
            count: u.count,
            other_count: u.other_count,
            aligned: u.aligned,
            method,
            confidence: u.confidence.unwrap_or(if u.aligned { 1.0 } else { 0.0 }),
            low: u.low,
            merged: u.merged,
            unmatched: u.unmatched,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alignment {
    pub units: Vec<UnitAlignment>,
    /// （原著段 seq, 译本段 seq），按原著的顺序；一对多、多对一写成多行
    pub pairs: Vec<(usize, usize)>,
}

impl Alignment {
    /// 原著的一段对着译本的哪几段（按顺序；没对上是空的）
    pub fn other_of(&self, seq: usize) -> Vec<usize> {
        self.pairs.iter().filter(|(a, _)| *a == seq).map(|(_, b)| *b).collect()
    }

    /// 译本的一段对着原著的哪几段
    pub fn original_of(&self, other: usize) -> Vec<usize> {
        self.pairs.iter().filter(|(_, b)| *b == other).map(|(a, _)| *a).collect()
    }

    /// 原著的一段所在的那一组：（原著的几段, 译本的几段）。两段对一段时，两段各自查到的是同一组
    pub fn group_of(&self, seq: usize) -> (Vec<usize>, Vec<usize>) {
        let others = self.other_of(seq);
        let mut originals: Vec<usize> = others.iter().flat_map(|o| self.original_of(*o)).collect();
        originals.sort_unstable();
        originals.dedup();
        if originals.is_empty() {
            originals.push(seq);
        }
        (originals, others)
    }

    /// 没对上的章
    pub fn mismatched(&self) -> Vec<&UnitAlignment> {
        self.units.iter().filter(|u| !u.aligned).collect()
    }

    /// 算法对的、置信度低的章
    pub fn low(&self) -> Vec<&UnitAlignment> {
        self.units.iter().filter(|u| u.low).collect()
    }
}

/// 权重。默认值是用两本真实的《国富论》（Penguin 英文版、杨敬年中译本）定的：在段数相同的章里人为合并、拆分、删掉段落，
/// 看算法找不找得回来（`tests/real_books.rs`，结果记在 README）。
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// 长度差的尺度：标准差 = sqrt(`len_linear`² · L + `len_ratio`² · L²)，L 是两边折算后的平均长度（按中文字数）
    pub len_linear: f32,
    pub len_ratio: f32,
    /// 长度代价的上限（两段长度差得再离谱也只罚这么多）
    pub len_cap: f32,
    /// 1-2 / 2-1 的代价
    pub merge: f32,
    /// 1-3 / 3-1 的代价
    pub merge3: f32,
    /// 1-0 / 0-1 的代价
    pub skip: f32,
    /// 表格段落空的代价
    pub skip_table: f32,
    /// 共有的锚点：每个的奖励（按稀有度折算，一组最多算 `anchor_cap` 个）
    pub anchor: f32,
    pub anchor_cap: f32,
    /// 语义相似度的权重（乘在「比同一章里不相干的两段高出几个标准差」上，最多 `similarity_cap` 个）
    pub similarity: f32,
    pub similarity_cap: f32,
    /// 置信度低于它标成低
    pub low_below: f32,
    /// 置信度低于它就不对
    pub none_below: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { len_linear: 1.5, len_ratio: 0.08, len_cap: 12.0, merge: 3.0, merge3: 6.0, skip: 5.0, skip_table: 0.5, anchor: 1.0, anchor_cap: 3.0, similarity: 1.0, similarity_cap: 6.0, low_below: 0.85, none_below: 0.4 }
    }
}

/// 一个单元在两个版本之间对得上的「身份」
fn identity(t: &Text, unit: usize) -> Option<String> {
    let u = &t.units[unit];
    match u.kind {
        UnitKind::Chapter => Some(format!("c:{}:{}", u.book.unwrap_or(0), u.chapter?)),
        // 篇名单元自己名下的正文是这一篇的引言（英文版第四篇的引言没有单独的目录项）
        UnitKind::Book => Some(format!("intro:{}", u.book.unwrap_or(0))),
        UnitKind::Part | UnitKind::Container => {
            let intro = u.key.rsplit('.').next().is_some_and(|k| k == "Intro");
            Some(if intro { format!("intro:{}", u.book.unwrap_or(0)) } else { format!("u:{}", u.key) })
        }
        _ => None,
    }
}

// ---------- 章内的单调对齐 ----------

/// 拉丁字母折算成中文字数：英文约 3.3 个字母对一个汉字。只影响中英混排的段落，整体的长度比另算
const LATIN_WEIGHT: f32 = 0.3;

/// 一段的长度（按中文字数折算）：汉字算 1，其他字母和数字算 0.3，标点和空白不算
pub fn size(text: &str) -> f32 {
    text.chars().map(|c| if is_cjk(c) { 1.0 } else if c.is_alphanumeric() { LATIN_WEIGHT } else { 0.0 }).sum()
}

/// 表格段：文字里有制表符（普通段落的空白都合并成了空格，制表符只会来自表格）
pub fn is_table(text: &str) -> bool {
    text.contains('\t')
}

/// 锚点：两个版本里都会原样出现的东西。阿拉伯数字（去掉千分位的逗号；一位数太常见，不要）、
/// 中文段落里保留的拉丁字母词（专名、原文术语；全拉丁字母的段落不取词，否则每个词都是锚点）
pub fn anchors(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let cjk = chars.iter().filter(|c| is_cjk(**c)).count();
    let latin = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();
    // 一个汉字约合三个字母：中文占一半以上才算中文段落
    let words = cjk * 3 > latin;
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == ',' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() && !s.is_empty())) {
                if chars[i] != ',' {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if s.len() >= 2 {
                out.push(s);
            }
        } else if c.is_ascii_alphabetic() {
            let mut s = String::new();
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                s.push(chars[i].to_ascii_lowercase());
                i += 1;
            }
            if words && s.len() >= 3 {
                out.push(s);
            }
        } else {
            i += 1;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 英文那边没法先知道哪些词在译本里保留了原文：对齐一章时，把译本里出现过的拉丁字母词在原著段落里也找出来
fn latin_words(text: &str, wanted: &HashMap<String, usize>) -> Vec<String> {
    let mut out: Vec<String> = text.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() >= 3).map(|w| w.to_ascii_lowercase()).filter(|w| wanted.contains_key(w)).collect();
    out.sort();
    out.dedup();
    out
}

/// 一段（对齐只看这些）
#[derive(Clone, Debug)]
struct Piece {
    seq: usize,
    /// 折算后的长度；表格段是 0
    size: f32,
    table: bool,
    anchors: Vec<String>,
}

fn pieces(t: &Text, unit: usize) -> Vec<Piece> {
    t.body_of(unit)
        .map(|p| {
            let table = is_table(&p.text);
            Piece { seq: p.seq, size: if table { 0.0 } else { size(&p.text) }, table, anchors: anchors(&p.text) }
        })
        .collect()
}

/// 一组：原著的 `a.0..a.1` 段对译本的 `b.0..b.1` 段（下标是章内的第几段）
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bead {
    a: (usize, usize),
    b: (usize, usize),
    /// 对得稳不稳（算置信度用）
    weak: bool,
}

/// 不相干的两段之间相似度的平均值和标准差（全书取样）
#[derive(Clone, Copy, Debug)]
struct Baseline {
    mean: f32,
    deviation: f32,
}

struct Chapter<'a> {
    a: &'a [Piece],
    b: &'a [Piece],
    /// 译本长度 / 原著长度（全书）
    ratio: f32,
    opt: &'a Options,
    similarity: Option<(Similarity<'a>, Baseline)>,
    /// 锚点在这一章两边各出现在几段里
    frequency: HashMap<&'a str, (usize, usize)>,
    /// 相似度算过的记下来（NaN = 没算过，无穷大 = 这两段没有向量）
    memo: std::cell::RefCell<Vec<f32>>,
}

impl<'a> Chapter<'a> {
    fn new(a: &'a [Piece], b: &'a [Piece], ratio: f32, opt: &'a Options, similarity: Option<(Similarity<'a>, Baseline)>) -> Self {
        let mut frequency: HashMap<&str, (usize, usize)> = HashMap::new();
        for p in a {
            for t in &p.anchors {
                frequency.entry(t.as_str()).or_default().0 += 1;
            }
        }
        for p in b {
            for t in &p.anchors {
                frequency.entry(t.as_str()).or_default().1 += 1;
            }
        }
        let memo = std::cell::RefCell::new(vec![f32::NAN; if similarity.is_some() { a.len() * b.len() } else { 0 }]);
        Chapter { a, b, ratio, opt, similarity, frequency, memo }
    }

    /// 两段的相似度比「不相干」高出几个标准差；没有向量是 None
    fn z(&self, i: usize, j: usize) -> Option<f32> {
        let (f, base) = self.similarity.as_ref()?;
        let at = i * self.b.len() + j;
        let cached = self.memo.borrow()[at];
        let z = if cached.is_nan() {
            let z = match f(self.a[i].seq, self.b[j].seq) {
                Some(cos) if cos.is_finite() => ((cos - base.mean) / base.deviation).clamp(-2.0, self.opt.similarity_cap),
                _ => f32::INFINITY,
            };
            self.memo.borrow_mut()[at] = z;
            z
        } else {
            cached
        };
        z.is_finite().then_some(z)
    }

    /// 一组的相似度：取最不像的那一对。合并的几段都得和对面那一段像才算数
    /// （按长度加权平均的话，一段没人要的短段落并进旁边那一组几乎不扣分）。这一组里都没有向量是 None（退回只看长度和锚点）
    fn bead_z(&self, a: (usize, usize), b: (usize, usize)) -> Option<f32> {
        let mut least: Option<f32> = None;
        for i in a.0..a.1 {
            for j in b.0..b.1 {
                if let Some(z) = self.z(i, j) {
                    least = Some(least.map_or(z, |l: f32| l.min(z)));
                }
            }
        }
        least
    }

    /// 两边共有的锚点（按稀有度折算：两边各只出现一次的算 1）
    fn shared(&self, a: (usize, usize), b: (usize, usize)) -> f32 {
        let mut total = 0.0;
        let mut seen: Vec<&str> = Vec::new();
        for p in &self.a[a.0..a.1] {
            for t in &p.anchors {
                if seen.contains(&t.as_str()) || !self.b[b.0..b.1].iter().any(|q| q.anchors.binary_search(t).is_ok()) {
                    continue;
                }
                seen.push(t.as_str());
                let (fa, fb) = self.frequency.get(t.as_str()).copied().unwrap_or((1, 1));
                total += 2.0 / (fa + fb) as f32;
            }
        }
        total
    }

    /// 长度差几个标准差
    fn delta(&self, a: (usize, usize), b: (usize, usize)) -> f32 {
        let la: f32 = self.a[a.0..a.1].iter().map(|p| p.size).sum::<f32>() * self.ratio;
        let lb: f32 = self.b[b.0..b.1].iter().map(|p| p.size).sum();
        let mean = (la + lb) / 2.0;
        let variance = self.opt.len_linear.powi(2) * mean + self.opt.len_ratio.powi(2) * mean * mean + 1.0;
        (lb - la) / variance.sqrt()
    }

    fn cost(&self, a: (usize, usize), b: (usize, usize)) -> f64 {
        let (na, nb) = (a.1 - a.0, b.1 - b.0);
        let o = self.opt;
        if na == 0 || nb == 0 {
            let table = self.a[a.0..a.1].iter().chain(&self.b[b.0..b.1]).all(|p| p.table);
            return if table { o.skip_table } else { o.skip } as f64;
        }
        let prior = match na.max(nb) {
            1 => 0.0,
            2 => o.merge,
            _ => o.merge3,
        };
        let d = self.delta(a, b);
        let length = (d * d / 2.0).min(o.len_cap);
        let anchor = o.anchor * self.shared(a, b).min(o.anchor_cap);
        let similarity = o.similarity * self.bead_z(a, b).unwrap_or(0.0);
        (prior + length - anchor - similarity) as f64
    }

    /// 这一组对得稳不稳
    fn weak(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        if a.1 == a.0 || b.1 == b.0 {
            // 落空：表格段不算（另一边多半是图片）
            return !self.a[a.0..a.1].iter().chain(&self.b[b.0..b.1]).all(|p| p.table);
        }
        let far = self.delta(a, b).abs() > 2.5;
        // 合并的那几段各只对着对面的一部分，本来就没有一段对一段那么像（实测一半的合并在 1.8 个标准差以下），门槛放低
        let merged = (a.1 - a.0).max(b.1 - b.0) > 1;
        match self.bead_z(a, b) {
            // 有相似度：相似度说了算；相似度一般、长度又差得远的也不稳
            Some(z) => z < if merged { 0.5 } else { 1.5 } || (far && z < if merged { 1.0 } else { 2.0 }),
            None => far && self.shared(a, b) < 1.0,
        }
    }

    /// 动态规划。各种对法按固定的顺序试，代价一样时取先试的（结果是确定的）
    fn fit(&self) -> Vec<Bead> {
        const SHAPES: [(usize, usize); 7] = [(1, 1), (1, 2), (2, 1), (1, 3), (3, 1), (1, 0), (0, 1)];
        let (n, m) = (self.a.len(), self.b.len());
        let width = m + 1;
        let mut best = vec![f64::INFINITY; (n + 1) * width];
        let mut from = vec![u8::MAX; (n + 1) * width];
        best[0] = 0.0;
        for i in 0..=n {
            for j in 0..=m {
                if i == 0 && j == 0 {
                    continue;
                }
                for (k, (da, db)) in SHAPES.iter().enumerate() {
                    if *da > i || *db > j {
                        continue;
                    }
                    let before = best[(i - da) * width + (j - db)];
                    if !before.is_finite() {
                        continue;
                    }
                    let c = before + self.cost((i - da, i), (j - db, j));
                    if c < best[i * width + j] {
                        best[i * width + j] = c;
                        from[i * width + j] = k as u8;
                    }
                }
            }
        }
        let mut out = Vec::new();
        let (mut i, mut j) = (n, m);
        while i > 0 || j > 0 {
            let (da, db) = SHAPES[from[i * width + j] as usize];
            let (a, b) = ((i - da, i), (j - db, j));
            out.push(Bead { a, b, weak: self.weak(a, b) });
            i -= da;
            j -= db;
        }
        out.reverse();
        out
    }

    /// 对得稳的段落占多少（按长度，两边合计；表格段不算）
    fn confidence(&self, beads: &[Bead]) -> f32 {
        let weigh = |bead: &Bead| -> f32 { self.a[bead.a.0..bead.a.1].iter().map(|p| p.size * self.ratio).sum::<f32>() + self.b[bead.b.0..bead.b.1].iter().map(|p| p.size).sum::<f32>() };
        let total: f32 = beads.iter().map(weigh).sum();
        let weak: f32 = beads.iter().filter(|b| b.weak).map(weigh).sum();
        if total <= 0.0 { 0.0 } else { ((1.0 - weak / total) * 100.0).round() / 100.0 }
    }
}

/// 全书的长度比（译本 / 原著）：所有两边都有正文的单元合起来算
fn overall_ratio(units: &[(Vec<Piece>, Vec<Piece>)]) -> f32 {
    let (mut a, mut b) = (0.0f32, 0.0f32);
    for (x, y) in units.iter().filter(|(x, y)| !x.is_empty() && !y.is_empty()) {
        a += x.iter().map(|p| p.size).sum::<f32>();
        b += y.iter().map(|p| p.size).sum::<f32>();
    }
    if a > 0.0 && b > 0.0 { b / a } else { 1.0 }
}

/// 确实一段对一段的那些段落的长度比：段数相同的章，加上第一遍里对得稳的 1-1。太少（不到 3 对）就不改
fn refined_ratio(units: &[(Vec<Piece>, Vec<Piece>)], fits: &[Option<Vec<Bead>>]) -> Option<f32> {
    let (mut a, mut b, mut n) = (0.0f32, 0.0f32, 0usize);
    for ((x, y), fit) in units.iter().zip(fits) {
        match fit {
            Some(beads) => {
                for bead in beads.iter().filter(|k| !k.weak && k.a.1 - k.a.0 == 1 && k.b.1 - k.b.0 == 1) {
                    a += x[bead.a.0].size;
                    b += y[bead.b.0].size;
                    n += 1;
                }
            }
            None if !x.is_empty() && x.len() == y.len() => {
                a += x.iter().map(|p| p.size).sum::<f32>();
                b += y.iter().map(|p| p.size).sum::<f32>();
                n += x.len();
            }
            None => {}
        }
    }
    (n >= 3 && a > 0.0 && b > 0.0).then(|| b / a)
}

/// 「不是同一段」的两段有多像：每章里取位置差三段的两段（全书合起来，最多两千对）。
/// 要拿近处的段落当对照：同一章里挨着的段落讲的是同一件事，本来就比隔得远的像；对齐要分清的正是它们
fn baseline(units: &[(Vec<Piece>, Vec<Piece>)], similarity: Similarity) -> Baseline {
    const APART: usize = 3;
    let mut samples: Vec<f32> = Vec::new();
    let total: usize = units.iter().map(|(a, _)| a.len()).sum();
    let stride = (total / 1000).max(1);
    let mut k = 0usize;
    for (a, b) in units {
        if a.len() < 2 * APART || b.len() < 2 * APART {
            continue;
        }
        for (i, p) in a.iter().enumerate() {
            k += 1;
            if !k.is_multiple_of(stride) {
                continue;
            }
            let here = i * b.len() / a.len();
            for j in [here.checked_sub(APART), Some(here + APART).filter(|j| *j < b.len())].into_iter().flatten() {
                if let Some(cos) = similarity(p.seq, b[j].seq).filter(|c| c.is_finite()) {
                    samples.push(cos);
                }
            }
        }
    }
    if samples.len() < 8 {
        // 书太短，取不到样：按实测的多语种向量模型（Qwen3-Embedding）估
        return Baseline { mean: 0.55, deviation: 0.1 };
    }
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    let variance = samples.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / samples.len() as f32;
    Baseline { mean, deviation: variance.sqrt().max(0.02) }
}

pub fn align(original: &Text, other: &Text) -> Alignment {
    align_with(original, other, None, &Options::default())
}

/// `similarity` 可以没有（只用长度和锚点）。同样的输入结果是确定的
pub fn align_with(original: &Text, other: &Text, similarity: Option<Similarity>, opt: &Options) -> Alignment {
    let other_chapters = other.chapters();
    let matched: Vec<(&crate::text::Unit, Option<&crate::text::Unit>)> = original
        .chapters()
        .into_iter()
        .filter_map(|ch| {
            let id = identity(original, ch.index)?;
            Some((ch, other_chapters.iter().copied().find(|u| identity(other, u.index).as_deref() == Some(id.as_str()))))
        })
        .collect();
    let mut bodies: Vec<(Vec<Piece>, Vec<Piece>)> = matched.iter().map(|(ch, found)| (pieces(original, ch.index), found.map(|u| pieces(other, u.index)).unwrap_or_default())).collect();
    // 译本里保留的拉丁字母词，在原著那边也当锚点找出来
    for ((ch, _), (a, b)) in matched.iter().zip(bodies.iter_mut()) {
        let mut wanted: HashMap<String, usize> = HashMap::new();
        for t in b.iter().flat_map(|p| &p.anchors).filter(|t| t.chars().all(|c| c.is_ascii_alphabetic())) {
            *wanted.entry(t.clone()).or_default() += 1;
        }
        if wanted.is_empty() {
            continue;
        }
        for (piece, p) in a.iter_mut().zip(original.body_of(ch.index)) {
            piece.anchors.extend(latin_words(&p.text, &wanted));
            piece.anchors.sort();
            piece.anchors.dedup();
        }
    }
    let similarity = similarity.map(|f| (f, baseline(&bodies, f)));
    // 段数不同的章要算；段数相同的照 v0.4 按顺序对
    let exact = |a: &[Piece], b: &[Piece]| !a.is_empty() && a.len() == b.len();
    let mut chapters: Vec<Option<Chapter>> = bodies.iter().map(|(a, b)| (!a.is_empty() && !b.is_empty()).then(|| Chapter::new(a, b, overall_ratio(&bodies), opt, similarity))).collect();
    let mut fits: Vec<Option<Vec<Bead>>> = chapters.iter().map(|c| c.as_ref().filter(|c| !exact(c.a, c.b)).map(Chapter::fit)).collect();
    // 第二遍：全书的长度比会被多出来、缺掉的段落带偏。改用确实一段对一段的那些（段数相同的章、第一遍里对得稳的 1-1）再算一遍
    if let Some(ratio) = refined_ratio(&bodies, &fits) {
        for (c, f) in chapters.iter_mut().zip(fits.iter_mut()) {
            if let Some(c) = c {
                c.ratio = ratio;
                if f.is_some() {
                    *f = Some(c.fit());
                }
            }
        }
    }
    let mut out = Alignment::default();
    for ((((ch, found), (a, b)), chapter), beads) in matched.iter().zip(&bodies).zip(&chapters).zip(&fits) {
        let mut unit = UnitAlignment {
            key: ch.key.clone(),
            label: ch.label.clone(),
            other_label: found.map(|u| u.label.clone()).unwrap_or_default(),
            unit: ch.index,
            other_unit: found.map(|u| u.index),
            count: a.len(),
            other_count: b.len(),
            aligned: false,
            method: Method::None,
            confidence: 0.0,
            low: false,
            merged: 0,
            unmatched: 0,
        };
        if found.is_some() && !a.is_empty() && a.len() == b.len() {
            out.pairs.extend(a.iter().map(|p| p.seq).zip(b.iter().map(|p| p.seq)));
            unit.aligned = true;
            unit.method = Method::Exact;
            // 照样按顺序对，只是多看一眼：长度（或相似度）对不上的段落多了，多半是段数碰巧相同、中间其实错开了，标出来
            let in_order: Vec<Bead> = (0..a.len()).map(|i| Bead { a: (i, i + 1), b: (i, i + 1), weak: chapter.as_ref().is_some_and(|c| c.weak((i, i + 1), (i, i + 1))) }).collect();
            unit.confidence = chapter.as_ref().map_or(1.0, |c| c.confidence(&in_order));
            unit.low = unit.confidence < opt.low_below;
        } else if let (Some(chapter), Some(beads)) = (chapter, beads) {
            let confidence = chapter.confidence(beads);
            // 差得太远（多半不是同一章，或者有一边缺了一大块）：不硬对
            if confidence >= opt.none_below {
                for bead in beads {
                    for p in &a[bead.a.0..bead.a.1] {
                        for q in &b[bead.b.0..bead.b.1] {
                            out.pairs.push((p.seq, q.seq));
                        }
                    }
                }
                unit.aligned = true;
                unit.method = Method::Fitted;
                unit.confidence = confidence;
                unit.low = confidence < opt.low_below;
                unit.merged = beads.iter().filter(|x| (x.a.1 - x.a.0).max(x.b.1 - x.b.0) > 1 && x.a.1 > x.a.0 && x.b.1 > x.b.0).count();
                unit.unmatched = beads.iter().filter(|x| x.a.1 == x.a.0 || x.b.1 == x.b.0).map(|x| (x.a.1 - x.a.0) + (x.b.1 - x.b.0)).sum();
            }
        }
        out.units.push(unit);
    }
    out
}

/// 对齐的中文段（`seq` 是原著的正文段）：一段可能对着几段；不是正文或没对上的是空的
pub fn counterpart<'a>(alignment: &Alignment, original: &Text, other: &'a Text, seq: usize) -> Vec<&'a Paragraph> {
    if original.paragraphs.get(seq).is_none_or(|p| p.role != Role::Body) {
        return Vec::new();
    }
    alignment.other_of(seq).into_iter().filter_map(|s| other.paragraphs.get(s)).collect()
}

/// 每种对法各有几组：`"1-1"`、`"2-1"`、`"1-0"`……（验收和调权重时看）
pub fn shapes(alignment: &Alignment) -> std::collections::BTreeMap<String, usize> {
    let mut out = std::collections::BTreeMap::new();
    let mut seen: Vec<usize> = Vec::new();
    for (a, _) in &alignment.pairs {
        if seen.last() == Some(a) || seen.contains(a) {
            continue;
        }
        let (originals, others) = alignment.group_of(*a);
        seen.extend(&originals);
        *out.entry(format!("{}-{}", originals.len(), others.len())).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Unit;

    /// 合成的一本书：每章若干段正文（篇.章.段）
    fn book(language: &str, chapters: &[Vec<String>]) -> Text {
        let mut t = Text { language: language.into(), ..Default::default() };
        for (c, paragraphs) in chapters.iter().enumerate() {
            let key = format!("I.{}", crate::text::roman(c as u32 + 1).to_lowercase());
            t.units.push(Unit { index: c, kind: UnitKind::Chapter, label: format!("Chapter {}", c + 1), key: key.clone(), depth: 1, parent: None, book: Some(1), chapter: Some(c as u32 + 1) });
            for (i, text) in paragraphs.iter().enumerate() {
                let seq = t.paragraphs.len();
                t.paragraphs.push(Paragraph { seq, unit: c, role: Role::Body, number: Some(i as u32 + 1), key: format!("{key}.{}", i + 1), attached: None, text: text.clone(), page: None, breaks: Vec::new(), marker: None, noterefs: Vec::new() });
            }
        }
        t
    }

    /// 英文段：n 个词（一个词 6 个字母，折算 1.8 个汉字）
    fn en(n: usize) -> String {
        vec!["labour"; n].join(" ")
    }

    /// 和 n 个英文词一样长的中文段
    fn zh(n: usize) -> String {
        "分工".repeat(n * 9 / 10)
    }

    fn keys(a: &Alignment, original: &Text, other: &Text) -> Vec<(String, String)> {
        a.pairs.iter().map(|(x, y)| (original.paragraphs[*x].key.clone(), other.paragraphs[*y].key.clone())).collect()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn merged_split_and_inserted_paragraphs_are_fitted() {
        // 英文 7 段；中文把第 2、3 段合成一段，把第 4 段拆成两段，在第 5 段后面多出一段英文没有的
        let english = book("en", &[vec![en(100), en(40), en(160), en(60), en(30), en(120), en(80)]]);
        let chinese = book("zh", &[vec![zh(100), zh(200), zh(35), zh(25), zh(30), zh(90), zh(120), zh(80)]]);
        let a = align(&english, &chinese);
        let u = &a.units[0];
        assert_eq!((u.count, u.other_count, u.aligned, u.method), (7, 8, true, Method::Fitted));
        assert_eq!(
            keys(&a, &english, &chinese),
            [pair("I.i.1", "I.i.1"), pair("I.i.2", "I.i.2"), pair("I.i.3", "I.i.2"), pair("I.i.4", "I.i.3"), pair("I.i.4", "I.i.4"), pair("I.i.5", "I.i.5"), pair("I.i.6", "I.i.7"), pair("I.i.7", "I.i.8")]
        );
        assert_eq!((u.merged, u.unmatched), (2, 1));
        // 落空的那一段算不稳：置信度不到 1，但其余都对得上，不算低
        assert!(u.confidence < 1.0 && u.confidence > 0.85 && !u.low, "{}", u.confidence);
        // 一对多、多对一
        let seq = |t: &Text, k: &str| t.paragraph(k).unwrap().seq;
        assert_eq!(a.other_of(seq(&english, "I.i.4")), [seq(&chinese, "I.i.3"), seq(&chinese, "I.i.4")]);
        assert_eq!(a.original_of(seq(&chinese, "I.i.2")), [seq(&english, "I.i.2"), seq(&english, "I.i.3")]);
        assert_eq!(a.group_of(seq(&english, "I.i.3")), (vec![seq(&english, "I.i.2"), seq(&english, "I.i.3")], vec![seq(&chinese, "I.i.2")]));
        assert!(a.original_of(seq(&chinese, "I.i.6")).is_empty());
        let group = counterpart(&a, &english, &chinese, seq(&english, "I.i.4"));
        assert_eq!(group.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["I.i.3", "I.i.4"]);
        assert_eq!(shapes(&a).into_iter().collect::<Vec<_>>(), [("1-1".to_string(), 4), ("1-2".to_string(), 1), ("2-1".to_string(), 1)]);
        // 结果是确定的
        assert_eq!(a, align(&english, &chinese));
    }

    #[test]
    fn equal_counts_pair_in_order_whatever_the_lengths() {
        // 段数相同：和 v0.4 一样按顺序一段对一段，不看长度
        let english = book("en", &[vec![en(10), en(200), en(10)], vec![en(50), en(50)]]);
        let chinese = book("zh", &[vec![zh(200), zh(10), zh(10)], vec![zh(50), zh(50)]]);
        let a = align(&english, &chinese);
        assert_eq!(a.pairs, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]);
        assert!(a.units.iter().all(|u| u.method == Method::Exact && u.aligned && u.merged == 0 && u.unmatched == 0));
        assert!(a.mismatched().is_empty());
        // 对法不变，只是标出来：第一章长度明显对不上（多半中间错开了），第二章没问题
        assert!(a.units[0].low && a.units[0].confidence < 0.5, "{}", a.units[0].confidence);
        assert_eq!((a.units[1].confidence, a.units[1].low), (1.0, false));
        assert_eq!(a.low().len(), 1);
    }

    #[test]
    fn a_moved_paragraph_is_left_unmatched_not_forced() {
        // 改坏：中文把第 2 段挪到了最后。挪走的那段两边都落空，别的段照样一段对一段，不跟着错位
        let english = book("en", &[vec![en(100), en(150), en(160), en(60), en(220), en(120)]]);
        let chinese = book("zh", &[vec![zh(100), zh(160), zh(60), zh(220), zh(120), zh(150)]]);
        let a = align(&english, &chinese);
        // 段数相同：照 v0.4 按顺序对（这种错位要段数不同才查得出来）
        assert_eq!(a.units[0].method, Method::Exact);
        // 最后一段又拆成了两段，段数不同：算法来对
        let chinese = book("zh", &[vec![zh(100), zh(160), zh(60), zh(220), zh(70), zh(50), zh(150)]]);
        let a = align(&english, &chinese);
        let u = &a.units[0];
        assert_eq!(u.method, Method::Fitted);
        assert_eq!(
            keys(&a, &english, &chinese),
            [pair("I.i.1", "I.i.1"), pair("I.i.3", "I.i.2"), pair("I.i.4", "I.i.3"), pair("I.i.5", "I.i.4"), pair("I.i.6", "I.i.5"), pair("I.i.6", "I.i.6")]
        );
        // 挪走的那段两边各落空一次；置信度低，页面上会提示
        assert!(u.unmatched == 2 && u.merged == 1 && u.confidence < 0.85 && u.low, "{} {}", u.unmatched, u.confidence);
    }

    #[test]
    fn chapters_too_far_apart_are_not_aligned() {
        // 第一章定下长度比；第二章两边完全对不上（译本只剩几行）：不硬对
        let english = book("en", &[vec![en(100), en(80), en(120)], vec![en(300), en(250), en(400), en(350)]]);
        let chinese = book("zh", &[vec![zh(100), zh(80), zh(120)], vec![zh(20), zh(10), zh(15), zh(12), zh(18)]]);
        let a = align(&english, &chinese);
        assert_eq!(a.units[0].method, Method::Exact);
        let u = &a.units[1];
        assert_eq!((u.aligned, u.method, u.confidence, u.low), (false, Method::None, 0.0, false));
        assert_eq!(a.pairs.len(), 3);
        assert_eq!(a.mismatched().len(), 1);
        // 译本里没有的章
        let short = book("zh", &[vec![zh(100), zh(80), zh(120)]]);
        let a = align(&english, &short);
        assert_eq!((a.units[1].method, a.units[1].other_unit, a.units[1].other_count), (Method::None, None, 0));
    }

    #[test]
    fn anchors_and_similarity_settle_what_length_cannot() {
        // 第一章段数相同（定下长度比）；第二章两段对三段，长度都一样：光看长度分不出多出来的是哪一段
        let first_chapter = (vec![en(100), en(80), en(120)], vec![zh(100), zh(80), zh(120)]);
        let english = book("en", &[first_chapter.0.clone(), vec![format!("{} 1776", en(50)), format!("{} 1759", en(50))]]);
        let chinese = book("zh", &[first_chapter.1.clone(), vec![format!("{}1776年", zh(50)), zh(50), format!("{}1759年", zh(50))]]);
        // 锚点：年份对上的那两段
        let a = align(&english, &chinese);
        assert_eq!(a.pairs[3..], [(3, 3), (4, 5)]);
        // 相似度（调用方给）：没有锚点时听它的
        let english = book("en", &[first_chapter.0, vec![en(50), en(50)]]);
        let chinese = book("zh", &[first_chapter.1, vec![zh(50), zh(50), zh(50)]]);
        let first = |x: usize, y: usize| Some(if [(3, 3), (4, 4)].contains(&(x, y)) { 0.85 } else { 0.4 });
        assert_eq!(align_with(&english, &chinese, Some(&first), &Options::default()).pairs[3..], [(3, 3), (4, 4)]);
        let last = |x: usize, y: usize| Some(if [(3, 4), (4, 5)].contains(&(x, y)) { 0.85 } else { 0.4 });
        assert_eq!(align_with(&english, &chinese, Some(&last), &Options::default()).pairs[3..], [(3, 4), (4, 5)]);
        // 没有向量的段（返回 None）不影响别的
        let none = |_: usize, _: usize| None;
        assert_eq!(align_with(&english, &chinese, Some(&none), &Options::default()), align(&english, &chinese));
    }

    #[test]
    fn tables_may_go_unmatched_for_free() {
        // 英文的表格在中文版里是图片：表格段落空，不拖累别的段，也不算不稳
        let table = "Years\tPrice\n1202\t12\n1205\t13".to_string();
        let english = book("en", &[vec![en(100), table, en(60), en(150)]]);
        let chinese = book("zh", &[vec![zh(100), zh(60), zh(150)]]);
        let a = align(&english, &chinese);
        assert_eq!(a.pairs, [(0, 0), (2, 1), (3, 2)]);
        let u = &a.units[0];
        assert_eq!((u.method, u.confidence, u.low, u.unmatched), (Method::Fitted, 1.0, false, 1));
        assert!(is_table("a\tb") && !is_table("a b"));
    }

    #[test]
    fn anchors_are_numbers_and_kept_latin_words() {
        assert_eq!(anchors("In 1776, about 1,000 pins and 3 men"), ["1000", "1776"]);
        assert_eq!(anchors("1776 年，斯密（Adam Smith）说到 20 先令"), ["1776", "20", "adam", "smith"]);
        // 全是拉丁字母的段落不取词（否则每个词都是锚点）
        assert_eq!(anchors("Adam Smith wrote it in 1776"), ["1776"]);
        assert!((size("分工 labour") - (2.0 + 6.0 * LATIN_WEIGHT)).abs() < 1e-4);
    }

    #[test]
    fn old_json_still_loads() {
        // v0.4 存的对齐没有 method、confidence：按 aligned 补上
        let old = r#"{"units":[{"key":"I.i","label":"CHAPTER I","otherLabel":"第一章","unit":1,"otherUnit":2,"count":3,"otherCount":3,"aligned":true},{"key":"I.ii","label":"CHAPTER II","otherLabel":"第二章","unit":2,"otherUnit":3,"count":3,"otherCount":4,"aligned":false}],"pairs":[[1,2]]}"#;
        let a: Alignment = serde_json::from_str(old).unwrap();
        assert_eq!((a.units[0].method, a.units[0].confidence, a.units[0].low), (Method::Exact, 1.0, false));
        assert_eq!((a.units[1].method, a.units[1].confidence), (Method::None, 0.0));
        // 新的：camelCase，来回一样
        let english = book("en", &[vec![en(100), en(40), en(160)]]);
        let chinese = book("zh", &[vec![zh(100), zh(200)]]);
        let a = align(&english, &chinese);
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains(r#""method":"fitted""#) && json.contains(r#""otherCount":2"#) && json.contains(r#""confidence":"#));
        assert_eq!(serde_json::from_str::<Alignment>(&json).unwrap(), a);
    }
}
