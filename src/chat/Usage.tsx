// 输入框下面一行（1.1.0，decisions/0036）：左边是用哪个模型（点开两层：先选一家、再选模型，decisions/0041），右边是本月花了多少的小圈（点开看明细）。
// 花钱只看月上限（decisions/0030）：不逐次确认，这里一直看得见。
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type UsageSummary } from "@/shared/ipc";
import { ModelPicker } from "@/chat/ModelPicker";
import { usePresence } from "@/shared/controls";
import { useNav } from "@/app/nav";
import { money } from "@/research/labels";

/** 一个圈：花了的占上限多少。过了九成换成提醒的颜色 */
export function UsageRing({ spent, budget, size = 18, stroke = 2.2 }: { spent: number; budget: number; size?: number; stroke?: number }) {
  const r = (size - stroke) / 2;
  const f = budget > 0 ? Math.min(1, spent / budget) : 0;
  // pathLength 100：出场时从 0 画到这里（use-ring-draw），之后变了就平滑地走过去
  const off = 100 * (1 - Math.max(f, spent > 0 ? 0.015 : 0));
  return (
    <svg className={`use-ring ${f >= 0.9 ? "is-high" : ""}`} width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <circle className="bg" cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} />
      <circle className="fg" cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} pathLength={100} strokeDasharray="100" strokeDashoffset={off} style={{ "--off": off } as React.CSSProperties} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
    </svg>
  );
}

/** 本月每天花了多少：一排细柱（今天的最深） */
export function DayBars({ days, height = 34 }: { days: number[]; height?: number }) {
  const max = Math.max(...days, 0.0001);
  return (
    <span className="use-days" style={{ height }} aria-hidden="true">
      {days.map((d, i) => (
        <i key={i} className={i === days.length - 1 ? "is-today" : ""} style={{ height: `${Math.max(d > 0 ? 8 : 3, (d / max) * 100)}%`, animationDelay: `${i * 18}ms` }} />
      ))}
    </span>
  );
}

function useClickAway(open: boolean, ref: React.RefObject<HTMLElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const off = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && close();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", off);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", off);
      window.removeEventListener("keydown", esc);
    };
  }, [open, ref, close]);
}

export function UsageLine({ conversation, below }: { conversation?: string | null; below?: boolean }) {
  const nav = useNav();
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["modelStatus"], queryFn: api.modelStatus, staleTime: 30_000 });
  const usage = useQuery({ queryKey: ["usage", conversation ?? null], queryFn: () => api.usageSummary(conversation), staleTime: 5_000 });
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  useClickAway(open, wrap, () => setOpen(false));
  const pop = usePresence(open);
  const s = status.data;
  const u = usage.data;
  if (!s || !u) return <div className="use-line" />;
  const pick = async (model: string | null) => {
    if (!model) return;
    try {
      qc.setQueryData(["modelStatus"], await api.modelPrefsSet({ model, subagent: s.prefs.subagent }));
    } catch (e) {
      nav.say(String(e));
    }
  };

  return (
    <div className={`use-line ${below ? "is-below" : ""}`} ref={wrap}>
      <ModelPicker
        status={s}
        value={s.prefs.model}
        onPick={(id) => void pick(id)}
        variant="line"
        footer={
          <button className="pith-link" onClick={() => nav.openSettings("keys")}>
            每个模型想多深：设置 › 模型与费用 →
          </button>
        }
      />
      <button className={`use-spend ${open ? "is-open" : ""}`} onClick={() => setOpen(!open)} aria-label="本月用量">
        <UsageRing spent={u.spentUsd} budget={u.budgetUsd} />
        <span className="num">{money(u.spentUsd)}</span>
      </button>
      {pop.mounted && <UsagePop u={u} leaving={pop.leaving} onMore={() => (setOpen(false), nav.openSettings("keys"))} />}
    </div>
  );
}

function UsagePop({ u, leaving, onMore }: { u: UsageSummary; leaving: boolean; onMore: () => void }) {
  const left = Math.max(0, u.budgetUsd - u.spentUsd);
  return (
    <div className={`use-pop is-usage ${leaving ? "is-leaving" : ""}`}>
      <div className="use-head">
        <UsageRing spent={u.spentUsd} budget={u.budgetUsd} size={54} stroke={4} />
        <div>
          <b className="num">{money(u.spentUsd)}</b>
          <span>
            本月 · 上限 {money(u.budgetUsd)}，还剩 {money(left)}
          </span>
        </div>
      </div>
      {u.days.length > 0 && <DayBars days={u.days} />}
      {u.ledgerError && <p className="use-warn selectable">{u.ledgerError}</p>}
      {u.mock && <p className="use-note">开发版用模拟模型：不联网、不花钱，账本里记作「模拟」。</p>}
      <dl className="use-kv">
        {u.conversationUsd !== null && (
          <>
            <dt>这段对话</dt>
            <dd className="num">{money(u.conversationUsd)}</dd>
          </>
        )}
        {u.byProvider.map((p) => (
          <Fragment key={p.key} k={p.label} v={money(p.usd)} />
        ))}
        {u.cache.read + u.cache.input > 0 && (
          <>
            <dt>缓存命中</dt>
            <dd className="num">{Math.round(u.cache.hitRate * 100)}%</dd>
          </>
        )}
      </dl>
      <button className="pith-link use-go" onClick={onMore}>
        明细 →
      </button>
    </div>
  );
}

function Fragment({ k, v }: { k: string; v: string }) {
  return (
    <>
      <dt>{k}</dt>
      <dd className="num">{v}</dd>
    </>
  );
}
