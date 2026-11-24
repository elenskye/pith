import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, BookOpen, CircleAlert, CircleCheck, FileText, FolderOpen, LoaderCircle, TriangleAlert } from "lucide-react";
import { Checks } from "@/shared/ui";
import { api, type FileIssue, type FilePreview, type RunDetail, type RunSummary, type ZoteroPreview } from "@/shared/ipc";
import { formatSize, formatTime, itemTypeLabel, sourceLabel } from "@/library/labels";
import { useMetadata } from "@/metadata/MetadataProvider";

type Stage =
  | { kind: "choose" }
  | { kind: "zotero"; dir: string; baseDir: string | null }
  | { kind: "file"; path: string }
  | { kind: "report"; run: RunDetail; fresh: boolean };

/** 导入：选来源 → 预览（不写库）→ 导入（先自动备份，一个事务写完并核对）→ 报告。 */
export function ImportPage({ onDone }: { onDone: () => void }) {
  const [stage, setStage] = useState<Stage>({ kind: "choose" });
  const [pickError, setPickError] = useState<string | null>(null);
  const back = () => setStage({ kind: "choose" });

  const pick = async (what: "zotero" | "file") => {
    setPickError(null);
    try {
      if (what === "zotero") {
        const dir = await api.pickFolder("选择 Zotero 数据目录（副本）");
        if (dir) setStage({ kind: "zotero", dir, baseDir: null });
      } else {
        const path = await api.pickFile("选择 BibTeX、RIS 或 CSL JSON 文件");
        if (path) setStage({ kind: "file", path });
      }
    } catch (e) {
      setPickError(String(e));
    }
  };

  return (
    <section className="min-w-0 flex-1 overflow-y-auto">
      <header className="flex h-12 items-center gap-2 border-b px-6">
        {stage.kind !== "choose" && (
          <button onClick={back} className="-ml-2 rounded-md p-1 text-muted-foreground hover:bg-muted" title="返回">
            <ArrowLeft size={16} />
          </button>
        )}
        <h1 className="text-sm font-semibold">{stage.kind === "report" ? "导入报告" : "导入"}</h1>
      </header>
      <div className="max-w-3xl px-6 py-6">
        {stage.kind === "choose" && <Choose onPick={pick} error={pickError} onOpenRun={(run) => setStage({ kind: "report", run, fresh: false })} />}
        {stage.kind === "zotero" && (
          <ZoteroStep
            dir={stage.dir}
            baseDir={stage.baseDir}
            onBaseDir={(baseDir) => setStage({ ...stage, baseDir })}
            onCancel={back}
            onDone={(run) => setStage({ kind: "report", run, fresh: true })}
          />
        )}
        {stage.kind === "file" && <FileStep path={stage.path} onCancel={back} onDone={(run) => setStage({ kind: "report", run, fresh: true })} />}
        {stage.kind === "report" && <RunReport run={stage.run} fresh={stage.fresh} onLibrary={onDone} onAgain={back} />}
      </div>
    </section>
  );
}

function Choose({ onPick, error, onOpenRun }: { onPick: (w: "zotero" | "file") => void; error: string | null; onOpenRun: (r: RunDetail) => void }) {
  const runs = useQuery({ queryKey: ["importRuns"], queryFn: api.importRunsList });
  const open = useMutation({ mutationFn: (id: number) => api.importRunsGet(id), onSuccess: onOpenRun });
  return (
    <>
      <div className="grid grid-cols-2 gap-4">
        <SourceCard
          icon={<BookOpen size={18} />}
          title="Zotero 库"
          desc="选择 Zotero 数据目录的副本（含 zotero.sqlite 与 storage）。条目、集合、标签、附件、笔记、标注和回收站原样导入，不改动副本。"
          button="选择文件夹…"
          onClick={() => onPick("zotero")}
        />
        <SourceCard
          icon={<FileText size={18} />}
          title="文献文件"
          desc="BibTeX（.bib）、RIS（.ris）、CSL JSON（.json），来自其他文献软件、数据库或期刊网站的导出。"
          button="选择文件…"
          onClick={() => onPick("file")}
        />
      </div>
      {error && <p className="mt-3 text-xs text-destructive">{error}</p>}
      <p className="mt-3 text-xs text-muted-foreground">导入前会自动备份数据库；导入在一个事务里完成，写完逐类核对，不一致就整批撤销。</p>

      <h2 className="mt-8 mb-2 text-xs font-medium text-muted-foreground">导入记录</h2>
      {runs.data && runs.data.length === 0 ? (
        <p className="text-muted-foreground">还没有导入过</p>
      ) : (
        <table className="w-full text-left">
          <thead className="text-xs text-muted-foreground">
            <tr className="border-b">
              <th className="py-1.5 font-normal">时间</th>
              <th className="py-1.5 font-normal">来源</th>
              <th className="py-1.5 font-normal">内容</th>
              <th className="py-1.5 text-right font-normal" />
            </tr>
          </thead>
          <tbody>
            {runs.data?.map((r) => (
              <tr key={r.id} className="border-b border-border/60">
                <td className="py-1.5 tabular-nums">{formatTime(r.startedAt)}</td>
                <td className="max-w-56 truncate py-1.5" title={r.sourcePath ?? ""}>
                  {sourceLabel(r.source)}
                  <span className="ml-1.5 text-xs text-muted-foreground">{baseName(r.sourcePath)}</span>
                </td>
                <td className="py-1.5 text-muted-foreground">{summaryText(r)}</td>
                <td className="py-1.5 text-right">
                  <button onClick={() => open.mutate(r.id)} className="rounded-md border px-2 py-0.5 text-xs hover:bg-muted">
                    报告
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

function baseName(p: string | null) {
  return p ? (p.split("/").filter(Boolean).pop() ?? p) : "";
}

function summaryText(r: RunSummary) {
  const s = r.stats;
  const parts = [`${s.items} 条文献`];
  if (s.attachments) parts.push(`${s.attachments} 个附件`);
  if (s.notes) parts.push(`${s.notes} 条笔记`);
  if (s.skipped) parts.push(`跳过 ${s.skipped}`);
  return parts.join(" · ");
}

function SourceCard({ icon, title, desc, button, onClick }: { icon: React.ReactNode; title: string; desc: string; button: string; onClick: () => void }) {
  return (
    <div className="flex flex-col rounded-lg border bg-card p-4">
      <div className="flex items-center gap-2 font-medium">
        <span className="text-muted-foreground">{icon}</span>
        {title}
      </div>
      <p className="mt-2 flex-1 text-xs leading-relaxed text-muted-foreground">{desc}</p>
      <button onClick={onClick} className="mt-3 flex items-center justify-center gap-1.5 self-start rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground">
        <FolderOpen size={13} />
        {button}
      </button>
    </div>
  );
}

function useImportDone(onDone: (r: RunDetail) => void) {
  const qc = useQueryClient();
  const metadata = useMetadata();
  return (r: RunDetail) => {
    for (const k of ["overview", "items", "collections", "importRuns", "backups", "item", "attachment", "note"]) qc.invalidateQueries({ queryKey: [k] });
    onDone(r);
    // 联网补全：第一次先问；已打开自动补全的，后端已经排进队列
    void metadata.afterImport(r.id);
  };
}

function Busy({ text }: { text: string }) {
  return (
    <p className="flex items-center gap-2 text-muted-foreground">
      <LoaderCircle size={15} className="animate-spin" />
      {text}
    </p>
  );
}

function Stat({ label, value, sub }: { label: string; value: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="rounded-md border px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="text-base font-semibold tabular-nums">{value}</div>
      {sub && <div className="text-[11px] text-muted-foreground">{sub}</div>}
    </div>
  );
}

function Notice({ tone, children }: { tone: "warn" | "error" | "info"; children: React.ReactNode }) {
  const cls = tone === "error" ? "bg-destructive/10 text-destructive" : tone === "warn" ? "bg-amber-500/10 text-amber-700 dark:text-amber-300" : "bg-muted text-muted-foreground";
  return (
    <div className={`mt-3 flex items-start gap-2 rounded-md px-3 py-2 text-xs ${cls}`}>
      {tone === "info" ? <CircleAlert size={14} className="mt-px shrink-0" /> : <TriangleAlert size={14} className="mt-px shrink-0" />}
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function Actions({ onCancel, onRun, disabled, running }: { onCancel: () => void; onRun: () => void; disabled?: boolean; running: boolean }) {
  return (
    <div className="mt-6 flex items-center gap-2">
      <button onClick={onRun} disabled={disabled || running} className="rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground disabled:opacity-40">
        开始导入
      </button>
      <button onClick={onCancel} disabled={running} className="rounded-md border px-3 py-1.5 text-xs hover:bg-muted disabled:opacity-40">
        取消
      </button>
      {running && <Busy text="正在导入：先备份数据库，再复制附件、写入并核对…" />}
    </div>
  );
}

function ZoteroStep({ dir, baseDir, onBaseDir, onCancel, onDone }: { dir: string; baseDir: string | null; onBaseDir: (b: string) => void; onCancel: () => void; onDone: (r: RunDetail) => void }) {
  const preview = useQuery({ queryKey: ["zoteroPreview", dir, baseDir], queryFn: () => api.importZoteroPreview(dir, baseDir), staleTime: 0, gcTime: 0 });
  const done = useImportDone(onDone);
  const run = useMutation({ mutationFn: () => api.importZoteroRun(dir, baseDir), onSuccess: done });
  const pickBase = async () => {
    const b = await api.pickFolder("选择 Zotero 的「链接附件根目录」");
    if (b) onBaseDir(b);
  };
  return (
    <>
      <p className="text-xs text-muted-foreground">Zotero 数据目录</p>
      <code className="selectable block truncate font-mono text-xs">{dir}</code>
      {preview.isPending && (
        <div className="mt-4">
          <Busy text="正在读取（只读，不改动副本）…" />
        </div>
      )}
      {preview.error && <Notice tone="error">{String(preview.error)}</Notice>}
      {preview.data && <ZoteroSummary p={preview.data} baseDir={baseDir} onPickBase={pickBase} />}
      {run.error && <Notice tone="error">导入没有完成，库里没有任何改动：{String(run.error)}</Notice>}
      <Actions onCancel={onCancel} onRun={() => run.mutate()} disabled={!preview.data || !!preview.data.blocked} running={run.isPending} />
    </>
  );
}

function ZoteroSummary({ p, baseDir, onPickBase }: { p: ZoteroPreview; baseDir: string | null; onPickBase: () => void }) {
  return (
    <>
      <p className="mt-1 text-xs text-muted-foreground">
        Zotero {p.clientVersion ?? "版本未知"} · 数据库版本 {p.userdataVersion ?? "?"}
      </p>
      <div className="mt-4 grid grid-cols-4 gap-2">
        <Stat label="文献条目" value={p.items} sub={p.groupLibraries > 0 ? `含 ${p.groupLibraries} 个群组库` : undefined} />
        <Stat label="附件" value={p.attachments} sub={`${p.files} 个文件 · ${formatSize(p.bytes)}`} />
        <Stat label="笔记" value={p.notes} sub={p.annotations > 0 ? `标注 ${p.annotations}` : undefined} />
        <Stat label="集合 / 标签" value={`${p.collections} / ${p.tags}`} sub={p.trashItems + p.trashCollections > 0 ? `回收站 ${p.trashItems} 条、${p.trashCollections} 个集合` : undefined} />
      </div>
      {p.blocked && <Notice tone="error">{p.blocked}</Notice>}
      {p.zoteroRunning && <Notice tone="warn">这个目录旁边有 Zotero 正在使用的临时文件。请先关闭 Zotero，或按指南复制一份数据目录再导入，否则可能读到不完整的数据。</Notice>}
      {p.baseRelative > 0 && (
        <Notice tone="info">
          有 {p.baseRelative} 个链接附件使用相对路径，需要 Zotero 首选项里的「链接附件根目录」才能找到文件。
          <div className="mt-1.5 flex items-center gap-2">
            <button onClick={onPickBase} className="rounded-md border bg-background px-2 py-0.5 text-foreground hover:bg-muted">
              选择根目录…
            </button>
            {baseDir && <code className="truncate font-mono">{baseDir}</code>}
          </div>
        </Notice>
      )}
      {p.missingFiles.length > 0 && (
        <Issues
          title={`${p.missingFiles.length} 个附件找不到文件（条目照常导入，标记为「文件缺失」）`}
          issues={p.missingFiles}
          hint="只在 Zotero 云端的文件：卸载 Zotero 之前，先在 Zotero 里下载它们，再重新导入。"
        />
      )}
      {p.warnings.length > 0 && <Messages title="提示" items={p.warnings} />}
    </>
  );
}

function FileStep({ path, onCancel, onDone }: { path: string; onCancel: () => void; onDone: (r: RunDetail) => void }) {
  const [skipDuplicates, setSkip] = useState(true);
  const [intoCollection, setInto] = useState(true);
  const preview = useQuery({ queryKey: ["filePreview", path], queryFn: () => api.importFilePreview(path), staleTime: 0, gcTime: 0 });
  const done = useImportDone(onDone);
  const run = useMutation({ mutationFn: () => api.importFileRun(path, skipDuplicates, intoCollection), onSuccess: done });
  const p = preview.data;
  return (
    <>
      <p className="text-xs text-muted-foreground">文件</p>
      <code className="selectable block truncate font-mono text-xs">{path}</code>
      {preview.isPending && (
        <div className="mt-4">
          <Busy text="正在读取…" />
        </div>
      )}
      {preview.error && <Notice tone="error">{String(preview.error)}</Notice>}
      {p && (
        <>
          <FileSummary p={p} />
          <div className="mt-4 space-y-1.5">
            {p.duplicates.length > 0 && (
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={skipDuplicates} onChange={(e) => setSkip(e.target.checked)} />
                跳过可能重复的 {p.duplicates.length} 条
              </label>
            )}
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={intoCollection} onChange={(e) => setInto(e.target.checked)} />
              放进新集合「导入：{p.fileName}」
            </label>
          </div>
        </>
      )}
      {run.error && <Notice tone="error">导入没有完成，库里没有任何改动：{String(run.error)}</Notice>}
      <Actions onCancel={onCancel} onRun={() => run.mutate()} disabled={!p} running={run.isPending} />
    </>
  );
}

function FileSummary({ p }: { p: FilePreview }) {
  return (
    <>
      <p className="mt-1 text-xs text-muted-foreground">{sourceLabel(p.format)} 格式</p>
      <div className="mt-4 grid grid-cols-3 gap-2">
        <Stat label="文献记录" value={p.records} sub={p.notes > 0 ? `带 ${p.notes} 条笔记` : undefined} />
        <Stat label="可能重复" value={p.duplicates.length} sub="DOI 或标题 + 年份相同" />
        <Stat label="类型" value={p.byType.length} sub={p.byType.slice(0, 3).map((t) => `${itemTypeLabel(t.itemType)} ${t.count}`).join("、")} />
      </div>
      {p.samples.length > 0 && <Messages title="前几条" items={p.samples} muted />}
      {p.duplicates.length > 0 && <Messages title="可能重复" items={p.duplicates.map((d) => `第 ${d.index} 条「${d.title ?? "（无标题）"}」：${d.reason}`)} />}
      {p.warnings.length > 0 && <Messages title="提示" items={p.warnings} />}
    </>
  );
}

function Messages({ title, items, muted }: { title: string; items: string[]; muted?: boolean }) {
  return (
    <div className="mt-4">
      <h3 className="mb-1 text-xs text-muted-foreground">{title}</h3>
      <ul className={`selectable max-h-48 space-y-0.5 overflow-y-auto text-xs ${muted ? "text-muted-foreground" : ""}`}>
        {items.map((m, i) => (
          <li key={i}>· {m}</li>
        ))}
      </ul>
    </div>
  );
}

function Issues({ title, issues, hint }: { title: string; issues: FileIssue[]; hint?: string }) {
  return (
    <div className="mt-4">
      <h3 className="mb-1 text-xs text-muted-foreground">{title}</h3>
      <ul className="selectable max-h-56 space-y-1 overflow-y-auto rounded-md border p-2 text-xs">
        {issues.map((i, n) => (
          <li key={n}>
            <span className="font-medium">{i.title || i.key || "（无标题附件）"}</span>
            {i.parentTitle && <span className="text-muted-foreground"> · {i.parentTitle}</span>}
            {i.inTrash && <span className="ml-1 rounded bg-muted px-1 text-[11px]">回收站</span>}
            <div className="text-muted-foreground">{i.reason}</div>
          </li>
        ))}
      </ul>
      {hint && <p className="mt-1 text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  );
}

function RunReport({ run, fresh, onLibrary, onAgain }: { run: RunDetail; fresh: boolean; onLibrary: () => void; onAgain: () => void }) {
  const s = run.stats;
  const r = run.report;
  return (
    <>
      {fresh && (
        <p className="mb-4 flex items-center gap-2 font-medium">
          <CircleCheck size={16} className="text-emerald-600 dark:text-emerald-400" />
          导入完成
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {sourceLabel(run.source)} · {formatTime(run.startedAt)} · <span className="selectable">{run.sourcePath}</span>
      </p>
      <div className="mt-4 grid grid-cols-4 gap-2">
        <Stat label="文献条目" value={s.items} sub={s.skipped > 0 ? `跳过 ${s.skipped}` : undefined} />
        <Stat label="附件" value={s.attachments} sub={s.attachments > 0 ? `复制 ${s.filesCopied} 个文件 · ${formatSize(s.bytesCopied)}` : undefined} />
        <Stat label="笔记" value={s.notes} sub={s.annotations > 0 ? `标注 ${s.annotations}` : undefined} />
        <Stat label="集合 / 新标签" value={`${s.collections} / ${s.newTags}`} sub={s.trash > 0 ? `回收站 ${s.trash}` : undefined} />
      </div>
      {s.filesReused > 0 && <p className="mt-2 text-xs text-muted-foreground">{s.filesReused} 个文件与已有的内容相同，只存了一份。</p>}
      <Checks title="写入核对（来源 ↔ 库）" checks={r.checks} />
      <Checks title="读取核对（Zotero 数据库 ↔ 读出的数据）" checks={r.sourceChecks} />
      {r.missingFiles.length > 0 && <Issues title={`${r.missingFiles.length} 个附件文件缺失`} issues={r.missingFiles} hint="条目照常导入，在列表里标记为「文件缺失」。" />}
      {r.fileWarnings.length > 0 && <Issues title="文件提示" issues={r.fileWarnings} />}
      {r.notMigrated.length > 0 && <Messages title="按设计没有迁移" items={r.notMigrated.map((n) => `${n.label}：${n.count}`)} muted />}
      {r.skipped.length > 0 && <Messages title="跳过" items={r.skipped} />}
      {r.warnings.length > 0 && <Messages title="提示" items={r.warnings} />}
      {run.backupFile && <p className="mt-4 text-xs text-muted-foreground">导入前的备份：{run.backupFile}（在设置页的备份列表里）</p>}
      <div className="mt-6 flex gap-2">
        <button onClick={onLibrary} className="rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground">
          查看文献
        </button>
        <button onClick={onAgain} className="rounded-md border px-3 py-1.5 text-xs hover:bg-muted">
          {fresh ? "再导入一个" : "返回"}
        </button>
      </div>
    </>
  );
}
