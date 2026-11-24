// 在 Word 外读写 .docx 里的 Research Lab 引文：内容控件（w:sdt）+ Custom XML 数据 + 域扫描。
// 只依赖 fflate（zip）与 @xmldom/xmldom（XML），供命令行工具与测试使用。

import { unzipSync, zipSync, strFromU8, strToU8 } from "fflate";
import { DOMParser, XMLSerializer } from "@xmldom/xmldom";
import { createHash, randomUUID } from "node:crypto";
import { BIB_TAG, CITE_TAG_PREFIX } from "../src/refresh.ts";
import { runsSignature } from "../src/runs.ts";
import { parseStore, serializeStore, STORE_NS, StoreError, type DocStore } from "../src/store.ts";
import type { Run } from "../src/types.ts";

export const W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const PKG_REL = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT = "http://schemas.openxmlformats.org/package/2006/content-types";
const REL_CUSTOMXML = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
const REL_CUSTOMXML_PROPS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
const CT_ITEMPROPS = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
const DS = "http://schemas.openxmlformats.org/officeDocument/2006/customXml";

export type Files = Map<string, Uint8Array>;

export const parseXml = (xml: string) => new DOMParser().parseFromString(xml, "application/xml") as unknown as Document;
const serialize = (doc: Document) => new XMLSerializer().serializeToString(doc as never);

export function unzipDocx(bytes: Uint8Array): Files {
  return new Map(Object.entries(unzipSync(bytes)));
}

export function zipDocx(files: Files): Uint8Array {
  // [Content_Types].xml 放第一个，部分读取器依赖这个顺序
  const entries: Record<string, Uint8Array> = {};
  const names = [...files.keys()].sort((a, b) => (a === "[Content_Types].xml" ? -1 : b === "[Content_Types].xml" ? 1 : 0));
  for (const n of names) entries[n] = files.get(n)!;
  return zipSync(entries, { level: 6 });
}

const text = (files: Files, path: string) => {
  const b = files.get(path);
  if (!b) throw new Error(`docx 缺少 ${path}`);
  return strFromU8(b);
};

// ---------- 文档数据（Custom XML Part） ----------

export type StoreLocation = { path: string; store: DocStore } | { path: string; error: string } | null;

export function findStore(files: Files): StoreLocation {
  const paths = [...files.keys()].filter((p) => /^customXml\/item\d+\.xml$/.test(p)).sort();
  for (const p of paths) {
    const xml = text(files, p);
    if (!xml.includes(STORE_NS)) continue;
    try {
      return { path: p, store: parseStore(xml, parseXml) };
    } catch (e) {
      return { path: p, error: e instanceof StoreError ? e.message : String(e) };
    }
  }
  return null;
}

export function writeStore(files: Files, store: DocStore): void {
  const xml = strToU8(serializeStore(store));
  const found = findStore(files);
  if (found) {
    files.set(found.path, xml);
    return;
  }
  let n = 1;
  while (files.has(`customXml/item${n}.xml`)) n++;
  const item = `customXml/item${n}.xml`;
  files.set(item, xml);
  files.set(
    `customXml/itemProps${n}.xml`,
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="no"?>` +
        `<ds:datastoreItem ds:itemID="{${randomUUID().toUpperCase()}}" xmlns:ds="${DS}">` +
        `<ds:schemaRefs><ds:schemaRef ds:uri="${STORE_NS}"/></ds:schemaRefs></ds:datastoreItem>`,
    ),
  );
  files.set(
    `customXml/_rels/item${n}.xml.rels`,
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
        `<Relationships xmlns="${PKG_REL}"><Relationship Id="rId1" Type="${REL_CUSTOMXML_PROPS}" Target="itemProps${n}.xml"/></Relationships>`,
    ),
  );
  // 文档主体 → customXml 关系
  const relsPath = "word/_rels/document.xml.rels";
  const rels = parseXml(files.has(relsPath) ? text(files, relsPath) : `<Relationships xmlns="${PKG_REL}"/>`);
  const used = new Set(Array.from(rels.getElementsByTagNameNS(PKG_REL, "Relationship")).map((r) => r.getAttribute("Id")));
  let rid = 1;
  while (used.has(`rIdRL${rid}`)) rid++;
  const rel = rels.createElementNS(PKG_REL, "Relationship");
  rel.setAttribute("Id", `rIdRL${rid}`);
  rel.setAttribute("Type", REL_CUSTOMXML);
  rel.setAttribute("Target", `../${item}`);
  rels.documentElement.appendChild(rel);
  files.set(relsPath, strToU8(serialize(rels)));
  // 内容类型
  const ct = parseXml(text(files, "[Content_Types].xml"));
  const defaults = Array.from(ct.getElementsByTagNameNS(CT, "Default")).map((d) => d.getAttribute("Extension"));
  if (!defaults.includes("xml")) {
    const d = ct.createElementNS(CT, "Default");
    d.setAttribute("Extension", "xml");
    d.setAttribute("ContentType", "application/xml");
    ct.documentElement.insertBefore(d, ct.documentElement.firstChild);
  }
  const o = ct.createElementNS(CT, "Override");
  o.setAttribute("PartName", `/customXml/itemProps${n}.xml`);
  o.setAttribute("ContentType", CT_ITEMPROPS);
  ct.documentElement.appendChild(o);
  files.set("[Content_Types].xml", strToU8(serialize(ct)));
}

// ---------- 内容控件 ----------

const kids = (el: Element, local: string) =>
  Array.from(el.childNodes).filter((n): n is Element => n.nodeType === 1 && (n as Element).namespaceURI === W && (n as Element).localName === local);
const kid = (el: Element, local: string) => kids(el, local)[0] as Element | undefined;
const descendants = (el: Element | Document, local: string) => Array.from(el.getElementsByTagNameNS(W, local));

export type Sdt = { el: Element; tag: string; block: boolean };

export function listSdts(doc: Document): Sdt[] {
  return descendants(doc, "sdt").map((el) => {
    const pr = kid(el, "sdtPr");
    const tag = (pr && kid(pr, "tag")?.getAttributeNS(W, "val")) ?? "";
    const content = kid(el, "sdtContent");
    const block = !!content && kids(content, "p").length > 0;
    return { el, tag, block };
  });
}

/** 把 w:r 读成带格式的片段（只关心正文文字与上下标、斜体、粗体）。 */
function readRuns(container: Element): Run[] {
  const out: Run[] = [];
  for (const r of descendants(container, "r")) {
    const rPr = kid(r, "rPr");
    const va = rPr && kid(rPr, "vertAlign")?.getAttributeNS(W, "val");
    const flag = (name: string) => {
      const e = rPr && kid(rPr, name);
      if (!e) return false;
      const v = e.getAttributeNS(W, "val");
      return v !== "0" && v !== "false";
    };
    let t = "";
    for (const c of Array.from(r.childNodes)) {
      if (c.nodeType !== 1) continue;
      const e = c as Element;
      if (e.namespaceURI !== W) continue;
      if (e.localName === "t") t += e.textContent ?? "";
      else if (e.localName === "tab") t += "\t";
      else if (e.localName === "br" || e.localName === "cr") t += "\n";
    }
    if (!t) continue;
    const run: Run = { text: t };
    if (flag("i")) run.italic = true;
    if (flag("b")) run.bold = true;
    if (va === "superscript") run.sup = true;
    if (va === "subscript") run.sub = true;
    const last = out[out.length - 1];
    if (last && !!last.sup === !!run.sup && !!last.sub === !!run.sub && !!last.italic === !!run.italic && !!last.bold === !!run.bold)
      last.text += t;
    else out.push(run);
  }
  return out;
}

export function sdtRuns(sdt: Sdt): Run[] {
  const content = kid(sdt.el, "sdtContent");
  return content ? readRuns(content) : [];
}

export function sdtSignature(sdt: Sdt): string {
  return runsSignature(sdtRuns(sdt));
}

/** 块级控件里每个段落的文字（含制表符）。 */
export function sdtParagraphTexts(sdt: Sdt): string[] {
  const content = kid(sdt.el, "sdtContent");
  if (!content) return [];
  return descendants(content, "p").map((p) => readRuns(p).map((r) => r.text).join(""));
}

// 保留原有字体、字号、语言，去掉会被引文格式覆盖的属性；按 schema 顺序重建 rPr
const KEEP_RPR = ["rStyle", "rFonts", "color", "sz", "szCs", "lang"];
const RPR_ORDER = ["rStyle", "rFonts", "b", "bCs", "i", "iCs", "smallCaps", "color", "sz", "szCs", "u", "vertAlign", "lang"];

function buildRPr(doc: Document, base: Element | undefined, run: Run): Element {
  const rPr = doc.createElementNS(W, "w:rPr");
  const parts = new Map<string, Element>();
  if (base) for (const name of KEEP_RPR) { const e = kid(base, name); if (e) parts.set(name, e.cloneNode(true) as Element); }
  const mk = (name: string, val?: string) => {
    const e = doc.createElementNS(W, `w:${name}`);
    if (val) e.setAttributeNS(W, "w:val", val);
    parts.set(name, e);
  };
  if (run.bold) { mk("b"); mk("bCs"); }
  if (run.italic) { mk("i"); mk("iCs"); }
  if (run.smallCaps) mk("smallCaps");
  if (run.underline) mk("u", "single");
  if (run.sup) mk("vertAlign", "superscript");
  if (run.sub) mk("vertAlign", "subscript");
  for (const name of RPR_ORDER) { const e = parts.get(name); if (e) rPr.appendChild(e); }
  return rPr;
}

function buildRun(doc: Document, base: Element | undefined, run: Run): Element {
  const r = doc.createElementNS(W, "w:r");
  const rPr = buildRPr(doc, base, run);
  if (rPr.childNodes.length) r.appendChild(rPr);
  const pieces = run.text.split(/(\t|\n)/);
  for (const piece of pieces) {
    if (!piece) continue;
    if (piece === "\t") r.appendChild(doc.createElementNS(W, "w:tab"));
    else if (piece === "\n") r.appendChild(doc.createElementNS(W, "w:br"));
    else {
      const t = doc.createElementNS(W, "w:t");
      t.setAttribute("xml:space", "preserve");
      t.appendChild(doc.createTextNode(piece));
      r.appendChild(t);
    }
  }
  return r;
}

function clear(el: Element) {
  while (el.firstChild) el.removeChild(el.firstChild);
}

/** 改写行内引文控件的内容。 */
export function setInlineRuns(doc: Document, sdt: Sdt, runs: Run[], tag?: string): void {
  const content = kid(sdt.el, "sdtContent")!;
  const baseR = descendants(content, "r")[0];
  const base = baseR ? kid(baseR, "rPr") : undefined;
  const baseClone = base?.cloneNode(true) as Element | undefined;
  clear(content);
  for (const run of runs) content.appendChild(buildRun(doc, baseClone, run));
  if (tag) {
    const pr = kid(sdt.el, "sdtPr")!;
    kid(pr, "tag")!.setAttributeNS(W, "w:val", tag);
    sdt.tag = tag;
  }
}

/** 改写块级书目控件：每条一段，悬挂缩进。 */
export function setBlockParagraphs(doc: Document, sdt: Sdt, entries: Run[][], hangingIndentPt: number): void {
  const content = kid(sdt.el, "sdtContent")!;
  const firstP = descendants(content, "p")[0];
  const basePPr = firstP ? kid(firstP, "pPr") : undefined;
  const firstR = firstP ? descendants(firstP, "r")[0] : undefined;
  const baseRPr = (firstR ? kid(firstR, "rPr") : undefined)?.cloneNode(true) as Element | undefined;
  const pStyle = basePPr ? kid(basePPr, "pStyle")?.cloneNode(true) : undefined;
  const spacing = basePPr ? kid(basePPr, "spacing")?.cloneNode(true) : undefined;
  clear(content);
  const list = entries.length ? entries : [[{ text: "（暂无引文）" }]];
  for (const runs of list) {
    const p = doc.createElementNS(W, "w:p");
    const pPr = doc.createElementNS(W, "w:pPr");
    if (pStyle) pPr.appendChild(pStyle.cloneNode(true));
    if (spacing) pPr.appendChild(spacing.cloneNode(true));
    if (hangingIndentPt > 0) {
      const ind = doc.createElementNS(W, "w:ind");
      const tw = String(Math.round(hangingIndentPt * 20));
      ind.setAttributeNS(W, "w:left", tw);
      ind.setAttributeNS(W, "w:hanging", tw);
      pPr.appendChild(ind);
    }
    if (pPr.childNodes.length) p.appendChild(pPr);
    for (const run of runs) p.appendChild(buildRun(doc, baseRPr, run));
    content.appendChild(p);
  }
}

export function newInlineSdt(doc: Document, tag: string, alias: string): Element {
  const sdt = doc.createElementNS(W, "w:sdt");
  const pr = doc.createElementNS(W, "w:sdtPr");
  const a = doc.createElementNS(W, "w:alias");
  a.setAttributeNS(W, "w:val", alias);
  const t = doc.createElementNS(W, "w:tag");
  t.setAttributeNS(W, "w:val", tag);
  const id = doc.createElementNS(W, "w:id");
  id.setAttributeNS(W, "w:val", String(Math.floor(Math.random() * 2 ** 31)));
  const lock = doc.createElementNS(W, "w:lock");
  lock.setAttributeNS(W, "w:val", "contentLocked");
  pr.appendChild(a);
  pr.appendChild(t);
  pr.appendChild(id);
  pr.appendChild(lock);
  sdt.appendChild(pr);
  sdt.appendChild(doc.createElementNS(W, "w:sdtContent"));
  return sdt;
}

// ---------- 域（Zotero 等） ----------

export type FieldInfo = { code: string; result: string };

/** 按文档顺序列出复杂域（fldChar）与简单域（fldSimple）。只读。 */
export function listFields(doc: Document): FieldInfo[] {
  const out: FieldInfo[] = [];
  const stack: { code: string; result: string; inResult: boolean }[] = [];
  const walk = (node: Node) => {
    for (const c of Array.from(node.childNodes)) {
      if (c.nodeType !== 1) continue;
      const e = c as Element;
      if (e.namespaceURI === W && e.localName === "fldSimple") {
        out.push({ code: (e.getAttributeNS(W, "instr") ?? "").trim(), result: readRuns(e).map((r) => r.text).join("") });
        continue;
      }
      if (e.namespaceURI === W && e.localName === "fldChar") {
        const type = e.getAttributeNS(W, "fldCharType");
        if (type === "begin") stack.push({ code: "", result: "", inResult: false });
        else if (type === "separate" && stack.length) stack[stack.length - 1].inResult = true;
        else if (type === "end" && stack.length) {
          const f = stack.pop()!;
          out.push({ code: f.code.trim(), result: f.result });
        }
        continue;
      }
      if (e.namespaceURI === W && e.localName === "instrText" && stack.length) {
        stack[stack.length - 1].code += e.textContent ?? "";
        continue;
      }
      if (e.namespaceURI === W && (e.localName === "t" || e.localName === "tab") && stack.length && stack[stack.length - 1].inResult) {
        stack[stack.length - 1].result += e.localName === "tab" ? "\t" : (e.textContent ?? "");
      }
      walk(e);
    }
  };
  walk(doc.documentElement);
  return out;
}

export function zoteroFields(fields: FieldInfo[]) {
  const z = fields.filter((f) => /^ADDIN ZOTERO_/.test(f.code));
  const digest = createHash("sha256").update(z.map((f) => `${f.code}\u0000${f.result}`).join("\u0001")).digest("hex");
  return {
    citations: z.filter((f) => f.code.startsWith("ADDIN ZOTERO_ITEM")).length,
    bibliographies: z.filter((f) => f.code.startsWith("ADDIN ZOTERO_BIBL")).length,
    digest,
    fields: z,
  };
}

// ---------- 读写整个文档 ----------

export type OpenDoc = { files: Files; doc: Document };

export function openDocx(bytes: Uint8Array): OpenDoc {
  const files = unzipDocx(bytes);
  return { files, doc: parseXml(text(files, "word/document.xml")) };
}

export function saveDocx(d: OpenDoc): Uint8Array {
  d.files.set("word/document.xml", strToU8(serialize(d.doc)));
  return zipDocx(d.files);
}

export function citationSdts(doc: Document): Sdt[] {
  return listSdts(doc).filter((s) => s.tag.startsWith(CITE_TAG_PREFIX));
}

export function bibliographySdts(doc: Document): Sdt[] {
  return listSdts(doc).filter((s) => s.tag === BIB_TAG);
}
