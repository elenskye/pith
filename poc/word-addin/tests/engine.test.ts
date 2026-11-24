import { test } from "node:test";
import assert from "node:assert/strict";
import { render } from "../src/engine.ts";
import { htmlToRuns, runsSignature, runsToText } from "../src/runs.ts";
import { library, locales, styleXml } from "./helpers.ts";

const CITES = [
  { citationID: "c1", items: [{ id: "rl-item-0001" }] },
  { citationID: "c2", items: [{ id: "rl-item-0003", locator: "17" }, { id: "rl-item-0002" }] },
  { citationID: "c3", items: [{ id: "rl-item-0005" }, { id: "rl-item-0006" }] },
  { citationID: "c4", items: [{ id: "rl-item-0004" }, { id: "rl-item-0007" }] },
];

function run(style: string) {
  return render({ styleXml: styleXml(style), locales: locales(), items: library(), citations: CITES });
}

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
    CITES.map((c) => runsSignature(htmlToRuns(r.citations.get(c.citationID) ?? ""))),
    ["^{[1]}", "^{[2,3]}", "^{[4,5]}", "^{[6,7]}"],
  );
  const bib = r.bibliography!;
  assert.deepEqual(bib.itemIds, [
    "rl-item-0001",
    "rl-item-0003",
    "rl-item-0002",
    "rl-item-0005",
    "rl-item-0006",
    "rl-item-0004",
    "rl-item-0007",
  ]);
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

test("没有引文时书目为空", () => {
  const r = render({ styleXml: styleXml("apa"), locales: locales(), items: library(), citations: [] });
  assert.equal(r.bibliography?.entries.length, 0);
});
