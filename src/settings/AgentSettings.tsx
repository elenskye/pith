// 设置 › 排版 Agent：模型与单价、API 密钥、月度上限、本月用量与最近的调用（decisions/0018）。
// 密钥只交给 Rust 保存：输入框保存后立即清空，界面上不显示、不记日志。
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, LoaderCircle } from "lucide-react";
import { api, type AgentKeyPlace, type AgentStatus, type FormatMode } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { count, MODE, MODE_HINT, modelName, PURPOSE, usd, when } from "@/format/labels";
import { Badge, Notice } from "@/format/parts";

const PLACE: Record<AgentKeyPlace, string> = {
  keychain: "存在 macOS 钥匙串",
  memory: "只在这次运行的内存里，退出 Pith 就没了",
  file: "来自 PITH_API_KEY_FILE 指的文件，不能在这里改",
};

export function AgentSettingsSection() {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const status = useQuery({ queryKey: ["agentStatus"], queryFn: api.agentStatus, staleTime: 0 });
  const put = (s: AgentStatus) => qc.setQueryData(["agentStatus"], s);
  const [key, setKey] = useState("");
  const [budget, setBudget] = useState("");
  const s = status.data;
  useEffect(() => {
    if (s) setBudget(String(s.budgetUsd));
  }, [s?.budgetUsd]);

  const [savingKey, setSavingKey] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);
  // 不用 useMutation：它会把参数（密钥）留在缓存里。成功失败都立即清空输入框
  const saveKey = async () => {
    const k = key.trim();
    if (!k) return;
    setKey("");
    setSavingKey(true);
    setKeyError(null);
    try {
      put(await api.agentKeySet(k));
      notify("密钥已保存");
    } catch (e) {
      setKeyError(String(e));
    } finally {
      setSavingKey(false);
    }
  };
  const clearKey = useMutation({ mutationFn: api.agentKeyClear, onSuccess: put });
  const saveMode = useMutation({
    mutationFn: (mode: FormatMode) => api.agentSendModeSet(mode),
    onSuccess: (st) => qc.setQueryData(["agentStatus"], st),
    onError: (e) => notify(String(e), "error"),
  });
  const saveBudget = useMutation({
    mutationFn: (usd: number) => api.agentBudgetSet(usd),
    onSuccess: (r) => {
      put(r);
      notify("月度上限已保存");
    },
  });

  const askClear = async () => {
    const ok = await confirm({ title: "清除 API 密钥？", message: "清除后提炼要求、生成计划都发不了，要用时再填。已经生成的清单、计划和报告不受影响。", confirm: "清除", danger: true });
    if (ok) clearKey.mutate();
  };

  const submitBudget = () => {
    const n = Number(budget);
    if (budget.trim() === "" || !Number.isFinite(n)) return notify("月度上限写一个 1 到 50 之间的数", "error");
    saveBudget.mutate(n);
  };

  const pct = s && s.budgetUsd > 0 ? Math.min(100, (s.monthSpentUsd / s.budgetUsd) * 100) : 0;
  const fromFile = s?.keyPlace === "file";
  const error = keyError ?? clearKey.error ?? saveBudget.error ?? status.error;

  return (
    <div>
      <h2 className="mb-3 text-xs font-medium text-muted-foreground">排版 Agent</h2>
      <p className="mb-3 text-xs text-muted-foreground">
        「排版」里提炼要求清单、生成文档计划这两步要调用模型。联网范围：只在你点「发送」时，把确认框里列出的内容发到 api.anthropic.com；排版和核对都在本机完成。
      </p>
      {status.isLoading && <LoaderCircle size={14} className="animate-spin text-muted-foreground" />}
      {s && (
        <div className="space-y-4">
          {s.mock && <Notice tone="info" title="开发版和测试用模拟模型：不联网、不花钱。下面的密钥和上限只在换成真模型时用到。" />}
          {s.ledgerError && (
            <Notice tone="danger" title="用量账本读不了，为了不超支，现在发不了请求。">
              {s.ledgerError}
            </Notice>
          )}

          <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-x-3 gap-y-2.5 text-sm">
            <dt className="text-muted-foreground">模型</dt>
            <dd>
              {modelName(s.model)}
              <span className="ml-2 text-xs text-muted-foreground tabular-nums">
                输入 ${s.priceInput} / 输出 ${s.priceOutput}（美元每百万 token）
              </span>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">API 密钥</dt>
            <dd className="min-w-0">
              <p className="flex items-center gap-1.5">
                {s.hasKey ? <CircleCheck size={13} className="shrink-0 text-emerald-600" /> : <CircleAlert size={13} className="shrink-0 text-amber-600" />}
                <span>{s.hasKey ? "已填" : "还没有填"}</span>
                <span className="text-xs text-muted-foreground">（{PLACE[s.keyPlace]}）</span>
              </p>
              {!fromFile && (
                <form
                  className="mt-1.5 flex gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void saveKey();
                  }}
                >
                  <input
                    type="password"
                    value={key}
                    onChange={(e) => setKey(e.target.value)}
                    placeholder={s.hasKey ? "填新的密钥来替换" : "sk-ant-…"}
                    autoComplete="off"
                    className="min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring"
                  />
                  <button type="submit" disabled={!key.trim() || savingKey} className="shrink-0 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50">
                    保存
                  </button>
                  <button type="button" onClick={askClear} disabled={!s.hasKey || clearKey.isPending} className="shrink-0 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
                    清除
                  </button>
                </form>
              )}
              <p className="mt-1 text-xs text-muted-foreground">在 console.anthropic.com 的 API Keys 里创建。保存后这里不再显示它。</p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">月度上限</dt>
            <dd>
              <form
                className="flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  submitBudget();
                }}
              >
                <span className="text-muted-foreground">$</span>
                <input
                  type="number"
                  min={1}
                  max={50}
                  step={1}
                  value={budget}
                  onChange={(e) => setBudget(e.target.value)}
                  className="selectable w-20 rounded-md border bg-background px-2 py-1 text-xs tabular-nums outline-none focus:border-ring"
                />
                <button type="submit" disabled={saveBudget.isPending || Number(budget) === s.budgetUsd} className="rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
                  保存
                </button>
                <span className="text-xs text-muted-foreground">$1 到 $50</span>
              </form>
              <p className="mt-1 text-xs text-muted-foreground">发送前按最坏情况估计，加上本月已花的会超过上限就不发。</p>
            </dd>

            <dt className="pt-0.5 text-muted-foreground">论文发送方式</dt>
            <dd>
              <div className="space-y-1">
                {(["brief", "full"] as FormatMode[]).map((m) => (
                  <label key={m} className="flex cursor-default items-start gap-2 text-xs">
                    <input type="radio" name="agent-send-mode" checked={s.sendMode === m} disabled={saveMode.isPending} onChange={() => saveMode.mutate(m)} className="mt-0.5" />
                    <span>
                      {MODE[m]}
                      {m === "brief" && <span className="text-muted-foreground">（默认）</span>}
                      <span className="block text-muted-foreground">{MODE_HINT[m]}</span>
                    </span>
                  </label>
                ))}
              </div>
              <p className="mt-1 text-xs text-muted-foreground">生成文档计划时给模型看论文的哪些内容。两种方式排出来的结果相当，「全文」约贵两三倍；每次发送前的确认框会写明用的是哪种。</p>
            </dd>

            <dt className="text-muted-foreground">本月已花</dt>
            <dd>
              <p className="tabular-nums">
                {usd(s.monthSpentUsd)} <span className="text-muted-foreground">/ 上限 {usd(s.budgetUsd)}</span>
                <span className="ml-2 text-xs text-muted-foreground">{s.month}</span>
              </p>
              <div className="mt-1.5 h-1.5 max-w-xs overflow-hidden rounded bg-muted">
                <div className={`h-full ${pct >= 90 ? "bg-destructive" : "bg-primary"}`} style={{ width: `${pct}%` }} />
              </div>
            </dd>
          </dl>

          {error && <p className="selectable text-xs whitespace-pre-wrap text-destructive">{String(error)}</p>}

          <div>
            <p className="mb-1.5 text-xs text-muted-foreground">最近的调用</p>
            {s.calls.length === 0 ? (
              <p className="text-xs text-muted-foreground">还没有调用过模型。</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-left text-xs">
                  <thead className="text-muted-foreground">
                    <tr className="border-b">
                      <th className="py-1.5 pr-3 font-normal">时间</th>
                      <th className="py-1.5 pr-3 font-normal">用途</th>
                      <th className="py-1.5 pr-3 font-normal">模型</th>
                      <th className="py-1.5 pr-3 text-right font-normal whitespace-nowrap">输入 / 输出 token</th>
                      <th className="py-1.5 pr-3 text-right font-normal">费用</th>
                      <th className="py-1.5 font-normal" />
                    </tr>
                  </thead>
                  <tbody>
                    {s.calls.map((c, i) => (
                      <tr key={`${c.at}-${i}`} className="border-b border-border/60" title={c.note ?? undefined}>
                        <td className="py-1.5 pr-3 whitespace-nowrap tabular-nums">{when(c.at)}</td>
                        <td className="py-1.5 pr-3 whitespace-nowrap">{PURPOSE[c.purpose] ?? c.purpose}</td>
                        <td className="py-1.5 pr-3 whitespace-nowrap text-muted-foreground">{modelName(c.model)}</td>
                        <td className="py-1.5 pr-3 text-right whitespace-nowrap tabular-nums">
                          {count(c.tokens.input)} / {count(c.tokens.output)}
                        </td>
                        <td className="py-1.5 pr-3 text-right tabular-nums">{c.mock ? "—" : usd(c.costUsd)}</td>
                        <td className="py-1.5">
                          <span className="flex gap-1">
                            {c.mock && <Badge>模拟</Badge>}
                            {c.estimated && (
                              <Badge tone="warn" title="中断或出错，拿不到确切用量，按估计记（宁多勿少）">
                                估计
                              </Badge>
                            )}
                            {c.note && !c.estimated && (
                              <Badge tone="accent" title={c.note}>
                                备注
                              </Badge>
                            )}
                          </span>
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
      {!s && error && <p className="selectable mt-2 text-xs text-destructive">{String(error)}</p>}
    </div>
  );
}
