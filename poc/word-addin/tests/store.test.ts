import { test } from "node:test";
import assert from "node:assert/strict";
import { emptyStore, parseStore, serializeStore, StoreError, STORE_NS } from "../src/store.ts";
import { parseXml } from "./helpers.ts";

test("序列化后能原样解析，特殊字符不丢", () => {
  const s = emptyStore("apa", "2026-11-07T00:00:00Z");
  s.items["x"] = { id: "x", type: "book", title: 'A <b> & "C" ]]> 中文' };
  s.citations["c1"] = { items: [{ id: "x", locator: "3" }], created: "2026-11-07T00:00:00Z" };
  const back = parseStore(serializeStore(s), parseXml);
  assert.deepEqual(back, s);
});

test("容忍 Word 改写前缀与空白", () => {
  const json = JSON.stringify({ style: "apa", items: {}, citations: {}, updated: "t" }).replace(/"/g, "&quot;");
  const xml = `<?xml version="1.0" standalone="no"?>\n<ns0:citations xmlns:ns0="${STORE_NS}" version="1">\n  <ns0:json>${json}</ns0:json>\n</ns0:citations>`;
  const s = parseStore(xml, parseXml);
  assert.equal(s.style, "apa");
});

test("数据版本比本程序新时拒绝修改", () => {
  const xml = `<rl:citations xmlns:rl="${STORE_NS}" version="2"><rl:json>{}</rl:json></rl:citations>`;
  assert.throws(() => parseStore(xml, parseXml), StoreError);
});

test("命名空间不对或 JSON 坏了都报错", () => {
  assert.throws(() => parseStore(`<x:citations xmlns:x="urn:other" version="1"/>`, parseXml), StoreError);
  assert.throws(
    () => parseStore(`<rl:citations xmlns:rl="${STORE_NS}" version="1"><rl:json>{oops</rl:json></rl:citations>`, parseXml),
    StoreError,
  );
});
