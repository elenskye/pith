// 设置 › 事实调查（decisions/0020）：计划模型和执行模型、各接口的专用搜索、下载失败时的网页抓取、单价、可信度等级表、最近的调用。
// 三把密钥和月度上限在「设置 › 密钥与费用」（KeysSettings，decisions/0023）；KeyRow 在这里定义、那边用。
// 密钥只交给 Rust 保存：输入框保存后立即清空，界面上不显示。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, LoaderCircle } from "lucide-react";
import { api, type AgentKeyPlace, type ResearchEndpoint, type ResearchKeyWhich, type ResearchSettings, type ResearchStatus, type ResearchTier } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Badge, Notice } from "@/format/parts";
import { count, when } from "@/format/labels";
import { ENGINE, PROVIDER, PURPOSE, TIER, TIERS, money } from "@/research/labels";

const PLACE: Record<AgentKeyPlace, string> = {
  keychain: "存在 macOS 钥匙串",
  memory: "只在这次运行的内存里，退出 Pith 就没了",
  file: "来自 PITH_API_KEY_FILE 指的文件",
};

const KEY_HINT: Record<ResearchKeyWhich, { placeholder: string; where: string }> = {
  anthropic: { placeholder: "sk-ant-…", where: "console.anthropic.com 的 API Keys。和「排版 Agent」用的是同一把。" },
  deepseek: { placeholder: "sk-…", where: "platform.deepseek.com 的 API keys（要先充值少量余额）。" },
  tavily: { placeholder: "tvly-…", where: "app.tavily.com（免费额度每月 1000 点）。" },
};

export function KeyRow({ k, onStatus }: { k: ResearchStatus["keys"][number]; onStatus: (s: ResearchStatus) => void }) {
  const { confirm, notify } = useUi();
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const save = async () => {
    const v = key.trim();
    if (!v) return;
    setKey("");
    setSaving(true);
    try {
      onStatus(await api.researchKeySet(k.which, v));
      notify(`${k.label} 的密钥已保存`);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setSaving(false);
    }
  };
  const clear = async () => {
    if (!(await confirm({ title: `清除 ${k.label} 的密钥？`, message: k.which === "anthropic" ? "排版 Agent 也用这一把，清除后两边都发不了。" : "清除后用到它的调查发不了，要用时再填。", confirm: "清除", danger: true }))) return;
    try {
      onStatus(await api.researchKeyClear(k.which));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  return (
    <div className="contents">
      <dt className="pt-0.5 text-muted-foreground">{k.label}</dt>
      <dd className="min-w-0">
        <p className="flex items-center gap-1.5">
          {k.has ? <CircleCheck size={13} className="shrink-0 text-emerald-600" /> : <CircleAlert size={13} className={`shrink-0 ${k.needed ? "text-amber-600" : "text-muted-foreground"}`} />}
          <span>{k.has ? "已填" : "还没有填"}</span>
          <span className="text-xs text-muted-foreground">（{PLACE[k.place]}）</span>
          {!k.needed && k.which !== "anthropic" && <Badge>现在的调查设置用不到</Badge>}
        </p>
        {k.place !== "file" && (
          <form
            className="mt-1 flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <input type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder={k.has ? "填新的密钥来替换" : KEY_HINT[k.which].placeholder} autoComplete="off" className="min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring" />
            <button type="submit" disabled={!key.trim() || saving} className="shrink-0 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50">
              保存
            </button>
            <button type="button" onClick={() => void clear()} disabled={!k.has} className="shrink-0 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
              清除
            </button>
          </form>
        )}
        <p className="mt-0.5 text-xs text-muted-foreground">{KEY_HINT[k.which].where}</p>
      </dd>
    </div>
  );
}

function EndpointEditor({ value, onChange }: { value: ResearchEndpoint; onChange: (e: ResearchEndpoint) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <select
        value={value.provider}
        onChange={(e) => {
          const provider = e.target.value as ResearchEndpoint["provider"];
          onChange(provider === "anthropic" ? { provider, model: "claude-opus-5-5", baseUrl: null } : { provider, model: "deepseek-flash", baseUrl: "https://api.deepseek.com" });
        }}
        className="rounded-md border bg-background px-1.5 py-1"
      >
        <option value="anthropic">{PROVIDER.anthropic}</option>
        <option value="openai_compatible">{PROVIDER.openai_compatible}</option>
      </select>
      <input value={value.model} onChange={(e) => onChange({ ...value, model: e.target.value })} placeholder="模型名" className="selectable w-44 rounded-md border bg-background px-2 py-1 font-mono outline-none focus:border-ring" />
      <input
        value={value.baseUrl ?? ""}
        onChange={(e) => onChange({ ...value, baseUrl: e.target.value.trim() ? e.target.value : null })}
        placeholder={value.provider === "anthropic" ? "https://api.anthropic.com（默认）" : "https://api.deepseek.com"}
        className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono outline-none focus:border-ring"
      />
    </div>
  );
}

function PriceInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const [v, setV] = useState(String(value));
  useEffect(() => setV(String(value)), [value]);
  return (
    <input
      value={v}
      inputMode="decimal"
      onChange={(e) => setV(e.target.value)}
      onBlur={() => {
        const n = Number(v);
        if (Number.isFinite(n) && n >= 0) onChange(n);
        else setV(String(value));
      }}
      className="selectable w-16 rounded border bg-background px-1.5 py-0.5 text-right text-xs tabular-nums outline-none focus:border-ring"
    />
  );
}

export function ResearchSettingsSection({ onKeys }: { onKeys: () => void }) {
  const qc = useQueryClient();
  const { notify } = useUi();
  const status = useQuery({ queryKey: ["researchStatus"], queryFn: api.researchStatus, staleTime: 0 });
  const put = (s: ResearchStatus) => qc.setQueryData(["researchStatus"], s);
  const s = status.data;
  const [draft, setDraft] = useState<ResearchSettings | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (s) setDraft(structuredClone(s.settings));
  }, [s]);
  const dirty = !!s && !!draft && JSON.stringify(draft) !== JSON.stringify(s.settings);

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      put(await api.researchSettingsSet(draft));
      notify("事实调查的设置已保存");
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setSaving(false);
    }
  };
  const setTier = (kind: string, tier: ResearchTier) => draft && setDraft({ ...draft, trust: { tiers: { ...draft.trust.tiers, [kind]: tier } } });
  const tierOf = (kind: string, fallback: ResearchTier) => (draft?.trust.tiers as Record<string, ResearchTier> | undefined)?.[kind] ?? fallback;
  const setPrice = (model: string, which: "input" | "output", v: number, current: { input: number; output: number }) =>
    draft && setDraft({ ...draft, prices: { ...draft.prices, models: { ...draft.prices.models, [model]: { ...current, ...(draft.prices.models[model] ?? {}), [which]: v } } } });

  return (
    <div>
      <p className="mb-3 text-xs text-muted-foreground">
        「调查」里拆问题、写结论用计划模型，读网页、摘证据用执行模型，检索用执行模型绑定的专用搜索（没有就用 Tavily）。网页和文件由 Pith 在这台电脑上下载，摘录和数字由 Pith 核对。每次发送前都会列出要发的内容和预计费用。
      </p>
      {status.isLoading && <LoaderCircle size={14} className="animate-spin text-muted-foreground" />}
      {status.error && <p className="selectable text-xs text-destructive">{String(status.error)}</p>}
      {s && draft && (
        <div className="space-y-4">
          {s.mock && <Notice tone="info" title="开发版和测试用模拟接口：不联网、不花钱。下面的密钥、模型和单价只在换成真接口时用到。" />}
          {s.ledgerError && <Notice tone="danger" title="用量账本读不了，为了不超支，现在发不了请求。">{s.ledgerError}</Notice>}

          <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-x-3 gap-y-3 text-sm">
            <dt className="text-muted-foreground">密钥与上限</dt>
            <dd className="text-xs">
              {(() => {
                const missing = s.keys.filter((k) => k.needed && !k.has).map((k) => k.label);
                return <span className={missing.length ? "text-amber-700" : ""}>{missing.length ? `还没有填 ${missing.join("、")} 的密钥` : "要用的密钥都填了"}</span>;
              })()}
              <span className="text-muted-foreground tabular-nums">
                {" "}
                · 本月已花 {money(s.monthSpentUsd)} / 上限 {money(s.budgetUsd)}（所有接口合计，和排版 Agent 共用）
              </span>
              <button onClick={onKeys} className="ml-2 text-primary hover:underline">
                去「密钥与费用」
              </button>
            </dd>

            <dt className="pt-1 text-muted-foreground">计划和结论</dt>
            <dd>
              <EndpointEditor value={draft.planner} onChange={(planner) => setDraft({ ...draft, planner })} />
              <p className="mt-0.5 text-xs text-muted-foreground">拆问题、下结论要准确，默认 Claude Opus 5.5。</p>
            </dd>

            <dt className="pt-1 text-muted-foreground">执行</dt>
            <dd>
              <EndpointEditor value={draft.worker} onChange={(worker) => setDraft({ ...draft, worker })} />
              <p className="mt-0.5 text-xs text-muted-foreground">读网页、摘证据，用便宜的模型，默认 DeepSeek（deepseek-flash，OpenAI 兼容接口）。</p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">专用搜索</dt>
            <dd className="space-y-1 text-xs">
              <label className="flex items-center gap-2">
                <span className="w-28 shrink-0">Anthropic</span>
                <select
                  value={draft.dedicatedSearch.anthropic ?? "none"}
                  onChange={(e) => setDraft({ ...draft, dedicatedSearch: { ...draft.dedicatedSearch, anthropic: e.target.value } })}
                  className="rounded-md border bg-background px-1.5 py-1"
                >
                  <option value="claude_web_search">Claude 网页搜索（每千次 $10）</option>
                  <option value="none">无（用 Tavily）</option>
                </select>
              </label>
              <p className="flex items-center gap-2">
                <span className="w-28 shrink-0">OpenAI 兼容</span>
                <span className="text-muted-foreground">没有专用搜索，用 Tavily</span>
              </p>
              <label className="flex items-center gap-2">
                <span className="w-28 shrink-0">Tavily 搜索深度</span>
                <select value={draft.tavilyDepth} onChange={(e) => setDraft({ ...draft, tavilyDepth: e.target.value as "basic" | "advanced" })} className="rounded-md border bg-background px-1.5 py-1">
                  <option value="basic">基础（1 点）</option>
                  <option value="advanced">高级（2 点）</option>
                </select>
              </label>
              <label className="flex items-center gap-2">
                <span className="w-28 shrink-0">计划时摸底</span>
                <input
                  type="number"
                  min={0}
                  max={10}
                  value={draft.planWebSearches}
                  onChange={(e) => setDraft({ ...draft, planWebSearches: Math.max(0, Math.min(10, Number(e.target.value) || 0)) })}
                  className="w-14 rounded-md border bg-background px-1.5 py-0.5 tabular-nums"
                />
                <span className="text-muted-foreground">次 Claude 网页搜索（计划模型是 Anthropic、专用搜索开着时；0 = 不搜）</span>
              </label>
              <p className="text-muted-foreground">
                现在检索用：<span className="text-foreground">{ENGINE[s.searchEngine] ?? s.searchEngine}</span>
              </p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">下载失败时</dt>
            <dd className="space-y-1 text-xs">
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={draft.webFetchFallback} onChange={(e) => setDraft({ ...draft, webFetchFallback: e.target.checked })} />
                <span>改用 Claude 网页抓取</span>
                <input
                  value={draft.fetchModel}
                  disabled={!draft.webFetchFallback}
                  onChange={(e) => setDraft({ ...draft, fetchModel: e.target.value })}
                  placeholder="claude-haiku-5-5"
                  className="selectable w-40 rounded-md border bg-background px-2 py-0.5 font-mono outline-none focus:border-ring disabled:opacity-50"
                />
              </label>
              <p className="text-muted-foreground">
                网站挡程序、证书太旧这类本机下载不了的网页和 PDF，交给 Claude 抓一次（要 Anthropic 的密钥，没填就不用）。没有按次收费，抓到的正文按输入 token 计，用便宜的模型一页通常不到 1 美分；报告里会注明哪些来源是这样拿到的。
              </p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">单价</dt>
            <dd className="text-xs">
              <table className="text-left">
                <tbody>
                  {s.prices.map((p) => (
                    <tr key={p.role}>
                      <td className="py-0.5 pr-3 text-muted-foreground">{p.role === "planner" ? "计划和结论" : "执行"}</td>
                      <td className="py-0.5 pr-3 font-mono">{p.model}</td>
                      <td className="py-0.5 pr-1">输入 $</td>
                      <td className="py-0.5 pr-2">
                        <PriceInput value={draft.prices.models[p.model]?.input ?? p.input} onChange={(v) => setPrice(p.model, "input", v, p)} />
                      </td>
                      <td className="py-0.5 pr-1">输出 $</td>
                      <td className="py-0.5 pr-2">
                        <PriceInput value={draft.prices.models[p.model]?.output ?? p.output} onChange={(v) => setPrice(p.model, "output", v, p)} />
                      </td>
                      <td className="py-0.5">{!p.known && !draft.prices.models[p.model] && <Badge tone="warn">单价表里没有，按 $10 / $50 记</Badge>}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1">
                <span>
                  Tavily 每点 $ <PriceInput value={draft.prices.tavilyCreditUsd ?? s.tavilyCreditUsd} onChange={(v) => setDraft({ ...draft, prices: { ...draft.prices, tavilyCreditUsd: v } })} />
                </span>
                <span>
                  Claude 网页搜索每次 $ <PriceInput value={draft.prices.claudeSearchUsd ?? s.claudeSearchUsd} onChange={(v) => setDraft({ ...draft, prices: { ...draft.prices, claudeSearchUsd: v } })} />
                </span>
              </p>
              <p className="mt-1 text-muted-foreground">美元每百万 token。DeepSeek 按高峰价记（闲时半价也按高峰价，宁可多记）；Tavily 的免费额度也照价记。</p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">可信度等级表</dt>
            <dd className="text-xs">
              <p className="mb-1 text-muted-foreground">按数据生产者（谁统计的，不是谁转述的）分级。来源和事实按它排序，报告里写明用的是哪张表。</p>
              <table className="w-full text-left">
                <tbody>
                  {s.trust.map((t) => {
                    const tier = tierOf(t.kind, t.defaultTier);
                    return (
                      <tr key={t.kind} className="border-b border-border/60">
                        <td className="py-1 pr-2" title={t.hint}>
                          {t.label}
                        </td>
                        <td className="py-1 pr-2">
                          <select value={tier} onChange={(e) => setTier(t.kind, e.target.value as ResearchTier)} className="rounded border bg-background px-1 py-0.5">
                            {TIERS.map((x) => (
                              <option key={x} value={x}>
                                {TIER[x]}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td className="py-1 text-muted-foreground">{tier !== t.defaultTier && `默认「${TIER[t.defaultTier]}」`}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <div className="mt-1 flex items-center gap-2">
                <span className="text-muted-foreground">现在用的是「{s.trustName}」</span>
                <button onClick={() => setDraft({ ...draft, trust: { tiers: {} } })} className="rounded border px-2 py-0.5 hover:bg-muted">
                  恢复默认
                </button>
              </div>
            </dd>

          </dl>

          <div className="flex items-center justify-end gap-2">
            {dirty && <span className="text-xs text-amber-700">有改动还没保存</span>}
            <button onClick={() => setDraft(structuredClone(s.settings))} disabled={!dirty} className="rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
              还原
            </button>
            <button onClick={() => void save()} disabled={!dirty || saving} className="rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50">
              保存设置
            </button>
          </div>

          <div>
            <p className="mb-1.5 text-xs text-muted-foreground">事实调查最近的调用</p>
            {s.calls.length === 0 ? (
              <p className="text-xs text-muted-foreground">还没有调用过。</p>
            ) : (
              <div className="max-h-64 overflow-auto">
                <table className="w-full text-left text-xs">
                  <thead className="text-muted-foreground">
                    <tr className="border-b">
                      <th className="py-1 pr-3 font-normal">时间</th>
                      <th className="py-1 pr-3 font-normal">用途</th>
                      <th className="py-1 pr-3 font-normal">接口 · 模型</th>
                      <th className="py-1 pr-3 text-right font-normal">用量</th>
                      <th className="py-1 text-right font-normal">费用</th>
                    </tr>
                  </thead>
                  <tbody>
                    {s.calls.map((c, i) => (
                      <tr key={`${c.at}-${i}`} className="border-b border-border/60" title={c.note ?? undefined}>
                        <td className="py-1 pr-3 whitespace-nowrap tabular-nums">{when(c.at)}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">{PURPOSE[c.purpose] ?? c.purpose}</td>
                        <td className="py-1 pr-3 whitespace-nowrap text-muted-foreground">
                          {PROVIDER[c.provider] ?? c.provider} · {c.model}
                        </td>
                        <td className="py-1 pr-3 text-right whitespace-nowrap tabular-nums">{c.kind === "search" ? `${c.searches} 次` : `${count(c.tokens.input)} / ${count(c.tokens.output)}`}</td>
                        <td className="py-1 text-right tabular-nums">
                          {c.mock ? "模拟" : money(c.costUsd)}
                          {c.estimated && <Badge tone="warn">估计</Badge>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
