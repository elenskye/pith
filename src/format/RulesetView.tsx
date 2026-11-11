// 一套规范：模型提炼的要求清单（逐条确认：可关掉、可改值）、由清单生成的规范、这套规范下排过的论文。
import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, FileText, LoaderCircle, Plus, Trash2, TriangleAlert, X } from "lucide-react";
import {
  api,
  type FormatHandling,
  type FormatPlanPreview,
  type FormatRequirement,
  type FormatRulesetView,
  type FormatRunSummary,
  type FormatSetting,
  type FormatSettingPatch,
} from "@/shared/ipc";
import { EditableText, useUi } from "@/shared/ui";
import { CERTAINTY, CERTAINTY_HINT, CHECK_KIND, count, HANDLING, HANDLING_HINT, HANDLING_ORDER, MODE, propLabel, RUN_STATE, targetLabel, usd, valueChoices, when } from "@/format/labels";
import { Badge, CommitInput, Notice, Page, SectionTitle, Switch } from "@/format/parts";
import { PlanSend } from "@/format/SendConfirm";

type Tab = "requirements" | "spec" | "runs";

export function RulesetView({ id, onBack, onRun, onSettings }: { id: string; onBack: () => void; onRun: (runId: string) => void; onSettings: () => void }) {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const key = ["formatRuleset", id];
  const q = useQuery({ queryKey: key, queryFn: () => api.formatRulesetGet(id) });
  const [tab, setTab] = useState<Tab>("requirements");
  const [preview, setPreview] = useState<FormatPlanPreview | null>(null);
  const v = q.data;
  const put = (next: FormatRulesetView) => qc.setQueryData(key, next);
  const refreshList = () => qc.invalidateQueries({ queryKey: ["formatRulesets"] });

  const rename = async (name: string) => {
    try {
      put(await api.formatRulesetRename(id, name));
      refreshList();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const remove = async () => {
    if (!v) return;
    const ok = await confirm({
      title: `删除规范「${v.name}」？`,
      message: `要求清单和这套规范下 ${v.runs.length} 份论文的计划、报告存档会一起删掉；重新提炼要再发一次、再花一次钱。已经写到论文旁边的排版结果和报告文件不动。`,
      confirm: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.formatRulesetDelete(id);
      refreshList();
      qc.removeQueries({ queryKey: key });
      onBack();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  // 选好论文就读入、算出要发什么（不联网），再交给发送确认框。发送方式用设置里的默认值
  const pickThesis = async () => {
    const path = await api.pickThesis();
    if (!path) return;
    try {
      setPreview(await api.formatPlanPrepare(id, path));
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const removeRun = async (r: FormatRunSummary) => {
    const ok = await confirm({ title: `删除「${r.thesisName}」的计划？`, message: "计划和报告的存档会删掉。论文原文件、已经写出的排版结果不动。", confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.formatRunDelete(id, r.id);
      qc.invalidateQueries({ queryKey: key });
      refreshList();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const crumbs = [{ label: "排版", onClick: onBack }, { label: v?.name ?? "规范" }];
  if (q.error) {
    return (
      <Page crumbs={crumbs}>
        <p className="selectable text-xs text-destructive">{String(q.error)}</p>
      </Page>
    );
  }
  if (!v) {
    return (
      <Page crumbs={crumbs}>
        <LoaderCircle size={14} className="animate-spin text-muted-foreground" />
      </Page>
    );
  }

  const autoOn = v.requirements.filter((r) => r.handling === "auto" && r.enabled).length;
  const tabs: [Tab, string][] = [
    ["requirements", `要求清单 ${v.requirements.length}`],
    ["spec", `生成的规范 ${v.assembled.lines.length}`],
    ["runs", `论文 ${v.runs.length}`],
  ];

  return (
    <Page
      crumbs={crumbs}
      actions={
        <>
          <button onClick={remove} title="删除这套规范" className="rounded-md border p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground">
            <Trash2 size={13} />
          </button>
          <button onClick={pickThesis} className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground">
            <FileText size={13} />
            排一份论文…
          </button>
        </>
      }
    >
      <div>
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1 text-base font-semibold">
            <EditableText value={v.name} onSave={rename} />
          </div>
          {v.mock && (
            <Badge tone="warn" title="模拟模型给的示例，不是真的提炼结果">
              模拟
            </Badge>
          )}
        </div>
        <dl className="mt-2 grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          <dt className="text-muted-foreground">文档</dt>
          <dd>
            {v.docs.map((d) => (
              <p key={d.label} className="selectable break-all">
                <span className="mr-1.5 font-mono text-muted-foreground">{d.label}</span>
                {d.name}
                <span className="ml-1.5 text-muted-foreground tabular-nums">{count(d.chars)} 字</span>
              </p>
            ))}
          </dd>
          <dt className="text-muted-foreground">提炼</dt>
          <dd className="tabular-nums">
            {when(v.createdAt)}
            <span className="ml-2 text-muted-foreground">
              {v.mock ? "模拟，没有花钱" : v.call ? `实际费用 ${usd(v.call.costUsd)}` : ""}
              {v.call?.note ? `（${v.call.note}）` : ""}
            </span>
          </dd>
        </dl>
      </div>

      {v.problems.length > 0 && <Notice title="读模型的回答时改正、丢掉了这些地方：" items={v.problems} />}

      <div>
        <div className="mb-4 flex gap-1 border-b">
          {tabs.map(([t, label]) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`-mb-px border-b-2 px-3 py-1.5 text-xs tabular-nums ${tab === t ? "border-primary font-medium" : "border-transparent text-muted-foreground hover:text-foreground"}`}
            >
              {label}
            </button>
          ))}
        </div>
        {tab === "requirements" && <Requirements v={v} onChange={put} />}
        {tab === "spec" && <Assembled v={v} autoOn={autoOn} />}
        {tab === "runs" && <Runs runs={v.runs} onOpen={onRun} onRemove={removeRun} onNew={pickThesis} />}
      </div>

      {preview && (
        <PlanSend
          preview={preview}
          onSettings={onSettings}
          onClose={() => setPreview(null)}
          onDone={(run) => {
            setPreview(null);
            qc.setQueryData(["formatRun", id, run.id], run);
            qc.invalidateQueries({ queryKey: key });
            refreshList();
            notify(run.mock ? "计划已生成（模拟模型，没有花钱）" : `计划已生成，实际费用 ${usd(run.call?.costUsd ?? 0)}`);
            onRun(run.id);
          }}
        />
      )}
    </Page>
  );
}

// ---------------- 要求清单 ----------------

function Requirements({ v, onChange }: { v: FormatRulesetView; onChange: (next: FormatRulesetView) => void }) {
  const [filter, setFilter] = useState<FormatHandling | null>(null);
  const counts = useMemo(() => {
    const m = new Map<FormatHandling, number>();
    for (const r of v.requirements) m.set(r.handling, (m.get(r.handling) ?? 0) + 1);
    return m;
  }, [v.requirements]);
  // 按清单里的顺序分组
  const groups = useMemo(() => {
    const out: { name: string; items: FormatRequirement[] }[] = [];
    for (const r of v.requirements) {
      if (filter && r.handling !== filter) continue;
      const g = out.find((x) => x.name === r.group);
      if (g) g.items.push(r);
      else out.push({ name: r.group, items: [r] });
    }
    return out;
  }, [v.requirements, filter]);
  const unverified = v.requirements.reduce((n, r) => n + r.sources.filter((s) => !s.verified).length, 0);

  const chip = (value: FormatHandling | null, label: string, n: number, title?: string) => (
    <button
      key={label}
      onClick={() => setFilter(value)}
      title={title}
      className={`rounded-full px-2.5 py-0.5 text-xs tabular-nums ${filter === value ? "bg-primary text-primary-foreground" : "bg-muted hover:bg-accent"}`}
    >
      {label} {n}
    </button>
  );

  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground">
        请对照原文逐条确认。只有「自动」的要求会进规范、由排版程序执行：不想应用的关掉，值不对的直接改。其余几类只列出来：「待做」程序还不会做，「只检查」排完后在报告里给结果，「不动」程序不碰。
      </p>
      {unverified > 0 && <Notice title={`有 ${unverified} 处出处的摘录在原文里没找到，可能是模型记错或编的。这几条请特别留意。`} />}
      <div className="flex flex-wrap gap-1.5">
        {chip(null, "全部", v.requirements.length)}
        {HANDLING_ORDER.filter((h) => counts.has(h)).map((h) => chip(h, HANDLING[h], counts.get(h) ?? 0, HANDLING_HINT[h]))}
      </div>
      {groups.map((g) => (
        <div key={g.name}>
          <SectionTitle>{g.name || "未分组"}</SectionTitle>
          <ul className="divide-y rounded-md border">
            {g.items.map((r) => (
              <RequirementRow key={r.id} rulesetId={v.id} r={r} onChange={onChange} />
            ))}
          </ul>
        </div>
      ))}
      {groups.length === 0 && <p className="text-xs text-muted-foreground">没有这一类的要求。</p>}
    </div>
  );
}

function RequirementRow({ rulesetId, r, onChange }: { rulesetId: string; r: FormatRequirement; onChange: (next: FormatRulesetView) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 保存失败时加一：输入框重建，回到原值
  const [rev, setRev] = useState(0);
  const auto = r.handling === "auto";
  const off = auto && !r.enabled;

  const save = async (patch: { enabled?: boolean; settings?: FormatSettingPatch[] }) => {
    setBusy(true);
    setError(null);
    try {
      onChange(await api.formatRequirementSet(rulesetId, r.id, patch));
    } catch (e) {
      setError(String(e));
      setRev((n) => n + 1);
    } finally {
      setBusy(false);
    }
  };
  const plain = (s: FormatSetting): FormatSettingPatch => ({ target: s.target, prop: s.prop, value: s.value });
  const setValue = (index: number, value: string) => save({ settings: r.settings.map((s, i) => (i === index ? { ...plain(s), value } : plain(s))) });
  const dropSetting = (index: number) => save({ settings: r.settings.filter((_, i) => i !== index).map(plain) });

  return (
    <li className="flex gap-3 px-3 py-2.5">
      <div className="flex w-12 shrink-0 flex-col items-start gap-1.5 pt-0.5">
        <span className="font-mono text-xs text-muted-foreground">{r.id}</span>
        {auto && <Switch on={r.enabled} disabled={busy} onChange={(enabled) => save({ enabled })} title={r.enabled ? "关掉：这条要求不进规范" : "打开：应用这条要求"} />}
      </div>
      <div className="min-w-0 flex-1 space-y-1.5">
        <div className="flex items-start gap-2">
          <p className={`selectable min-w-0 flex-1 ${off ? "text-muted-foreground line-through" : ""}`}>{r.text}</p>
          <div className="flex shrink-0 flex-wrap justify-end gap-1">
            <Badge tone={r.certainty === "inferred" || r.certainty === "unspecified" ? "warn" : r.certainty === "template" ? "accent" : "plain"} title={CERTAINTY_HINT[r.certainty]}>
              {CERTAINTY[r.certainty]}
            </Badge>
            <Badge tone={auto ? "ok" : "plain"} title={HANDLING_HINT[r.handling]}>
              {HANDLING[r.handling]}
            </Badge>
          </div>
        </div>

        {r.sources.map((s, i) => (
          <p key={i} className="selectable text-xs text-muted-foreground">
            <span className="font-mono">{s.doc}</span>
            {s.section && <span> · {s.section}</span>}
            <span className="text-foreground/80">「{s.quote}」</span>
            {s.verified ? (
              <span className="ml-1 inline-flex items-center gap-0.5 whitespace-nowrap text-emerald-700 dark:text-emerald-400">
                <CircleCheck size={11} />
                已在原文核对
              </span>
            ) : (
              <span className="ml-1 inline-flex items-center gap-0.5 rounded bg-destructive/10 px-1 font-medium whitespace-nowrap text-destructive">
                <TriangleAlert size={11} />
                原文里没找到这句
              </span>
            )}
          </p>
        ))}
        {r.sources.length === 0 && (r.certainty === "explicit" || r.certainty === "template") && <p className="text-xs text-destructive">模型没有给出处</p>}

        {r.note && <p className="selectable text-xs text-muted-foreground">注：{r.note}</p>}

        {r.handling === "check" && (
          <p className="text-xs text-muted-foreground">
            {r.check ? `排版后由程序统计：${CHECK_KIND[r.check.kind] ?? r.check.kind}${range(r.check.min, r.check.max)}` : "程序不会检查这一项，报告里标「需人工核对」"}
          </p>
        )}

        {r.settings.length > 0 && (
          <ul className={`space-y-1 pt-0.5 ${off ? "opacity-50" : ""}`}>
            {r.settings.map((s, i) => (
              <li key={`${i}-${rev}`} className="text-xs">
                <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
                  <span className="text-muted-foreground">
                    {targetLabel(s.target)} · {propLabel(s.prop)} =
                  </span>
                  {auto ? <SettingValue s={s} disabled={busy} onCommit={(value) => setValue(i, value)} /> : <span className="selectable">{s.value}</span>}
                  {s.error && auto && r.settings.length > 1 && (
                    <button onClick={() => dropSetting(i)} disabled={busy} title="去掉这个设置" className="rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground">
                      <X size={12} />
                    </button>
                  )}
                </div>
                {s.error && <p className="selectable mt-0.5 text-destructive">读不懂这个值，没有进规范：{s.error}</p>}
              </li>
            ))}
          </ul>
        )}
        {error && <p className="selectable text-xs text-destructive">没有保存：{error}</p>}
      </div>
    </li>
  );
}

function range(min: number | null, max: number | null): string {
  if (min !== null && max !== null) return `（${min}–${max}）`;
  if (min !== null) return `（不少于 ${min}）`;
  if (max !== null) return `（不超过 ${max}）`;
  return "";
}

/** 一个设置的值：固定几种取值的用下拉，其余是输入框（回车或离开时保存） */
function SettingValue({ s, disabled, onCommit }: { s: FormatSetting; disabled: boolean; onCommit: (v: string) => void }) {
  const choices = valueChoices(s.target, s.prop);
  const bad = s.error ? "border-destructive text-destructive" : "";
  if (choices?.some((c) => c[0] === s.value.toLowerCase())) {
    return (
      <select value={s.value.toLowerCase()} disabled={disabled} onChange={(e) => onCommit(e.target.value)} className={`rounded border bg-background px-1 py-0.5 outline-none focus:border-ring ${bad}`}>
        {choices.map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
    );
  }
  return <CommitInput value={s.value} disabled={disabled} onCommit={onCommit} className={`w-36 ${bad}`} />;
}

// ---------------- 生成的规范 ----------------

function Assembled({ v, autoOn }: { v: FormatRulesetView; autoOn: number }) {
  const a = v.assembled;
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        由清单里开着的 {autoOn} 条「自动」要求合成，排版程序照这张表设置；表里没有的属性不改，保留原文档的样子。值已经由程序换算（如小四 → 12 磅）。标着「排版程序的默认做法」的不是规范的要求：规范没规定、但程序清掉手动格式后必须有个确定值的地方，按固定规矩补的。
      </p>
      {a.problems.length > 0 && <Notice title="合成规范时的问题：" items={a.problems} />}
      {a.lines.length === 0 ? (
        <p className="text-xs text-muted-foreground">没有能自动应用的设置（都关掉了，或值读不懂），现在排不了。</p>
      ) : (
        <table className="w-full text-left text-xs">
          <thead className="text-muted-foreground">
            <tr className="border-b">
              <th className="py-1.5 pr-3 font-normal">对象</th>
              <th className="py-1.5 pr-3 font-normal">属性</th>
              <th className="py-1.5 pr-3 font-normal">值</th>
              <th className="py-1.5 font-normal">来自</th>
            </tr>
          </thead>
          <tbody>
            {a.lines.map((l, i) => {
              const sameTarget = i > 0 && a.lines[i - 1].target === l.target;
              return (
                <tr key={`${l.target}.${l.prop}`} className={`align-top ${sameTarget ? "" : "border-t border-border/60"}`}>
                  <td className="py-1 pr-3 whitespace-nowrap">{sameTarget ? "" : l.targetLabel}</td>
                  <td className="py-1 pr-3 text-muted-foreground">{l.propLabel}</td>
                  <td className="selectable py-1 pr-3 break-all">{l.value}</td>
                  <td className="py-1">
                    {l.requirement === null ? (
                      <span className="text-muted-foreground">排版程序的默认做法{l.note ? `：${l.note}` : ""}</span>
                    ) : (
                      <span className="flex flex-wrap items-center gap-1">
                        <span className="font-mono text-muted-foreground">{l.requirement}</span>
                        {l.certainty && l.certainty !== "explicit" && (
                          <Badge tone={l.certainty === "template" ? "accent" : "warn"} title={CERTAINTY_HINT[l.certainty]}>
                            {CERTAINTY[l.certainty]}
                          </Badge>
                        )}
                      </span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}

// ---------------- 论文 ----------------

function Runs({ runs, onOpen, onRemove, onNew }: { runs: FormatRunSummary[]; onOpen: (id: string) => void; onRemove: (r: FormatRunSummary) => void; onNew: () => void }) {
  if (runs.length === 0) {
    return (
      <div className="rounded-md border border-dashed px-4 py-6 text-center text-xs text-muted-foreground">
        <p>还没有用这套规范排过论文。</p>
        <button onClick={onNew} className="mt-2 inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-foreground hover:bg-muted">
          <Plus size={13} />
          排一份论文…
        </button>
      </div>
    );
  }
  return (
    <ul className="divide-y rounded-md border">
      {runs.map((r) => (
        <li key={r.id} className="group flex items-center gap-3 px-3 py-2">
          <button onClick={() => onOpen(r.id)} className="min-w-0 flex-1 text-left">
            <p className="truncate hover:underline" title={r.thesisPath}>
              {r.thesisName}
            </p>
            <p className="truncate text-xs text-muted-foreground tabular-nums">
              {when(r.createdAt)} · {MODE[r.mode]} · {r.mock ? "模拟" : usd(r.costUsd)}
            </p>
          </button>
          <Badge tone={r.state === "delivered" ? "ok" : r.state === "blocked" ? "danger" : "plain"}>{RUN_STATE[r.state] ?? r.state}</Badge>
          <button onClick={() => onRemove(r)} title="删除这份计划" className="shrink-0 rounded p-1 text-muted-foreground opacity-0 group-hover:opacity-100 hover:bg-muted focus:opacity-100">
            <Trash2 size={13} />
          </button>
        </li>
      ))}
    </ul>
  );
}
