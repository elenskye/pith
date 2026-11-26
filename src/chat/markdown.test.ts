/// <reference types="node" />
// 回答的轻量 Markdown（1.1.0 第 5 期真实验收里发现：模型常用单个换行把小标题、列表、段落隔开）
import { test } from "node:test";
import assert from "node:assert/strict";
import { segments } from "./markdown.ts";

test("a block with a bold heading, a list and a plain line comes apart in order", () => {
  const t = "**1. 核心论证（第一篇第三章开头）**\n- 原话：“As it is…” [1]\n- 中译：“交换能力……” [2]\n白话：市场小就卖不掉。\n## 例子\n1. 搬运工\n2. 钉匠";
  assert.deepEqual(segments(t), [
    { kind: "head", text: "1. 核心论证（第一篇第三章开头）" },
    { kind: "list", ordered: false, items: ["原话：“As it is…” [1]", "中译：“交换能力……” [2]"] },
    { kind: "p", lines: ["白话：市场小就卖不掉。"] },
    { kind: "head", text: "例子" },
    { kind: "list", ordered: true, items: ["搬运工", "钉匠"] },
  ]);
});

test("plain lines stay one paragraph; bold inside a line is not a heading", () => {
  assert.deepEqual(segments("第一行 **要点** 在中间。\n第二行"), [{ kind: "p", lines: ["第一行 **要点** 在中间。", "第二行"] }]);
  assert.deepEqual(segments("**结论：** 斯密的理由是交换。"), [{ kind: "p", lines: ["**结论：** 斯密的理由是交换。"] }]);
  assert.deepEqual(segments("**结论：**"), [{ kind: "head", text: "结论：" }]);
});

test("quoted lines become one quote; blank > lines only separate", () => {
  assert.deepEqual(segments("> “Those ten persons…” [1]\n>\n> “But if they had all wrought separately…” [1]\n斯密接着说。"), [
    { kind: "quote", lines: ["“Those ten persons…” [1]", "“But if they had all wrought separately…” [1]"] },
    { kind: "p", lines: ["斯密接着说。"] },
  ]);
});
