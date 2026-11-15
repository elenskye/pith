//! 两本真实 EPUB 的解析和全书对齐（默认忽略；书不进 Git，书里的文字不进断言和输出，只打印聚合数字）。
//!
//! ```bash
//! PITH_BOOK_EN=local/wealth-of-nations/en-penguin-skinner.epub PITH_BOOK_ZH=local/wealth-of-nations/zh-yang-jingnian-2011.epub cargo test -p concept-engine --test real_books -- --ignored --nocapture
//! ```
//! 路径相对于仓库根目录或写绝对路径。
//!
//! 想比较「只用长度和锚点」和「再加语义相似度」，再给一个本机向量服务的地址（OpenAI 兼容的 `/v1/embeddings`，
//! 用 `curl` 调，引擎本身不联网）：`PITH_EMBED_URL=http://127.0.0.1:8081/v1/embeddings PITH_EMBED_MODEL=qwen3-embedding-0.6b`。
//! 向量缓存在 `target/tmp/`（只有哈希和数字，没有书里的文字）。

use std::collections::HashMap;
use std::io::Write;

use concept_engine::align::{self, Alignment, Method, Options};
use concept_engine::text::{self, Numbering, Paragraph, Role, Text, Unit, UnitKind};

fn load(var: &str) -> Option<Text> {
    let path = std::env::var(var).ok().filter(|p| !p.is_empty())?;
    let p = std::path::Path::new(&path);
    let p = if p.is_absolute() { p.to_path_buf() } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p) };
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("读不了 {}：{e}", p.display()));
    Some(text::from_epub(&bytes, None).expect("解析失败"))
}

/// 第一篇第 1–3 章
fn first_three(t: &Text) -> Vec<usize> {
    let mut out = Vec::new();
    for u in t.chapters() {
        if u.kind == UnitKind::Chapter && u.book == Some(1) && matches!(u.chapter, Some(1..=3)) {
            out.push(u.index);
        }
    }
    out
}

fn report(name: &str, t: &Text) {
    let p = text::preview(t);
    println!("== {name}：语言 {}，段落号 {:?}，页码来源 {}，有页码 {}", p.language, p.numbering, p.page_source, p.has_pages);
    println!("   全书：段 {}，正文 {}，标题 {}，旁注 {}，注释 {}，导读 {}，前后附 {}；单元 {}", p.paragraphs, p.totals.body, p.totals.heading, p.totals.sidenote, p.totals.note, p.totals.guide, p.totals.matter, t.units.len());
    for u in first_three(t) {
        let up = p.units.iter().find(|x| x.index == u).unwrap();
        println!("   {}：正文 {}，旁注 {}，注释 {}，标题 {}，页 {:?}", up.key, up.counts.body, up.counts.sidenote, up.counts.note, up.counts.heading, up.pages);
    }
    for w in &p.warnings {
        println!("   提示：{w}");
    }
}

#[test]
#[ignore]
fn real_epubs_first_three_chapters() {
    let en = load("PITH_BOOK_EN");
    let zh = load("PITH_BOOK_ZH");
    if en.is_none() && zh.is_none() {
        println!("没给 PITH_BOOK_EN / PITH_BOOK_ZH，跳过");
        return;
    }
    if let Some(en) = &en {
        report("英文", en);
        assert_eq!(en.numbering, Some(Numbering::Glasgow));
        let chapters = first_three(en);
        let counts: Vec<usize> = chapters.iter().map(|u| en.body_of(*u).count()).collect();
        assert_eq!(counts, [11, 5, 8]);
        let keys: Vec<String> = chapters.iter().map(|u| en.units[*u].key.clone()).collect();
        assert_eq!(keys, ["I.i", "I.ii", "I.iii"]);
        // 页码：第 1–3 章正文都在 109–126 页
        let pages: Vec<u32> = chapters.iter().flat_map(|u| en.body_of(*u)).flat_map(|p| {
            let mut v: Vec<u32> = p.page.iter().filter_map(|x| x.parse().ok()).collect();
            v.extend(p.breaks.iter().filter_map(|b| b.page.parse::<u32>().ok()));
            v
        }).collect();
        assert_eq!(pages.iter().min(), Some(&109));
        assert_eq!(pages.iter().max(), Some(&126));
        assert!(chapters.iter().flat_map(|u| en.body_of(*u)).all(|p| p.page.is_some()));
        // 小型大写照样取字：第一段以全大写的词开头
        let first = en.body_of(chapters[0]).next().unwrap();
        assert!(first.text.split_whitespace().next().unwrap().chars().all(|c| c.is_uppercase()));
        println!("   I.i.1 跨页：{:?}；I.i.2 页码 {:?}", en.paragraph_pages(first.seq), en.paragraph("I.i.2").and_then(|p| en.paragraph_pages(p.seq)));
        // 归类（`decisions/0025` 记的三处）：书名页和作者简介、第二册编者的 Introduction、第二册平铺的脚注目录都不是正文
        let body_units: Vec<&Unit> = en.chapters();
        println!("   有正文的单元 {} 个：{:?}", body_units.len(), body_units.iter().map(|u| u.key.as_str()).collect::<Vec<_>>());
        assert!(body_units.iter().all(|u| matches!(u.kind, UnitKind::Chapter | UnitKind::Book) || u.key.ends_with("Intro") || u.label == "Appendix"), "还有归错的正文单元");
        assert_eq!(body_units.iter().filter(|u| u.key == "Intro").count(), 1, "只有斯密自己的引言是正文");
        assert!(en.units.iter().filter(|u| u.label == "Introduction").all(|u| u.kind == UnitKind::Guide));
        assert!(en.units.iter().filter(|u| u.label.starts_with("Page ")).all(|u| u.kind == UnitKind::Notes));
        // 表格整张算一段：没有哪一章因为表格多出几百段
        let tables = en.paragraphs.iter().filter(|p| p.role == Role::Body && align::is_table(&p.text)).count();
        println!("   正文里的表格 {tables} 张；最长的一章 {} 段", body_units.iter().map(|u| en.body_of(u.index).count()).max().unwrap_or(0));
        assert!(body_units.iter().all(|u| en.body_of(u.index).count() < 400));
    }
    if let Some(zh) = &zh {
        report("中文", zh);
        assert_eq!(zh.language, "zh");
        let chapters = first_three(zh);
        let counts: Vec<usize> = chapters.iter().map(|u| zh.body_of(*u).count()).collect();
        assert_eq!(counts, [11, 5, 8]);
        assert!(!zh.paragraphs.iter().any(|p| p.page.is_some()), "中文版没有页码");
        let p = text::preview(zh);
        assert!(p.totals.guide > 0 && p.totals.sidenote > 0 && p.totals.note > 0);
        // 导读、旁注、注释都不是正文
        for u in &chapters {
            let unit_paras: Vec<_> = zh.paragraphs.iter().filter(|p| p.unit == *u).collect();
            let notes = unit_paras.iter().filter(|p| p.role == Role::Note).count();
            let sides = unit_paras.iter().filter(|p| p.role == Role::Sidenote).count();
            println!("   {}：注释 {notes}，旁注 {sides}", zh.units[*u].key);
        }
        // 归类：书名页不是正文；第五编的第三章和附录归第五编（目录把它们和第五编放在同一层）
        let body_units: Vec<&Unit> = zh.chapters();
        println!("   有正文的单元 {} 个：{:?}", body_units.len(), body_units.iter().map(|u| u.key.as_str()).collect::<Vec<_>>());
        assert!(body_units.iter().all(|u| u.kind == UnitKind::Chapter || u.key.ends_with("Intro") || u.label.contains("附录")), "还有归错的正文单元");
        assert!(body_units.iter().any(|u| u.key == "V.iii") && body_units.iter().filter(|u| u.key == "IV.iii").count() == 1);
    }
    if let (Some(en), Some(zh)) = (&en, &zh) {
        let a = align::align(en, zh);
        let first: Vec<_> = a.units.iter().filter(|u| ["I.i", "I.ii", "I.iii"].contains(&u.key.as_str())).collect();
        println!("== 对齐：{} 章对上（段数相同 {}，算法对的 {}），{} 章没对上；第一篇 1–3 章：{:?}", a.units.iter().filter(|u| u.aligned).count(), count(&a, Method::Exact), count(&a, Method::Fitted), a.mismatched().len(), first.iter().map(|u| (u.key.as_str(), u.count, u.other_count, u.method)).collect::<Vec<_>>());
        assert!(first.len() == 3 && first.iter().all(|u| u.method == Method::Exact));
    }
}

fn count(a: &Alignment, m: Method) -> usize {
    a.units.iter().filter(|u| u.method == m).count()
}

// ---------- 全书对齐 ----------

/// 本机向量服务（可选）。只在这个默认忽略的测试里用
struct Vectors {
    url: String,
    model: String,
    cache: std::path::PathBuf,
    map: HashMap<u64, Vec<f32>>,
}

/// FNV-1a
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// 送去算向量的文字：太长的只取前面（够比相似度了）
fn clip(text: &str) -> String {
    text.chars().take(3000).collect()
}

impl Vectors {
    fn open() -> Option<Vectors> {
        let url = std::env::var("PITH_EMBED_URL").ok().filter(|u| !u.is_empty())?;
        let model = std::env::var("PITH_EMBED_MODEL").unwrap_or_else(|_| "qwen3-embedding-0.6b".into());
        let cache = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("embed-cache-{:016x}.bin", hash(&model)));
        let mut map = HashMap::new();
        if let Ok(bytes) = std::fs::read(&cache)
            && bytes.len() >= 4
        {
            let dim = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            for rec in bytes[4..].chunks_exact(8 + dim * 4) {
                let key = u64::from_le_bytes(rec[..8].try_into().unwrap());
                map.insert(key, rec[8..].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect());
            }
        }
        Some(Vectors { url, model, cache, map })
    }

    fn save(&self) {
        let Some(dim) = self.map.values().next().map(Vec::len) else { return };
        let mut out = Vec::with_capacity(4 + self.map.len() * (8 + dim * 4));
        out.extend((dim as u32).to_le_bytes());
        let mut keys: Vec<&u64> = self.map.keys().collect();
        keys.sort();
        for k in keys {
            out.extend(k.to_le_bytes());
            out.extend(self.map[k].iter().flat_map(|f| f.to_le_bytes()));
        }
        std::fs::write(&self.cache, out).expect("写不了向量缓存");
    }

    /// 把还没有向量的段落算出来（一批 16 段）
    fn ensure<'a>(&mut self, texts: impl Iterator<Item = &'a str>) {
        let mut missing: Vec<String> = Vec::new();
        for t in texts {
            let c = clip(t);
            if !self.map.contains_key(&hash(&c)) && !missing.contains(&c) {
                missing.push(c);
            }
        }
        if missing.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        for (n, batch) in missing.chunks(16).enumerate() {
            let body = serde_json::json!({ "input": batch, "model": self.model }).to_string();
            let mut child = std::process::Command::new("curl")
                .args(["-s", "-m", "600", "-X", "POST", &self.url, "-H", "Content-Type: application/json", "--data-binary", "@-"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("起不了 curl");
            child.stdin.take().unwrap().write_all(body.as_bytes()).unwrap();
            let out = child.wait_with_output().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("向量服务的回复读不了：{e}"));
            let data = v["data"].as_array().unwrap_or_else(|| panic!("向量服务没给 data（第 {n} 批）"));
            assert_eq!(data.len(), batch.len());
            for (text, d) in batch.iter().zip(data) {
                let mut vec: Vec<f32> = d["embedding"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
                let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                vec.iter_mut().for_each(|x| *x /= norm);
                self.map.insert(hash(text), vec);
            }
            if n % 40 == 39 {
                self.save();
            }
        }
        self.save();
        println!("   算了 {} 段的向量，用时 {:.0} 秒", missing.len(), started.elapsed().as_secs_f32());
    }

    /// 每段（按 seq）的向量
    fn table<'a>(&'a self, t: &Text) -> Vec<Option<&'a [f32]>> {
        t.paragraphs.iter().map(|p| if p.role == Role::Body { self.map.get(&hash(&clip(&p.text))).map(Vec::as_slice) } else { None }).collect()
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 用一组正文段落拼一本合成的书（每章一个单元）
fn book(language: &str, chapters: &[Vec<String>]) -> Text {
    let mut t = Text { language: language.into(), ..Default::default() };
    for (c, paragraphs) in chapters.iter().enumerate() {
        let key = format!("I.{}", text::roman(c as u32 + 1).to_lowercase());
        t.units.push(Unit { index: c, kind: UnitKind::Chapter, label: key.clone(), key: key.clone(), depth: 1, parent: None, book: Some(1), chapter: Some(c as u32 + 1) });
        for (i, body) in paragraphs.iter().enumerate() {
            let seq = t.paragraphs.len();
            t.paragraphs.push(Paragraph { seq, unit: c, role: Role::Body, number: Some(i as u32 + 1), key: format!("{key}.{}", i + 1), attached: None, text: body.clone(), page: None, breaks: Vec::new(), marker: None, noterefs: Vec::new() });
        }
    }
    t
}

/// 一组：（原著的几段, 译本的几段），下标是章内第几段
type Truth = Vec<(Vec<usize>, Vec<usize>)>;

/// 把一章（两边一段对一段）按固定的规则改乱。一章只往一个方向改，免得改完段数又一样（那样算法根本不会去对）：
/// 双数章合并译本的两段（2-1）、删掉译本的一段（1-0）；单数章合并原著的两段（1-2）、删掉原著的一段（0-1）。
/// `every`：大约每几段改一处。返回改过的两边和正确答案
fn perturb(a: &[String], b: &[String], every: usize, salt: usize) -> (Vec<String>, Vec<String>, Truth) {
    let (mut x, mut y, mut truth) = (Vec::new(), Vec::new(), Truth::new());
    let shrink_translation = salt.is_multiple_of(2);
    let mut i = 0;
    while i < a.len() {
        let roll = (i * 7 + salt * 13) % (every * 2);
        let last = i + 1 >= a.len();
        if roll == 0 && !last {
            if shrink_translation {
                x.push(a[i].clone());
                x.push(a[i + 1].clone());
                y.push(format!("{}{}", b[i], b[i + 1]));
                truth.push((vec![x.len() - 2, x.len() - 1], vec![y.len() - 1]));
            } else {
                x.push(format!("{} {}", a[i], a[i + 1]));
                y.push(b[i].clone());
                y.push(b[i + 1].clone());
                truth.push((vec![x.len() - 1], vec![y.len() - 2, y.len() - 1]));
            }
            i += 2;
        } else if roll == every {
            if shrink_translation {
                x.push(a[i].clone());
                truth.push((vec![x.len() - 1], vec![]));
            } else {
                y.push(b[i].clone());
                truth.push((vec![], vec![y.len() - 1]));
            }
            i += 1;
        } else {
            x.push(a[i].clone());
            y.push(b[i].clone());
            truth.push((vec![x.len() - 1], vec![y.len() - 1]));
            i += 1;
        }
    }
    (x, y, truth)
}

/// 算法找回了多少：按段算（每段对着的那几段和答案一模一样才算对），再按改动的种类分开算
#[derive(Default)]
struct Score {
    right: usize,
    total: usize,
    by_shape: std::collections::BTreeMap<String, (usize, usize)>,
    chapters_right: usize,
    chapters: usize,
}

impl Score {
    fn line(&self) -> String {
        let shapes: Vec<String> = self.by_shape.iter().map(|(k, (r, t))| format!("{k} {r}/{t}")).collect();
        format!("段 {}/{}（{:.1}%），整章全对 {}/{}；{}", self.right, self.total, 100.0 * self.right as f32 / self.total.max(1) as f32, self.chapters_right, self.chapters, shapes.join("，"))
    }
}

fn grade(al: &Alignment, x: &Text, y: &Text, truths: &[Truth], score: &mut Score) {
    for (c, truth) in truths.iter().enumerate() {
        let xs: Vec<usize> = x.body_of(c).map(|p| p.seq).collect();
        let ys: Vec<usize> = y.body_of(c).map(|p| p.seq).collect();
        let mut all = true;
        for (ta, tb) in truth {
            let shape = format!("{}-{}", ta.len(), tb.len());
            let want_b: Vec<usize> = tb.iter().map(|j| ys[*j]).collect();
            let want_a: Vec<usize> = ta.iter().map(|i| xs[*i]).collect();
            let ok = ta.iter().all(|i| al.other_of(xs[*i]) == want_b) && tb.iter().all(|j| al.original_of(ys[*j]) == want_a);
            let e = score.by_shape.entry(shape).or_default();
            e.1 += 1;
            e.0 += ok as usize;
            score.total += ta.len() + tb.len();
            score.right += if ok { ta.len() + tb.len() } else { 0 };
            all &= ok;
        }
        score.chapters += 1;
        score.chapters_right += all as usize;
    }
}

/// 两边都出现的数字（三位以上：年份、金额）有多少落在对上的那一组里
fn anchor_agreement(al: &Alignment, en: &Text, zh: &Text, method: Method) -> (usize, usize) {
    let numbers = |p: &Paragraph| -> Vec<String> { align::anchors(&p.text).into_iter().filter(|t| t.len() >= 3 && t.chars().all(|c| c.is_ascii_digit())).collect() };
    let (mut hit, mut total) = (0, 0);
    for u in al.units.iter().filter(|u| u.method == method) {
        let Some(other) = u.other_unit else { continue };
        let en_numbers: Vec<(usize, Vec<String>)> = en.body_of(u.unit).filter(|p| !align::is_table(&p.text)).map(|p| (p.seq, numbers(p))).collect();
        for q in zh.body_of(other) {
            let partners = al.original_of(q.seq);
            for t in numbers(q) {
                // 只看原著这一章里也有、而且只在一段里有的数字（否则说明不了问题）
                let holders: Vec<usize> = en_numbers.iter().filter(|(_, n)| n.contains(&t)).map(|(s, _)| *s).collect();
                if holders.len() == 1 {
                    total += 1;
                    hit += partners.contains(&holders[0]) as usize;
                }
            }
        }
    }
    (hit, total)
}

fn percentile(v: &mut [f32], p: f32) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * p).round() as usize]
}

fn summary(name: &str, al: &Alignment, en: &Text, zh: &Text) {
    let fitted: Vec<_> = al.units.iter().filter(|u| u.method == Method::Fitted).collect();
    let bodies: usize = al.units.iter().map(|u| u.count).sum();
    let with_partner = al.units.iter().flat_map(|u| en.body_of(u.unit)).filter(|p| !al.other_of(p.seq).is_empty()).count();
    let zh_bodies: usize = al.units.iter().map(|u| u.other_count).sum();
    let zh_with = al.units.iter().filter_map(|u| u.other_unit).flat_map(|o| zh.body_of(o)).filter(|p| !al.original_of(p.seq).is_empty()).count();
    println!("== {name}：{} 个单元，段数相同 {}，算法对的 {}（其中置信度低 {}），没对上 {}", al.units.len(), count(al, Method::Exact), fitted.len(), al.low().len(), count(al, Method::None));
    println!("   原著正文 {bodies} 段，有中译的 {with_partner}；译本正文 {zh_bodies} 段，对上原文的 {zh_with}；各种对法 {:?}；落空 {} 段", align::shapes(al), al.units.iter().map(|u| u.unmatched).sum::<usize>());
    for u in &fitted {
        println!("   {:9} {:3} / {:3} 段  置信度 {:.2}{}  合并 {:2} 组  落空 {:2} 段", u.key, u.count, u.other_count, u.confidence, if u.low { " 低" } else { "   " }, u.merged, u.unmatched);
    }
    for u in al.mismatched() {
        println!("   没对上：{}（{} / {} 段）", u.key, u.count, u.other_count);
    }
    let exact: Vec<_> = al.units.iter().filter(|u| u.method == Method::Exact).collect();
    println!("   段数相同的章：置信度最低 {:.2}，标成低的 {:?}", exact.iter().map(|u| u.confidence).fold(1.0, f32::min), exact.iter().filter(|u| u.low).map(|u| u.key.as_str()).collect::<Vec<_>>());
    let (eh, et) = anchor_agreement(al, en, zh, Method::Exact);
    let (fh, ft) = anchor_agreement(al, en, zh, Method::Fitted);
    println!("   数字锚点落在对上的那一组里：段数相同的章 {eh}/{et}（{:.1}%），算法对的章 {fh}/{ft}（{:.1}%）", 100.0 * eh as f32 / et.max(1) as f32, 100.0 * fh as f32 / ft.max(1) as f32);
}

/// 全书对齐：每个单元都有结果，第一篇 1–3 章和 v0.4 一样；给了向量服务就比较两种做法，并在段数相同的章上做「改乱再找回」的实验
#[test]
#[ignore]
fn real_epubs_full_alignment() {
    let (Some(en), Some(zh)) = (load("PITH_BOOK_EN"), load("PITH_BOOK_ZH")) else {
        println!("没给 PITH_BOOK_EN / PITH_BOOK_ZH，跳过");
        return;
    };
    let opt = Options::default();
    let plain = align::align(&en, &zh);
    // v0.4 的算法：段数相同才对齐
    let old_units = plain.units.iter().filter(|u| u.other_unit.is_some() && u.count > 0 && u.count == u.other_count).count();
    let old_pairs: usize = plain.units.iter().filter(|u| u.other_unit.is_some() && u.count == u.other_count).map(|u| u.count).sum();
    println!("== v0.4 的算法（段数相同才对齐，新的解析）：{} 个单元里对上 {old_units} 个，{old_pairs} 段", plain.units.len());
    summary("只用长度和锚点", &plain, &en, &zh);
    // 每个单元都有结果；段数相同的和 v0.4 完全一样（按顺序一段对一段）
    assert!(plain.units.iter().all(|u| u.other_unit.is_some()), "译本里找不到的单元：{:?}", plain.units.iter().filter(|u| u.other_unit.is_none()).map(|u| &u.key).collect::<Vec<_>>());
    assert!(plain.mismatched().is_empty(), "没对上的单元：{:?}", plain.mismatched().iter().map(|u| &u.key).collect::<Vec<_>>());
    for u in plain.units.iter().filter(|u| u.method == Method::Exact) {
        let want: Vec<(usize, usize)> = en.body_of(u.unit).map(|p| p.seq).zip(zh.body_of(u.other_unit.unwrap()).map(|p| p.seq)).collect();
        let got: Vec<(usize, usize)> = plain.pairs.iter().copied().filter(|(a, _)| en.paragraphs[*a].unit == u.unit).collect();
        assert_eq!(got, want, "{}", u.key);
    }
    for k in ["I.i", "I.ii", "I.iii"] {
        assert_eq!(plain.units.iter().find(|u| u.key == k).map(|u| u.method), Some(Method::Exact));
    }
    assert!(plain.pairs.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 <= w[1].1), "顺序交叉了");
    assert_eq!(plain, align::align(&en, &zh), "结果不确定");

    // 「改乱再找回」：段数相同的章当正确答案，按固定规则合并、删段，看找回多少
    let exact: Vec<(Vec<String>, Vec<String>)> = plain
        .units
        .iter()
        .filter(|u| u.method == Method::Exact && u.count >= 5)
        .map(|u| (en.body_of(u.unit).map(|p| p.text.clone()).collect(), zh.body_of(u.other_unit.unwrap()).map(|p| p.text.clone()).collect()))
        .collect();
    let mut experiments = Vec::new();
    for (name, every) in [("约每 20 段改一处（比真实的书多一倍）", 20usize), ("约每 8 段改一处", 8), ("约每 3 段改一处", 3)] {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut truths = Vec::new();
        for (c, (a, b)) in exact.iter().enumerate() {
            let (x, y, truth) = perturb(a, b, every, c);
            xs.push(x);
            ys.push(y);
            truths.push(truth);
        }
        experiments.push((name, book("en", &xs), book("zh", &ys), truths));
    }
    println!("== 改乱再找回（{} 个段数相同的章，共 {} 段；只用长度和锚点）", exact.len(), exact.iter().map(|(a, _)| a.len()).sum::<usize>());
    for (name, x, y, truths) in &experiments {
        let mut score = Score::default();
        grade(&align::align(x, y), x, y, truths, &mut score);
        println!("   {name}：{}", score.line());
    }

    let Some(mut vectors) = Vectors::open() else {
        println!("没给 PITH_EMBED_URL：不比较语义相似度");
        return;
    };
    let body = |t: &Text, units: Vec<usize>| -> Vec<String> { units.into_iter().flat_map(|u| t.body_of(u).map(|p| p.text.clone()).collect::<Vec<_>>()).collect() };
    let mut texts = body(&en, plain.units.iter().map(|u| u.unit).collect());
    texts.extend(body(&zh, plain.units.iter().filter_map(|u| u.other_unit).collect()));
    for (_, x, y, _) in &experiments {
        texts.extend(x.paragraphs.iter().chain(&y.paragraphs).map(|p| p.text.clone()));
    }
    vectors.ensure(texts.iter().map(String::as_str));
    let (ev, zv) = (vectors.table(&en), vectors.table(&zh));
    let similarity = |a: usize, b: usize| Some(cosine(ev[a]?, zv[b]?));
    // 这个模型下「对着的两段」和「不相干的两段」各有多像
    let mut matched: Vec<f32> = plain.units.iter().filter(|u| u.method == Method::Exact).flat_map(|u| en.body_of(u.unit).map(|p| p.seq).zip(zh.body_of(u.other_unit.unwrap()).map(|p| p.seq)).collect::<Vec<_>>()).filter_map(|(a, b)| similarity(a, b)).collect();
    let mut apart: Vec<f32> = plain.units.iter().filter(|u| u.method == Method::Exact && u.count >= 6).flat_map(|u| {
        let a: Vec<usize> = en.body_of(u.unit).map(|p| p.seq).collect();
        let b: Vec<usize> = zh.body_of(u.other_unit.unwrap()).map(|p| p.seq).collect();
        (0..a.len()).map(|i| (a[i], b[(i + b.len() / 2) % b.len()])).collect::<Vec<_>>()
    }).filter_map(|(a, b)| similarity(a, b)).collect();
    let offset = |d: usize| -> Vec<f32> {
        plain.units.iter().filter(|u| u.method == Method::Exact && u.count > d).flat_map(|u| {
            let a: Vec<usize> = en.body_of(u.unit).map(|p| p.seq).collect();
            let b: Vec<usize> = zh.body_of(u.other_unit.unwrap()).map(|p| p.seq).collect();
            (0..a.len() - d).map(|i| (a[i], b[i + d])).collect::<Vec<_>>()
        }).filter_map(|(a, b)| similarity(a, b)).collect()
    };
    println!("== 余弦相似度（段数相同的章）：对着的两段 p10 {:.2} / p50 {:.2} / p90 {:.2}；同一章里隔半章的两段 p10 {:.2} / p50 {:.2} / p90 {:.2}", percentile(&mut matched, 0.1), percentile(&mut matched, 0.5), percentile(&mut matched, 0.9), percentile(&mut apart, 0.1), percentile(&mut apart, 0.5), percentile(&mut apart, 0.9));
    for d in [1, 2, 3] {
        let mut v = offset(d);
        let mean = v.iter().sum::<f32>() / v.len().max(1) as f32;
        let deviation = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len().max(1) as f32).sqrt();
        println!("   错开 {d} 段的两段：p10 {:.2} / p50 {:.2} / p90 {:.2} / p99 {:.2}；平均 {mean:.2}，标准差 {deviation:.2}", percentile(&mut v, 0.1), percentile(&mut v, 0.5), percentile(&mut v, 0.9), percentile(&mut v, 0.99));
    }
    println!("   对着的两段 p1 {:.2} / p5 {:.2}", percentile(&mut matched, 0.01), percentile(&mut matched, 0.05));

    let with = align::align_with(&en, &zh, Some(&similarity), &opt);
    summary("长度、锚点加语义相似度", &with, &en, &zh);
    // 有相似度时，内容对不上的单元（中文版的附录只有几行说明）会如实标成没对上；章都要对上
    assert!(with.mismatched().iter().all(|u| en.units[u.unit].kind != UnitKind::Chapter), "没对上的章：{:?}", with.mismatched().iter().map(|u| &u.key).collect::<Vec<_>>());
    // 两种做法在算法对的章里有多少段对法一样
    let fitted_paragraphs: Vec<usize> = plain.units.iter().filter(|u| u.method == Method::Fitted).flat_map(|u| en.body_of(u.unit).map(|p| p.seq).collect::<Vec<_>>()).collect();
    let same = fitted_paragraphs.iter().filter(|s| plain.other_of(**s) == with.other_of(**s)).count();
    println!("   两种做法对法一样的段：{same}/{}（{:.1}%）", fitted_paragraphs.len(), 100.0 * same as f32 / fitted_paragraphs.len().max(1) as f32);
    let sims = |xs: &Text, ys: &Text| (vectors.table(xs), vectors.table(ys));

    println!("== 改乱再找回：不同权重");
    let with_skip = |similarity: f32, skip: f32| Options { similarity, skip, ..Options::default() };
    let grid: Vec<(&str, Options)> = vec![
        ("只用长度和锚点，落空 4", with_skip(0.0, 4.0)),
        ("只用长度和锚点，落空 5（默认）", with_skip(0.0, 5.0)),
        ("只用长度和锚点，落空 6", with_skip(0.0, 6.0)),
        ("只用长度和锚点，落空 7", with_skip(0.0, 7.0)),
        ("只用长度和锚点，落空 5，合并 4 / 8", Options { merge: 4.0, merge3: 8.0, ..with_skip(0.0, 5.0) }),
        ("只用长度和锚点，落空 5，合并 2 / 4", Options { merge: 2.0, merge3: 4.0, ..with_skip(0.0, 5.0) }),
        ("只用长度和锚点，落空 5，不看锚点", Options { anchor: 0.0, ..with_skip(0.0, 5.0) }),
        ("只用长度和锚点，落空 5，锚点 2", Options { anchor: 2.0, ..with_skip(0.0, 5.0) }),
        ("只用长度和锚点，落空 5，长度尺度收紧到实测", Options { len_linear: 1.05, len_ratio: 0.046, ..with_skip(0.0, 5.0) }),
        ("只用长度和锚点，落空 5，长度尺度放宽一倍", Options { len_linear: 3.0, len_ratio: 0.16, ..with_skip(0.0, 5.0) }),
        ("相似度 0.5，落空 5", with_skip(0.5, 5.0)),
        ("相似度 1，落空 4", with_skip(1.0, 4.0)),
        ("相似度 1，落空 5（默认）", with_skip(1.0, 5.0)),
        ("相似度 1，落空 6", with_skip(1.0, 6.0)),
        ("相似度 1，落空 7", with_skip(1.0, 7.0)),
        ("相似度 1.5，落空 5", with_skip(1.5, 5.0)),
        ("相似度 2，落空 5", with_skip(2.0, 5.0)),
        ("相似度 1，落空 5，合并 4 / 8", Options { merge: 4.0, merge3: 8.0, ..with_skip(1.0, 5.0) }),
        ("相似度 1，落空 5，合并 2 / 4", Options { merge: 2.0, merge3: 4.0, ..with_skip(1.0, 5.0) }),
        ("相似度 1，落空 5，不看锚点", Options { anchor: 0.0, ..with_skip(1.0, 5.0) }),
    ];
    // 合并的段落：每一半和合起来的那一段有多像（定「一组里最不像的那一对」的门槛用）
    {
        let (_, x, y, truths) = &experiments[1];
        let (xv, yv) = sims(x, y);
        let mut halves: Vec<f32> = Vec::new();
        for (c, truth) in truths.iter().enumerate() {
            let xs: Vec<usize> = x.body_of(c).map(|p| p.seq).collect();
            let ys: Vec<usize> = y.body_of(c).map(|p| p.seq).collect();
            for (ta, tb) in truth.iter().filter(|(ta, tb)| ta.len() + tb.len() == 3) {
                let least = ta.iter().flat_map(|i| tb.iter().map(move |j| (*i, *j))).filter_map(|(i, j)| Some(cosine(xv[xs[i]]?, yv[ys[j]]?))).fold(f32::INFINITY, f32::min);
                halves.push(least);
            }
        }
        println!("   合并的两段里较不像的那一半：p10 {:.2} / p50 {:.2} / p90 {:.2}（{} 组）", percentile(&mut halves, 0.1), percentile(&mut halves, 0.5), percentile(&mut halves, 0.9), halves.len());
    }
    for (name, x, y, truths) in &experiments {
        let (xv, yv) = sims(x, y);
        let similarity = |a: usize, b: usize| Some(cosine(xv[a]?, yv[b]?));
        println!("   {name}");
        for (label, o) in &grid {
            let mut score = Score::default();
            let al = if o.similarity == 0.0 { align::align_with(x, y, None, o) } else { align::align_with(x, y, Some(&similarity), o) };
            grade(&al, x, y, truths, &mut score);
            let low = al.low().len();
            println!("      {label}：{}；置信度低 {low} 章，没对上 {} 章", score.line(), al.mismatched().len());
        }
    }
}
