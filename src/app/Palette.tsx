// ⌘K：去任何一屏、任何一段对话、任何一个知识库、设置的任何一页；换配色、明暗；打两个字以上也搜文献库。Esc 关上。
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { PALETTES, useLook } from "@/shared/theme";
import { useNav, skillName, when, type SetSection } from "@/app/nav";

type Row = { g: string; k: string; n: string; r?: string; run: (e?: { x: number; y: number }) => void };

const SECTIONS: [SetSection, string][] = [
  ["roles", "角色"],
  ["skills", "技能"],
  ["kb", "知识库与检索"],
  ["keys", "模型与费用"],
  ["word", "Word 与引文"],
  ["data", "数据"],
  ["look", "外观"],
];

export function Palette({ onClose }: { onClose: () => void }) {
  const nav = useNav();
  const [look, setLook] = useLook();
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const chats = useQuery({ queryKey: ["chats"], queryFn: api.chatList, staleTime: 0 });
  const kbs = useQuery({ queryKey: ["kbs"], queryFn: api.kbList });
  const term = q.trim();
  const items = useQuery({
    queryKey: ["palette-items", term],
    queryFn: () => api.itemsList({ scope: { kind: "all" }, q: term, tags: [], itemType: null, sort: null, desc: false }),
    enabled: term.length >= 2,
  });

  const rows = useMemo(() => {
    const close = (f: () => void) => () => {
      onClose();
      f();
    };
    const all: Row[] = [
      { g: "对话", k: "对话", n: "新对话", r: "⌘N", run: close(() => nav.openChat(null)) },
      { g: "对话", k: "对话", n: "以前的对话", r: "⌘\\ 或两指往右滑", run: close(() => (nav.go("chat"), nav.setSide(true))) },
      ...(chats.data ?? []).slice(0, 8).map((c) => ({
        g: "对话",
        k: c.skill ? skillName(c.skill) : "对话",
        n: c.title,
        r: when(c.updatedAt),
        run: close(() => nav.openChat(c.uid ? { kind: "chat", uid: c.uid } : { kind: "ref", skill: c.skill!, ref: c.refUid!, title: c.title })),
      })),
      { g: "去", k: "知识库", n: "知识库", run: close(() => nav.go("kb")) },
      { g: "去", k: "文献库", n: "文献库", run: close(() => nav.go("lib")) },
      { g: "去", k: "文献库", n: "从 Zotero、BibTeX 导入", run: close(() => nav.openLibrary({ kind: "import" })) },
      ...SECTIONS.map(([s, n]) => ({ g: "去", k: "设置", n, run: close(() => nav.openSettings(s)) })),
      ...(kbs.data ?? []).map((k) => ({ g: "知识库", k: "知识库", n: k.name, r: `${k.count} 份`, run: close(() => nav.openKb(k.uid)) })),
      ...PALETTES.filter((p) => p.id !== look.palette).map((p) => ({
        g: "外观",
        k: "配色",
        n: `换成${p.name}`,
        r: p.line,
        run: (e?: { x: number; y: number }) => {
          onClose();
          nav.lake(e?.x ?? innerWidth / 2, e?.y ?? innerHeight / 3);
          setLook({ ...look, palette: p.id });
        },
      })),
      ...(["system", "light", "dark"] as const)
        .filter((t) => t !== look.theme)
        .map((t) => ({ g: "外观", k: "明暗", n: { system: "跟随系统", light: "亮", dark: "暗" }[t], run: close(() => setLook({ ...look, theme: t })) })),
      ...(items.data ?? [])
        .filter((r) => r.kind === "item")
        .slice(0, 8)
        .map((r) => ({ g: "文献", k: r.year ? String(r.year) : "文献", n: r.title ?? "（无标题）", r: r.firstCreator ?? "", run: close(() => nav.openItem(r.id)) })),
    ];
    if (!term) return all;
    const t = term.toLowerCase();
    return all.filter((r) => r.g === "文献" || `${r.k} ${r.n} ${r.r ?? ""}`.toLowerCase().includes(t));
  }, [chats.data, kbs.data, items.data, look, term, nav, onClose, setLook]);

  useEffect(() => setActive(0), [term]);
  useEffect(() => {
    listRef.current?.querySelector(".is-active")?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(rows.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter" && rows[active]) {
      e.preventDefault();
      rows[active].run();
    }
  };

  let lastG = "";
  return (
    <>
      <div className="pal-scrim" onClick={onClose} />
      <div className="palette" role="dialog" aria-label="去任何地方">
        <div className="palette-in">
          <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={onKey} placeholder="去哪？对话、知识库、设置，或者搜文献……" />
          <kbd>esc</kbd>
        </div>
        <div className="palette-list" ref={listRef}>
          {rows.length === 0 && <p className="palette-g">没有找到</p>}
          {rows.map((r, i) => {
            const head = r.g !== lastG ? <p className="palette-g">{r.g}</p> : null;
            lastG = r.g;
            return (
              <div key={`${r.g}-${i}`}>
                {head}
                <button className={`palette-row ${i === active ? "is-active" : ""}`} onMouseEnter={() => setActive(i)} onClick={(e) => r.run({ x: e.clientX, y: e.clientY })}>
                  <span className="pk">{r.k}</span>
                  <span className="pn">{r.n}</span>
                  <span className="pr">{r.r}</span>
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
