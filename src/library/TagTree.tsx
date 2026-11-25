// 文献库的标签树：没有按钮，两指往右滑从左边长出来、往左滑收起（Esc 也收起）。和设置的导航是同一种枝桠：
// 一根主枝，每个标签一根小枝；名字里有「/」的按前面那段分组（「政治/效能感」挂在「政治」下）。
// 点一个筛选（可以多选，要同时带着这几个），选中的节点是珊瑚色；右键改名、删掉。
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type TagCount } from "@/shared/ipc";
import { useMutate, useUi } from "@/shared/ui";
import { branch, labelDelay, type BranchNode } from "@/app/branch";

type Row = BranchNode & { t: string; tag?: TagCount };

const TOP = 112;
const GAP = 40;

export function TagTree({ open, tags, onTags }: { open: boolean; tags: string[]; onTags: (t: string[]) => void }) {
  const ui = useUi();
  const mutate = useMutate();
  const list = useQuery({ queryKey: ["tags"], queryFn: api.tagsList });
  const [find, setFind] = useState("");
  const all = list.data ?? [];

  const rows = useMemo(() => {
    const q = find.trim().toLowerCase();
    const shown = all.filter((t) => !q || t.name.toLowerCase().includes(q));
    const flat = shown.filter((t) => !t.name.includes("/")).sort((a, b) => a.name.localeCompare(b.name, "zh"));
    const groups = new Map<string, TagCount[]>();
    for (const t of shown.filter((x) => x.name.includes("/"))) {
      const g = t.name.slice(0, t.name.indexOf("/"));
      groups.set(g, [...(groups.get(g) ?? []), t]);
    }
    const out: Row[] = [];
    let y = TOP;
    for (const t of flat) {
      out.push({ kind: "i", y, t: t.name, tag: t, on: tags.includes(t.name) });
      y += GAP;
    }
    for (const g of [...groups.keys()].sort((a, b) => a.localeCompare(b, "zh"))) {
      y += out.length ? 18 : 0;
      out.push({ kind: "g", y, t: g });
      y += 34;
      for (const t of groups.get(g)!.sort((a, b) => a.name.localeCompare(b.name, "zh"))) {
        out.push({ kind: "i", y, t: t.name.slice(g.length + 1), tag: t, on: tags.includes(t.name) });
        y += GAP;
      }
    }
    return out;
  }, [all, find, tags]);

  const height = Math.max(TOP + 40, (rows[rows.length - 1]?.y ?? TOP) + 70);
  const { paths, dots } = branch(10, height - 30, rows, { lead: 0.05, step: Math.min(0.05, 1.2 / Math.max(1, rows.length)) });
  const toggle = (name: string) => onTags(tags.includes(name) ? tags.filter((t) => t !== name) : [...tags, name]);

  const menu = (e: React.MouseEvent, t: TagCount) =>
    ui.menu(e, [
      {
        label: "改名…",
        onClick: async () => {
          const name = await ui.prompt({ title: "给标签改名", label: "改成已有的名字会合并两个标签；名字里的「/」用来分组", initial: t.name, confirm: "改名" });
          if (name && name !== t.name) {
            const r = await mutate(() => api.tagsRename(t.id, name));
            if (r !== undefined && tags.includes(t.name)) onTags(tags.map((x) => (x === t.name ? name : x)));
          }
        },
      },
      "-",
      {
        label: "删掉…",
        danger: true,
        onClick: async () => {
          const ok = await ui.confirm({ title: `删掉标签「${t.name}」？`, message: `会从 ${t.count} 条记录上去掉这个标签，文献本身不受影响。`, confirm: "删掉", danger: true });
          if (ok) {
            await mutate(() => api.tagsDelete(t.id));
            onTags(tags.filter((x) => x !== t.name));
          }
        },
      },
    ]);

  return (
    <nav className="tagtree no-swipe-x" aria-label="标签" aria-hidden={!open}>
      <div className="tagtree-in" style={{ height }}>
        <svg className="br-svg" width={280} height={height} viewBox={`0 0 280 ${height}`} aria-hidden="true">
          {paths.map((p, i) => (
            <path key={i} className={p.cls} pathLength={1} d={p.d} style={{ "--d": `${p.delay}s`, ...(p.dur ? { "--dur": `${p.dur}s` } : {}) } as React.CSSProperties} />
          ))}
          {dots.map((d, i) => (
            <circle key={i} className={d.on ? "on" : ""} cx={d.cx} cy={d.cy} r={2.8} style={{ "--d": `${d.delay}s` } as React.CSSProperties} />
          ))}
        </svg>
        <p className="br-lbl tagtree-h" style={{ "--d": ".1s" } as React.CSSProperties}>
          标签<span className="num">{all.length || ""}</span>
        </p>
        <div className="br-lbl tagtree-tools" style={{ "--d": ".16s" } as React.CSSProperties}>
          {all.length > 10 && <input className="pith-field tagtree-find" value={find} onChange={(e) => setFind(e.target.value)} placeholder="找标签" tabIndex={open ? 0 : -1} />}
          {tags.length > 0 && (
            <button className="pith-link" onClick={() => onTags([])} tabIndex={open ? 0 : -1}>
              清除 {tags.length} 个
            </button>
          )}
        </div>
        {list.data && all.length === 0 && (
          <p className="br-lbl br-empty" style={{ "--d": ".3s", top: TOP - 8 } as React.CSSProperties}>
            还没有标签。在右边的面板里给文献加一个，它就长在这里。
          </p>
        )}
        {rows.map((r, k) => {
          const style = { "--d": `${labelDelay(k, 0.05, Math.min(0.05, 1.2 / Math.max(1, rows.length)))}s` } as React.CSSProperties;
          if (r.kind === "g")
            return (
              <p key={`g${r.t}`} className="br-lbl br-g tagtree-g" style={{ ...style, top: r.y - 7 }}>
                {r.t}
              </p>
            );
          return (
            <button
              key={r.tag!.id}
              className={`br-lbl tagtree-it pith-row ${r.on ? "is-on" : ""}`}
              style={{ ...style, top: r.y - 11 }}
              onClick={() => toggle(r.tag!.name)}
              onContextMenu={(e) => menu(e, r.tag!)}
              title={`${r.tag!.name}（${r.tag!.count} 条）`}
              tabIndex={open ? 0 : -1}
            >
              <b>{r.t}</b>
              <em className="num">{r.tag!.count}</em>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
