import { test } from "node:test";
import assert from "node:assert/strict";
import { CITE_TAG_PREFIX, planRefresh, type DocCitation } from "../src/refresh.ts";
import { runsSignature } from "../src/runs.ts";
import { emptyStore, type DocStore } from "../src/store.ts";
import { idGen, library, locales, styleXml } from "./helpers.ts";

const NOW = "2026-11-07T12:00:00Z";

function storeWith(style: string, cites: Record<string, string[]>): DocStore {
  const s = emptyStore(style, NOW);
  for (const [id, items] of Object.entries(cites)) s.citations[id] = { items: items.map((i) => ({ id: i })), created: NOW };
  return s;
}

const tags = (...ids: string[]): DocCitation[] => ids.map((id) => ({ tag: CITE_TAG_PREFIX + id, signature: "" }));

function plan(store: DocStore, doc: DocCitation[], lib = library()) {
  return planRefresh({ doc, store, library: lib, styleXml: styleXml(store.style), locales: locales(), newId: idGen(), now: NOW });
}

test("首次刷新：全部改写，书目按首次出现编号", () => {
  const s = storeWith("gbt7714-2015-numeric", { a: ["rl-item-0002"], b: ["rl-item-0001"], c: ["rl-item-0002"] });
  const p = plan(s, tags("a", "b", "c"));
  assert.deepEqual(p.citations.map((c) => c.text), ["[1]", "[2]", "[1]"]);
  assert.ok(p.citations.every((c) => c.changed));
  assert.equal(p.bibliography.texts.length, 2);
  assert.match(p.bibliography.texts[0], /^\[1\]\tHALVORSEN I\./);
  assert.ok(p.bibliography.format.hangingIndentPt > 0);
  // 文献快照写进文档数据
  assert.ok(p.store.items["rl-item-0001"] && p.store.items["rl-item-0002"]);
});

test("在最前面插入新引文：后面的编号整体后移", () => {
  const s = storeWith("gbt7714-2015-numeric", { a: ["rl-item-0002"], b: ["rl-item-0001"], n: ["rl-item-0004"] });
  const before = plan(s, tags("a", "b"));
  const doc = tags("a", "b").map((d, i) => ({ ...d, signature: runsSignature(before.citations[i].runs) }));
  const after = plan(s, [{ tag: CITE_TAG_PREFIX + "n", signature: "" }, ...doc]);
  assert.deepEqual(after.citations.map((c) => c.text), ["[1]", "[2]", "[3]"]);
  assert.ok(after.citations.every((c) => c.changed));
  assert.match(after.bibliography.texts[0], /^\[1\]\t张伟\./);
});

test("没变化的引文不改写", () => {
  const s = storeWith("apa", { a: ["rl-item-0002"] });
  const first = plan(s, tags("a"));
  const second = plan(first.store, [{ tag: CITE_TAG_PREFIX + "a", signature: runsSignature(first.citations[0].runs) }]);
  assert.equal(second.citations[0].changed, false);
});

test("删除引文：书目去掉只被它引用的文献，数据保留以便撤销", () => {
  const s = storeWith("apa", { a: ["rl-item-0002"], b: ["rl-item-0004"] });
  const p = plan(s, tags("a"));
  assert.equal(p.bibliography.texts.length, 1);
  assert.ok(p.store.citations["b"], "被删引文的数据仍在");
});

test("复制粘贴造成的重复 ID 会重新分配", () => {
  const s = storeWith("apa", { a: ["rl-item-0002"] });
  const p = plan(s, tags("a", "a"));
  assert.equal(p.citations[1].retag, CITE_TAG_PREFIX + "dup-1");
  assert.ok(p.store.citations["dup-1"]);
  assert.equal(p.warnings.length, 1);
});

test("缺少数据的控件跳过并说明原因", () => {
  const s = storeWith("apa", { a: ["rl-item-0002"] });
  const p = plan(s, [...tags("a", "ghost"), { tag: "some-other-control", signature: "" }]);
  assert.equal(p.citations.length, 1);
  assert.equal(p.skipped.length, 2);
});

test("文献库没有该条目时用文档快照；库里有更新时刷新快照", () => {
  const s = storeWith("apa", { a: ["rl-item-0002"] });
  const first = plan(s, tags("a"));
  const offline = plan(first.store, tags("a"), {});
  assert.equal(offline.citations[0].text, "(Halvorsen, 2018)");
  const lib = library();
  lib["rl-item-0002"] = { ...lib["rl-item-0002"], issued: { "date-parts": [[2019]] } };
  const updated = plan(first.store, tags("a"), lib);
  assert.equal(updated.citations[0].text, "(Halvorsen, 2019)");
});

test("文献两边都找不到：警告并跳过", () => {
  const s = storeWith("apa", { a: ["no-such-item"] });
  const p = plan(s, tags("a"), {});
  assert.equal(p.citations.length, 0);
  assert.equal(p.skipped[0].reason, "引文里的文献全部缺失");
  assert.equal(p.warnings.length, 1);
});
