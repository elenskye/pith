/// <reference types="node" />
import { test } from "node:test";
import assert from "node:assert/strict";
import { columnsFor, place } from "./masonry.ts";

test("列数：每列至少 240，最多 4 列", () => {
  assert.equal(columnsFor(1200).cols, 4);
  assert.equal(columnsFor(700).cols, 2);
  assert.equal(columnsFor(200).cols, 1);
  const { cols, colW } = columnsFor(516);
  assert.equal(cols, 2);
  assert.equal(colW, 249);
});

test("放进最矮的那一列，一样矮放左边", () => {
  const { pos, height } = place([100, 50, 30, 40], 2, 200);
  assert.deepEqual(pos, [
    { x: 0, y: 0 },
    { x: 218, y: 0 },
    { x: 218, y: 66 },
    { x: 218, y: 112 },
  ]);
  assert.equal(height, 152);
});

test("没有卡片时墙高 0", () => {
  assert.deepEqual(place([], 3, 100), { pos: [], height: 0 });
});
