// 设置 › 排版 Agent：模型与单价、论文发送方式、最近的调用（decisions/0018）。
// 密钥和月度上限在「设置 › 密钥与费用」，和事实调查共用（decisions/0023）。
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type FormatMode } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { count, MODE, MODE_HINT, modelName, PURPOSE, usd, when } from "@/format/labels";
import { Badge, Notice } from "@/format/parts";

export function AgentSettingsSection({ onKeys }: { onKeys: () => void }) {
  const qc = useQueryClient();
  const { notify } = useUi();
  const status = useQuery({ queryKey: ["agentStatus"], queryFn: api.agentStatus, staleTime: 0 });
  const s = status.data;
  const saveMode = useMutation({
    mutationFn: (mode: FormatMode) => api.agentSendModeSet(mode),
    onSuccess: (st) => qc.setQueryData(["agentStatus"], st),
    onError: (e) => notify(String(e), "error"),
  });
  const error = status.error;

  return (
    <div>
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

            <dt className="text-muted-foreground">密钥与上限</dt>
            <dd className="text-xs">
              <span className={s.hasKey ? "" : "text-amber-700"}>{s.hasKey ? "Anthropic 密钥已填" : "还没有填 Anthropic 密钥"}</span>
              <span className="text-muted-foreground tabular-nums">
                {" "}
                · 本月已花 {usd(s.monthSpentUsd)} / 上限 {usd(s.budgetUsd)}（和事实调查共用）
              </span>
              <button onClick={onKeys} className="ml-2 text-primary hover:underline">
                去「密钥与费用」
              </button>
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
