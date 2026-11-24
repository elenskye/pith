// 刷新计划：输入「文档里按顺序出现的引文控件」与文档数据，输出每处引文和书目应有的内容。
// 纯函数，不碰 Word；Office.js、VBA 桥接和 docx 直写三条路都用它。

import { render } from "./engine.ts";
import { htmlToRuns, runsSignature, runsToText } from "./runs.ts";
import type { DocStore } from "./store.ts";
import type { CiteItemRef, CslItem, Run } from "./types.ts";

export const CITE_TAG_PREFIX = "rl:cite:";
export const BIB_TAG = "rl:bibliography";

export type DocCitation = {
  tag: string;
  /** 文档里现有的文字签名（runsSignature 格式），用于判断是否需要改写。 */
  signature: string;
};

export type CitationUpdate = {
  index: number;
  citationID: string;
  /** 文档里有重复 ID（复制粘贴产生）时给新 ID，调用方要改控件的 tag。 */
  retag: string | null;
  runs: Run[];
  text: string;
  changed: boolean;
};

export type BibFormat = { hangingIndentPt: number };

export type Plan = {
  citations: CitationUpdate[];
  skipped: { index: number; tag: string; reason: string }[];
  bibliography: { entries: Run[][]; texts: string[]; format: BibFormat };
  store: DocStore;
  warnings: string[];
};

export type PlanInput = {
  doc: DocCitation[];
  store: DocStore;
  /** 文献库里的最新数据；没有的条目退回文档快照。 */
  library: Record<string, CslItem>;
  styleXml: string;
  locales: Record<string, string>;
  newId: () => string;
  now: string;
};

export function citationIdOf(tag: string): string | null {
  return tag.startsWith(CITE_TAG_PREFIX) ? tag.slice(CITE_TAG_PREFIX.length) : null;
}

export function planRefresh(input: PlanInput): Plan {
  const store: DocStore = structuredClone(input.store);
  const warnings: string[] = [];
  const skipped: Plan["skipped"] = [];
  const seen = new Set<string>();
  const ordered: { index: number; citationID: string; retag: string | null; items: CiteItemRef[] }[] = [];

  input.doc.forEach((dc, index) => {
    let id = citationIdOf(dc.tag);
    if (!id) {
      skipped.push({ index, tag: dc.tag, reason: "不是引文控件" });
      return;
    }
    const data = store.citations[id];
    if (!data) {
      skipped.push({ index, tag: dc.tag, reason: "文档里缺少这处引文的数据（可能是从别的文档粘贴来的）" });
      return;
    }
    let retag: string | null = null;
    if (seen.has(id)) {
      const nid = input.newId();
      store.citations[nid] = structuredClone(data);
      retag = CITE_TAG_PREFIX + nid;
      warnings.push(`第 ${index + 1} 处引文与前面重复（复制粘贴），已分配新 ID`);
      id = nid;
    }
    seen.add(id);
    const items: CiteItemRef[] = [];
    for (const ref of store.citations[id].items) {
      const lib = input.library[ref.id];
      if (lib) store.items[ref.id] = structuredClone(lib);
      if (store.items[ref.id]) items.push(ref);
      else warnings.push(`第 ${index + 1} 处引文：文献 ${ref.id} 在文献库和文档里都找不到`);
    }
    if (!items.length) {
      skipped.push({ index, tag: dc.tag, reason: "引文里的文献全部缺失" });
      return;
    }
    ordered.push({ index, citationID: id, retag, items });
  });

  const result = render({
    styleXml: input.styleXml,
    locales: input.locales,
    items: store.items,
    citations: ordered.map((o) => ({ citationID: o.citationID, items: o.items })),
  });

  const citations: CitationUpdate[] = ordered.map((o) => {
    const html = result.citations.get(o.citationID) ?? "";
    const runs = htmlToRuns(html);
    return {
      index: o.index,
      citationID: o.citationID,
      retag: o.retag,
      runs,
      text: runsToText(runs),
      changed: o.retag !== null || runsSignature(runs) !== input.doc[o.index].signature,
    };
  });

  const bib = result.bibliography;
  const entries = (bib?.entries ?? []).map(htmlToRuns);
  const hangingIndentPt = !bib
    ? 0
    : bib.hangingIndent
      ? 36
      : bib.secondFieldAlign
        ? Math.max(21, (bib.maxOffset + 1) * 7)
        : 0;

  store.updated = input.now;
  return {
    citations,
    skipped,
    bibliography: { entries, texts: entries.map(runsToText), format: { hangingIndentPt } },
    store,
    warnings,
  };
}
