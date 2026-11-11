/// <reference types="node" />
// 测试用（node --test）：直接从磁盘读样式、语言文件与合成文献（App 里由 resources.ts 经 Vite 加载）。
import { readFileSync } from "node:fs";
import { LOCALE_FILES, styleById } from "./styles.ts";
import type { StyleSource } from "./engine.ts";
import type { CslItem } from "./types.ts";

const here = new URL("./", import.meta.url);

export const styleXml = (id: string) => readFileSync(new URL(`csl/${styleById(id).file}`, here), "utf8");

/** 内置样式的 StyleSource（App 里由 resources.ts 给） */
export const styleSource = (id: string): StyleSource => ({ xml: styleXml(id), locatorAfterBrackets: styleById(id).locator === "after-brackets" });

export const locales = () =>
  Object.fromEntries(Object.entries(LOCALE_FILES).map(([k, f]) => [k, readFileSync(new URL(`locales/${f}`, here), "utf8")]));

/** 与 Rust 测试同一份合成文献（crates/docx-engine/src/cite/refs.json） */
export function library(): Record<string, CslItem> {
  const list = JSON.parse(readFileSync(new URL("../../crates/docx-engine/src/cite/refs.json", here), "utf8")) as CslItem[];
  return Object.fromEntries(list.map((i) => [i.id, i]));
}
