// 仅开发用：事实调查各命令在浏览器预览（?mock）里的模拟实现，由 mockIpc.ts 并进命令表。
// 示例数据 mockResearch.json 由 Rust 引擎在合成夹具上生成（虚构的「示例市」），不是手写的：
//   cargo run -q -p research-engine --example mock_data > src/dev/mockResearch.json
// 这里只是一个内存里的小模型，大致模仿 Rust（src-tauri/src/research/commands.rs），不是规则的来源。
// 预览别的状态：?mock&research=real  假装用真接口（要先在设置里填密钥，调用有费用）
import { emit } from "@tauri-apps/api/event";
import data from "./mockResearch.json";
import type {
  ResearchEstimate,
  ResearchExecutePreview,
  ResearchPlan,
  ResearchPlanPreview,
  ResearchProgress,
  ResearchQuestion,
  ResearchReport,
  ResearchRun,
  ResearchRunView,
  ResearchSettings,
  ResearchStage,
  ResearchStatus,
  ResearchTier,
} from "@/shared/ipc";

type Args = Record<string, unknown>;
const fail = (msg: string): never => {
  throw msg;
};
const real = new URLSearchParams(location.search).get("research") === "real";
const now = () => new Date().toISOString();
const clone = <T>(v: T): T => structuredClone(v);
type Snapshot = { run: ResearchRun; report: ResearchReport };
const PLANNED = data.planned as unknown as Snapshot;
const DONE = data.done as unknown as Snapshot;
const RERUN = data.rerun as unknown as Snapshot;

// ---------- 设置、密钥 ----------

let settings: ResearchSettings = {
  planner: { provider: "anthropic", model: "claude-opus-5-5", baseUrl: null },
  worker: { provider: "openai_compatible", model: "deepseek-flash", baseUrl: "https://api.deepseek.com" },
  dedicatedSearch: { anthropic: "claude_web_search", openai_compatible: "none" },
  tavilyDepth: "basic",
  planWebSearches: 3,
  trust: { tiers: {} },
  prices: { models: {}, tavilyCreditUsd: null, claudeSearchUsd: null },
  webFetchFallback: true,
  fetchModel: "claude-haiku-5-5",
};
const keys = { anthropic: false, deepseek: false, tavily: false };
const DEFAULT_TIERS: Record<string, ResearchTier> = {
  intl_org: "high",
  foreign_gov_stats: "high",
  academic: "high",
  independent_survey: "medium",
  platform_data: "medium",
  industry_report: "medium",
  commercial_media: "medium",
  cn_gov_stats: "low",
  cn_state_media: "low",
  self_media: "low",
  unknown: "low",
};
const KIND_LABEL: Record<string, [string, string]> = {
  intl_org: ["国际组织", "国际劳工组织、经合组织、世界银行、联合国机构等"],
  foreign_gov_stats: ["其他国家和地区的政府统计", "中国大陆以外的政府统计机构"],
  academic: ["学术研究", "同行评议的论文，高校、研究机构自己做的调查和估计"],
  independent_survey: ["独立调查机构、智库", "不隶属政府的调查机构、智库"],
  platform_data: ["招聘平台、企业的业务数据", "招聘平台、互联网平台、企业用自己的业务数据做的统计"],
  industry_report: ["企业、行业协会的报告", "企业、咨询公司、行业协会发布的报告"],
  commercial_media: ["市场化媒体自己的调查", "市场化媒体自己采访、统计出来的数字"],
  cn_gov_stats: ["中国政府官方统计", "国家统计局、地方统计局、人社部门等中国政府机构"],
  cn_state_media: ["中国官方媒体", "中国官方媒体自己给出、没有说明出处的数字"],
  self_media: ["自媒体、个人、论坛", "自媒体、个人博客、论坛帖子"],
  unknown: ["看不出是谁统计的", "正文里看不出这个数是谁统计的"],
};
const PRICES: Record<string, { input: number; output: number }> = { "claude-opus-5-5": { input: 4, output: 20 }, "deepseek-flash": { input: 0.3, output: 1.2 }, "claude-haiku-5-5": { input: 0.5, output: 2.5 } };
const engine = () => ((settings.dedicatedSearch[settings.worker.provider] ?? "none") === "none" ? "tavily" : "claude_web_search");
const needed = () => {
  const out = new Set<string>();
  for (const e of [settings.planner, settings.worker]) out.add(e.provider === "anthropic" ? "anthropic" : "deepseek");
  if (engine() === "tavily") out.add("tavily");
  return out;
};
const missing = () => (real ? [...needed()].filter((k) => !keys[k as keyof typeof keys]).map((k) => ({ anthropic: "Anthropic", deepseek: "DeepSeek", tavily: "Tavily" })[k]!) : []);
let spent = 0;
const calls: ResearchStatus["calls"] = [];
let running: string | null = null;

function status(): ResearchStatus {
  const tiers = { ...DEFAULT_TIERS, ...settings.trust.tiers } as Record<string, ResearchTier>;
  const price = (role: "planner" | "worker") => {
    const m = settings[role].model;
    const p = settings.prices.models[m] ?? PRICES[m] ?? { input: 10, output: 50 };
    return { role, model: m, input: p.input, output: p.output, known: m in PRICES || m in settings.prices.models };
  };
  return {
    mock: !real,
    settings: clone(settings),
    keys: (["anthropic", "deepseek", "tavily"] as const).map((which) => ({ which, label: { anthropic: "Anthropic", deepseek: "DeepSeek", tavily: "Tavily" }[which], has: keys[which], place: "memory" as const, needed: needed().has(which) })),
    searchEngine: engine(),
    planWebSearches: settings.planner.provider === "anthropic" && settings.dedicatedSearch.anthropic === "claude_web_search" ? settings.planWebSearches : 0,
    prices: [price("planner"), price("worker")],
    tavilyCreditUsd: settings.prices.tavilyCreditUsd ?? 0.008,
    claudeSearchUsd: settings.prices.claudeSearchUsd ?? 0.01,
    trust: Object.keys(DEFAULT_TIERS).map((k) => ({ kind: k as never, label: KIND_LABEL[k][0], hint: KIND_LABEL[k][1], tier: tiers[k], defaultTier: DEFAULT_TIERS[k] })),
    trustName: Object.entries(settings.trust.tiers).some(([k, t]) => DEFAULT_TIERS[k] !== t) ? "自定义等级表" : "默认等级表",
    budgetUsd: 30,
    month: now().slice(0, 7),
    monthSpentUsd: spent,
    calls: clone(calls).reverse(),
    ledgerError: null,
    running,
  };
}

// ---------- 调查 ----------

type MView = { questionId: number; run: ResearchRun; report: ResearchReport; planEditable: boolean };
const questions: { id: number; uid: string; text: string; createdAt: string; updatedAt: string }[] = [];
const views = new Map<string, MView>();
let nextQuestion = 1;
let seq = 0;
const uid = () => `01MOCKRUN${String(Date.now()).slice(-8)}${String(++seq).padStart(4, "0")}`.padEnd(26, "0");

function snapshotAs(s: Snapshot, o: { uid: string; question: string; seq: number; previous: string | null; at: string }): Snapshot {
  const run = clone(s.run);
  const report = clone(s.report);
  Object.assign(run, { uid: o.uid, question: o.question, seq: o.seq, previous: o.previous, createdAt: o.at, updatedAt: o.at, mock: !real });
  if (run.finishedAt) run.finishedAt = o.at;
  Object.assign(report, { uid: o.uid, question: o.question, seq: o.seq, createdAt: o.at, finishedAt: run.finishedAt, mock: !real });
  if (real) {
    // 假装用了真接口：每步记一点钱
    for (const step of [...run.steps, ...report.steps]) {
      if (step.provider) {
        step.mock = false;
        step.provider = step.kind === "search" ? "tavily" : step.kind === "extract" ? "openai_compatible" : "anthropic";
        step.model = step.kind === "search" ? "" : step.kind === "extract" ? "deepseek-flash" : "claude-opus-5-5";
        step.costUsd = step.kind === "search" ? 0.008 : step.kind === "extract" ? 0.0016 : 0.12;
      }
    }
    report.totals.costUsd = report.steps.reduce((n, s) => n + s.costUsd, 0);
    report.notes = report.notes.filter((n) => !n.startsWith("这是模拟接口"));
  }
  return { run, report };
}

function addQuestion(text: string, at: string) {
  const q = { id: nextQuestion++, uid: uid(), text, createdAt: at, updatedAt: at };
  questions.push(q);
  return q;
}

// 一开始就有一个做完的问题：第 1 次 + 重查（带新旧对比），点开就能看全部界面
{
  const q = addQuestion(DONE.run.question, "2026-11-12T01:00:00.000Z");
  const first = snapshotAs(DONE, { uid: uid(), question: q.text, seq: 1, previous: null, at: "2026-11-12T01:00:00.000Z" });
  views.set(first.run.uid, { questionId: q.id, ...first, planEditable: false });
  const second = snapshotAs(RERUN, { uid: uid(), question: q.text, seq: 2, previous: first.run.uid, at: "2026-12-01T01:00:00.000Z" });
  views.set(second.run.uid, { questionId: q.id, ...second, planEditable: false });
  q.updatedAt = "2026-12-01T01:00:00.000Z";
}

const findView = (u: unknown) => views.get(String(u)) ?? fail("这次调查不存在了。");
const toView = (v: MView): ResearchRunView => clone({ questionId: v.questionId, run: v.run, report: v.report, findings: [], running: running === v.run.uid, planEditable: v.planEditable && running !== v.run.uid });

function summaries(id: number): ResearchQuestion["runs"] {
  return [...views.values()]
    .filter((v) => v.questionId === id)
    .sort((a, b) => b.run.seq - a.run.seq)
    .map((v) => ({
      uid: v.run.uid,
      seq: v.run.seq,
      stage: v.run.stage,
      status: running === v.run.uid ? "running" : v.run.status,
      mock: v.run.mock,
      createdAt: v.run.createdAt,
      finishedAt: v.run.finishedAt,
      sources: v.run.sources.length,
      verifiedFacts: v.run.facts.filter((f) => f.check.status === "verified").length,
      conclusions: v.run.conclusions.length,
      costUsd: v.report.totals.costUsd,
    }));
}

function estimateOf(lines: { label: string; count: number; typicalUsd: number; maxUsd: number }[]): ResearchEstimate {
  const typicalUsd = lines.reduce((n, l) => n + l.typicalUsd, 0);
  const maxUsd = lines.reduce((n, l) => n + l.maxUsd, 0);
  return { typicalUsd, maxUsd, budgetUsd: 30, monthSpentUsd: spent, withinBudget: spent + maxUsd <= 30, lines };
}

const tickets = new Map<number, { kind: "plan"; question: string } | { kind: "execute"; uid: string }>();
let nextTicket = 1;
const progress = (p: ResearchProgress) => void emit("research-progress", p);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let cancelled = false;

function record(purpose: string, provider: string, model: string, cost: number, input = 0, output = 0, searches = 0) {
  calls.push({ at: now(), purpose, provider, kind: purpose === "research_search" ? "search" : "model", searches, model, tokens: { input, output, cacheRead: 0, cacheWrite: 0 }, costUsd: real ? cost : 0, estimated: false, mock: !real, note: null, requestId: null });
  if (real) spent += cost;
}

/** 后台做：按步骤发进度，做完换成引擎生成的结果 */
async function execute(v: MView) {
  const u = v.run.uid;
  running = u;
  cancelled = false;
  v.run.status = "running";
  const order: ResearchStage[] = ["search", "download", "extract", "verify", "conclude", "report"];
  const counts: Record<string, number> = { search: 7, download: 9, extract: 7, verify: 1, conclude: 1, report: 1 };
  const label = (s: ResearchStage, i: number) =>
    ({
      plan: "",
      search: `搜索：${DONE.run.searches[i]?.query ?? "…"}`,
      download: `下载：${DONE.run.sources[i]?.url ?? "…"}`,
      extract: `读 ${DONE.run.sources[i]?.id ?? ""}：${DONE.run.sources[i]?.title ?? ""}`,
      verify: "逐字核对摘录和数值…",
      conclude: "模型在写结论…",
      report: "完成",
    })[s];
  for (const stage of order.slice(Math.max(0, order.indexOf(v.run.stage)))) {
    v.run.stage = stage;
    for (let i = 0; i < counts[stage]; i++) {
      if (cancelled) {
        v.run.status = "cancelled";
        running = null;
        void emit("research-done", { uid: u, status: "cancelled", error: null });
        return;
      }
      progress({ uid: u, stage, done: i, total: counts[stage], message: label(stage, i) });
      if (stage === "search") record("research_search", engine(), engine(), 0.008, 0, 0, 1);
      if (stage === "extract") record("research_extract", "openai_compatible", "deepseek-flash", 0.0016, 3000, 600);
      if (stage === "conclude") record("research_conclude", "anthropic", "claude-opus-5-5", 0.12, 6400, 2300);
      await sleep(stage === "conclude" ? 1200 : 260);
    }
  }
  const prev = v.run.previous ? views.get(v.run.previous) : undefined;
  const done = snapshotAs(prev ? RERUN : DONE, { uid: u, question: v.run.question, seq: v.run.seq, previous: v.run.previous, at: now() });
  // 用户改过的计划留着；没有前一次的不显示对比
  done.run.plan = v.run.plan;
  done.report.subQuestions = v.run.plan.subQuestions;
  if (!prev) done.report.diff = null;
  v.run = done.run;
  v.report = done.report;
  v.planEditable = false;
  running = null;
  void emit("research-done", { uid: u, status: "done", error: null });
}

function renumber(plan: ResearchPlan) {
  plan.subQuestions.forEach((q, i) => (q.id = `Q${i + 1}`));
}

export const researchHandlers: Record<string, (a: Args) => unknown> = {
  research_status: () => status(),
  research_settings_set: (a) => {
    settings = clone(a.settings as ResearchSettings);
    return status();
  },
  research_key_set: (a) => {
    const which = a.which as keyof typeof keys;
    const key = String(a.key).trim();
    const ok = { anthropic: key.startsWith("sk-ant-") && key.length >= 40, deepseek: key.startsWith("sk-") && !key.startsWith("sk-ant-") && key.length >= 20, tavily: key.startsWith("tvly-") && key.length >= 20 }[which];
    if (!ok) fail(`这看起来不是 ${which === "anthropic" ? "Anthropic" : which === "deepseek" ? "DeepSeek" : "Tavily"} 的 API 密钥。`);
    keys[which] = true;
    return status();
  },
  research_key_clear: (a) => {
    keys[a.which as keyof typeof keys] = false;
    return status();
  },
  research_questions_list: () =>
    [...questions].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).map((q) => ({ ...q, runs: summaries(q.id) })),
  research_question_get: (a) => {
    const q = questions.find((x) => x.id === a.id) ?? fail("这个问题不存在了。");
    return { ...q, runs: summaries(q.id) };
  },
  research_plan_prepare: (a): ResearchPlanPreview => {
    const question = String(a.question ?? "").trim();
    if (!question) fail("先写下要调查的问题。");
    const ticket = nextTicket++;
    tickets.set(ticket, { kind: "plan", question });
    return {
      ticket,
      question,
      chars: [...question].length,
      provider: settings.planner.provider,
      model: settings.planner.model,
      webSearches: status().planWebSearches,
      estimate: estimateOf([{ label: `拆成子问题（${settings.planner.model}，最多搜 3 次）`, count: 1, typicalUsd: 0.11, maxUsd: 0.36 }]),
      mock: !real,
      missingKeys: missing(),
    };
  },
  research_plan_run: async (a) => {
    const t = tickets.get(Number(a.ticket));
    if (!t || t.kind !== "plan") fail("这次确认已经过期，请重新确认。");
    tickets.delete(Number(a.ticket));
    running = "plan";
    cancelled = false;
    for (const [i, m] of ["模型在拆问题…", "模型在用网页搜索摸底…", "模型在写子问题…"].entries()) {
      progress({ uid: null, stage: "plan", done: i, total: 3, message: m });
      await sleep(700);
      if (cancelled) {
        running = null;
        fail("已停止。");
      }
    }
    running = null;
    record("research_plan", "anthropic", "claude-opus-5-5", 0.14, 2100, 1600, 1);
    const at = now();
    const q = addQuestion((t as { question: string }).question, at);
    const s = snapshotAs(PLANNED, { uid: uid(), question: q.text, seq: 1, previous: null, at });
    const v: MView = { questionId: q.id, ...s, planEditable: true };
    views.set(s.run.uid, v);
    return toView(v);
  },
  research_run_get: (a) => toView(findView(a.uid)),
  research_plan_update: (a) => {
    const v = findView(a.uid);
    if (!v.planEditable) fail("已经开始检索了，计划不能再改。");
    const plan = clone(a.plan as ResearchPlan);
    plan.subQuestions = plan.subQuestions.filter((q) => q.text.trim());
    for (const q of plan.subQuestions) q.queries = q.queries.length ? q.queries.slice(0, 3) : [q.text];
    if (!plan.subQuestions.some((q) => q.enabled)) fail("至少留一个子问题。");
    renumber(plan);
    v.run.plan = plan;
    v.report.subQuestions = plan.subQuestions;
    return toView(v);
  },
  research_source_add: (a) => {
    const v = findView(a.uid);
    const url = String(a.url).trim();
    if (!/^https?:\/\//.test(url) && !url.startsWith("/")) fail("要填 http(s) 开头的网址，或者选一个本机文件。");
    if (!v.run.sources.some((s) => s.url === url)) {
      v.run.sources.push({ id: `S${v.run.sources.length + 1}`, url, origin: "manual", subQuestions: [], title: "", finalUrl: null, fetchedAt: null, fetchedVia: null, sha256: null, kind: null, contentType: null, ext: null, bytes: 0, textChars: 0, fetchError: null, textError: null, extracted: false, readChars: 0, truncated: false, extractError: null, publisher: "", published: "", tier: null, itemId: null });
    }
    return toView(v);
  },
  research_source_remove: (a) => {
    const v = findView(a.uid);
    v.run.sources = v.run.sources.filter((s) => s.id !== a.key);
    return toView(v);
  },
  research_execute_prepare: (a): ResearchExecutePreview => {
    const v = findView(a.uid);
    if (v.run.status === "done") fail("这次调查已经做完了。");
    const ticket = nextTicket++;
    tickets.set(ticket, { kind: "execute", uid: v.run.uid });
    const queries = v.run.plan.subQuestions.filter((q) => q.enabled).reduce((n, q) => n + Math.min(3, q.queries.length), 0);
    const estimate = estimateOf([
      { label: `检索（${engine()}）`, count: queries, typicalUsd: queries * 0.008, maxUsd: queries * 0.008 },
      { label: "摘证据（deepseek-flash）", count: Math.min(40, queries * 5), typicalUsd: 0.03, maxUsd: 1.9 },
      { label: "写结论（claude-opus-5-5）", count: 1, typicalUsd: 0.12, maxUsd: 0.86 },
    ]);
    return {
      ticket,
      uid: v.run.uid,
      stage: v.run.stage,
      subQuestions: v.run.plan.subQuestions.filter((q) => q.enabled).length,
      queries,
      searchEngine: real ? engine() : "mock",
      maxSources: Math.min(40, queries * 5),
      manualSources: v.run.sources.filter((s) => s.origin === "manual").length,
      workerModel: settings.worker.model,
      plannerModel: settings.planner.model,
      estimate,
      mock: !real,
      missingKeys: missing(),
      mayStop: real && !estimate.withinBudget,
    };
  },
  research_execute: (a) => {
    const t = tickets.get(Number(a.ticket));
    if (!t || t.kind !== "execute" || t.uid !== a.uid) fail("这次确认已经过期，请重新确认。");
    tickets.delete(Number(a.ticket));
    if (running) fail("上一次调查还在进行。等它做完，或者先停止。");
    const m = missing();
    if (m.length) fail(`还没有填 ${m.join("、")} 的 API 密钥。到「设置 › 密钥与费用」填写后再试。`);
    const v = findView(a.uid);
    v.planEditable = false;
    void execute(v);
    return null;
  },
  research_cancel: () => {
    cancelled = true;
    return null;
  },
  research_rerun: (a) => {
    const prev = findView(a.uid);
    if (prev.run.status !== "done") fail("上一次调查还没有做完。");
    const at = now();
    const s = snapshotAs(PLANNED, { uid: uid(), question: prev.run.question, seq: Math.max(...summaries(prev.questionId).map((r) => r.seq)) + 1, previous: prev.run.uid, at });
    s.run.plan = clone(prev.run.plan);
    s.report.subQuestions = s.run.plan.subQuestions;
    const v: MView = { questionId: prev.questionId, ...s, planEditable: true };
    views.set(s.run.uid, v);
    const q = questions.find((x) => x.id === prev.questionId);
    if (q) q.updatedAt = at;
    return toView(v);
  },
  research_run_delete: (a) => {
    const v = findView(a.uid);
    views.delete(v.run.uid);
    if (summaries(v.questionId).length === 0) questions.splice(questions.findIndex((q) => q.id === v.questionId), 1);
    return null;
  },
  research_question_delete: (a) => {
    for (const [k, v] of views) if (v.questionId === a.id) views.delete(k);
    questions.splice(questions.findIndex((q) => q.id === a.id), 1);
    return null;
  },
  research_export: (a) => {
    const v = findView(a.uid);
    const ext = { markdown: "md", html: "html", docx: "docx" }[String(a.format)] ?? "md";
    return `${String(a.dir)}/${v.run.question.slice(0, 40).replace(/[/\\:*?"<>|？]/g, " ").trim()}（第 ${v.run.seq} 次调查）.${ext}`;
  },
  research_reveal: () => null,
  research_add_to_library: (a) => {
    const v = findView(a.uid);
    const added = (a.keys as string[]).map((key, i) => {
      const s = v.run.sources.find((x) => x.id === key) ?? fail(`这次调查里没有来源 ${key}`);
      const created = s.itemId === null;
      if (created) s.itemId = 9000 + i;
      const row = v.report.sources.find((x) => x.id === key);
      if (row) row.itemId = s.itemId;
      return { key, itemId: s.itemId!, created, attachmentId: created ? 9500 + i : null, attachmentError: null };
    });
    return { added, view: toView(v) };
  },
};
