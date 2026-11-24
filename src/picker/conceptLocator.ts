// Word 搜索窗里「卡片里的原文」（v0.4，decisions/0024「Word 引文」）：选一句卡片上的原文，定位可选「页」或「段」。
// - 有页码的版本（英文）：label = page，locator = 「页码 (段落号)」，如 110 (I.i.3)；跨页写 110–111 (I.i.3)；
// - 没有页码的（中文），或选了「段」：label = paragraph，locator = 段落号，如 I.i.3；
// - 原文本身放在引文数据的 pithQuote 里（CiteItemRef 多出来的字段原样存进文档的数据部件），宏在引文前面插入这句原文（C4 本地做）。
// 各样式怎么显示见 src/cite/concept-locator.test.ts。
import type { CiteRef, ConceptQuote } from "@/shared/ipc";

export type LocatorMode = "page" | "paragraph";

export type PithQuote = { text: string; lang: string; paragraph: string; pages: string | null; card: string };

export function conceptCite(base: CiteRef, q: ConceptQuote, mode: LocatorMode): CiteRef {
  const pithQuote: PithQuote = { text: q.text, lang: q.language, paragraph: q.paragraph, pages: q.pages, card: `${q.runUid}/${q.card}` };
  if (mode === "page" && q.pages) return { ...base, label: "page", locator: `${q.pages} (${q.paragraph})`, pithQuote };
  return { ...base, label: "paragraph", locator: q.paragraph, pithQuote };
}

/** 去掉原文（换成普通引文） */
export function withoutQuote(c: CiteRef): CiteRef {
  const { pithQuote: _q, ...rest } = c;
  return rest as CiteRef;
}
