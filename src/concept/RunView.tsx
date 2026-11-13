// 一次概念研究的结果（decisions/0024「一次概念研究」第 6、7 步）：
// - 卡片按种类筛；每句标类别（原文 / 他人 / 解释 / 推演），「没挂上原文」的醒目标出；点锚点看那一段原文、引文高亮，「去原文」跳回章里；
// - 关系图（和 HTML 导出同一份 SVG，点节点跳到卡片）；
// - 导出三种；网上的评论来源一键加入文献库；接着做 / 重跑 / 停止；审计有问题时顶上标红。
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { Download, FileText, LoaderCircle, Play, RotateCcw, Square, Trash2 } from "lucide-react";
import { api, type ConceptAnchoredParagraph, type ConceptCard, type ConceptCardKind, type ConceptExportFormat, type ConceptProgress, type ConceptRunRequest, type ConceptRunView, type ConceptSentence } from "@/shared/ipc";
import { Modal, useUi } from "@/shared/ui";
import { Badge, Notice, Page } from "@/format/parts";
import { money } from "@/research/labels";
import { CARD_KIND, CARD_KINDS, CATEGORY, CATEGORY_HINT, CATEGORY_TONE, RELATION, RUN_STATUS, RUN_TONE, anchorLabel, shown } from "@/concept/labels";
import type { Pending } from "@/concept/Confirm";
import type { Focus } from "@/concept/Reader";

function Tag({ s }: { s: ConceptSentence }) {
  return (
    <span title={CATEGORY_HINT[s.category]} className={`mr-1.5 inline-block rounded px-1 align-[1px] text-[10px] leading-relaxed ${CATEGORY_TONE[s.category]}`}>
      {CATEGORY[s.category]}
    </span>
  );
}

function Highlighted({ p, start, end }: { p: ConceptAnchoredParagraph; start?: number; end?: number }) {
  const chars = Array.from(p.text);
  if (start === undefined || end === undefined) return <>{p.text}</>;
  return (
    <>
      {chars.slice(0, start).join("")}
      <mark className="rounded bg-yellow-200 px-0.5 dark:bg-yellow-700/60">{chars.slice(start, end).join("")}</mark>
      {chars.slice(end).join("")}
    </>
  );
}

function Sentence({ s, onAnchor }: { s: ConceptSentence; onAnchor: (s: ConceptSentence) => void }) {
  const a = anchorLabel(s);
  return (
    <li className={`leading-relaxed ${s.category === "speculation" ? "italic" : ""}`}>
      <Tag s={s} />
      <span className={s.category === "original" ? "font-serif" : ""}>{shown(s)}</span>
      {s.category === "speculation" && s.quote && <span className="text-muted-foreground not-italic">（前提：“{s.quote}”）</span>}
      {a && (
        <button onClick={() => onAnchor(s)} className="ml-1 text-xs text-primary tabular-nums hover:underline" title="看原文">
          （{a}）
        </button>
      )}
      {s.check.status === "unanchored" && (
        <span title={s.check.note ?? ""} className="ml-1 rounded bg-destructive/10 px-1 text-[11px] font-medium text-destructive not-italic">
          没挂上原文
        </span>
      )}
      {s.category === "original" && s.check.zhStatus === "verified" && <span className="block text-[13px] text-muted-foreground">中译：“{s.zh}”（{s.check.zhParagraph}）</span>}
      {s.category === "original" && s.check.zhStatus === "paragraph_only" && <span className="block text-[11px] text-muted-foreground">中译句核不上，点锚点看整段译文（{s.check.zhParagraph}）</span>}
      {s.category === "original" && s.check.zhStatus === "unaligned" && <span className="block text-[11px] text-muted-foreground">这一章中英段数不同，没有对齐的中译</span>}
    </li>
  );
}

function CardBox({ card, active, onAnchor, onFollow }: { card: ConceptCard; active: boolean; onAnchor: (s: ConceptSentence) => void; onFollow?: (skill: string, card: ConceptCard) => void }) {
  return (
    <article id={`card-${card.id}`} className={`rounded-lg border bg-card px-4 py-3 ${active ? "ring-2 ring-primary/60" : ""}`}>
      <header className="mb-1.5 flex items-start gap-2">
        <span className="pt-0.5 text-[11px] font-medium text-muted-foreground tabular-nums">{card.id}</span>
        <h3 className="min-w-0 flex-1 text-sm font-semibold">{card.title}</h3>
        <Badge>{CARD_KIND[card.kind]}</Badge>
      </header>
      {card.about.length > 0 && <p className="mb-1 text-xs text-muted-foreground">针对：{card.about.join("、")}</p>}
      <ul className="space-y-1 text-[13px]">
        {card.sentences.map((s, i) => (
          <Sentence key={i} s={s} onAnchor={onAnchor} />
        ))}
      </ul>
      {onFollow && (card.kind === "concept" || card.kind === "argument") && (
        <div className="mt-2 flex gap-2 text-xs">
          <button onClick={() => onFollow("critics", card)} className="rounded-md border px-2 py-0.5 hover:bg-muted">
            反方与后世评论…
          </button>
          {card.kind === "argument" && (
            <button onClick={() => onFollow("counterfactual", card)} className="rounded-md border px-2 py-0.5 hover:bg-muted">
              假设推演…
            </button>
          )}
        </div>
      )}
    </article>
  );
}

/** 范围的一句话：问题、检索词、章、段落、卡片 */
function scopeLabel(r: ConceptRunView["run"]): string {
  const s = r.scope;
  if (!s) return "";
  if (s.question) return s.question;
  if (s.terms.length) return s.terms.join(" / ");
  if (s.unit) return `第 ${s.unit} 章`;
  if (s.paragraphs.length) return s.paragraphs.length > 1 ? `${s.paragraphs[0]} – ${s.paragraphs[s.paragraphs.length - 1]}` : s.paragraphs[0];
  return r.targets.map((c) => c.title).join("、");
}

export function RunScreen({ uid, card, onCard, onOpenText, onPrepare, onDeleted }: { uid: string; card: string | null; onCard: (c: string | null) => void; onOpenText: (unit: number, focus: Focus) => void; onPrepare: (p: Promise<Pending>) => void; onDeleted: () => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const q = useQuery({ queryKey: ["conceptRun", uid], queryFn: () => api.conceptRunGet(uid) });
  const [progress, setProgress] = useState<ConceptProgress | null>(null);
  const [filter, setFilter] = useState<ConceptCardKind | "all">("all");
  const [anchor, setAnchor] = useState<ConceptSentence | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const graph = useRef<HTMLDivElement>(null);
  const v = q.data;
  const running = v?.running || v?.run.status === "running";

  useEffect(() => {
    const p = listen<ConceptProgress>("concept-progress", (e) => e.payload.uid === uid && setProgress(e.payload));
    const d = listen<{ uid: string; error: string | null }>("concept-done", (e) => {
      if (e.payload.uid !== uid) return;
      setProgress(null);
      if (e.payload.error) ui.notify(e.payload.error, "error");
      qc.invalidateQueries({ queryKey: ["conceptRun", uid] });
      qc.invalidateQueries({ queryKey: ["conceptRuns"] });
    });
    return () => {
      p.then((f) => f());
      d.then((f) => f());
    };
  }, [uid]);

  // 点关系图的节点：跳到卡片
  useEffect(() => {
    const el = graph.current;
    if (!el) return;
    const click = (e: MouseEvent) => {
      const node = (e.target as Element).closest("[data-card]");
      if (node) onCard(node.getAttribute("data-card"));
    };
    el.addEventListener("click", click);
    el.querySelectorAll(".node").forEach((n) => n.classList.toggle("active", n.getAttribute("data-card") === card));
    return () => el.removeEventListener("click", click);
  }, [v?.svg, card]);
  useEffect(() => {
    if (card) document.getElementById(`card-${card}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [card, v?.run.cards.length]);

  if (q.error) return <p className="selectable m-6 text-xs text-destructive">{String(q.error)}</p>;
  if (!v) return <LoaderCircle size={16} className="m-6 animate-spin text-muted-foreground" />;
  const r = v.run;
  const cards = r.cards.filter((c) => filter === "all" || c.kind === filter);
  const scope = r.scope;

  const exportAs = async (format: ConceptExportFormat) => {
    const dir = await api.pickFolder("导出到哪个文件夹");
    if (!dir) return;
    try {
      const path = await api.conceptExport(uid, format, dir);
      ui.notify(`已导出：${path}`);
      void api.researchReveal(path).catch(() => {});
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };
  const act = async (what: string, f: () => Promise<void>) => {
    setBusy(what);
    try {
      await f();
    } catch (e) {
      ui.notify(String(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const remove = async () => {
    if (!(await ui.confirm({ title: "删除这次研究？", message: `${r.cards.length} 张卡片会一起删掉。`, confirm: "删除", danger: true }))) return;
    await act("delete", async () => {
      await api.conceptRunDelete(uid);
      qc.invalidateQueries({ queryKey: ["conceptRuns"] });
      onDeleted();
    });
  };
  const follow = (skill: string, c: ConceptCard) => {
    if (!scope) return;
    const request: ConceptRunRequest = { textUid: v.textUid, skill, entry: "card", cards: [[uid, c.id]] };
    onPrepare(api.conceptRunPrepare(request).then((preview) => ({ request, preview })));
  };
  const addToLibrary = () =>
    act("library", async () => {
      const res = await api.conceptAddToLibrary(uid, [...picked]);
      qc.setQueryData(["conceptRun", uid], res.view);
      setPicked(new Set());
      const bad = res.added.filter((a) => a.attachmentError);
      ui.notify(`已加入文献库 ${res.added.length} 条${bad.length ? `（${bad.length} 条没带上原件）` : ""}`);
    });
  const para = anchor?.paragraph ? v.paragraphs.find((p) => p.key === anchor.paragraph) : undefined;
  const zhPara = anchor?.check.zhParagraph ? v.paragraphs.find((p) => p.key === `译:${anchor.check.zhParagraph}`) : undefined;
  const sources = r.sources;

  return (
    <Page
      crumbs={[{ label: "概念" }, { label: `${r.skillTitle}：${scopeLabel(r)}` }]}
      actions={
        <>
          {r.mock && <Badge tone="warn">模拟</Badge>}
          <Badge tone={RUN_TONE[r.status]}>{RUN_STATUS[r.status]}</Badge>
          {running ? (
            <button onClick={() => void api.conceptCancel()} className="flex items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-muted">
              <Square size={12} />
              停止
            </button>
          ) : (
            <>
              {r.status !== "done" && (
                <button onClick={() => onPrepare(api.conceptRunResumePrepare(uid).then((preview) => ({ request: null, preview })))} className="flex items-center gap-1 rounded-md bg-primary px-2 py-1 text-xs text-primary-foreground">
                  <Play size={12} />
                  继续…
                </button>
              )}
              {r.status === "done" && (
                <button onClick={() => onPrepare(api.conceptRerunPrepare(uid).then((preview) => ({ request: null, preview })))} title="同一技能对同一范围再跑一次（旧结果保留）" className="flex items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-muted">
                  <RotateCcw size={12} />
                  重跑…
                </button>
              )}
              <button onClick={() => void remove()} disabled={!!busy} title="删除这次研究" className="rounded-md border p-1 text-muted-foreground hover:bg-muted">
                <Trash2 size={13} />
              </button>
            </>
          )}
        </>
      }
    >
      {v.findings.length > 0 && <Notice tone="danger" title="审计发现问题：存下的结果和存下的原文对不上。" items={v.findings.map((f) => `${f.at}${f.sentence ? ` 第 ${f.sentence} 句` : ""}：${f.message}`)} />}
      {r.error && <Notice tone="danger" title={r.error} />}
      {running && (
        <div className="flex items-center gap-2 rounded-md border px-3 py-2 text-xs">
          <LoaderCircle size={14} className="animate-spin text-muted-foreground" />
          <span>{progress?.message ?? "正在做…"}</span>
          {progress && <span className="text-muted-foreground tabular-nums">（第 {Math.min(progress.done + 1, progress.total)} / {progress.total} 步）</span>}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span>
          {r.cards.length} 张卡片、{r.relations.length} 条关系{r.rejected.length > 0 && <span className="text-destructive">；核对拦下 {r.rejected.length} 句</span>}
        </span>
        <span className="tabular-nums">费用 {r.mock ? "（模拟，不花钱）" : money(v.costUsd)}</span>
        <span>{r.model.model}</span>
        <span className="flex gap-1">
          {(["markdown", "html", "docx"] as ConceptExportFormat[]).map((f) => (
            <button key={f} onClick={() => void exportAs(f)} disabled={r.cards.length === 0} className="flex items-center gap-1 rounded-md border px-2 py-0.5 text-foreground hover:bg-muted disabled:opacity-50">
              {f === "docx" ? <FileText size={12} /> : <Download size={12} />}
              {f === "markdown" ? "Markdown" : f === "html" ? "HTML" : "Word"}
            </button>
          ))}
        </span>
      </div>

      {v.svg && (
        <section>
          <h2 className="mb-1 text-xs font-medium text-muted-foreground">关系图（点节点跳到卡片）</h2>
          <div ref={graph} className="overflow-x-auto rounded-lg border bg-card p-2 [&_svg]:h-auto [&_svg]:max-w-full [&_.node.active_rect]:stroke-[3]" dangerouslySetInnerHTML={{ __html: v.svg }} />
        </section>
      )}

      <section className="space-y-3">
        <div className="flex flex-wrap gap-1 text-xs">
          {(["all", ...CARD_KINDS] as const).map((k) => {
            const n = k === "all" ? r.cards.length : r.cards.filter((c) => c.kind === k).length;
            if (n === 0 && k !== "all") return null;
            return (
              <button key={k} onClick={() => setFilter(k)} className={`rounded-full border px-2.5 py-0.5 ${filter === k ? "border-primary text-primary" : "hover:bg-muted"}`}>
                {k === "all" ? "全部" : CARD_KIND[k]}（{n}）
              </button>
            );
          })}
          <span className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
            {(["original", "others", "explanation", "speculation"] as const).map((c) => (
              <span key={c} className={`rounded px-1 ${CATEGORY_TONE[c]}`} title={CATEGORY_HINT[c]}>
                {CATEGORY[c]}
              </span>
            ))}
          </span>
        </div>
        {r.targets.length > 0 && (
          <details className="rounded-md border px-3 py-2 text-xs">
            <summary className="cursor-pointer text-muted-foreground">入口选的卡片（{r.targets.length}）</summary>
            <div className="mt-2 space-y-2">
              {r.targets.map((c) => (
                <CardBox key={c.id} card={c} active={false} onAnchor={setAnchor} />
              ))}
            </div>
          </details>
        )}
        {cards.length === 0 && !running && <p className="text-xs text-muted-foreground">{r.status === "done" ? "这次没有做出卡片。" : "还没有卡片。"}</p>}
        {cards.map((c) => (
          <CardBox key={c.id} card={c} active={card === c.id} onAnchor={setAnchor} onFollow={r.status === "done" ? follow : undefined} />
        ))}
      </section>

      {r.relations.length > 0 && (
        <section>
          <h2 className="mb-1 text-xs font-medium text-muted-foreground">关系</h2>
          <table className="w-full text-left text-xs">
            <tbody>
              {r.relations.map((x) => (
                <tr key={x.id} className="border-b border-border/60 align-top">
                  <td className="py-1 pr-2 whitespace-nowrap">
                    <button onClick={() => onCard(x.from)} className="text-primary hover:underline">
                      {x.from}
                    </button>{" "}
                    → {RELATION[x.kind]} →{" "}
                    <button onClick={() => onCard(x.to)} className="text-primary hover:underline">
                      {x.to}
                    </button>
                  </td>
                  <td className="py-1">
                    <ul>
                      <Sentence s={x.note} onAnchor={setAnchor} />
                    </ul>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {sources.length > 0 && (
        <section>
          <h2 className="mb-1 text-xs font-medium text-muted-foreground">评论的来源</h2>
          <ul className="space-y-1 text-xs">
            {sources.map((s) => (
              <li key={s.id} className="flex items-start gap-2">
                {s.origin === "web" && !s.error && !s.addedItem ? (
                  <input type="checkbox" className="mt-0.5" checked={picked.has(s.id)} onChange={() => setPicked((p) => (p.has(s.id) ? new Set([...p].filter((x) => x !== s.id)) : new Set([...p, s.id])))} />
                ) : (
                  <span className="w-[13px]" />
                )}
                <span className="font-medium tabular-nums">{s.id}</span>
                <span className="min-w-0 flex-1">
                  {s.title || s.url}
                  <span className="ml-1 text-muted-foreground">{s.origin === "library" ? "（文献库）" : s.url}</span>
                  {s.error && <span className="ml-1 text-destructive">{s.error}</span>}
                  {s.addedItem && <Badge tone="ok">已在文献库</Badge>}
                </span>
              </li>
            ))}
          </ul>
          {picked.size > 0 && (
            <button onClick={() => void addToLibrary()} disabled={busy === "library"} className="mt-2 rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50">
              把选的 {picked.size} 个网页加入文献库
            </button>
          )}
        </section>
      )}

      {r.rejected.length > 0 && (
        <details className="rounded-md border px-3 py-2 text-xs">
          <summary className="cursor-pointer">核对报告：拦下 {r.rejected.length} 句（不在卡片里）</summary>
          <table className="mt-2 w-full text-left">
            <tbody>
              {r.rejected.map((x, i) => (
                <tr key={i} className="border-b border-border/60 align-top">
                  <td className="py-1 pr-2 whitespace-nowrap text-muted-foreground">{x.card}</td>
                  <td className="py-1 pr-2">
                    <Tag s={x.sentence} />
                    {x.sentence.text}
                  </td>
                  <td className="py-1 text-destructive">{x.reason}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}

      <details className="rounded-md border px-3 py-2 text-xs">
        <summary className="cursor-pointer text-muted-foreground">步骤与费用（{r.steps.length} 步）</summary>
        <table className="mt-2 w-full text-left">
          <tbody>
            {r.steps.map((s, i) => (
              <tr key={i} className="border-b border-border/60">
                <td className="py-1 pr-2">{s.label}</td>
                <td className="py-1 pr-2 text-muted-foreground">{s.provider ? `${s.provider} ${s.model}` : "本机"}</td>
                <td className="py-1 pr-2 text-right text-muted-foreground tabular-nums">{s.inputTokens + s.outputTokens > 0 ? `${s.inputTokens.toLocaleString()} / ${s.outputTokens.toLocaleString()}` : s.searches > 0 ? `搜索 ${s.searches} 次` : ""}</td>
                <td className="py-1 pr-2 text-right tabular-nums">{s.mock ? "模拟" : money(s.costUsd)}</td>
                <td className={`py-1 ${s.ok ? "text-muted-foreground" : "text-destructive"}`}>{s.note}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>

      {anchor && (
        <Modal title={`原文：${anchorLabel(anchor) ?? ""}`} onClose={() => setAnchor(null)} width="w-[640px]">
          <div className="space-y-3 text-[13px] leading-relaxed">
            {para ? (
              <p className="selectable font-serif">
                <Highlighted p={para} start={anchor.check.span?.start} end={anchor.check.span?.end} />
              </p>
            ) : anchor.source ? (
              <p className="text-xs text-muted-foreground">来源 {anchor.source} 的正文在快照里；导出 HTML 可以看。</p>
            ) : (
              <p className="text-xs text-muted-foreground">找不到这一段。</p>
            )}
            {zhPara && (
              <p className="selectable border-t pt-2 text-muted-foreground">
                <span className="mr-1 text-[11px]">中译 {anchor.check.zhParagraph}</span>
                <Highlighted p={zhPara} start={anchor.check.zhSpan?.start} end={anchor.check.zhSpan?.end} />
              </p>
            )}
            <div className="flex justify-end gap-2 text-xs">
              {para && !para.translation && (
                <button
                  onClick={() => {
                    setAnchor(null);
                    onOpenText(para.unit, { key: para.key, start: anchor.check.span?.start, end: anchor.check.span?.end });
                  }}
                  className="rounded-md bg-primary px-3 py-1 text-primary-foreground"
                >
                  去原文
                </button>
              )}
              <button onClick={() => setAnchor(null)} className="rounded-md border px-3 py-1 hover:bg-muted">
                关闭
              </button>
            </div>
          </div>
        </Modal>
      )}
    </Page>
  );
}

export type { ConceptRunView };
