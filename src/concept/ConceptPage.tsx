// 概念研究（v0.4，decisions/0024）：读一本书 → 选入口（整章 / 划选 / 问题 / 追踪 / 卡片）和技能 → 确认要发什么、多少钱 →
// 后台按技能的步骤跑（模型 / 查文献库 / 网页检索）→ 代码逐字核对每句引文 → 卡片、关系图、导出。
// 规则都在 Rust（crates/concept-engine、src-tauri/src/concept/）；这里只管界面。
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpenText, LoaderCircle } from "lucide-react";
import { api, type ConceptRunRequest } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Badge, Notice, Page, SectionTitle } from "@/format/parts";
import { money } from "@/research/labels";
import { MODEL_NAME } from "@/concept/labels";
import { RunConfirm, prepare, type Pending } from "@/concept/Confirm";
import { UnitScreen, type Focus } from "@/concept/Reader";
import { RunScreen } from "@/concept/RunView";

/** 概念页看到哪：哪本书、哪一章、哪次研究、哪张卡片 */
export type ConceptAt = { text: string | null; unit: number | null; run: string | null; card: string | null; focus?: Focus };

export function ConceptPage({ at, onAt, onSettings }: { at: ConceptAt; onAt: (at: ConceptAt) => void; onSettings: () => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const status = useQuery({ queryKey: ["conceptStatus"], queryFn: api.conceptStatus, staleTime: 0 });
  const [pending, setPending] = useState<Pending | null>(null);
  const [preparing, setPreparing] = useState(false);
  const skills = status.data?.skills ?? [];

  const ask = async (p: Promise<Pending>) => {
    setPreparing(true);
    try {
      setPending(await p);
    } catch (e) {
      ui.notify(String(e), "error");
    } finally {
      setPreparing(false);
    }
  };
  const request = (r: ConceptRunRequest) => void ask(prepare(r));
  const sent = (uid: string) => {
    setPending(null);
    qc.invalidateQueries({ queryKey: ["conceptRuns"] });
    qc.invalidateQueries({ queryKey: ["conceptRun", uid] });
    qc.invalidateQueries({ queryKey: ["conceptStatus"] });
    onAt({ ...at, run: uid, card: null });
  };

  let body: React.ReactNode;
  if (at.run) {
    body = (
      <RunScreen
        key={at.run}
        uid={at.run}
        card={at.card}
        onCard={(card) => onAt({ ...at, card })}
        onOpenText={(unit, focus) => onAt({ ...at, unit, run: null, card: null, focus })}
        onPrepare={(p) => void ask(p)}
        onDeleted={() => onAt({ ...at, run: null, card: null })}
      />
    );
  } else if (at.text && at.unit !== null) {
    body = <UnitScreen key={`${at.text}-${at.unit}`} textUid={at.text} unit={at.unit} skills={skills} focus={at.focus ?? null} onRequest={request} onSettings={onSettings} />;
  } else if (at.text) {
    body = <BookHome uid={at.text} onUnit={(unit) => onAt({ ...at, unit, run: null, card: null })} />;
  } else {
    body = <Shelf onOpen={(uid) => onAt({ text: uid, unit: null, run: null, card: null })} onSettings={onSettings} />;
  }
  return (
    <>
      {body}
      {preparing && (
        <div className="fixed right-4 bottom-4 z-40 flex items-center gap-2 rounded-md border bg-card px-3 py-2 text-xs shadow">
          <LoaderCircle size={13} className="animate-spin" />
          估算要发什么、多少钱…
        </div>
      )}
      {pending && <RunConfirm pending={pending} skill={skills.find((s) => s.name === pending.preview.skill)} onSent={sent} onClose={() => setPending(null)} onSettings={onSettings} />}
    </>
  );
}

/** 没选书：解析过的书、怎么加书 */
function Shelf({ onOpen, onSettings }: { onOpen: (uid: string) => void; onSettings: () => void }) {
  const texts = useQuery({ queryKey: ["conceptTexts"], queryFn: api.conceptTextsList, staleTime: 0 });
  const status = useQuery({ queryKey: ["conceptStatus"], queryFn: api.conceptStatus, staleTime: 0 });
  const s = status.data;
  const books = (texts.data ?? []).filter((t) => !t.originalUid);
  return (
    <Page crumbs={[{ label: "概念" }, { label: "书" }]}>
      <div className="space-y-2 text-xs text-muted-foreground">
        <p>
          读一本经典著作：书是文献库里的一条文献，它的 EPUB（或短 PDF）附件「解析成文本」后在这里读。原文带段落号（有页码的版本也标页码），中英并排；
          划选一段、整章、问一个问题、追踪一个概念，交给技能做成卡片。<span className="text-foreground">卡片里每句「原文」都由 Pith 在存下的原文里逐字核对</span>，
          解释、推演、他人的话分开标明。
        </p>
        {s && !s.mock && (
          <p className="tabular-nums">
            模型 {MODEL_NAME[s.settings.model] ?? s.settings.model}；本月已花 {money(s.monthSpentUsd)} / 上限 {money(s.budgetUsd)}（和排版、调查共用）。
          </p>
        )}
      </div>
      {s?.mock && <Notice tone="info" title="现在是模拟接口：不联网、不花钱，卡片来自录制的回复（公有领域的英文夹具 + 合成的中文）。" />}
      {s && !s.mock && s.keys.some((k) => k.needed && !k.has) && (
        <Notice title={`还没有填 ${s.keys.filter((k) => k.needed && !k.has).map((k) => k.label).join("、")} 的密钥。`}>
          <button onClick={onSettings} className="text-primary hover:underline">
            去「设置 › 密钥与费用」填写
          </button>
        </Notice>
      )}
      <section>
        <SectionTitle>解析过的书</SectionTitle>
        {texts.error && <p className="selectable text-xs text-destructive">{String(texts.error)}</p>}
        {texts.data && books.length === 0 && (
          <p className="rounded-md border border-dashed px-3 py-4 text-xs text-muted-foreground">
            还没有。到文献库里选一条有 EPUB 或 PDF 附件的文献，在附件上点「解析成文本」：先看结构预览（各章段数、正文 / 旁注 / 注释 / 导读的数量、有没有页码），确认后才存。
            原著和译本各是一条文献，存译本时选它对应的原著，两份文本按段落对齐。
          </p>
        )}
        <ul className="space-y-2">
          {books.map((b) => {
            const tr = texts.data?.find((t) => t.uid === b.translationUid);
            return (
              <li key={b.uid}>
                <button onClick={() => onOpen(b.uid)} className="flex w-full items-start gap-3 rounded-lg border bg-card px-3 py-2 text-left hover:bg-muted">
                  <BookOpenText size={16} className="mt-0.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium">{b.title || b.itemTitle}</span>
                    <span className="block text-xs text-muted-foreground">
                      {[b.format.toUpperCase(), b.language === "zh" ? "中文" : "英文", b.hasPages ? "有页码" : "没有页码", b.numbering === "glasgow" ? "段落号：篇.章.段" : b.numbering === "chapter" ? "段落号：章.段" : "段落号：段"].join(" · ")}
                    </span>
                    {tr && <span className="block text-xs text-muted-foreground">中译：{tr.title || tr.itemTitle}</span>}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </section>
    </Page>
  );
}

/** 一本书：各章（段数、页码、对没对齐）、译本 */
function BookHome({ uid, onUnit }: { uid: string; onUnit: (unit: number) => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const v = useQuery({ queryKey: ["conceptText", uid], queryFn: () => api.conceptTextGet(uid) });
  const texts = useQuery({ queryKey: ["conceptTexts"], queryFn: api.conceptTextsList });
  if (v.error) return <p className="selectable m-6 text-xs text-destructive">{String(v.error)}</p>;
  if (!v.data) return <LoaderCircle size={16} className="m-6 animate-spin text-muted-foreground" />;
  const d = v.data;
  const chapters = d.units.filter((u) => u.body > 0);
  const orphans = (texts.data ?? []).filter((t) => t.uid !== uid && t.language !== d.summary.language && !t.originalUid && !t.translationUid);
  const link = async (translation: string) => {
    try {
      await api.conceptTextLink(translation, uid);
      qc.invalidateQueries({ queryKey: ["conceptText"] });
      qc.invalidateQueries({ queryKey: ["conceptTexts"] });
      ui.notify("已关联并对齐");
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };
  const remove = async () => {
    if (!(await ui.confirm({ title: "删除这份文本？", message: "解析出来的段落和对它做的研究、卡片都会删掉。文献库里的文献和附件不受影响，可以再解析。", confirm: "删除", danger: true }))) return;
    try {
      await api.conceptTextDelete(uid);
      qc.invalidateQueries({ queryKey: ["conceptTexts"] });
      window.dispatchEvent(new CustomEvent("pith:concept-home"));
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };
  return (
    <Page crumbs={[{ label: "概念" }, { label: d.summary.title || d.summary.itemTitle || "书" }]} actions={<button onClick={() => void remove()} className="rounded-md border px-2 py-1 text-xs text-destructive hover:bg-muted">删除这份文本…</button>}>
      <div className="text-xs text-muted-foreground">
        <p>
          {d.summary.itemTitle && <>文献库：{d.summary.itemTitle} · </>}
          {d.summary.format.toUpperCase()} · {d.summary.hasPages ? `有页码（${d.summary.pageSource}）` : "没有页码，只给段落号"}
          {d.summary.converter && ` · ${d.summary.converter}`}
        </p>
        {d.translation ? (
          <p>
            中译：{d.translation.title || d.translation.itemTitle}；{d.alignedUnits} 章对齐
            {d.mismatchedUnits.length > 0 && <span className="text-amber-700 dark:text-amber-400">，{d.mismatchedUnits.length} 章段数不同没有对齐（{d.mismatchedUnits.slice(0, 8).join("、")}{d.mismatchedUnits.length > 8 ? "…" : ""}）</span>}
          </p>
        ) : (
          orphans.length > 0 && (
            <p className="flex items-center gap-2">
              还没有关联中译。
              {orphans.map((o) => (
                <button key={o.uid} onClick={() => void link(o.uid)} className="rounded-md border px-2 py-0.5 text-foreground hover:bg-muted">
                  把「{o.title || o.itemTitle}」作为它的译本
                </button>
              ))}
            </p>
          )
        )}
        {d.summary.warnings.length > 0 && <Notice title="解析时的提示" items={d.summary.warnings} />}
      </div>
      <section>
        <SectionTitle>各章</SectionTitle>
        <table className="w-full text-left text-xs">
          <thead className="text-muted-foreground">
            <tr className="border-b">
              <th className="py-1 pr-2 font-normal">段落号</th>
              <th className="py-1 pr-2 font-normal">章</th>
              <th className="py-1 pr-2 text-right font-normal">正文</th>
              <th className="py-1 pr-2 text-right font-normal">旁注 / 注释</th>
              <th className="py-1 pr-2 font-normal">页</th>
              <th className="py-1 font-normal">中译</th>
            </tr>
          </thead>
          <tbody>
            {chapters.map((u) => (
              <tr key={u.index} onClick={() => onUnit(u.index)} className="cursor-pointer border-b border-border/60 hover:bg-muted">
                <td className="py-1 pr-2 font-medium tabular-nums">{u.key}</td>
                <td className="py-1 pr-2">{u.label}</td>
                <td className="py-1 pr-2 text-right tabular-nums">{u.body}</td>
                <td className="py-1 pr-2 text-right text-muted-foreground tabular-nums">{u.sidenotes + u.notes > 0 ? `${u.sidenotes} / ${u.notes}` : ""}</td>
                <td className="py-1 pr-2 text-muted-foreground tabular-nums">{u.pages ? `${u.pages[0]}–${u.pages[1]}` : ""}</td>
                <td className="py-1">{u.aligned === null ? "" : u.aligned ? <Badge tone="ok">对齐</Badge> : <Badge tone="warn">{`段数不同（${u.otherCount ?? 0}）`}</Badge>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </Page>
  );
}
