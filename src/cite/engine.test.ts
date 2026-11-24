/// <reference types="node" />
import { test } from "node:test";
import assert from "node:assert/strict";
import { collapseNumbers, render, renderHtml } from "./engine.ts";
import { htmlToRuns, runsSignature, runsToText } from "./runs.ts";
import { STYLES } from "./styles.ts";
import { library, locales, styleSource, styleXml } from "./testing.ts";

const CITES = [
  { id: "c1", items: [{ id: "rl-item-0001" }] },
  { id: "c2", items: [{ id: "rl-item-0003", locator: "17" }, { id: "rl-item-0002" }] },
  { id: "c3", items: [{ id: "rl-item-0005" }, { id: "rl-item-0006" }] },
  { id: "c4", items: [{ id: "rl-item-0004" }, { id: "rl-item-0007" }] },
];

const run = (style: string) => renderHtml({ styleXml: styleXml(style), locales: locales(), items: library(), citations: CITES });
const text = (html: string | undefined) => runsToText(htmlToRuns(html ?? ""));

test("APA 7：作者-年份、同作者同年消歧、et al.、页码", () => {
  const r = run("apa");
  assert.equal(text(r.citations.get("c1")), "(Lindqvist & Okafor, 2021)");
  assert.equal(text(r.citations.get("c2")), "(Halvorsen, 2018; 王 & 李, 2020, p. 17)");
  assert.equal(text(r.citations.get("c3")), "(Lindqvist, 2021a, 2021b)");
  assert.equal(text(r.citations.get("c4")), "(Moreau et al., 2023; 张, 2019)");
  const bib = r.bibliography!;
  assert.equal(bib.entries.length, 7);
  assert.equal(bib.hangingIndent, true);
  const first = htmlToRuns(bib.entries[0]);
  assert.equal(runsToText(first), "Halvorsen, I. (2018). The architecture of evidence. Northbridge University Press.");
  assert.deepEqual(first[1], { text: "The architecture of evidence", italic: true });
});

test("GB/T 7714-2015 顺序编码：按首次出现编号、上标、4 位作者用「等」", () => {
  const r = run("gbt7714-2015-numeric");
  assert.deepEqual(
    CITES.map((c) => runsSignature(htmlToRuns(r.citations.get(c.id) ?? ""))),
    ["^{[1]}", "^{[2,3]}", "^{[4,5]}", "^{[6,7]}"],
  );
  const bib = r.bibliography!;
  assert.deepEqual(bib.itemIds, ["rl-item-0001", "rl-item-0003", "rl-item-0002", "rl-item-0005", "rl-item-0006", "rl-item-0004", "rl-item-0007"]);
  const texts = bib.entries.map((e) => runsToText(htmlToRuns(e)));
  assert.equal(texts[1], "[2]\t王晓明, 李华. 合成数据中的政治效能感测量[J]. 社会科学方法, 2020, 8(2): 15-29.");
  assert.match(texts[6], /^\[7\]\tMOREAU C, SATO K, ABEBE H, 等\./);
  assert.equal(bib.secondFieldAlign, "flush");
});

test("同一份数据切换样式，输出随样式改变", () => {
  const apa = run("apa");
  const gbt25 = run("gbt7714-2025-numeric");
  const chicago = run("chicago-author-date");
  assert.notEqual(text(apa.citations.get("c1")), text(gbt25.citations.get("c1")));
  assert.equal(text(chicago.citations.get("c1")), "(Lindqvist and Okafor 2021)");
});

test("交给 Rust 的结果：片段与书目参数（字段名与 Rust 的 Rendered 一致）", () => {
  const out = render({ style: "gbt7714-2015-numeric", items: library(), citations: CITES }, styleSource("gbt7714-2015-numeric"), locales());
  assert.deepEqual(out.citations.c1, [{ text: "[1]", sup: true }]);
  assert.deepEqual(out.citations.c2, [{ text: "[2]17,[3]", sup: true }], "有页码的那条页码在方括号外");
  assert.equal(out.bibliography?.entries.length, 7);
  assert.equal(out.bibliography?.secondFieldAlign, "flush");
  assert.equal(typeof out.bibliography?.maxOffset, "number");
  assert.equal(out.bibliography?.hangingIndent, false);
});

test("没有引文时书目为空", () => {
  const r = renderHtml({ styleXml: styleXml("apa"), locales: locales(), items: library(), citations: [] });
  assert.equal(r.bibliography?.entries.length, 0);
});

test("每个内置样式都能排", () => {
  for (const s of STYLES) {
    const r = run(s.id);
    assert.equal(r.citations.size, 4, s.id);
    assert.equal(r.bibliography?.entries.length, 7, s.id);
  }
});

test("GB/T 顺序编码：同一文献再引，页码在方括号外「[2]17」；编号与书目一致", () => {
  const cites = [
    { id: "a", items: [{ id: "rl-item-0001" }, { id: "rl-item-0002" }, { id: "rl-item-0003" }] },
    { id: "b", items: [{ id: "rl-item-0002", locator: "17" }] },
    { id: "c", items: [{ id: "rl-item-0004" }, { id: "rl-item-0001", locator: "3-5" }, { id: "rl-item-0002" }] },
    { id: "d", items: [{ id: "rl-item-0003" }] },
  ];
  for (const style of ["gbt7714-2015-numeric", "gbt7714-2025-numeric"]) {
    const src = styleSource(style);
    assert.equal(src.locatorAfterBrackets, true, style);
    const r = renderHtml({ styleXml: src.xml, locatorAfterBrackets: true, locales: locales(), items: library(), citations: cites });
    const t = (id: string) => runsSignature(htmlToRuns(r.citations.get(id) ?? ""));
    assert.equal(t("a"), "^{[1–3]}", `${style}：没有页码的照旧合并`);
    assert.equal(t("b"), "^{[2]17}", style);
    assert.equal(t("c"), "^{[1]3-5,[2,4]}", style);
    assert.equal(t("d"), "^{[3]}", style);
  }
  assert.equal(styleSource("apa").locatorAfterBrackets, false);
});

test("编号合并与 citeproc 一致：连续 3 个以上用连接号", () => {
  assert.equal(collapseNumbers([1, 2]), "1,2");
  assert.equal(collapseNumbers([1, 2, 3, 5, 7, 8, 9, 10]), "1–3,5,7–10");
  assert.equal(collapseNumbers([4]), "4");
});

test("前后缀、不显示作者（Zotero 转换来的引文会带）", () => {
  const r = renderHtml({
    styleXml: styleXml("apa"),
    locales: locales(),
    items: library(),
    citations: [
      { id: "a", items: [{ id: "rl-item-0002", locator: "3", prefix: "see", suffix: "for details" }] },
      { id: "b", items: [{ id: "rl-item-0002", "suppress-author": true }] },
    ],
  });
  assert.equal(text(r.citations.get("a")), "(see Halvorsen, 2018, p. 3 for details)");
  assert.equal(text(r.citations.get("b")), "(2018)");
});

test("中文全名（Rust 交来时已合成 literal）：APA 正文引文出全名", () => {
  const items = library();
  items["rl-item-0003"] = { ...items["rl-item-0003"], author: [{ literal: "王晓明" }, { literal: "李华" }] };
  const r = renderHtml({ styleXml: styleXml("apa"), locales: locales(), items, citations: [{ id: "a", items: [{ id: "rl-item-0003" }] }] });
  assert.equal(text(r.citations.get("a")), "(王晓明 & 李华, 2020)");
  const g = renderHtml({ styleXml: styleXml("gbt7714-2015-numeric"), locales: locales(), items, citations: [{ id: "a", items: [{ id: "rl-item-0003" }] }] });
  assert.match(runsToText(htmlToRuns(g.bibliography!.entries[0])), /^\[1\]\t王晓明, 李华\. 合成数据/, "GB/T 书目不变");
});
