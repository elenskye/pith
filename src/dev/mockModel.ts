// ?mock 的模型和用量（1.1.0）：五家、每家三档（和 src-tauri/src/gateway/catalog.rs 同一张表），一本合成的账。
import type { ApiKeyWhich, ModelPrefs, ModelProvider, ModelProviderView, ModelStatus, ModelTierView, UsageCall, UsageSummary } from "@/shared/ipc";

type Args = Record<string, unknown>;

const T = (tier: ModelTierView["tier"], modelLabel: string, model: string, effort: string | null, input: number, output: number, cacheRead: number): ModelTierView => ({ tier, label: { cheap: "便宜", balanced: "均衡", strong: "强" }[tier], model, modelLabel, effort, input, output, cacheRead });

const TIERS: Record<ModelProvider, ModelTierView[]> = {
  anthropic: [T("cheap", "Claude Haiku 5.5", "anthropic/claude-haiku-5-5", "low", 0.1, 0.5, 0.01), T("balanced", "Claude Sonnet 5.5", "anthropic/claude-sonnet-5-5", "medium", 2, 10, 0.1), T("strong", "Claude Opus 5.5", "anthropic/claude-opus-5-5", "high", 4, 20, 0.2)],
  openai: [T("cheap", "GPT-6 Luna", "openai/gpt-6-luna", "low", 0.1, 0.5, 0.01), T("balanced", "GPT-6.2 Sol", "openai/gpt-6.2-sol", "medium", 2, 10, 0.1), T("strong", "GPT-6.2 Astra", "openai/gpt-6.2-astra", "high", 8, 40, 0.4)],
  deepseek: [T("cheap", "DeepSeek V4.1 Flash", "deepseek/deepseek-flash", null, 0.3, 1.2, 0.006), T("balanced", "DeepSeek V4.1 Flash", "deepseek/deepseek-flash", "high", 0.3, 1.2, 0.006), T("strong", "DeepSeek V4 Pro", "deepseek/deepseek-v4-pro", "high", 1.35, 4.05, 0.045)],
  google: [T("cheap", "Gemini 3.5 Flash Lite", "google/gemini-3.5-flash-lite", "low", 0.3, 2.5, 0.03), T("balanced", "Gemini 3.8 Flash", "google/gemini-3.8-flash", "medium", 0.75, 3.75, 0.075), T("strong", "Gemini 4 Carbon", "google/gemini-4-carbon", "high", 2, 10, 0.1)],
  vertex: [T("cheap", "Gemini 3.5 Flash Lite", "vertex/gemini-3.5-flash-lite", "low", 0.3, 2.5, 0.03), T("balanced", "Gemini 3.8 Flash", "vertex/gemini-3.8-flash", "medium", 0.75, 3.75, 0.075), T("strong", "Gemini 4 Carbon", "vertex/gemini-4-carbon", "high", 2, 10, 0.1)],
};
const LABEL: Record<ModelProvider, string> = { anthropic: "Anthropic", openai: "OpenAI", deepseek: "DeepSeek", google: "Google Gemini", vertex: "Vertex AI" };
const KEY: Record<ModelProvider, ApiKeyWhich | null> = { anthropic: "anthropic", openai: "openai", deepseek: "deepseek", google: "google", vertex: null };

const keys = new Set<ApiKeyWhich>(["anthropic", "deepseek"]);
const vertex = { project: "", location: "global", adc: false, gcloud: true };
let prefs: ModelPrefs = { provider: "anthropic", tier: "balanced" };
let budget = 30;

const status = (): ModelStatus => ({
  mock: true,
  prefs,
  providers: (Object.keys(TIERS) as ModelProvider[]).map(
    (id): ModelProviderView => ({ id, label: LABEL[id], ready: id === "vertex" ? !!vertex.project && vertex.adc : keys.has(KEY[id]!), key: KEY[id], keyPlace: KEY[id] ? "memory" : null, tiers: TIERS[id] }),
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
  const PU = (p: string) => (p === "chat" ? "对话" : p === "chat_subagent" ? "子代理" : p.startsWith("research") ? "事实调查" : p.startsWith("concept") ? "概念研究" : "其他");
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
