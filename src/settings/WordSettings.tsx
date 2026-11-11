// 设置 › Word：安装 / 卸载 Word 全局模板与桥接脚本，看本机服务和引文排版器的状态（decisions/0016）。
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, FolderOpen, LoaderCircle } from "lucide-react";
import { api, type WordInstallReport } from "@/shared/ipc";
import { useUi } from "@/shared/ui";

function Light({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  return (
    <span className="flex items-center gap-1.5">
      {ok ? <CircleCheck size={13} className="shrink-0 text-emerald-600" /> : <CircleAlert size={13} className="shrink-0 text-amber-600" />}
      <span>{children}</span>
    </span>
  );
}

export function WordSettingsSection() {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const status = useQuery({ queryKey: ["wordStatus"], queryFn: api.wordStatus, staleTime: 0 });
  const refresh = () => qc.invalidateQueries({ queryKey: ["wordStatus"] });
  const install = useMutation({
    mutationFn: api.wordInstall,
    onSuccess: (r: WordInstallReport) => {
      refresh();
      if (r.templateError) placeTemplate(r.status.startupDir);
      else notify("已安装。重新打开 Word 后，顶栏会出现「Pith」选项卡");
    },
  });
  const uninstall = useMutation({ mutationFn: api.wordUninstall, onSuccess: refresh });
  // macOS 只认用户本人的动作：打开一个已经指向 Word 启动文件夹的对话框，用户点「打开」，Pith 就能写进去
  const into = useMutation({
    mutationFn: async (startupDir: string) => {
      const dir = await api.pickFolder("Pith 要把模板放进 Word 的启动文件夹：直接点「打开」", startupDir);
      return dir ? api.wordInstallInto(dir) : null;
    },
    onSuccess: (r) => {
      refresh();
      if (r) notify("模板已放进 Word 启动文件夹。重新打开 Word 后，顶栏会出现「Pith」选项卡");
    },
  });
  const placeTemplate = (startupDir: string) => into.mutate(startupDir);
  const reveal = useMutation({ mutationFn: api.wordReveal });
  const s = status.data;
  const inst = s?.install;
  const installed = !!inst?.record;
  const otherDir = inst?.bridgeDataDir && s && inst.bridgeDataDir !== s.dataDir ? inst.bridgeDataDir : null;
  // 桥接脚本要启动的 Pith 不在原处了（清过编译目录，或 Pith 挪进了「应用程序」）
  const moved = !!inst?.bridgeLaunchMissing && !otherDir;

  const askUninstall = async () => {
    const ok = await confirm({
      title: "从 Word 卸载 Pith？",
      message: "会删掉 Word 启动文件夹里的 Pith.dotm 和 Word 的 Pith 脚本。文档里已经插入的引文不受影响，重新安装后照常刷新。Word 开着的话，重新打开后「Pith」选项卡才会消失。",
      confirm: "卸载",
      danger: true,
    });
    if (ok) uninstall.mutate();
  };

  const busy = install.isPending || uninstall.isPending;

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-xs font-medium text-muted-foreground">Word</h2>
        <div className="flex gap-2">
          {installed && (
            <button onClick={askUninstall} disabled={busy} className="rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
              卸载
            </button>
          )}
          <button
            onClick={() => install.mutate()}
            disabled={busy || !s}
            className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50"
          >
            {install.isPending && <LoaderCircle size={13} className="animate-spin" />}
            {!installed ? "安装到 Word" : inst?.upToDate && !otherDir && !moved ? "重新安装" : "更新"}
          </button>
        </div>
      </div>
      <p className="mb-3 text-xs text-muted-foreground">
        安装后重新打开 Word，顶栏会出现「Pith」选项卡（按钮与提示是英文）：Add/Edit Citation 弹出 Pith 的搜索窗选文献、填页码（光标在引文里时改这处引文或删除它）、Bibliography
        插入参考文献表、Refresh 刷新全部引文、Style 切换样式、Convert Zotero 把文档里的 Zotero 引文换成 Pith 引文、Open Pith 切到 Pith。Word 里点按钮时 Pith 没开，会在后台自动打开。
        macOS 不让 Pith 自己写 Word 的启动文件夹：安装时会弹出一个已经打开那个文件夹的对话框，点「打开」就好。
      </p>
      {status.isLoading && <LoaderCircle size={14} className="animate-spin text-muted-foreground" />}
      {s && inst && (
        <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-x-3 gap-y-2 text-sm">
          <dt className="text-muted-foreground">Word 模板</dt>
          <dd>
            {!inst.record ? (
              <Light ok={false}>未安装</Light>
            ) : inst.manualTemplate ? (
              <Light ok={false}>等你手动放进 Word 启动文件夹</Light>
            ) : (
              <Light ok={inst.upToDate}>
                {inst.upToDate ? "已安装" : "旧版本，需要更新"}（{inst.record.version}，{inst.record.installedAt.slice(0, 10)}）
              </Light>
            )}
          </dd>
          <dt className="text-muted-foreground">Word 脚本</dt>
          <dd>
            <Light ok={inst.scriptInstalled && !otherDir && !moved}>
              {inst.scriptInstalled ? (otherDir ? "已安装，但连的是另一个数据目录" : moved ? "已安装，但它记的 Pith 已经不在原处" : "已安装") : "未安装"}
            </Light>
            {moved && <p className="mt-0.5 text-xs text-muted-foreground">Word 里点按钮时会打不开 Pith。点「更新」改成现在这个 Pith。</p>}
            {otherDir && (
              <p className="mt-0.5 text-xs text-muted-foreground">
                现在连到 <code className="selectable font-mono">{otherDir}</code>（开发版和正式版各自安装时会互相覆盖）。点「更新」改连这里。
              </p>
            )}
          </dd>
          <dt className="text-muted-foreground">本机服务</dt>
          <dd>
            <Light ok={s.servicePort !== null}>{s.servicePort !== null ? `运行中（127.0.0.1:${s.servicePort}）` : `没有启动：${s.serviceError ?? "原因不明"}`}</Light>
          </dd>
          <dt className="text-muted-foreground">引文排版器</dt>
          <dd>
            <Light ok={s.rendererReady}>{s.rendererReady ? `就绪（新文档默认 ${s.styles[0]?.label ?? ""}）` : "还没加载好"}</Light>
          </dd>
          {!s.wordInstalled && (
            <>
              <dt className="text-muted-foreground">Microsoft Word</dt>
              <dd>
                <Light ok={false}>「应用程序」里没有找到 Microsoft Word</Light>
              </dd>
            </>
          )}
        </dl>
      )}
      {inst?.manualTemplate && (
        <div className="mt-3 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-xs dark:bg-amber-950/30">
          <p className="mb-1.5 font-medium">模板还没放进 Word 的启动文件夹（macOS 不让 Pith 自己写）：</p>
          <p className="mb-2 text-muted-foreground">
            点「放进 Word 启动文件夹」，弹出的对话框已经打开了那个文件夹，直接点「打开」；然后重新打开 Word。也可以手动放：「显示模板」后把 Pith.dotm 拖进「打开 Word 启动文件夹」打开的文件夹。
          </p>
          {install.data?.templateError && <p className="mb-2 break-all text-muted-foreground">原因：{install.data.templateError}</p>}
          <div className="flex gap-2">
            <button
              onClick={() => placeTemplate(inst.startupDir)}
              disabled={into.isPending}
              className="flex items-center gap-1 rounded-md bg-primary px-2 py-1 text-primary-foreground disabled:opacity-50"
            >
              {into.isPending ? <LoaderCircle size={13} className="animate-spin" /> : <FolderOpen size={13} />}
              放进 Word 启动文件夹
            </button>
            <button onClick={() => reveal.mutate("manual")} className="flex items-center gap-1 rounded-md border bg-background px-2 py-1 hover:bg-muted">
              <FolderOpen size={13} />
              显示模板
            </button>
            <button onClick={() => reveal.mutate("startup")} className="flex items-center gap-1 rounded-md border bg-background px-2 py-1 hover:bg-muted">
              <FolderOpen size={13} />
              打开 Word 启动文件夹
            </button>
          </div>
        </div>
      )}
      {(install.error || uninstall.error || reveal.error || into.error || status.error) && (
        <p className="mt-2 text-xs text-destructive">{String(install.error ?? uninstall.error ?? reveal.error ?? into.error ?? status.error)}</p>
      )}
    </div>
  );
}
