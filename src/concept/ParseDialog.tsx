// 文献库附件上的「解析成文本」（decisions/0024「书与文献」）：先显示结构预览（各章段数、各角色数量、有没有页码），用户确认后才存。
// EPUB 直接解析；PDF 先确认页数和费用（Mathpix 每页 $0.005；超过页数上限提示改用 EPUB；没有 Mathpix 密钥时用本机取文字）。
// 存之前做「导入前」备份；是译本的话选它对应的原著，两份文本按段落对齐。
// 附件已经解析过的：同一个对话框走「重新解析」（就地替换文本，旧卡片按引文重新找锚点），做完给结果摘要。
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type ConceptExisting, type ConceptNumbering, type ConceptPrepared, type ConceptReparseReport, type ConceptReparseResult, type ConceptTextPreview } from "@/shared/ipc";
import { Modal, useUi } from "@/shared/ui";
import { Badge, Notice } from "@/format/parts";
import { money } from "@/research/labels";

const KIND: Record<string, string> = { book: "篇", chapter: "章", section: "节", part: "正文", container: "", guide: "导读", notes: "注释", matter: "前后附" };

function Preview({ p, onNumbering, original, onOriginal }: { p: ConceptTextPreview; onNumbering: (n: ConceptNumbering) => void; original: string | null; onOriginal: (u: string | null) => void }) {
  const pv = p.preview;
  const shown = pv.units.filter((u) => u.counts.body + u.counts.sidenote + u.counts.note + u.counts.guide > 0);
  return (
    <div className="space-y-3 text-xs">
      <div className="grid grid-cols-[88px_minmax(0,1fr)] gap-x-3 gap-y-1">
        <span className="text-muted-foreground">书名</span>
        <span>{pv.title || "（没有书名）"}</span>
        <span className="text-muted-foreground">语言</span>
        <span>{pv.language === "zh" ? "中文" : pv.language === "en" ? "英文" : pv.language}</span>
        <span className="text-muted-foreground">页码</span>
        <span>{pv.hasPages ? `有（来自 ${pv.pageSource}）` : "没有：引文只给段落号"}</span>
        <span className="text-muted-foreground">段落号</span>
        <span className="flex gap-1">
          {(pv.format === "pdf" ? (["plain"] as ConceptNumbering[]) : (["glasgow", "chapter"] as ConceptNumbering[])).map((n) => (
            <button key={n} onClick={() => onNumbering(n)} className={`rounded-md border px-2 py-0.5 ${pv.numbering === n ? "border-primary text-primary" : "hover:bg-muted"}`}>
              {n === "glasgow" ? "篇.章.段（I.iii.5）" : n === "chapter" ? "章.段（3.5）" : "段（5）"}
            </button>
          ))}
        </span>
        <span className="text-muted-foreground">全书</span>
        <span className="tabular-nums">
          正文 {pv.totals.body} 段、标题 {pv.totals.heading}、旁注 {pv.totals.sidenote}、注释 {pv.totals.note}、导读 {pv.totals.guide}、前后附 {pv.totals.matter}；共 {pv.chars.toLocaleString()} 字
        </span>
      </div>
      {pv.warnings.length > 0 && <Notice title="解析时的提示" items={pv.warnings} />}
      {p.existing && <ReparseNotice existing={p.existing} />}
      <div className="max-h-[40vh] overflow-y-auto rounded-md border">
        <table className="w-full text-left">
          <thead className="sticky top-0 bg-card text-muted-foreground">
            <tr className="border-b">
              <th className="px-2 py-1 font-normal">单元</th>
              <th className="px-2 py-1 text-right font-normal">正文</th>
              <th className="px-2 py-1 text-right font-normal">旁注</th>
              <th className="px-2 py-1 text-right font-normal">注释</th>
              <th className="px-2 py-1 text-right font-normal">导读</th>
              <th className="px-2 py-1 font-normal">页</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((u) => (
              <tr key={u.index} className="border-b border-border/60 align-top">
                <td className="px-2 py-1" style={{ paddingLeft: `${(u.depth - 1) * 10 + 8}px` }}>
                  <span className="font-medium tabular-nums">{u.key}</span> <span className="text-muted-foreground">{u.label}</span> {KIND[u.kind] && <Badge>{KIND[u.kind]}</Badge>}
                  {u.first && <span className="line-clamp-1 text-[11px] text-muted-foreground">{u.first}…</span>}
                </td>
                <td className="px-2 py-1 text-right tabular-nums">{u.counts.body || ""}</td>
                <td className="px-2 py-1 text-right tabular-nums">{u.counts.sidenote || ""}</td>
                <td className="px-2 py-1 text-right tabular-nums">{u.counts.note || ""}</td>
                <td className="px-2 py-1 text-right tabular-nums">{u.counts.guide || ""}</td>
                <td className="px-2 py-1 text-muted-foreground tabular-nums">{u.pages ? `${u.pages[0]}–${u.pages[1]}` : ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {!p.existing && p.candidates.length > 0 && (
        <label className="flex items-center gap-2">
          <span className="text-muted-foreground">这是译本？对应的原著：</span>
          <select value={original ?? ""} onChange={(e) => onOriginal(e.target.value || null)} className="rounded-md border bg-background px-2 py-1">
            <option value="">（不是译本 / 以后再关联）</option>
            {p.candidates.map((c) => (
              <option key={c.uid} value={c.uid}>
                {c.title || c.itemTitle}（{c.language === "zh" ? "中文" : "英文"}）
              </option>
            ))}
          </select>
        </label>
      )}
    </div>
  );
}

/** 附件已经解析过：说明重新解析会做什么 */
function ReparseNotice({ existing }: { existing: ConceptExisting }) {
  return (
    <Notice title={`这份附件已经解析过。重新解析会替换文本，并为 ${existing.runs} 次研究、${existing.cards} 张卡片重新找锚点。`}>
      <p className="mt-1 text-muted-foreground">文本本身、它上面的研究和译本关联都保留；每句按引文在新文本里重新定位，找不到的标成「没挂上原文」。替换前自动备份。</p>
    </Notice>
  );
}

const anchorName = (k: string | null) => k ?? "（没有）";

/** 重新解析做完：多少句跟过去了、位置变了、没挂上，点开看是哪些卡片 */
function ReparseSummary({ r }: { r: ConceptReparseReport }) {
  const touched = r.runs.filter((x) => x.changes.length > 0 || x.findings.length > 0 || x.scopeDropped > 0);
  return (
    <div className="space-y-3 text-xs">
      <p>
        文本已经换成新解析的（{r.paragraphs.toLocaleString()} 段）。
        {r.runs.length === 0 ? "这份文本上还没有研究，没有要重新找锚点的卡片。" : `${r.runs.length} 次研究、${r.cards} 张卡片重新找了锚点：`}
      </p>
      {r.runs.length > 0 && (
        <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 tabular-nums">
          <span className="text-right font-medium">{r.kept}</span>
          <span>句原位跟过去了</span>
          <span className="text-right font-medium">{r.moved}</span>
          <span>句位置变了（段落号、位置或页码已经更新）</span>
          <span className={`text-right font-medium ${r.lost > 0 ? "text-destructive" : ""}`}>{r.lost}</span>
          <span>句没挂上原文（在新文本里找不到，卡片上会标出来）</span>
          {r.zhLost > 0 && (
            <>
              <span className="text-right font-medium text-warn">{r.zhLost}</span>
              <span>句的中译核不上了（只显示整段译文，或这一章不再对齐）</span>
            </>
          )}
        </div>
      )}
      {r.runs.length > 0 && (r.findings > 0 ? <Notice tone="danger" title={`重新审计发现 ${r.findings} 个问题，打开对应的研究能看到。`} /> : <p className="text-muted-foreground">重新审计：没有问题。</p>)}
      {touched.length > 0 && (
        <div className="max-h-[40vh] space-y-1 overflow-y-auto rounded-md border p-2">
          {touched.map((x) => (
            <details key={x.uid}>
              <summary className="cursor-pointer">
                <span className="font-medium">{x.skillTitle}</span>
                {x.label && <span className="text-muted-foreground"> · {x.label}</span>}
                <span className="text-muted-foreground tabular-nums">
                  （{x.cards} 张卡片：位置变了 {x.moved}，没挂上 {x.lost}
                  {x.zhLost > 0 && `，中译核不上 ${x.zhLost}`}
                  {x.scopeDropped > 0 && `；范围里有 ${x.scopeDropped} 段找不到了`}）
                </span>
              </summary>
              <ul className="mt-1 space-y-0.5 pl-4">
                {x.changes.map((c, i) => (
                  <li key={i} className="selectable">
                    <span className="font-medium tabular-nums">{c.at}</span> <span className="text-muted-foreground">{c.title}</span>
                    {c.sentence !== null && <span className="text-muted-foreground tabular-nums"> · 第 {c.sentence} 句</span>}：
                    {c.outcome === "lost" ? (
                      <span className="text-destructive" title={c.note ?? ""}>
                        没挂上原文{c.from && `（原来在 ${c.from}）`}
                      </span>
                    ) : c.outcome === "moved" ? (
                      <span className="tabular-nums">{c.from === c.to ? `${anchorName(c.to)}（位置或页码变了）` : `${anchorName(c.from)} → ${anchorName(c.to)}`}</span>
                    ) : (
                      <span className="tabular-nums">{anchorName(c.to)}</span>
                    )}
                    {c.zhLost && <span className="text-warn">；中译核不上了</span>}
                  </li>
                ))}
                {x.findings.map((f, i) => (
                  <li key={`f${i}`} className="selectable text-destructive">
                    审计：{f.at}
                    {f.sentence !== null && ` 第 ${f.sentence} 句`}，{f.message}
                  </li>
                ))}
              </ul>
            </details>
          ))}
        </div>
      )}
    </div>
  );
}

export function ParseDialog({ attachmentId, title, onClose, onSaved }: { attachmentId: number; title: string; onClose: () => void; onSaved?: (uid: string) => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const [prepared, setPrepared] = useState<ConceptPrepared | null>(null);
  const [preview, setPreview] = useState<ConceptTextPreview | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [done, setDone] = useState<ConceptReparseResult | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.conceptTextPrepare(attachmentId).then(
      (p) => {
        setPrepared(p);
        if (p.kind === "epub") setPreview(p);
        setBusy(false);
      },
      (e) => {
        setError(String(e));
        setBusy(false);
      },
    );
  }, [attachmentId]);

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const convert = (local: boolean) => prepared?.kind === "pdf" && run(async () => setPreview(await api.conceptPdfConvert(prepared.ticket, local)));
  const renumber = (n: ConceptNumbering) => preview && run(async () => setPreview(await api.conceptTextRenumber(preview.ticket, n)));
  const save = () =>
    preview &&
    run(async () => {
      const s = await api.conceptTextSave(preview.ticket, original);
      qc.invalidateQueries({ queryKey: ["conceptTexts"] });
      qc.invalidateQueries({ queryKey: ["conceptAttachment", attachmentId] });
      ui.notify(`已存好「${s.title || title}」，到最左一列的「概念」里读`);
      onSaved?.(s.uid);
      onClose();
    });
  // 附件已经解析过：就地替换，做完留在对话框里给结果摘要
  const reparse = () =>
    preview &&
    run(async () => {
      const r = await api.conceptTextReparse(preview.ticket);
      for (const key of ["conceptTexts", "conceptText", "conceptUnit", "conceptRuns", "conceptRun"]) qc.invalidateQueries({ queryKey: [key] });
      qc.invalidateQueries({ queryKey: ["conceptAttachment", attachmentId] });
      setDone(r);
    });

  const pdf = prepared?.kind === "pdf" ? prepared : null;
  if (done)
    return (
      <Modal title={`已重新解析：${done.summary.title || title}`} onClose={onClose} width="w-[680px]">
        <ReparseSummary r={done.report} />
        <div className="mt-3 flex justify-end gap-2 text-xs">
          {onSaved && (
            <button
              onClick={() => {
                onSaved(done.summary.uid);
                onClose();
              }}
              className="rounded-md border px-3 py-1.5 hover:bg-muted"
            >
              在概念页里看
            </button>
          )}
          <button onClick={onClose} className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground">
            完成
          </button>
        </div>
      </Modal>
    );
  const again = (preview?.existing ?? pdf?.existing) != null;
  return (
    <Modal title={`${again ? "重新解析" : "解析成文本"}：${title}`} onClose={onClose} width="w-[680px]">
      {busy && !preview && !pdf && (
        <p className="flex items-center gap-2 text-xs text-muted-foreground">
          <LoaderCircle size={13} className="animate-spin" />
          正在解析…
        </p>
      )}
      {pdf && !preview && (
        <div className="space-y-3 text-xs">
          <p>
            这份 PDF 有 <span className="font-medium tabular-nums">{pdf.pages}</span> 页。
            {pdf.hasMathpix ? (
              <>
                用 Mathpix 转文字：上传 → 转换 → 下载每页的行（带页号）→ 删掉 Mathpix 上的副本。预计 <span className="tabular-nums">{money(pdf.costUsd)}</span>（每页 $0.005）
                {!pdf.mock && <span className="text-muted-foreground tabular-nums">；本月已花 {money(pdf.monthSpentUsd)} / 上限 {money(pdf.budgetUsd)}</span>}。
              </>
            ) : (
              "还没有填 Mathpix 的密钥，只能用本机取文字（段落可能断得不对，中文 PDF 可能是乱码）。"
            )}
          </p>
          {pdf.existing && <ReparseNotice existing={pdf.existing} />}
          {pdf.overLimit && <Notice title={`超过 ${pdf.limit} 页：长书建议改用 EPUB（页数上限在「设置 › 概念研究」里改）。`} />}
          {pdf.mock && <Notice tone="info" title="模拟模式：不真的调用 Mathpix，用本机取的文字拼成行，不花钱。" />}
          <div className="flex justify-end gap-2">
            <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
              取消
            </button>
            <button onClick={() => void convert(true)} disabled={busy} className="rounded-md border px-3 py-1.5 hover:bg-muted disabled:opacity-50">
              用本机取文字
            </button>
            {pdf.hasMathpix && (
              <button onClick={() => void convert(false)} disabled={busy || (!pdf.mock && pdf.monthSpentUsd + pdf.costUsd > pdf.budgetUsd)} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
                {busy && <LoaderCircle size={13} className="animate-spin" />}
                用 Mathpix 转（{money(pdf.costUsd)}）
              </button>
            )}
          </div>
        </div>
      )}
      {preview && (
        <>
          <Preview p={preview} onNumbering={(n) => void renumber(n)} original={original} onOriginal={setOriginal} />
          <div className="mt-3 flex justify-end gap-2 text-xs">
            <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
              {preview.existing ? "取消" : "不存"}
            </button>
            <button onClick={() => void (preview.existing ? reparse() : save())} disabled={busy} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
              {busy && <LoaderCircle size={13} className="animate-spin" />}
              {preview.existing ? "重新解析" : "确认，存成文本"}
            </button>
          </div>
        </>
      )}
      {error && <p className="selectable mt-2 text-xs whitespace-pre-wrap text-destructive">{error}</p>}
    </Modal>
  );
}
