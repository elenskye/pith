import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, LoaderCircle, RefreshCw, Save } from "lucide-react";
import { api, type BackupInfo, type RestoreReport } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { backupLabel, formatSize } from "@/library/labels";
import { MetadataSettingsSection } from "@/metadata/MetadataSettings";
import { ExportSection, RestoreResult } from "@/settings/Transfer";
import { WordSettingsSection } from "@/settings/WordSettings";
import { CiteStylesSection, ZoteroConvertSection } from "@/settings/CiteSettings";
import { AgentSettingsSection } from "@/settings/AgentSettings";
import { ResearchSettingsSection } from "@/settings/ResearchSettings";

/** onRestored：库被整个换掉（恢复）之后，让外框回到初始状态 */
export function SettingsPage({ onRestored }: { onRestored: () => void }) {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo });
  const backups = useQuery({ queryKey: ["backups"], queryFn: api.backupsList });
  const backupNow = useMutation({ mutationFn: api.backupsCreate, onSuccess: () => qc.invalidateQueries({ queryKey: ["backups"] }) });
  const reveal = useMutation({ mutationFn: api.revealDataDir });
  const rebuild = useMutation({ mutationFn: api.searchRebuild });
  const [restoring, setRestoring] = useState<string | null>(null);
  const [restored, setRestored] = useState<{ b: BackupInfo; r: RestoreReport } | null>(null);
  const i = info.data;

  // 库换掉了：全部数据重新读，外框回到「全部文献」
  const afterRestore = () => {
    qc.invalidateQueries();
    onRestored();
  };

  const restore = async (b: BackupInfo) => {
    const ok = await confirm({
      title: "恢复到这份备份？",
      message: (
        <>
          <p className="mb-1.5 font-medium text-foreground">
            {b.createdAt}（{backupLabel(b.label)}）
          </p>
          现在的整个库会换成这份备份，这之后做的修改都会不见。替换前会自动备份一份「恢复前」，需要时可以再恢复回来。附件文件不受影响。
        </>
      ),
      confirm: "恢复",
      danger: true,
    });
    if (!ok) return;
    setRestoring(b.fileName);
    try {
      const r = await api.backupsRestore(b.fileName);
      setRestored({ b, r });
      afterRestore();
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setRestoring(null);
    }
  };

  return (
    <section className="min-w-0 flex-1 overflow-y-auto">
      <header className="flex h-12 items-center border-b px-6">
        <h1 className="text-sm font-semibold">设置</h1>
      </header>
      <div className="max-w-2xl space-y-8 px-6 py-6">
        <div>
          <h2 className="mb-3 text-xs font-medium text-muted-foreground">数据</h2>
          <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-center gap-x-3 gap-y-2">
            <dt className="text-muted-foreground">数据目录</dt>
            <dd className="flex items-center gap-2">
              <code className="selectable min-w-0 truncate font-mono text-xs">{i?.dataDir}</code>
              <button onClick={() => reveal.mutate()} className="flex shrink-0 items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-muted">
                <FolderOpen size={13} />
                在访达中显示
              </button>
            </dd>
            <dt className="text-muted-foreground">数据库版本</dt>
            <dd>{i ? (i.schemaVersion === null ? "未打开" : `第 ${i.schemaVersion} 版（App 支持到第 ${i.latestSchema} 版）`) : ""}</dd>
            <dt className="text-muted-foreground">App 版本</dt>
            <dd>
              {i?.appVersion}
              {i?.isDev && <span className="ml-2 rounded bg-accent px-1.5 py-0.5 text-xs text-accent-foreground">开发版：数据与正式版分开存放</span>}
            </dd>
          </dl>
          {reveal.error && <p className="mt-2 text-xs text-destructive">{String(reveal.error)}</p>}
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between">
            <h2 className="text-xs font-medium text-muted-foreground">搜索索引</h2>
            <button
              onClick={() => rebuild.mutate()}
              disabled={rebuild.isPending}
              className="flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50"
            >
              <RefreshCw size={13} className={rebuild.isPending ? "animate-spin" : ""} />
              重建索引
            </button>
          </div>
          <p className="text-xs text-muted-foreground">
            搜索索引（search.db）由文献库自动生成，启动时会检查是否需要重建。搜索结果不对时可以手动重建，不影响文献数据。
            {rebuild.data !== undefined && <span className="ml-1 text-foreground">已重建：{rebuild.data} 条。</span>}
          </p>
          {rebuild.error && <p className="mt-1 text-xs text-destructive">{String(rebuild.error)}</p>}
        </div>

        <MetadataSettingsSection />

        <WordSettingsSection />

        <CiteStylesSection />

        <ZoteroConvertSection />

        <AgentSettingsSection />

        <ResearchSettingsSection />

        <ExportSection exportsDir={i ? `${i.dataDir}/exports` : undefined} onRestored={afterRestore} />

        <div>
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-xs font-medium text-muted-foreground">备份</h2>
            <button
              onClick={() => backupNow.mutate()}
              disabled={backupNow.isPending}
              className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50"
            >
              <Save size={13} />
              立即备份
            </button>
          </div>
          <p className="mb-3 text-xs text-muted-foreground">
            每天第一次打开时自动备份（保留最近 30 份，另外每月留一份）；数据库升级、导入、永久删除、恢复之前也会自动备份。备份只含数据库，附件文件共用：还有备份用到的文件不会删，恢复任何一份附件都在。
          </p>
          {backupNow.error && <p className="mb-2 text-xs text-destructive">{String(backupNow.error)}</p>}
          {restored && <RestoreResult r={restored.r} title={`已恢复到 ${restored.b.createdAt} 的备份（${backupLabel(restored.b.label)}）`} onClose={() => setRestored(null)} />}
          {backups.data && backups.data.length === 0 ? (
            <p className="text-muted-foreground">还没有备份</p>
          ) : (
            <table className="w-full text-left">
              <thead className="text-xs text-muted-foreground">
                <tr className="border-b">
                  <th className="py-1.5 font-normal">时间</th>
                  <th className="py-1.5 font-normal">类型</th>
                  <th className="py-1.5 text-right font-normal">大小</th>
                  <th className="w-16 py-1.5" />
                </tr>
              </thead>
              <tbody>
                {backups.data?.map((b) => (
                  <tr key={b.fileName} className="group border-b border-border/60" title={b.fileName}>
                    <td className="py-1.5 tabular-nums">{b.createdAt}</td>
                    <td className="py-1.5">{backupLabel(b.label)}</td>
                    <td className="py-1.5 text-right tabular-nums text-muted-foreground">{formatSize(b.size)}</td>
                    <td className="py-1 text-right">
                      {restoring === b.fileName ? (
                        <LoaderCircle size={13} className="ml-auto animate-spin text-muted-foreground" />
                      ) : (
                        <button
                          onClick={() => restore(b)}
                          disabled={restoring !== null}
                          className="rounded px-1.5 py-0.5 text-xs text-muted-foreground opacity-0 group-hover:opacity-100 hover:bg-muted hover:text-foreground focus:opacity-100 disabled:opacity-0"
                        >
                          恢复
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </section>
  );
}
