// 联网补全的界面部分（规则都在 Rust：src-tauri/src/metadata/）：
// - 第一次联网前说明范围、征得同意（需求 L3）；
// - 看着后台队列的进度，一轮结束时提示结果；
// - 「按 DOI / ISBN 添加」对话框。
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type AddReport, type MetadataSettings } from "@/shared/ipc";
import { Modal, useUi } from "@/shared/ui";

type MetadataApi = {
  settings: MetadataSettings | undefined;
  /** 已打开就直接 true；还没问过就弹出说明，用户同意才 true；关掉了就提示去设置里开 */
  ensureConsent: () => Promise<boolean>;
  /** 手动联网补全这些文献（后台排队） */
  lookup: (ids: number[]) => Promise<void>;
  /** 按 DOI / ISBN 新建；返回新建的文献 id（取消为 null） */
  addByIdentifier: (collectionId: number | null) => Promise<number[] | null>;
  /** 导入完成后：还没问过就问；同意了就把这次导入的文献排进队列（已打开自动补全的，后端已经排好） */
  afterImport: (runId: number) => Promise<void>;
};

const Ctx = createContext<MetadataApi | null>(null);

export function useMetadata(): MetadataApi {
  const m = useContext(Ctx);
  if (!m) throw new Error("MetadataProvider 缺失");
  return m;
}

const REFRESH = ["items", "item", "overview", "metadata", "tags"];

export function MetadataProvider({ children, onReview }: { children: React.ReactNode; onReview: () => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const settings = useQuery({ queryKey: ["metadataSettings"], queryFn: api.metadataSettingsGet });
  const status = useQuery({
    queryKey: ["lookupStatus"],
    queryFn: api.metadataStatus,
    refetchInterval: (q) => (q.state.data?.running ? 1000 : false),
  });
  const [consent, setConsent] = useState<((ok: boolean) => void) | null>(null);
  const [adding, setAdding] = useState<{ collectionId: number | null; resolve: (ids: number[] | null) => void } | null>(null);

  // 一轮跑完：刷新列表与详情，提示结果
  const seen = useRef<{ run: number; running: boolean; done: number } | null>(null);
  const p = status.data;
  useEffect(() => {
    if (!p) return;
    const prev = seen.current;
    seen.current = { run: p.run, running: p.running, done: p.done };
    if (p.running) {
      // 查完一条刷新一次（列表、详情里能看到补上的字段）
      if (prev?.done !== p.done) for (const k of REFRESH) qc.invalidateQueries({ queryKey: [k] });
      return;
    }
    if (!prev || (prev.run === p.run && !prev.running) || p.total === 0) return;
    for (const k of REFRESH) qc.invalidateQueries({ queryKey: [k] });
    const looked = p.done - p.skipped;
    const parts: string[] = [];
    if (looked) parts.push(`查了 ${looked} 条`);
    if (p.filledItems) parts.push(`${p.filledItems} 条补上了 ${p.filledFields} 处空字段`);
    if (p.notFound) parts.push(`${p.notFound} 条没找到`);
    if (p.failed) parts.push(`${p.failed} 条失败`);
    if (looked && !p.filledItems && !p.conflictItems && !p.notFound && !p.failed) parts.push("没有需要补的");
    if (p.skipped) parts.push(looked ? `${p.skipped} 条已经查过或没有 DOI / ISBN，跳过` : "都已经查过了（或没有 DOI / ISBN），没有再查");
    const msg = `${p.stopped ? `联网补全停下了（${p.stopped}）` : "联网补全完成"}：${parts.join("，")}${p.conflictItems ? `；${p.conflictItems} 条与现有不同，等你确认` : ""}`;
    ui.notify(msg, p.stopped && p.stopped !== "已停止" ? "error" : "info", p.conflictItems ? { label: "去确认", onClick: onReview } : undefined);
  }, [p, qc, ui, onReview]);

  const ensureConsent = useCallback(async () => {
    const s: MetadataSettings = settings.data ?? (await api.metadataSettingsGet());
    if (s.enabled === true) return true;
    if (s.enabled === false) {
      ui.notify("联网补全已关闭，可以在「设置」里打开", "error");
      return false;
    }
    return new Promise<boolean>((resolve) => setConsent(() => resolve));
  }, [settings.data, qc, ui]);

  const lookup = useCallback(
    async (ids: number[]) => {
      if (!ids.length || !(await ensureConsent())) return;
      try {
        const n = await api.metadataLookup(ids);
        ui.notify(n > 1 ? `正在联网补全 ${n} 条…` : "正在联网补全…");
      } catch (e) {
        ui.notify(String(e), "error");
      }
      qc.invalidateQueries({ queryKey: ["lookupStatus"] });
    },
    [ensureConsent, ui, qc],
  );

  const addByIdentifier = useCallback(
    async (collectionId: number | null) => {
      if (!(await ensureConsent())) return null;
      return new Promise<number[] | null>((resolve) => setAdding({ collectionId, resolve }));
    },
    [ensureConsent],
  );

  const afterImport = useCallback(
    async (runId: number) => {
      const s = await qc.fetchQuery({ queryKey: ["metadataSettings"], queryFn: api.metadataSettingsGet, staleTime: 0 });
      if (s.enabled === null && (await ensureConsent())) {
        await api.metadataLookupRun(runId).catch((e) => ui.notify(String(e), "error"));
      }
      qc.invalidateQueries({ queryKey: ["lookupStatus"] });
    },
    [qc, ensureConsent, ui],
  );

  const value = useMemo<MetadataApi>(() => ({ settings: settings.data, ensureConsent, lookup, addByIdentifier, afterImport }), [settings.data, ensureConsent, lookup, addByIdentifier, afterImport]);

  return (
    <Ctx.Provider value={value}>
      {children}
      {consent && (
        <ConsentDialog
          initialEmail={settings.data?.email ?? ""}
          onDone={async (choice) => {
            const resolve = consent;
            setConsent(null);
            if (choice) {
              try {
                await api.metadataSettingsSet({ enabled: true, auto: choice.auto, email: choice.email });
              } catch (e) {
                ui.notify(String(e), "error");
                resolve(false);
                return;
              }
            } else {
              await api.metadataSettingsSet({ enabled: false }).catch(() => {});
            }
            await qc.invalidateQueries({ queryKey: ["metadataSettings"] });
            resolve(!!choice);
          }}
        />
      )}
      {adding && (
        <AddDialog
          collectionId={adding.collectionId}
          onClose={(ids) => {
            adding.resolve(ids);
            setAdding(null);
          }}
        />
      )}
    </Ctx.Provider>
  );
}

/** 第一次联网前的说明：发什么、发给谁；邮箱；要不要自动查 */
function ConsentDialog({ initialEmail, onDone }: { initialEmail: string; onDone: (choice: { auto: boolean; email: string } | null) => void }) {
  const [email, setEmail] = useState(initialEmail);
  const [auto, setAuto] = useState(true);
  return (
    <Modal title="联网补全元数据" onClose={() => onDone(null)} width="w-[460px]">
      <div className="space-y-3 text-xs leading-relaxed">
        <p>按文献的 DOI / ISBN 联网查出版信息：只补<strong>空着的</strong>字段；和现有不同的放进「待确认」，由你决定用不用。补上的可以撤销。</p>
        <div className="rounded-md bg-muted px-3 py-2">
          <p className="mb-1 font-medium">会发送什么、发给谁</p>
          <ul className="list-disc space-y-0.5 pl-4 text-muted-foreground">
            <li>DOI → doi.org（再转到 Crossref、DataCite 等登记机构），附带下面的联系邮箱（Crossref 的「礼貌请求」，带了限流更宽松）</li>
            <li>ISBN → Open Library（openlibrary.org），不带邮箱</li>
            <li>只发 DOI / ISBN 本身；标题、笔记、PDF、标注都不发</li>
          </ul>
        </div>
        <label className="block">
          <span className="text-muted-foreground">联系邮箱（可留空）</span>
          <input
            autoFocus
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="name@example.com"
            className="selectable mt-1 w-full rounded-md border bg-background px-2 py-1 outline-none focus:border-ring"
          />
        </label>
        <label className="flex items-center gap-2">
          <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} />
          导入后、填写 DOI / ISBN 后自动查
        </label>
        <p className="text-muted-foreground">随时可以在「设置」里关闭或修改。</p>
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button onClick={() => onDone(null)} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
          不联网
        </button>
        <button onClick={() => onDone({ auto, email: email.trim() })} className="rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground">
          同意并开始
        </button>
      </div>
    </Modal>
  );
}

/** 粘贴 DOI / ISBN（可以多个）→ 联网查 → 新建文献；库里已有的不重复建 */
function AddDialog({ collectionId, onClose }: { collectionId: number | null; onClose: (ids: number[] | null) => void }) {
  const qc = useQueryClient();
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<AddReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.metadataAdd(text, collectionId);
      setReport(r);
      for (const k of ["items", "overview", "collections", "tags", "metadata"]) qc.invalidateQueries({ queryKey: [k] });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const done = () => onClose(report ? [...report.created, ...report.existing.map((e) => e.itemId)] : null);
  return (
    <Modal title="按 DOI / ISBN 添加" onClose={busy ? () => {} : done} width="w-[460px]">
      {!report ? (
        <>
          <p className="mb-1.5 text-xs text-muted-foreground">每行一个，也可以用空格、逗号隔开。DOI 可以带 https://doi.org/ 前缀。</p>
          <textarea
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            rows={5}
            placeholder={"10.1038/nature14539\n978-7-100-00000-0"}
            className="selectable w-full rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring"
          />
          {error && <p className="selectable mt-2 text-xs text-destructive">{error}</p>}
          <div className="mt-3 flex items-center justify-end gap-2">
            {busy && (
              <span className="mr-auto flex items-center gap-1.5 text-xs text-muted-foreground">
                <LoaderCircle size={13} className="animate-spin" />
                正在联网查…
              </span>
            )}
            <button onClick={done} disabled={busy} className="rounded-md border px-3 py-1 text-xs hover:bg-muted disabled:opacity-40">
              取消
            </button>
            <button onClick={run} disabled={busy || !text.trim()} className="rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-40">
              添加
            </button>
          </div>
        </>
      ) : (
        <>
          <ul className="selectable space-y-1.5 text-xs leading-relaxed">
            <li>新建了 {report.created.length} 条文献{collectionId !== null && report.created.length > 0 ? "（已放进当前集合）" : ""}。</li>
            {report.existing.length > 0 && (
              <li>
                库里已经有 {report.existing.length} 条，没有重复添加：
                <span className="text-muted-foreground">{report.existing.map((e) => e.title || e.identifier).join("；")}</span>
              </li>
            )}
            {report.notFound.length > 0 && (
              <li>
                没找到 {report.notFound.length} 个：
                <span className="text-muted-foreground">{report.notFound.map((n) => `${n.identifier}（${n.message}）`).join("；")}</span>
              </li>
            )}
            {report.invalid.length > 0 && <li className="text-muted-foreground">认不出是 DOI 还是 ISBN：{report.invalid.join("、")}</li>}
            {report.failed && <li className="text-destructive">联网失败：{report.failed}</li>}
          </ul>
          <div className="mt-4 flex justify-end">
            <button onClick={done} className="rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground">
              完成
            </button>
          </div>
        </>
      )}
    </Modal>
  );
}
