// 整库导出、从导出恢复，以及恢复结果（备份恢复共用）。规则与核对都在 Rust（transfer.rs）。
import { useState } from "react";
import { CircleCheck, Download, FolderOpen, LoaderCircle, TriangleAlert, Upload, X } from "lucide-react";
import { api, type ExportPreview, type ExportReport, type LibraryCounts, type RestoreReport } from "@/shared/ipc";
import { Checks, Modal, useUi } from "@/shared/ui";
import { formatSize, formatTime } from "@/library/labels";

export function countsLine(c: LibraryCounts) {
  return `文献 ${c.items} · 附件 ${c.attachments} · 笔记 ${c.notes} · 标注 ${c.annotations} · 集合 ${c.collections} · 附件文件 ${c.files} 个（${formatSize(c.bytes)}）`;
}

/** 恢复完成后的结果（从备份或从导出） */
export function RestoreResult({ r, title, onClose }: { r: RestoreReport; title: string; onClose: () => void }) {
  return (
    <div className="mb-3 rounded-md border bg-card px-3 py-2.5 text-xs">
      <div className="flex items-start gap-2">
        <CircleCheck size={15} className="mt-px shrink-0 text-emerald-600 dark:text-emerald-400" />
        <div className="min-w-0 flex-1 space-y-1">
          <p className="font-medium">{title}</p>
          <p className="text-muted-foreground">{countsLine(r.counts)}</p>
          {r.fromSchema !== r.toSchema && <p className="text-muted-foreground">数据库从第 {r.fromSchema} 版升级到了第 {r.toSchema} 版。</p>}
          {r.missingFiles > 0 && (
            <p className="flex items-center gap-1 text-destructive">
              <TriangleAlert size={13} />
              {r.missingFiles} 个附件文件找不到，这些附件打不开（记录还在）。
            </p>
          )}
          {r.safetyBackup && <p className="text-muted-foreground">恢复前的库已自动备份（下面列表里的「恢复前」），需要时可以再恢复回来。</p>}
        </div>
        <button onClick={onClose} className="rounded p-0.5 text-muted-foreground hover:bg-muted">
          <X size={13} />
        </button>
      </div>
      <Checks title="核对（导出 ↔ 恢复后的库）" checks={r.checks} columns={["导出里", "库里"]} />
    </div>
  );
}

function ExportResult({ r, onClose }: { r: ExportReport; onClose: () => void }) {
  const { notify } = useUi();
  return (
    <div className="mb-3 rounded-md border bg-card px-3 py-2.5 text-xs">
      <div className="flex items-start gap-2">
        <CircleCheck size={15} className="mt-px shrink-0 text-emerald-600 dark:text-emerald-400" />
        <div className="min-w-0 flex-1 space-y-1">
          <p className="font-medium">已导出</p>
          <p className="text-muted-foreground">{countsLine(r.counts)}</p>
          <p className="flex items-center gap-2">
            <code className="selectable min-w-0 truncate font-mono">{r.path}</code>
            <button
              onClick={() => api.exportReveal(r.path).catch((e) => notify(String(e), "error"))}
              className="flex shrink-0 items-center gap-1 rounded-md border px-2 py-0.5 hover:bg-muted"
            >
              <FolderOpen size={12} />
              在访达中显示
            </button>
          </p>
          {r.missingFiles.length > 0 && (
            <p className="text-destructive">
              {r.missingFiles.length} 个附件文件在文件库里本来就找不到，没有导出：{r.missingFiles.slice(0, 5).join("、")}
              {r.missingFiles.length > 5 ? " 等" : ""}
            </p>
          )}
        </div>
        <button onClick={onClose} className="rounded p-0.5 text-muted-foreground hover:bg-muted">
          <X size={13} />
        </button>
      </div>
      <Checks title="自检（库里 ↔ 按导出重建的库）" checks={r.checks} columns={["库里", "导出里"]} />
    </div>
  );
}

function PreviewDialog({ p, onCancel, onOk }: { p: ExportPreview; onCancel: () => void; onOk: () => void }) {
  return (
    <Modal title="从导出恢复" onClose={onCancel} width="w-[30rem]">
      <div className="space-y-2 text-xs leading-relaxed">
        <p>
          导出于 {formatTime(p.exportedAt)} · App {p.appVersion} · 数据库第 {p.schemaVersion} 版
        </p>
        <p className="selectable truncate font-mono text-muted-foreground" title={p.path}>
          {p.path}
        </p>
        <p className="text-muted-foreground">{countsLine(p.counts)}</p>
        {p.missingFiles > 0 && (
          <p className="flex items-start gap-1 text-destructive">
            <TriangleAlert size={13} className="mt-0.5 shrink-0" />
            {p.missingFiles} 个附件文件在导出里找不到（或大小不对），恢复后这些附件打不开。
          </p>
        )}
        <p className="rounded-md bg-muted px-2 py-1.5">现在的整个库会换成这份导出。替换前会自动备份一份「恢复前」，需要时可以从备份列表恢复回来。</p>
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button onClick={onCancel} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
          取消
        </button>
        <button onClick={onOk} className="rounded-md bg-destructive px-3 py-1 text-xs text-background">
          替换现在的库
        </button>
      </div>
    </Modal>
  );
}

/** 设置页「整库导出」一节。onRestored：库被整个换掉之后（界面要回到初始状态、全部刷新） */
export function ExportSection({ exportsDir, onRestored }: { exportsDir: string | undefined; onRestored: () => void }) {
  const { notify } = useUi();
  const [busy, setBusy] = useState<string | null>(null);
  const [exported, setExported] = useState<ExportReport | null>(null);
  const [preview, setPreview] = useState<{ dir: string; p: ExportPreview } | null>(null);
  const [restored, setRestored] = useState<RestoreReport | null>(null);

  const run = async <T,>(label: string, f: () => Promise<T>): Promise<T | undefined> => {
    setBusy(label);
    try {
      return await f();
    } catch (e) {
      notify(String(e), "error");
      return undefined;
    } finally {
      setBusy(null);
    }
  };

  const doExport = async () => {
    const dir = await api.pickFolder("选择导出到哪个文件夹（会在里面新建一个文件夹）", exportsDir);
    if (!dir) return;
    setRestored(null);
    const r = await run("正在导出并自检…", () => api.exportCreate(dir));
    if (r) setExported(r);
  };

  const pickRestore = async () => {
    const dir = await api.pickFolder("选择整库导出的文件夹（里面有 library.json）", exportsDir);
    if (!dir) return;
    const p = await run("正在读取导出…", () => api.exportInspect(dir));
    if (p) setPreview({ dir, p });
  };

  const doRestore = async () => {
    if (!preview) return;
    const { dir } = preview;
    setPreview(null);
    setExported(null);
    const r = await run("正在恢复…", () => api.exportRestore(dir));
    if (r) {
      setRestored(r);
      onRestored();
    }
  };

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-xs font-medium text-muted-foreground">整库导出</h2>
        <div className="flex gap-2">
          <button onClick={pickRestore} disabled={busy !== null} className="flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
            <Upload size={13} />
            从导出恢复…
          </button>
          <button onClick={doExport} disabled={busy !== null} className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50">
            <Download size={13} />
            导出到…
          </button>
        </div>
      </div>
      <p className="mb-3 text-xs text-muted-foreground">
        把整个库（文献、笔记、标注、设置）连同全部附件文件导出到一个文件夹，导出后自动核对一遍。library.json 是普通 JSON，不靠本 App 也能读。换电脑、从开发版搬到正式版时，在新的地方「从导出恢复」。
      </p>
      {busy && (
        <p className="mb-3 flex items-center gap-1.5 text-xs text-muted-foreground">
          <LoaderCircle size={13} className="animate-spin" />
          {busy}
        </p>
      )}
      {exported && <ExportResult r={exported} onClose={() => setExported(null)} />}
      {restored && <RestoreResult r={restored} title="已从导出恢复" onClose={() => setRestored(null)} />}
      {preview && <PreviewDialog p={preview.p} onCancel={() => setPreview(null)} onOk={doRestore} />}
    </div>
  );
}
