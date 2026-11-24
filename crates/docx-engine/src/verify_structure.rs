//! 结构类排版的独立核对（v0.2 W4）：与 `structure`（quick-xml 的可编辑树）分开，用 roxmltree 重新读排版后的文档。
//!
//! 两件事：
//! 1. 保真（`compare`）：结构类排版允许的改动只有——新的页眉页脚部件（及其关系、内容类型登记）、
//!    分界处多出来的分节符。别的照旧一个字、一个结构都不许变。允许多少由这里按规范和计划从排版前的文档自己推，
//!    不看排版器的报告。
//! 2. 生效（`check`）：每一类结构逐项核对——分节、页眉内容与横线、页码形式与起始、奇数页起、三线表的每条线、
//!    脚注编号、紧跟上一级标题的段前。

use std::collections::{BTreeMap, BTreeSet};

use roxmltree::{Document, Node, ParsingOptions};

use crate::outline;
use crate::package::{relationships, Package};
use crate::plan::{Anchor, Plan};
use crate::spec::{page_format_ooxml, HeaderLine, Spec, CHAPTER, RULE_HEAVY, RULE_LIGHT};
use crate::structure::{self, Zone, CHAPTER_FIELD, FOOTER_PART, HEADER_PART};
use crate::verify::{self, Check, Problem};
use crate::{Error, Result, NS_R, NS_W};

const DOC: &str = "word/document.xml";
const RELS: &str = "word/_rels/document.xml.rels";
const TYPES: &str = "[Content_Types].xml";

fn parse(s: &str) -> Result<Document<'_>> {
    Document::parse_with_options(s, ParsingOptions { allow_dtd: true, ..ParsingOptions::default() }).map_err(|e| Error::Xml(e.to_string()))
}

fn is(n: &Node, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(NS_W)
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| is(c, name))
}

fn wattr<'a>(n: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.attribute((NS_W, name))
}

/// 新部件的名字：`word/header_pith<N>.xml` / `word/footer_pith<N>.xml`
fn new_part_kind(name: &str) -> Option<&'static str> {
    let file = name.strip_prefix("word/")?.strip_suffix(".xml")?;
    for (prefix, kind) in [(HEADER_PART, "header"), (FOOTER_PART, "footer")] {
        if let Some(n) = file.strip_prefix(prefix)
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
        {
            return Some(kind);
        }
    }
    None
}

/// 正文顶层的元素：(节点, 是不是段落, 它里面第一个段落的编号)；不含最后的分节属性
fn top_items<'a>(body: Node<'a, 'a>) -> Vec<(Node<'a, 'a>, bool, usize)> {
    let mut ord = 0;
    let mut out = Vec::new();
    for c in body.children().filter(|c| c.is_element() && !is(c, "sectPr")) {
        out.push((c, is(&c, "p"), ord + 1));
        ord += c.descendants().filter(|d| is(d, "p")).count();
    }
    out
}

fn para_sect<'a>(p: Node<'a, 'a>) -> Option<Node<'a, 'a>> {
    child(p, "pPr").and_then(|pp| child(pp, "sectPr"))
}

fn item_of(items: &[(Node, bool, usize)], a: &Anchor) -> Option<usize> {
    items.iter().position(|(_, is_p, ord)| *is_p && *ord == a.paragraph)
}

/// 分区实际从哪个顶层元素开始、要不要新加分节符：分界点前面只隔着空段落就是分节符（或文档开头）时，
/// 那些空段落算进后面的分区，不用再加。
fn settle(items: &[(Node, bool, usize)], o: &outline::Outline, i: usize) -> (usize, bool) {
    let blank = |j: usize| {
        let (n, is_p, ord) = items[j];
        is_p && o.paragraphs.get(ord - 1).is_some_and(|p| p.is_empty()) && items.get(j + 1).is_none_or(|x| x.2 == ord + 1) && para_sect(n).is_none()
    };
    let mut j = i;
    while j > 0 && blank(j - 1) {
        j -= 1;
    }
    if j == 0 || (items[j - 1].1 && para_sect(items[j - 1].0).is_some()) { (j, false) } else { (i, true) }
}

/// 规范要不要分区处理、计划给没给正文的起点
fn zoned(spec: &Spec, plan: &Plan) -> bool {
    spec.structure.needs_zones() && plan.layout.body_start.is_some()
}

/// 按规范和计划，排版会在排版前的文档里新加几个分节符
fn expected_breaks(before: &Package, spec: &Spec, plan: &Plan) -> Result<usize> {
    if !zoned(spec, plan) {
        return Ok(0);
    }
    let xml = before.get_str(DOC)?;
    let doc = parse(&xml)?;
    let Some(body) = doc.descendants().find(|n| is(n, "body")) else { return Ok(0) };
    let items = top_items(body);
    let o = outline::read(before)?;
    let l = &plan.layout;
    Ok([&l.front_start, &l.body_start, &l.back_start].into_iter().flatten().filter_map(|a| item_of(&items, a)).filter(|i| settle(&items, &o, *i).1).count())
}

/// 内容类型登记：排版后 = 排版前 + 每个新部件一条 Override
fn content_types_ok(before: &Package, after: &Package, new_parts: &BTreeMap<String, &'static str>) -> bool {
    type Entry = (String, Vec<(String, String)>);
    let entries = |pkg: &Package| -> Option<BTreeSet<Entry>> {
        let xml = pkg.get_str(TYPES).ok()?;
        let doc = Document::parse(&xml).ok()?;
        Some(
            doc.root_element()
                .children()
                .filter(|c| c.is_element())
                .map(|c| {
                    let mut attrs: Vec<(String, String)> = c.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect();
                    attrs.sort();
                    (c.tag_name().name().to_string(), attrs)
                })
                .collect(),
        )
    };
    let (Some(b), Some(a)) = (entries(before), entries(after)) else { return false };
    let mut want = b;
    for (name, kind) in new_parts {
        let ct = format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml");
        want.insert(("Override".into(), vec![("ContentType".into(), ct), ("PartName".into(), format!("/{name}"))]));
    }
    a == want
}

/// 主文档的关系：排版后 = 排版前 + 每个新部件正好一条页眉 / 页脚关系
fn rels_ok(before: &Package, after: &Package, new_parts: &BTreeMap<String, &'static str>) -> bool {
    let (b, a) = (relationships(before, RELS), relationships(after, RELS));
    if a.len() != b.len() + new_parts.len() || a[..b.len()] != b[..] {
        return false;
    }
    let mut seen = BTreeSet::new();
    a[b.len()..].iter().all(|(id, ty, target, mode)| {
        let name = format!("word/{target}");
        !id.is_empty() && mode.is_empty() && new_parts.get(&name).is_some_and(|kind| ty.ends_with(&format!("/relationships/{kind}"))) && seen.insert(name)
    }) && seen.len() == new_parts.len()
}

/// 保真核对：结构类排版允许的那几种改动放行，其余照旧。
pub fn compare(before: &Package, after: &Package, spec: &Spec, plan: &Plan) -> Result<Vec<Problem>> {
    let (sb, sa) = (verify::snapshot(before)?, verify::snapshot(after)?);
    let mut problems = verify::compare(&sb, &sa);
    problems.extend(order_problems(before, after));
    if spec.structure.is_empty() {
        return Ok(problems);
    }
    let parts_wanted = zoned(spec, plan) && (spec.structure.header.is_some() || spec.structure.page_numbers.is_some());
    let new_parts: BTreeMap<String, &'static str> = sa.parts.keys().filter(|n| !sb.parts.contains_key(*n)).filter_map(|n| new_part_kind(n).map(|k| (n.clone(), k))).collect();
    let types_ok = parts_wanted && content_types_ok(before, after, &new_parts);
    let rel_ok = parts_wanted && rels_ok(before, after, &new_parts);
    let sections = |s: &verify::Snapshot| s.stories.get(DOC).and_then(|st| st.counts.get("分节").copied()).unwrap_or(0);
    let breaks_ok = sections(&sa) == sections(&sb) + expected_breaks(before, spec, plan)?;
    problems.retain(|p| {
        let allowed = match (p.kind.as_str(), p.part.as_str()) {
            ("新增部件", name) => parts_wanted && new_parts.contains_key(name),
            ("部件被改动", TYPES) => types_ok,
            // 关系文件既按内容比（关系变化），也按字节比（部件被改动）
            ("关系变化", RELS) | ("部件被改动", RELS) => rel_ok,
            ("结构数量变化", DOC) => p.detail.starts_with("分节：") && breaks_ok,
            _ => false,
        };
        !allowed
    });
    Ok(problems)
}

// ---------- 子元素顺序 ----------

const BORDER_ORDER: &[&str] = &["top", "left", "start", "bottom", "right", "end", "insideH", "insideV", "tl2br", "tr2bl"];

/// 这些元素的子元素有规定的先后顺序，排错了 Word 会说文件损坏。数一数各类元素里顺序不对的有几个。
fn order_violations(pkg: &Package) -> BTreeMap<&'static str, usize> {
    let tables: [(&'static str, &[&str]); 7] = [
        ("sectPr", structure::SECT_ORDER),
        ("pPr", structure::P_ORDER),
        ("tblPr", structure::TBLPR_ORDER),
        ("tcPr", structure::TCPR_ORDER),
        ("tblBorders", BORDER_ORDER),
        ("tcBorders", BORDER_ORDER),
        ("footnotePr", structure::FTNPR_ORDER),
    ];
    let mut out = BTreeMap::new();
    let mut scan = |xml: &str, root_order: Option<(&'static str, &[&str])>| {
        let Ok(doc) = parse(xml) else { return };
        for n in doc.descendants().filter(|n| n.is_element() && n.tag_name().namespace() == Some(NS_W)) {
            let name = n.tag_name().name();
            let Some((label, order)) = tables.iter().find(|(k, _)| *k == name).copied().or(root_order.filter(|(k, _)| *k == name)) else { continue };
            let ranks: Vec<usize> = n.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(NS_W)).filter_map(|c| order.iter().position(|o| *o == c.tag_name().name())).collect();
            if ranks.windows(2).any(|w| w[0] > w[1]) {
                *out.entry(label).or_default() += 1;
            }
        }
    };
    if let Ok(x) = pkg.get_str(DOC) {
        scan(&x, None);
    }
    if let Ok(x) = pkg.get_str("word/settings.xml") {
        scan(&x, Some(("settings", structure::SETTINGS_ORDER)));
    }
    out
}

/// 排版后子元素顺序不对的元素比排版前多了：是排版器写错了位置（原文档里本来就有的不算）
fn order_problems(before: &Package, after: &Package) -> Vec<Problem> {
    let (b, a) = (order_violations(before), order_violations(after));
    a.iter()
        .filter(|(k, n)| **n > b.get(*k).copied().unwrap_or(0))
        .map(|(k, n)| Problem { part: if *k == "settings" || *k == "footnotePr" { "word/settings.xml".into() } else { DOC.into() }, kind: "元素顺序不对".into(), detail: format!("{k} 里子元素顺序不合规定的有 {n} 处（排版前 {} 处），Word 可能打不开", b.get(k).copied().unwrap_or(0)) })
        .collect()
}

// ---------- 生效核对 ----------

struct Sect<'a> {
    node: Node<'a, 'a>,
    zone: Zone,
    first_in_zone: bool,
}

/// 排版后的分节与各自的分区
fn sections<'a>(body: Node<'a, 'a>, plan: &Plan, o: &outline::Outline) -> Vec<Sect<'a>> {
    let items = top_items(body);
    let l = &plan.layout;
    let at = |a: &Option<Anchor>| a.as_ref().and_then(|a| item_of(&items, a)).map(|i| settle(&items, o, i).0);
    let (f, b, k) = (at(&l.front_start), at(&l.body_start), at(&l.back_start));
    let zone_of = |i: usize| {
        if k.is_some_and(|k| i >= k) {
            Zone::Back
        } else if b.is_some_and(|b| i >= b) {
            Zone::Main
        } else if f.is_some_and(|f| i >= f) {
            Zone::Front
        } else {
            Zone::Cover
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut seen = BTreeSet::new();
    let mut push = |node: Node<'a, 'a>, start: usize, out: &mut Vec<Sect<'a>>| {
        let zone = zone_of(start);
        out.push(Sect { node, zone, first_in_zone: seen.insert(zone) });
    };
    for (i, (n, is_p, _)) in items.iter().enumerate() {
        if *is_p
            && let Some(s) = para_sect(*n)
        {
            push(s, start, &mut out);
            start = i + 1;
        }
    }
    if let Some(s) = child(body, "sectPr") {
        push(s, start, &mut out);
    }
    out
}

/// 分节引用的页眉 / 页脚部件（kind = headerReference / footerReference，ty = default / even）
fn referenced<'a>(sect: Node, kind: &str, ty: &str, rels: &'a BTreeMap<String, String>) -> Option<&'a String> {
    let id = sect.children().find(|c| is(c, kind) && wattr(*c, "type") == Some(ty))?.attribute((NS_R, "id"))?;
    rels.get(id)
}

/// 一行页眉排出来应该是什么文字（章名是域，结果留空）、几个章名域
fn expected_line(line: &HeaderLine) -> (String, usize) {
    let (l, c, r) = (line.left.as_deref().unwrap_or(""), line.center.as_deref().unwrap_or(""), line.right.as_deref().unwrap_or(""));
    let fields = [l, c, r].iter().map(|s| s.matches(CHAPTER).count()).sum();
    let text = if [l, c, r].iter().filter(|s| !s.is_empty()).count() <= 1 {
        format!("{l}{c}{r}")
    } else {
        // 左、中、右之间各隔一个制表符（左边空着也要有，才能跳到居中的位置）
        let mut t = l.to_string();
        for part in [c, r] {
            if !part.is_empty() {
                t.push('\t');
                t.push_str(part);
            }
        }
        t
    };
    (text.replace(CHAPTER, ""), fields)
}

fn header_problem(after: &Package, part: &str, line: &HeaderLine, rule: Option<bool>) -> Option<String> {
    let Ok(xml) = after.get_str(part) else { return Some(format!("找不到页眉部件 {part}")) };
    let Ok(story) = verify::story_of(&xml) else { return Some(format!("{part} 读不了")) };
    let (text, fields) = expected_line(line);
    if story.text.trim_end_matches('\n') != text {
        return Some(format!("文字是「{}」，应为「{}」", story.text.trim_end_matches('\n').replace('\t', "⇥"), text.replace('\t', "⇥")));
    }
    if story.field_codes.iter().filter(|c| c.as_str() == CHAPTER_FIELD).count() != fields || story.field_codes.len() != fields {
        return Some(format!("章名域 {} 个，应为 {fields} 个", story.field_codes.len()));
    }
    if let Some(want) = rule {
        let doc = parse(&xml).ok()?;
        let has = doc.descendants().filter(|n| is(n, "pBdr")).filter_map(|b| child(b, "bottom")).any(|b| wattr(b, "val").is_some_and(|v| v != "nil" && v != "none"));
        if has != want {
            return Some(if want { "页眉下面没有横线".into() } else { "页眉下面还有横线".into() });
        }
    }
    None
}

fn footer_problem(after: &Package, part: &str, align: &str) -> Option<String> {
    let Ok(xml) = after.get_str(part) else { return Some(format!("找不到页脚部件 {part}")) };
    let Ok(story) = verify::story_of(&xml) else { return Some(format!("{part} 读不了")) };
    if story.field_codes != ["PAGE"] {
        return Some(format!("页脚里应当只有一个页码域，现在是 {:?}", story.field_codes));
    }
    let doc = parse(&xml).ok()?;
    let jc = doc.descendants().find(|n| is(n, "jc")).and_then(|n| wattr(n, "val")).unwrap_or("left");
    (jc != align).then(|| format!("页码对齐是 {jc}，应为 {align}"))
}

fn check_table(tbl: Node) -> Option<String> {
    let side = |parent: Option<Node>, name: &str| -> (bool, u32) {
        let b = parent.and_then(|p| child(p, name));
        let val = b.and_then(|b| wattr(b, "val")).unwrap_or("");
        (!val.is_empty() && val != "nil" && val != "none", b.and_then(|b| wattr(b, "sz")).and_then(|v| v.parse().ok()).unwrap_or(0))
    };
    let tb = child(tbl, "tblPr").and_then(|p| child(p, "tblBorders"));
    for (name, want) in [("top", Some(RULE_HEAVY)), ("bottom", Some(RULE_HEAVY)), ("left", None), ("right", None), ("insideH", None), ("insideV", None)] {
        let (on, sz) = side(tb, name);
        match want {
            Some(w) if !on || sz != w => return Some(format!("表格的 {name} 线应为 {} 磅实线", w as f64 / 8.0)),
            None if on => return Some(format!("表格不该有 {name} 线")),
            _ => {}
        }
    }
    let rows: Vec<Node> = tbl.children().filter(|c| is(c, "tr")).collect();
    // 栏目线：最后一行以外，底边是细线的那一行（只能有一行，且在上半部分）
    let mut light_rows = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if child(*row, "tblPrEx").and_then(|e| child(e, "tblBorders")).is_some() {
            return Some(format!("第 {} 行还有自己的边框设置", i + 1));
        }
        let mut row_bottom = None;
        for tc in row.children().filter(|c| is(c, "tc")) {
            let b = child(tc, "tcPr").and_then(|p| child(p, "tcBorders"));
            if b.is_none() {
                return Some(format!("第 {} 行有单元格没有写明边框", i + 1));
            }
            let (top_on, top_sz) = side(b, "top");
            if (i == 0) != top_on || (i == 0 && top_sz != RULE_HEAVY) {
                return Some(format!("第 {} 行单元格的上边线不对", i + 1));
            }
            if side(b, "left").0 || side(b, "right").0 {
                return Some(format!("第 {} 行有竖线", i + 1));
            }
            let bottom = side(b, "bottom");
            if row_bottom.is_some_and(|x| x != bottom) {
                return Some(format!("第 {} 行各格的下边线不一致", i + 1));
            }
            row_bottom = Some(bottom);
        }
        match row_bottom {
            Some((true, sz)) if i + 1 == rows.len() => {
                if sz != RULE_HEAVY {
                    return Some("底线应为 1.5 磅".into());
                }
            }
            Some((true, sz)) => {
                if sz != RULE_LIGHT {
                    return Some(format!("第 {} 行下面的线应为 0.75 磅", i + 1));
                }
                light_rows.push(i);
            }
            Some((false, _)) if i + 1 == rows.len() => return Some("没有底线".into()),
            _ => {}
        }
    }
    match light_rows.len() {
        1 => None,
        0 => Some("没有栏目线（表头下面的细线）".into()),
        n => Some(format!("表内有 {n} 条横线，三线表只该有一条栏目线")),
    }
}

/// 结构类要求逐项核对。`before` 用来推出预期（哪些表格该排），`after` 是排版结果。
pub fn check(before: &Package, after: &Package, spec: &Spec, plan: &Plan) -> Result<Vec<Check>> {
    let st = &spec.structure;
    let mut out = Vec::new();
    if st.is_empty() {
        return Ok(out);
    }
    let mut push = |item: &str, problem: Option<String>, good: &str| out.push(Check { item: item.to_string(), ok: problem.is_none(), detail: problem.unwrap_or_else(|| good.to_string()) });
    let xml = after.get_str(DOC)?;
    let doc = parse(&xml)?;
    let body = doc.descendants().find(|n| is(n, "body")).ok_or_else(|| Error::Missing("word/document.xml 的 body".into()))?;
    let settings_xml = after.get_str("word/settings.xml").unwrap_or_default();
    let settings = parse(&settings_xml).ok();
    let o_after = outline::read(after)?;

    if st.needs_zones() {
        let layout = &plan.layout;
        if layout.body_start.is_none() {
            push("分区", Some("计划里没有指出正文从哪一段开始，页眉、页码、奇数页起没有处理".into()), "");
        } else {
            // 分界处有分节符
            let items = top_items(body);
            for (a, what) in [(&layout.front_start, "前置部分"), (&layout.body_start, "正文"), (&layout.back_start, "正文之后")] {
                let Some(a) = a else { continue };
                let problem = match item_of(&items, a) {
                    None => Some(format!("第 {} 段不在正文顶层", a.paragraph)),
                    Some(i) => settle(&items, &o_after, i).1.then(|| "前面没有分节符".to_string()),
                }
                .or_else(|| structure::check_anchor(&o_after, a, what).err().map(|e| e.to_string()));
                push(&format!("分区：{what}从「{}」开始", outline::snippet(&a.text, 12)), problem, "前面有分节符");
            }
            let rels: BTreeMap<String, String> = relationships(after, RELS).into_iter().map(|(id, _, target, _)| (id, format!("word/{target}"))).collect();
            let sects = sections(body, plan, &o_after);
            let duplex = layout.duplex;
            let even_on = settings.as_ref().is_some_and(|s| s.descendants().any(|n| is(&n, "evenAndOddHeaders")));
            if st.header.is_some() || st.page_numbers.is_some() {
                push("页眉页脚：奇偶页", (even_on != duplex).then(|| if duplex { "双面印制，但文档没有打开「奇偶页不同」".to_string() } else { "单面印制，但文档还开着「奇偶页不同」".to_string() }), if duplex { "奇偶页不同（双面）" } else { "奇偶页相同（单面）" });
            }
            if let Some(h) = &st.header {
                let (odd, even) = h.lines(duplex);
                for zone in [Zone::Front, Zone::Main, Zone::Back] {
                    let mine: Vec<&Sect> = sects.iter().filter(|s| s.zone == zone).collect();
                    if mine.is_empty() {
                        continue;
                    }
                    let empty = HeaderLine::default();
                    let (want_odd, want_even, rule) = if zone == Zone::Back { (&empty, Some(&empty), Some(false)) } else { (&odd, even.as_ref(), h.rule) };
                    let mut problem = None;
                    for (i, s) in mine.iter().enumerate() {
                        let mut kinds = vec![("default", want_odd)];
                        if duplex && let Some(e) = want_even {
                            kinds.push(("even", e));
                        }
                        for (ty, line) in kinds {
                            let p = match referenced(s.node, "headerReference", ty, &rels) {
                                None => Some(format!("没有{}页眉", if ty == "even" { "偶数页" } else { "" })),
                                Some(part) if new_part_kind(part) != Some("header") => Some("引用的不是这次生成的页眉".to_string()),
                                Some(part) => header_problem(after, part, line, rule),
                            };
                            if let Some(p) = p {
                                problem.get_or_insert(format!("第 {} 节{}：{p}", i + 1, if ty == "even" { "（偶数页）" } else { "" }));
                            }
                        }
                        if child(s.node, "titlePg").is_some() {
                            problem.get_or_insert(format!("第 {} 节还开着「首页不同」", i + 1));
                        }
                    }
                    let show = |l: &HeaderLine| {
                        [("左", &l.left), ("中", &l.center), ("右", &l.right)].into_iter().filter_map(|(k, v)| v.as_ref().filter(|v| !v.is_empty()).map(|v| format!("{k}「{v}」"))).collect::<Vec<_>>().join(" ")
                    };
                    let good = if zone == Zone::Back {
                        "空页眉".to_string()
                    } else {
                        format!("{}{}{}", show(want_odd), want_even.filter(|_| duplex).map(|e| format!("；偶数页 {}", show(e))).unwrap_or_default(), if h.rule == Some(true) { "，有横线" } else { "" })
                    };
                    push(&format!("{}：页眉", zone.label()), problem, &format!("{} 节，{good}", mine.len()));
                }
            }
            if let Some(pn) = &st.page_numbers {
                let align = pn.align.as_deref().unwrap_or("center");
                for zone in [Zone::Front, Zone::Main, Zone::Back] {
                    let mine: Vec<&Sect> = sects.iter().filter(|s| s.zone == zone).collect();
                    if mine.is_empty() {
                        continue;
                    }
                    let fmt = match zone {
                        Zone::Front => pn.front_format.as_deref(),
                        _ => pn.body_format.as_deref(),
                    }
                    .and_then(page_format_ooxml);
                    let mut problem = None;
                    for (i, s) in mine.iter().enumerate() {
                        let pg = child(s.node, "pgNumType");
                        let got_fmt = pg.and_then(|p| wattr(p, "fmt"));
                        let got_start = pg.and_then(|p| wattr(p, "start"));
                        let want_start = (s.first_in_zone && zone != Zone::Back).then_some("1");
                        let mut p = None;
                        if got_fmt != fmt && !(fmt.is_none() && got_fmt.is_none()) {
                            p = Some(format!("页码形式是 {}，应为 {}", got_fmt.unwrap_or("（默认）"), fmt.unwrap_or("（默认）")));
                        } else if got_start != want_start {
                            p = Some(match want_start {
                                Some(_) => "页码没有从 1 起".to_string(),
                                None => "页码不该在这里重新起".to_string(),
                            });
                        }
                        let mut types = vec!["default"];
                        if duplex {
                            types.push("even");
                        }
                        for ty in types {
                            if p.is_some() {
                                break;
                            }
                            p = match referenced(s.node, "footerReference", ty, &rels) {
                                None => Some("没有页脚".to_string()),
                                Some(part) if new_part_kind(part) != Some("footer") => Some("引用的不是这次生成的页脚".to_string()),
                                Some(part) => footer_problem(after, part, align),
                            };
                        }
                        if let Some(p) = p {
                            problem.get_or_insert(format!("第 {} 节：{p}", i + 1));
                        }
                    }
                    let name = match fmt {
                        Some("upperRoman") => "大写罗马数字",
                        Some("lowerRoman") => "小写罗马数字",
                        Some("decimal") => "阿拉伯数字",
                        _ => "原来的数字形式",
                    };
                    let place = match align {
                        "left" => "居左",
                        "right" => "居右",
                        _ => "居中",
                    };
                    push(&format!("{}：页码", zone.label()), problem, &format!("{} 节，{name}{}，页脚{place}", mine.len(), if zone == Zone::Back { "，接着正文" } else { "，从 1 起" }));
                }
            }
            if st.body_on_odd_page == Some(true) {
                let first = sects.iter().find(|s| s.zone == Zone::Main);
                let ok = first.is_some_and(|s| child(s.node, "type").and_then(|t| wattr(t, "val")) == Some("oddPage"));
                push("正文从奇数页开始", (!ok).then(|| "正文第一节的分节类型不是「奇数页」".to_string()), "正文第一节从奇数页起");
            }
        }
    }

    if st.three_line_tables == Some(true) {
        let (targets, _) = structure::three_line_targets(&outline::read(before)?, plan);
        let tables: Vec<Node> = body.descendants().filter(|n| is(n, "tbl")).collect();
        for n in targets {
            let problem = match tables.get(n - 1) {
                Some(t) => check_table(*t),
                None => Some("排版后找不到这个表格".to_string()),
            };
            push(&format!("第 {n} 个表格：三线表"), problem, "顶线、栏目线、底线，没有竖线");
        }
    }

    if let Some(f) = &st.footnote_numbering {
        let pr = settings.as_ref().and_then(|s| s.root_element().children().find(|c| is(c, "footnotePr")));
        let get = |k: &str| pr.and_then(|p| child(p, k)).and_then(|n| wattr(n, "val"));
        let mut problem = None;
        if let Some(want) = f.format_ooxml()
            && get("numFmt") != Some(want)
        {
            problem = Some(format!("编号形式是 {}，应为 {want}", get("numFmt").unwrap_or("（默认）")));
        }
        if let Some(want) = f.restart_ooxml()
            && get("numRestart") != Some(want)
        {
            problem.get_or_insert(format!("重新编号是 {}，应为 {want}", get("numRestart").unwrap_or("（默认）")));
        }
        if doc.descendants().filter(|n| is(n, "sectPr")).filter_map(|s| child(s, "footnotePr")).any(|p| child(p, "numFmt").is_some() || child(p, "numRestart").is_some()) {
            problem.get_or_insert("有分节单独设了脚注编号，会盖过文档的设置".to_string());
        }
        let what = [f.format.as_deref().map(|x| if x == "circled" { "①②③" } else { "1 2 3" }), f.restart.as_deref().map(|x| match x {
            "each_page" => "每页重新编号",
            "each_section" => "每节重新编号",
            _ => "连续编号",
        })]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("，");
        push("脚注编号", problem, &what);
    }

    if !st.tight_headings.is_empty() {
        let targets = structure::tight_heading_paragraphs(&o_after, plan, &st.tight_headings);
        let paras: Vec<Node> = body.descendants().filter(|n| is(n, "p")).collect();
        let bad: Vec<String> = targets
            .iter()
            .filter(|n| {
                let before = paras.get(**n - 1).and_then(|p| child(*p, "pPr")).and_then(|pp| child(pp, "spacing")).and_then(|s| wattr(s, "before"));
                before != Some("0")
            })
            .map(|n| format!("第 {n} 段"))
            .collect();
        push("紧跟上一级标题的标题：段前不空", (!bad.is_empty()).then(|| format!("{} 的段前不是 0", bad.join("、"))), &format!("{} 个", targets.len()));
    }
    Ok(out)
}
