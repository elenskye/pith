// 调查做完以后：短答在前（数字可以点回原文），展开看子问题、搜索、来源、事实表、计算过程、费用（decisions/0020「报告」）。
// 内容全部来自 Rust 的 Report（crates/research-engine/src/report.rs），这里只负责显示。
import { useState } from "react";
import { ChevronRight, ExternalLink } from "lucide-react";
import { api, type ResearchCalc, type ResearchConclusion, type ResearchFactRow, type ResearchReport, type ResearchSegment, type ResearchSourceRow } from "@/shared/ipc";
import { Modal } from "@/shared/ui";
import { Badge, Notice } from "@/format/parts";
import { count, when } from "@/format/labels";
import { ENGINE, KIND, SOURCE_STATE, TIER, TIER_TONE, UNCERTAINTY, UNCERTAINTY_HINT, hostOf, isWeb, money } from "@/research/labels";

type Pick = { kind: "fact"; id: string } | { kind: "calc"; id: string } | null;

const openUrl = (url: string) => void api.openUrl(url).catch(() => {});

function Segments({ segments, onPick }: { segments: ResearchSegment[]; onPick: (p: Pick) => void }) {
  return (
    <>
      {segments.map((s, i) => {
        if (s.kind === "text") return <span key={i}>{s.text}</span>;
        if (s.kind === "unmatched")
          return (
            <mark key={i} title={s.why} className="rounded bg-destructive/15 px-0.5 font-medium text-destructive">
              {s.text}
            </mark>
          );
        return (
          <button
            key={i}
            onClick={() => onPick({ kind: s.kind, id: s.id })}
            title={s.kind === "fact" ? `点开看原文（${s.id}）` : `点开看算式（${s.id}）`}
            className="rounded px-0.5 font-semibold text-primary underline decoration-primary/40 decoration-dotted underline-offset-2 hover:bg-primary/10"
          >
            {s.text}
          </button>
        );
      })}
    </>
  );
}

function ConclusionItem({ k, report, onPick }: { k: ResearchConclusion; report: ResearchReport; onPick: (p: Pick) => void }) {
  return (
    <li className="leading-relaxed">
      <p className="selectable">
        <Segments segments={k.segments} onPick={onPick} />
      </p>
      <div className="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
        {k.uncertainty.map((u) => (
          <Badge key={u} tone="warn" title={UNCERTAINTY_HINT[u]}>
            {UNCERTAINTY[u]}
          </Badge>
        ))}
        {k.note && <span>{k.note}</span>}
        {k.facts.length > 0 && <span>依据：</span>}
        {k.facts.map((id) => {
          const f = report.facts.find((x) => x.id === id);
          return (
            <button key={id} onClick={() => onPick({ kind: "fact", id })} className="rounded border px-1 hover:bg-muted" title={f ? `${f.producer} · 可信度${f.tierLabel}` : id}>
              {id}
              {f && <span className="ml-1 text-[10px]">{f.tierLabel}</span>}
            </button>
          );
        })}
      </div>
      {k.issues.length > 0 && <Notice tone="danger" title="这条结论里有对不上的地方" items={k.issues} />}
    </li>
  );
}

function FactDialog({ f, report, onClose }: { f: ResearchFactRow; report: ResearchReport; onClose: () => void }) {
  const source = report.sources.find((s) => s.id === f.source);
  const url = source?.finalUrl ?? source?.url ?? f.sourceUrl;
  return (
    <Modal title={`${f.id}　${f.metric}`} onClose={onClose} width="w-[36rem]">
      <div className="space-y-3 text-xs">
        <p className="text-base font-semibold tabular-nums">{f.value + (f.unit && !f.value.endsWith(f.unit) ? f.unit : "")}</p>
        <dl className="grid grid-cols-[5rem_minmax(0,1fr)] gap-x-2 gap-y-1">
          {[
            ["地区", f.region],
            ["人群", f.population],
            ["时间", f.period],
            ["口径", f.definition],
          ]
            .filter(([, v]) => v)
            .map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="text-muted-foreground">{k}</dt>
                <dd className="selectable">{v}</dd>
              </div>
            ))}
          <dt className="text-muted-foreground">数据生产者</dt>
          <dd>
            {f.producer || "（没写）"} <span className="text-muted-foreground">· {f.producerKindLabel}</span>
            {f.relayed && <span className="text-muted-foreground"> · 这份来源是转述</span>}
          </dd>
          <dt className="text-muted-foreground">可信度</dt>
          <dd>
            <Badge tone={TIER_TONE[f.tier]}>{f.tierLabel}</Badge>
            <span className="ml-1 text-muted-foreground">按「{report.tableName}」</span>
          </dd>
          <dt className="text-muted-foreground">核对</dt>
          <dd>{f.check.status === "verified" ? <Badge tone="ok">已核实：摘录在保存的原文里逐字找到，数值和单位对得上</Badge> : <Badge tone="danger">未核实：{f.check.reason}</Badge>}</dd>
        </dl>
        <div>
          <p className="mb-1 font-medium">原文摘录</p>
          <blockquote className="selectable border-l-2 border-primary/50 bg-muted/50 px-3 py-2 whitespace-pre-wrap">{f.quote}</blockquote>
          {f.unitQuote && <p className="selectable mt-1 text-muted-foreground">单位见原文：「{f.unitQuote}」</p>}
        </div>
        {source && (
          <div className="rounded-md border px-3 py-2">
            <p className="flex items-center gap-1.5">
              <span className="font-mono text-muted-foreground">{source.id}</span>
              <span className="min-w-0 flex-1 truncate font-medium" title={source.title}>
                {source.title || url}
              </span>
              {isWeb(url) && (
                <button onClick={() => openUrl(url)} className="flex shrink-0 items-center gap-1 text-primary hover:underline">
                  <ExternalLink size={12} />
                  打开网页
                </button>
              )}
            </p>
            <p className="selectable mt-0.5 break-all text-muted-foreground">{url}</p>
            {source.fetchedAt && (
              <p className="mt-0.5 text-muted-foreground tabular-nums">
                抓取于 {when(source.fetchedAt)} · SHA-256 {source.sha256?.slice(0, 16)}…
              </p>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
}

function CalcDialog({ c, onClose }: { c: ResearchCalc; onClose: () => void }) {
  return (
    <Modal title={`${c.id}　${c.label || "计算"}`} onClose={onClose} width="w-[32rem]">
      <div className="space-y-2 text-xs">
        <p className="text-base font-semibold tabular-nums">{c.shown || "算不了"}</p>
        <p className="selectable rounded-md bg-muted px-3 py-2 font-mono">{c.error ? `算不了：${c.error}` : c.steps}</p>
        <p className="text-muted-foreground">计算由 Pith 执行（精确的十进制，不用浮点），不由模型口算。用到的事实：{c.inputs.join("、")}。</p>
      </div>
    </Modal>
  );
}

function Section({ title, children, open = false }: { title: string; children: React.ReactNode; open?: boolean }) {
  const [o, setO] = useState(open);
  return (
    <div className="rounded-md border">
      <button onClick={() => setO(!o)} className="flex w-full items-center gap-1.5 px-3 py-2 text-left text-xs font-medium hover:bg-muted/60">
        <ChevronRight size={13} className={`shrink-0 transition-transform ${o ? "rotate-90" : ""}`} />
        {title}
      </button>
      {o && <div className="space-y-2 border-t px-3 py-2 text-xs">{children}</div>}
    </div>
  );
}

function Table({ head, children }: { head: string[]; children: React.ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-xs">
        <thead className="text-muted-foreground">
          <tr className="border-b">
            {head.map((h) => (
              <th key={h} className="py-1 pr-3 font-normal whitespace-nowrap">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

function SourceTitle({ s }: { s: ResearchSourceRow }) {
  const url = s.finalUrl ?? s.url;
  return (
    <span className="min-w-0">
      <span className="block truncate" title={s.title || url}>
        {s.title || url}
      </span>
      {isWeb(url) ? (
        <button onClick={() => openUrl(url)} className="text-[11px] text-muted-foreground hover:text-primary hover:underline">
          {hostOf(url)}
        </button>
      ) : (
        <span className="block truncate text-[11px] text-muted-foreground" title={url}>
          {url}
        </span>
      )}
    </span>
  );
}

export function ShortAnswer({ report }: { report: ResearchReport }) {
  const [pick, setPick] = useState<Pick>(null);
  const fact = pick?.kind === "fact" ? report.facts.find((f) => f.id === pick.id) : undefined;
  const calc = pick?.kind === "calc" ? report.calcs.find((c) => c.id === pick.id) : undefined;
  return (
    <div className="space-y-3">
      {report.conclusions.length === 0 ? (
        <p className="text-sm text-muted-foreground">这次调查没有得出结论。</p>
      ) : (
        <ol className="list-decimal space-y-3 pl-5 text-sm">
          {report.conclusions.map((k) => (
            <ConclusionItem key={k.id} k={k} report={report} onPick={setPick} />
          ))}
        </ol>
      )}
      {report.gaps.length > 0 && <Notice tone="info" title="没有查到的" items={report.gaps} />}
      {report.notes.length > 0 && <Notice title="读之前要知道" items={report.notes} />}
      {fact && <FactDialog f={fact} report={report} onClose={() => setPick(null)} />}
      {calc && <CalcDialog c={calc} onClose={() => setPick(null)} />}
    </div>
  );
}

export function DiffView({ report }: { report: ResearchReport }) {
  const d = report.diff;
  if (!d) return null;
  if (d.changed.length + d.added.length + d.removed.length + d.sourcesAdded.length + d.sourcesRemoved.length + d.conclusionsAdded.length + d.conclusionsRemoved.length === 0)
    return <Notice tone="info" title={`和上一次（${when(d.previousAt)}）相比，数字、来源和结论都没有变（${d.unchanged} 条事实对得上）。`} />;
  const brief = (f: { metric: string; region: string; population: string; period: string }) => [f.metric, f.region, f.population, f.period].filter(Boolean).join(" · ");
  return (
    <div className="space-y-2 rounded-md border border-primary/30 bg-accent/30 px-3 py-2 text-xs">
      <p className="font-medium">和上一次调查（{when(d.previousAt)}）相比</p>
      {d.changed.length > 0 && (
        <div>
          <p className="text-muted-foreground">变了的数字</p>
          <ul className="space-y-0.5">
            {d.changed.map((c) => (
              <li key={c.new.id}>
                {brief(c.new)}：<s className="text-muted-foreground">{c.old.value}</s> → <span className="font-semibold">{c.new.value}</span>
                <span className="text-muted-foreground">（{c.new.producer}）</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {d.added.length > 0 && (
        <div>
          <p className="text-muted-foreground">这次新查到的</p>
          <ul className="space-y-0.5">
            {d.added.map((f) => (
              <li key={f.id}>
                <Badge tone="ok">新</Badge> {brief(f)}：<span className="font-semibold">{f.value}</span>
                <span className="text-muted-foreground">（{f.producer}）</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {d.removed.length > 0 && (
        <div>
          <p className="text-muted-foreground">上次有、这次没查到的</p>
          <ul className="space-y-0.5">
            {d.removed.map((f) => (
              <li key={f.id} className="text-muted-foreground">
                {brief(f)}：{f.value}（{f.producer}）
              </li>
            ))}
          </ul>
        </div>
      )}
      {(d.sourcesAdded.length > 0 || d.sourcesRemoved.length > 0) && (
        <p>
          来源：新增 {d.sourcesAdded.length} 个{d.sourcesAdded.length > 0 && `（${d.sourcesAdded.map((s) => s.title || hostOf(s.url)).join("、")}）`}，消失 {d.sourcesRemoved.length} 个
          {d.sourcesRemoved.length > 0 && `（${d.sourcesRemoved.map((s) => s.title || hostOf(s.url)).join("、")}）`}
        </p>
      )}
      {(d.conclusionsAdded.length > 0 || d.conclusionsRemoved.length > 0) && (
        <p>
          结论：{d.conclusionsAdded.length} 条变了，{d.conclusionsSame} 条没变。上次的说法：
          {d.conclusionsRemoved.map((t, i) => (
            <span key={i} className="mt-0.5 block text-muted-foreground">
              · {t}
            </span>
          ))}
        </p>
      )}
    </div>
  );
}

/** 展开看全过程 */
export function Process({ report, selected, onSelect }: { report: ResearchReport; selected: string[]; onSelect: (keys: string[]) => void }) {
  const t = report.totals;
  const toggle = (id: string) => onSelect(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);
  return (
    <div className="space-y-2">
      <Section title={`子问题（${t.subQuestions} 个）`}>
        {report.summary && <p>{report.summary}</p>}
        <Table head={["编号", "子问题", "角度", "指标 · 口径"]}>
          {report.subQuestions
            .filter((q) => q.enabled)
            .map((q) => (
              <tr key={q.id} className="border-b border-border/60 align-top">
                <td className="py-1 pr-3 font-mono">{q.id}</td>
                <td className="py-1 pr-3">{q.text}</td>
                <td className="py-1 pr-3 whitespace-nowrap">{q.angle === "alternative" ? <Badge tone="accent">官方统计之外</Badge> : <Badge>官方统计</Badge>}</td>
                <td className="py-1 pr-3 text-muted-foreground">{[q.metric, q.definition].filter(Boolean).join(" · ")}</td>
              </tr>
            ))}
        </Table>
        {report.dimensions.length > 0 && <Notice tone="info" title="官方统计之外的衡量维度" items={report.dimensions.map((d) => `${d.name}：${d.why}${d.sources ? `（${d.sources}）` : ""}`)} />}
        {report.caveats.length > 0 && <Notice title="比较时要小心的地方" items={report.caveats} />}
      </Section>

      <Section title={`搜索（${report.searches.length} 次）`}>
        <Table head={["子问题", "搜索词", "引擎", "结果"]}>
          {report.searches.map((s, i) => (
            <tr key={i} className="border-b border-border/60 align-top">
              <td className="py-1 pr-3 font-mono">{s.subQuestion || "计划时"}</td>
              <td className="selectable py-1 pr-3">{s.query}</td>
              <td className="py-1 pr-3 whitespace-nowrap">{ENGINE[s.engine] ?? s.engine}</td>
              <td className="py-1 pr-3">{s.error ? <span className="text-destructive">{s.error}</span> : `${s.hits.length} 条`}</td>
            </tr>
          ))}
        </Table>
      </Section>

      <Section title={`来源（${t.sources} 个，${t.readable} 个可读）`} open>
        <p className="text-muted-foreground">
          按可信度从高到低，用的是「{report.tableName}」：看的是数据生产者（谁统计的），不是谁转述的。勾上的可以加入文献库。
        </p>
        <Table head={["", "编号", "可信度", "来源", "数据生产者", "读到的"]}>
          {report.sources.map((s) => (
            <tr key={s.id} className="border-b border-border/60 align-top">
              <td className="py-1 pr-1">
                <input type="checkbox" checked={selected.includes(s.id)} disabled={s.state === "failed" || s.state === "pending"} onChange={() => toggle(s.id)} aria-label={`选择 ${s.id}`} />
              </td>
              <td className="py-1 pr-3 font-mono">{s.id}</td>
              <td className="py-1 pr-3">{s.tier ? <Badge tone={TIER_TONE[s.tier]}>{TIER[s.tier]}</Badge> : <span className="text-muted-foreground">—</span>}</td>
              <td className="max-w-64 py-1 pr-3">
                <SourceTitle s={s} />
                {s.itemId !== null && <Badge tone="ok">已在文献库</Badge>}
                {s.origin === "manual" && <Badge>手动加的</Badge>}
                {s.fetchedVia === "claude_web_fetch" && (
                  <Badge tone="accent" title={`本机下载失败，改用 Claude 网页抓取拿到的正文`}>
                    经 Claude 网页抓取
                  </Badge>
                )}
              </td>
              <td className="py-1 pr-3">{s.producers.join("、")}</td>
              <td className="py-1 pr-3">
                {s.state === "ok" ? (
                  <span>
                    {s.facts} 条事实，{s.verified} 条核实
                    {s.truncated && <span className="text-amber-700">；正文没读完</span>}
                    {s.extractError && <span className="block text-destructive">{s.extractError}</span>}
                  </span>
                ) : (
                  <span className={s.state === "pending" ? "text-muted-foreground" : "text-destructive"}>{s.fetchError ?? s.textError ?? SOURCE_STATE[s.state]}</span>
                )}
              </td>
            </tr>
          ))}
        </Table>
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-muted-foreground">
          {report.table.map((r) => (
            <span key={r.tier}>
              <Badge tone={TIER_TONE[r.tier]}>{r.label}</Badge> {r.kinds.join("、")}
            </span>
          ))}
        </div>
        {report.tableChanges.length > 0 && <p className="text-muted-foreground">和默认表不同：{report.tableChanges.join("；")}</p>}
      </Section>

      <Section title={`事实表（${t.facts} 条，${t.verified} 条核实）`}>
        <p className="text-muted-foreground">每条的摘录都由 Pith 在保存下来的原文里逐字找过（只容忍空白和全半角），数值和单位也在摘录里对过。没核上的不进结论。</p>
        <Table head={["编号", "指标", "数值", "地区 · 人群 · 时间", "数据生产者", "可信度", "核对"]}>
          {report.facts.map((f) => (
            <tr key={f.id} className="border-b border-border/60 align-top">
              <td className="py-1 pr-3 font-mono">{f.id}</td>
              <td className="py-1 pr-3">{f.metric}</td>
              <td className="py-1 pr-3 font-medium whitespace-nowrap tabular-nums">{f.value + (f.unit && !f.value.endsWith(f.unit) ? f.unit : "")}</td>
              <td className="py-1 pr-3 text-muted-foreground">{[f.region, f.population, f.period].filter(Boolean).join(" · ")}</td>
              <td className="py-1 pr-3" title={KIND[f.producerKind]}>
                {f.producer}
                {f.relayed && <span className="text-muted-foreground">（转述）</span>}
              </td>
              <td className="py-1 pr-3">
                <Badge tone={TIER_TONE[f.tier]}>{f.tierLabel}</Badge>
              </td>
              <td className="py-1 pr-3">{f.check.status === "verified" ? <Badge tone="ok">已核实</Badge> : <span className="text-destructive">{f.check.reason ?? "还没核对"}</span>}</td>
            </tr>
          ))}
        </Table>
      </Section>

      {report.calcs.length > 0 && (
        <Section title={`计算过程（${report.calcs.length} 个）`}>
          <p className="text-muted-foreground">计算由 Pith 执行，不由模型口算。</p>
          <ul className="space-y-1">
            {report.calcs.map((c) => (
              <li key={c.id}>
                <span className="font-mono text-muted-foreground">{c.id}</span> {c.label}：<span className="selectable font-mono">{c.error ? `算不了（${c.error}）` : c.steps}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      <Section title={`过程与费用（${t.calls} 次调用，${report.mock ? "模拟，没有花钱" : money(t.costUsd)}）`}>
        <Table head={["步骤", "接口", "用量", "费用", "备注"]}>
          {report.steps.map((s, i) => (
            <tr key={i} className="border-b border-border/60 align-top">
              <td className="py-1 pr-3">{s.label}</td>
              <td className="py-1 pr-3 whitespace-nowrap text-muted-foreground">{s.provider ? `${s.provider}${s.model ? ` · ${s.model}` : ""}` : "本机"}</td>
              <td className="py-1 pr-3 whitespace-nowrap tabular-nums">{s.inputTokens + s.outputTokens > 0 ? `${count(s.inputTokens)} / ${count(s.outputTokens)}` : s.searches > 0 ? `${s.searches} 次` : ""}</td>
              <td className="py-1 pr-3 tabular-nums">{s.provider ? (s.mock ? "模拟" : money(s.costUsd)) : ""}</td>
              <td className="py-1 pr-3">{s.ok ? <span className="text-muted-foreground">{s.note}</span> : <span className="text-destructive">失败：{s.note}</span>}</td>
            </tr>
          ))}
        </Table>
      </Section>
    </div>
  );
}

