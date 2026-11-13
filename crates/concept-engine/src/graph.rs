//! 关系图（`decisions/0024`「关系图」）：节点是卡片，边是关系。布局在这里算：确定（同样的输入永远同样的图）、可测、不引依赖。
//!
//! 算法（分层，Sugiyama 的简化版）：
//! 1. 边按 from → to 的方向；按卡片编号顺序做深度优先，回头的边（成环的）不参与分层；
//! 2. 层 = 从源头起的最长路径；
//! 3. 每层内按上一层邻居位置的平均数排（上下各扫两遍），一样的按卡片编号；
//! 4. 从左往右排层，层内从上往下；边是三次贝塞尔曲线，同层或往回的边从上方绕。
//!
//! App 页面和 HTML 导出画同一份 SVG：节点 `data-card="K1"`，边 `data-relation="R1"`，点了由页面自己处理。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::model::{CardKind, Run};

pub const NODE_W: f64 = 188.0;
pub const GAP_X: f64 = 84.0;
pub const GAP_Y: f64 = 26.0;
pub const PAD: f64 = 24.0;
const LINE_H: f64 = 17.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub card: String,
    pub kind: CardKind,
    pub lines: Vec<String>,
    pub layer: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub relation: String,
    pub from: String,
    pub to: String,
    pub label: String,
    /// SVG 的 path d
    pub path: String,
    pub label_x: f64,
    pub label_y: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub width: f64,
    pub height: f64,
}

/// 把标题折成几行：中文约 11 个字一行，西文约 22 个字母，最多 3 行
fn wrap(title: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut width = 0.0;
    for word in title.split_inclusive(' ') {
        for c in word.chars() {
            let cw = if c.is_ascii() { 0.5 } else { 1.0 };
            if width + cw > 11.0 && !cur.trim().is_empty() {
                lines.push(cur.trim().to_string());
                cur.clear();
                width = 0.0;
            }
            cur.push(c);
            width += cw;
        }
    }
    if !cur.trim().is_empty() {
        lines.push(cur.trim().to_string());
    }
    if lines.len() > 3 {
        lines.truncate(3);
        if let Some(l) = lines.last_mut() {
            l.push('…');
        }
    }
    if lines.is_empty() {
        lines.push("（没写标题）".into());
    }
    lines
}

fn number(id: &str) -> u32 {
    id.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(u32::MAX)
}

/// 布局：有关系的卡片才画
pub fn layout(run: &Run) -> Graph {
    let mut ids: BTreeSet<(u32, String)> = BTreeSet::new();
    for r in &run.relations {
        if run.card(&r.from).is_some() && run.card(&r.to).is_some() {
            ids.insert((number(&r.from), r.from.clone()));
            ids.insert((number(&r.to), r.to.clone()));
        }
    }
    let order: Vec<String> = ids.into_iter().map(|(_, id)| id).collect();
    if order.is_empty() {
        return Graph::default();
    }
    let index: HashMap<&str, usize> = order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    let edges: Vec<(usize, usize, usize)> = run.relations.iter().enumerate().filter_map(|(ri, r)| Some((index.get(r.from.as_str()).copied()?, index.get(r.to.as_str()).copied()?, ri))).filter(|(a, b, _)| a != b).collect();
    // 1. 深度优先找回头的边
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); order.len()];
    for (k, (a, _, _)) in edges.iter().enumerate() {
        out_edges[*a].push(k);
    }
    let mut state = vec![0u8; order.len()]; // 0 没到过，1 正在，2 走完
    let mut back = vec![false; edges.len()];
    fn dfs(v: usize, edges: &[(usize, usize, usize)], out_edges: &[Vec<usize>], state: &mut [u8], back: &mut [bool]) {
        state[v] = 1;
        for &k in &out_edges[v] {
            let w = edges[k].1;
            match state[w] {
                0 => dfs(w, edges, out_edges, state, back),
                1 => back[k] = true,
                _ => {}
            }
        }
        state[v] = 2;
    }
    for v in 0..order.len() {
        if state[v] == 0 {
            dfs(v, &edges, &out_edges, &mut state, &mut back);
        }
    }
    // 2. 最长路径分层
    let mut layer = vec![0usize; order.len()];
    for _ in 0..order.len() {
        let mut changed = false;
        for (k, (a, b, _)) in edges.iter().enumerate() {
            if !back[k] && layer[*b] < layer[*a] + 1 {
                layer[*b] = layer[*a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // 3. 层内排序
    let layers = layer.iter().max().copied().unwrap_or(0) + 1;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); layers];
    for v in 0..order.len() {
        rows[layer[v]].push(v);
    }
    let mut pos: Vec<f64> = vec![0.0; order.len()];
    let set_pos = |rows: &Vec<Vec<usize>>, pos: &mut Vec<f64>| {
        for row in rows {
            for (i, v) in row.iter().enumerate() {
                pos[*v] = i as f64;
            }
        }
    };
    set_pos(&rows, &mut pos);
    for sweep in 0..4 {
        let down = sweep % 2 == 0;
        let range: Vec<usize> = if down { (1..layers).collect() } else { (0..layers.saturating_sub(1)).rev().collect() };
        for l in range {
            let mut keyed: Vec<(f64, usize)> = rows[l]
                .iter()
                .map(|v| {
                    let ns: Vec<f64> = edges.iter().filter_map(|(a, b, _)| if down && b == v && layer[*a] < l { Some(pos[*a]) } else if !down && a == v && layer[*b] > l { Some(pos[*b]) } else { None }).collect();
                    let bary = if ns.is_empty() { pos[*v] } else { ns.iter().sum::<f64>() / ns.len() as f64 };
                    (bary, *v)
                })
                .collect();
            keyed.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)));
            rows[l] = keyed.into_iter().map(|(_, v)| v).collect();
            set_pos(&rows, &mut pos);
        }
    }
    // 4. 坐标
    let mut nodes: Vec<Option<Node>> = vec![None; order.len()];
    let mut height: f64 = 0.0;
    for (l, row) in rows.iter().enumerate() {
        let mut y = PAD + 18.0;
        for v in row {
            let card = run.card(&order[*v]).expect("有这张卡");
            let lines = wrap(&card.title);
            let h = 30.0 + lines.len() as f64 * LINE_H;
            nodes[*v] = Some(Node { card: card.id.clone(), kind: card.kind, lines, layer: l, x: PAD + l as f64 * (NODE_W + GAP_X), y, w: NODE_W, h });
            y += h + GAP_Y;
        }
        height = height.max(y - GAP_Y + PAD);
    }
    let nodes: Vec<Node> = nodes.into_iter().map(|n| n.expect("每个节点都排了")).collect();
    let width = PAD * 2.0 + layers as f64 * NODE_W + (layers - 1) as f64 * GAP_X;
    let mut out_edges = Vec::new();
    for (a, b, ri) in &edges {
        let (na, nb) = (&nodes[*a], &nodes[*b]);
        let r = &run.relations[*ri];
        let (path, lx, ly) = if nb.layer > na.layer {
            let (x1, y1, x2, y2) = (na.x + na.w, na.y + na.h / 2.0, nb.x, nb.y + nb.h / 2.0);
            let dx = (x2 - x1) / 2.0;
            (format!("M{x1:.1},{y1:.1} C{:.1},{y1:.1} {:.1},{y2:.1} {x2:.1},{y2:.1}", x1 + dx, x2 - dx), (x1 + x2) / 2.0, (y1 + y2) / 2.0 - 4.0)
        } else {
            // 同层或往回：从两个节点的上沿绕过去
            let (x1, y1, x2, y2) = (na.x + na.w / 2.0, na.y, nb.x + nb.w / 2.0, nb.y);
            let top = y1.min(y2) - 16.0 - (x1 - x2).abs() / 8.0;
            (format!("M{x1:.1},{y1:.1} C{x1:.1},{top:.1} {x2:.1},{top:.1} {x2:.1},{y2:.1}"), (x1 + x2) / 2.0, top + 6.0)
        };
        out_edges.push(Edge { relation: r.id.clone(), from: r.from.clone(), to: r.to.clone(), label: r.kind.label().into(), path, label_x: lx, label_y: ly });
    }
    let min_y = out_edges.iter().map(|e| e.label_y - 12.0).fold(0.0_f64, f64::min);
    let mut g = Graph { nodes, edges: out_edges, width, height };
    if min_y < 0.0 {
        // 绕到上面的边超出了画布：整体往下挪
        let shift = -min_y + 4.0;
        for n in &mut g.nodes {
            n.y += shift;
        }
        g.height += shift;
        let edges = std::mem::take(&mut g.edges);
        g.edges = edges
            .into_iter()
            .map(|mut e| {
                e.path = shift_path(&e.path, shift);
                e.label_y += shift;
                e
            })
            .collect();
    }
    g
}

fn shift_path(d: &str, dy: f64) -> String {
    d.split(' ')
        .map(|part| {
            let (cmd, rest) = part.split_at(if part.starts_with(['M', 'C']) { 1 } else { 0 });
            match rest.split_once(',') {
                Some((x, y)) => format!("{cmd}{x},{:.1}", y.parse::<f64>().unwrap_or(0.0) + dy),
                None => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// SVG：样式写在 `<style>` 里（浅色、深色两套），不引外部资源
pub fn svg(g: &Graph) -> String {
    if g.nodes.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"pith-graph\" viewBox=\"0 0 {w:.0} {h:.0}\" width=\"{w:.0}\" height=\"{h:.0}\" role=\"img\" aria-label=\"关系图\">",
        w = g.width,
        h = g.height
    );
    s.push_str("<style>.pith-graph{font-family:-apple-system,'PingFang SC','Noto Sans CJK SC',sans-serif;font-size:12px}.pith-graph .edge path{fill:none;stroke:#8a8f98;stroke-width:1.4}.pith-graph .edge text{fill:#5b6170;font-size:11px;text-anchor:middle;paint-order:stroke;stroke:#fff;stroke-width:3px}.pith-graph .node{cursor:pointer}.pith-graph .node rect{stroke-width:1.2}.pith-graph .node text{fill:#1f2328}.pith-graph .node .kind{fill:#5b6170;font-size:10px}.pith-graph .node.concept rect{fill:#e8f1fd;stroke:#4a7fd0}.pith-graph .node.argument rect{fill:#eaf6ec;stroke:#3f9654}.pith-graph .node.comment rect{fill:#fdf2e3;stroke:#c98a2c}.pith-graph .node.counterfactual rect{fill:#f4ecfb;stroke:#8d5cc4;stroke-dasharray:5 3}.pith-graph .node.note rect{fill:#f2f3f5;stroke:#8a8f98}.pith-graph .node.active rect{stroke-width:3}.pith-graph #pith-arrow path{fill:#8a8f98}@media (prefers-color-scheme:dark){.pith-graph .node text{fill:#e6e8eb}.pith-graph .node .kind,.pith-graph .edge text{fill:#a9afb8}.pith-graph .edge text{stroke:#1d2026}.pith-graph .node.concept rect{fill:#1d2b40}.pith-graph .node.argument rect{fill:#1c3123}.pith-graph .node.comment rect{fill:#3a2c17}.pith-graph .node.counterfactual rect{fill:#2e2240}.pith-graph .node.note rect{fill:#26292f}}</style>");
    s.push_str("<defs><marker id=\"pith-arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\"/></marker></defs>");
    for e in &g.edges {
        s.push_str(&format!(
            "<g class=\"edge\" data-relation=\"{}\"><path d=\"{}\" marker-end=\"url(#pith-arrow)\"/><text x=\"{:.1}\" y=\"{:.1}\">{}</text></g>",
            esc(&e.relation),
            e.path,
            e.label_x,
            e.label_y,
            esc(&e.label)
        ));
    }
    for n in &g.nodes {
        s.push_str(&format!("<g class=\"node {}\" data-card=\"{}\" tabindex=\"0\"><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"7\"/>", n.kind.key(), esc(&n.card), n.x, n.y, n.w, n.h));
        s.push_str(&format!("<text class=\"kind\" x=\"{:.1}\" y=\"{:.1}\">{} · {}</text>", n.x + 10.0, n.y + 16.0, esc(&n.card), n.kind.label()));
        for (i, l) in n.lines.iter().enumerate() {
            s.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\">{}</text>", n.x + 10.0, n.y + 34.0 + i as f64 * LINE_H, esc(l)));
        }
        s.push_str("</g>");
    }
    s.push_str("</svg>");
    s
}

/// 每种卡片几张（关系图下面的图例用）
pub fn kinds(g: &Graph) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for n in &g.nodes {
        *m.entry(n.kind.label()).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Card, Category, Relation, RelationKind, Sentence};

    fn card(id: &str, kind: CardKind) -> Card {
        Card { id: id.into(), kind, title: format!("卡片 {id} 的标题比较长一些要折行"), sentences: Vec::new(), step: "x".into(), about: Vec::new() }
    }

    fn rel(id: &str, from: &str, to: &str, kind: RelationKind) -> Relation {
        Relation { id: id.into(), from: from.into(), to: to.into(), kind, note: Sentence { category: Category::Explanation, text: "x".into(), paragraph: None, quote: None, zh: None, source: None, check: Default::default() }, step: "x".into() }
    }

    #[test]
    fn layered_layout_is_deterministic() {
        let run = Run {
            cards: vec![card("K1", CardKind::Concept), card("K2", CardKind::Concept), card("K3", CardKind::Argument), card("K4", CardKind::Comment), card("K9", CardKind::Note)],
            relations: vec![rel("R1", "K1", "K3", RelationKind::Premise), rel("R2", "K2", "K3", RelationKind::Premise), rel("R3", "K4", "K3", RelationKind::Opposes), rel("R4", "K3", "K1", RelationKind::Example)],
            ..Default::default()
        };
        let g = layout(&run);
        // K9 没有关系，不画
        assert_eq!(g.nodes.len(), 4);
        let layer = |id: &str| g.nodes.iter().find(|n| n.card == id).unwrap().layer;
        assert_eq!((layer("K1"), layer("K2"), layer("K4"), layer("K3")), (0, 0, 0, 1));
        // 回头的边（K3 → K1）从上方绕
        let back = g.edges.iter().find(|e| e.relation == "R4").unwrap();
        assert!(back.path.starts_with('M'));
        assert!(g.edges.iter().all(|e| e.label_y >= 0.0));
        // 同样的输入同样的图
        assert_eq!(g, layout(&run));
        let s = svg(&g);
        assert!(s.contains("data-card=\"K3\"") && s.contains("data-relation=\"R4\"") && s.contains(">对立<"));
        assert!(!s.contains("http://") || s.matches("http://").count() == 1, "只有 xmlns，不引外部资源");
        assert!(svg(&layout(&Run::default())).is_empty());
    }

    #[test]
    fn titles_wrap() {
        assert_eq!(wrap("division of labour / 分工"), vec!["division of labour /", "分工"]);
        assert_eq!(wrap("一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五"), vec!["一二三四五六七八九十一", "二三四五六七八九十一二", "三四五六七八九十一二三…"]);
    }
}
