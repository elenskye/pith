// 外框（1.0.2，照 Explore R1）：顶栏（Logo + 知识库、文献库、设置三个图标）下面是一屏。
// 打开 Pith 就是对话；对话历史没有按钮，⌘\ 或两指往右滑叫出来（枝桠）；文献库里两指往右滑叫出的是标签树；⌘K 去任何地方。
// 阅读 PDF 时阅读器占满窗口，各屏只是藏起来，返回就回到原样。
import { Component, lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { api } from "@/shared/ipc";
import { INK_READY } from "@/shared/theme";
import { UiProvider, useUi } from "@/shared/ui";
import { ReadContext } from "@/reader/useRead";
import { MetadataProvider } from "@/metadata/MetadataProvider";
import { NavContext, type ChatTarget, type LibRequest, type Nav, type Screen, type SetSection } from "@/app/nav";
import { TopBar } from "@/app/TopBar";
import { History } from "@/app/History";
import { Palette } from "@/app/Palette";
import { ChatScreen } from "@/chat/ChatScreen";
import { KnowledgePage } from "@/kb/KnowledgePage";
import { LibraryScreen } from "@/library/LibraryScreen";
import { SettingsScreen } from "@/settings/SettingsScreen";

// 阅读器连同 pdf.js 单独打包，第一次阅读时才加载
const Reader = lazy(() => import("@/reader/Reader"));

export function App() {
  return (
    <UiProvider>
      <Shell />
    </UiProvider>
  );
}

function Shell() {
  const ui = useUi();
  const qc = useQueryClient();
  const [screen, setScreen] = useState<Screen>("chat");
  const [chat, setChat] = useState<ChatTarget>(null);
  const [setSection, setSetSection] = useState<SetSection>("roles");
  const [kb, setKb] = useState<{ uid: string | null; material: string | null; passage: number | null }>({ uid: null, material: null, passage: null });
  const [libRequest, setLibRequest] = useState<LibRequest | null>(null);
  const [side, setSide] = useState(false);
  const [tags, setTags] = useState(false);
  const [palette, setPalette] = useState(false);
  const [reading, setReading] = useState<number | null>(null);
  const [lakeAt, setLakeAt] = useState<{ x: number; y: number; n: number; run: boolean } | null>(null);
  const [seedPath, setSeedPath] = useState<{ d: string; n: number } | null>(null);
  const [fed, setFed] = useState(false);
  const kbIcon = useRef<HTMLButtonElement>(null);
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo });

  const go = useCallback((s: Screen) => {
    setScreen(s);
    setSide(false);
    setTags(false);
  }, []);

  const seed = useCallback((from: Element | null) => {
    const to = kbIcon.current?.getBoundingClientRect();
    const a = from?.getBoundingClientRect();
    void qc.invalidateQueries({ queryKey: ["kbs"] });
    if (!to || !a || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    // 起点、落点每次量（窗口大小、顶栏布局会变）；控制点照方案：中点往右 120、比高的那头再高 40
    const [x0, y0, x1, y1] = [a.left + a.width / 2, a.top + a.height / 2, to.left + to.width / 2, to.top + to.height / 2];
    const d = `M ${x0} ${y0} Q ${(x0 + x1) / 2 + 120} ${Math.min(y0, y1) - 40} ${x1} ${y1}`;
    setSeedPath((p) => ({ d, n: (p?.n ?? 0) + 1 }));
    setTimeout(() => setFed(true), 820);
    setTimeout(() => {
      setFed(false);
      setSeedPath(null);
    }, 2700);
  }, [qc]);

  // 换配色：五圈波纹（2.3 秒）。整页的水面扭曲（SVG 滤镜）在 WebKit 里每帧要 100 毫秒，1.0.3 拿掉了；
  // 颜色从点的地方洇开在 shared/theme 的 saveLook（View Transitions）。波纹先摆好，等洇开真的开始（INK_READY）才漾开
  const lakeN = useRef(0);
  const lake = useCallback((x: number, y: number) => {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const n = ++lakeN.current;
    setLakeAt({ x, y, n, run: false });
    const go = () => {
      setLakeAt((l) => (l && l.n === n ? { ...l, run: true } : l));
      setTimeout(() => setLakeAt((l) => (l && l.n === n ? null : l)), 2400);
    };
    window.addEventListener(INK_READY, go, { once: true });
  }, []);

  const nav: Nav = useMemo(
    () => ({
      screen,
      go,
      chat,
      openChat: (t) => {
        setChat(t);
        setScreen("chat");
        setSide(false);
      },
      setSection,
      openSettings: (s) => {
        setSetSection(s);
        go("set");
      },
      kb,
      openKb: (uid, material = null, passage = null) => {
        setKb((k) => ({ uid: uid ?? k.uid, material, passage }));
        go("kb");
      },
      openLibrary: (r) => {
        setLibRequest(r);
        go("lib");
      },
      openItem: (id) => {
        setLibRequest({ kind: "item", id });
        go("lib");
      },
      side,
      setSide,
      tags,
      openPalette: () => setPalette(true),
      seed,
      lake,
      say: (text) => ui.notify(text),
    }),
    [screen, go, chat, setSection, kb, side, tags, seed, lake, ui],
  );

  // ⌘K 去任何地方；⌘\ 对话历史；⌘N 新对话；Esc 收起
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (reading !== null) return;
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (mod && e.key === "\\") {
        e.preventDefault();
        setScreen("chat");
        setSide((s) => !s);
      } else if (mod && e.key.toLowerCase() === "n" && !e.shiftKey) {
        e.preventDefault();
        nav.openChat(null);
      } else if (e.key === "Escape" && side) {
        setSide(false);
      } else if (e.key === "Escape" && tags && !(e.target as HTMLElement | null)?.closest("input, textarea, .pith-menu")) {
        setTags(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [reading, side, tags, nav]);

  // 两指横滑：横向比纵向大、累计过 60 才算，一次锁 0.6 秒；手指往右（自然滚动下 deltaX 为负）叫出来，往左收起。
  // 文献库里叫出的是标签树，别处是对话历史。输入框里、能横向滚动的地方（集合那一栏）不算。
  const screenRef = useRef(screen);
  screenRef.current = screen;
  useEffect(() => {
    let acc = 0;
    let locked = 0;
    const onWheel = (e: WheelEvent) => {
      if (reading !== null || Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest("input, textarea, [contenteditable], .tabs, .no-swipe")) return;
      for (let el = t; el && el !== document.body; el = el.parentElement) {
        if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX !== "hidden" && getComputedStyle(el).overflowX !== "visible") return;
      }
      const now = Date.now();
      if (now < locked) return;
      acc += e.deltaX;
      if (Math.abs(acc) < 60) return;
      const open = acc < 0;
      acc = 0;
      locked = now + 600;
      if (screenRef.current === "lib") setTags(open);
      else if (open) {
        setScreen("chat");
        setSide(true);
      } else setSide(false);
    };
    window.addEventListener("wheel", onWheel, { passive: true });
    return () => window.removeEventListener("wheel", onWheel);
  }, [reading]);

  // 文献库附件上「解析成文本」存好后、或点「概念页」：打开那本书
  useEffect(() => {
    const open = async (e: Event) => {
      const uid = (e as CustomEvent<string>).detail;
      const t = (await api.conceptTextsList()).find((x) => x.uid === uid);
      setReading(null);
      setChat({ kind: "book", text: uid, title: t?.title ? `读《${t.title}》` : "读这本书" });
      go("chat");
    };
    window.addEventListener("pith:open-concept", open);
    return () => window.removeEventListener("pith:open-concept", open);
  }, [go]);

  // 库被整个换掉（恢复）：各屏回到起点，留在设置页看结果
  const afterRestore = useCallback(() => {
    setReading(null);
    setChat(null);
    setKb({ uid: null, material: null, passage: null });
    setLibRequest(null);
    void qc.invalidateQueries();
  }, [qc]);

  // 联网补全一轮结束后「去确认」：文献库里的「待确认的补全」
  const toReview = useCallback(() => {
    setReading(null);
    setLibRequest({ kind: "review" });
    go("lib");
  }, [go]);

  return (
    <NavContext.Provider value={nav}>
      <MetadataProvider onReview={toReview}>
        <ReadContext.Provider value={setReading}>
          {reading !== null && (
            <div className="flex h-full flex-col">
              <ReaderBoundary key={reading} onClose={() => setReading(null)}>
                <Suspense fallback={<p className="m-auto p-10 text-muted-foreground">正在加载阅读器…</p>}>
                  <Reader
                    attachmentId={reading}
                    onClose={() => setReading(null)}
                    onShowItem={(id) => {
                      setReading(null);
                      nav.openItem(id);
                    }}
                  />
                </Suspense>
              </ReaderBoundary>
            </div>
          )}
          <div className={`app ${side && screen === "chat" ? "is-side" : ""}`} style={reading !== null ? { display: "none" } : undefined}>
            <TopBar kbRef={kbIcon} fed={fed} />
            {info.data?.startupError && (
              <div className="startup-error">
                <TriangleAlert size={16} className="mt-0.5 shrink-0" />
                <span className="selectable">数据库没有打开：{info.data.startupError}</span>
              </div>
            )}
            <main className="screen">
              {screen === "chat" && (
                <>
                  <ChatScreen />
                  <History />
                </>
              )}
              {screen === "kb" && <KnowledgePage />}
              {screen === "lib" && <LibraryScreen request={libRequest} onDone={() => setLibRequest(null)} />}
              {screen === "set" && <SettingsScreen onRestored={afterRestore} />}
            </main>
          </div>
          {lakeAt && (
            <div key={lakeAt.n} className="lake-layer">
              <svg className={`pith-lake ${lakeAt.run ? "is-in" : ""}`} style={{ left: lakeAt.x, top: lakeAt.y }} viewBox="-20 -20 40 40" aria-hidden="true">
                {[1, 2, 3, 4, 5].map((k) => (
                  <g key={k}>
                    <circle className={`rg rg${k}`} r="10" />
                    <circle className={`rh rh${k}`} r="10.6" />
                  </g>
                ))}
              </svg>
            </div>
          )}
          {seedPath && (
            <div className="seed-layer">
              <i key={seedPath.n} className="pith-seed is-in" style={{ offsetPath: `path('${seedPath.d}')` }} />
            </div>
          )}
          <Palette open={palette} onClose={() => setPalette(false)} />
        </ReadContext.Provider>
      </MetadataProvider>
    </NavContext.Provider>
  );
}

/** 阅读器出了意外错误时只关掉阅读器，别的照常（不让整个窗口变空白） */
class ReaderBoundary extends Component<{ onClose: () => void; children: React.ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: String((e as Error)?.message ?? e) };
  }
  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="empty">
        <p>阅读器出错了</p>
        <p className="selectable mt-1 text-xs">{this.state.error}</p>
        <button onClick={this.props.onClose} className="pith-button pith-button--quiet">
          返回
        </button>
      </div>
    );
  }
}
