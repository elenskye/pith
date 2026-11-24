import { test } from "node:test";
import assert from "node:assert/strict";
import { strFromU8 } from "fflate";
import { citationSdts, findStore, openDocx, saveDocx, unzipDocx, W } from "../docx/docx.ts";
import { refreshDocx, seedDocx, verifyDocx } from "../docx/ops.ts";
import { idGen, library } from "./helpers.ts";

const NOW = "2026-11-07T12:00:00Z";
const seed = (zotero = true) => seedDocx({ style: "apa", zotero, library: library(), now: NOW });

test("生成的测试文档自洽：引文、书目、数据部件都对", () => {
  const r = verifyDocx(seed(), { library: library() });
  assert.equal(r.ok, true, r.problems.join("\n"));
  assert.equal(r.citations.length, 4);
  assert.equal(r.bibliography.actual.length, 7);
  assert.equal(r.zotero.citations, 1);
});

test("数据部件按 OOXML 规范挂到文档上", () => {
  const files = unzipDocx(seed());
  const rels = strFromU8(files.get("word/_rels/document.xml.rels")!);
  assert.match(rels, /relationships\/customXml" Target="\.\.\/customXml\/item1\.xml"/);
  assert.match(strFromU8(files.get("[Content_Types].xml")!), /itemProps1\.xml/);
  assert.ok(files.has("customXml/_rels/item1.xml.rels"));
});

test("切换到 GB/T 7714 顺序编码：全部改写，Zotero 域原样保留", () => {
  const before = seed();
  const { bytes } = refreshDocx(before, { style: "gbt7714-2015-numeric", library: library(), now: NOW });
  const r = verifyDocx(bytes, { library: library() });
  assert.equal(r.ok, true, r.problems.join("\n"));
  assert.deepEqual(r.citations.map((c) => c.actual), ["[1]", "[2,3]", "[4,5]", "[6,7]"]);
  assert.equal(r.zotero.digest, verifyDocx(before, { library: library() }).zotero.digest);
});

test("手工改了引文文字会被校验发现", () => {
  const d = openDocx(seed(false));
  const t = citationSdts(d.doc)[0].el.getElementsByTagNameNS(W, "t")[0];
  t.textContent = "(Someone, 1999)";
  const r = verifyDocx(saveDocx(d), { library: library() });
  assert.equal(r.ok, false);
  assert.match(r.problems[0], /第 1 处引文不一致/);
});

test("删掉一处引文后书目过期，刷新后恢复一致", () => {
  const d = openDocx(seed(false));
  const sdt = citationSdts(d.doc)[3].el; // 引用 0007 与 0004 的那处
  sdt.parentNode!.removeChild(sdt);
  const stale = saveDocx(d);
  assert.equal(verifyDocx(stale, { library: library() }).ok, false);
  const { bytes, plan } = refreshDocx(stale, { library: library(), now: NOW });
  assert.equal(plan.bibliography.texts.length, 5);
  assert.equal(verifyDocx(bytes, { library: library() }).ok, true);
  // 被删引文的数据仍保留在文档里（Word 撤销后能恢复）
  const loc = findStore(unzipDocx(bytes));
  assert.ok(loc && "store" in loc && loc.store.citations["seed-d"]);
});

test("复制粘贴出的重复引文会被分配新 ID", () => {
  const d = openDocx(seed(false));
  const first = citationSdts(d.doc)[0].el;
  first.parentNode!.insertBefore(first.cloneNode(true), first.nextSibling);
  const { bytes, plan } = refreshDocx(saveDocx(d), { library: library(), now: NOW, newId: idGen("copy") });
  assert.equal(plan.citations[1].retag, "rl:cite:copy-1");
  const r = verifyDocx(bytes, { library: library() });
  assert.equal(r.ok, true, r.problems.join("\n"));
  assert.equal(new Set(r.citations.map((c) => c.tag)).size, 5);
});

test("生成的文档声明 Word 2013+ 兼容模式，Word 不会以兼容模式打开", () => {
  const files = unzipDocx(seed(false));
  assert.match(strFromU8(files.get("word/settings.xml")!), /compatibilityMode" w:uri="http:\/\/schemas\.microsoft\.com\/office\/word" w:val="15"/);
  assert.match(strFromU8(files.get("word/_rels/document.xml.rels")!), /relationships\/settings" Target="settings\.xml"/);
  assert.match(strFromU8(files.get("[Content_Types].xml")!), /\/word\/settings\.xml/);
});
