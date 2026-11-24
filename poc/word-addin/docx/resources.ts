// Node 侧读取打包的样式、语言文件与合成文献（命令行、桥接与测试共用）。
import { readFileSync } from "node:fs";
import { LOCALE_FILES, styleById } from "../src/styles.ts";
import type { CslItem } from "../src/types.ts";

const root = new URL("../public/", import.meta.url);

export function styleXml(id: string): string {
  return readFileSync(new URL(`csl/${styleById(id).file}`, root), "utf8");
}

let localeCache: Record<string, string> | null = null;
export function locales(): Record<string, string> {
  localeCache ??= Object.fromEntries(
    Object.entries(LOCALE_FILES).map(([k, f]) => [k, readFileSync(new URL(`locales/${f}`, root), "utf8")]),
  );
  return localeCache;
}

export function libraryList(): CslItem[] {
  return JSON.parse(readFileSync(new URL("refs.json", root), "utf8")) as CslItem[];
}

export function library(): Record<string, CslItem> {
  return Object.fromEntries(libraryList().map((i) => [i.id, i]));
}
