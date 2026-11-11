// citeproc-js 的 HTML 输出 → 带格式的文字片段（Run）。
// 只认 citeproc 会产出的少数标签，其余标签忽略、文字保留。

import type { Run } from "./types.ts";

type Fmt = Omit<Run, "text">;
type Frame = { tag: string; cls: string; fmt: Fmt };

const NAMED: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

export function decodeEntities(s: string): string {
  return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, (m, e: string) => {
    if (e[0] === "#") {
      const code = e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : m;
    }
    return NAMED[e] ?? m;
  });
}

function applyTag(tag: string, attrs: string, fmt: Fmt): Fmt {
  const f = { ...fmt };
  switch (tag) {
    case "i":
    case "em":
      f.italic = true;
      break;
    case "b":
    case "strong":
      f.bold = true;
      break;
    case "sup":
      f.sup = true;
      break;
    case "sub":
      f.sub = true;
      break;
    case "u":
      f.underline = true;
      break;
  }
  const style = /style="([^"]*)"/.exec(attrs)?.[1] ?? "";
  for (const decl of style.split(";")) {
    const [k, v] = decl.split(":").map((x) => x.trim().toLowerCase());
    if (!k || !v) continue;
    if (k === "font-style") f.italic = v === "italic" || v === "oblique";
    if (k === "font-weight") f.bold = v === "bold" || Number(v) >= 600;
    if (k === "font-variant") f.smallCaps = v === "small-caps";
    if (k === "text-decoration") f.underline = v.includes("underline");
    if (k === "vertical-align") {
      f.sup = v === "super" || v === "sup";
      f.sub = v === "sub";
    }
  }
  return f;
}

function sameFmt(a: Fmt, b: Fmt): boolean {
  return (
    !!a.italic === !!b.italic &&
    !!a.bold === !!b.bold &&
    !!a.sup === !!b.sup &&
    !!a.sub === !!b.sub &&
    !!a.smallCaps === !!b.smallCaps &&
    !!a.underline === !!b.underline
  );
}

function cleanFmt(f: Fmt): Fmt {
  const out: Fmt = {};
  for (const k of ["italic", "bold", "sup", "sub", "smallCaps", "underline"] as const) if (f[k]) out[k] = true;
  return out;
}

const VOID = new Set(["br", "img", "hr", "meta"]);

export function htmlToRuns(html: string): Run[] {
  const raw: Run[] = [];
  const stack: Frame[] = [{ tag: "#root", cls: "", fmt: {} }];
  const re = /<(\/?)([a-zA-Z][a-zA-Z0-9]*)([^>]*?)\/?>|([^<]+)/g;
  for (let m = re.exec(html); m; m = re.exec(html)) {
    const top = stack[stack.length - 1];
    if (m[4] !== undefined) {
      // 块之间的换行缩进是 citeproc 的排版空白，不是内容
      if (/^\s*$/.test(m[4]) && m[4].includes("\n")) continue;
      raw.push({ text: decodeEntities(m[4]), ...cleanFmt(top.fmt) });
      continue;
    }
    const closing = m[1] === "/";
    const tag = m[2].toLowerCase();
    if (tag === "br") {
      raw.push({ text: "\n" });
      continue;
    }
    if (VOID.has(tag)) continue;
    if (!closing) {
      const cls = /class="([^"]*)"/.exec(m[3])?.[1] ?? "";
      stack.push({ tag, cls, fmt: applyTag(tag, m[3], top.fmt) });
      continue;
    }
    // 关闭标签：弹到同名标签为止（容错不配对的情况）
    for (let i = stack.length - 1; i > 0; i--) {
      if (stack[i].tag === tag) {
        const frame = stack[i];
        stack.length = i;
        // 顺序编码样式：编号列与正文之间用制表符，配合悬挂缩进对齐
        if (frame.cls.includes("csl-left-margin")) raw.push({ text: "\t" });
        break;
      }
    }
  }
  // 去掉首尾空白并合并相邻同格式片段
  const merged: Run[] = [];
  for (const r of raw) {
    if (!r.text) continue;
    const last = merged[merged.length - 1];
    if (last && sameFmt(last, r)) last.text += r.text;
    else merged.push({ ...r });
  }
  if (merged.length) {
    merged[0].text = merged[0].text.replace(/^\s+/, "");
    const end = merged[merged.length - 1];
    end.text = end.text.replace(/\s+$/, "");
  }
  return merged.filter((r) => r.text.length > 0);
}

export function runsToText(runs: Run[]): string {
  return runs.map((r) => r.text).join("");
}

/** 用于比较 Word 中现有内容与应有内容：只比文字与上下标。 */
export function runsSignature(runs: Run[]): string {
  return runs.map((r) => (r.sup ? `^{${r.text}}` : r.sub ? `_{${r.text}}` : r.text)).join("");
}
