// 文献库的第二栏：集合，一栏左右滑的（pith-strip）。有子集合的点开，子集合接在它后面、从一根小枝上长出来；
// 选中的那条一路展开。右键：新建子集合、改名、移动、移到回收站；最后一个是「＋ 集合」。
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type CollectionNode } from "@/shared/ipc";
import { childrenMap, useMutate, useUi } from "@/shared/ui";
import type { Scope } from "./scope";

export function Collections({ scope, onScope, extra }: { scope: Scope; onScope: (s: Scope) => void; extra?: React.ReactNode }) {
  const ui = useUi();
  const mutate = useMutate();
  const list = useQuery({ queryKey: ["collections"], queryFn: api.collectionsList });
  const nodes = useMemo(() => list.data ?? [], [list.data]);
  const kids = useMemo(() => childrenMap(nodes), [nodes]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const active = scope.kind === "collection" ? scope.id : null;
  const [open, setOpen] = useState<Set<number>>(new Set());
  const strip = useRef<HTMLDivElement>(null);

  // 选中的那条：它和它的上级都展开
  useEffect(() => {
    if (active === null) return;
    const up = new Set<number>();
    for (let n = byId.get(active); n; n = n.parentId !== null ? byId.get(n.parentId) : undefined) up.add(n.id);
    setOpen((o) => new Set([...o, ...up]));
  }, [active, byId]);

  // 鼠标滚轮竖着滚也能左右走（触控板两指横滑本来就行）
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);
  useEffect(() => {
    strip.current?.querySelector(".is-on")?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  }, [active, open]);

  const create = async (parent: CollectionNode | null) => {
    const name = await ui.prompt({ title: parent ? `在「${parent.name}」下新建集合` : "新建集合", label: "集合名", confirm: "新建" });
    if (!name) return;
    const id = await mutate(() => api.collectionsCreate(name, parent?.id ?? null));
    if (id !== undefined) onScope({ kind: "collection", id, name });
  };

  const menu = (e: React.MouseEvent, n: CollectionNode) =>
    ui.menu(e, [
      { label: "新建子集合…", onClick: () => create(n) },
      {
        label: "改名…",
        onClick: async () => {
          const name = await ui.prompt({ title: "给集合改名", initial: n.name, confirm: "改名" });
          if (name) await mutate(() => api.collectionsRename(n.id, name));
        },
      },
      {
        label: "移动到…",
        onClick: async () => {
          const to = await ui.pickCollection({ title: `把「${n.name}」移到`, allowRoot: true, exclude: n.id, confirm: "移动" });
          if (to !== null) await mutate(() => api.collectionsMove(n.id, to === 0 ? null : to));
        },
      },
      "-",
      {
        label: "移到回收站",
        danger: true,
        onClick: async () => {
          const ok = await mutate(() => api.trashMove([{ kind: "collection", id: n.id }]), `「${n.name}」已移到回收站（里面的文献还在）`);
          if (ok !== undefined && active === n.id) onScope({ kind: "all" });
        },
      },
    ]);

  const render = (parent: number | null, depth: number): React.ReactNode[] =>
    (kids.get(parent) ?? []).flatMap((n) => {
      const sub = kids.get(n.id);
      const isOpen = !!sub && open.has(n.id);
      const out: React.ReactNode[] = [
        <button
          key={n.id}
          className={`pith-strip-item ${depth > 0 ? "is-sub" : ""} ${active === n.id ? "is-on" : ""} ${sub ? "has-kids" : ""} ${isOpen ? "is-open" : ""}`}
          onClick={() => {
            if (active === n.id && sub) setOpen((o) => (o.has(n.id) ? new Set([...o].filter((x) => x !== n.id)) : new Set([...o, n.id])));
            else onScope({ kind: "collection", id: n.id, name: n.name });
          }}
          onContextMenu={(e) => menu(e, n)}
          title={sub ? `${n.name}（再点一下${isOpen ? "收起" : "展开"}子集合）` : n.name}
        >
          {n.name}
          {n.itemCount > 0 && <em className="num">{n.itemCount}</em>}
        </button>,
      ];
      if (isOpen)
        out.push(
          <span key={`t${n.id}`} className="pith-strip-twig" aria-hidden="true" />,
          ...render(n.id, depth + 1),
          <span key={`e${n.id}`} className="pith-strip-end" aria-hidden="true" />,
        );
      return out;
    });

  return (
    <div className="lib-cols">
      <div ref={strip} className="pith-strip" role="toolbar" aria-label="集合">
        {nodes.length === 0 && list.data && <span className="pith-strip-empty">还没有集合</span>}
        {render(null, 0)}
        <button className="pith-strip-item is-add" onClick={() => create(null)} title="新建集合">
          ＋ 集合
        </button>
      </div>
      {extra}
    </div>
  );
}
