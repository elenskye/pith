// Office.js 适配层：把刷新计划写进 Word。与 docx 直写、VBA 宏用同一套数据格式。
// 注意：本机 Word 16.114 侧载失效（见 docs/feasibility.md 0-B），这一层只通过了类型检查，未在 Word 里实测。

import { BIB_TAG, CITE_TAG_PREFIX, type Plan } from "./refresh.ts";
import { parseStore, serializeStore, STORE_NS, StoreError, type DocStore } from "./store.ts";
import type { Run } from "./types.ts";

const parseXml = (xml: string) => new DOMParser().parseFromString(xml, "application/xml");

export type DocSnapshot = {
  tags: string[];
  signatures: string[];
  bibliographyCount: number;
  store: DocStore | null;
  storeError: string | null;
};

export type EnvInfo = { platform: string; sets: Record<string, boolean>; smallCaps: boolean };

export function envInfo(): EnvInfo {
  const sets: Record<string, boolean> = {};
  for (const v of ["1.1", "1.3", "1.4", "1.5", "1.9"]) sets[`WordApi ${v}`] = Office.context.requirements.isSetSupported("WordApi", v);
  for (const v of ["1.1", "1.3", "1.5"]) sets[`WordApiDesktop ${v}`] = Office.context.requirements.isSetSupported("WordApiDesktop", v);
  return {
    platform: String(Office.context.platform),
    sets,
    smallCaps: Office.context.requirements.isSetSupported("WordApiDesktop", "1.3"),
  };
}

function applyFont(range: Word.Range | Word.Paragraph, r: Run, smallCaps: boolean) {
  const f = range.font;
  f.italic = !!r.italic;
  f.bold = !!r.bold;
  f.superscript = !!r.sup;
  f.subscript = !!r.sub;
  f.underline = r.underline ? "Single" : "None";
  if (smallCaps) f.smallCaps = !!r.smallCaps;
}

function writeInline(cc: Word.ContentControl, runs: Run[], smallCaps: boolean) {
  const list = runs.length ? runs : [{ text: "?" }];
  cc.cannotEdit = false;
  list.forEach((r, i) => applyFont(cc.insertText(r.text, i === 0 ? "Replace" : "End"), r, smallCaps));
  cc.cannotEdit = true;
}

async function writeBibliography(ctx: Word.RequestContext, cc: Word.ContentControl, entries: Run[][], hangPt: number, smallCaps: boolean) {
  cc.cannotEdit = false;
  const paras = cc.paragraphs;
  paras.load("items");
  await ctx.sync();
  for (let i = paras.items.length - 1; i >= 1; i--) paras.items[i].delete();
  const list = entries.length ? entries : [[{ text: "（暂无引文）" }]];
  let para = paras.items[0];
  list.forEach((runs, i) => {
    if (i > 0) para = cc.insertParagraph("", "End");
    runs.forEach((r, j) => applyFont(para.insertText(r.text, j === 0 ? "Replace" : "End"), r, smallCaps));
    para.leftIndent = hangPt;
    para.firstLineIndent = -hangPt;
  });
  cc.cannotEdit = true;
}

async function writeStore(ctx: Word.RequestContext, store: DocStore) {
  const parts = ctx.document.customXmlParts.getByNamespace(STORE_NS);
  parts.load("items");
  await ctx.sync();
  const xml = serializeStore(store);
  if (parts.items.length) parts.items[0].setXml(xml);
  else ctx.document.customXmlParts.add(xml);
}

export async function readDoc(): Promise<DocSnapshot> {
  return Word.run(async (ctx) => {
    const ccs = ctx.document.body.contentControls;
    ccs.load("items/tag,items/text,items/font/superscript");
    const parts = ctx.document.customXmlParts.getByNamespace(STORE_NS);
    parts.load("items");
    await ctx.sync();
    const cites = ccs.items.filter((c) => c.tag.startsWith(CITE_TAG_PREFIX));
    let store: DocStore | null = null;
    let storeError: string | null = null;
    if (parts.items.length) {
      const xml = parts.items[0].getXml();
      await ctx.sync();
      try {
        store = parseStore(xml.value, parseXml);
      } catch (e) {
        storeError = e instanceof StoreError ? e.message : String(e);
      }
    }
    return {
      tags: cites.map((c) => c.tag),
      // 整段上标时按上标签名比较；混合格式（null）只比文字
      signatures: cites.map((c) => (c.font.superscript === true ? `^{${c.text}}` : c.text)),
      bibliographyCount: ccs.items.filter((c) => c.tag === BIB_TAG).length,
      store,
      storeError,
    };
  });
}

export async function applyPlan(plan: Plan, expectTags: string[], smallCaps: boolean): Promise<void> {
  await Word.run(async (ctx) => {
    const ccs = ctx.document.body.contentControls;
    ccs.load("items/tag");
    await ctx.sync();
    const cites = ccs.items.filter((c) => c.tag.startsWith(CITE_TAG_PREFIX));
    if (cites.map((c) => c.tag).join("\n") !== expectTags.join("\n")) {
      throw new Error("刷新期间文档里的引文变了，请再点一次刷新");
    }
    for (const u of plan.citations) {
      if (!u.changed) continue;
      const cc = cites[u.index];
      if (u.retag) cc.tag = u.retag;
      writeInline(cc, u.runs, smallCaps);
    }
    for (const b of ccs.items.filter((c) => c.tag === BIB_TAG)) {
      await writeBibliography(ctx, b, plan.bibliography.entries, plan.bibliography.format.hangingIndentPt, smallCaps);
    }
    await writeStore(ctx, plan.store);
    await ctx.sync();
  });
}

export async function insertCitation(store: DocStore, citationID: string): Promise<void> {
  await Word.run(async (ctx) => {
    const sel = ctx.document.getSelection();
    const parent = sel.parentContentControlOrNullObject;
    parent.load("tag");
    await ctx.sync();
    if (!parent.isNullObject && (parent.tag.startsWith(CITE_TAG_PREFIX) || parent.tag === BIB_TAG)) {
      throw new Error("光标在引文或参考文献表里，请先把光标移到正文");
    }
    const cc = sel.insertText("…", "End").insertContentControl();
    cc.tag = CITE_TAG_PREFIX + citationID;
    cc.title = "Research Lab 引文";
    cc.appearance = "BoundingBox";
    await writeStore(ctx, store);
    await ctx.sync();
  });
}

export async function updateStore(store: DocStore): Promise<void> {
  await Word.run(async (ctx) => {
    await writeStore(ctx, store);
    await ctx.sync();
  });
}

export async function insertBibliography(): Promise<void> {
  await Word.run(async (ctx) => {
    const ccs = ctx.document.body.contentControls;
    ccs.load("items/tag");
    await ctx.sync();
    if (ccs.items.some((c) => c.tag === BIB_TAG)) throw new Error("文档里已经有参考文献表，点「刷新」即可");
    const p = ctx.document.getSelection().paragraphs.getLast().insertParagraph("（参考文献生成中）", "After");
    const cc = p.insertContentControl();
    cc.tag = BIB_TAG;
    cc.title = "Research Lab 参考文献";
    cc.appearance = "BoundingBox";
    await ctx.sync();
  });
}

export async function selectedCitationId(): Promise<string | null> {
  return Word.run(async (ctx) => {
    const p = ctx.document.getSelection().parentContentControlOrNullObject;
    p.load("tag");
    await ctx.sync();
    return !p.isNullObject && p.tag.startsWith(CITE_TAG_PREFIX) ? p.tag.slice(CITE_TAG_PREFIX.length) : null;
  });
}

export async function deleteCitation(citationID: string): Promise<void> {
  await Word.run(async (ctx) => {
    const ccs = ctx.document.body.contentControls.getByTag(CITE_TAG_PREFIX + citationID);
    ccs.load("items");
    await ctx.sync();
    for (const cc of ccs.items) {
      cc.cannotDelete = false;
      cc.delete(false);
    }
    await ctx.sync();
  });
}

/** 只读扫描文档里的域，统计 Zotero 引文（ADDIN ZOTERO_ITEM）与书目（ADDIN ZOTERO_BIBL）。 */
export async function scanZoteroFields(): Promise<{ total: number; citations: number; bibliographies: number; sample: string | null }> {
  return Word.run(async (ctx) => {
    const fields = ctx.document.body.fields;
    fields.load("items/code");
    await ctx.sync();
    const codes = fields.items.map((f) => f.code.trim());
    const cit = codes.filter((c) => c.startsWith("ADDIN ZOTERO_ITEM"));
    return {
      total: codes.length,
      citations: cit.length,
      bibliographies: codes.filter((c) => c.startsWith("ADDIN ZOTERO_BIBL")).length,
      sample: cit[0]?.slice(0, 160) ?? null,
    };
  });
}
