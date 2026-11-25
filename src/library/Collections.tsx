// 文献库的第二栏：集合，一栏左右滑的（pith-strip）。有子集合的点开，长出一小段枝桠：从它身上分出一根带叶的岔、
// 底下一根茎，每个子集合下面一颗节点，茎的尽头一个芽（pith-strip-sub）；选中的那条一路展开。
// 右键：新建子集合、改名、移动、移到回收站；最后一个是「＋ 集合」。
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type CollectionNode } from "@/shared/ipc";
import { childrenMap, useMutate, useUi } from "@/shared/ui";
import { useWheelX } from "@/shared/controls";
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

  useWheelX(strip);
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
    (kids.get(parent) ?? []).flatMap((n, i) => {
      const sub = kids.get(n.id);
      const isOpen = !!sub && open.has(n.id);
      const out: React.ReactNode[] = [
        <button
          key={n.id}
          style={depth > 0 ? ({ "--i": i } as React.CSSProperties) : undefined}
          className={`pith-strip-item pith-ink ${depth > 0 ? "is-sub" : ""} ${active === n.id ? "is-on" : ""} ${sub ? "has-kids" : ""} ${isOpen ? "is-open" : ""}`}
          onClick={() => {
            if (active === n.id && sub) setOpen((o) => (o.has(n.id) ? new Set([...o].filter((x) => x !== n.id)) : new Set([...o, n.id])));
            else onScope({ kind: "collection", id: n.id, name: n.name });
          }}
          onContextMenu={(e) => menu(e, n)}
          title={sub ? `${n.name}（再点一下${isOpen ? "收起" : "展开"}子集合）` : n.name}
        >
          {depth > 0 && <i className="pith-strip-node" aria-hidden="true" />}
          {n.name}
          {n.itemCount > 0 && <em className="num">{n.itemCount}</em>}
        </button>,
      ];
      if (isOpen)
        out.push(
          <span key={`s${n.id}`} className="pith-strip-sub" style={{ "--n": sub.length } as React.CSSProperties}>
            <svg className="pith-strip-fork" viewBox="0 0 24 36" aria-hidden="true">
              <path className="st" pathLength={1} d="M0 18C7.5 18 9 31.4 24 31.4" />
              <path className="lf" d="M9.4 25.2C8.2 20.6 10.6 17.6 15 16.6C15.8 20.8 13.8 24.2 9.4 25.2Z" />
            </svg>
            {render(n.id, depth + 1)}
            <i className="pith-strip-bud" aria-hidden="true" />
          </span>,
        );
      return out;
    });

  return (
    <div className="lib-cols">
      <div ref={strip} className="pith-strip" role="toolbar" aria-label="集合">
        {nodes.length === 0 && list.data && <span className="pith-strip-empty">还没有集合</span>}
        {render(null, 0)}
        <button className="pith-strip-item pith-ink is-add" onClick={() => create(null)} title="新建集合">
          ＋ 集合
        </button>
      </div>
      {extra}
    </div>
  );
}
