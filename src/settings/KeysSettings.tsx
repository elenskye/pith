// 设置 › 密钥与费用（decisions/0023）：三把密钥（Anthropic 的排版 Agent 和事实调查共用）、月度上限、本月已花。
// 以前分在「排版 Agent」「事实调查」两处，现在放一起：上限本来就是所有接口合计。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type ResearchStatus } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Notice } from "@/format/parts";
import { money } from "@/research/labels";
import { KeyRow } from "@/settings/ResearchSettings";

export function KeysSettingsSection() {
  const qc = useQueryClient();
  const { notify } = useUi();
  const status = useQuery({ queryKey: ["researchStatus"], queryFn: api.researchStatus, staleTime: 0 });
  const s = status.data;
  const [budget, setBudget] = useState("");
  useEffect(() => {
    if (s) setBudget(String(s.budgetUsd));
  }, [s?.budgetUsd]);

  // 密钥变了：排版 Agent 那边的状态也跟着刷新（Anthropic 那把两边共用）
  const put = (next: ResearchStatus) => {
    qc.setQueryData(["researchStatus"], next);
    qc.invalidateQueries({ queryKey: ["agentStatus"] });
  };
  const saveBudget = async () => {
    const n = Number(budget);
    if (budget.trim() === "" || !Number.isFinite(n)) return notify("月度上限写一个 1 到 50 之间的数", "error");
    try {
      await api.agentBudgetSet(n);
      qc.invalidateQueries({ queryKey: ["agentStatus"] });
      await status.refetch();
      notify("月度上限已保存");
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const pct = s && s.budgetUsd > 0 ? Math.min(100, (s.monthSpentUsd / s.budgetUsd) * 100) : 0;
  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground">
        排版 Agent 和事实调查用的 API 密钥，和两边共用的月度上限。正式版把密钥存在 macOS 钥匙串，保存后这里不再显示；每次发送前按最坏情况估计，加上本月已花的会超过上限就不发。
      </p>
      {status.isLoading && <LoaderCircle size={14} className="animate-spin text-muted-foreground" />}
      {status.error && <p className="selectable text-xs text-destructive">{String(status.error)}</p>}
      {s && (
        <>
          {s.mock && <Notice tone="info" title="开发版和测试用模拟接口：不联网、不花钱。下面的密钥只在换成真接口时用到。" />}
          {s.ledgerError && (
            <Notice tone="danger" title="用量账本读不了，为了不超支，现在发不了请求。">
              {s.ledgerError}
            </Notice>
          )}
          <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-x-3 gap-y-3 text-sm">
            {s.keys.map((k) => (
              <KeyRow key={k.which} k={k} onStatus={put} />
            ))}

            <dt className="pt-1 text-muted-foreground">月度上限</dt>
            <dd className="text-xs">
              <form
                className="flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  void saveBudget();
                }}
              >
                <span className="text-muted-foreground">$</span>
                <input type="number" min={1} max={50} step={1} value={budget} onChange={(e) => setBudget(e.target.value)} className="selectable w-20 rounded-md border bg-background px-2 py-1 tabular-nums outline-none focus:border-ring" />
                <button type="submit" disabled={Number(budget) === s.budgetUsd} className="rounded-md border px-2.5 py-1 hover:bg-muted disabled:opacity-50">
                  保存
                </button>
                <span className="text-muted-foreground">$1 到 $50，所有接口合计</span>
              </form>
            </dd>

            <dt className="text-muted-foreground">本月已花</dt>
            <dd>
              <p className="tabular-nums">
                {money(s.monthSpentUsd)} <span className="text-muted-foreground">/ 上限 {money(s.budgetUsd)}</span>
                <span className="ml-2 text-xs text-muted-foreground">{s.month}</span>
              </p>
              <div className="mt-1.5 h-1.5 max-w-xs overflow-hidden rounded bg-muted">
                <div className={`h-full ${pct >= 90 ? "bg-destructive" : "bg-primary"}`} style={{ width: `${pct}%` }} />
              </div>
              <p className="mt-1 text-xs text-muted-foreground">每一笔在「排版 Agent」「事实调查」的「最近的调用」里看。</p>
            </dd>
          </dl>
        </>
      )}
    </div>
  );
}
