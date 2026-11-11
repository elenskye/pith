// 设置 › 引文样式（内置 + 导入 .csl）与 Zotero 文档转换（Word 外，写新文件）。decisions/0017
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRightLeft, CircleAlert, CircleCheck, FileUp, FolderOpen, LoaderCircle, Trash2 } from "lucide-react";
import { api, type CiteStyle, type ZoteroConvertReport, type ZoteroDocPreview } from "@/shared/ipc";
import { useUi } from "@/shared/ui";

function kindLabel(s: CiteStyle): string {
  return { numeric: "顺序编码", "author-date": "著者-出版年", author: "著者", label: "标签" }[s.kind] ?? s.kind;
}

export function CiteStylesSection() {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const styles = useQuery({ queryKey: ["citeStyles"], queryFn: api.citeStylesList });
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["citeStyles"] });
    qc.invalidateQueries({ queryKey: ["wordStatus"] });
  };
  const importStyle = useMutation({
    mutationFn: async () => {
      const path = await api.pickCsl();
      return path ? api.citeStyleImport(path) : null;
    },
    onSuccess: (r) => {
      if (!r) return;
      refresh();
      notify(r.replaced ? `已更新「${r.style.label}」` : `已导入「${r.style.label}」，Word 里 Pith › Style 可以选它`);
    },
  });
  const remove = useMutation({ mutationFn: api.citeStyleRemove, onSuccess: refresh });

  const askRemove = async (s: CiteStyle) => {
    const ok = await confirm({
      title: `删除样式「${s.label}」？`,
      message: "已经用这个样式的文档，在 Word 里刷新时会提示样式不在了，换一个样式或重新导入就好。文档里的引文不受影响。",
      confirm: "删除",
      danger: true,
    });
    if (ok) remove.mutate(s.id);
  };

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-xs font-medium text-muted-foreground">引文样式</h2>
        <button
          onClick={() => importStyle.mutate()}
          disabled={importStyle.isPending}
          className="flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50"
        >
          {importStyle.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <FileUp size={13} />}
          导入 .csl…
        </button>
      </div>
      <p className="mb-3 text-xs text-muted-foreground">
        Word 里 Pith › Style 切换。新文档默认 GB/T 7714-2015 顺序编码制；同一文献再引时页码标在方括号外，如「[2]17」。导入的 .csl 可以是 Zotero 样式库里下载的任意样式（依赖样式要先有它的母样式）。
      </p>
      <ul className="divide-y rounded-md border text-sm">
        {(styles.data ?? []).map((s) => (
          <li key={s.id} className="flex items-center gap-3 px-3 py-1.5">
            <div className="min-w-0 flex-1">
              <p className="truncate">{s.label}</p>
              <p className="truncate text-xs text-muted-foreground">
                {kindLabel(s)}
                {s.imported ? ` · 导入的${s.parent ? `（依赖 ${s.parent}）` : ""}` : " · 内置"}
                {s.en !== s.label && !s.imported ? ` · Word 里叫 ${s.en}` : ""}
              </p>
            </div>
            {s.imported && (
              <button onClick={() => askRemove(s)} title="删除这个样式" className="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted">
                <Trash2 size={13} />
              </button>
            )}
          </li>
        ))}
      </ul>
      {(importStyle.error || remove.error || styles.error) && <p className="mt-2 text-xs text-destructive">{String(importStyle.error ?? remove.error ?? styles.error)}</p>}
    </div>
  );
}

function Stat({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  return (
    <p className="flex items-start gap-1.5">
      {ok ? <CircleCheck size={13} className="mt-0.5 shrink-0 text-emerald-600" /> : <CircleAlert size={13} className="mt-0.5 shrink-0 text-amber-600" />}
      <span>{children}</span>
    </p>
  );
}

function Preview({ p }: { p: ZoteroDocPreview }) {
  const f = p.found;
  return (
    <div className="space-y-1 text-xs">
      <Stat ok={f.codes.length > 0}>
        {f.codes.length} 处 Zotero 引文{f.bibliography ? "、1 个 Zotero 书目" : "，没有找到 Zotero 书目"}
        {f.fields > f.codes.length + (f.bibliography ? 1 : 0) ? `（另有 ${f.fields - f.codes.length - (f.bibliography ? 1 : 0)} 个结构特殊，会原样留着）` : ""}
      </Stat>
      <Stat ok={p.stats.fromDocument + p.stats.missing === 0}>
        引文里的 {p.stats.items} 条文献：{p.stats.inLibrary} 条在文献库里
        {p.stats.fromDocument > 0 && `，${p.stats.fromDocument} 条不在（用文档里自带的数据）`}
        {p.stats.missing > 0 && `，${p.stats.missing} 条哪里都没有数据（会去掉）`}
      </Stat>
      <Stat ok={!!p.style || !f.zoteroStyle}>
        样式：{p.style ?? (f.zoteroStyle ? `Zotero 用的 ${f.zoteroStyle} Pith 里没有，先用默认样式` : "文档里没有 Zotero 设置，用默认样式")}
      </Stat>
      {f.noteStyle && <Stat ok={false}>Zotero 用的是脚注样式；Pith 只转换正文里的引文</Stat>}
      {f.hasPith && <Stat ok>文档里已经有 Pith 引文，沿用它的样式</Stat>}
      <p className="pt-1 text-muted-foreground">
        转换后写到：<span className="selectable break-all">{p.out}</span>（原文件不动）
      </p>
    </div>
  );
}

function Result({ out, r, onReveal }: { out: string; r: ZoteroConvertReport; onReveal: () => void }) {
  const totalCites = r.sameCitations + r.citationDiffs.length;
  return (
    <div className="space-y-1 text-xs">
      <Stat ok>
        已转换 {r.stats.converted}/{r.stats.citations} 处引文{r.bibliography ? "和书目" : ""}，独立核对通过
      </Stat>
      <Stat ok={r.citationDiffs.length === 0}>
        引文文字与 Zotero 原来显示的一致 {r.sameCitations}/{totalCites}
      </Stat>
      {r.bibliography && (
        <Stat ok={r.entryDiffs.length === 0}>
          书目条目一致 {r.sameEntries}/{r.zoteroEntries}
        </Stat>
      )}
      {r.left > 0 && <Stat ok={false}>{r.left} 个 Zotero 域没法转换，原样留着</Stat>}
      {r.styleNote && <Stat ok={false}>{r.styleNote}</Stat>}
      {(r.citationDiffs.length > 0 || r.entryDiffs.length > 0 || r.notes.length > 0) && (
        <details className="rounded-md border px-2 py-1">
          <summary className="cursor-default">不一致的地方与提示</summary>
          <ul className="mt-1 space-y-1">
            {r.citationDiffs.slice(0, 20).map((d) => (
              <li key={`c${d.index}`}>
                第 {d.index + 1} 处引文：Zotero「{d.zotero}」→ Pith「{d.pith}」
              </li>
            ))}
            {r.entryDiffs.slice(0, 20).map((d) => (
              <li key={`e${d.index}`} className="break-all">
                书目第 {d.index + 1} 条：Zotero「{d.zotero}」→ Pith「{d.pith}」
              </li>
            ))}
            {r.notes.slice(0, 20).map((n, i) => (
              <li key={`n${i}`}>{n}</li>
            ))}
          </ul>
        </details>
      )}
      <div className="flex items-center gap-2 pt-1">
        <span className="selectable min-w-0 flex-1 truncate text-muted-foreground">{out}</span>
        <button onClick={onReveal} className="flex shrink-0 items-center gap-1 rounded-md border px-2 py-1 hover:bg-muted">
          <FolderOpen size={13} />
          在访达中显示
        </button>
      </div>
    </div>
  );
}

/** Word 外转换：选一份有 Zotero 引文的 .docx → 预览 → 转换成新文件 */
export function ZoteroConvertSection() {
  const [path, setPath] = useState<string | null>(null);
  const preview = useMutation({ mutationFn: api.wordZoteroInspect });
  const convert = useMutation({ mutationFn: api.wordZoteroConvert });
  const reveal = useMutation({ mutationFn: api.wordRevealFile });

  const choose = async () => {
    const p = await api.pickDocx();
    if (!p) return;
    setPath(p);
    convert.reset();
    preview.mutate(p);
  };

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-xs font-medium text-muted-foreground">转换 Zotero 文档</h2>
        <button onClick={choose} className="flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted">
          <FolderOpen size={13} />
          选择 Word 文档…
        </button>
      </div>
      <p className="mb-3 text-xs text-muted-foreground">
        把用 Zotero 插的引文和书目换成 Pith 引文：按 Zotero 条目对应到文献库，文字与 Zotero 原来显示的逐处比对。在 Word 里也可以直接点 Pith › Convert Zotero（改的是打开的那份）。
      </p>
      {path && (
        <div className="rounded-md border px-3 py-2">
          <p className="mb-2 truncate text-xs font-medium">{path.split("/").pop()}</p>
          {preview.isPending && <LoaderCircle size={14} className="animate-spin text-muted-foreground" />}
          {preview.error && <p className="text-xs text-destructive">{String(preview.error)}</p>}
          {preview.data && !convert.data && (
            <>
              <Preview p={preview.data} />
              <div className="mt-2 flex justify-end">
                <button
                  onClick={() => convert.mutate(path)}
                  disabled={convert.isPending || (preview.data.found.codes.length === 0 && !preview.data.found.bibliography)}
                  className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50"
                >
                  {convert.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <ArrowRightLeft size={13} />}
                  转换成新文件
                </button>
              </div>
            </>
          )}
          {convert.data && <Result out={convert.data.out} r={convert.data.report} onReveal={() => reveal.mutate(convert.data.out)} />}
          {(convert.error || reveal.error) && <p className="mt-2 text-xs text-destructive">{String(convert.error ?? reveal.error)}</p>}
        </div>
      )}
    </div>
  );
}
