// 一次调查：确认计划（删改子问题、加网址或文件）→ 进度（可以停止）→ 短答 + 全过程 + 导出 / 加入文献库 / 重查。
import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { BookPlus, FileDown, FileUp, LoaderCircle, Play, Plus, RefreshCw, Square, Trash2, X } from "lucide-react";
import { api, type ResearchExecutePreview, type ResearchExportFormat, type ResearchPlan, type ResearchProgress, type ResearchRunView, type ResearchSubQuestion } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Badge, Notice, Page, SectionTitle } from "@/format/parts";
import { modelName, when } from "@/format/labels";
import { ExecuteSend } from "@/research/Confirm";
import { DiffView, Process, ShortAnswer } from "@/research/Result";
import { STAGE, STAGES, STATUS, STATUS_TONE, money } from "@/research/labels";

const blankSub = (): ResearchSubQuestion => ({ id: "", text: "", metric: "", definition: "", population: "", region: "", period: "", angle: "alternative", sourceTypes: [], queries: [], enabled: true });

function SubQuestionEditor({ q, onChange, onRemove }: { q: ResearchSubQuestion; onChange: (q: ResearchSubQuestion) => void; onRemove: () => void }) {
  const [text, setText] = useState(q.text);
  const [queries, setQueries] = useState(q.queries.join("\n"));
  useEffect(() => setText(q.text), [q.text]);
  useEffect(() => setQueries(q.queries.join("\n")), [q.queries]);
  const commitText = () => text.trim() !== q.text && onChange({ ...q, text: text.trim() });
  const commitQueries = () => {
    const list = queries.split("\n").map((x) => x.trim()).filter(Boolean);
    if (list.join("\n") !== q.queries.join("\n")) onChange({ ...q, queries: list });
  };
  return (
    <li className={`rounded-md border px-3 py-2 ${q.enabled ? "" : "opacity-50"}`}>
      <div className="flex items-start gap-2">
        <input type="checkbox" className="mt-1.5" checked={q.enabled} onChange={(e) => onChange({ ...q, enabled: e.target.checked })} title="不想查这一项就去掉勾" />
        <span className="mt-1 w-6 shrink-0 font-mono text-xs text-muted-foreground">{q.id || "新"}</span>
        <div className="min-w-0 flex-1 space-y-1.5">
          <textarea
            value={text}
            rows={1}
            onChange={(e) => setText(e.target.value)}
            onBlur={commitText}
            placeholder="子问题"
            className="selectable w-full resize-y rounded border bg-background px-2 py-1 text-sm outline-none focus:border-ring"
          />
          <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            {q.angle === "alternative" ? <Badge tone="accent">官方统计之外</Badge> : <Badge>官方统计</Badge>}
            {[q.metric, q.definition, q.population, q.region, q.period].filter(Boolean).map((x, i) => (
              <span key={i}>{x}</span>
            ))}
          </div>
          <label className="block text-xs">
            <span className="text-muted-foreground">搜索词（一行一个）</span>
            <textarea
              value={queries}
              rows={Math.max(1, Math.min(4, q.queries.length))}
              onChange={(e) => setQueries(e.target.value)}
              onBlur={commitQueries}
              className="selectable mt-0.5 w-full resize-y rounded border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring"
            />
          </label>
        </div>
        <button onClick={onRemove} title="删掉这个子问题" className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-destructive">
          <Trash2 size={13} />
        </button>
      </div>
    </li>
  );
}

function PlanEditor({ view, onView, onStart }: { view: ResearchRunView; onView: (v: ResearchRunView) => void; onStart: () => void }) {
  const { notify } = useUi();
  const run = view.run;
  const [saving, setSaving] = useState(false);
  const [url, setUrl] = useState("");
  const save = async (plan: ResearchPlan) => {
    setSaving(true);
    try {
      onView(await api.researchPlanUpdate(run.uid, plan));
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setSaving(false);
    }
  };
  const subs = run.plan.subQuestions;
  const setSub = (i: number, q: ResearchSubQuestion) => void save({ ...run.plan, subQuestions: subs.map((x, j) => (j === i ? q : x)) });
  const removeSub = (i: number) => void save({ ...run.plan, subQuestions: subs.filter((_, j) => j !== i) });
  const [draft, setDraft] = useState("");
  const addSub = () => {
    if (!draft.trim()) return;
    void save({ ...run.plan, subQuestions: [...subs, { ...blankSub(), text: draft.trim(), queries: [draft.trim()] }] });
    setDraft("");
  };
  const addSource = async (value: string | null) => {
    if (!value?.trim()) return;
    try {
      onView(await api.researchSourceAdd(run.uid, value.trim()));
      setUrl("");
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const removeSource = async (key: string) => {
    try {
      onView(await api.researchSourceRemove(run.uid, key));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const manual = run.sources.filter((s) => s.origin === "manual");
  const enabled = subs.filter((s) => s.enabled).length;

  return (
    <div className="space-y-5">
      <div className="space-y-2 text-xs">
        {run.plan.summary && <p className="text-sm">{run.plan.summary}</p>}
        {run.plan.problems.length > 0 && <Notice title="读回计划时改正、丢掉的地方" items={run.plan.problems} />}
        {run.plan.dimensions.length > 0 && <Notice tone="info" title="官方统计之外的衡量维度" items={run.plan.dimensions.map((d) => `${d.name}：${d.why}${d.sources ? `（${d.sources}）` : ""}`)} />}
        {run.plan.caveats.length > 0 && <Notice title="比较时要小心的地方" items={run.plan.caveats} />}
      </div>

      <div>
        <SectionTitle right={saving ? <LoaderCircle size={13} className="animate-spin text-muted-foreground" /> : <span className="text-xs text-muted-foreground">{enabled} 个要查</span>}>子问题：可以删改、去掉勾、加新的</SectionTitle>
        <ul className="space-y-2">
          {subs.map((q, i) => (
            <SubQuestionEditor key={`${q.id}-${i}`} q={q} onChange={(n) => setSub(i, n)} onRemove={() => removeSub(i)} />
          ))}
        </ul>
        <form
          className="mt-2 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            addSub();
          }}
        >
          <input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="再加一个子问题（会直接拿它当搜索词，可以再改）" className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1 text-xs outline-none focus:border-ring" />
          <button type="submit" disabled={!draft.trim() || saving} className="flex shrink-0 items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
            <Plus size={12} />
            加子问题
          </button>
        </form>
      </div>

      <div>
        <SectionTitle>自己加的来源（可选）</SectionTitle>
        <p className="mb-2 text-xs text-muted-foreground">知道哪份统计表、报告、网页有用，可以直接加进来，和搜到的一起读。PDF、Excel、CSV、网页、Word、文本都行。</p>
        {manual.length > 0 && (
          <ul className="mb-2 divide-y rounded-md border text-xs">
            {manual.map((s) => (
              <li key={s.id} className="flex items-center gap-2 px-3 py-1.5">
                <span className="font-mono text-muted-foreground">{s.id}</span>
                <span className="selectable min-w-0 flex-1 truncate" title={s.url}>
                  {s.url}
                </span>
                <button onClick={() => void removeSource(s.id)} className="rounded p-0.5 text-muted-foreground hover:bg-muted" title="去掉">
                  <X size={12} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void addSource(url);
          }}
        >
          <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring" />
          <button type="submit" disabled={!url.trim()} className="shrink-0 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
            加网址
          </button>
          <button type="button" onClick={async () => addSource(await api.pickResearchFile())} className="flex shrink-0 items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted">
            <FileUp size={12} />
            加文件…
          </button>
        </form>
      </div>

      <div className="flex justify-end">
        <button onClick={onStart} disabled={enabled === 0 || saving} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground disabled:opacity-50">
          <Play size={13} />
          确认计划，开始调查…
        </button>
      </div>
    </div>
  );
}

function ProgressPanel({ uid }: { uid: string }) {
  const [p, setP] = useState<ResearchProgress | null>(null);
  const [stopping, setStopping] = useState(false);
  useEffect(() => {
    const un = listen<ResearchProgress>("research-progress", (e) => e.payload.uid === uid && setP(e.payload));
    return () => void un.then((f) => f());
  }, [uid]);
  const at = p ? STAGES.indexOf(p.stage) : 1;
  return (
    <div className="space-y-3 rounded-md border px-4 py-3 text-xs">
      <ol className="flex flex-wrap items-center gap-1">
        {STAGES.map((s, i) => (
          <li key={s} className="flex items-center gap-1">
            <span className={`rounded px-1.5 py-0.5 ${i < at ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300" : i === at ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"}`}>{STAGE[s]}</span>
            {i < STAGES.length - 1 && <span className="text-muted-foreground">›</span>}
          </li>
        ))}
      </ol>
      <div className="flex items-center gap-2">
        <LoaderCircle size={14} className="shrink-0 animate-spin text-muted-foreground" />
        <span className="selectable min-w-0 flex-1 truncate">{stopping ? "正在停止（做完手上这一步）…" : (p?.message ?? "正在开始…")}</span>
        {p && p.total > 0 && (
          <span className="shrink-0 text-muted-foreground tabular-nums">
            {Math.min(p.done + 1, p.total)} / {p.total}
          </span>
        )}
        <button
          onClick={() => {
            setStopping(true);
            void api.researchCancel();
          }}
          disabled={stopping}
          className="flex shrink-0 items-center gap-1 rounded-md border px-2.5 py-1 hover:bg-muted disabled:opacity-50"
        >
          <Square size={11} />
          停止
        </button>
      </div>
      {p && p.total > 0 && (
        <div className="h-1.5 overflow-hidden rounded bg-muted">
          <div className="h-full bg-primary transition-all" style={{ width: `${Math.min(100, ((p.done + 1) / p.total) * 100)}%` }} />
        </div>
      )}
      <p className="text-muted-foreground">每做完一步都存下来：停止、关掉 App 之后可以接着做，做过的不重做。</p>
    </div>
  );
}

function Actions({ view, selected, onView, onOpen }: { view: ResearchRunView; selected: string[]; onView: (v: ResearchRunView) => void; onOpen: (uid: string) => void }) {
  const { notify } = useUi();
  const qc = useQueryClient();
  const uid = view.run.uid;
  const [busy, setBusy] = useState<string | null>(null);
  const exportAs = async (format: ResearchExportFormat) => {
    const dir = await api.pickFolder("导出到哪个文件夹");
    if (!dir) return;
    setBusy(format);
    try {
      const path = await api.researchExport(uid, format, dir);
      notify(`已导出：${path.split("/").pop()}`, "info", { label: "在访达中显示", onClick: () => void api.researchReveal(path) });
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const addToLibrary = async () => {
    setBusy("library");
    try {
      const r = await api.researchAddToLibrary(uid, selected);
      onView(r.view);
      qc.invalidateQueries({ queryKey: ["overview"] });
      qc.invalidateQueries({ queryKey: ["items"] });
      const created = r.added.filter((a) => a.created).length;
      const missing = r.added.filter((a) => a.attachmentError).length;
      notify(`${created} 个来源加入了文献库${r.added.length - created > 0 ? `，${r.added.length - created} 个已经在库里` : ""}${missing > 0 ? `；${missing} 个的原件没能作为附件` : ""}`);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const rerun = async () => {
    setBusy("rerun");
    try {
      const v = await api.researchRerun(uid);
      qc.invalidateQueries({ queryKey: ["researchQuestions"] });
      onOpen(v.run.uid);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const spin = (k: string) => busy === k && <LoaderCircle size={12} className="animate-spin" />;
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <span className="text-muted-foreground">导出：</span>
      {(
        [
          ["markdown", "Markdown"],
          ["html", "单文件 HTML"],
          ["docx", "Word"],
        ] as [ResearchExportFormat, string][]
      ).map(([f, label]) => (
        <button key={f} onClick={() => void exportAs(f)} disabled={busy !== null} className="flex items-center gap-1 rounded-md border px-2.5 py-1 hover:bg-muted disabled:opacity-50">
          {spin(f) || <FileDown size={12} />}
          {label}
        </button>
      ))}
      <span className="mx-1 h-4 w-px bg-border" />
      <button onClick={() => void addToLibrary()} disabled={busy !== null || selected.length === 0} title={selected.length === 0 ? "先在下面「来源」里勾上要加的" : undefined} className="flex items-center gap-1 rounded-md border px-2.5 py-1 hover:bg-muted disabled:opacity-50">
        {spin("library") || <BookPlus size={12} />}
        把勾上的 {selected.length} 个来源加入文献库
      </button>
      <button onClick={() => void rerun()} disabled={busy !== null} className="flex items-center gap-1 rounded-md border px-2.5 py-1 hover:bg-muted disabled:opacity-50" title="同一个问题再查一次，标出哪些数字、来源和结论变了">
        {spin("rerun") || <RefreshCw size={12} />}
        重查
      </button>
    </div>
  );
}

export function RunScreen({ uid, onRoot, onOpen, onSettings }: { uid: string; onRoot: () => void; onOpen: (uid: string) => void; onSettings: () => void }) {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const q = useQuery({ queryKey: ["researchRun", uid], queryFn: () => api.researchRunGet(uid), staleTime: 0 });
  const setView = (v: ResearchRunView) => qc.setQueryData(["researchRun", uid], v);
  const [preview, setPreview] = useState<ResearchExecutePreview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const view = q.data;

  // 后台做完（或停下）：刷新
  useEffect(() => {
    const un = listen<{ uid: string; status: string | null; error: string | null }>("research-done", (e) => {
      if (e.payload.uid !== uid) return;
      qc.invalidateQueries({ queryKey: ["researchRun", uid] });
      qc.invalidateQueries({ queryKey: ["researchQuestions"] });
      qc.invalidateQueries({ queryKey: ["researchStatus"] });
      if (e.payload.error) notify(e.payload.error, "error");
    });
    return () => void un.then((f) => f());
  }, [uid, qc, notify]);

  // 默认勾上有已核实事实、还没进文献库的来源
  const defaults = useMemo(() => (view ? view.report.sources.filter((s) => s.verified > 0 && s.itemId === null).map((s) => s.id) : []), [view]);
  useEffect(() => setSelected(defaults), [defaults]);

  const prepare = async () => {
    try {
      setPreview(await api.researchExecutePrepare(uid));
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const remove = async () => {
    if (!view) return;
    const ok = await confirm({ title: `删除第 ${view.run.seq} 次调查？`, message: "调查记录和只有它用到的网页快照会删掉。已经加入文献库的来源不受影响。", confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.researchRunDelete(uid);
      qc.invalidateQueries({ queryKey: ["researchQuestions"] });
      onRoot();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const title = view ? (view.run.question.length > 24 ? `${view.run.question.slice(0, 24)}…` : view.run.question) : "…";
  const run = view?.run;
  const running = view?.running || run?.status === "running";
  return (
    <Page
      crumbs={[{ label: "调查", onClick: onRoot }, { label: run ? `${title}（第 ${run.seq} 次）` : title }]}
      actions={
        run && !running ? (
          <button onClick={() => void remove()} className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-destructive" title="删除这次调查">
            <Trash2 size={14} />
          </button>
        ) : undefined
      }
    >
      {q.error && <p className="selectable text-xs text-destructive">{String(q.error)}</p>}
      {!view && !q.error && <LoaderCircle size={16} className="animate-spin text-muted-foreground" />}
      {view && run && (
        <>
          <div className="space-y-1">
            <p className="selectable text-base leading-relaxed font-medium">{run.question}</p>
            <p className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <Badge tone={STATUS_TONE[running ? "running" : run.status]}>{running ? "进行中" : STATUS[run.status]}</Badge>
              {run.mock && (
                <Badge tone="warn" title="模拟接口给的示例：不联网、不花钱，数字是虚构的">
                  模拟
                </Badge>
              )}
              <span>{when(run.createdAt)}</span>
              {run.config.planner.provider !== "mock" && (
                <span>
                  {modelName(run.config.planner.model)} 定计划、下结论 · {run.config.worker.model} 摘证据
                </span>
              )}
              {!run.mock && view.report.totals.costUsd > 0 && <span className="tabular-nums">费用 {money(view.report.totals.costUsd)}</span>}
            </p>
          </div>

          {running ? (
            <ProgressPanel uid={uid} />
          ) : view.planEditable ? (
            <>
              <Notice tone="info" title="先看一下计划：子问题可以删改、去掉勾、加新的，也可以自己加网址或文件。确认以后才开始检索、下载、花钱。" />
              <PlanEditor view={view} onView={setView} onStart={() => void prepare()} />
            </>
          ) : run.status === "cancelled" || run.status === "failed" ? (
            <Notice tone={run.status === "failed" ? "danger" : "warn"} title={run.status === "failed" ? `停在了「${STAGE[run.stage]}」：${run.error ?? "出错了"}` : `停在了「${STAGE[run.stage]}」。`}>
              做过的都存着。处理好以后可以接着做（不重做已经做过的）。
              <span className="mt-2 flex gap-2">
                <button onClick={() => void prepare()} className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-primary-foreground">
                  <Play size={12} />
                  接着做…
                </button>
                <button onClick={onSettings} className="rounded-md border px-2.5 py-1 hover:bg-muted">
                  去设置
                </button>
              </span>
            </Notice>
          ) : null}

          {run.status === "done" && !running && (
            <>
              {view.findings.length > 0 && <Notice tone="danger" title="从头再核一遍时发现对不上的地方（存下来的结果或快照可能被改过）" items={view.findings.map((f) => f.message)} />}
              <div>
                <SectionTitle>短答</SectionTitle>
                <ShortAnswer report={view.report} />
              </div>
              <DiffView report={view.report} />
              <Actions view={view} selected={selected} onView={setView} onOpen={onOpen} />
              <div>
                <SectionTitle>全过程</SectionTitle>
                <Process report={view.report} selected={selected} onSelect={setSelected} />
              </div>
            </>
          )}
        </>
      )}
      {preview && (
        <ExecuteSend
          preview={preview}
          onSettings={onSettings}
          onClose={() => setPreview(null)}
          onStarted={() => {
            setPreview(null);
            qc.invalidateQueries({ queryKey: ["researchRun", uid] });
          }}
        />
      )}
    </Page>
  );
}
