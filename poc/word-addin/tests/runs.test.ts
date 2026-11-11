import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeEntities, htmlToRuns, runsSignature, runsToText } from "../src/runs.ts";

test("实体解码", () => {
  assert.equal(decodeEntities("A &#38; B &amp; &lt;x&gt; &#x4E2D;&nbsp;"), "A & B & <x> 中 ");
});

test("斜体与实体", () => {
  const runs = htmlToRuns('<div class="csl-entry">Halvorsen, I. (2018). <i>The architecture</i>. A &#38; B.</div>');
  assert.deepEqual(runs, [
    { text: "Halvorsen, I. (2018). " },
    { text: "The architecture", italic: true },
    { text: ". A & B." },
  ]);
});

test("上标引文", () => {
  assert.deepEqual(htmlToRuns("<sup>[2,3]</sup>"), [{ text: "[2,3]", sup: true }]);
  assert.equal(runsSignature(htmlToRuns("<sup>[2,3]</sup>")), "^{[2,3]}");
});

test("顺序编码书目：编号后接制表符，去掉块间缩进空白", () => {
  const html =
    '<div class="csl-entry">\n    <div class="csl-left-margin">[1]</div><div class="csl-right-inline">LINDQVIST M. Title[J]. 2021.</div>\n  </div>';
  const runs = htmlToRuns(html);
  assert.equal(runsToText(runs), "[1]\tLINDQVIST M. Title[J]. 2021.");
});

test("斜体内的正体翻转、小型大写、加粗", () => {
  const runs = htmlToRuns(
    '<i>Title <span style="font-style:normal;">Inner</span> end</i> <span style="font-variant:small-caps;">Sc</span> <b>B</b>',
  );
  assert.deepEqual(runs, [
    { text: "Title ", italic: true },
    { text: "Inner" },
    { text: " end", italic: true },
    { text: " " },
    { text: "Sc", smallCaps: true },
    { text: " " },
    { text: "B", bold: true },
  ]);
});

test("不配对的关闭标签不崩溃", () => {
  assert.equal(runsToText(htmlToRuns("a</i>b<i>c")), "abc");
});
