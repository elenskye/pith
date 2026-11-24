// docx 直写的三个操作：生成测试文档、刷新（可切换样式）、校验。

import { strToU8 } from "fflate";
import { randomUUID } from "node:crypto";
import { BIB_TAG, CITE_TAG_PREFIX, planRefresh, type Plan } from "../src/refresh.ts";
import { runsToText } from "../src/runs.ts";
import { emptyStore, type DocStore } from "../src/store.ts";
import type { CslItem } from "../src/types.ts";
import {
  bibliographySdts,
  citationSdts,
  findStore,
  listFields,
  openDocx,
  saveDocx,
  sdtParagraphTexts,
  sdtRuns,
  sdtSignature,
  setBlockParagraphs,
  setInlineRuns,
  writeStore,
  zipDocx,
  zoteroFields,
  W,
  type Files,
} from "./docx.ts";
import { locales, styleXml } from "./resources.ts";

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const run = (t: string) => `<w:r><w:t xml:space="preserve">${esc(t)}</w:t></w:r>`;
const citeSdt = (id: string) =>
  `<w:sdt><w:sdtPr><w:alias w:val="Research Lab 引文"/><w:tag w:val="${CITE_TAG_PREFIX}${id}"/>` +
  `<w:id w:val="${Math.floor(Math.random() * 2 ** 31)}"/><w:lock w:val="contentLocked"/></w:sdtPr>` +
  `<w:sdtContent>${run("[…]")}</w:sdtContent></w:sdt>`;

/** 合成的 Zotero 引文域：结构仿照 Zotero 7 写入 Word 的格式，内容全部是合成数据。 */
export function zoteroCitationField(): string {
  const json = JSON.stringify({
    citationID: "zSyn0001",
    properties: { formattedCitation: "(Halvorsen, 2018)", plainCitation: "(Halvorsen, 2018)", noteIndex: 0 },
    citationItems: [
      {
        id: 101,
        uris: ["http://zotero.org/users/local/SYNTHETIC/items/SYNTH002"],
        itemData: {
          id: 101,
          type: "book",
          title: "The architecture of evidence",
          author: [{ family: "Halvorsen", given: "Ingrid" }],
          issued: { "date-parts": [["2018"]] },
          publisher: "Northbridge University Press",
        },
      },
    ],
    schema: "https://github.com/citation-style-language/schema/raw/master/csl-citation.json",
  });
  const code = ` ADDIN ZOTERO_ITEM CSL_CITATION ${json} `;
  // Word 会把很长的域代码拆成多个 instrText，这里也拆开，模拟真实情况
  const chunks = code.match(/[\s\S]{1,120}/g) ?? [];
  return (
    `<w:r><w:fldChar w:fldCharType="begin"/></w:r>` +
    chunks.map((c) => `<w:r><w:instrText xml:space="preserve">${esc(c)}</w:instrText></w:r>`).join("") +
    `<w:r><w:fldChar w:fldCharType="separate"/></w:r>` +
    run("(Halvorsen, 2018)") +
    `<w:r><w:fldChar w:fldCharType="end"/></w:r>`
  );
}

export const SEED_CITATIONS: Record<string, { id: string; locator?: string }[]> = {
  "seed-a": [{ id: "rl-item-0001" }],
  "seed-b": [{ id: "rl-item-0003", locator: "17" }, { id: "rl-item-0002" }],
  "seed-c": [{ id: "rl-item-0005" }, { id: "rl-item-0006" }],
  "seed-d": [{ id: "rl-item-0007" }, { id: "rl-item-0004" }],
};

export function seedDocx(opts: { style: string; zotero: boolean; library: Record<string, CslItem>; now: string }): Uint8Array {
  const body = [
    `<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="32"/></w:rPr><w:t>Research Lab 引文 PoC 测试文档</w:t></w:r></w:p>`,
    `<w:p>${run("政治效能感的测量方法近年变化很大")}${citeSdt("seed-a")}${run("。早期研究依赖问卷")}${citeSdt("seed-b")}${run("，后来出现了基于合成数据的方法")}${citeSdt("seed-c")}${run("。")}</w:p>`,
    `<w:p>${run("多作者研究常见于方法学期刊")}${citeSdt("seed-d")}${run("。")}</w:p>`,
    opts.zotero ? `<w:p>${run("下面这处引文由 Zotero 插入，Research Lab 不应改动它：")}${zoteroCitationField()}${run("。")}</w:p>` : "",
    `<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>参考文献</w:t></w:r></w:p>`,
    `<w:sdt><w:sdtPr><w:alias w:val="Research Lab 参考文献"/><w:tag w:val="${BIB_TAG}"/><w:id w:val="${Math.floor(Math.random() * 2 ** 31)}"/><w:lock w:val="contentLocked"/></w:sdtPr>` +
      `<w:sdtContent><w:p>${run("（参考文献生成中）")}</w:p></w:sdtContent></w:sdt>`,
    `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>`,
  ].join("");
  const files: Files = new Map();
  files.set(
    "[Content_Types].xml",
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
        `<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>` +
        `<Default Extension="xml" ContentType="application/xml"/>` +
        `<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>` +
        `<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>`,
    ),
  );
  files.set(
    "_rels/.rels",
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
        `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`,
    ),
  );
  files.set(
    "word/_rels/document.xml.rels",
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
        `<Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>`,
    ),
  );
  // 声明 Word 2013+ 兼容模式，否则 Word 以「兼容模式」打开并在另存时提示升级
  files.set(
    "word/settings.xml",
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="${W}">` +
        `<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>`,
    ),
  );
  files.set(
    "word/document.xml",
    strToU8(
      `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="${W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>${body}</w:body></w:document>`,
    ),
  );
  const store: DocStore = emptyStore(opts.style, opts.now);
  for (const [id, items] of Object.entries(SEED_CITATIONS)) store.citations[id] = { items, created: opts.now };
  writeStore(files, store);
  return refreshDocx(zipDocx(files), { library: opts.library, now: opts.now }).bytes;
}

export function refreshDocx(
  bytes: Uint8Array,
  opts: { style?: string; library: Record<string, CslItem>; now: string; newId?: () => string },
): { bytes: Uint8Array; plan: Plan } {
  const d = openDocx(bytes);
  const loc = findStore(d.files);
  if (!loc) throw new Error("文档里没有 Research Lab 引文数据");
  if ("error" in loc) throw new Error(loc.error);
  const store = opts.style ? { ...loc.store, style: opts.style } : loc.store;
  const sdts = citationSdts(d.doc);
  const plan = planRefresh({
    doc: sdts.map((s) => ({ tag: s.tag, signature: sdtSignature(s) })),
    store,
    library: opts.library,
    styleXml: styleXml(store.style),
    locales: locales(),
    newId: opts.newId ?? (() => randomUUID()),
    now: opts.now,
  });
  for (const u of plan.citations) if (u.changed) setInlineRuns(d.doc, sdts[u.index], u.runs, u.retag ?? undefined);
  for (const b of bibliographySdts(d.doc)) setBlockParagraphs(d.doc, b, plan.bibliography.entries, plan.bibliography.format.hangingIndentPt);
  writeStore(d.files, plan.store);
  return { bytes: saveDocx(d), plan };
}

export type VerifyReport = {
  ok: boolean;
  style: string | null;
  citations: { index: number; tag: string; actual: string; expected: string | null; ok: boolean }[];
  skipped: Plan["skipped"];
  bibliography: { count: number; ok: boolean; expected: string[]; actual: string[] };
  zotero: { citations: number; bibliographies: number; digest: string };
  problems: string[];
};

/** 用确定性计算核对文档：每处引文与书目是否就是当前数据应有的样子。不改文档。 */
export function verifyDocx(bytes: Uint8Array, opts: { library: Record<string, CslItem> }): VerifyReport {
  const d = openDocx(bytes);
  const problems: string[] = [];
  const z = zoteroFields(listFields(d.doc));
  const loc = findStore(d.files);
  const empty: VerifyReport = {
    ok: false,
    style: null,
    citations: [],
    skipped: [],
    bibliography: { count: 0, ok: false, expected: [], actual: [] },
    zotero: { citations: z.citations, bibliographies: z.bibliographies, digest: z.digest },
    problems,
  };
  if (!loc) return { ...empty, problems: ["文档里没有 Research Lab 引文数据"] };
  if ("error" in loc) return { ...empty, problems: [loc.error] };
  const sdts = citationSdts(d.doc);
  const plan = planRefresh({
    doc: sdts.map((s) => ({ tag: s.tag, signature: sdtSignature(s) })),
    store: loc.store,
    library: opts.library,
    styleXml: styleXml(loc.store.style),
    locales: locales(),
    newId: () => "verify-new-id",
    now: loc.store.updated,
  });
  const byIndex = new Map(plan.citations.map((c) => [c.index, c]));
  const citations = sdts.map((s, index) => {
    const u = byIndex.get(index);
    const actual = runsToText(sdtRuns(s));
    return { index, tag: s.tag, actual, expected: u ? u.text : null, ok: !!u && !u.changed };
  });
  for (const c of citations) if (!c.ok) problems.push(`第 ${c.index + 1} 处引文不一致：现为「${c.actual}」，应为「${c.expected ?? "（无法生成）"}」`);
  for (const s of plan.skipped) problems.push(`第 ${s.index + 1} 处引文被跳过：${s.reason}`);
  for (const w of plan.warnings) problems.push(w);
  const bibs = bibliographySdts(d.doc);
  const actualBib = bibs[0] ? sdtParagraphTexts(bibs[0]) : [];
  const expectedBib = plan.bibliography.texts.length ? plan.bibliography.texts : ["（暂无引文）"];
  const bibOk = bibs.length === 1 && actualBib.length === expectedBib.length && actualBib.every((t, i) => t === expectedBib[i]);
  if (bibs.length !== 1) problems.push(`书目控件应有 1 个，实际 ${bibs.length} 个`);
  else if (!bibOk) problems.push("书目与当前引文不一致，需要刷新");
  return {
    ok: problems.length === 0,
    style: loc.store.style,
    citations,
    skipped: plan.skipped,
    bibliography: { count: bibs.length, ok: bibOk, expected: expectedBib, actual: actualBib },
    zotero: { citations: z.citations, bibliographies: z.bibliographies, digest: z.digest },
    problems,
  };
}
