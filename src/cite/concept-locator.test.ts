/// <reference types="node" />
// v0.4：卡片原文的引文定位在 GB/T 7714 和 APA 7 下怎么显示（decisions/0024「Word 引文」：C3 各排一遍定下来）。
// 英文版有页码：locator 是「页码 (段落号)」、label page；中文版没有页码：locator 是段落号、label paragraph。
import { test } from "node:test";
import assert from "node:assert/strict";
import { render } from "./engine.ts";
import { runsToText } from "./runs.ts";
import { locales, styleSource } from "./testing.ts";
import { conceptCite } from "../picker/conceptLocator.ts";
import type { CslItem } from "./types.ts";

const ITEMS: Record<string, CslItem> = {
  en: { id: "en", type: "book", title: "The Wealth of Nations: Books I–III", author: [{ family: "Smith", given: "Adam" }], editor: [{ family: "Skinner", given: "Andrew" }], publisher: "Penguin Books", "publisher-place": "London", issued: { "date-parts": [[1999]] }, language: "en" } as CslItem,
  zh: { id: "zh", type: "book", title: "国富论", author: [{ family: "斯密", given: "亚当" }], translator: [{ literal: "杨敬年" }], publisher: "陕西人民出版社", "publisher-place": "西安", issued: { "date-parts": [[2011]] }, language: "zh-CN" } as CslItem,
};

const quote = (lang: string, pages: string | null) => ({ runUid: "R", card: "K1", cardTitle: "x", text: "t", language: lang, paragraph: "I.i.3", pages, zh: null, zhParagraph: null });

function cite(style: string, id: string, ref: Record<string, unknown>) {
  const r = render({ citations: [{ id: "c1", items: [{ ...ref, id }] }], items: ITEMS } as never, styleSource(style), locales());
  return runsToText(r.citations["c1"]);
}

test("英文带页码和段落号、中文只有段落号：APA 7 与 GB/T 7714", () => {
  const enPage = conceptCite({ id: "en" }, quote("en", "110"), "page");
  assert.deepEqual([enPage.label, enPage.locator], ["page", "110 (I.i.3)"]);
  const enSpan = conceptCite({ id: "en" }, quote("en", "110–111"), "page");
  const enPara = conceptCite({ id: "en" }, quote("en", "110"), "paragraph");
  const zh = conceptCite({ id: "zh" }, quote("zh", null), "page");
  assert.deepEqual([zh.label, zh.locator], ["paragraph", "I.i.3"], "没有页码的版本选「页」也只给段落号");
  assert.equal((enPage.pithQuote as { text: string }).text, "t");

  assert.equal(cite("apa", "en", enPage), "(Smith, 1999, p. 110 (I.i.3))");
  assert.equal(cite("apa", "en", enSpan), "(Smith, 1999, pp. 110–111 (I.i.3))");
  assert.equal(cite("apa", "en", enPara), "(Smith, 1999, para. I.i.3)");
  assert.equal(cite("apa", "zh", zh), "(斯密, 2011, para. I.i.3)");

  assert.equal(cite("gbt7714-2015-numeric", "en", enPage), "[1]110 (I.i.3)");
  assert.equal(cite("gbt7714-2015-numeric", "zh", zh), "[1]I.i.3");
  // GB/T 著者-出版年制：这份样式的引文里不出定位（页码要写在引文后面的话，C4 在 Word 里再看要不要改样式）
  assert.equal(cite("gbt7714-2015-author-date", "en", enPage), "(Smith, 1999)");
  assert.equal(cite("gbt7714-2015-author-date", "zh", zh), "(斯密, 2011)");
});
