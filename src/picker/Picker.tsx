// 置顶搜索窗（decisions/0015、0017）：Word 里点 Add/Edit Citation 时 Pith 打开它；选文献、填页码，回车插入。
// 窗口由 Rust 创建、显示、藏起；每次打开 Rust 发 picker-open 事件带上这一次（新插入 / 编辑已有引文的预填）。
// 键盘：输入即搜；↑↓ 选、回车加入；搜索框空着时回车 = 插入；⌘回车 = 插入；Esc = 取消。
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BookOpen, CornerDownLeft, LoaderCircle, Search, Trash2, X } from "lucide-react";
import { api, type CiteRef, type PickerSession, type PickInfo } from "@/shared/ipc";

type Chosen = { cite: CiteRef; info: PickInfo };

function Meta({ info }: { info: PickInfo }) {
  const bits = [info.creators, info.year ? String(info.year) : "", info.container ?? ""].filter(Boolean);
  return <span className="truncate text-xs text-muted-foreground">{bits.join(" · ") || "（无作者、年份）"}</span>;
}

export function PickerApp() {
  const [session, setSession] = useState<PickerSession | null>(null);
  const [chosen, setChosen] = useState<Chosen[]>([]);
  const [q, setQ] = useState("");
  const [results, setResults] = useState<PickInfo[]>([]);
  const [active, setActive] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const seq = useRef(0);

  const start = useCallback((s: PickerSession | null) => {
    setSession(s);
    setChosen(s?.items ?? []);
    setQ("");
    setActive(0);
    setError(null);
    setTimeout(() => input.current?.focus(), 30);
  }, []);

  // 窗口第一次加载：取当前这一次；之后每次打开 Rust 发事件
  useEffect(() => {
    api.pickerSession().then(start, (e) => setError(String(e)));
    const un = listen<PickerSession>("picker-open", (e) => start(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [start]);

  // 搜索（输入停一下再查；只认最后一次的结果）
  useEffect(() => {
    const n = ++seq.current;
    setSearching(true);
    const t = setTimeout(
      () => {
        api.pickerSearch(q).then(
          (r) => {
            if (n !== seq.current) return;
            setResults(r);
            setActive(0);
            setSearching(false);
          },
          (e) => {
            if (n !== seq.current) return;
            setError(String(e));
            setSearching(false);
          },
        );
      },
      q ? 120 : 0,
    );
    return () => clearTimeout(t);
  }, [q, session?.id]);

  useEffect(() => {
    list.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const finish = async (done: Parameters<typeof api.pickerDone>[1]) => {
    if (!session) return;
    const id = session.id;
    setSession(null);
    await api.pickerDone(id, done);
  };
  const cancel = () => finish({ kind: "cancel" });
  const insert = () => {
    if (!chosen.length) return;
    finish({ kind: "items", items: chosen.map((c) => (c.cite.locator?.trim() ? { ...c.cite, locator: c.cite.locator.trim() } : withoutLocator(c.cite))) });
  };
  const add = (info: PickInfo | undefined) => {
    if (!info) return;
    setChosen((cs) => (cs.some((c) => c.cite.id === info.uid) ? cs : [...cs, { cite: { id: info.uid }, info }]));
    setQ("");
    input.current?.focus();
  };
  const setLocator = (i: number, locator: string) => setChosen((cs) => cs.map((c, k) => (k === i ? { ...c, cite: { ...c.cite, locator } } : c)));
  const remove = (i: number) => setChosen((cs) => cs.filter((_, k) => k !== i));

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      insert();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (q.trim() || !chosen.length) add(results[active]);
      else insert();
    }
  };

  if (!session) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
        {error ?? <LoaderCircle size={16} className="animate-spin" />}
      </div>
    );
  }

  const chosenIds = new Set(chosen.map((c) => c.cite.id));

  return (
    <div className="flex h-full flex-col bg-background text-sm text-foreground" onKeyDown={(e) => e.key === "Escape" && (e.preventDefault(), cancel())}>
      <div className="flex items-center gap-2 border-b px-3 py-2">
        <Search size={15} className="shrink-0 text-muted-foreground" />
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={onKey}
          placeholder={session.editing ? "搜索要加进这处引文的文献…" : "搜索标题、作者、年份…"}
          className="selectable min-w-0 flex-1 bg-transparent py-1 text-sm outline-none placeholder:text-muted-foreground"
        />
        {searching && <LoaderCircle size={14} className="shrink-0 animate-spin text-muted-foreground" />}
      </div>

      <div ref={list} className="min-h-0 flex-1 overflow-y-auto py-1">
        {results.length === 0 && !searching && <p className="px-4 py-6 text-center text-xs text-muted-foreground">{q ? "没有找到" : "文献库是空的"}</p>}
        {!q && results.length > 0 && <p className="px-3 pt-1 pb-0.5 text-[11px] text-muted-foreground">最近加入的</p>}
        {results.map((r, i) => (
          <button
            key={r.uid}
            data-i={i}
            onMouseMove={() => setActive(i)}
            onClick={() => add(r)}
            className={`flex w-full flex-col items-start px-3 py-1.5 text-left ${i === active ? "bg-accent text-accent-foreground" : ""} ${chosenIds.has(r.uid) ? "opacity-50" : ""}`}
          >
            <span className="w-full truncate">{r.title}</span>
            <Meta info={r} />
          </button>
        ))}
      </div>

      <div className="border-t bg-sidebar px-3 py-2">
        {chosen.length === 0 ? (
          <p className="py-1 text-xs text-muted-foreground">还没选文献：在上面搜索，回车或点一下加进来。</p>
        ) : (
          <ul className="mb-2 max-h-36 space-y-1 overflow-y-auto">
            {chosen.map((c, i) => (
              <li key={c.cite.id} className="flex items-center gap-2 rounded-md border bg-background px-2 py-1">
                <BookOpen size={13} className="shrink-0 text-muted-foreground" />
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-xs">{c.info.title}</span>
                  <span className="truncate text-[11px] text-muted-foreground">
                    {c.info.inTrash ? "在回收站里 · " : !c.info.inLibrary ? "不在文献库里，用文档自带的数据 · " : ""}
                    {[c.info.creators, c.info.year].filter(Boolean).join(" · ")}
                  </span>
                </div>
                <label className="flex shrink-0 items-center gap-1 text-[11px] text-muted-foreground">
                  页码
                  <input
                    value={c.cite.locator ?? ""}
                    onChange={(e) => setLocator(i, e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        insert();
                      }
                    }}
                    placeholder="如 17"
                    className="selectable w-20 rounded border bg-background px-1.5 py-0.5 text-xs text-foreground outline-none focus:border-ring"
                  />
                </label>
                <button onClick={() => remove(i)} title="从这处引文去掉" className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-muted">
                  <X size={13} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex items-center gap-2">
          {session.editing && (
            <button onClick={() => finish({ kind: "remove" })} className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-destructive hover:bg-muted">
              <Trash2 size={13} />
              删除这处引文
            </button>
          )}
          <span className="flex-1 truncate text-[11px] text-muted-foreground">{error}</span>
          <button onClick={cancel} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
            取消
          </button>
          <button
            onClick={insert}
            disabled={!chosen.length}
            className="flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50"
          >
            {session.editing ? "更新引文" : "插入引文"}
            <CornerDownLeft size={12} />
          </button>
        </div>
      </div>
    </div>
  );
}

function withoutLocator(c: CiteRef): CiteRef {
  const { locator: _l, label: _b, ...rest } = c;
  return rest as CiteRef;
}
