// 可选引文样式。CSL 文件与语言文件来自 citation-style-language 官方仓库，
// 版本与哈希见 public/csl/SOURCES.md。

export type StyleInfo = {
  id: string;
  label: string;
  file: string;
  kind: "author-date" | "numeric";
};

export const STYLES: StyleInfo[] = [
  { id: "apa", label: "APA 第 7 版", file: "apa.csl", kind: "author-date" },
  {
    id: "gbt7714-2015-numeric",
    label: "GB/T 7714-2015 顺序编码",
    file: "china-national-standard-gb-t-7714-2015-numeric.csl",
    kind: "numeric",
  },
  {
    id: "gbt7714-2015-author-date",
    label: "GB/T 7714-2015 著者-出版年",
    file: "china-national-standard-gb-t-7714-2015-author-date.csl",
    kind: "author-date",
  },
  {
    id: "gbt7714-2025-numeric",
    label: "GB/T 7714-2025 顺序编码",
    file: "china-national-standard-gb-t-7714-2025-numeric.csl",
    kind: "numeric",
  },
  {
    id: "chicago-author-date",
    label: "Chicago 第 18 版（作者-日期）",
    file: "chicago-author-date.csl",
    kind: "author-date",
  },
];

export const DEFAULT_STYLE = "apa";

export function styleById(id: string): StyleInfo {
  const s = STYLES.find((x) => x.id === id);
  if (!s) throw new Error(`未知样式：${id}`);
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
