// 排版要的样式与语言文件：内置的打包进 App（来源与版本见 SOURCES.md），第一次排版时才加载；
// 用户导入的 .csl 在数据目录里，向 Rust 要（依赖样式由 Rust 换成母样式，decisions/0017）。
import { invoke } from "@tauri-apps/api/core";
import type { StyleSource } from "./engine.ts";
import { builtinStyle, LOCALE_FILES } from "./styles.ts";

const csl = import.meta.glob<string>("./csl/*.csl", { query: "?raw", import: "default" });
const localeFiles = import.meta.glob<string>("./locales/*.xml", { query: "?raw", import: "default" });

export async function styleSource(id: string): Promise<StyleSource> {
  const s = builtinStyle(id);
  if (!s) {
    const r = await invoke<{ xml: string; lang: string | null }>("cite_style_source", { id });
    return { xml: r.xml, lang: r.lang ?? undefined };
  }
  const load = csl[`./csl/${s.file}`];
  if (!load) throw new Error(`缺少样式文件：${s.file}`);
  return { xml: await load(), locatorAfterBrackets: s.locator === "after-brackets" };
}

let cached: Record<string, string> | null = null;

export async function locales(): Promise<Record<string, string>> {
  if (cached) return cached;
  const out: Record<string, string> = {};
  for (const [lang, file] of Object.entries(LOCALE_FILES)) out[lang] = await localeFiles[`./locales/${file}`]();
  cached = out;
  return out;
}
