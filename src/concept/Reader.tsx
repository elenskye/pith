// 一章的原文（decisions/0024 第 10 条：原文直接显示在概念页里，带段落号，划选就能分析；不做完整的 EPUB 阅读器）。
// - 段落号在左边；有页码的版本在边上标段落开头的页，段内分页处插一个小标记；
// - 中英并排（可以只看原文或只看中译）；译本的旁注、注释、导读和正文的样子分开，导读收在最上面；
// - 划选（或点段落号）选几段 → 选技能；顶上有「整章分析」「问这本书」「追踪概念」「自由」。
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { BookText, LoaderCircle, MessageCircleQuestion, Route, Sparkles, X } from "lucide-react";
import { api, type ConceptPara, type ConceptRunRequest, type ConceptSkill, type ConceptUnitView } from "@/shared/ipc";
import { Badge, Notice, Page } from "@/format/parts";
import { useStoredState } from "@/shared/ui";
import { ROLE } from "@/concept/labels";
import { AskDialog, FreeDialog, TraceDialog } from "@/concept/Entries";

export type Focus = { key: string; start?: number; end?: number } | null;
type Mode = "both" | "en" | "zh";

/** 段落文字：段内分页处插标记，要高亮的那一段加 <mark> */
function Body({ p, focus, pages }: { p: ConceptPara; focus: Focus; pages: boolean }) {
  const chars = Array.from(p.text);
  const cuts = new Set<number>([0, chars.length]);
  for (const b of pages ? p.breaks : []) cuts.add(b.at);
  const hl = focus && focus.key === p.key && focus.start !== undefined && focus.end !== undefined ? [focus.start, focus.end] : null;
  if (hl) {
    cuts.add(hl[0]);
    cuts.add(hl[1]);
  }
  const points = [...cuts].filter((x) => x >= 0 && x <= chars.length).sort((a, b) => a - b);
  const out: React.ReactNode[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const [a, b] = [points[i], points[i + 1]];
    const brk = pages ? p.breaks.find((x) => x.at === a) : undefined;
    if (brk) out.push(<span key={`b${a}`} className="mx-0.5 rounded bg-muted px-1 align-[1px] text-[10px] text-muted-foreground tabular-nums select-none" title={`第 ${brk.page} 页从这里开始`}>{`p.${brk.page}`}</span>);
    const piece = chars.slice(a, b).join("");
    out.push(hl && a >= hl[0] && b <= hl[1] ? <mark key={a} className="rounded bg-yellow-200 px-0.5 dark:bg-yellow-700/60">{piece}</mark> : <span key={a}>{piece}</span>);
  }
  return <>{out}</>;
}

function Apparatus({ p }: { p: ConceptPara }) {
  const tone = p.role === "sidenote" ? "border-amber-300/80 bg-amber-50/70 dark:border-amber-800/60 dark:bg-amber-950/30" : "border-border bg-muted/50";
  return (
    <div className={`mt-1 rounded border-l-2 px-2 py-1 text-[12px] leading-relaxed text-muted-foreground ${tone}`}>
      <span className="mr-1 text-[10px] font-medium">{ROLE[p.role]}</span>
      <span className="text-[10px] tabular-nums">{p.key}</span>
      <p className="selectable">{p.text}</p>
    </div>
  );
}

export function Reader({ view, skills, focus, onRequest, onSettings }: { view: ConceptUnitView; skills: ConceptSkill[]; focus: Focus; onRequest: (r: ConceptRunRequest) => void; onSettings: () => void }) {
  const [mode, setMode] = useStoredState<Mode>("concept.mode", "both");
  const [chosen, setChosen] = useState<number[]>([]);
  const [dialog, setDialog] = useState<"ask" | "trace" | "free" | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const textUid = view.textUid;
  const zhBySeq = useMemo(() => new Map(view.translation.map((p) => [p.seq, p])), [view.translation]);
  const guides = view.translation.filter((p) => p.role === "guide");
  const body = view.paragraphs.filter((p) => p.role === "body");
  const hasZh = view.translation.length > 0;
  const showZh = hasZh && mode !== "en";
  const showEn = mode !== "zh" || !hasZh;
  const pages = view.paragraphs.some((p) => p.page);

  // 跳到要看的段落
  useEffect(() => {
    if (!focus) return;
    const el = box.current?.querySelector(`[data-key="${CSS.escape(focus.key)}"]`);
    el?.scrollIntoView({ block: "center" });
  }, [focus?.key, view.unit.index]);

  // 划选：选区两头所在的原文段落，中间的都算
  const onMouseUp = () => {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed) return;
    const seqOf = (n: Node | null) => {
      const el = (n instanceof Element ? n : n?.parentElement)?.closest("[data-seq][data-side='en']");
      return el ? Number(el.getAttribute("data-seq")) : null;
    };
    const a = seqOf(sel.anchorNode);
    const b = seqOf(sel.focusNode);
    if (a === null || b === null) return;
    const [lo, hi] = a <= b ? [a, b] : [b, a];
    setChosen(body.filter((p) => p.seq >= lo && p.seq <= hi).map((p) => p.seq));
  };
  const toggle = (seq: number) => setChosen((c) => (c.includes(seq) ? c.filter((x) => x !== seq) : [...c, seq].sort((x, y) => x - y)));
  const chosenKeys = body.filter((p) => chosen.includes(p.seq)).map((p) => p.key);
  const chapterKeys = body.map((p) => p.key);
  const has = (name: string) => skills.some((s) => s.name === name);

  const unitLabel = `${view.unit.key} ${view.unit.label}`;
  const selectionRequest = (skill: string): ConceptRunRequest => ({ textUid, skill, entry: "selection", paragraphs: chosenKeys });
  const chapterRequest: ConceptRunRequest = { textUid, skill: "chapter", entry: "chapter", unit: view.unit.key, paragraphs: chapterKeys };

  const row = (p: ConceptPara) => {
    const zh = p.partner !== null ? zhBySeq.get(p.partner) : undefined;
    const apparatus = zh ? view.translation.filter((x) => x.attached === zh.seq && (x.role === "sidenote" || x.role === "note")) : [];
    const ownNotes = view.paragraphs.filter((x) => x.attached === p.seq && (x.role === "sidenote" || x.role === "note"));
    const picked = chosen.includes(p.seq);
    return (
      <div key={p.seq} className={`grid gap-x-4 border-b border-border/50 py-2 ${showEn && showZh ? "grid-cols-[64px_minmax(0,1fr)_minmax(0,1fr)]" : "grid-cols-[64px_minmax(0,1fr)]"} ${picked ? "bg-accent/40" : ""} ${focus?.key === p.key ? "ring-1 ring-primary/50" : ""}`}>
        <div className="pt-0.5 text-right">
          <button onClick={() => toggle(p.seq)} title={picked ? "不选这一段" : "选这一段"} className={`rounded px-1 text-[11px] font-medium tabular-nums ${picked ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted"}`}>
            {p.key}
          </button>
          {pages && p.page && <div className="mt-0.5 text-[10px] text-muted-foreground tabular-nums">p.{p.page}</div>}
        </div>
        {showEn && (
          <div data-seq={p.seq} data-side="en" data-key={p.key} className="selectable font-serif text-[14px] leading-relaxed">
            <Body p={p} focus={focus} pages={pages} />
            {ownNotes.map((n) => (
              <Apparatus key={n.seq} p={n} />
            ))}
          </div>
        )}
        {showZh && (
          <div data-key={zh ? `译:${zh.key}` : undefined} className="selectable text-[14px] leading-relaxed">
            {zh ? (
              <>
                <Body p={zh} focus={focus && focus.key === `译:${zh.key}` ? { ...focus, key: zh.key } : null} pages={false} />
                {apparatus.map((n) => (
                  <Apparatus key={n.seq} p={n} />
                ))}
              </>
            ) : (
              <span className="text-xs text-muted-foreground">（没有对齐的中译）</span>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <Page
      crumbs={[{ label: "概念" }, { label: unitLabel }]}
      wide={mode === "both" && hasZh}
      actions={
        <>
          {hasZh && (
            <div className="flex overflow-hidden rounded-md border text-xs">
              {(["both", "en", "zh"] as Mode[]).map((m) => (
                <button key={m} onClick={() => setMode(m)} className={`px-2 py-1 ${mode === m ? "bg-accent text-accent-foreground" : "hover:bg-muted"}`}>
                  {m === "both" ? "中英并排" : m === "en" ? "原文" : "中译"}
                </button>
              ))}
            </div>
          )}
        </>
      }
    >
      <div className="flex flex-wrap gap-2 text-xs">
        {has("chapter") && (
          <button onClick={() => onRequest(chapterRequest)} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-primary-foreground">
            <BookText size={13} />
            整章分析…
          </button>
        )}
        {has("ask") && (
          <button onClick={() => setDialog("ask")} className="flex items-center gap-1 rounded-md border px-3 py-1.5 hover:bg-muted">
            <MessageCircleQuestion size={13} />
            问这本书…
          </button>
        )}
        {has("trace") && (
          <button onClick={() => setDialog("trace")} className="flex items-center gap-1 rounded-md border px-3 py-1.5 hover:bg-muted">
            <Route size={13} />
            追踪概念…
          </button>
        )}
        {has("free") && (
          <button onClick={() => setDialog("free")} className="flex items-center gap-1 rounded-md border px-3 py-1.5 hover:bg-muted">
            <Sparkles size={13} />
            自由…
          </button>
        )}
        <button onClick={onSettings} className="ml-auto text-muted-foreground hover:text-foreground hover:underline">
          模型与费用设置
        </button>
      </div>
      <p className="text-xs text-muted-foreground">
        {body.length} 段正文{view.unit.sidenotes + view.unit.notes > 0 ? `、${view.unit.sidenotes} 条旁注、${view.unit.notes} 条注释` : ""}
        {pages && view.unit.pages ? `，第 ${view.unit.pages[0]}–${view.unit.pages[1]} 页` : "，这个版本没有页码（只给段落号）"}
        。划选或点段落号选几段，再选技能。
      </p>
      {hasZh && !view.aligned && (
        <Notice title={`这一章中英正文段数不同（原文 ${body.length} 段 / 中译 ${view.unit.otherCount ?? "?"} 段），没有对齐。`}>中译单独看：右上角切到「中译」。卡片里的原文句不带中译句。</Notice>
      )}
      {showZh && guides.length > 0 && (
        <details className="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs">
          <summary className="cursor-pointer font-medium">
            译本的导读（{guides.length} 段） <Badge>不是原著正文</Badge>
          </summary>
          <div className="selectable mt-2 space-y-1.5 leading-relaxed text-muted-foreground">
            {guides.map((g) => (
              <p key={g.seq}>
                <span className="mr-1 text-[10px] tabular-nums">{g.key}</span>
                {g.text}
              </p>
            ))}
          </div>
        </details>
      )}

      <div ref={box} onMouseUp={onMouseUp}>
        {mode === "zh" && hasZh && !view.aligned
          ? view.translation
              .filter((p) => p.role === "body")
              .map((p) => (
                <div key={p.seq} className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-4 border-b border-border/50 py-2">
                  <span className="pt-0.5 text-right text-[11px] text-muted-foreground tabular-nums">{p.key}</span>
                  <div className="selectable text-[14px] leading-relaxed">
                    {p.text}
                    {view.translation
                      .filter((x) => x.attached === p.seq && (x.role === "sidenote" || x.role === "note"))
                      .map((n) => (
                        <Apparatus key={n.seq} p={n} />
                      ))}
                  </div>
                </div>
              ))
          : view.paragraphs.map((p) =>
              p.role === "heading" ? (
                <h2 key={p.seq} className="mt-4 mb-1 text-center text-sm font-semibold tracking-wide whitespace-pre-line">
                  {p.text}
                </h2>
              ) : p.role === "body" ? (
                row(p)
              ) : p.attached === null && (p.role === "note" || p.role === "sidenote") ? (
                <div key={p.seq} className="ml-[80px]">
                  <Apparatus p={p} />
                </div>
              ) : null,
            )}
      </div>

      {chosen.length > 0 && (
        <div className="sticky bottom-3 z-10 flex flex-wrap items-center gap-2 rounded-lg border bg-card px-3 py-2 text-xs shadow-lg">
          <span>
            已选 {chosen.length} 段（{chosenKeys[0]}
            {chosenKeys.length > 1 ? ` – ${chosenKeys[chosenKeys.length - 1]}` : ""}）
          </span>
          {has("passage") && (
            <button onClick={() => onRequest(selectionRequest("passage"))} className="rounded-md bg-primary px-2.5 py-1 text-primary-foreground">
              读一段…
            </button>
          )}
          {has("counterfactual") && (
            <button onClick={() => onRequest(selectionRequest("counterfactual"))} className="rounded-md border px-2.5 py-1 hover:bg-muted">
              假设推演…
            </button>
          )}
          {skills
            .filter((s) => s.origin === "user" && !s.overridesBuiltin && s.entries.includes("selection"))
            .map((s) => (
              <button key={s.name} onClick={() => onRequest(selectionRequest(s.name))} className="rounded-md border px-2.5 py-1 hover:bg-muted" title={s.description}>
                {s.title}…
              </button>
            ))}
          {has("free") && (
            <button onClick={() => setDialog("free")} className="rounded-md border px-2.5 py-1 hover:bg-muted">
              自由…
            </button>
          )}
          <button onClick={() => setChosen([])} className="ml-auto rounded p-1 text-muted-foreground hover:bg-muted" title="不选了">
            <X size={13} />
          </button>
        </div>
      )}

      {dialog === "ask" && <AskDialog textUid={textUid} onClose={() => setDialog(null)} onRequest={(r) => (setDialog(null), onRequest(r))} />}
      {dialog === "trace" && <TraceDialog textUid={textUid} onClose={() => setDialog(null)} onRequest={(r) => (setDialog(null), onRequest(r))} />}
      {dialog === "free" && (
        <FreeDialog
          base={chosen.length ? { ...selectionRequest("free"), entry: "selection" } : { ...chapterRequest, skill: "free" }}
          label={chosen.length ? `划选的 ${chosen.length} 段` : unitLabel}
          onClose={() => setDialog(null)}
          onRequest={(r) => (setDialog(null), onRequest(r))}
        />
      )}
    </Page>
  );
}

/** 读一章（加载中、出错的外壳） */
export function UnitScreen({ textUid, unit, skills, focus, onRequest, onSettings }: { textUid: string; unit: number; skills: ConceptSkill[]; focus: Focus; onRequest: (r: ConceptRunRequest) => void; onSettings: () => void }) {
  const q = useQuery({ queryKey: ["conceptUnit", textUid, unit], queryFn: () => api.conceptUnitGet(textUid, unit) });
  if (q.error) return <p className="selectable m-6 text-xs text-destructive">{String(q.error)}</p>;
  if (!q.data) return <LoaderCircle size={16} className="m-6 animate-spin text-muted-foreground" />;
  return <Reader view={q.data} skills={skills} focus={focus} onRequest={onRequest} onSettings={onSettings} />;
}
