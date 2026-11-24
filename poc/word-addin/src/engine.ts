// citeproc-js 封装：给定样式、语言文件、文献与按文档顺序排好的引文，
// 一次性重建整篇文档的引文文字与参考文献表（消歧、编号都依赖全文上下文）。

import CSL from "citeproc";
import type { CiteItemRef, CslItem } from "./types.ts";
import { normalizeLocale } from "./styles.ts";

export type RenderCitation = { citationID: string; items: CiteItemRef[] };

export type RenderInput = {
  styleXml: string;
  locales: Record<string, string>;
  items: Record<string, CslItem>;
  citations: RenderCitation[];
};

export type Bibliography = {
  entries: string[];
  itemIds: string[];
  hangingIndent: boolean;
  secondFieldAlign: false | "flush" | "margin";
  maxOffset: number;
  lineSpacing: number;
  entrySpacing: number;
};

export type RenderResult = {
  citations: Map<string, string>;
  bibliography: Bibliography | null;
};

export const PROCESSOR_VERSION: string = CSL.PROCESSOR_VERSION;

export function render(input: RenderInput): RenderResult {
  const sys = {
    retrieveLocale: (lang: string) => input.locales[normalizeLocale(lang)],
    retrieveItem: (id: string) => {
      const item = input.items[id];
      if (!item) throw new Error(`缺少文献数据：${id}`);
      // citeproc 会在条目上挂内部字段，给它副本，避免污染调用方数据
      return structuredClone(item);
    },
  };
  const engine = new CSL.Engine(sys, input.styleXml);
  const cits = input.citations.map((c) => ({
    citationID: c.citationID,
    citationItems: c.items.map((i) =>
      i.locator ? { id: i.id, locator: i.locator, label: i.label ?? "page" } : { id: i.id },
    ),
    properties: { noteIndex: 0 },
  }));
  const out = cits.length ? engine.rebuildProcessorState(cits, "html") : [];
  const citations = new Map<string, string>(out.map(([id, , html]) => [id, html]));

  let bibliography: Bibliography | null = null;
  if (cits.length) {
    const bib = engine.makeBibliography();
    if (bib) {
      const [p, entries] = bib;
      bibliography = {
        entries,
        itemIds: p.entry_ids.map((ids) => ids[0]),
        hangingIndent: !!p.hangingindent,
        secondFieldAlign: p["second-field-align"],
        maxOffset: p.maxoffset,
        lineSpacing: p.linespacing,
        entrySpacing: p.entryspacing,
      };
    }
  } else {
    bibliography = {
      entries: [],
      itemIds: [],
      hangingIndent: false,
      secondFieldAlign: false,
      maxOffset: 0,
      lineSpacing: 1,
      entrySpacing: 0,
    };
  }
  return { citations, bibliography };
}
