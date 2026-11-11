// citeproc-js 封装：给定样式、语言文件、文献与按文档顺序排好的引文，
// 一次性重建整篇文档的引文文字与参考文献表（消歧、编号都依赖全文上下文）。
// 只排版、不做业务判断：哪些引文要排、缺数据怎么办、写回 Word 都在 Rust（decisions/0007）。

import CSL, { type CslCitation, type CslEngine } from "citeproc";
import { htmlToRuns } from "./runs.ts";
import { normalizeLocale } from "./styles.ts";
import type { CiteItemRef, CslItem, Rendered, RenderRequest, Run } from "./types.ts";

/** 一个样式排版要的东西：CSL 全文、（依赖样式指定的）语言、Pith 补的规则 */
export type StyleSource = {
  xml: string;
  /** 依赖样式的 default-locale：强制用这个语言 */
  lang?: string;
  /** GB/T 7714 顺序编码：同一文献再引时页码标在方括号外「[2]17」（官方 CSL 不输出引文页码） */
  locatorAfterBrackets?: boolean;
};

export type RenderInput = {
  styleXml: string;
  lang?: string;
  locatorAfterBrackets?: boolean;
  locales: Record<string, string>;
  items: Record<string, CslItem>;
  citations: { id: string; items: CiteItemRef[] }[];
};

export type HtmlBibliography = {
  entries: string[];
  itemIds: string[];
  hangingIndent: boolean;
  secondFieldAlign: false | "flush" | "margin";
  maxOffset: number;
};

export type HtmlResult = { citations: Map<string, string>; bibliography: HtmlBibliography | null };

export const PROCESSOR_VERSION: string = CSL.PROCESSOR_VERSION;

/** citeproc 的原始 HTML 输出 */
export function renderHtml(input: RenderInput): HtmlResult {
  const sys = {
    retrieveLocale: (lang: string) => input.locales[normalizeLocale(lang)],
    retrieveItem: (id: string) => {
      const item = input.items[id];
      if (!item) throw new Error(`缺少文献数据：${id}`);
      // citeproc 会在条目上挂内部字段，给它副本，避免污染调用方数据
      return structuredClone(item);
    },
  };
  const engine = input.lang ? new CSL.Engine(sys, input.styleXml, input.lang, true) : new CSL.Engine(sys, input.styleXml);
  const cits: CslCitation[] = input.citations.map((c) => ({ citationID: c.id, citationItems: c.items.map(citeItem), properties: { noteIndex: 0 } }));
  if (!cits.length) {
    return { citations: new Map(), bibliography: { entries: [], itemIds: [], hangingIndent: false, secondFieldAlign: false, maxOffset: 0 } };
  }
  const out = engine.rebuildProcessorState(cits, "html");
  const citations = new Map<string, string>(out.map(([id, , html]) => [id, html]));
  if (input.locatorAfterBrackets) {
    for (const c of input.citations) {
      const html = citations.get(c.id);
      if (html !== undefined && c.items.some((i) => i.locator)) citations.set(c.id, locatorsAfterBrackets(engine, c.items, html));
    }
  }
  const bib = engine.makeBibliography();
  if (!bib) return { citations, bibliography: null };
  const [p, entries] = bib;
  return {
    citations,
    bibliography: {
      entries,
      itemIds: p.entry_ids.map((ids) => ids[0]),
      hangingIndent: !!p.hangingindent,
      secondFieldAlign: p["second-field-align"],
      maxOffset: p.maxoffset,
    },
  };
}

const str = (v: unknown) => (typeof v === "string" && v !== "" ? v : undefined);

/** 引文里的一条文献 → citeproc：页码、前后缀、不显示作者 / 只显示作者（Zotero 转换来的会带） */
function citeItem(i: CiteItemRef): CslCitation["citationItems"][number] {
  const out: CslCitation["citationItems"][number] = { id: i.id };
  if (i.locator) {
    out.locator = i.locator;
    out.label = i.label ?? "page";
  }
  const prefix = str(i.prefix);
  const suffix = str(i.suffix);
  if (prefix) out.prefix = prefix;
  if (suffix) out.suffix = suffix;
  if (i["suppress-author"] === true) out["suppress-author"] = true;
  if (i["author-only"] === true) out["author-only"] = true;
  return out;
}

const esc = (s: string) => s.replace(/&/g, "&#38;").replace(/</g, "&#60;").replace(/>/g, "&#62;");

/** 编号列表 → 「1,2」「4–6」（与 citeproc 的 collapse 一致：连续 3 个以上用连接号） */
export function collapseNumbers(nums: number[]): string {
  const out: string[] = [];
  let i = 0;
  while (i < nums.length) {
    let j = i;
    while (j + 1 < nums.length && nums[j + 1] === nums[j] + 1) j++;
    if (j - i >= 2) out.push(`${nums[i]}–${nums[j]}`);
    else for (let k = i; k <= j; k++) out.push(String(nums[k]));
    i = j + 1;
  }
  return out.join(",");
}

/**
 * GB/T 7714-2015 10.1.3：同一文献多次引用时，页码标在方括号外，如「[2]17」。
 * 编号用 citeproc 自己排的（registry 里的 seq），所以与书目编号一致。
 * 一处引文里有多条文献时：没有页码的照常合在一个方括号里，有页码的各自「[n]页码」，按编号排、用「,」连。
 */
function locatorsAfterBrackets(engine: CslEngine, items: CiteItemRef[], html: string): string {
  const parts = items
    .map((i) => ({ n: engine.registry.registry[i.id]?.seq ?? 0, loc: i.locator ?? "", affix: !!(str(i.prefix) || str(i.suffix)), item: i }))
    .sort((a, b) => a.n - b.n);
  const pieces: string[] = [];
  let plain: number[] = [];
  const flush = () => {
    if (plain.length) pieces.push(`[${collapseNumbers(plain)}]`);
    plain = [];
  };
  for (const p of parts) {
    if (!p.loc && !p.affix) {
      plain.push(p.n);
      continue;
    }
    flush();
    pieces.push(`[${str(p.item.prefix) ?? ""}${p.n}${str(p.item.suffix) ?? ""}]${p.loc}`);
  }
  flush();
  const text = esc(pieces.join(","));
  return html.trimStart().startsWith("<sup>") ? `<sup>${text}</sup>` : text;
}

/** Rust 的排版请求 → 带格式的片段 */
export function render(req: RenderRequest, style: StyleSource, locales: Record<string, string>): Rendered {
  const r = renderHtml({ styleXml: style.xml, lang: style.lang, locatorAfterBrackets: style.locatorAfterBrackets, locales, items: req.items, citations: req.citations });
  const citations: Record<string, Run[]> = {};
  for (const [id, html] of r.citations) citations[id] = htmlToRuns(html);
  const b = r.bibliography;
  return {
    citations,
    bibliography: b && { entries: b.entries.map(htmlToRuns), hangingIndent: b.hangingIndent, secondFieldAlign: b.secondFieldAlign, maxOffset: b.maxOffset },
  };
}

