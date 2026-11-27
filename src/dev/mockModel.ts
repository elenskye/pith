// ?mock 的模型和用量（1.1.0；验收时改成每家一份模型清单、每个模型各定思考，decisions/0041）：和 src-tauri/src/gateway/catalog.rs 同一张表，一本合成的账。
import type { ApiKeyWhich, ModelChoice, ModelPrefs, ModelProvider, ModelProviderView, ModelStatus, ModelView, UsageCall, UsageSummary } from "@/shared/ipc";

type Args = Record<string, unknown>;

const CLAUDE = ["low", "medium", "high", "xhigh", "max"];
const GEMINI = ["low", "medium", "high"];
const M = (id: string, label: string, thinking: ModelView["thinking"], efforts: string[], effort: string, input: number, output: number, cacheRead: number, extra: Partial<ModelView> = {}): ModelView => ({
  id,
  label,
  apiModel: id.split("/")[1],
  thinking,
  efforts,
  source: "builtin",
  found: false,
  priced: true,
  input,
  output,
  cacheRead,
  context: 1_000_000,
  choice: { on: true, effort },
  defaultChoice: { on: true, effort },
  custom: false,
  ...extra,
});

const MODELS: Record<ModelProvider, ModelView[]> = {
  anthropic: [
    M("anthropic/claude-fable-5-2", "Claude Fable 5.2", "always", CLAUDE, "high", 8, 40, 0.2),
    M("anthropic/claude-opus-5-5", "Claude Opus 5.5", "always", CLAUDE, "high", 4, 20, 0.2),
    M("anthropic/claude-sonnet-5-5", "Claude Sonnet 5.5", "switch", CLAUDE, "medium", 2, 10, 0.1),
    M("anthropic/claude-haiku-5-5", "Claude Haiku 5.5", "switch", CLAUDE, "low", 0.1, 0.5, 0.01),
    M("anthropic/claude-opus-5", "Claude Opus 5", "switch", CLAUDE, "medium", 10, 50, 10, { source: "api", found: true, priced: false }),
    M("anthropic/claude-sonnet-4-6", "Claude Sonnet 4.6", "switch", ["low", "medium", "high", "max"], "medium", 10, 50, 10, { source: "api", found: true, priced: false }),
  ],
  openai: [
    M("openai/gpt-6.2-astra", "GPT-6.2 Astra", "always", CLAUDE, "high", 8, 40, 0.4),
    M("openai/gpt-6.2-sol", "GPT-6.2 Sol", "always", CLAUDE, "medium", 2, 10, 0.1),
    M("openai/gpt-6.1-terra", "GPT-6.1 Terra", "switch", CLAUDE, "medium", 1, 5, 0.05),
    M("openai/gpt-6-luna", "GPT-6 Luna", "switch", CLAUDE, "low", 0.1, 0.5, 0.01),
  ],
  deepseek: [M("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro", "switch", ["low", "high", "max"], "high", 1.35, 4.05, 0.045), M("deepseek/deepseek-flash", "DeepSeek V4.1 Flash", "switch", ["low", "high", "max"], "high", 0.3, 1.2, 0.006)],
  google: [
    M("google/gemini-4-carbon", "Gemini 4 Carbon", "always", GEMINI, "high", 2, 10, 0.1),
    M("google/gemini-4-argon", "Gemini 4 Argon", "always", GEMINI, "high", 2, 10, 0.1),
    M("google/gemini-3.8-flash", "Gemini 3.8 Flash", "always", GEMINI, "medium", 0.75, 3.75, 0.075),
    M("google/gemini-3.5-flash-lite", "Gemini 3.5 Flash Lite", "always", ["minimal", ...GEMINI], "low", 0.3, 2.5, 0.03),
  ],
  vertex: [
    M("vertex/gemini-4-carbon", "Gemini 4 Carbon", "always", GEMINI, "high", 2, 10, 0.1),
    M("vertex/gemini-3.8-flash", "Gemini 3.8 Flash", "always", GEMINI, "medium", 0.75, 3.75, 0.075),
    M("vertex/gemini-3.5-flash-lite", "Gemini 3.5 Flash Lite", "always", ["minimal", ...GEMINI], "low", 0.3, 2.5, 0.03),
  ],
};
const STRONG: Record<ModelProvider, string> = { anthropic: "anthropic/claude-opus-5-5", openai: "openai/gpt-6.2-astra", deepseek: "deepseek/deepseek-v4-pro", google: "google/gemini-4-carbon", vertex: "vertex/gemini-4-carbon" };
const LABEL: Record<ModelProvider, string> = { anthropic: "Anthropic", openai: "OpenAI", deepseek: "DeepSeek", google: "Google Gemini", vertex: "Vertex AI" };
const KEY: Record<ModelProvider, ApiKeyWhich | null> = { anthropic: "anthropic", openai: "openai", deepseek: "deepseek", google: "google", vertex: null };

const keys = new Set<ApiKeyWhich>(["anthropic", "deepseek"]);
const vertex = { project: "", location: "global", adc: false, gcloud: true };
let prefs: ModelPrefs = { model: "anthropic/claude-sonnet-5-5", subagent: null };
const choices = new Map<string, ModelChoice>();
const free = new Set<ModelProvider>();
let budget = 30;

const view = (m: ModelView): ModelView => {
  const c = choices.get(m.id);
  return { ...m, choice: c ? { on: m.thinking === "always" || c.on, effort: c.on || m.thinking === "always" ? c.effort : null } : m.defaultChoice, custom: !!c };
};
const provOf = (id: string) => id.split("/")[0] as ModelProvider;

const status = (): ModelStatus => ({
  mock: true,
  prefs: { ...prefs, subagentUsed: prefs.subagent && provOf(prefs.subagent) === provOf(prefs.model) ? prefs.subagent : STRONG[provOf(prefs.model)] },
  providers: (Object.keys(MODELS) as ModelProvider[]).map(
    (id): ModelProviderView => ({
      id,
      label: LABEL[id],
      ready: id === "vertex" ? !!vertex.project && vertex.adc : keys.has(KEY[id]!),
      key: KEY[id],
      keyPlace: KEY[id] ? "memory" : null,
      models: MODELS[id].map(view),
      refreshedAt: id === "anthropic" ? "2026-11-27T09:00:00.000Z" : null,
      refreshNote: null,
      free: id === "google" || id === "vertex" ? free.has(id) : null,
    }),
  ),
  otherKeys: [
    { which: "tavily", label: "Tavily", has: keys.has("tavily"), place: "memory", note: "联网搜索：模型没有自带搜索时用它" },
    { which: "mathpix", label: "Mathpix", has: keys.has("mathpix"), place: "memory", note: "概念研究：短的 PDF 转成文字" },
  ],
  vertex,
  budgetUsd: budget,
});

const day = (d: number, h: number) => `2026-11-${String(d).padStart(2, "0")}T${String(h).padStart(2, "0")}:12:00.000Z`;
const call = (at: string, purpose: string, provider: string, model: string, input: number, output: number, cacheRead: number, costUsd: number, conversation?: string): UsageCall => ({
  at,
  purpose,
  provider,
  kind: "model",
  searches: 0,
  model,
  tokens: { input, output, cacheRead, cacheWrite: 0 },
  costUsd,
  estimated: false,
  mock: false,
  note: null,
  requestId: null,
  conversation: conversation ?? null,
});

export const ledger: UsageCall[] = [
  call(day(3, 9), "research_plan", "anthropic", "claude-opus-5-5", 9_000, 3_000, 0, 0.096),
  call(day(3, 9), "research_extract", "openai_compatible", "deepseek-flash", 60_000, 9_000, 0, 0.029),
  call(day(9, 14), "concept", "openai", "gpt-6.1-sol", 40_000, 6_000, 12_000, 0.14),
  call(day(14, 20), "chat", "anthropic", "claude-sonnet-5-5", 2_400, 900, 18_000, 0.0156, "c-wn"),
  call(day(21, 10), "chat", "anthropic", "claude-sonnet-5-5", 1_800, 1_100, 26_000, 0.0172, "c-wn"),
  call(day(25, 16), "chat_subagent", "anthropic", "claude-opus-5-5", 30_000, 4_000, 0, 0.2, "c-wn"),
  call(day(26, 8), "chat", "anthropic", "claude-sonnet-5-5", 1_200, 700, 31_000, 0.0125, "c-wn"),
];

function summary(conversation: string | null): UsageSummary {
  const real = ledger.filter((c) => !c.mock);
  const days = Array.from({ length: 26 }, (_, i) => real.filter((c) => Number(c.at.slice(8, 10)) === i + 1).reduce((a, c) => a + c.costUsd, 0));
  const slice = (key: (c: UsageCall) => string, label: (k: string) => string) => {
    const m = new Map<string, { usd: number; calls: number }>();
    for (const c of real) {
      const k = key(c);
      const v = m.get(k) ?? { usd: 0, calls: 0 };
      m.set(k, { usd: v.usd + c.costUsd, calls: v.calls + 1 });
    }
    return [...m.entries()].map(([k, v]) => ({ key: k, label: label(k), ...v })).sort((a, b) => b.usd - a.usd);
  };
  const PL: Record<string, string> = { anthropic: "Anthropic", openai: "OpenAI", openai_compatible: "DeepSeek", deepseek: "DeepSeek" };
  const PU = (p: string) => (p === "chat" ? "对话" : p === "chat_subagent" ? "子代理" : p === "chat_web" ? "联网" : p.startsWith("research") ? "事实调查" : p.startsWith("concept") ? "概念研究" : "其他");
  const input = real.reduce((a, c) => a + c.tokens.input, 0);
  const read = real.reduce((a, c) => a + c.tokens.cacheRead, 0);
  return {
    month: "2026-11",
    spentUsd: real.reduce((a, c) => a + c.costUsd, 0),
    budgetUsd: budget,
    mock: true,
    ledgerError: null,
    byProvider: slice((c) => PL[c.provider] ?? c.provider, (k) => k),
    byPurpose: slice((c) => PU(c.purpose), (k) => k),
    days,
    cache: { input, read, write: 0, hitRate: read / Math.max(1, input + read) },
    recent: [...ledger].reverse().slice(0, 20),
    conversationUsd: conversation ? real.filter((c) => c.conversation === conversation).reduce((a, c) => a + c.costUsd, 0) : null,
  };
}

export const modelHandlers: Record<string, (a: Args) => unknown> = {
  model_status: () => status(),
  model_prefs_set: ({ prefs: p }) => {
    prefs = p as ModelPrefs;
    return status();
  },
  model_thinking_set: ({ model, choice }) => {
    if (choice) choices.set(String(model), choice as ModelChoice);
    else choices.delete(String(model));
    return status();
  },
  model_refresh: () => {
    throw "开发版用模拟模型，不问接口；正式版（或 PITH_REAL_MODEL=1）里才能刷新。";
  },
  model_free_set: ({ provider, free: on }) => {
    if (on) free.add(provider as ModelProvider);
    else free.delete(provider as ModelProvider);
    return status();
  },
  model_key_set: ({ which, key }) => {
    if (!String(key).trim()) throw "先粘贴密钥。";
    keys.add(which as ApiKeyWhich);
    return status();
  },
  model_key_clear: ({ which }) => {
    keys.delete(which as ApiKeyWhich);
    return status();
  },
  model_vertex_set: ({ config }) => {
    const c = config as { project: string; location: string };
    vertex.project = c.project;
    vertex.location = c.location || "global";
    return status();
  },
  usage_summary: ({ conversation }) => summary((conversation as string | null) ?? null),
  agent_budget_set: ({ usd }) => {
    budget = Number(usd);
    return null;
  },
};
