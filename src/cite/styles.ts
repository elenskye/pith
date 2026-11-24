// 内置引文样式。清单只写一份：styles.json（Rust 的 word::styles 也读它）。
import list from "./styles.json" with { type: "json" };

export type StyleInfo = {
  id: string;
  label: string;
  en: string;
  file: string;
  /** CSL 文件里的 <id>：Zotero 文档设置里记的就是它（转换时对应样式用） */
  uri: string;
  kind: "numeric" | "author-date";
  /** after-brackets：页码标在方括号外「[2]17」（GB/T 7714 顺序编码） */
  locator?: "after-brackets";
};

export const STYLES = list as StyleInfo[];

export function builtinStyle(id: string): StyleInfo | undefined {
  return STYLES.find((x) => x.id === id);
}

export function styleById(id: string): StyleInfo {
  const s = builtinStyle(id);
  if (!s) throw new Error(`没有这个引文样式：${id}`);
  return s;
}

export const LOCALE_FILES: Record<string, string> = {
  "en-US": "locales-en-US.xml",
  "zh-CN": "locales-zh-CN.xml",
};

/** citeproc 可能请求 "zh"、"en-GB" 等，统一落到已打包的两个语言文件。 */
export function normalizeLocale(lang: string): string {
  return lang.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}
