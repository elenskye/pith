// 回答的轻量 Markdown：一段文字拆成小标题、列表、普通的几行（Reply 照它画）。单独放一个文件，node --test 能直接测。
export const LIST = /^\s*([-*•]|\d+[.、)])\s+/;
/** 单独一行的小标题：# 开头，或者整行加粗（「**1. 核心论证**」） */
const headOf = (l: string) => /^#{1,4}\s+(.*)$/.exec(l)?.[1] ?? /^\*\*([^*]+)\*\*[:：]?$/.exec(l.trim())?.[1] ?? null;

export type Seg = { kind: "head"; text: string } | { kind: "list"; ordered: boolean; items: string[] } | { kind: "quote"; lines: string[] } | { kind: "p"; lines: string[] };

/** 一段文字拆成几截：小标题、列表（- 或 1.）、「>」引文、普通的几行。模型常常只用单个换行把它们隔开 */
export function segments(text: string): Seg[] {
  const out: Seg[] = [];
  for (const l of text.split("\n").filter((x) => x.trim())) {
    const last = out[out.length - 1];
    const head = headOf(l);
    const quote = /^\s*>\s?(.*)$/.exec(l);
    if (quote) {
      // 「>」引文：连着的几行是一段；空的「>」行只是分隔
      if (!quote[1].trim()) continue;
      if (last?.kind === "quote") last.lines.push(quote[1]);
      else out.push({ kind: "quote", lines: [quote[1]] });
    } else if (head !== null) out.push({ kind: "head", text: head });
    else if (LIST.test(l)) {
      const ordered = /^\s*\d/.test(l);
      const item = l.replace(LIST, "");
      if (last?.kind === "list" && last.ordered === ordered) last.items.push(item);
      else out.push({ kind: "list", ordered, items: [item] });
    } else if (last?.kind === "p") last.lines.push(l);
    else out.push({ kind: "p", lines: [l] });
  }
  return out;
}

