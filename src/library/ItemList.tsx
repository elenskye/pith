import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowDown, ArrowUp, FileText, Folder, Paperclip, StickyNote } from "lucide-react";
import { api, type ListQuery, type ListRow, type RowKind, type Target } from "@/shared/ipc";
import { childrenMap, useMutate, useUi } from "@/shared/ui";
import { useRead } from "@/reader/useRead";
import { useMetadata } from "@/metadata/MetadataProvider";
import { scopeKey, type Scope } from "./scope";
import { DELETE_FOREVER_NOTE, NEW_ITEM_TYPES, contentTypeLabel, creatorsSummary, formatTime, itemTypeLabel } from "./labels";

export type Selection = { kind: RowKind; id: number };

type Props = {
  scope: Scope;
  tags: string[];
  q: string;
  itemType: string;
  includeChildren: boolean;
  onShowSubcollections: () => void;
  selection: Selection[];
  onSelection: (s: Selection[]) => void;
  onImport: () => void;
  /** 页头下面的两栏：范围、类型、搜索；集合（LibraryScreen 给） */
  bars?: React.ReactNode;
  /** 页头右边的进度（联网补全） */
  status?: React.ReactNode;
};

const same = (a: Selection, b: Selection) => a.kind === b.kind && a.id === b.id;
export const targetsOf = (s: Selection[]): Target[] => s.map(({ kind, id }) => ({ kind, id }));

function toQueryScope(s: Scope, includeChildren: boolean): ListQuery["scope"] {
  return s.kind === "collection" ? { kind: "collection", id: s.id, includeChildren } : { kind: s.kind };
}

/** 中间的列表。切换范围时排序、Shift 选的起点清空，表格回到顶上；不整个重建（页头下面的两栏也在这里，重建会让集合的枝桠再长一遍）。 */
export function ItemList({ scope, tags, q, itemType, includeChildren, onShowSubcollections, selection, onSelection, onImport, bars, status }: Props) {
  const ui = useUi();
  const mutate = useMutate();
  const { openItem, openAttachment } = useRead();
  const metadata = useMetadata();
  const [sort, setSort] = useState<{ key: string; desc: boolean } | null>(null);
  const anchor = useRef<number | null>(null);
  const tableRef = useRef<HTMLDivElement>(null);
  const sk = scopeKey(scope);
  const [shownScope, setShownScope] = useState(sk);
  if (sk !== shownScope) {
    setShownScope(sk);
    setSort(null);
  }
  useEffect(() => {
    anchor.current = null;
    tableRef.current?.scrollTo({ top: 0 });
  }, [sk]);

  const query: ListQuery = { scope: toQueryScope(scope, includeChildren), q, tags, itemType: itemType || null, sort: sort?.key ?? null, desc: sort?.desc ?? false };
  const items = useQuery({ queryKey: ["items", query], queryFn: () => api.itemsList(query), placeholderData: (prev) => prev });
  const rows = useMemo(() => items.data ?? [], [items.data]);
  const trash = scope.kind === "trash";
  const title = { all: "全部文献", unfiled: "未分类", missing: "文件缺失", duplicates: "可能重复", review: "待确认的补全", trash: "回收站" }[scope.kind as string] ?? (scope.kind === "collection" ? scope.name : "");
  const filtered = q !== "" || tags.length > 0 || itemType !== "";
  const collections = useQuery({ queryKey: ["collections"], queryFn: api.collectionsList, enabled: scope.kind === "collection" });
  // 集合本身没有文献时，看子集合里有没有（Zotero 的父集合常常只用来分组）
  const inSubcollections = useMemo(() => {
    if (scope.kind !== "collection" || !collections.data) return 0;
    const kids = childrenMap(collections.data);
    const stack = [...(kids.get(scope.id) ?? [])];
    let n = 0;
    while (stack.length) {
      const c = stack.pop()!;
      n += c.itemCount;
      stack.push(...(kids.get(c.id) ?? []));
    }
    return n;
  }, [scope, collections.data]);

  // 列表变了：去掉已经不在列表里的选择
  useEffect(() => {
    if (!items.data) return;
    const kept = selection.filter((s) => items.data.some((r) => same(r, s)));
    if (kept.length !== selection.length) onSelection(kept);
  }, [items.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const click = (i: number, e: React.MouseEvent) => {
    const row = rows[i];
    const sel = { kind: row.kind, id: row.id };
    if (e.metaKey || e.ctrlKey) {
      onSelection(selection.some((s) => same(s, sel)) ? selection.filter((s) => !same(s, sel)) : [...selection, sel]);
      anchor.current = i;
    } else if (e.shiftKey && anchor.current !== null) {
      const [a, b] = [Math.min(anchor.current, i), Math.max(anchor.current, i)];
      onSelection(rows.slice(a, b + 1).map((r) => ({ kind: r.kind, id: r.id })));
    } else {
      onSelection([sel]);
      anchor.current = i;
    }
  };

  const trashSelected = async (sel: Selection[]) => {
    const targets = targetsOf(sel);
    if (targets.length === 0) return;
    const r = await mutate(() => api.trashMove(targets), `${targets.length} 项已移到回收站`);
    if (r !== undefined) onSelection([]);
  };

  const deleteForever = async (sel: Selection[]) => {
    const ok = await ui.confirm({
      title: `永久删除 ${sel.length} 项`,
      message: DELETE_FOREVER_NOTE,
      confirm: "永久删除",
      danger: true,
    });
    if (!ok) return;
    const r = await mutate(() => api.trashDelete(targetsOf(sel)), (d) => `已永久删除 ${d.records} 项${d.filesRemoved ? `，清理 ${d.filesRemoved} 个文件` : ""}`);
    if (r) onSelection([]);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    const idx = selection.length > 0 ? rows.findIndex((r) => same(r, selection[selection.length - 1])) : -1;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.min(rows.length - 1, Math.max(0, idx + (e.key === "ArrowDown" ? 1 : -1)));
      if (rows[next]) {
        onSelection([{ kind: rows[next].kind, id: rows[next].id }]);
        anchor.current = next;
        tableRef.current?.querySelector(`[data-row="${next}"]`)?.scrollIntoView({ block: "nearest" });
      }
    } else if ((e.key === "Backspace" || e.key === "Delete") && selection.length > 0 && !trash) {
      e.preventDefault();
      trashSelected(selection);
    } else if (e.key === "a" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      onSelection(rows.map((r) => ({ kind: r.kind, id: r.id })));
    }
  };

  const rowMenu = (e: React.MouseEvent, i: number) => {
    const row = rows[i];
    let sel = selection;
    if (!selection.some((s) => same(s, row))) {
      sel = [{ kind: row.kind, id: row.id }];
      onSelection(sel);
      anchor.current = i;
    }
    const n = sel.length;
    const items = sel.filter((s) => s.kind === "item");
    if (trash) {
      ui.menu(e, [
        { label: `恢复${n > 1 ? ` ${n} 项` : ""}`, onClick: () => mutate(() => api.trashRestore(targetsOf(sel)), "已恢复").then((r) => r !== undefined && onSelection([])) },
        "-",
        { label: "永久删除…", danger: true, onClick: () => deleteForever(sel) },
      ]);
      return;
    }
    const taggable = sel.filter((s) => s.kind !== "collection");
    ui.menu(e, [
      ...(n === 1 && row.kind === "attachment"
        ? [
            { label: "阅读 / 打开", onClick: () => void openAttachment(row.id) },
            { label: "用系统程序打开", onClick: () => mutate(() => api.attachmentsOpen(row.id)) },
            "-" as const,
          ]
        : []),
      ...(n === 1 && row.kind === "item" && row.attachments > 0 ? [{ label: "阅读 PDF", onClick: () => void openItem(row.id) }, "-" as const] : []),
      {
        label: "添加到集合…",
        onClick: async () => {
          const cid = await ui.pickCollection({ title: `把 ${n} 项添加到集合`, confirm: "添加" });
          if (cid) await mutate(() => api.collectionsAdd(cid, targetsOf(sel)), (k) => (k > 0 ? `已添加 ${k} 项` : "都已经在这个集合里了（子附件、子笔记跟着所属文献，不单独加入）"));
        },
      },
      ...(scope.kind === "collection"
        ? [{ label: `从「${scope.name}」移除`, onClick: () => mutate(() => api.collectionsRemove(scope.id, targetsOf(sel)), (k) => `已移出 ${k} 项`) }]
        : []),
      {
        label: "添加标签…",
        disabled: taggable.length === 0,
        onClick: async () => {
          const name = await ui.prompt({ title: `给 ${taggable.length} 项添加标签`, label: "标签名", confirm: "添加" });
          if (name) await mutate(() => api.tagsAdd(targetsOf(taggable), name), `已添加标签「${name}」`);
        },
      },
      ...(scope.kind === "duplicates" && items.length >= 2
        ? ["-" as const, { label: "不是重复", onClick: () => mutate(() => api.duplicatesDismiss(items.map((s) => s.id)), "已标记，不再提示") }]
        : []),
      "-",
      { label: `移到回收站${n > 1 ? `（${n} 项）` : ""}`, danger: true, onClick: () => trashSelected(sel) },
    ]);
  };

  const newMenu = (e: React.MouseEvent) => {
    const cid = scope.kind === "collection" ? scope.id : null;
    ui.menu(e, [
      ...NEW_ITEM_TYPES.map((t) => ({
        label: `新建${itemTypeLabel(t)}`,
        onClick: async () => {
          const id = await mutate(() => api.itemsCreate(t, cid));
          if (id !== undefined) onSelection([{ kind: "item", id }]);
        },
      })),
      {
        label: "按 DOI / ISBN 添加…",
        onClick: async () => {
          const ids = await metadata.addByIdentifier(cid);
          if (ids?.length) onSelection(ids.map((id) => ({ kind: "item" as const, id })));
        },
      },
      "-",
      {
        label: "添加独立附件…",
        onClick: async () => {
          const path = await api.pickAnyFile("选择要添加的文件（会复制进文献库，原文件不动）");
          if (!path) return;
          const id = await mutate(() => api.attachmentsAdd(null, path), "已添加附件");
          if (id !== undefined) {
            if (cid) await mutate(() => api.collectionsAdd(cid, [{ kind: "attachment", id }]));
            onSelection([{ kind: "attachment", id }]);
          }
        },
      },
    ]);
  };

  const emptyTrash = async () => {
    const ok = await ui.confirm({
      title: "清空回收站",
      message: `永久删除回收站里的 ${rows.length} 项。${DELETE_FOREVER_NOTE}`,
      confirm: "清空",
      danger: true,
    });
    if (ok) {
      const r = await mutate(api.trashEmpty, (d) => `已清空：${d.records} 项${d.filesRemoved ? `，清理 ${d.filesRemoved} 个文件` : ""}`);
      if (r) onSelection([]);
    }
  };

  const header = (key: string, label: string, cls: string) => {
    const activeKey = sort?.key ?? (q ? "relevance" : "added");
    const on = activeKey === key;
    return (
      <th className={cls}>
        <button
          onClick={() => setSort(on && sort ? { key, desc: !sort.desc } : { key, desc: key === "added" || key === "year" })}
          className={`flex items-center gap-0.5 hover:text-foreground ${on ? "text-foreground" : ""}`}
        >
          {label}
          {on && sort && (sort.desc ? <ArrowDown size={11} /> : <ArrowUp size={11} />)}
        </button>
      </th>
    );
  };

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col">
      <p className="page-count num">{items.data ? `${rows.length} 条` : " "}</p>
      <div className="lib-head">
        <h1 className="page-h1 truncate">{scope.kind === "all" ? "文献库" : title}</h1>
        <div className="lib-acts">
          {status}
          {trash ? (
            <button onClick={emptyTrash} disabled={rows.length === 0} className="pith-link is-danger disabled:opacity-40">
              清空回收站
            </button>
          ) : (
            <button onClick={newMenu} className="pith-link">
              ＋ 新建
            </button>
          )}
        </div>
      </div>
      {bars}

      <div ref={tableRef} tabIndex={0} onKeyDown={onKey} className="lib-table min-h-0 flex-1 overflow-y-auto outline-none">
        {items.error ? (
          <p className="selectable p-6 text-destructive">{String(items.error)}</p>
        ) : items.data && rows.length === 0 ? (
          <Empty scope={scope} filtered={filtered} onImport={onImport} inSubcollections={includeChildren ? 0 : inSubcollections} onShowSubcollections={onShowSubcollections} />
        ) : (
          <table className="w-full table-fixed border-collapse">
            <thead>
              <tr className="border-b">
                {header("title", "题名", "pl-4")}
                {header("creator", "作者", "w-48 col-a")}
                {trash ? header("added", "删除于", "w-32") : header("year", "年份", "w-16")}
                {header("type", "类型", "w-28 col-t")}
              </tr>
            </thead>
            <tbody>
              {rows.map((it, i) => {
                const active = selection.some((s) => same(s, it));
                const groupStart = it.group !== undefined && rows[i - 1]?.group !== it.group;
                return (
                  <Fragment key={`${it.kind}-${it.id}`}>
                    {groupStart && (
                      <tr className="grp">
                        <td colSpan={4}>
                          第 {(it.group ?? 0) + 1} 组 · {it.groupReason}
                        </td>
                      </tr>
                    )}
                    <tr
                      data-row={i}
                      onClick={(e) => click(i, e)}
                      onDoubleClick={() => {
                        // 双击：文献打开它的 PDF，附件能读就读、否则交给系统程序（回收站里也能看）
                        if (it.kind === "item") void openItem(it.id);
                        else if (it.kind === "attachment") void openAttachment(it.id);
                      }}
                      onContextMenu={(e) => rowMenu(e, i)}
                      className={active ? "is-on" : ""}
                    >
                      <td className="ti">
                        <div className="flex min-w-0 items-center gap-3">
                          <i className="sp" style={{ background: spine(it) }} />
                          <KindIcon row={it} />
                          <b className="truncate">{it.title || <span className="text-muted-foreground">（无标题）</span>}</b>
                          <Badges row={it} />
                        </div>
                      </td>
                      <td className="c col-a truncate">{creatorsSummary(it.firstCreator, it.creatorCount)}</td>
                      <td className="c truncate tabular-nums">{trash ? formatTime(it.deletedAt) : (it.year ?? "")}</td>
                      <td className="c f col-t truncate">{typeLabel(it)}</td>
                    </tr>
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

/** 书脊的颜色：按类型从方案的几种颜色里取 */
function spine(r: ListRow): string {
  if (r.kind !== "item") return "var(--color-line-2)";
  const t = r.itemType ?? "";
  if (t === "book" || t === "bookSection") return "var(--color-accent-text)";
  if (t === "journalArticle" || t === "magazineArticle" || t === "newspaperArticle") return "var(--color-second)";
  if (t === "thesis" || t === "report") return "color-mix(in srgb, var(--color-gold) 70%, var(--color-text-3))";
  return "var(--color-text-3)";
}

function typeLabel(r: ListRow): string {
  switch (r.kind) {
    case "item":
      return itemTypeLabel(r.itemType ?? "");
    case "attachment":
      return contentTypeLabel(r.itemType);
    case "note":
      return "笔记";
    case "collection":
      return "集合";
  }
}

function KindIcon({ row }: { row: ListRow }) {
  const cls = "shrink-0 text-muted-foreground";
  if (row.kind === "attachment") return <FileText size={14} className={cls} />;
  if (row.kind === "note") return <StickyNote size={14} className={cls} />;
  if (row.kind === "collection") return <Folder size={14} className={cls} />;
  return null;
}

/** 子附件个数（回形针）与缺失文件提示 */
function Badges({ row }: { row: ListRow }) {
  return (
    <>
      {row.kind === "item" && row.attachments > 0 && (
        <span className="flex shrink-0 items-center text-muted-foreground" title={`${row.attachments} 个附件`}>
          <Paperclip size={12} />
          {row.attachments > 1 && <span className="text-[11px] tabular-nums">{row.attachments}</span>}
        </span>
      )}
      {row.missingFiles > 0 && (
        <span className="shrink-0 rounded bg-destructive/10 px-1 text-[11px] text-destructive" title="附件文件缺失">
          文件缺失
        </span>
      )}
    </>
  );
}

function Empty({
  scope,
  filtered,
  onImport,
  inSubcollections,
  onShowSubcollections,
}: {
  scope: Scope;
  filtered: boolean;
  onImport: () => void;
  inSubcollections: number;
  onShowSubcollections: () => void;
}) {
  if (filtered) return <Centered main="没有符合条件的" sub="换个搜索词，或者去掉标签、类型的筛选。" />;
  if (scope.kind === "trash") return <Centered main="回收站是空的" />;
  if (scope.kind === "collection" && inSubcollections > 0)
    return (
      <Centered main="这个集合本身没有文献，都在子集合里">
        <button onClick={onShowSubcollections} className="pith-button pith-button--quiet">
          显示子集合里的文献
        </button>
      </Centered>
    );
  if (scope.kind === "collection") return <Centered main="这个集合里还没有文献" sub="在列表里右键「添加到集合…」" />;
  if (scope.kind === "unfiled") return <Centered main="所有文献都在集合里了" />;
  if (scope.kind === "duplicates") return <Centered main="没有可能重复的文献" />;
  if (scope.kind === "missing") return <Centered main="没有缺失的附件文件" />;
  if (scope.kind === "review") return <Centered main="联网补全没有要你确认的了" />;
  return (
    <Centered main="还没有文献" sub="从 Zotero 库、BibTeX、RIS 文件搬进来。">
      <button onClick={onImport} className="pith-button pith-sweep">
        去 设置 › 导入 <i className="ar not-italic">→</i>
      </button>
    </Centered>
  );
}

function Centered({ main, sub, children }: { main: string; sub?: string; children?: React.ReactNode }) {
  return (
    <div className="empty">
      <p>{main}</p>
      {sub && <p className="empty-sub">{sub}</p>}
      {children}
    </div>
  );
}
