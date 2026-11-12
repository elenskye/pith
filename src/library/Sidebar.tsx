import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy, FileWarning, Folder, FolderPlus, Globe, Import, Inbox, Library, PencilRuler, Settings, Tag, Telescope, Trash2, X } from "lucide-react";
import { LookupStatus } from "@/metadata/LookupStatus";
import { api, type CollectionNode, type TagCount } from "@/shared/ipc";
import { Chevron, childrenMap, useMutate, useStoredState, useUi } from "@/shared/ui";

export type View = "library" | "settings" | "import" | "format" | "research";
export type Scope =
  | { kind: "all" }
  | { kind: "unfiled" }
  | { kind: "missing" }
  | { kind: "duplicates" }
  | { kind: "review" }
  | { kind: "trash" }
  | { kind: "collection"; id: number; name: string };

type Props = {
  view: View;
  scope: Scope;
  onScope: (s: Scope) => void;
  onView: (v: View) => void;
  tags: string[];
  onTags: (t: string[]) => void;
};

export function Sidebar({ view, scope, onScope, onView, tags, onTags }: Props) {
  const ui = useUi();
  const mutate = useMutate();
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const collections = useQuery({ queryKey: ["collections"], queryFn: api.collectionsList });
  const o = overview.data;
  const active = (s: Scope) => view === "library" && s.kind === scope.kind && (s.kind !== "collection" || (scope.kind === "collection" && s.id === scope.id));

  const newCollection = async (parent: CollectionNode | null) => {
    const name = await ui.prompt({ title: parent ? `在「${parent.name}」下新建集合` : "新建集合", label: "集合名", confirm: "新建" });
    if (!name) return;
    const id = await mutate(() => api.collectionsCreate(name, parent?.id ?? null));
    if (id !== undefined) onScope({ kind: "collection", id, name });
  };

  const collectionMenu = (e: React.MouseEvent, n: CollectionNode) =>
    ui.menu(e, [
      { label: "新建子集合…", onClick: () => newCollection(n) },
      {
        label: "重命名…",
        onClick: async () => {
          const name = await ui.prompt({ title: "重命名集合", initial: n.name, confirm: "改名" });
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
          if (ok !== undefined && active({ kind: "collection", id: n.id, name: n.name })) onScope({ kind: "all" });
        },
      },
    ]);

  return (
    <nav className="flex w-60 shrink-0 flex-col border-r bg-sidebar">
      <div className="px-4 pt-4 pb-2 text-[11px] font-medium tracking-wide text-muted-foreground">文献库</div>
      <div className="px-2">
        <Row icon={<Library size={15} />} label="全部文献" count={o?.items} active={active({ kind: "all" })} onClick={() => onScope({ kind: "all" })} />
        <Row icon={<Inbox size={15} />} label="未分类" count={o?.unfiled} active={active({ kind: "unfiled" })} onClick={() => onScope({ kind: "unfiled" })} />
        {(o?.missingFiles ?? 0) > 0 && (
          <Row icon={<FileWarning size={15} />} label="文件缺失" count={o?.missingFiles} active={active({ kind: "missing" })} onClick={() => onScope({ kind: "missing" })} />
        )}
        {(o?.duplicates ?? 0) > 0 && (
          <Row icon={<Copy size={15} />} label="可能重复" count={o?.duplicates} active={active({ kind: "duplicates" })} onClick={() => onScope({ kind: "duplicates" })} />
        )}
        {((o?.review ?? 0) > 0 || scope.kind === "review") && (
          <Row icon={<Globe size={15} />} label="待确认的补全" count={o?.review} active={active({ kind: "review" })} onClick={() => onScope({ kind: "review" })} />
        )}
      </div>

      <div className="flex items-center justify-between px-4 pt-4 pb-1">
        <span className="text-[11px] font-medium tracking-wide text-muted-foreground">集合</span>
        <button onClick={() => newCollection(null)} title="新建集合" className="rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground">
          <FolderPlus size={14} />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2">
        {collections.data && collections.data.length > 0 ? (
          <Tree
            nodes={collections.data}
            isActive={(id) => active({ kind: "collection", id, name: "" })}
            onPick={(n) => onScope({ kind: "collection", id: n.id, name: n.name })}
            onMenu={collectionMenu}
          />
        ) : (
          <p className="px-2 py-1 text-xs text-muted-foreground">还没有集合</p>
        )}
      </div>

      <TagPanel tags={tags} onTags={onTags} />
      <LookupStatus />

      <div className="border-t px-2 py-2">
        <Row icon={<Import size={15} />} label="导入" active={view === "import"} onClick={() => onView("import")} />
        <Row icon={<Trash2 size={15} />} label="回收站" count={o?.trash} active={active({ kind: "trash" })} onClick={() => onScope({ kind: "trash" })} />
        <Row icon={<PencilRuler size={15} />} label="排版" active={view === "format"} onClick={() => onView("format")} />
        <Row icon={<Telescope size={15} />} label="调查" active={view === "research"} onClick={() => onView("research")} />
        <Row icon={<Settings size={15} />} label="设置" active={view === "settings"} onClick={() => onView("settings")} />
      </div>
    </nav>
  );
}

function Row({
  icon,
  label,
  count,
  active,
  onClick,
  onMenu,
  indent = 0,
  chevron,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  active: boolean;
  onClick: () => void;
  onMenu?: (e: React.MouseEvent) => void;
  indent?: number;
  chevron?: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      onContextMenu={onMenu}
      style={{ paddingLeft: 8 + indent * 14 }}
      className={`flex w-full items-center gap-2 rounded-md py-1.5 pr-2 text-left ${active ? "bg-accent text-accent-foreground" : "hover:bg-muted"}`}
    >
      {chevron}
      <span className="shrink-0 text-muted-foreground">{icon}</span>
      <span className="min-w-0 flex-1 truncate" title={label}>
        {label}
      </span>
      {count !== undefined && count > 0 && <span className="text-xs text-muted-foreground tabular-nums">{count}</span>}
    </button>
  );
}

function Tree({
  nodes,
  isActive,
  onPick,
  onMenu,
}: {
  nodes: CollectionNode[];
  isActive: (id: number) => boolean;
  onPick: (n: CollectionNode) => void;
  onMenu: (e: React.MouseEvent, n: CollectionNode) => void;
}) {
  const children = useMemo(() => childrenMap(nodes), [nodes]);
  const [open, setOpen] = useState<Set<number>>(new Set());
  const toggle = (id: number) => setOpen((s) => (s.has(id) ? new Set([...s].filter((x) => x !== id)) : new Set([...s, id])));

  const render = (parent: number | null, depth: number): React.ReactNode =>
    (children.get(parent) ?? []).map((n) => {
      const kids = children.get(n.id);
      const isOpen = open.has(n.id);
      return (
        <div key={n.id}>
          <Row
            icon={<Folder size={15} />}
            label={n.name}
            count={n.itemCount}
            active={isActive(n.id)}
            indent={depth}
            onClick={() => onPick(n)}
            onMenu={(e) => onMenu(e, n)}
            chevron={
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  if (kids) toggle(n.id);
                }}
                className={`flex w-3 shrink-0 justify-center text-muted-foreground ${kids ? "" : "invisible"}`}
              >
                <Chevron open={isOpen} />
              </span>
            }
          />
          {kids && isOpen && render(n.id, depth + 1)}
        </div>
      );
    });

  return <>{render(null, 0)}</>;
}

/** 标签：点一下筛选（可多选，「同时带有」），右键改名 / 删除 */
function TagPanel({ tags, onTags }: { tags: string[]; onTags: (t: string[]) => void }) {
  const ui = useUi();
  const mutate = useMutate();
  const list = useQuery({ queryKey: ["tags"], queryFn: api.tagsList });
  const [filter, setFilter] = useState("");
  // 可以收起，把地方让给集合树；记住开合
  const [open, setOpen] = useStoredState("tagPanelOpen", true);
  const shown = (list.data ?? []).filter((t) => !filter || t.name.toLowerCase().includes(filter.toLowerCase()));
  const toggle = (name: string) => onTags(tags.includes(name) ? tags.filter((t) => t !== name) : [...tags, name]);

  const tagMenu = (e: React.MouseEvent, t: TagCount) =>
    ui.menu(e, [
      {
        label: "重命名…",
        onClick: async () => {
          const name = await ui.prompt({ title: "重命名标签", label: "改成已有的名字会合并两个标签", initial: t.name, confirm: "改名" });
          if (name && name !== t.name) {
            const r = await mutate(() => api.tagsRename(t.id, name));
            if (r !== undefined && tags.includes(t.name)) onTags(tags.map((x) => (x === t.name ? name : x)));
          }
        },
      },
      {
        label: "删除…",
        danger: true,
        onClick: async () => {
          const ok = await ui.confirm({ title: `删除标签「${t.name}」`, message: `会从 ${t.count} 条记录上去掉这个标签，文献本身不受影响。`, confirm: "删除", danger: true });
          if (ok) {
            await mutate(() => api.tagsDelete(t.id));
            onTags(tags.filter((x) => x !== t.name));
          }
        },
      },
    ]);

  if (!list.data || list.data.length === 0) return null;
  return (
    <div className="border-t px-3 pt-2 pb-2">
      <div className={`flex items-center gap-1.5 ${open ? "mb-1.5" : ""}`}>
        <button onClick={() => setOpen(!open)} title={open ? "收起标签" : "展开标签"} className="flex items-center gap-1.5 text-muted-foreground hover:text-foreground">
          <Chevron open={open} />
          <Tag size={12} />
          <span className="text-[11px] font-medium tracking-wide">标签</span>
          {!open && <span className="text-[11px] tabular-nums">{list.data.length}</span>}
        </button>
        {tags.length > 0 && (
          <button onClick={() => onTags([])} className="ml-auto flex items-center gap-0.5 text-[11px] text-muted-foreground hover:text-foreground">
            <X size={11} />
            清除筛选
          </button>
        )}
      </div>
      {open && list.data.length > 12 && (
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="找标签"
          className="selectable mb-1.5 w-full rounded border bg-background px-1.5 py-0.5 text-xs outline-none focus:border-ring"
        />
      )}
      {open && (
        <div className="flex max-h-36 flex-wrap gap-1 overflow-y-auto">
          {shown.map((t) => (
            <button
              key={t.id}
              onClick={() => toggle(t.name)}
              onContextMenu={(e) => tagMenu(e, t)}
              title={`${t.name}（${t.count} 条）`}
              className={`max-w-full truncate rounded-full px-2 py-0.5 text-xs ${tags.includes(t.name) ? "bg-primary text-primary-foreground" : "bg-muted hover:bg-accent"}`}
            >
              {t.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
