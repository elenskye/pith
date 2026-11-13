// 文献库附件上的「解析成文本」（decisions/0024「书与文献」）：先显示结构预览（各章段数、各角色数量、有没有页码），用户确认后才存。
// EPUB 直接解析；PDF 先确认页数和费用（Mathpix 每页 $0.005；超过页数上限提示改用 EPUB；没有 Mathpix 密钥时用本机取文字）。
// 存之前做「导入前」备份；是译本的话选它对应的原著，两份文本按段落对齐。
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type ConceptNumbering, type ConceptPrepared, type ConceptTextPreview } from "@/shared/ipc";
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
      {p.existing && <Notice title="这个附件已经解析过一次。再存是第二份文本（旧的和它的卡片保留）。" />}
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
      {p.candidates.length > 0 && (
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

export function ParseDialog({ attachmentId, title, onClose, onSaved }: { attachmentId: number; title: string; onClose: () => void; onSaved?: (uid: string) => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const [prepared, setPrepared] = useState<ConceptPrepared | null>(null);
  const [preview, setPreview] = useState<ConceptTextPreview | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
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

  const pdf = prepared?.kind === "pdf" ? prepared : null;
  return (
    <Modal title={`解析成文本：${title}`} onClose={onClose} width="w-[680px]">
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
              不存
            </button>
            <button onClick={() => void save()} disabled={busy} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
              {busy && <LoaderCircle size={13} className="animate-spin" />}
              确认，存成文本
            </button>
          </div>
        </>
      )}
      {error && <p className="selectable mt-2 text-xs whitespace-pre-wrap text-destructive">{error}</p>}
    </Modal>
  );
}
