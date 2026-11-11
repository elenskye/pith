// 阅读器（Phase 4）：PDF 翻页、缩放、查找，选字高亮 / 下划线，放便签，右侧标注列表，点一条跳回原处。记住读到哪。
import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ChevronLeft, ChevronRight, ExternalLink, Minus, PanelRight, Plus, Search, StickyNote, Underline, X } from "lucide-react";
import { api, type Annotation, type ReadingState } from "@/shared/ipc";
import { useMutate, useUi } from "@/shared/ui";
import { COLORS, DEFAULT_COLOR, excerptForCopy } from "./colors";
import { FIND_NOT_FOUND, FIND_PENDING, PdfController, type FindResult, type SelectionInfo } from "./controller";
import { fetchPdf, loadPdfjs } from "./pdfjs";
import { AnnotationList } from "./AnnotationList";

type Props = { attachmentId: number; onClose: () => void; onShowItem: (itemId: number) => void };

const ZOOMS: { value: string; label: string }[] = [
  { value: "auto", label: "自动" },
  { value: "page-width", label: "适合宽度" },
  { value: "page-fit", label: "整页" },
  ...[0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4].map((z) => ({ value: String(z), label: `${z * 100}%` })),
];

const NONE: Annotation[] = [];

/** 焦点在输入框里时，快捷键不抢 */
const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));

export default function Reader({ attachmentId, onClose, onShowItem }: Props) {
  // 每次打开都重新取（阅读位置可能变了），不用缓存
  const doc = useQuery({ queryKey: ["reader", attachmentId], queryFn: () => api.readerGet(attachmentId), staleTime: 0, gcTime: 0 });
  const anns = useQuery({ queryKey: ["annotations", attachmentId], queryFn: () => api.annotationsList(attachmentId) });
  const ui = useUi();
  const mutate = useMutate();

  const containerRef = useRef<HTMLDivElement>(null);
  const ctrl = useRef<PdfController | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | { error: string }>("loading");
  const [pages, setPages] = useState<{ count: number; labels: string[] | null }>({ count: 0, labels: null });
  const [page, setPage] = useState(1);
  const [scale, setScale] = useState("auto");
  const [selected, setSelected] = useState<number | null>(null);
  const [fresh, setFresh] = useState<number | null>(null);
  const [popup, setPopup] = useState<SelectionInfo | null>(null);
  const [placing, setPlacing] = useState(false);
  const [sidebar, setSidebar] = useState(true);
  const [find, setFind] = useState<{ open: boolean; query: string; result: FindResult | null }>({ open: false, query: "", result: null });
  const [lastColor, setLastColor] = useState(DEFAULT_COLOR);
  const findInput = useRef<HTMLInputElement>(null);
  const down = useRef<{ x: number; y: number } | null>(null);

  const readonly = doc.data?.readonly ?? null;
  const list = anns.data ?? NONE;

  // ---- 打开 PDF ----
  const state = doc.data?.state ?? null;
  const ready = doc.isSuccess;
  useEffect(() => {
    if (!ready || !containerRef.current) return;
    const container = containerRef.current;
    let c: PdfController | null = null;
    let cancelled = false;
    let pending: ReadingState | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const save = () => {
      if (pending) void api.readerSaveState(attachmentId, pending).catch(() => {});
      pending = null;
    };
    (async () => {
      try {
        const lib = await loadPdfjs();
        if (cancelled) return;
        c = new PdfController(lib, container, {
          onPages: (count, labels) => {
            setPages({ count, labels });
            setStatus("ready");
          },
          onPage: setPage,
          onScale: setScale,
          onLocation: (s) => {
            pending = s;
            clearTimeout(timer);
            timer = setTimeout(save, 800);
          },
          onFind: (result) => setFind((f) => ({ ...f, result })),
          onError: (error) => setStatus({ error }),
        });
        c.openExternal = (url) => api.openUrl(url).catch((e) => ui.notify(String(e), "error"));
        ctrl.current = c;
        const data = await fetchPdf(api.fileUrl(attachmentId));
        if (cancelled) return;
        await c.open(data, state);
      } catch (e) {
        if (!cancelled) setStatus({ error: String((e as Error)?.message ?? e) });
      }
    })();
    return () => {
      cancelled = true;
      clearTimeout(timer);
      save();
      c?.destroy();
      ctrl.current = null;
    };
    // 阅读位置（state）只在打开时用一次
  }, [ready, attachmentId]);

  // ---- 标注画到页面上 ----
  useEffect(() => {
    ctrl.current?.setAnnotations(list, selected);
    // 选中哪条单独同步（不用整页重画）
  }, [list, status]);
  useEffect(() => ctrl.current?.select(selected), [selected]);

  const focusCard = (id: number | null) => {
    setSelected(id);
    if (id !== null) requestAnimationFrame(() => document.getElementById(`rl-card-${id}`)?.scrollIntoView({ block: "nearest" }));
  };
  const jump = (a: Annotation) => {
    setSelected(a.id);
    ctrl.current?.scrollTo(a);
  };

  // ---- 新建、删除 ----
  const create = async (kind: "highlight" | "underline", color: string) => {
    const s = popup;
    if (!s) return;
    const a = await mutate(() =>
      api.annotationsCreate({ attachmentId, kind, pageIndex: s.pageIndex, pageLabel: s.pageLabel, pageBox: s.pageBox, rects: s.rects, text: s.text, color, charOffset: s.charOffset }),
    );
    if (a) {
      document.getSelection()?.removeAllRanges();
      setPopup(null);
      setLastColor(color);
      focusCard(a.id);
    }
  };
  const placeNote = async (x: number, y: number) => {
    setPlacing(false);
    const p = ctrl.current?.notePlacement(x, y);
    if (!p) return;
    const a = await mutate(() => api.annotationsCreate({ attachmentId, kind: "note", pageIndex: p.pageIndex, pageLabel: p.pageLabel, pageBox: p.pageBox, rects: [p.rect], color: lastColor }));
    if (a) {
      setSidebar(true);
      setFresh(a.id);
      focusCard(a.id);
    }
  };
  const remove = useCallback(
    async (ids: number[]) => {
      if ((await mutate(() => api.annotationsDelete(ids))) === undefined) return;
      setSelected(null);
      ui.notify(`已删除 ${ids.length} 条标注`, "info", { label: "撤销", onClick: () => void mutate(() => api.annotationsRestore(ids)) });
    },
    [mutate, ui],
  );
  const copy = (text: string) =>
    navigator.clipboard.writeText(text).then(
      () => ui.notify("已复制"),
      () => ui.notify("复制失败", "error"),
    );

  // ---- 鼠标：选字 → 浮动工具条；点击 → 选中标注；放便签 ----
  // 在页面上按下、在哪里松开都算（拖选时常常松在页面外）：松开在 window 上监听
  const onMouseUp = (e: MouseEvent) => {
    const start = down.current;
    if (e.button !== 0 || !start) return;
    down.current = null;
    const click = start && Math.abs(start.x - e.clientX) < 4 && Math.abs(start.y - e.clientY) < 4;
    if (placing && click) {
      void placeNote(e.clientX, e.clientY);
      return;
    }
    const { clientX, clientY } = e;
    // 等浏览器更新完选区
    setTimeout(() => {
      const s = ctrl.current?.selection() ?? null;
      if (s === "cross-page") {
        setPopup(null);
        ui.notify("高亮不能跨页，请分两次选");
      } else if (s) {
        setPopup(s);
      } else {
        setPopup(null);
        if (click) focusCard(ctrl.current?.hitTest(clientX, clientY) ?? null);
      }
    }, 0);
  };

  const upRef = useRef(onMouseUp);
  upRef.current = onMouseUp;
  useEffect(() => {
    const up = (e: MouseEvent) => upRef.current(e);
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  // ---- 键盘 ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const c = ctrl.current;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key === "f") {
        e.preventDefault();
        setFind((f) => ({ ...f, open: true }));
        requestAnimationFrame(() => findInput.current?.select());
        return;
      }
      if (mod && (e.key === "=" || e.key === "+")) {
        e.preventDefault();
        c?.zoom(1);
        return;
      }
      if (mod && e.key === "-") {
        e.preventDefault();
        c?.zoom(-1);
        return;
      }
      if (mod && e.key === "0") {
        e.preventDefault();
        c?.setScale("auto");
        return;
      }
      if (typing(e.target)) return;
      if (e.key === "Escape") {
        if (placing) setPlacing(false);
        else if (popup) {
          document.getSelection()?.removeAllRanges();
          setPopup(null);
        } else setSelected(null);
      } else if ((e.key === "Delete" || e.key === "Backspace") && selected !== null) {
        const a = list.find((x) => x.id === selected);
        if (a?.editable) {
          e.preventDefault();
          void remove([a.id]);
        }
      } else if (!mod && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const box = containerRef.current;
        if (box && box.scrollWidth <= box.clientWidth + 1) {
          e.preventDefault();
          if (e.key === "ArrowLeft") c?.previous();
          else c?.next();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [placing, popup, selected, list, remove]);

  // ---- 查找 ----
  const runFind = (again: boolean, previous = false) => find.query.trim() && ctrl.current?.find(find.query, again, previous);
  const closeFind = () => {
    setFind({ open: false, query: "", result: null });
    ctrl.current?.closeFind();
  };

  const label = pages.labels?.[page - 1];
  const scaleOptions = ZOOMS.some((z) => z.value === scale) ? ZOOMS : [...ZOOMS, { value: scale, label: `${Math.round(Number(scale) * 100)}%` }];
  const error = doc.error ? String(doc.error) : typeof status === "object" ? status.error : null;

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      {/* 工具栏 */}
      <div className="flex h-11 shrink-0 items-center gap-2 border-b bg-sidebar px-2">
        <ToolButton onClick={onClose} title="返回文献库">
          <ArrowLeft size={15} />
          <span>文献库</span>
        </ToolButton>
        <div className="min-w-0 flex-1 px-1 leading-tight">
          <div className="truncate font-medium">{doc.data?.title ?? "…"}</div>
          {doc.data?.parent && (
            <button onClick={() => onShowItem(doc.data!.parent!.id)} className="block max-w-full truncate text-[11px] text-muted-foreground hover:text-foreground hover:underline">
              {doc.data.parent.title || "（无标题文献）"}
            </button>
          )}
        </div>
        {status === "ready" && (
          <>
            <div className="flex items-center gap-0.5">
              <ToolButton onClick={() => ctrl.current?.previous()} title="上一页" disabled={page <= 1}>
                <ChevronLeft size={15} />
              </ToolButton>
              <PageInput key={`${page}-${label}`} value={label ?? String(page)} onGo={(v) => ctrl.current?.goToLabel(v) ?? false} />
              <span className="px-1 text-xs whitespace-nowrap text-muted-foreground tabular-nums">
                {label && label !== String(page) ? `${page} / ${pages.count}` : `/ ${pages.count}`}
              </span>
              <ToolButton onClick={() => ctrl.current?.next()} title="下一页" disabled={page >= pages.count}>
                <ChevronRight size={15} />
              </ToolButton>
            </div>
            <div className="flex items-center gap-0.5">
              <ToolButton onClick={() => ctrl.current?.zoom(-1)} title="缩小（⌘−）">
                <Minus size={15} />
              </ToolButton>
              <select value={scale} onChange={(e) => ctrl.current?.setScale(e.target.value)} className="rounded border bg-background px-1 py-0.5 text-xs" title="缩放">
                {scaleOptions.map((z) => (
                  <option key={z.value} value={z.value}>
                    {z.label}
                  </option>
                ))}
              </select>
              <ToolButton onClick={() => ctrl.current?.zoom(1)} title="放大（⌘+）">
                <Plus size={15} />
              </ToolButton>
            </div>
            <div className="flex items-center gap-0.5">
              <ToolButton onClick={() => setPlacing((p) => !p)} title={readonly ?? "放便签：点一下按钮，再点页面上的位置"} active={placing} disabled={!!readonly}>
                <StickyNote size={15} />
              </ToolButton>
              <ToolButton
                onClick={() => {
                  setFind((f) => ({ ...f, open: true }));
                  requestAnimationFrame(() => findInput.current?.select());
                }}
                title="在文档里查找（⌘F）"
                active={find.open}
              >
                <Search size={15} />
              </ToolButton>
            </div>
          </>
        )}
        <ToolButton onClick={() => void api.attachmentsOpen(attachmentId).catch((e) => ui.notify(String(e), "error"))} title="用系统程序打开">
          <ExternalLink size={15} />
        </ToolButton>
        <ToolButton onClick={() => setSidebar((s) => !s)} title="标注列表" active={sidebar}>
          <PanelRight size={15} />
          {list.length > 0 && <span className="tabular-nums">{list.length}</span>}
        </ToolButton>
      </div>

      <div className="flex min-h-0 flex-1">
        <div className="relative min-w-0 flex-1">
          <div
            ref={containerRef}
            tabIndex={0}
            className={`rl-viewer absolute inset-0 overflow-auto outline-none ${placing ? "rl-placing" : ""}`}
            onMouseDown={(e) => {
              if (e.button === 0) down.current = { x: e.clientX, y: e.clientY };
            }}
            onScroll={() => popup && setPopup(null)}
          >
            <div className="pdfViewer" />
          </div>
          {(status === "loading" || error) && (
            <div className="absolute inset-0 flex items-center justify-center bg-muted p-8">
              {error ? (
                <div className="max-w-md rounded-lg border bg-card p-4 text-center">
                  <p className="selectable text-destructive">{error}</p>
                  <div className="mt-3 flex justify-center gap-2">
                    <button onClick={() => void api.attachmentsOpen(attachmentId).catch((e) => ui.notify(String(e), "error"))} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
                      用系统程序打开
                    </button>
                    <button onClick={onClose} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
                      返回文献库
                    </button>
                  </div>
                </div>
              ) : (
                <p className="text-muted-foreground">正在打开 PDF…</p>
              )}
            </div>
          )}
          {find.open && (
            <div className="absolute top-2 right-4 z-10 flex items-center gap-1 rounded-md border bg-card px-2 py-1 shadow-md">
              <Search size={13} className="text-muted-foreground" />
              <input
                ref={findInput}
                autoFocus
                value={find.query}
                onChange={(e) => {
                  const query = e.target.value;
                  setFind((f) => ({ ...f, query }));
                  if (query.trim()) ctrl.current?.find(query, false, false);
                }}
                onKeyDown={(e) => {
                  if (e.nativeEvent.isComposing) return;
                  if (e.key === "Enter") runFind(true, e.shiftKey);
                  if (e.key === "Escape") closeFind();
                }}
                placeholder="在文档里查找"
                className="selectable w-44 bg-transparent text-xs outline-none"
              />
              <span className="w-16 text-right text-[11px] text-muted-foreground tabular-nums">
                {!find.query.trim() || !find.result
                  ? ""
                  : find.result.state === FIND_PENDING
                    ? "查找中…"
                    : find.result.state === FIND_NOT_FOUND || find.result.total === 0
                      ? "没找到"
                      : `${find.result.current} / ${find.result.total}`}
              </span>
              <ToolButton onClick={() => runFind(true, true)} title="上一个（⇧Enter）">
                <ChevronLeft size={13} />
              </ToolButton>
              <ToolButton onClick={() => runFind(true)} title="下一个（Enter）">
                <ChevronRight size={13} />
              </ToolButton>
              <ToolButton onClick={closeFind} title="关闭（Esc）">
                <X size={13} />
              </ToolButton>
            </div>
          )}
          {placing && <div className="pointer-events-none absolute top-2 left-1/2 z-10 -translate-x-1/2 rounded-md bg-foreground/80 px-3 py-1 text-xs text-background">点页面上的位置放便签（Esc 取消）</div>}
        </div>
        {sidebar && (
          <AnnotationList
            list={list}
            readonly={readonly}
            selected={selected}
            fresh={fresh}
            onJump={jump}
            onDelete={(id) => void remove([id])}
            onCopy={(a) => void copy(excerptForCopy(a.text ?? "", a.pageLabel, a.pageIndex))}
          />
        )}
      </div>

      {popup && (
        <SelectionPopup
          at={popup.anchor}
          readonly={readonly}
          lastColor={lastColor}
          onHighlight={(color) => void create("highlight", color)}
          onUnderline={() => void create("underline", lastColor)}
          onCopy={() => {
            void copy(excerptForCopy(popup.text.replace(/\s+/g, " ").trim(), popup.pageLabel, popup.pageIndex));
          }}
        />
      )}
    </div>
  );
}

function ToolButton({ onClick, title, children, disabled, active }: { onClick: () => void; title: string; children: React.ReactNode; disabled?: boolean; active?: boolean }) {
  return (
    <button
      onClick={onClick}
      title={title}
      aria-label={title}
      disabled={disabled}
      className={`flex h-7 shrink-0 items-center gap-1 rounded-md px-1.5 text-xs hover:bg-muted disabled:opacity-35 disabled:hover:bg-transparent ${active ? "bg-accent text-accent-foreground" : ""}`}
    >
      {children}
    </button>
  );
}

function PageInput({ value, onGo }: { value: string; onGo: (v: string) => boolean }) {
  const [draft, setDraft] = useState(value);
  return (
    <input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onFocus={(e) => e.target.select()}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !onGo(draft)) setDraft(value);
        if (e.key === "Escape") setDraft(value);
      }}
      onBlur={() => setDraft(value)}
      className="selectable w-12 rounded border bg-background px-1 py-0.5 text-center text-xs tabular-nums"
      aria-label="页码"
      title="输入页码后回车"
    />
  );
}

/** 选中文字后的浮动工具条：点颜色 = 高亮；下划线用上次的颜色；复制带页码 */
function SelectionPopup({
  at,
  readonly,
  lastColor,
  onHighlight,
  onUnderline,
  onCopy,
}: {
  at: { x: number; y: number };
  readonly: string | null;
  lastColor: string;
  onHighlight: (color: string) => void;
  onUnderline: () => void;
  onCopy: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: at.x, top: at.y + 6 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    setPos({ left: Math.max(8, Math.min(at.x - w / 2, window.innerWidth - w - 8)), top: at.y + h + 12 > window.innerHeight ? at.y - h - 28 : at.y + 6 });
  }, [at]);
  return (
    <div
      ref={ref}
      // 不抢选区：按下时不让浏览器把焦点和选区移走
      onMouseDown={(e) => e.preventDefault()}
      style={{ left: pos.left, top: pos.top }}
      className="fixed z-30 flex items-center gap-1 rounded-lg border bg-card px-2 py-1.5 shadow-lg"
    >
      {!readonly && (
        <>
          {COLORS.map((c) => (
            <button
              key={c.value}
              onClick={() => onHighlight(c.value)}
              title={`${c.label}色高亮`}
              aria-label={`${c.label}色高亮`}
              className={`size-4.5 rounded-full border border-black/10 hover:scale-110 ${c.value === lastColor ? "ring-2 ring-ring ring-offset-1 ring-offset-card" : ""}`}
              style={{ background: c.value }}
            />
          ))}
          <span className="mx-1 h-4 w-px bg-border" />
          <button onClick={onUnderline} title="下划线（上次的颜色）" aria-label="下划线" className="rounded p-0.5 hover:bg-muted">
            <Underline size={15} />
          </button>
        </>
      )}
      <button onClick={onCopy} className="rounded px-1.5 py-0.5 text-xs hover:bg-muted">
        复制
      </button>
      {readonly && <span className="ml-1 text-[11px] text-muted-foreground">只读</span>}
    </div>
  );
}
