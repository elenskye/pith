// 仅开发用：概念研究各命令在浏览器预览（?mock）里的模拟实现，由 mockIpc.ts 并进命令表。
// 示例数据 mockConcept.json 由 Rust 引擎在夹具上生成（公有领域的英文本 + 合成中文，七个技能各跑一次），不是手写的：
//   cargo run -q -p concept-engine --example mock_data > src/dev/mockConcept.json
// 这里只是一个内存里的小模型，大致模仿 Rust（src-tauri/src/concept/commands.rs），不是规则的来源。
import { emit } from "@tauri-apps/api/event";
import data from "./mockConcept.json";
import type {
  ConceptAnchoredParagraph,
  ConceptCard,
  ConceptPara,
  ConceptPreview,
  ConceptQuote,
  ConceptRun,
  ConceptRunPreview,
  ConceptRunRequest,
  ConceptRunSummary,
  ConceptSearchHit,
  ConceptSettings,
  ConceptSkill,
  ConceptStatus,
  ConceptTextSummary,
  ConceptTextView,
  ConceptUnitInfo,
  ConceptUnitView,
} from "@/shared/ipc";

type Args = Record<string, unknown>;
const fail = (msg: string): never => {
  throw msg;
};
const now = () => new Date().toISOString();
const clone = <T>(v: T): T => structuredClone(v);

type Para = { seq: number; unit: number; role: ConceptPara["role"]; number: number | null; key: string; attached: number | null; text: string; page: string | null; breaks: { at: number; page: string }[]; marker: string | null; noterefs: [number, string][] };
type Unit = { index: number; kind: ConceptUnitInfo["kind"]; label: string; key: string; depth: number; parent: number | null; book: number | null; chapter: number | null };
type MText = { title: string; language: string; format: string; numbering: string; pageSource: string; units: Unit[]; paragraphs: Para[]; warnings: string[] };
type Alignment = { units: { key: string; label: string; otherLabel: string; unit: number; otherUnit: number | null; count: number; otherCount: number; aligned: boolean }[]; pairs: [number, number][] };

const EN = data.en as unknown as MText;
const ZH = data.zh as unknown as MText;
const AL = data.alignment as unknown as Alignment;
const RUNS = data.runs as unknown as Record<string, ConceptRun>;
const SVGS = data.svgs as Record<string, string>;
const SKILLS = data.skills as unknown as ConceptSkill[];
const PREVIEW_EN = data.previewEn as unknown as ConceptPreview;
const PREVIEW_ZH = data.previewZh as unknown as ConceptPreview;

/** 模拟文献库里那两本书（mockIpc.ts 的条目 6、7，附件 104、105） */
export const BOOK_ITEMS = { en: { item: 6, uid: "U6", attachment: 104 }, zh: { item: 7, uid: "U7", attachment: 105 } };

type Stored = { summary: ConceptTextSummary; text: MText };
const texts: Stored[] = [
  { summary: summary("MOCK-EN", EN, BOOK_ITEMS.en.item, "The Wealth of Nations (fixture)", BOOK_ITEMS.en.attachment, null, "MOCK-ZH"), text: EN },
  { summary: summary("MOCK-ZH", ZH, BOOK_ITEMS.zh.item, "国富论（合成）", BOOK_ITEMS.zh.attachment, "MOCK-EN", null), text: ZH },
];
function summary(uid: string, t: MText, itemId: number | null, itemTitle: string, attachmentId: number | null, originalUid: string | null, translationUid: string | null): ConceptTextSummary {
  return { uid, title: t.title, language: t.language, format: t.format, itemId, itemTitle, attachmentId, numbering: t.numbering, pageSource: t.pageSource, hasPages: t.pageSource !== "none", originalUid, translationUid, createdAt: "2026-11-13T09:00:00", converter: null, warnings: t.warnings };
}

let settings: ConceptSettings = { model: "gpt-6.1-sol", effort: "medium", dedicatedSearch: true, pdfPageLimit: 60, openaiBase: null, mathpixBase: null, prices: { models: {}, tavilyCreditUsd: null, claudeSearchUsd: null } };
const keys = { openai: false, mathpix: false };
const runs: { textUid: string; run: ConceptRun; template: string }[] = [{ textUid: "MOCK-EN", run: clone(RUNS["chapter:I.i"]), template: "chapter:I.i" }];
let running: string | null = null;
let cancelled = false;
let nextTicket = 1;
const tickets = new Map<number, { kind: "text"; preview: ConceptPreview; attachmentId: number; pdf: boolean } | { kind: "pdf"; attachmentId: number } | { kind: "run"; request: ConceptRunRequest | null; resume: string | null; previous: string | null }>();

function status(): ConceptStatus {
  return {
    mock: true,
    settings,
    keys: [
      { which: "openai", label: "OpenAI", has: keys.openai, place: "memory", needed: settings.model === "gpt-6.1-sol" },
      { which: "mathpix", label: "Mathpix", has: keys.mathpix, place: "memory", needed: true },
    ],
    provider: settings.model.startsWith("claude") ? "anthropic" : "openai",
    searchEngine: settings.dedicatedSearch ? (settings.model.startsWith("claude") ? "claude_web_search" : "openai_web_search") : "tavily",
    skills: SKILLS,
    brokenSkills: [],
    skillsDir: "~/Pith Dev/skills",
    priceInput: settings.model.startsWith("claude") ? 4 : 2,
    priceCached: settings.model.startsWith("claude") ? 4 : 0.1,
    priceOutput: settings.model.startsWith("claude") ? 20 : 10,
    longContextTokens: 272000,
    webSearchUsd: 0.01,
    mathpixPageUsd: 0.005,
    budgetUsd: 30,
    monthSpentUsd: 0,
    calls: [],
    running,
  };
}

const textOf = (uid: string) => texts.find((t) => t.summary.uid === uid) ?? fail("找不到这份文本（可能已经删掉了）");
const partnerOf = (uid: string) => {
  const s = textOf(uid).summary;
  return s.translationUid ? texts.find((t) => t.summary.uid === s.translationUid) ?? null : null;
};
const isEn = (uid: string) => textOf(uid).text === EN;
/** 对齐只有 EN ↔ ZH 这一对 */
const alignmentOf = (uid: string) => (isEn(uid) && partnerOf(uid)?.text === ZH ? AL : null);

function unitInfos(t: MText, al: Alignment | null): ConceptUnitInfo[] {
  const pv = t === EN ? PREVIEW_EN : t === ZH ? PREVIEW_ZH : pdfPreview(t);
  return pv.units.map((u) => {
    const a = al?.units.find((x) => x.unit === u.index);
    return { index: u.index, key: u.key, label: u.label, kind: u.kind, depth: u.depth, body: u.counts.body, sidenotes: u.counts.sidenote, notes: u.counts.note, guide: u.counts.guide, pages: u.pages, aligned: a ? a.aligned : null, otherCount: a ? a.otherCount : null };
  });
}

const view = (p: Para, partner: number | null): ConceptPara => ({ seq: p.seq, key: p.key, role: p.role, number: p.number, text: p.text, page: p.page, breaks: p.breaks, attached: p.attached, marker: p.marker, partner });
const pagesOf = (p: Para) => (p.page ? (p.breaks.length ? `${p.page}–${p.breaks[p.breaks.length - 1].page}` : p.page) : null);
const hasCjk = (s: string) => /[㐀-鿿]/.test(s);

function pdfPreview(t: MText): ConceptPreview {
  const counts = { body: t.paragraphs.filter((p) => p.role === "body").length, heading: t.paragraphs.filter((p) => p.role === "heading").length, sidenote: 0, note: t.paragraphs.filter((p) => p.role === "note").length, guide: 0, matter: 0 };
  return { title: t.title, language: t.language, format: "pdf", numbering: "plain", pageSource: "pdf", hasPages: true, totals: counts, paragraphs: t.paragraphs.length, chars: t.paragraphs.reduce((n, p) => n + p.text.length, 0), units: [{ index: 0, key: "", label: "全文", kind: "part", depth: 1, counts, pages: ["1", "2"], first: t.paragraphs.find((p) => p.role === "body")?.text.slice(0, 24) ?? "" }], warnings: t.warnings };
}

const PDF_TEXT: MText = {
  title: "合成短文：分工与市场",
  language: "zh",
  format: "pdf",
  numbering: "plain",
  pageSource: "pdf",
  units: [{ index: 0, kind: "part", label: "全文", key: "", depth: 1, parent: null, book: null, chapter: null }],
  paragraphs: [
    { seq: 0, unit: 0, role: "heading", number: null, key: " 标题", attached: null, text: "合成短文：分工与市场", page: "1", breaks: [], marker: null, noterefs: [] },
    { seq: 1, unit: 0, role: "body", number: 1, key: "1", attached: null, text: "分工越细，每个人能做的活就越专，但前提是市场足够大，否则多做出来的东西卖不出去。", page: "1", breaks: [{ at: 26, page: "2" }], marker: null, noterefs: [] },
    { seq: 2, unit: 0, role: "body", number: 2, key: "2", attached: null, text: "第二段：水运让市场变大。", page: "2", breaks: [], marker: null, noterefs: [] },
  ],
  warnings: ["模拟模式：没有真的调用 Mathpix，用本机取的文字拼成行。"],
};

function textPreview(ticket: number, preview: ConceptPreview, attachmentId: number) {
  const lang = preview.language;
  const existing = texts.find((t) => t.summary.attachmentId === attachmentId)?.summary.uid ?? null;
  return { ticket, preview, candidates: texts.filter((t) => t.summary.language !== lang && !t.summary.originalUid).map((t) => t.summary), existing, converter: preview.format === "pdf" ? "mathpix（模拟）" : null, mock: true };
}

function bodyOf(uid: string) {
  return textOf(uid).text.paragraphs;
}

function anchored(uid: string, run: ConceptRun): ConceptAnchoredParagraph[] {
  const en = textOf(uid).text;
  const zh = partnerOf(uid)?.text;
  const keysSeen: string[] = [];
  for (const s of [...run.cards.flatMap((c) => c.sentences), ...run.relations.map((r) => r.note), ...run.targets.flatMap((c) => c.sentences)]) {
    if (s.paragraph && !keysSeen.includes(s.paragraph)) keysSeen.push(s.paragraph);
    if (s.check.zhParagraph && !keysSeen.includes(`译:${s.check.zhParagraph}`)) keysSeen.push(`译:${s.check.zhParagraph}`);
  }
  const out: ConceptAnchoredParagraph[] = [];
  for (const k of keysSeen) {
    const tr = k.startsWith("译:");
    const t = tr ? zh : en;
    const p = t?.paragraphs.find((x) => x.key === (tr ? k.slice(2) : k));
    if (p) out.push({ key: k, text: p.text, role: p.role, pages: tr ? null : pagesOf(p), unit: p.unit, seq: p.seq, translation: tr });
  }
  return out;
}

function runSummary(textUid: string, r: ConceptRun): ConceptRunSummary {
  const s = r.scope;
  const label = s?.question ?? (s?.terms.length ? s.terms.join(" / ") : null) ?? s?.instruction ?? (s && s.paragraphs.length > 1 ? `${s.paragraphs[0]} 等 ${s.paragraphs.length} 段` : (s?.paragraphs[0] ?? ""));
  return { uid: r.uid, textUid, skill: r.skill, skillTitle: r.skillTitle, status: r.status, mock: r.mock, unit: s?.unit ?? null, firstParagraph: s?.paragraphs[0] ?? null, label, createdAt: r.createdAt, cards: r.cards.map((c) => ({ key: c.id, kind: c.kind, title: c.title })) };
}

const findRun = (uid: string) => runs.find((r) => r.run.uid === uid) ?? fail("找不到这次研究（可能已经删掉了）");

function template(req: ConceptRunRequest): string {
  if (req.skill === "chapter") return RUNS[`chapter:${req.unit}`] ? `chapter:${req.unit}` : "chapter:I.i";
  return RUNS[req.skill] ? req.skill : "free";
}

function runPreview(ticket: number, req: ConceptRunRequest, resume: string | null): ConceptRunPreview {
  const skill = SKILLS.find((s) => s.name === req.skill) ?? fail(`没有「${req.skill}」这个技能`);
  if (!skill.entries.includes(req.entry)) fail(`「${skill.title}」不能从这个入口开始。`);
  if (skill.needsInstruction && !req.instruction?.trim()) fail(`「${skill.title}」要写一句要求。`);
  const paras = bodyOf(req.textUid).filter((p) => (req.paragraphs ?? []).includes(p.key));
  const active = skill.steps.filter((s) => !(req.skip ?? []).includes(s.id) && (!s.offByDefault || (req.with ?? []).includes(s.id)));
  if (!active.some((s) => s.kind === "model")) fail("至少要留一个模型步骤。");
  const chars = paras.reduce((n, p) => n + p.text.length, 0);
  const targets = (req.cards ?? []).map(([uid, key]) => {
    const c = runs.find((r) => r.run.uid === uid)?.run.cards.find((x) => x.id === key);
    return c ? `${c.id} ${c.title}` : key;
  });
  return {
    ticket,
    skill: skill.name,
    skillTitle: skill.title,
    resume,
    paragraphs: paras.length,
    chars,
    languages: partnerOf(req.textUid) ? ["英文", "对齐的中译"] : ["英文"],
    first: paras[0]?.key ?? null,
    last: paras[paras.length - 1]?.key ?? null,
    targets,
    steps: active.map((s) => ({
      id: s.id,
      title: s.title,
      kind: s.kind,
      sends: s.kind === "library" ? `在文献库里搜检索词，最多 ${s.maxResults} 条（不联网、不花钱）` : s.kind === "web" ? `网页检索 1 次，下载最多 ${s.maxResults} 个网页（本机下载不花钱）` : s.input.map((i) => ({ scope: `${paras.length} 段原文（${chars} 字）`, translation: "对齐的中译和译本的旁注、注释", cards: "前面做出的卡片", sources: "找到的来源", question: "问题", instruction: "你写的要求", terms: "检索词" })[i] ?? i).join(" + "),
      maxOutputTokens: s.maxOutputTokens,
      inputTokens: Math.round(chars / 2 + 1800),
      typicalUsd: 0,
      maxUsd: 0,
    })),
    provider: status().provider,
    model: "模拟模型",
    effort: settings.effort,
    searchEngine: status().searchEngine,
    typicalUsd: 0,
    maxUsd: 0,
    budgetUsd: 30,
    monthSpentUsd: 0,
    withinBudget: true,
    missingKeys: [],
    mock: true,
  };
}

function requestOf(run: ConceptRun, textUid: string): ConceptRunRequest {
  const s = run.scope!;
  return { textUid, skill: run.skill, entry: s.entry, paragraphs: s.paragraphs, unit: s.unit, question: s.question, terms: s.terms, cards: s.cards, instruction: s.instruction, skip: s.skip, with: s.with };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function execute(uid: string) {
  const entry = findRun(uid);
  const skill = SKILLS.find((s) => s.name === entry.run.skill)!;
  const scope = entry.run.scope!;
  const steps = skill.steps.filter((s) => !scope.skip.includes(s.id) && (!s.offByDefault || scope.with.includes(s.id)));
  running = uid;
  cancelled = false;
  entry.run.status = "running";
  for (let i = entry.run.nextStep; i < steps.length; i++) {
    await emit("concept-progress", { uid, step: steps[i].id, done: i, total: steps.length, message: `${steps[i].title || steps[i].id}…` });
    await wait(900);
    if (cancelled) {
      entry.run.status = "cancelled";
      entry.run.nextStep = i;
      running = null;
      await emit("concept-done", { uid, status: "cancelled", error: null });
      return;
    }
  }
  const t = clone(RUNS[entry.template]);
  const skipped = new Set(scope.skip);
  entry.run = {
    ...t,
    uid,
    scope,
    previous: entry.run.previous,
    createdAt: entry.run.createdAt,
    updatedAt: now(),
    finishedAt: now(),
    status: "done",
    cards: t.cards.filter((c) => !skipped.has(c.step)),
    relations: t.relations.filter((r) => !skipped.has(r.step)),
    steps: t.steps.filter((s) => !skipped.has(s.id)),
    sources: scope.with.includes("web") ? t.sources : t.sources.filter((s) => s.origin !== "web"),
  };
  running = null;
  await emit("concept-done", { uid, status: "done", error: null });
}

function quotes(itemUid: string): ConceptQuote[] {
  const out: ConceptQuote[] = [];
  const en = itemUid === BOOK_ITEMS.en.uid;
  if (!en && itemUid !== BOOK_ITEMS.zh.uid) return out;
  for (const { run } of runs) {
    for (const c of run.cards) {
      for (const s of c.sentences) {
        if (s.category !== "original" || s.check.status !== "verified" || !s.paragraph) continue;
        if (en && !out.some((q) => q.text === (s.quote ?? s.text))) out.push({ runUid: run.uid, card: c.id, cardTitle: c.title, text: s.quote ?? s.text, language: "en", paragraph: s.paragraph, pages: s.check.pages, zh: s.check.zhStatus === "verified" ? s.zh : null, zhParagraph: s.check.zhParagraph });
        if (!en && s.check.zhStatus === "verified" && s.zh && !out.some((q) => q.text === s.zh)) out.push({ runUid: run.uid, card: c.id, cardTitle: c.title, text: s.zh, language: "zh", paragraph: s.check.zhParagraph!, pages: null, zh: null, zhParagraph: null });
      }
    }
  }
  return out;
}

export const conceptHandlers: Record<string, (a: Args) => unknown> = {
  concept_status: () => status(),
  concept_settings_set: ({ settings: s }) => {
    settings = s as ConceptSettings;
    return status();
  },
  concept_key_set: ({ which, key, appKey }) => {
    const w = which as "openai" | "mathpix";
    if (w === "openai" && !String(key).trim().startsWith("sk-")) fail("这看起来不是 OpenAI 的 API 密钥（应以 sk- 开头）。");
    if (w === "mathpix" && (!String(key).trim() || !String(appKey ?? "").trim())) fail("Mathpix 要填 app_id 和 app_key 两项。");
    keys[w] = true;
    return status();
  },
  concept_key_clear: ({ which }) => {
    keys[which as "openai" | "mathpix"] = false;
    return status();
  },
  concept_skills_reveal: () => void console.info("（预览）在访达中打开技能文件夹"),
  concept_text_prepare: ({ attachmentId }) => {
    const id = attachmentId as number;
    const ticket = nextTicket++;
    if (id === BOOK_ITEMS.en.attachment || id === BOOK_ITEMS.zh.attachment) {
      const preview = id === BOOK_ITEMS.en.attachment ? PREVIEW_EN : PREVIEW_ZH;
      tickets.set(ticket, { kind: "text", preview, attachmentId: id, pdf: false });
      return { kind: "epub", ...textPreview(ticket, preview, id) };
    }
    tickets.set(ticket, { kind: "pdf", attachmentId: id });
    return { kind: "pdf", ticket, pages: 2, limit: settings.pdfPageLimit, overLimit: 2 > settings.pdfPageLimit, hasMathpix: true, costUsd: 0.01, budgetUsd: 30, monthSpentUsd: 0, mock: true };
  },
  concept_pdf_convert: ({ ticket }) => {
    const t = tickets.get(ticket as number);
    if (!t || t.kind !== "pdf") fail("这次确认已经过期，请重新确认。");
    const n = nextTicket++;
    const preview = pdfPreview(PDF_TEXT);
    tickets.set(n, { kind: "text", preview, attachmentId: (t as { attachmentId: number }).attachmentId, pdf: true });
    return textPreview(n, preview, (t as { attachmentId: number }).attachmentId);
  },
  concept_text_renumber: ({ ticket, numbering }) => {
    const t = tickets.get(ticket as number);
    if (!t || t.kind !== "text") fail("这次确认已经过期，请重新确认。");
    const x = t as { preview: ConceptPreview; attachmentId: number };
    const preview = { ...x.preview, numbering: numbering as ConceptPreview["numbering"] };
    const n = nextTicket++;
    tickets.set(n, { kind: "text", preview, attachmentId: x.attachmentId, pdf: false });
    return textPreview(n, preview, x.attachmentId);
  },
  concept_text_save: ({ ticket, originalUid }) => {
    const t = tickets.get(ticket as number);
    if (!t || t.kind !== "text") fail("这次确认已经过期，请重新确认。");
    const x = t as { preview: ConceptPreview; attachmentId: number; pdf: boolean };
    const text = x.pdf ? PDF_TEXT : x.attachmentId === BOOK_ITEMS.zh.attachment ? ZH : EN;
    const uid = `MOCK-T${nextTicket++}`;
    const s = summary(uid, text, null, x.preview.title, x.attachmentId, (originalUid as string | null) ?? null, null);
    if (x.pdf) s.converter = "mathpix（模拟）";
    texts.push({ summary: s, text });
    if (s.originalUid) textOf(s.originalUid).summary.translationUid = uid;
    return s;
  },
  concept_text_link: ({ uid, originalUid }) => {
    const t = textOf(uid as string);
    t.summary.originalUid = (originalUid as string | null) ?? null;
    if (t.summary.originalUid) textOf(t.summary.originalUid).summary.translationUid = t.summary.uid;
    return t.summary;
  },
  concept_text_delete: ({ uid }) => {
    const i = texts.findIndex((t) => t.summary.uid === uid);
    if (i < 0) fail("找不到这份文本");
    texts.splice(i, 1);
    for (const t of texts) {
      if (t.summary.translationUid === uid) t.summary.translationUid = null;
      if (t.summary.originalUid === uid) t.summary.originalUid = null;
    }
  },
  concept_texts_list: () => texts.map((t) => t.summary),
  concept_attachment_text: ({ attachmentId }) => texts.find((t) => t.summary.attachmentId === attachmentId)?.summary.uid ?? null,
  concept_text_get: ({ uid }): ConceptTextView => {
    const t = textOf(uid as string);
    const al = alignmentOf(uid as string);
    const tr = partnerOf(uid as string);
    return { summary: t.summary, units: unitInfos(t.text, al), translation: tr?.summary ?? null, original: t.summary.originalUid ? textOf(t.summary.originalUid).summary : null, alignedUnits: al?.units.filter((u) => u.aligned).length ?? 0, mismatchedUnits: al?.units.filter((u) => !u.aligned).map((u) => u.key) ?? [] };
  },
  concept_unit_get: ({ uid, unit }): ConceptUnitView => {
    const t = textOf(uid as string);
    const al = alignmentOf(uid as string);
    const info = unitInfos(t.text, al).find((u) => u.index === unit) ?? fail("找不到这一章");
    const pairOf = (seq: number) => al?.pairs.find((p) => p[0] === seq)?.[1] ?? null;
    const paragraphs = t.text.paragraphs.filter((p) => p.unit === unit).map((p) => view(p, pairOf(p.seq)));
    const tr = partnerOf(uid as string);
    const ua = al?.units.find((u) => u.unit === unit);
    let translation: ConceptPara[] = [];
    if (tr && ua?.otherUnit !== null && ua?.otherUnit !== undefined) {
      const zu = ua.otherUnit;
      const guides: number[] = [];
      for (let g = zu; g > 0 && tr.text.units[g - 1].kind === "guide"; g--) guides.unshift(g - 1);
      translation = [...guides.flatMap((g) => tr.text.paragraphs.filter((p) => p.unit === g).map((p) => view(p, null))), ...tr.text.paragraphs.filter((p) => p.unit === zu).map((p) => view(p, al!.pairs.find((x) => x[1] === p.seq)?.[0] ?? null))];
    }
    return { textUid: uid as string, unit: info, paragraphs, translationUid: tr?.summary.uid ?? null, translation, translationLabel: ua?.otherLabel ?? null, aligned: ua?.aligned ?? false, language: t.text.language };
  },
  concept_search: ({ uid, query }): ConceptSearchHit[] => {
    const t = textOf(uid as string).text;
    const al = alignmentOf(uid as string);
    const zh = partnerOf(uid as string)?.text;
    const terms = String(query).split(/[；;,，\n]/).map((x) => x.trim()).filter(Boolean);
    const hits = new Map<number, boolean>();
    for (const term of terms) {
      const low = term.toLowerCase();
      for (const p of t.paragraphs) if (["body", "sidenote", "note", "guide"].includes(p.role) && p.text.toLowerCase().includes(low) && !hits.has(p.seq)) hits.set(p.seq, false);
      if (zh && al && hasCjk(term)) {
        for (const z of zh.paragraphs) {
          if (!["body", "sidenote", "note"].includes(z.role) || !z.text.includes(term)) continue;
          const body = z.role === "body" ? z.seq : z.attached;
          const o = al.pairs.find((x) => x[1] === body)?.[0];
          if (o !== undefined && !hits.has(o)) hits.set(o, true);
        }
      }
    }
    return [...hits.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([seq, via]) => {
        const p = t.paragraphs[seq];
        const u = t.units[p.unit];
        const z = al?.pairs.find((x) => x[0] === seq)?.[1];
        const zp = z !== undefined ? zh?.paragraphs[z] : undefined;
        return { seq, key: p.key, role: p.role, unit: p.unit, unitKey: u.key, unitLabel: u.label, text: p.text, viaTranslation: via, translationKey: zp?.key ?? null, translationText: zp?.text ?? null };
      });
  },
  concept_run_prepare: ({ request, resume }) => {
    const ticket = nextTicket++;
    if (resume) {
      const e = findRun(resume as string);
      if (e.run.status === "done") fail("这次研究已经做完了。");
      tickets.set(ticket, { kind: "run", request: null, resume: resume as string, previous: null });
      return runPreview(ticket, requestOf(e.run, e.textUid), resume as string);
    }
    const req = request as ConceptRunRequest;
    tickets.set(ticket, { kind: "run", request: req, resume: null, previous: null });
    return runPreview(ticket, req, null);
  },
  concept_rerun_prepare: ({ uid }) => {
    const e = findRun(uid as string);
    const ticket = nextTicket++;
    const req = requestOf(e.run, e.textUid);
    tickets.set(ticket, { kind: "run", request: req, resume: null, previous: uid as string });
    return runPreview(ticket, req, null);
  },
  concept_run_execute: ({ ticket }) => {
    const t = tickets.get(ticket as number);
    if (!t || t.kind !== "run") fail("这次确认已经过期，请重新确认。");
    if (running) fail("上一次概念研究还在进行。等它做完，或者先停止。");
    const x = t as { request: ConceptRunRequest | null; resume: string | null; previous: string | null };
    tickets.delete(ticket as number);
    if (x.resume) {
      void execute(x.resume);
      return x.resume;
    }
    const req = x.request!;
    const skill = SKILLS.find((s) => s.name === req.skill)!;
    const uid = `MOCK-R${nextTicket++}`;
    const targets: ConceptCard[] = (req.cards ?? []).flatMap(([ru, key]) => runs.find((r) => r.run.uid === ru)?.run.cards.filter((c) => c.id === key) ?? []);
    const run: ConceptRun = {
      uid,
      skill: skill.name,
      skillTitle: skill.title,
      skillVersion: skill.version,
      scope: { entry: req.entry, text: req.textUid, translation: partnerOf(req.textUid)?.summary.uid ?? null, paragraphs: req.paragraphs ?? [], unit: req.unit ?? null, question: req.question ?? null, terms: req.terms ?? [], cards: req.cards ?? [], instruction: req.instruction ?? null, skip: req.skip ?? [], with: req.with ?? [] },
      status: "running",
      nextStep: 0,
      error: null,
      model: { provider: "mock", model: "模拟模型" },
      effort: settings.effort,
      mock: true,
      createdAt: now(),
      updatedAt: now(),
      finishedAt: null,
      steps: [],
      cards: [],
      relations: [],
      sources: [],
      rejected: [],
      targets,
      previous: x.previous,
    };
    runs.push({ textUid: req.textUid, run, template: template(req) });
    void execute(uid);
    return uid;
  },
  concept_cancel: () => {
    cancelled = true;
  },
  concept_runs_list: ({ textUid }) => runs.filter((r) => r.textUid === textUid).map((r) => runSummary(r.textUid, r.run)),
  concept_run_get: ({ uid }) => {
    const e = findRun(uid as string);
    const done = e.run.status === "done";
    return { textUid: e.textUid, translationUid: partnerOf(e.textUid)?.summary.uid ?? null, run: clone(e.run), findings: done ? ((data.findings as Record<string, unknown[]>)[e.template] ?? []) : [], svg: done && e.run.relations.length ? SVGS[e.template] : "", running: running === uid, costUsd: 0, paragraphs: anchored(e.textUid, e.run) };
  },
  concept_run_delete: ({ uid }) => {
    if (running === uid) fail("这次研究正在进行，先停止。");
    const i = runs.findIndex((r) => r.run.uid === uid);
    if (i >= 0) runs.splice(i, 1);
  },
  concept_export: ({ uid, format, dir }) => {
    const e = findRun(uid as string);
    const ext = format === "markdown" ? "md" : format === "html" ? "html" : "docx";
    return `${dir}/${e.run.skillTitle}.${ext}`;
  },
  concept_add_to_library: ({ uid, keys: k }) => {
    const e = findRun(uid as string);
    const added = (k as string[]).map((key, i) => {
      const s = e.run.sources.find((x) => x.id === key) ?? fail(`这次研究里没有来源 ${key}`);
      s.addedItem = 9100 + i;
      return { key, itemId: s.addedItem, created: true, attachmentError: null };
    });
    return { added, view: conceptHandlers.concept_run_get({ uid }) };
  },
  concept_quotes: ({ itemUid }) => quotes(itemUid as string),
};
