// 界面通用部件（不引入组件库）：弹窗（确认 / 输入 / 选集合）、右键菜单、提示条、就地编辑的文本、执行修改并刷新。
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, CircleCheck, Folder, TriangleAlert, X } from "lucide-react";
import { api, type Check, type CollectionNode } from "@/shared/ipc";

// ---------------- 弹窗与提示 ----------------

type ConfirmOpts = { title: string; message?: React.ReactNode; confirm?: string; danger?: boolean };
type PromptOpts = { title: string; label?: string; initial?: string; confirm?: string; placeholder?: string };
type PickOpts = { title: string; confirm?: string; allowRoot?: boolean; exclude?: number };
type ToastAction = { label: string; onClick: () => void };
type MenuItem = { label: string; onClick: () => void; danger?: boolean; disabled?: boolean } | "-";

type Ui = {
  confirm: (o: ConfirmOpts) => Promise<boolean>;
  prompt: (o: PromptOpts) => Promise<string | null>;
  /** 选一个集合；allowRoot 时可选「顶层」（返回 0） */
  pickCollection: (o: PickOpts) => Promise<number | null>;
  /** action：提示条上的按钮（如「撤销」），点了就关掉提示 */
  notify: (msg: string, tone?: "info" | "error", action?: ToastAction) => void;
  menu: (e: React.MouseEvent, items: MenuItem[]) => void;
};

const UiContext = createContext<Ui | null>(null);

export function useUi(): Ui {
  const ui = useContext(UiContext);
  if (!ui) throw new Error("UiProvider 缺失");
  return ui;
}

type Dialog =
  | { kind: "confirm"; o: ConfirmOpts; resolve: (v: boolean) => void }
  | { kind: "prompt"; o: PromptOpts; resolve: (v: string | null) => void }
  | { kind: "pick"; o: PickOpts; resolve: (v: number | null) => void };

export function UiProvider({ children }: { children: React.ReactNode }) {
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [toasts, setToasts] = useState<{ id: number; msg: string; tone: "info" | "error"; action?: ToastAction }[]>([]);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);
  const notify = useCallback(
    (msg: string, tone: "info" | "error" = "info", action?: ToastAction) => {
      const id = nextId.current++;
      setToasts((t) => [...t, { id, msg, tone, action }]);
      setTimeout(() => dismiss(id), tone === "error" ? 8000 : action ? 6000 : 3500);
    },
    [dismiss],
  );

  const ui = useMemo<Ui>(
    () => ({
      confirm: (o) => new Promise((resolve) => setDialog({ kind: "confirm", o, resolve })),
      prompt: (o) => new Promise((resolve) => setDialog({ kind: "prompt", o, resolve })),
      pickCollection: (o) => new Promise((resolve) => setDialog({ kind: "pick", o, resolve })),
      notify,
      menu: (e, items) => {
        e.preventDefault();
        e.stopPropagation();
        setMenu({ x: e.clientX, y: e.clientY, items });
      },
    }),
    [notify],
  );

  const close = () => setDialog(null);

  return (
    <UiContext.Provider value={ui}>
      {children}
      {menu && <ContextMenu {...menu} onClose={() => setMenu(null)} />}
      {dialog?.kind === "confirm" && (
        <ConfirmDialog
          o={dialog.o}
          onDone={(v) => {
            dialog.resolve(v);
            close();
          }}
        />
      )}
      {dialog?.kind === "prompt" && (
        <PromptDialog
          o={dialog.o}
          onDone={(v) => {
            dialog.resolve(v);
            close();
          }}
        />
      )}
      {dialog?.kind === "pick" && (
        <PickCollectionDialog
          o={dialog.o}
          onDone={(v) => {
            dialog.resolve(v);
            close();
          }}
        />
      )}
      <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex max-w-sm flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`selectable pointer-events-auto flex items-center gap-3 rounded-md border px-3 py-2 text-xs shadow-md ${t.tone === "error" ? "border-destructive/40 bg-card text-destructive" : "bg-card"}`}
          >
            <span className="min-w-0 flex-1">{t.msg}</span>
            {t.action && (
              <button
                onClick={() => {
                  dismiss(t.id);
                  t.action?.onClick();
                }}
                className="shrink-0 font-medium text-primary hover:underline"
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </UiContext.Provider>
  );
}

export function Modal({ title, children, onClose, width = "w-96" }: { title: string; children: React.ReactNode; onClose: () => void; width?: string }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30" onMouseDown={onClose}>
      <div className={`${width} max-w-[90vw] rounded-lg border bg-card p-4 shadow-xl`} onMouseDown={(e) => e.stopPropagation()}>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button onClick={onClose} className="rounded p-0.5 text-muted-foreground hover:bg-muted">
            <X size={14} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

function Buttons({ onCancel, onOk, ok, danger, disabled }: { onCancel: () => void; onOk: () => void; ok: string; danger?: boolean; disabled?: boolean }) {
  return (
    <div className="mt-4 flex justify-end gap-2">
      <button onClick={onCancel} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
        取消
      </button>
      <button
        onClick={onOk}
        disabled={disabled}
        className={`rounded-md px-3 py-1 text-xs disabled:opacity-40 ${danger ? "bg-destructive text-background" : "bg-primary text-primary-foreground"}`}
      >
        {ok}
      </button>
    </div>
  );
}

function ConfirmDialog({ o, onDone }: { o: ConfirmOpts; onDone: (v: boolean) => void }) {
  return (
    <Modal title={o.title} onClose={() => onDone(false)}>
      {o.message && <div className="text-xs leading-relaxed text-muted-foreground">{o.message}</div>}
      <Buttons onCancel={() => onDone(false)} onOk={() => onDone(true)} ok={o.confirm ?? "确定"} danger={o.danger} />
    </Modal>
  );
}

function PromptDialog({ o, onDone }: { o: PromptOpts; onDone: (v: string | null) => void }) {
  const [v, setV] = useState(o.initial ?? "");
  const ok = () => v.trim() && onDone(v.trim());
  return (
    <Modal title={o.title} onClose={() => onDone(null)}>
      {o.label && <p className="mb-1.5 text-xs text-muted-foreground">{o.label}</p>}
      <input
        autoFocus
        value={v}
        placeholder={o.placeholder}
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && ok()}
        className="selectable w-full rounded-md border bg-background px-2 py-1 outline-none focus:border-ring"
      />
      <Buttons onCancel={() => onDone(null)} onOk={ok} ok={o.confirm ?? "确定"} disabled={!v.trim()} />
    </Modal>
  );
}

function PickCollectionDialog({ o, onDone }: { o: PickOpts; onDone: (v: number | null) => void }) {
  const cols = useQuery({ queryKey: ["collections"], queryFn: api.collectionsList });
  const [chosen, setChosen] = useState<number | null>(null);
  return (
    <Modal title={o.title} onClose={() => onDone(null)}>
      <div className="max-h-72 overflow-y-auto rounded-md border p-1">
        {o.allowRoot && (
          <button onClick={() => setChosen(0)} className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left ${chosen === 0 ? "bg-accent" : "hover:bg-muted"}`}>
            <span className="text-muted-foreground">（顶层）</span>
          </button>
        )}
        {cols.data && cols.data.length === 0 && !o.allowRoot && <p className="p-2 text-xs text-muted-foreground">还没有集合</p>}
        {cols.data && <CollectionTree nodes={cols.data} exclude={o.exclude} chosen={chosen} onChoose={setChosen} />}
      </div>
      <Buttons onCancel={() => onDone(null)} onOk={() => chosen !== null && onDone(chosen)} ok={o.confirm ?? "确定"} disabled={chosen === null} />
    </Modal>
  );
}

/** 弹窗里的集合树：全部展开；exclude 那一枝（移动集合时的自己）不能选 */
function CollectionTree({ nodes, exclude, chosen, onChoose }: { nodes: CollectionNode[]; exclude?: number; chosen: number | null; onChoose: (id: number) => void }) {
  const children = useMemo(() => childrenMap(nodes), [nodes]);
  const render = (parent: number | null, depth: number): React.ReactNode =>
    (children.get(parent) ?? []).map((n) =>
      n.id === exclude ? null : (
        <div key={n.id}>
          <button
            onClick={() => onChoose(n.id)}
            style={{ paddingLeft: 8 + depth * 14 }}
            className={`flex w-full items-center gap-1.5 rounded py-1 pr-2 text-left ${chosen === n.id ? "bg-accent text-accent-foreground" : "hover:bg-muted"}`}
          >
            <Folder size={13} className="shrink-0 text-muted-foreground" />
            <span className="truncate">{n.name}</span>
          </button>
          {render(n.id, depth + 1)}
        </div>
      ),
    );
  return <>{render(null, 0)}</>;
}

export function childrenMap(nodes: CollectionNode[]) {
  const m = new Map<number | null, CollectionNode[]>();
  const ids = new Set(nodes.map((n) => n.id));
  for (const n of nodes) {
    const parent = n.parentId !== null && ids.has(n.parentId) ? n.parentId : null;
    m.set(parent, [...(m.get(parent) ?? []), n]);
  }
  return m;
}

function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
  }, [x, y]);
  useEffect(() => {
    const close = () => onClose();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);
  return (
    <div ref={ref} style={{ left: pos.x, top: pos.y }} className="fixed z-50 min-w-40 rounded-md border bg-card py-1 shadow-lg" onMouseDown={(e) => e.stopPropagation()}>
      {items.map((it, i) =>
        it === "-" ? (
          <div key={i} className="my-1 border-t" />
        ) : (
          <button
            key={i}
            disabled={it.disabled}
            onClick={() => {
              onClose();
              it.onClick();
            }}
            className={`block w-full px-3 py-1 text-left text-xs hover:bg-muted disabled:opacity-40 ${it.danger ? "text-destructive" : ""}`}
          >
            {it.label}
          </button>
        ),
      )}
    </div>
  );
}

// ---------------- 核对结果 ----------------

/** 逐类核对的结果（导入、导出、恢复共用）：一行摘要，展开看每一类的数量。columns：两边的名字 */
export function Checks({ title, checks, columns = ["来源", "库里"] }: { title: string; checks: Check[]; columns?: [string, string] }) {
  if (checks.length === 0) return null;
  const ok = checks.every((c) => c.ok);
  return (
    <details className="mt-4 rounded-md border px-3 py-2">
      <summary className="flex cursor-default items-center gap-1.5 text-xs">
        {ok ? <CircleCheck size={14} className="text-emerald-600 dark:text-emerald-400" /> : <TriangleAlert size={14} className="text-destructive" />}
        {title}：{checks.length} 类{ok ? "全部一致" : "有不一致"}
      </summary>
      <table className="mt-2 w-full text-left text-xs">
        <thead className="text-muted-foreground">
          <tr>
            <th className="py-0.5 font-normal">类别</th>
            <th className="py-0.5 text-right font-normal">{columns[0]}</th>
            <th className="py-0.5 text-right font-normal">{columns[1]}</th>
            <th className="py-0.5 text-right font-normal" />
          </tr>
        </thead>
        <tbody>
          {checks.map((c) => (
            <tr key={c.category}>
              <td className="py-0.5">{c.label}</td>
              <td className="py-0.5 text-right tabular-nums">{c.source}</td>
              <td className="py-0.5 text-right tabular-nums">{c.stored}</td>
              <td className="py-0.5 text-right">{c.ok ? "一致" : <span className="text-destructive">不一致</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </details>
  );
}

// ---------------- 修改并刷新 ----------------

const AFTER_CHANGE = ["items", "overview", "collections", "tags", "item", "attachment", "note", "collection", "backups", "annotations", "metadata", "lookupStatus"];

/** 执行一个会改库的调用：成功后刷新列表 / 详情 / 侧栏，失败弹出原因。
 *  返回：成功时是调用的结果（没有结果的调用给 null），失败时是 undefined——所以用 `!== undefined` 判断成功。 */
export function useMutate() {
  const qc = useQueryClient();
  const { notify } = useUi();
  return useCallback(
    async <T,>(fn: () => Promise<T>, success?: string | ((r: T) => string)): Promise<([T] extends [void] ? null : T) | undefined> => {
      try {
        const r = await fn();
        for (const k of AFTER_CHANGE) qc.invalidateQueries({ queryKey: [k] });
        if (success) notify(typeof success === "string" ? success : success(r));
        return (r === undefined ? null : r) as [T] extends [void] ? null : T;
      } catch (e) {
        notify(String(e), "error");
        return undefined;
      }
    },
    [qc, notify],
  );
}

// ---------------- 记住的界面偏好 ----------------

/** 只影响界面显示的小偏好（折叠、勾选）记在 WebView 的 localStorage；文献数据一律走 Rust */
export function useStoredState<T>(key: string, initial: T) {
  const [v, setV] = useState<T>(() => {
    try {
      const s = localStorage.getItem(`ui.${key}`);
      return s === null ? initial : (JSON.parse(s) as T);
    } catch {
      return initial;
    }
  });
  const set = useCallback(
    (x: T) => {
      setV(x);
      try {
        localStorage.setItem(`ui.${key}`, JSON.stringify(x));
      } catch {
        // 存不下就只在这次打开里有效
      }
    },
    [key],
  );
  return [v, set] as const;
}

// ---------------- 标签输入 ----------------

/**
 * 标签输入框，边打字边提示已有标签（系统的 datalist 在 macOS 上和输入框一样窄，名字被截断）。
 * 空着时列出最常用的；↑↓ 选、Enter 用选中的（没选就用打的字）、Esc 收起。
 * onPick：选了提示里的标签时调用；不传就只把名字填进输入框。
 */
export function TagInput({
  value,
  onChange,
  onEnter,
  onPick,
  onBlur,
  exclude = [],
  placeholder,
  className = "",
  boxClassName = "",
}: {
  value: string;
  onChange: (v: string) => void;
  onEnter: (v: string) => void;
  onPick?: (name: string) => void;
  onBlur?: () => void;
  exclude?: string[];
  placeholder?: string;
  className?: string;
  /** 外框的 class（如占满一行时 "flex-1"） */
  boxClassName?: string;
}) {
  const all = useQuery({ queryKey: ["tags"], queryFn: api.tagsList });
  const [open, setOpen] = useState(false);
  const [hi, setHi] = useState(-1);
  const q = value.trim().toLowerCase();
  const matches = useMemo(() => {
    const pool = (all.data ?? []).filter((t) => !exclude.includes(t.name) && t.name.toLowerCase() !== q);
    if (!q) return [...pool].sort((a, b) => b.count - a.count).slice(0, 8);
    const hits = pool.filter((t) => t.name.toLowerCase().includes(q));
    return [...hits.filter((t) => t.name.toLowerCase().startsWith(q)), ...hits.filter((t) => !t.name.toLowerCase().startsWith(q))].slice(0, 8);
  }, [all.data, exclude, q]);
  useEffect(() => setHi(-1), [q]);
  const shown = open && matches.length > 0;
  const pick = (name: string) => {
    setOpen(false);
    if (onPick) onPick(name);
    else onChange(name);
  };
  return (
    <span className={`relative inline-flex min-w-0 ${boxClassName}`}>
      <input
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => {
          setOpen(false);
          onBlur?.();
        }}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return;
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setOpen(true);
            if (matches.length) setHi((h) => (e.key === "ArrowDown" ? (h + 1) % matches.length : (h <= 0 ? matches.length : h) - 1));
          } else if (e.key === "Enter") {
            e.preventDefault();
            if (shown && hi >= 0) pick(matches[hi].name);
            else if (value.trim()) onEnter(value.trim());
          } else if (e.key === "Escape" && shown) {
            e.stopPropagation();
            setOpen(false);
          }
        }}
        placeholder={placeholder}
        className={className}
      />
      {shown && (
        <ul className="absolute top-full left-0 z-30 mt-1 max-h-56 w-max max-w-72 min-w-44 overflow-y-auto rounded-md border bg-card py-1 text-xs shadow-md">
          {matches.map((t, i) => (
            <li key={t.id}>
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(t.name)}
                onMouseEnter={() => setHi(i)}
                className={`flex w-full items-center gap-3 px-2.5 py-1 text-left ${i === hi ? "bg-accent text-accent-foreground" : ""}`}
              >
                <span className="min-w-0 flex-1 truncate">{t.name}</span>
                <span className="shrink-0 text-[11px] text-muted-foreground tabular-nums">{t.count}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}

// ---------------- 就地编辑 ----------------

/** 点一下就能改的文本：Enter（多行时 ⌘Enter）或离开时保存，Esc 取消；值没变不保存。 */
export function EditableText({
  value,
  onSave,
  placeholder = "（空）",
  multiline,
  disabled,
  className = "",
  startEditing = false,
}: {
  value: string | null;
  onSave: (v: string) => void | Promise<unknown>;
  placeholder?: string;
  multiline?: boolean;
  disabled?: boolean;
  className?: string;
  /** 一出现就进入编辑（如刚放下的便签） */
  startEditing?: boolean;
}) {
  const [editing, setEditing] = useState(startEditing);
  const [draft, setDraft] = useState(value ?? "");
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (!editing) setDraft(value ?? "");
  }, [value, editing]);
  // 单行字段也用会长高的文本框：长标题、长刊名改的时候能看全
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || multiline) return;
    el.style.height = "auto";
    // 变高后外面可能冒出滚动条、框变窄、多折一行，所以量到稳定为止（含边框）
    for (let i = 0; i < 3; i++) {
      const need = el.scrollHeight + el.offsetHeight - el.clientHeight;
      if (Math.abs(el.offsetHeight - need) < 1) break;
      el.style.height = `${need}px`;
    }
  }, [draft, editing, multiline]);
  // 进入编辑时光标放到末尾
  useEffect(() => {
    const el = box.current;
    if (editing && el) el.setSelectionRange(el.value.length, el.value.length);
  }, [editing]);
  const commit = () => {
    setEditing(false);
    if (draft.trim() !== (value ?? "").trim()) onSave(draft);
  };
  if (!editing || disabled) {
    return (
      <span
        onClick={() => !disabled && setEditing(true)}
        className={`-mx-1 block rounded px-1 ${disabled ? "" : "cursor-text hover:bg-muted"} ${value ? "" : "text-muted-foreground"} ${multiline ? "whitespace-pre-wrap" : ""} ${className}`}
      >
        {value || placeholder}
      </span>
    );
  }
  const common = {
    ref: box,
    autoFocus: true,
    value: draft,
    // 单行字段：粘贴进来的换行换成空格
    onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => setDraft(multiline ? e.target.value : e.target.value.replace(/\s*[\r\n]+\s*/g, " ")),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.nativeEvent.isComposing) return;
      if (e.key === "Escape") {
        setDraft(value ?? "");
        setEditing(false);
      } else if (e.key === "Enter" && (!multiline || e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        commit();
      }
    },
    className: `selectable -mx-1 block w-[calc(100%+0.5rem)] rounded border border-ring bg-background px-1 outline-none ${multiline ? "" : "resize-none overflow-hidden"} ${className}`,
  };
  return <textarea rows={multiline ? Math.min(12, Math.max(3, draft.split("\n").length + 1)) : 1} {...common} />;
}

export function Chevron({ open }: { open: boolean }) {
  return <ChevronRight size={12} className={open ? "rotate-90" : ""} />;
}
