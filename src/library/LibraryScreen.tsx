// 文献库（1.0.3）：页头；第一栏左边是范围（全部、未分类、文件缺失……回收站），右边是类型和搜索；
// 第二栏只放集合，左右滑；标签树两指往右滑从左边长出来。点一行右边滑出面板（详情、编辑、附件、阅读）。
// 导入在 设置 › 导入。
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { Select, useWheelX } from "@/shared/controls";
import { useNav, type LibRequest } from "@/app/nav";
import { ItemList, type Selection } from "@/library/ItemList";
import { DetailPane } from "@/library/ItemDetail";
import { Collections } from "@/library/Collections";
import { TagTree } from "@/library/TagTree";
import { ITEM_TYPE_OPTIONS, itemTypeLabel } from "@/library/labels";
import type { Scope } from "@/library/scope";
import { LookupStatus } from "@/metadata/LookupStatus";
import { useStoredState } from "@/shared/ui";
import { Mark } from "@/shared/marks";
import { X } from "lucide-react";

const SPECIAL: { kind: Scope["kind"]; name: string; n: "missingFiles" | "duplicates" | "review" | "trash" }[] = [
  { kind: "missing", name: "文件缺失", n: "missingFiles" },
  { kind: "duplicates", name: "可能重复", n: "duplicates" },
  { kind: "review", name: "待确认的补全", n: "review" },
  { kind: "trash", name: "回收站", n: "trash" },
];

export function LibraryScreen({ request, onDone }: { request: LibRequest | null; onDone: () => void }) {
  const nav = useNav();
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [tags, setTags] = useState<string[]>([]);
  const [selection, setSelection] = useState<Selection[]>([]);
  const [input, setInput] = useState("");
  const [q, setQ] = useState("");
  const [itemType, setItemType] = useState("");
  // 「含子集合」记住，不用每进一个集合勾一次
  const [includeChildren, setIncludeChildren] = useStoredState("includeChildren", false);
  const o = useQuery({ queryKey: ["overview"], queryFn: api.overview }).data;
  const scopesRef = useRef<HTMLDivElement>(null);
  useWheelX(scopesRef);
  // 页面的内容有多宽（标签树、面板开着会挤）：m 藏类型列，s 再藏作者列
  const pageRef = useRef<HTMLElement>(null);
  const [w, setW] = useState<"l" | "m" | "s">("l");
  useEffect(() => {
    const el = pageRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      const x = e.contentRect.width;
      setW(x <= 620 ? "s" : x <= 820 ? "m" : "l");
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // 标签树收起时页面要退回来（tags-pull），但第一次进文献库不演
  const opened = useRef(false);
  if (nav.tags) opened.current = true;

  useEffect(() => {
    const t = setTimeout(() => setQ(input.trim()), 250);
    return () => clearTimeout(t);
  }, [input]);

  // 换范围：清空选择、搜索词、类型（标签树的筛选留着，它是跨范围的）
  const choose = (s: Scope) => {
    setScope(s);
    setSelection([]);
    setInput("");
    setQ("");
    setItemType("");
  };

  // 从别处跳来：看某一条（⌘K、阅读器的「在文献库里」）、待确认的补全（联网补全的「去确认」）
  useEffect(() => {
    if (!request) return;
    if (request.kind === "review") choose({ kind: "review" });
    else {
      setScope({ kind: "all" });
      setTags([]);
      setSelection([{ kind: "item", id: request.id }]);
    }
    onDone();
  }, [request, onDone]); // eslint-disable-line react-hooks/exhaustive-deps

  const panel = selection.length > 0;
  const bars = (
    <>
      <div className="lib-bar">
        <div className="tabs lib-scopes" ref={scopesRef}>
          <button className={scope.kind === "all" ? "is-on" : ""} onClick={() => choose({ kind: "all" })}>
            全部<span className="num">{o?.items ?? ""}</span>
          </button>
          <button className={scope.kind === "unfiled" ? "is-on" : ""} onClick={() => choose({ kind: "unfiled" })}>
            未分类<span className="num">{o?.unfiled ?? ""}</span>
          </button>
          {SPECIAL.filter((s) => (o?.[s.n] ?? 0) > 0 || scope.kind === s.kind).map((s) => (
            <button key={s.kind} className={scope.kind === s.kind ? "is-on" : ""} onClick={() => choose({ kind: s.kind } as Scope)}>
              {s.name}
              <span className="num">{o?.[s.n] || ""}</span>
            </button>
          ))}
        </div>
        <div className="lib-find-row">
          <Select value={itemType} onChange={setItemType} title="按类型筛选" menuClassName="pith-menu--tall">
            <option value="">全部类型</option>
            {ITEM_TYPE_OPTIONS.map((t) => (
              <option key={t} value={t}>
                {itemTypeLabel(t)}
              </option>
            ))}
            <option value="attachment">独立附件</option>
            <option value="note">独立笔记</option>
          </Select>
          <label className="pith-search">
            <Mark path="icons/search" className="pith-search-g" />
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setInput("")}
              placeholder="搜索标题、作者、标签、摘要、笔记"
              className="selectable"
            />
            {input && (
              <button onClick={() => setInput("")} aria-label="清空" className="pith-search-x">
                ×
              </button>
            )}
          </label>
        </div>
      </div>
      <Collections
        scope={scope}
        onScope={choose}
        extra={
          scope.kind === "collection" && (
            <label className="lib-sub" title="同时显示下级集合里的">
              <input type="checkbox" className="pith-check" checked={includeChildren} onChange={(e) => setIncludeChildren(e.target.checked)} />
              含子集合
            </label>
          )
        }
      />
      {tags.length > 0 && (
        <p className="lib-tagline">
          <span>标签</span>
          {tags.map((t) => (
            <button key={t} className="pith-tag is-on" onClick={() => setTags(tags.filter((x) => x !== t))} title="去掉这个标签">
              {t}
              <i aria-hidden="true">×</i>
            </button>
          ))}
          <button className="pith-link" onClick={() => setTags([])}>
            清除
          </button>
        </p>
      )}
    </>
  );

  return (
    <div className={`lib ${panel ? "has-panel" : ""} ${nav.tags ? "is-tags" : opened.current ? "was-tags" : ""}`}>
      <TagTree open={nav.tags} tags={tags} onTags={setTags} />
      <main ref={pageRef} className="page lib-page page-enter" data-w={w}>
        <ItemList
          scope={scope}
          tags={tags}
          q={q}
          itemType={itemType}
          includeChildren={includeChildren}
          onShowSubcollections={() => setIncludeChildren(true)}
          selection={selection}
          onSelection={setSelection}
          onImport={() => nav.openSettings("import")}
          bars={bars}
          status={<LookupStatus />}
        />
      </main>
      <aside className={`panel ${panel ? "is-open" : ""}`} aria-hidden={!panel}>
        <button className="x" onClick={() => setSelection([])} aria-label="关上">
          <X size={18} />
        </button>
        {panel && <DetailPane selection={selection} onSelection={setSelection} scope={scope} />}
      </aside>
    </div>
  );
}
