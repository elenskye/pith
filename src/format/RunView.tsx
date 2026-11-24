// 一份论文的文档计划：模型给的样式映射、文字规则、受保护的分节、分区与表格（都可以改），确认后确定性排版，看结果与报告。
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, FolderOpen, LoaderCircle, Play, Plus, Trash2, X } from "lucide-react";
import { api, type FormatAnchor, type FormatDraft, type FormatHit, type FormatRule, type FormatRunResult, type FormatRunView, type FormatStyleMapping } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { baseName, count, MODE, ROLES, usd, when } from "@/format/labels";
import { Badge, CommitInput, Markdown, Notice, Page, SectionTitle } from "@/format/parts";

export function RunView({ rulesetId, runId, onRoot, onBack }: { rulesetId: string; runId: string; onRoot: () => void; onBack: () => void }) {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const key = ["formatRun", rulesetId, runId];
  const q = useQuery({ queryKey: key, queryFn: () => api.formatRunGet(rulesetId, runId) });
  const ruleset = useQuery({ queryKey: ["formatRuleset", rulesetId], queryFn: () => api.formatRulesetGet(rulesetId) });
  const [saving, setSaving] = useState(false);
  // 保存失败时加一：输入框重建，回到原值
  const [rev, setRev] = useState(0);
  const refreshRuleset = () => qc.invalidateQueries({ queryKey: ["formatRuleset", rulesetId] });
  const apply = useMutation({
    mutationFn: () => api.formatApply(rulesetId, runId),
    onSuccess: (v) => {
      qc.setQueryData(key, v);
      refreshRuleset();
    },
  });
  const reveal = useMutation({ mutationFn: api.wordRevealFile });
  const v = q.data;

  /** 改计划：整个 draft 交给 Rust 核对、保存，用返回的预览刷新。之前的排版结果作废 */
  const save = async (draft: FormatDraft) => {
    setSaving(true);
    try {
      const hadResult = !!v?.result;
      qc.setQueryData(key, await api.formatPlanUpdate(rulesetId, runId, draft));
      apply.reset();
      if (hadResult) {
        refreshRuleset();
        notify("计划改了，之前的排版结果作废，需要重新排版");
      }
    } catch (e) {
      notify(String(e), "error");
      setRev((n) => n + 1);
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!v) return;
    const ok = await confirm({ title: `删除「${v.thesisName}」的计划？`, message: "计划和报告的存档会删掉。论文原文件、已经写出的排版结果不动。", confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.formatRunDelete(rulesetId, runId);
      refreshRuleset();
      qc.invalidateQueries({ queryKey: ["formatRulesets"] });
      qc.removeQueries({ queryKey: key });
      onBack();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const crumbs = [{ label: "排版", onClick: onRoot }, { label: ruleset.data?.name ?? "规范", onClick: onBack }, { label: v?.thesisName ?? "论文" }];
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

  const d = v.draft;
  const busy = saving || apply.isPending;

  return (
    <Page
      crumbs={crumbs}
      actions={
        <button onClick={remove} title="删除这份计划" className="rounded-md border p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground">
          <Trash2 size={13} />
        </button>
      }
    >
      <div>
        <div className="flex items-start gap-2">
          <h2 className="selectable min-w-0 flex-1 text-base font-semibold break-all">{v.thesisName}</h2>
          {v.mock && (
            <Badge tone="warn" title="模拟模型按固定规矩推的计划，不代表真模型的水平">
              模拟
            </Badge>
          )}
        </div>
        <p className="selectable mt-1 text-xs break-all text-muted-foreground">{v.thesisPath}</p>
        <p className="mt-1 text-xs text-muted-foreground tabular-nums">
          {when(v.createdAt)} · {MODE[v.mode]}（发出 {count(v.sentChars)} 字） · {v.mock ? "模拟，没有花钱" : v.call ? `实际费用 ${usd(v.call.costUsd)}` : ""}
          {v.call?.note ? `（${v.call.note}）` : ""}
        </p>
      </div>

      {v.thesisProblem && <Notice tone="danger" title={v.thesisProblem} />}

      {d.summary && (
        <div>
          <SectionTitle>模型看到的情况</SectionTitle>
          <p className="selectable">{d.summary}</p>
        </div>
      )}

      {d.problems.length > 0 && <Notice title="模型给的计划里，这些项没通过程序核对，已经去掉：" items={d.problems} />}

      <p className="text-xs text-muted-foreground">
        下面几部分是交给排版程序的计划：勾上的才执行，可以改、可以加减。「命中」是程序按现在的勾选在论文里实际数出来的，不是模型说的。
      </p>

      <fieldset disabled={busy} className="min-w-0 space-y-6">
        <StyleMap v={v} rev={rev} onSave={save} />
        <Rules v={v} rev={rev} onSave={save} />
        <Protect v={v} onSave={save} />
        {v.needsZones && <Zones v={v} onSave={save} />}
        {v.threeLine && v.tables.length > 0 && <Tables v={v} onSave={save} />}
      </fieldset>

      {d.observations.length > 0 && (
        <div>
          <SectionTitle>计划解决不了的情况</SectionTitle>
          <p className="mb-2 text-xs text-muted-foreground">模型分析时看到的，供参考（没有经过程序核对）。</p>
          <ul className="selectable list-disc space-y-1 pl-5">
            {d.observations.map((o, i) => (
              <li key={i}>
                {o.text}
                {o.paragraphs.length > 0 && <span className="ml-1 text-xs text-muted-foreground tabular-nums">第 {o.paragraphs.join("、")} 段</span>}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="border-t pt-5">
        <div className="flex items-center gap-3">
          <p className="min-w-0 flex-1 text-xs text-muted-foreground">
            排版是确定性的：不联网、不花钱，改的是一份副本，原文件不动；写出前由另一套程序核对文字、公式、域都没有变。
          </p>
          <button onClick={() => apply.mutate()} disabled={busy} className="flex shrink-0 items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground disabled:opacity-50">
            {apply.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <Play size={13} />}
            {apply.isPending ? "正在排版…" : v.result ? "重新排版" : "开始排版"}
          </button>
        </div>
        {apply.error && <p className="selectable mt-2 text-xs whitespace-pre-wrap text-destructive">{String(apply.error)}</p>}
        {reveal.error && <p className="selectable mt-2 text-xs text-destructive">{String(reveal.error)}</p>}
      </div>

      {v.result && <Result r={v.result} onReveal={(path) => reveal.mutate(path)} />}
    </Page>
  );
}

const roleSelect = "rounded border bg-background px-1 py-0.5 text-xs outline-none focus:border-ring";

function RoleSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const known = ROLES.some((r) => r[0] === value.toLowerCase());
  return (
    <select value={value.toLowerCase()} onChange={(e) => onChange(e.target.value)} className={roleSelect}>
      {!known && <option value={value.toLowerCase()}>{value}</option>}
      {ROLES.map(([key, label]) => (
        <option key={key} value={key}>
          {label}
        </option>
      ))}
    </select>
  );
}

/** 命中的段落：数量和几段开头 */
function Hits({ enabled, hits }: { enabled: boolean; hits: FormatHit[] }) {
  if (!enabled) return <p className="text-xs text-muted-foreground">没有勾选，不执行</p>;
  const n = hits.reduce((sum, h) => sum + h.count, 0);
  const samples = hits.flatMap((h) => h.samples).slice(0, 5);
  if (n === 0) return <p className="text-xs text-amber-700 dark:text-amber-400">命中 0 段：论文里没有段落会因此改变</p>;
  return (
    <div className="text-xs">
      <p className="tabular-nums">
        命中 <span className="font-medium">{count(n)}</span> 段
        {samples.length > 0 && <span className="text-muted-foreground">，例如：</span>}
      </p>
      {samples.length > 0 && (
        <ul className="selectable mt-0.5 space-y-0.5 text-muted-foreground">
          {samples.map((s, i) => (
            <li key={i} className="truncate" title={s}>
              「{s}」
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Row({ enabled, onToggle, onRemove, children }: { enabled: boolean; onToggle: (v: boolean) => void; onRemove?: () => void; children: React.ReactNode }) {
  return (
    <li className="flex items-start gap-2.5 px-3 py-2.5">
      <input type="checkbox" checked={enabled} onChange={(e) => onToggle(e.target.checked)} className="mt-1 shrink-0" />
      <div className="min-w-0 flex-1 space-y-1">{children}</div>
      {onRemove && (
        <button onClick={onRemove} title="删掉这一项" className="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
          <X size={13} />
        </button>
      )}
    </li>
  );
}

const addButton = "flex shrink-0 items-center gap-1 rounded-md border px-2 py-0.5 text-xs hover:bg-muted disabled:opacity-50";

// ---------------- 样式映射 ----------------

function StyleMap({ v, rev, onSave }: { v: FormatRunView; rev: number; onSave: (d: FormatDraft) => void }) {
  const d = v.draft;
  const free = v.styles.filter((s) => !d.styleMap.some((m) => m.from.toLowerCase() === s.toLowerCase()));
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("normal");
  const pick = free.includes(from) ? from : (free[0] ?? "");
  const set = (i: number, patch: Partial<FormatStyleMapping>) => onSave({ ...d, styleMap: d.styleMap.map((m, j) => (j === i ? { ...m, ...patch } : m)) });
  const add = () => {
    if (!pick) return;
    onSave({ ...d, styleMap: [...d.styleMap, { from: pick, to, reason: "手动添加", enabled: true }] });
    setFrom("");
  };
  return (
    <div>
      <SectionTitle>样式映射</SectionTitle>
      <p className="mb-2 text-xs text-muted-foreground">用了别的样式、实际充当某个角色的段落，整体改到目标样式（例如粘贴来的样式充当正文）。</p>
      <ul className="divide-y rounded-md border">
        {d.styleMap.map((m, i) => (
          <Row key={`${m.from}-${rev}`} enabled={m.enabled} onToggle={(enabled) => set(i, { enabled })} onRemove={() => onSave({ ...d, styleMap: d.styleMap.filter((_, j) => j !== i) })}>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span className="selectable rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">{m.from}</span>
              <span className="text-muted-foreground">→</span>
              <RoleSelect value={m.to} onChange={(next) => set(i, { to: next })} />
            </div>
            {m.reason && <p className="selectable text-xs text-muted-foreground">{m.reason}</p>}
            <Hits enabled={m.enabled} hits={v.hits.filter((h) => h.how === "style" && h.from === m.from.toLowerCase())} />
          </Row>
        ))}
        {d.styleMap.length === 0 && <li className="px-3 py-2.5 text-xs text-muted-foreground">没有样式映射：论文里的样式都用对了，或模型没看出要改的。</li>}
        <li className="flex flex-wrap items-center gap-2 bg-muted/40 px-3 py-2 text-xs">
          <span className="text-muted-foreground">加一条</span>
          {free.length === 0 ? (
            <span className="text-muted-foreground">{v.styles.length === 0 ? "读不到论文里的样式" : "论文里的样式都已经列在上面"}</span>
          ) : (
            <>
              <select value={pick} onChange={(e) => setFrom(e.target.value)} className={`${roleSelect} max-w-48 min-w-0`}>
                {free.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
              <span className="text-muted-foreground">→</span>
              <RoleSelect value={to} onChange={setTo} />
              <button onClick={add} className={addButton}>
                <Plus size={12} />
                添加
              </button>
            </>
          )}
        </li>
      </ul>
    </div>
  );
}

// ---------------- 按文字识别 ----------------

/** 输入框里的字数上限 → 数字；空着、0 都是不限 */
function toMaxChars(text: string): number | null {
  const t = text.trim();
  if (t === "") return 0;
  return /^\d{1,4}$/.test(t) ? Number(t) : null;
}

function Rules({ v, rev, onSave }: { v: FormatRunView; rev: number; onSave: (d: FormatDraft) => void }) {
  const { notify } = useUi();
  const d = v.draft;
  const [pattern, setPattern] = useState("");
  const [max, setMax] = useState("");
  const [style, setStyle] = useState("caption");
  const set = (i: number, patch: Partial<FormatRule>) => onSave({ ...d, paragraphRules: d.paragraphRules.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const setMaxChars = (i: number, text: string) => {
    const n = toMaxChars(text);
    if (n === null) notify("字数上限写整数，0 或空着是不限", "error");
    else set(i, { maxChars: n });
  };
  const add = () => {
    const n = toMaxChars(max);
    if (!pattern.trim()) return;
    if (n === null) return notify("字数上限写整数，0 或空着是不限", "error");
    onSave({ ...d, paragraphRules: [...d.paragraphRules, { pattern: pattern.trim(), maxChars: n, style, reason: "手动添加", enabled: true }] });
    setPattern("");
    setMax("");
  };
  // 同一个目标样式的规则分不开各自的命中：预览只按「改到哪个样式」合计
  const shared = (r: FormatRule) => d.paragraphRules.filter((x) => x.enabled && x.style.toLowerCase() === r.style.toLowerCase()).length > 1;
  return (
    <div>
      <SectionTitle>按文字识别</SectionTitle>
      <p className="mb-2 text-xs text-muted-foreground">
        用正则认出手动排的图题、表题这类段落（匹配段落去掉首尾空白后的文字，从头匹配要写 ^），优先于样式映射。字数上限用来排除以同样文字开头的正文句子，0 是不限。
      </p>
      <ul className="divide-y rounded-md border">
        {d.paragraphRules.map((r, i) => (
          <Row key={`${i}-${rev}`} enabled={r.enabled} onToggle={(enabled) => set(i, { enabled })} onRemove={() => onSave({ ...d, paragraphRules: d.paragraphRules.filter((_, j) => j !== i) })}>
            <CommitInput value={r.pattern} onCommit={(next) => set(i, { pattern: next })} className="w-full font-mono text-xs" />
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
              <span className="text-muted-foreground">不超过</span>
              <CommitInput value={String(r.maxChars)} inputMode="numeric" onCommit={(next) => setMaxChars(i, next)} className="w-14 text-right tabular-nums" />
              <span className="text-muted-foreground">字{r.maxChars === 0 ? "（不限）" : ""}的段落 →</span>
              <RoleSelect value={r.style} onChange={(next) => set(i, { style: next })} />
            </div>
            {r.reason && <p className="selectable text-xs text-muted-foreground">{r.reason}</p>}
            <Hits enabled={r.enabled} hits={v.hits.filter((h) => h.how === "rule" && h.to === r.style.toLowerCase())} />
            {r.enabled && shared(r) && <p className="text-xs text-muted-foreground">几条规则改到同一个样式时，命中是它们合起来的数。</p>}
          </Row>
        ))}
        {d.paragraphRules.length === 0 && <li className="px-3 py-2.5 text-xs text-muted-foreground">没有文字规则。</li>}
        <li className="flex flex-wrap items-center gap-2 bg-muted/40 px-3 py-2 text-xs">
          <span className="text-muted-foreground">加一条</span>
          <input
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            placeholder="正则，如 ^图\s*\d+"
            className="selectable min-w-32 flex-1 rounded border bg-background px-1.5 py-0.5 font-mono outline-none focus:border-ring"
          />
          <input
            value={max}
            onChange={(e) => setMax(e.target.value)}
            placeholder="字数上限"
            inputMode="numeric"
            className="selectable w-20 rounded border bg-background px-1.5 py-0.5 outline-none focus:border-ring"
          />
          <span className="text-muted-foreground">→</span>
          <RoleSelect value={style} onChange={setStyle} />
          <button onClick={add} disabled={!pattern.trim()} className={addButton}>
            <Plus size={12} />
            添加
          </button>
        </li>
      </ul>
    </div>
  );
}

// ---------------- 受保护的分节 ----------------

function Protect({ v, onSave }: { v: FormatRunView; onSave: (d: FormatDraft) => void }) {
  const d = v.draft;
  // 论文读不到时没有分节信息：只列计划里已有的
  const indexes = [...new Set([...v.sections.map((s) => s.index), ...d.protectSections.map((p) => p.section)])].sort((a, b) => a - b);
  const toggle = (section: number, enabled: boolean) => {
    const has = d.protectSections.some((p) => p.section === section);
    const next = has ? d.protectSections.map((p) => (p.section === section ? { ...p, enabled } : p)) : [...d.protectSections, { section, reason: "手动添加", enabled }];
    onSave({ ...d, protectSections: next.sort((a, b) => a.section - b.section) });
  };
  return (
    <div>
      <SectionTitle>受保护的分节</SectionTitle>
      <p className="mb-2 text-xs text-muted-foreground">勾上的分节整节保持原样：不清理、不映射（封面、题名页、声明这类学校有统一格式的内容）。节里还有摘要、目录或正文就不要勾。</p>
      {indexes.length === 0 ? (
        <p className="text-xs text-muted-foreground">读不到论文的分节。</p>
      ) : (
        <ul className="divide-y rounded-md border">
          {indexes.map((index) => {
            const s = v.sections.find((x) => x.index === index);
            const p = d.protectSections.find((x) => x.section === index);
            const on = !!p?.enabled;
            return (
              <Row key={index} enabled={on} onToggle={(enabled) => toggle(index, enabled)}>
                <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="font-medium">第 {index} 节</span>
                  {s && (
                    <span className="text-xs text-muted-foreground tabular-nums">
                      {count(s.paragraphs)} 个段落，{count(s.chars)} 字
                    </span>
                  )}
                  {on && <Badge tone="accent">保持原样</Badge>}
                </p>
                {p?.reason && <p className="selectable text-xs text-muted-foreground">{p.reason}</p>}
                {s && s.opening.length > 0 && (
                  <ul className="selectable space-y-0.5 text-xs text-muted-foreground">
                    {s.opening.map((line, i) => (
                      <li key={i} className="truncate" title={line}>
                        「{line}」
                      </li>
                    ))}
                  </ul>
                )}
              </Row>
            );
          })}
        </ul>
      )}
    </div>
  );
}

// ---------------- 分区（页眉、页码按它分别处理） ----------------

function Zones({ v, onSave }: { v: FormatRunView; onSave: (d: FormatDraft) => void }) {
  const d = v.draft;
  const l = d.layout;
  const set = (patch: Partial<typeof l>) => onSave({ ...d, layout: { ...l, ...patch } });
  const rows: { key: "frontStart" | "bodyStart" | "backStart"; label: string; hint: string; none: string }[] = [
    { key: "frontStart", label: "前置部分从", hint: "一般是中文摘要的标题。它前面是封面、题名页：没有页眉页码。", none: "（没有前置部分）" },
    { key: "bodyStart", label: "正文从", hint: "一般是引言（绪论）的标题。页码从这里重新从 1 起。", none: "（没有指出：页眉、页码不处理）" },
    { key: "backStart", label: "正文之后从", hint: "致谢后面不要页眉的部分，如原创性声明。参考文献、附录、致谢都算正文。", none: "（没有）" },
  ];
  const pick = (key: (typeof rows)[number]["key"], value: string) => {
    const a = v.anchors.find((x) => String(x.paragraph) === value) ?? null;
    set({ [key]: a } as Partial<typeof l>);
  };
  const label = (a: FormatAnchor) => `¶${a.paragraph} ${a.text}`;
  return (
    <div>
      <SectionTitle>分区</SectionTitle>
      <p className="mb-2 text-xs text-muted-foreground">
        页眉、页码按论文的几个部分分别处理。分界处没有分节符的会加一个（不新增段落）；起点选错了页眉页码就会排错地方，请对照论文确认。
      </p>
      {!l.bodyStart && <Notice title="还没有指出正文从哪一段开始：页眉、页码、奇数页起这几项不会处理，报告里会标成不符合。" />}
      <dl className="grid grid-cols-[6.5rem_minmax(0,1fr)] items-start gap-x-3 gap-y-2 rounded-md border px-3 py-2.5">
        {rows.map((r) => (
          <div key={r.key} className="contents">
            <dt className="pt-1 text-muted-foreground">{r.label}</dt>
            <dd className="min-w-0">
              <select
                value={l[r.key] ? String(l[r.key]!.paragraph) : ""}
                onChange={(e) => pick(r.key, e.target.value)}
                className="w-full max-w-md rounded-md border bg-background px-2 py-1 text-xs outline-none focus:border-ring"
              >
                <option value="">{r.none}</option>
                {v.anchors.map((a) => (
                  <option key={a.paragraph} value={a.paragraph}>
                    {label(a)}
                  </option>
                ))}
              </select>
              <p className="mt-0.5 text-xs text-muted-foreground">{r.hint}</p>
            </dd>
          </div>
        ))}
        <dt className="pt-0.5 text-muted-foreground">印制</dt>
        <dd>
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
            {[false, true].map((duplex) => (
              <label key={String(duplex)} className="flex cursor-default items-center gap-1.5">
                <input type="radio" name="format-duplex" checked={l.duplex === duplex} onChange={() => set({ duplex })} />
                {duplex ? "双面（页眉分奇偶页）" : "单面"}
              </label>
            ))}
          </div>
          {l.duplexNote && <p className="selectable mt-0.5 text-xs text-muted-foreground">{l.duplexNote}</p>}
        </dd>
      </dl>
      {l.reason && <p className="selectable mt-1.5 text-xs text-muted-foreground">模型的依据：{l.reason}</p>}
    </div>
  );
}

// ---------------- 表格（三线表） ----------------

function Tables({ v, onSave }: { v: FormatRunView; onSave: (d: FormatDraft) => void }) {
  const d = v.draft;
  const toggle = (table: number, threeLine: boolean) => {
    const has = d.skipTables.some((t) => t.table === table);
    const next = has ? d.skipTables.map((t) => (t.table === table ? { ...t, enabled: !threeLine } : t)) : [...d.skipTables, { table, reason: "手动选的", enabled: !threeLine }];
    onSave({ ...d, skipTables: next.sort((a, b) => a.table - b.table) });
  };
  return (
    <div>
      <SectionTitle>表格</SectionTitle>
      <p className="mb-2 text-xs text-muted-foreground">
        勾上的排成三线表：只留顶线、表头下的栏目线、底线（1.5 / 0.75 / 1.5 磅），去掉竖线和其他横线。不是数据表的（信息栏、签名栏、用表格排的公式）去掉勾。没有边框的表格程序自己跳过。
      </p>
      <ul className="divide-y rounded-md border">
        {v.tables.map((t) => {
          const skip = d.skipTables.find((x) => x.table === t.index && x.enabled);
          return (
            <li key={t.index} className="flex items-start gap-2 px-3 py-2">
              <input type="checkbox" checked={t.state === "three_line"} disabled={t.state === "auto"} onChange={(e) => toggle(t.index, e.target.checked)} className="mt-1" />
              <div className="min-w-0 flex-1 space-y-0.5">
                <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="font-medium">第 {t.index} 个表格</span>
                  <span className="text-xs text-muted-foreground tabular-nums">
                    {t.rows} 行 × {t.cols} 列，从 ¶{t.paragraph} 开始
                  </span>
                  {t.state === "three_line" && <Badge tone="accent">三线表</Badge>}
                </p>
                {t.firstRow && (
                  <p className="selectable truncate text-xs text-muted-foreground" title={t.firstRow}>
                    第一行：{t.firstRow}
                  </p>
                )}
                {t.state === "auto" && <p className="text-xs text-muted-foreground">不排：{t.why}</p>}
                {t.state === "skipped" && <p className="selectable text-xs text-muted-foreground">不排：{skip?.reason || t.why}</p>}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

// ---------------- 排版结果 ----------------

function Result({ r, onReveal }: { r: FormatRunResult; onReveal: (path: string) => void }) {
  return (
    <div className="space-y-4">
      {r.delivered ? (
        <div className="rounded-md border border-emerald-300/70 bg-emerald-50 px-3 py-2 text-xs dark:border-emerald-800/60 dark:bg-emerald-950/30">
          <div className="flex items-center gap-2">
            <CircleCheck size={14} className="shrink-0 text-emerald-600" />
            <p className="min-w-0 flex-1">
              <span className="font-medium">已写出</span>
              <span className="selectable ml-1.5 break-all">{r.outputPath ? baseName(r.outputPath) : ""}</span>
            </p>
            {r.outputPath && (
              <button onClick={() => onReveal(r.outputPath!)} className="flex shrink-0 items-center gap-1 rounded-md border bg-background px-2 py-1 hover:bg-muted">
                <FolderOpen size={13} />
                在访达中显示
              </button>
            )}
          </div>
          <p className="mt-1 text-muted-foreground tabular-nums">
            {when(r.appliedAt)} · 保真校验通过，原文件没有改动{r.reportPath ? `；报告也存在旁边：${baseName(r.reportPath)}` : ""}
          </p>
        </div>
      ) : (
        <Notice tone="danger" title="保真校验没通过，没有写出文件，原文件没有改动。">
          发现 {r.fidelityProblems} 处问题：排版后的文字、公式、域或结构与原文不一致。具体见下面报告里的「保真问题」。
        </Notice>
      )}

      <p className="flex items-center gap-1.5 text-xs">
        {r.specFailed === 0 ? <CircleCheck size={13} className="shrink-0 text-emerald-600" /> : <CircleAlert size={13} className="shrink-0 text-amber-600" />}
        <span className="tabular-nums">规范核对：{r.specChecks === 0 ? "没有检查" : r.specFailed === 0 ? `${r.specChecks} 项全部符合` : `${r.specChecks} 项中 ${r.specFailed} 项不符合`}</span>
      </p>

      {r.checks.length > 0 && (
        <div>
          <SectionTitle>只检查、不改的要求</SectionTitle>
          <p className="mb-2 text-xs text-muted-foreground">统计由程序完成，不是模型数的。「需人工核对」的程序没有检查。</p>
          <table className="w-full text-left text-xs">
            <thead className="text-muted-foreground">
              <tr className="border-b">
                <th className="py-1.5 pr-3 font-normal">要求</th>
                <th className="py-1.5 pr-3 font-normal whitespace-nowrap">结果</th>
                <th className="py-1.5 pr-3 font-normal">统计</th>
                <th className="py-1.5 font-normal">说明</th>
              </tr>
            </thead>
            <tbody className="selectable">
              {r.checks.map((c) => (
                <tr key={c.requirement} className="border-b border-border/60 align-top">
                  <td className="py-1.5 pr-3">
                    <span className="mr-1.5 font-mono text-muted-foreground">{c.requirement}</span>
                    {c.text}
                  </td>
                  <td className="py-1.5 pr-3 whitespace-nowrap">
                    {c.ok === true ? <span className="text-emerald-700 dark:text-emerald-400">✓ 符合</span> : c.ok === false ? <span className="font-medium text-destructive">✗ 不符合</span> : <span className="text-amber-700 dark:text-amber-400">需人工核对</span>}
                  </td>
                  <td className="py-1.5 pr-3 tabular-nums">{c.value}</td>
                  <td className="py-1.5 text-muted-foreground">{c.detail}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <details open className="rounded-md border px-3 py-2">
        <summary className="cursor-default text-xs font-medium">报告全文</summary>
        <div className="mt-3">
          <Markdown text={r.reportMd} />
        </div>
      </details>
    </div>
  );
}
