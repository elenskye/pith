// 仅开发用：浏览器里不启动 Rust 也能看界面（`pnpm dev` → http://localhost:1430/?mock）。
// 一个很小的内存模型，大致模仿 Rust 命令，不是规则的来源（规则与测试在 Rust）。数据全部是合成的。新增命令时同步这里。
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { agentHandlers } from "./mockAgent";
import { researchHandlers } from "./mockResearch";
import { conceptHandlers } from "./mockConcept";
import type {
  Annotation,
  AnnotationPatch,
  AppInfo,
  AttachmentDetail,
  BackupInfo,
  Check,
  CollectionNode,
  LibraryCounts,
  RestoreReport,
  Creator,
  CreatorInput,
  FilePreview,
  ItemDetail,
  ItemPatch,
  ListQuery,
  ListRow,
  LookupChange,
  LookupProgress,
  LookupView,
  MetadataSettings,
  MetadataSettingsPatch,
  NewAnnotation,
  NoteDetail,
  ReadingState,
  RunDetail,
  Target,
  ZoteroPreview,
} from "@/shared/ipc";

type Args = Record<string, unknown>;
const t = (d: string) => `${d}T08:00:00.000Z`;
const now = () => new Date().toISOString();

type MItem = { id: number; uid: string; itemType: string; fields: Record<string, string>; creators: Creator[]; tags: string[]; collectionIds: number[]; source: string; createdAt: string; updatedAt: string; deletedAt: string | null };
type MAtt = { id: number; itemId: number | null; kind: string; title: string; originalName: string; contentType: string; status: string; size: number | null; tags: string[]; collectionIds: number[]; createdAt: string; deletedAt: string | null };
type MNote = { id: number; itemId: number | null; title: string; text: string; tags: string[]; collectionIds: number[]; imageCount: number; createdAt: string; updatedAt: string; deletedAt: string | null };
type MColl = { id: number; parentId: number | null; name: string; createdAt: string; deletedAt: string | null };

const person = (family: string, given: string, role = "author"): Creator => ({ role, name: `${family} ${given}`, family, given, literal: null });
const single = (name: string, role = "author"): Creator => ({ role, name, family: null, given: null, literal: name });

let nextId = 2000;
const items: MItem[] = [
  { id: 1, uid: "U1", itemType: "journalArticle", fields: { title: "示例：公民政治效能感的测量与比较", date: "2021-03", publicationTitle: "示例社会科学学报", DOI: "10.0000/example.2021.001", language: "zh", abstractNote: "这是一段合成的摘要，用来预览详情栏的排版。", volume: "36", issue: "2", pages: "45-70" }, creators: [single("张明"), single("李华")], tags: ["政治心理", "测量"], collectionIds: [1], source: "zotero", createdAt: t("2026-11-01"), updatedAt: t("2026-11-01"), deletedAt: null },
  { id: 2, uid: "U2", itemType: "book", fields: { title: "An Example Book on Survey Methods", date: "2015", ISBN: "9780000000002", language: "en", publisher: "Synthetic Press", place: "London" }, creators: [person("Doe", "Jane"), person("Roe", "Richard", "editor")], tags: ["方法"], collectionIds: [3], source: "manual", createdAt: t("2026-11-02"), updatedAt: t("2026-11-02"), deletedAt: null },
  { id: 3, uid: "U3", itemType: "thesis", fields: { title: "示例学位论文：城市就业结构的变化", date: "2019-06", university: "合成大学" }, creators: [single("王芳")], tags: [], collectionIds: [], source: "manual", createdAt: t("2026-11-03"), updatedAt: t("2026-11-03"), deletedAt: null },
  { id: 4, uid: "U4", itemType: "webpage", fields: { title: "已删除的示例网页" }, creators: [], tags: [], collectionIds: [], source: "manual", createdAt: t("2026-11-04"), updatedAt: t("2026-11-05"), deletedAt: t("2026-11-05") },
  { id: 5, uid: "U5", itemType: "journalArticle", fields: { title: "公民政治效能感的测量与比较", date: "2021", DOI: "10.0000/EXAMPLE.2021.001", publicationTitle: "示例社会科学学报" }, creators: [single("张明")], tags: [], collectionIds: [], source: "bibtex", createdAt: t("2026-11-06"), updatedAt: t("2026-11-06"), deletedAt: null },
  // v0.4 概念研究的两本书（mockConcept.ts）：英文夹具、合成中文，各带一个 EPUB 附件
  { id: 6, uid: "U6", itemType: "book", fields: { title: "The Wealth of Nations (fixture)", date: "1776", language: "en", publisher: "Project Gutenberg #3300（公有领域，测试夹具）" }, creators: [person("Smith", "Adam")], tags: ["国富论"], collectionIds: [], source: "manual", createdAt: t("2026-11-13"), updatedAt: t("2026-11-13"), deletedAt: null },
  { id: 7, uid: "U7", itemType: "book", fields: { title: "国富论（合成）", date: "2011", language: "zh" }, creators: [single("亚当·斯密")], tags: ["国富论"], collectionIds: [], source: "manual", createdAt: t("2026-11-13"), updatedAt: t("2026-11-13"), deletedAt: null },
];
const atts: MAtt[] = [
  { id: 101, itemId: 1, kind: "stored", title: "Full Text PDF", originalName: "Zhang - 2021 - 示例.pdf", contentType: "application/pdf", status: "ok", size: 482113, tags: [], collectionIds: [], createdAt: t("2026-11-01"), deletedAt: null },
  { id: 102, itemId: 2, kind: "stored", title: "Full Text PDF", originalName: "missing.pdf", contentType: "application/pdf", status: "missing", size: null, tags: [], collectionIds: [], createdAt: t("2026-11-02"), deletedAt: null },
  { id: 103, itemId: null, kind: "stored", title: "独立的报告.pdf", originalName: "独立的报告.pdf", contentType: "application/pdf", status: "ok", size: 120344, tags: ["待读"], collectionIds: [3], createdAt: t("2026-11-06"), deletedAt: null },
  { id: 104, itemId: 6, kind: "stored", title: "wealth-of-nations.epub", originalName: "wealth-of-nations.epub", contentType: "application/epub+zip", status: "ok", size: 61234, tags: [], collectionIds: [], createdAt: t("2026-11-13"), deletedAt: null },
  { id: 105, itemId: 7, kind: "stored", title: "国富论-合成.epub", originalName: "国富论-合成.epub", contentType: "application/epub+zip", status: "ok", size: 12034, tags: [], collectionIds: [], createdAt: t("2026-11-13"), deletedAt: null },
];
const notes: MNote[] = [
  { id: 201, itemId: 1, title: "读书笔记", text: "读书笔记\n内部效能感与外部效能感的区别……\n第二段。", tags: ["笔记"], collectionIds: [], imageCount: 0, createdAt: t("2026-11-01"), updatedAt: t("2026-11-01"), deletedAt: null },
  { id: 202, itemId: null, title: "独立笔记：开题思路", text: "独立笔记：开题思路\n先读综述，再定变量。", tags: [], collectionIds: [1], imageCount: 1, createdAt: t("2026-11-07"), updatedAt: t("2026-11-07"), deletedAt: null },
];
const colls: MColl[] = [
  { id: 1, parentId: null, name: "毕业论文", createdAt: t("2026-10-01"), deletedAt: null },
  { id: 2, parentId: 1, name: "文献综述", createdAt: t("2026-10-01"), deletedAt: null },
  { id: 3, parentId: null, name: "方法论", createdAt: t("2026-10-01"), deletedAt: null },
  { id: 9, parentId: null, name: "已删除的集合", createdAt: t("2026-10-01"), deletedAt: t("2026-11-05") },
];
const dismissed = new Set<string>();

// ---- 阅读：合成 PDF 与标注 ----
const readable = (a: MAtt) => a.status === "ok" && a.contentType === "application/pdf";
const annCount = (id: number) => anns.filter((x) => x.attachmentId === id && !x.deletedAt).length;
type MAnn = Annotation & { attachmentId: number; deletedAt: string | null };
const ann = (id: number, attachmentId: number, kind: string, pageIndex: number, rects: number[][], text: string | null, color: string, extra: Partial<MAnn> = {}): MAnn => ({
  id, uid: `AN${id}`, attachmentId, kind, pageIndex, pageLabel: pageIndex === 0 ? "i" : String(pageIndex), position: { pageIndex, rects }, sortKey: `${String(pageIndex).padStart(5, "0")}|000000|${String(Math.floor(792 - Math.max(...rects.map((r) => r[3])))).padStart(5, "0")}`,
  text, comment: null, color, tags: [], authorName: null, isExternal: false, editable: true, createdAt: t("2026-11-02"), updatedAt: t("2026-11-02"), deletedAt: null, ...extra,
});
// 103（独立报告）带两条「从 Zotero 导入」的标注：一条 PDF 自带（只读）、一条便签
const anns: MAnn[] = [
  ann(301, 103, "highlight", 1, [[72, 661, 519, 673]], "Political efficacy is the feeling that individual political action does have an impact.", "#5fb236", { tags: ["定义"], comment: "经典定义，综述里引用", isExternal: true, editable: false, authorName: "示例作者" }),
  ann(302, 103, "note", 2, [[480, 700, 502, 722]], null, "#ffd400", { comment: "这一页的测量量表可以借用" }),
];
const readerStates = new Map<number, ReadingState>();
const samplePdfUrls = new Map<number, string>();

/** 浏览器预览用的合成 PDF：三页英文段落（Helvetica，可选字），页码标签 i、1、2，第一页有一个外部链接。 */
function samplePdf(title: string): Uint8Array<ArrayBuffer> {
  const para = [
    "Political efficacy is the feeling that individual political action does have an impact.",
    "Scholars distinguish internal efficacy, the belief that one can understand politics,",
    "from external efficacy, the belief that the government responds to citizens.",
    "This synthetic page exists only so the reader can be tried in the browser preview.",
    "Select a sentence, pick a colour, and the highlight appears in the sidebar list.",
  ];
  const esc = (x: string) => x.replace(/[\\()]/g, (m) => "\\" + m);
  const objs: string[] = [];
  const pageIds: number[] = [];
  const pageCount = 3;
  let next = 4;
  const pageObjs: [number, string][] = [];
  for (let p = 0; p < pageCount; p++) {
    const pid = next++;
    const cid = next++;
    pageIds.push(pid);
    const lines = [p === 0 ? title : `Section ${p}`, "", ...para, "", ...para.slice().reverse()];
    const body = `BT /F1 ${p === 0 ? 16 : 14} Tf 72 720 Td (${esc(lines[0])}) Tj /F1 11 Tf 0 -26 Td 15 TL ${lines.slice(1).map((l) => `(${esc(l)}) '`).join(" ")} ET` +
      (p === 0 ? " BT /F1 11 Tf 72 104 Td (https://example.org/) Tj ET" : "");
    const annots = p === 0 ? " /Annots [<< /Type /Annot /Subtype /Link /Rect [70 100 190 116] /Border [0 0 0] /A << /S /URI /URI (https://example.org/) >> >>]" : "";
    pageObjs.push([pid, `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents ${cid} 0 R${annots} >>`]);
    pageObjs.push([cid, `<< /Length ${body.length} >>\nstream\n${body}\nendstream`]);
  }
  objs[1] = "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /r >> 1 << /S /D >>] >> >>";
  objs[2] = `<< /Type /Pages /Kids [${pageIds.map((i) => `${i} 0 R`).join(" ")}] /Count ${pageCount} >>`;
  objs[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
  for (const [id, o] of pageObjs) objs[id] = o;
  let out = "%PDF-1.4\n";
  const offsets: number[] = [];
  for (let i = 1; i < objs.length; i++) {
    offsets[i] = out.length;
    out += `${i} 0 obj\n${objs[i]}\nendobj\n`;
  }
  const xref = out.length;
  out += `xref\n0 ${objs.length}\n0000000000 65535 f \n${offsets.slice(1).map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("")}`;
  out += `trailer\n<< /Size ${objs.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(out);
}
const tagIds = new Map<string, number>();
const tagId = (name: string) => {
  if (!tagIds.has(name)) tagIds.set(name, tagIds.size + 1);
  return tagIds.get(name)!;
};
const backups: BackupInfo[] = [{ fileName: "library-20261108-090000-daily.db", label: "daily", createdAt: "2026-11-08 09:00:00", size: 286720 }];
const info: AppInfo = { appVersion: "0.3.0", isDev: true, dataDir: "/Users/mock/Pith Dev", schemaVersion: 7, latestSchema: 7, startupError: null };

// ---- 联网补全（模拟：不联网，按条目编一份「查到的记录」；规则的来源是 Rust） ----
const mSettings: MetadataSettings = { enabled: null, auto: true, email: null };
const lookups = new Map<number, LookupView>(); // 每条文献最近一次
let lookupSeq = 5000;
const progress: LookupProgress = { running: false, run: 0, total: 0, done: 0, filledItems: 0, filledFields: 0, conflictItems: 0, notFound: 0, failed: 0, skipped: 0, stopped: null, finishedAt: null };
let lookupQueue: [number, string][] = [];
let lookupCancel = false;
const identifierOf = (i: MItem) => (i.fields.DOI ? `doi:${i.fields.DOI.toLowerCase()}` : i.fields.ISBN ? `isbn:${i.fields.ISBN.replace(/-/g, "")}` : null);
const reviewIds = () => [...lookups.values()].filter((l) => l.changes.some((c) => c.state === "pending")).map((l) => l.itemId);
const CONFLICT = ["title", "date", "publicationTitle", "bookTitle", "volume", "issue", "pages", "publisher"];
function fakeFound(i: MItem): { source: string; fields: Record<string, string> } | null {
  if (i.fields.DOI?.includes("missing")) return null;
  if (i.fields.DOI) return { source: "crossref", fields: { title: i.fields.title ?? "Synthetic found title", date: i.fields.date ?? "2021-03", publicationTitle: i.fields.publicationTitle ?? "示例社会科学学报", volume: "36", issue: "3", pages: "45-70", ISSN: "0000-0001" } };
  if (i.fields.ISBN) return { source: "openlibrary", fields: { title: i.fields.title ?? "Found book", publisher: "Synthetic Academic Press", place: "London", date: i.fields.date ?? "2015", numPages: "320" } };
  return null;
}
function mockLookup(i: MItem, trigger: string): "skipped" | "found" | "not_found" {
  const key = identifierOf(i);
  if (!key || i.deletedAt) return "skipped";
  if (trigger === "import" && lookups.get(i.id)?.identifier === key) return "skipped";
  const f = fakeFound(i);
  const base = { id: lookupSeq++, itemId: i.id, identifier: key, trigger, createdAt: now(), updatedAt: now() };
  if (!f) {
    lookups.set(i.id, { ...base, status: "not_found", source: null, message: "doi.org 上没有这个 DOI（可能写错了）", changes: [] });
    return "not_found";
  }
  const changes: LookupChange[] = [];
  for (const [k, v] of Object.entries(f.fields)) {
    const cur = i.fields[k];
    if (!cur) {
      changes.push({ field: k, current: null, found: v, kind: "fill", state: "applied" });
      i.fields[k] = v;
    } else if (CONFLICT.includes(k) && norm(cur) !== norm(v) && !(k === "date" && cur.slice(0, 4) === v.slice(0, 4))) {
      changes.push({ field: k, current: cur, found: v, kind: "conflict", state: "pending" });
    }
  }
  lookups.set(i.id, { ...base, status: "found", source: f.source, message: null, changes });
  return "found";
}
function startLookups(ids: number[], trigger: string) {
  const fresh = !progress.running;
  for (const id of ids) if (!lookupQueue.some(([x]) => x === id)) lookupQueue.push([id, trigger]);
  if (!fresh) {
    progress.total = progress.done + lookupQueue.length;
    return;
  }
  Object.assign(progress, { running: true, run: progress.run + 1, total: lookupQueue.length, done: 0, filledItems: 0, filledFields: 0, conflictItems: 0, notFound: 0, failed: 0, skipped: 0, stopped: null, finishedAt: null });
  lookupCancel = false;
  const tick = setInterval(() => {
    const job = lookupQueue.shift();
    if (lookupCancel || !job) {
      if (lookupCancel) {
        progress.stopped = "已停止";
        progress.total = progress.done;
        lookupQueue = [];
      }
      Object.assign(progress, { running: false, finishedAt: now() });
      clearInterval(tick);
      return;
    }
    const i = items.find((x) => x.id === job[0]);
    const r = i ? mockLookup(i, job[1]) : "skipped";
    progress.done++;
    if (r === "skipped") progress.skipped++;
    if (r === "not_found") progress.notFound++;
    if (r === "found" && i) {
      const ch = lookups.get(i.id)!.changes;
      const filled = ch.filter((c) => c.state === "applied").length;
      progress.filledFields += filled;
      progress.filledItems += filled ? 1 : 0;
      progress.conflictItems += ch.some((c) => c.state === "pending") ? 1 : 0;
    }
  }, 700);
}
const requireEnabled = () => {
  if (mSettings.enabled === false) fail("联网补全已关闭，可以在「设置」里打开");
  if (mSettings.enabled === null) fail("还没有打开联网补全：第一次使用前请先确认联网范围");
};
const withIdentifier = () => items.filter((i) => !i.deletedAt && identifierOf(i)).map((i) => i.id);
function findLookup(id: unknown): LookupView {
  return [...lookups.values()].find((l) => l.id === id) ?? fail("找不到这次联网补全的记录");
}

const CONTAINER: Record<string, string> = { journalArticle: "publicationTitle", bookSection: "bookTitle", conferencePaper: "proceedingsTitle", webpage: "websiteTitle" };
const containerField = (i: MItem) => ["publicationTitle", "bookTitle", "proceedingsTitle", "websiteTitle"].find((f) => i.fields[f]) ?? CONTAINER[i.itemType] ?? null;
const year = (i: MItem) => Number(/\d{4}/.exec(i.fields.date ?? "")?.[0]) || null;
const norm = (s: string) => s.toLowerCase().replace(/[\p{P}\s]+/gu, "");
const live = (c: MColl): boolean => !c.deletedAt && (c.parentId === null || live(colls.find((p) => p.id === c.parentId)!));
const liveColls = () => colls.filter(live);
const named = (ids: number[]) => liveColls().filter((c) => ids.includes(c.id)).map((c) => ({ id: c.id, name: c.name }));
const fail = (msg: string): never => {
  throw msg;
};
const findItem = (id: unknown) => items.find((i) => i.id === id) ?? fail("找不到这条文献，可能已被删除");

function dupGroups(): { ids: number[]; reason: string }[] {
  const alive = items.filter((i) => !i.deletedAt);
  const groups: { ids: number[]; reason: string }[] = [];
  const used = new Set<number>();
  for (const a of alive) {
    if (used.has(a.id)) continue;
    const g = alive.filter((b) => b.id !== a.id && !dismissed.has([a.id, b.id].sort().join("-")) && ((a.fields.DOI && a.fields.DOI.toLowerCase() === b.fields.DOI?.toLowerCase()) || (norm(a.fields.title ?? "") === norm(b.fields.title ?? "") && year(a) === year(b))));
    if (g.length) {
      const ids = [a.id, ...g.map((b) => b.id)];
      ids.forEach((i) => used.add(i));
      groups.push({ ids, reason: g.some((b) => b.fields.DOI && b.fields.DOI.toLowerCase() === a.fields.DOI?.toLowerCase()) ? "DOI 相同" : "标题与年份相同" });
    }
  }
  return groups;
}

const itemRow = (i: MItem): ListRow => {
  const kids = atts.filter((a) => a.itemId === i.id && !a.deletedAt);
  return {
    kind: "item", id: i.id, uid: i.uid, itemType: i.itemType, title: i.fields.title ?? null, firstCreator: i.creators[0]?.name ?? null, creatorCount: i.creators.length,
    year: year(i), containerTitle: i.fields[containerField(i) ?? ""] ?? null, createdAt: i.createdAt, deletedAt: i.deletedAt,
    attachments: kids.length, notes: notes.filter((n) => n.itemId === i.id && !n.deletedAt).length, missingFiles: kids.filter((a) => a.status === "missing").length,
  };
};
const blankRow = { firstCreator: null, creatorCount: 0, year: null, containerTitle: null, attachments: 0, notes: 0, missingFiles: 0 };

function list(q: ListQuery): ListRow[] {
  const s = q.scope;
  const inColl = (ids: number[]) => {
    if (s.kind !== "collection") return true;
    const want = new Set([s.id]);
    if (s.includeChildren) for (let grew = true; grew; ) { grew = false; for (const c of liveColls()) if (c.parentId !== null && want.has(c.parentId) && !want.has(c.id)) { want.add(c.id); grew = true; } }
    return ids.some((x) => want.has(x));
  };
  const unfiled = (ids: number[]) => s.kind !== "unfiled" || named(ids).length === 0;
  const groups = dupGroups();
  const groupOf = (id: number) => groups.findIndex((g) => g.ids.includes(id));
  const trash = s.kind === "trash";
  let rows: ListRow[] = items
    .filter((i) => (trash ? i.deletedAt : !i.deletedAt) && inColl(i.collectionIds) && unfiled(i.collectionIds))
    .filter((i) => s.kind !== "missing" || itemRow(i).missingFiles > 0)
    .filter((i) => s.kind !== "duplicates" || groupOf(i.id) >= 0)
    .filter((i) => s.kind !== "review" || reviewIds().includes(i.id))
    .map((i) => ({ ...itemRow(i), ...(s.kind === "duplicates" ? { group: groupOf(i.id), groupReason: groups[groupOf(i.id)].reason } : {}) }));
  if (s.kind !== "duplicates" && s.kind !== "review") {
    rows.push(...atts.filter((a) => (trash ? a.deletedAt : !a.deletedAt && a.itemId === null) && inColl(a.collectionIds) && unfiled(a.collectionIds) && (s.kind !== "missing" || a.status === "missing")).map((a): ListRow => ({ ...blankRow, kind: "attachment", id: a.id, uid: `A${a.id}`, itemType: a.contentType, title: a.title, createdAt: a.createdAt, deletedAt: a.deletedAt, missingFiles: a.status === "missing" ? 1 : 0 })));
    if (s.kind !== "missing") rows.push(...notes.filter((n) => (trash ? n.deletedAt : !n.deletedAt && n.itemId === null) && inColl(n.collectionIds) && unfiled(n.collectionIds)).map((n): ListRow => ({ ...blankRow, kind: "note", id: n.id, uid: `N${n.id}`, itemType: null, title: n.title, createdAt: n.createdAt, deletedAt: n.deletedAt })));
  }
  if (trash) rows.push(...colls.filter((c) => c.deletedAt).map((c): ListRow => ({ ...blankRow, kind: "collection", id: c.id, uid: `C${c.id}`, itemType: null, title: c.name, createdAt: c.createdAt, deletedAt: c.deletedAt })));
  if (q.itemType) rows = rows.filter((r) => (q.itemType === "attachment" || q.itemType === "note" ? r.kind === q.itemType : r.kind === "item" && r.itemType === q.itemType));
  if (q.tags?.length) rows = rows.filter((r) => { const tg = r.kind === "item" ? findItem(r.id).tags : r.kind === "attachment" ? atts.find((a) => a.id === r.id)!.tags : r.kind === "note" ? notes.find((n) => n.id === r.id)!.tags : []; return q.tags!.every((x) => tg.includes(x)); });
  const needle = (q.q ?? "").trim().toLowerCase();
  if (needle) rows = rows.filter((r) => {
    const hay = r.kind === "item" ? (() => { const i = findItem(r.id); return [Object.values(i.fields).join(" "), i.creators.map((c) => c.name).join(" "), i.tags.join(" "), notes.filter((n) => n.itemId === i.id).map((n) => n.text).join(" ")].join(" "); })() : r.kind === "note" ? notes.find((n) => n.id === r.id)!.text : (r.title ?? "");
    return needle.split(/\s+/).every((w) => hay.toLowerCase().includes(w));
  });
  const key = q.sort ?? (needle ? "relevance" : "added");
  const desc = q.sort ? !!q.desc : key === "added";
  const val = (r: ListRow): string | number | null => ({ title: r.title?.toLowerCase() ?? null, creator: r.firstCreator?.toLowerCase() ?? null, year: r.year, type: r.itemType, relevance: 0, added: trash ? r.deletedAt : r.createdAt } as Record<string, string | number | null>)[key] ?? null;
  return rows.sort((a, b) => {
    if (s.kind === "duplicates") return (a.group ?? 0) - (b.group ?? 0);
    const [x, y] = [val(a), val(b)];
    if (x === null || y === null) return x === y ? 0 : x === null ? 1 : -1;
    return (x < y ? -1 : x > y ? 1 : 0) * (desc ? -1 : 1);
  });
}

function itemDetail(i: MItem): ItemDetail {
  const cf = containerField(i);
  const shown = ["title", "date", cf, "DOI", "ISBN", "language", "abstractNote", "extra"];
  return {
    id: i.id, uid: i.uid, itemType: i.itemType, title: i.fields.title ?? null, creators: i.creators, date: i.fields.date ?? null, year: year(i),
    containerTitle: cf ? (i.fields[cf] ?? null) : null, containerField: cf, doi: i.fields.DOI ?? null, isbn: i.fields.ISBN ?? null, language: i.fields.language ?? null,
    abstract: i.fields.abstractNote ?? null, extra: i.fields.extra ?? null,
    otherFields: Object.entries(i.fields).filter(([k]) => !shown.includes(k)).map(([name, value]) => ({ name, value })).sort((a, b) => a.name.localeCompare(b.name)),
    tags: [...i.tags].sort(), collections: named(i.collectionIds),
    attachments: atts.filter((a) => a.itemId === i.id && !a.deletedAt).map((a) => ({ id: a.id, title: a.title, kind: a.kind, contentType: a.contentType, status: a.status, size: a.size, url: null, readable: readable(a), annotationCount: annCount(a.id) })),
    notes: notes.filter((n) => n.itemId === i.id && !n.deletedAt).map((n) => ({ id: n.id, title: n.title, preview: n.text.split("\n").slice(1).join(" ") })),
    source: i.source, createdAt: i.createdAt, updatedAt: i.updatedAt, deletedAt: i.deletedAt,
  };
}

const toCreator = (c: CreatorInput): Creator | null => {
  const lit = c.literal?.trim();
  if (lit) return single(lit, c.role);
  const [f, g] = [c.family?.trim() ?? "", c.given?.trim() ?? ""];
  return f || g ? { role: c.role, name: `${f} ${g}`.trim(), family: f || null, given: g || null, literal: null } : null;
};

function applyPatch(i: MItem, p: ItemPatch) {
  if (p.itemType && p.itemType !== i.itemType) {
    const old = containerField(i);
    const next = CONTAINER[p.itemType];
    if (old && next && old !== next && i.fields[old] && !i.fields[next]) {
      i.fields[next] = i.fields[old];
      delete i.fields[old];
    }
    i.itemType = p.itemType;
  }
  for (const [k0, v] of Object.entries(p.fields ?? {})) {
    const k = k0 === "container" ? (containerField(i) ?? fail("这种类型没有「期刊 / 书名」")) : k0;
    if (v.trim()) i.fields[k] = v.trim();
    else delete i.fields[k];
  }
  if (p.creators) i.creators = p.creators.map(toCreator).filter((c): c is Creator => c !== null);
  i.updatedAt = now();
}

type Tagged = { tags: string[] };
const tagHolder = (tg: Target): Tagged =>
  tg.kind === "item" ? findItem(tg.id) : tg.kind === "attachment" ? atts.find((a) => a.id === tg.id)! : tg.kind === "note" ? notes.find((n) => n.id === tg.id)! : fail("集合不能加标签");
const rowOf = (tg: Target): { deletedAt: string | null; collectionIds?: number[]; itemId?: number | null } =>
  tg.kind === "item" ? findItem(tg.id) : tg.kind === "attachment" ? atts.find((a) => a.id === tg.id)! : tg.kind === "note" ? notes.find((n) => n.id === tg.id)! : colls.find((c) => c.id === tg.id)!;
const allTagged = (): Tagged[] => [...items, ...atts, ...notes];

const checks = (labels: [string, number][]) => labels.map(([label, n], i) => ({ category: `c${i}`, label, source: n, stored: n, ok: true }));
const runs: RunDetail[] = [];
let nextRun = 1;
const zoteroPreview = (dir: string, baseDir: string | null): ZoteroPreview => ({
  dir, clientVersion: "9.0.3", userdataVersion: 125, versionTested: true, items: 21, attachments: 13, notes: 4, annotations: 4, collections: 8, tags: 9,
  trashItems: 3, trashCollections: 1, groupLibraries: 1, files: baseDir ? 10 : 9, bytes: 6_400_000,
  missingFiles: [
    { key: "ATT00008", title: "Full Text PDF", parentTitle: "Measuring External Efficacy", reason: "Zotero 的 storage 里没有这个文件（可能只在 Zotero 云端，需要先在 Zotero 里下载）", inTrash: false },
    ...(baseDir ? [] : [{ key: "ATT00007", title: null, parentTitle: "Efficacy Online", reason: "使用相对路径，需要提供 Zotero 的「链接附件根目录」", inTrash: false }]),
  ],
  baseRelative: 1, alreadyImported: runs.some((r) => r.source === "zotero") ? 21 : 0, zoteroRunning: false,
  warnings: ["有 1 个群组库：其中的条目会并入本库（Pith 没有群组）。"],
  blocked: runs.some((r) => r.source === "zotero") ? "这个 Zotero 库已经导入过（21 条记录已在库里）。重复导入与增量更新在 v0.1.x 做；要重新导入，请先恢复导入前的备份。" : null,
});
const filePreview = (path: string): FilePreview => ({
  path, fileName: path.split("/").pop() ?? path, format: path.endsWith(".ris") ? "ris" : path.endsWith(".json") ? "csl-json" : "bibtex",
  records: 3, byType: [{ itemType: "journalArticle", count: 2 }, { itemType: "book", count: 1 }],
  duplicates: [{ index: 3, title: "示例：公民政治效能感的测量与比较", reason: "DOI 相同（10.0000/example.2021.001）" }],
  samples: ["A Synthetic Panel Study", "国民财富的性质和原因的研究", "示例：公民政治效能感的测量与比较"], notes: 1,
  warnings: ["第 2 条「国民财富的性质和原因的研究」：作者列表里的 others 已忽略"],
});
function finishRun(source: string, path: string, added: number, extra: Partial<RunDetail["stats"]> = {}): RunDetail {
  const fresh: number[] = [];
  for (let k = 0; k < added; k++) fresh.push(nextId), items.push({ id: nextId++, uid: `M${nextId}`, itemType: "journalArticle", fields: { title: `导入的示例文献 ${nextId}`, date: "2019", publicationTitle: "Journal of Synthetic Politics", DOI: `10.5555/mock.${nextId}` }, creators: [person("Smith", "Alice")], tags: [], collectionIds: [], source, createdAt: now(), updatedAt: now(), deletedAt: null });
  const run: RunDetail = {
    id: nextRun++, source, sourcePath: path, backupFile: "library-20261108-101500-before-import.db", startedAt: now(), finishedAt: now(),
    stats: { items: added, attachments: 0, notes: 0, annotations: 0, collections: 0, newTags: 0, filesCopied: 0, filesReused: 0, bytesCopied: 0, missingFiles: 0, trash: 0, skipped: 0, ...extra },
    report: {
      checks: checks(source === "zotero" ? [["条目（全部类型）", 42], ["字段值", 318], ["作者 / 编者", 61], ["附件文件", 11], ["笔记", 4]] : [["文献条目", added], ["字段值", added * 6], ["作者 / 编者", added * 2]]),
      sourceChecks: source === "zotero" ? checks([["条目（全部类型）", 42], ["字段值", 318], ["附件记录", 13]]) : [],
      missingFiles: source === "zotero" ? zoteroPreview(path, null).missingFiles : [], fileWarnings: [],
      notMigrated: source === "zotero" ? [{ label: "Zotero 全文索引（可从 PDF 重建）", count: 2 }] : [],
      skipped: extra.skipped ? ["第 3 条「示例：公民政治效能感的测量与比较」可能重复（DOI 相同），没有导入"] : [], warnings: [],
    },
  };
  runs.unshift(run);
  // 打开了自动补全：这次导入的排进队列（与 Rust 一致）
  if (mSettings.enabled && mSettings.auto) startLookups(fresh, "import");
  return run;
}

// ---- 备份与整库导出（模拟：不真的换库，只返回结果） ----
const pad = (n: number) => String(n).padStart(2, "0");
const stampOf = (d: Date) => `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
function addBackup(label: string): BackupInfo {
  const stamp = stampOf(new Date());
  const b: BackupInfo = { fileName: `library-${stamp}-${label}.db`, label, createdAt: `${stamp.slice(0, 4)}-${stamp.slice(4, 6)}-${stamp.slice(6, 8)} ${stamp.slice(9, 11)}:${stamp.slice(11, 13)}:${stamp.slice(13, 15)}`, size: 286720 };
  backups.unshift(b);
  return b;
}
const counts = (): LibraryCounts => ({
  items: items.length,
  attachments: atts.length,
  notes: notes.length,
  annotations: anns.length,
  collections: colls.length,
  tags: new Set([...items, ...atts, ...notes].flatMap((r) => r.tags)).size,
  files: atts.filter((a) => a.status === "ok").length,
  bytes: atts.reduce((n, a) => n + (a.size ?? 0), 0),
});
const tableChecks = (): Check[] =>
  (
    [
      ["items", "文献", items.length],
      ["item_fields", "文献字段", items.reduce((n, i) => n + Object.keys(i.fields).length, 0)],
      ["creators", "作者", items.reduce((n, i) => n + i.creators.length, 0)],
      ["collections", "集合", colls.length],
      ["attachments", "附件", atts.length],
      ["notes", "笔记", notes.length],
      ["annotations", "标注", anns.length],
      ["settings", "设置", 3],
    ] as const
  ).map(([category, label, n]) => ({ category, label, source: n, stored: n, ok: true }));

const handlers: Record<string, (a: Args) => unknown> = {
  app_info: () => info,
  app_reveal_data_dir: () => null,
  library_overview: () => ({
    items: items.filter((i) => !i.deletedAt).length,
    trash: [...items, ...atts, ...notes, ...colls].filter((x) => x.deletedAt).length,
    collections: liveColls().length, tags: new Set(allTagged().flatMap((x) => x.tags)).size,
    attachments: atts.filter((a) => !a.deletedAt).length, notes: notes.filter((n) => !n.deletedAt).length,
    missingFiles: atts.filter((a) => !a.deletedAt && a.status === "missing").length,
    unfiled: items.filter((i) => !i.deletedAt && named(i.collectionIds).length === 0).length, duplicates: dupGroups().length,
    review: reviewIds().filter((id) => !findItem(id).deletedAt).length,
  }),
  items_list: ({ query }) => list(query as ListQuery),
  tags_list: () => {
    const counts = new Map<string, number>();
    for (const x of [...items, ...atts, ...notes].filter((x) => !x.deletedAt)) for (const tg of x.tags) counts.set(tg, (counts.get(tg) ?? 0) + 1);
    return [...counts].map(([name, count]) => ({ id: tagId(name), name, count })).sort((a, b) => a.name.localeCompare(b.name));
  },
  items_get: ({ id }) => itemDetail(findItem(id)),
  items_create: ({ itemType, collectionId }) => {
    const id = nextId++;
    items.push({ id, uid: `M${id}`, itemType: itemType as string, fields: {}, creators: [], tags: [], collectionIds: collectionId ? [collectionId as number] : [], source: "manual", createdAt: now(), updatedAt: now(), deletedAt: null });
    return id;
  },
  items_update: ({ id, patch }) => {
    const p = patch as ItemPatch;
    applyPatch(findItem(id), p);
    const touched = Object.entries(p.fields ?? {}).some(([k, v]) => ["DOI", "ISBN", "extra"].includes(k) && v.trim());
    if (touched && mSettings.enabled && mSettings.auto) startLookups([id as number], "edit");
  },
  word_status: () => wordStatus(),
  word_install: () => {
    wordInstalledAt = now();
    return { status: wordStatus().install, templateError: null };
  },
  word_install_into: () => {
    wordInstalledAt = now();
    return { status: wordStatus().install, templateError: null };
  },
  word_uninstall: () => {
    wordInstalledAt = null;
    return wordStatus().install;
  },
  word_reveal: () => undefined,
  word_reveal_file: () => undefined,
  cite_styles_list: () => citeStyles(),
  cite_style_import: () => {
    const style = { id: "http://www.zotero.org/styles/synthetic-journal", label: "Synthetic Journal of Social Science", en: "Synthetic Journal of Social Science", file: "synthetic-journal.csl", uri: "http://www.zotero.org/styles/synthetic-journal", kind: "author-date", imported: true };
    const replaced = importedStyles.some((x) => x.id === style.id);
    if (!replaced) importedStyles.push(style);
    return { style, replaced };
  },
  cite_style_remove: ({ id }) => {
    importedStyles = importedStyles.filter((x) => x.id !== id);
  },
  word_zotero_inspect: ({ path }) => ({
    found: { codes: Array(63).fill(" ADDIN ZOTERO_ITEM CSL_CITATION {}"), bibliography: true, bibliographyTexts: [], fields: 64, zoteroStyle: "http://www.zotero.org/styles/china-national-standard-gb-t-7714-2015-numeric", noteStyle: false, hasPith: false },
    stats: { citations: 63, converted: 63, items: 87, inLibrary: 85, fromDocument: 2, missing: 0, edited: 0 },
    style: "GB/T 7714-2015 顺序编码制",
    out: String(path).replace(/\.docx$/, "（Pith）.docx"),
  }),
  word_zotero_convert: ({ path }) => ({
    out: String(path).replace(/\.docx$/, "（Pith）.docx"),
    report: {
      stats: { citations: 63, converted: 63, items: 87, inLibrary: 85, fromDocument: 2, missing: 0, edited: 0 },
      notes: ["第 12 处：「示例：不在文献库里的文献」不在文献库里，用文档里自带的数据"],
      bibliography: true,
      zoteroStyle: "http://www.zotero.org/styles/china-national-standard-gb-t-7714-2015-numeric",
      style: "gbt7714-2015-numeric",
      styleNote: null,
      left: 0,
      sameCitations: 63,
      citationDiffs: [],
      zoteroEntries: 61,
      sameEntries: 60,
      entryDiffs: [{ index: 6, zotero: "[7]\t示例作者. 示例报告: 卷 46 46[R/OL].", pith: "[7]\t示例作者. 示例报告: 46[R/OL]." }],
    },
  }),
  picker_session: () => pickerSession,
  picker_search: ({ q }) => {
    const query = String(q ?? "").trim().toLowerCase();
    return items
      .filter((i) => !i.deletedAt && (!query || (i.fields.title ?? "").toLowerCase().includes(query) || i.creators.some((c) => c.name.toLowerCase().includes(query))))
      .slice(0, 50)
      .map(pickInfo);
  },
  picker_done: ({ session, done }) => {
    console.info("mock：搜索窗结束", session, done);
    // 预览时再开一次，方便连着试
    pickerSession = { ...pickerSession, id: pickerSession.id + 1 };
    setTimeout(() => void emit("picker-open", pickerSession), 600);
  },
  metadata_settings_get: () => ({ ...mSettings }),
  metadata_settings_set: ({ patch }) => {
    const p = patch as MetadataSettingsPatch;
    if (p.email !== undefined && p.email.trim() && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(p.email.trim())) fail(`邮箱格式不对：${p.email}`);
    if (p.enabled !== undefined) mSettings.enabled = p.enabled;
    if (p.auto !== undefined) mSettings.auto = p.auto;
    if (p.email !== undefined) mSettings.email = p.email.trim() || null;
    return { ...mSettings };
  },
  metadata_status: () => ({ ...progress }),
  metadata_cancel: () => void (lookupCancel = true),
  metadata_lookup: ({ ids }) => {
    requireEnabled();
    startLookups(ids as number[], "manual");
    return (ids as number[]).length;
  },
  metadata_lookup_run: () => {
    requireEnabled();
    const ids = withIdentifier();
    startLookups(ids, "import");
    return ids.length;
  },
  metadata_lookup_all: () => {
    requireEnabled();
    const ids = withIdentifier();
    startLookups(ids, "import");
    return ids.length;
  },
  metadata_latest: ({ itemId }) => lookups.get(itemId as number) ?? null,
  metadata_resolve: ({ lookupId, field, accept }) => {
    const l = findLookup(lookupId);
    const c = l.changes.find((x) => x.field === field && x.state === "pending") ?? fail("这一处已经处理过了，请刷新");
    if (accept) findItem(l.itemId).fields[c.field] = c.found;
    c.state = accept ? "accepted" : "ignored";
    l.updatedAt = now();
    return l;
  },
  metadata_undo: ({ lookupId }) => {
    const l = findLookup(lookupId);
    const i = findItem(l.itemId);
    let reverted = 0;
    for (const c of l.changes.filter((x) => x.state === "applied")) {
      if (i.fields[c.field] === c.found) {
        delete i.fields[c.field];
        c.state = "undone";
        reverted++;
      } else c.state = "kept";
    }
    return { lookup: l, reverted };
  },
  metadata_add: ({ text, collectionId }) => {
    requireEnabled();
    const report = { created: [] as number[], existing: [] as { identifier: string; itemId: number; title: string | null }[], notFound: [] as { identifier: string; message: string }[], invalid: [] as string[], failed: null };
    for (const tok of String(text).split(/[\s,，;；]+/).filter(Boolean)) {
      const doi = /^(?:https?:\/\/(?:dx\.)?doi\.org\/|doi:)?(10\.\d+\/\S+)$/i.exec(tok)?.[1];
      const isbn = /^[\d-]{10,17}[\dXx]?$/.test(tok) ? tok.replace(/-/g, "") : null;
      if (!doi && !isbn) {
        report.invalid.push(tok);
        continue;
      }
      const label = doi ? `DOI ${doi}` : `ISBN ${isbn}`;
      const hit = items.find((i) => !i.deletedAt && (doi ? i.fields.DOI?.toLowerCase() === doi.toLowerCase() : i.fields.ISBN?.replace(/-/g, "") === isbn));
      if (hit) {
        report.existing.push({ identifier: label, itemId: hit.id, title: hit.fields.title ?? null });
        continue;
      }
      if (tok.includes("missing")) {
        report.notFound.push({ identifier: label, message: "doi.org 上没有这个 DOI（可能写错了）" });
        continue;
      }
      const id = nextId++;
      items.push({
        id, uid: `L${id}`, itemType: doi ? "journalArticle" : "book",
        fields: doi ? { title: `联网添加的示例文章（${doi}）`, DOI: doi, date: "2022-05", publicationTitle: "Journal of Synthetic Studies", volume: "12", issue: "4", pages: "101-120" } : { title: "联网添加的示例图书", ISBN: isbn!, date: "2018", publisher: "Synthetic Press", place: "London" },
        creators: [person("Doe", "Jane")], tags: [], collectionIds: collectionId ? [collectionId as number] : [], source: "lookup", createdAt: now(), updatedAt: now(), deletedAt: null,
      });
      lookups.set(id, { id: lookupSeq++, itemId: id, identifier: doi ? `doi:${doi.toLowerCase()}` : `isbn:${isbn}`, trigger: "add", status: "found", source: doi ? "crossref" : "openlibrary", message: null, changes: [], createdAt: now(), updatedAt: now() });
      report.created.push(id);
    }
    return report;
  },
  items_set_field: ({ ids, field, value }) => (ids as number[]).forEach((id) => applyPatch(findItem(id), field === "itemType" ? { itemType: value as string } : { fields: { [field as string]: value as string } })),
  items_merge: ({ masterId, otherIds }) => {
    const m = findItem(masterId);
    for (const o of (otherIds as number[]).map(findItem)) {
      for (const [k, v] of Object.entries(o.fields)) if (!m.fields[k]) m.fields[k] = v;
      m.tags = [...new Set([...m.tags, ...o.tags])];
      m.collectionIds = [...new Set([...m.collectionIds, ...o.collectionIds])];
      atts.filter((a) => a.itemId === o.id).forEach((a) => (a.itemId = m.id));
      notes.filter((n) => n.itemId === o.id).forEach((n) => (n.itemId = m.id));
      o.deletedAt = now();
    }
  },
  duplicates_dismiss: ({ ids }) => {
    const v = ids as number[];
    v.forEach((a) => v.forEach((b) => a < b && dismissed.add(`${a}-${b}`)));
  },
  attachments_get: ({ id }): AttachmentDetail => {
    const a = atts.find((x) => x.id === id) ?? fail("找不到这个附件，可能已被删除");
    const parent = a.itemId ? findItem(a.itemId) : null;
    return { id: a.id, uid: `A${a.id}`, kind: a.kind, title: a.title, originalName: a.originalName, originalPath: null, url: null, contentType: a.contentType, status: a.status, size: a.size, sha256: null, parent: parent ? { kind: "item", id: parent.id, title: parent.fields.title ?? null } : null, tags: a.tags, collections: named(a.collectionIds), note: null, annotationCount: annCount(a.id), readable: readable(a), createdAt: a.createdAt, deletedAt: a.deletedAt };
  },
  app_open_url: ({ url }) => void console.info(`（预览）打开网址：${url}`),
  reader_get: ({ id }) => {
    const a = atts.find((x) => x.id === id) ?? fail("找不到这个附件，可能已被删除");
    if (a.status === "missing") fail("这个附件的文件缺失，导入时就没有找到");
    if (!readable(a)) fail("这个附件不是 PDF，App 里还不能阅读；请用「打开」交给系统程序");
    const parent = a.itemId ? findItem(a.itemId) : null;
    const readonly = a.deletedAt ? "这个附件在回收站里，只能阅读；恢复后才能加标注" : parent?.deletedAt ? "所属文献在回收站里，只能阅读；恢复后才能加标注" : null;
    return { id: a.id, title: a.title || "PDF", parent: parent ? { kind: "item", id: parent.id, title: parent.fields.title ?? null } : null, size: a.size, readonly, state: readerStates.get(a.id) ?? null };
  },
  reader_save_state: ({ id, reading }) => void readerStates.set(id as number, reading as ReadingState),
  annotations_list: ({ attachmentId }) =>
    anns.filter((x) => x.attachmentId === attachmentId && !x.deletedAt).sort((a, b) => a.sortKey.localeCompare(b.sortKey) || a.id - b.id).map(({ attachmentId: _, deletedAt: __, ...a }) => a),
  annotations_create: ({ input }) => {
    const n = input as NewAnnotation;
    const text = n.text?.replace(/-\n(?=[a-z])/g, "").replace(/\s+/g, " ").trim() ?? null;
    if (n.kind !== "note" && !text) fail("没有选中文字（这一页可能是扫描图片，没有文字层）");
    // 很粗的合并：同一行（上沿相差不到 3pt）的并成一个；真正的规则在 Rust
    const lines: number[][] = [];
    for (const r of [...n.rects].sort((a, b) => b[3] - a[3] || a[0] - b[0])) {
      const l = lines.find((x) => Math.abs(x[3] - r[3]) < 3);
      if (l) [l[0], l[1], l[2], l[3]] = [Math.min(l[0], r[0]), Math.min(l[1], r[1]), Math.max(l[2], r[2]), Math.max(l[3], r[3])];
      else lines.push([...r]);
    }
    const a = ann(nextId++, n.attachmentId, n.kind, n.pageIndex, lines.map((r) => r.map((v) => Math.round(v * 1000) / 1000)), n.kind === "note" ? null : text, n.color ?? "#ffd400", { pageLabel: n.pageLabel, createdAt: now(), updatedAt: now() });
    anns.push(a);
    const { attachmentId: _, deletedAt: __, ...out } = a;
    return out;
  },
  annotations_update: ({ id, patch }) => {
    const a = anns.find((x) => x.id === id && !x.deletedAt) ?? fail("找不到这条标注，可能已被删除");
    if (!a.editable) fail("这条标注来自 PDF 文件本身，不能修改");
    const p = patch as AnnotationPatch;
    if (p.color !== undefined) a.color = p.color;
    if (p.comment !== undefined) a.comment = p.comment.trim() || null;
    if (p.text !== undefined) a.text = p.text.trim() || null;
    a.updatedAt = now();
    const { attachmentId: _, deletedAt: __, ...out } = a;
    return out;
  },
  annotations_delete: ({ ids }) => (ids as number[]).forEach((id) => ((anns.find((x) => x.id === id) ?? fail("找不到这条标注")).deletedAt = now())),
  annotations_restore: ({ ids }) => (ids as number[]).forEach((id) => ((anns.find((x) => x.id === id) ?? fail("找不到这条标注")).deletedAt = null)),
  attachments_open: ({ id }) => (atts.find((a) => a.id === id)?.status === "missing" ? fail("这个附件的文件缺失，导入时就没有找到") : null),
  attachments_add: ({ itemId, path }) => {
    const id = nextId++;
    const name = String(path).split("/").pop() ?? "file";
    atts.push({ id, itemId: (itemId as number | null) ?? null, kind: "stored", title: name, originalName: name, contentType: name.endsWith(".pdf") ? "application/pdf" : "application/octet-stream", status: "ok", size: 123456, tags: [], collectionIds: [], createdAt: now(), deletedAt: null });
    return id;
  },
  attachments_rename: ({ id, title }) => void ((atts.find((a) => a.id === id) ?? fail("找不到这个附件")).title = String(title).trim() || fail("附件名不能为空")),
  notes_get: ({ id }): NoteDetail => {
    const n = notes.find((x) => x.id === id) ?? fail("找不到这条笔记，可能已被删除");
    const parent = n.itemId ? findItem(n.itemId) : null;
    return { id: n.id, uid: `N${n.id}`, title: n.title, text: n.text, parent: parent ? { kind: "item", id: parent.id, title: parent.fields.title ?? null } : null, tags: n.tags, collections: named(n.collectionIds), imageCount: n.imageCount, createdAt: n.createdAt, updatedAt: n.updatedAt, deletedAt: n.deletedAt };
  },
  tags_add: ({ targets, name }) => {
    const nm = String(name).trim() || fail("标签不能为空");
    for (const tg of targets as Target[]) { const h = tagHolder(tg); if (!h.tags.includes(nm)) h.tags.push(nm); }
  },
  tags_remove: ({ targets, name }) => (targets as Target[]).forEach((tg) => { const h = tagHolder(tg); h.tags = h.tags.filter((x) => x !== name); }),
  tags_rename: ({ id, name }) => {
    const old = [...tagIds].find(([, v]) => v === id)?.[0] ?? fail("找不到这个标签");
    for (const h of allTagged()) if (h.tags.includes(old)) h.tags = [...new Set(h.tags.map((x) => (x === old ? String(name) : x)))];
  },
  tags_delete: ({ id }) => {
    const old = [...tagIds].find(([, v]) => v === id)?.[0];
    for (const h of allTagged()) h.tags = h.tags.filter((x) => x !== old);
  },
  collections_list: (): CollectionNode[] =>
    liveColls().map((c) => ({ id: c.id, parentId: c.parentId, name: c.name, itemCount: [...items, ...atts.filter((a) => a.itemId === null), ...notes.filter((n) => n.itemId === null)].filter((x) => !x.deletedAt && x.collectionIds.includes(c.id)).length })).sort((a, b) => a.name.localeCompare(b.name)),
  collections_get: ({ id }) => {
    const c = colls.find((x) => x.id === id) ?? fail("找不到这个集合");
    return { id: c.id, name: c.name, parentName: colls.find((p) => p.id === c.parentId)?.name ?? null, children: colls.filter((k) => k.parentId === c.id).length, members: items.filter((i) => i.collectionIds.includes(c.id)).length, createdAt: c.createdAt, deletedAt: c.deletedAt };
  },
  collections_create: ({ name, parentId }) => {
    const id = nextId++;
    colls.push({ id, parentId: (parentId as number | null) ?? null, name: String(name).trim() || fail("集合名不能为空"), createdAt: now(), deletedAt: null });
    return id;
  },
  collections_rename: ({ id, name }) => void ((colls.find((c) => c.id === id) ?? fail("找不到这个集合")).name = String(name)),
  collections_move: ({ id, parentId }) => {
    for (let p = parentId as number | null; p !== null; p = colls.find((c) => c.id === p)?.parentId ?? null) if (p === id) fail("不能把集合移到它自己或它的下级集合里");
    colls.find((c) => c.id === id)!.parentId = (parentId as number | null) ?? null;
  },
  collections_add: ({ id, targets }) => {
    let n = 0;
    for (const tg of targets as Target[]) {
      const r = rowOf(tg);
      if (tg.kind === "collection" || (r.itemId !== undefined && r.itemId !== null) || !r.collectionIds) continue;
      if (!r.collectionIds.includes(id as number)) { r.collectionIds.push(id as number); n++; }
    }
    return n;
  },
  collections_remove: ({ id, targets }) => {
    let n = 0;
    for (const tg of targets as Target[]) { const r = rowOf(tg); if (r.collectionIds?.includes(id as number)) { r.collectionIds.splice(r.collectionIds.indexOf(id as number), 1); n++; } }
    return n;
  },
  trash_move: ({ targets }) => (targets as Target[]).forEach((tg) => { const r = rowOf(tg); r.deletedAt ??= now(); }),
  trash_restore: ({ targets }) => (targets as Target[]).forEach((tg) => {
    rowOf(tg).deletedAt = null;
    const parent = (rowOf(tg).itemId ?? null) as number | null;
    if (parent) findItem(parent).deletedAt = null;
  }),
  trash_delete: ({ targets }) => {
    for (const tg of targets as Target[]) {
      const arr: { id: number }[] = tg.kind === "item" ? items : tg.kind === "attachment" ? atts : tg.kind === "note" ? notes : colls;
      arr.splice(arr.findIndex((x) => x.id === tg.id), 1);
    }
    return { records: (targets as Target[]).length, filesRemoved: 0, backupFile: "library-20261108-120000-before-delete.db" };
  },
  trash_empty: () => {
    const n = [...items, ...atts, ...notes, ...colls].filter((x) => x.deletedAt).length;
    for (const arr of [items, atts, notes, colls] as { deletedAt: string | null }[][]) for (let i = arr.length - 1; i >= 0; i--) if (arr[i].deletedAt) arr.splice(i, 1);
    return { records: n, filesRemoved: 1, backupFile: "library-20261108-120000-before-delete.db" };
  },
  search_rebuild: () => items.length + atts.length + notes.length,
  backups_list: () => backups,
  backups_create: () => addBackup("manual"),
  backups_restore: ({ fileName }) => {
    const b = backups.find((x) => x.fileName === fileName) ?? fail("找不到这份备份，可能已经被清理了");
    const safety = addBackup("before-restore");
    return { source: b.fileName, safetyBackup: safety.fileName, fromSchema: 5, toSchema: 5, counts: counts(), checks: [], filesCopied: 0, missingFiles: 0 } satisfies RestoreReport;
  },
  export_create: ({ dir }) => ({ path: `${dir}/Pith 导出 ${stampOf(new Date())}`, counts: counts(), checks: tableChecks(), missingFiles: [] }),
  export_reveal: () => null,
  export_inspect: ({ dir }) => {
    if (!String(dir).includes("导出")) fail("这个文件夹里没有 library.json，不是 Pith 的整库导出");
    return { path: String(dir), exportedAt: "2026-11-09T05:00:00.000Z", appVersion: "0.1.0", schemaVersion: 5, counts: counts(), missingFiles: 1 };
  },
  export_restore: ({ dir }) => {
    const safety = addBackup("before-restore");
    return { source: String(dir), safetyBackup: safety.fileName, fromSchema: 5, toSchema: 5, counts: counts(), checks: tableChecks(), filesCopied: 2, missingFiles: 1 } satisfies RestoreReport;
  },
  import_zotero_preview: ({ dir, baseDir }) => zoteroPreview(dir as string, (baseDir as string | null) ?? null),
  import_zotero_run: ({ dir }) => {
    if (runs.some((r) => r.source === "zotero")) fail("这个 Zotero 库已经导入过（21 条记录已在库里），没有重复导入。");
    return finishRun("zotero", dir as string, 3, { attachments: 13, notes: 4, annotations: 4, collections: 8, newTags: 9, filesCopied: 9, filesReused: 1, bytesCopied: 6_400_000, missingFiles: 1, trash: 4 });
  },
  import_file_preview: ({ path }) => filePreview(path as string),
  import_file_run: ({ path, skipDuplicates, intoCollection }) =>
    finishRun(filePreview(path as string).format, path as string, skipDuplicates ? 2 : 3, { skipped: skipDuplicates ? 1 : 0, notes: 1, collections: intoCollection ? 1 : 0, newTags: 2 }),
  import_runs_list: () => runs.map(({ report: _, ...s }) => s),
  import_runs_get: ({ id }) => runs.find((r) => r.id === id) ?? fail("找不到这次导入的记录"),
  // 排版 Agent（模拟实现和示例数据在 mockAgent.ts）
  ...agentHandlers,
  // 事实调查（模拟实现和示例数据在 mockResearch.ts）
  ...researchHandlers,
  ...conceptHandlers,
  // 系统对话框（@tauri-apps/plugin-dialog）：返回合成路径
  "plugin:dialog|open": (a) => {
    const o = (a.options ?? {}) as { directory?: boolean; multiple?: boolean; title?: string; filters?: unknown[] };
    // 排版：规范文档（多选）、要排的论文
    if (o.multiple) return ["/Users/mock/Documents/学校规范/某某大学本科毕业论文撰写规范（示例）.docx", "/Users/mock/Documents/学校规范/论文模板.docx"];
    if (o.title?.includes("要排版的论文")) return "/Users/mock/Documents/毕业论文/论文二稿.docx";
    // 调查：手动加的来源文件、导出报告的文件夹
    if (o.title?.includes("要加进来的文件")) return "/Users/mock/Downloads/示例市统计年鉴2024.xlsx";
    if (o.title?.includes("导出到哪个文件夹")) return "/Users/mock/Documents/调查报告";
    if (o.directory) {
      if (o.title?.includes("整库导出的文件夹")) return "/Users/mock/Pith Dev/exports/Pith 导出 20261109-130000";
      if (o.title?.includes("导出到")) return "/Users/mock/Pith Dev/exports";
      return o.title?.includes("根目录") ? "/Users/mock/Documents/papers" : "/Users/mock/ResearchLab-test/zotero-copy";
    }
    return o.filters ? "/Users/mock/Downloads/refs.bib" : "/Users/mock/Downloads/新下载的论文.pdf";
  },
};

// ---------- Word 集成 ----------

let wordInstalledAt: string | null = null;

function wordStatus() {
  const dataDir = "/Users/mock/Pith Dev";
  return {
    install: {
      record: wordInstalledAt ? { version: "0.2.0", hash: "mock", template: "/Users/mock/Library/Group Containers/UBF8T346G9.Office/User Content.localized/Startup.localized/Word/Pith.dotm", installedAt: wordInstalledAt } : null,
      upToDate: wordInstalledAt !== null,
      scriptInstalled: wordInstalledAt !== null,
      bridgeDataDir: wordInstalledAt ? dataDir : null,
      bridgeLaunchMissing: false,
      startupDir: "/Users/mock/Library/Group Containers/UBF8T346G9.Office/User Content.localized/Startup.localized/Word",
      scriptsDir: "/Users/mock/Library/Application Scripts/com.microsoft.Word",
      manualTemplate: null,
    },
    wordInstalled: true,
    servicePort: 51234,
    serviceError: null,
    rendererReady: true,
    dataDir,
    styles: citeStyles(),
  };
}

let importedStyles: { id: string; label: string; en: string; file: string; uri: string; kind: string; imported: boolean; parent?: string }[] = [];

function citeStyles() {
  const builtin = [
    { id: "gbt7714-2015-numeric", label: "GB/T 7714-2015 顺序编码制", en: "GB/T 7714-2015 (numeric)", kind: "numeric", locator: "after-brackets" },
    { id: "gbt7714-2015-author-date", label: "GB/T 7714-2015 著者-出版年制", en: "GB/T 7714-2015 (author-date)", kind: "author-date" },
    { id: "apa", label: "APA 第 7 版", en: "APA 7th edition", kind: "author-date" },
    { id: "chicago-author-date", label: "Chicago 著者-日期", en: "Chicago (author-date)", kind: "author-date" },
    { id: "gbt7714-2025-numeric", label: "GB/T 7714-2025 顺序编码制", en: "GB/T 7714-2025 (numeric)", kind: "numeric", locator: "after-brackets" },
  ].map((x) => ({ ...x, file: "", uri: `http://www.zotero.org/styles/${x.id}`, imported: false }));
  return [...builtin, ...importedStyles];
}

function pickInfo(i: MItem) {
  return {
    uid: i.uid,
    title: i.fields.title ?? "（无标题）",
    creators: i.creators.length > 1 ? `${i.creators[0].name} 等` : (i.creators[0]?.name ?? ""),
    year: i.fields.date ? Number(i.fields.date.slice(0, 4)) : null,
    container: i.fields.publicationTitle ?? null,
    itemType: i.itemType,
    inLibrary: true,
    inTrash: !!i.deletedAt,
  };
}

// 搜索窗预览：?mock&window=picker 是新插入；加 &edit 是编辑一处已有引文（预填两条，其中一条不在文献库里）
let pickerSession = new URLSearchParams(location.search).has("edit")
  ? {
      id: 1,
      editing: true,
      items: [
        { cite: { id: "U1", locator: "17" }, info: pickInfo(items[0]) },
        { cite: { id: "zotero-u-SYNTH006" }, info: { uid: "zotero-u-SYNTH006", title: "Open data for local labour markets", creators: "Lindqvist", year: 2021, container: null, itemType: null, inLibrary: false, inTrash: false } },
      ],
    }
  : { id: 1, editing: false, items: [] as { cite: { id: string; locator?: string }; info: ReturnType<typeof pickInfo> }[] };

export async function installMockIpc() {
  const picker = new URLSearchParams(location.search).get("window") === "picker";
  mockWindows(picker ? "picker" : "main");
  mockIPC(
    (cmd, payload) => {
      const h = handlers[cmd];
      if (!h) throw `mock 里没有命令 ${cmd}`;
      return h((payload ?? {}) as Args);
    },
    { shouldMockEvents: true },
  );
  // rl-file 协议在浏览器里没有：按附件 id 给一份合成 PDF（blob 地址）。
  // 调阅读器时可以 ?mock&pdf=/local/某个.pdf：所有附件都打开这个文件（放在不进 Git 的 local/ 里）
  const internals = (window as unknown as { __TAURI_INTERNALS__: { convertFileSrc: (path: string, protocol?: string) => string } }).__TAURI_INTERNALS__;
  const realPdf = new URLSearchParams(location.search).get("pdf");
  internals.convertFileSrc = (path) => {
    if (realPdf?.startsWith("/local/")) return realPdf;
    const id = Number(path);
    if (!samplePdfUrls.has(id)) {
      // 合成 PDF 只用标准字体（不含中文），标题用英文
      samplePdfUrls.set(id, URL.createObjectURL(new Blob([samplePdf(`Synthetic sample PDF #${id}`)], { type: "application/pdf" })));
    }
    return samplePdfUrls.get(id)!;
  };
}
