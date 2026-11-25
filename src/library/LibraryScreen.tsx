// 文献库（R1）：页头、一排范围、表格；点一行右边滑出面板（详情、编辑元信息、附件、阅读）。
// 原来左边那一栏（集合树、标签筛选、文件缺失、可能重复、回收站……）收进「集合与标签」的下拉里，功能都还在。
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, X } from "lucide-react";
import { api } from "@/shared/ipc";
import { Sidebar, type Scope, type View } from "@/library/Sidebar";
import { ItemList, type Selection } from "@/library/ItemList";
import { DetailPane } from "@/library/ItemDetail";
import { ImportPage } from "@/import/ImportPage";
import type { LibRequest } from "@/app/nav";

const SPECIAL: Partial<Record<Scope["kind"], string>> = { missing: "文件缺失", duplicates: "可能重复", review: "待确认的补全", trash: "回收站" };

export function LibraryScreen({ request, onDone }: { request: LibRequest | null; onDone: () => void }) {
  const [view, setView] = useState<View>("library");
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [tags, setTags] = useState<string[]>([]);
  const [selection, setSelection] = useState<Selection[]>([]);
  const [navOpen, setNavOpen] = useState(false);
  const navRef = useRef<HTMLDivElement>(null);
  const o = useQuery({ queryKey: ["overview"], queryFn: api.overview }).data;

  // 换范围：清空选择与标签筛选；列表用 key 重建，搜索词、类型筛选、排序也清空
  const choose = (s: Scope) => {
    setScope(s);
    setSelection([]);
    setTags([]);
    setView("library");
    setNavOpen(false);
  };

  // 从别处跳来：看某一条（⌘K、阅读器的「在文献库里」）、待确认的补全（联网补全的「去确认」）、导入
  useEffect(() => {
    if (!request) return;
    if (request.kind === "import") setView("import");
    else if (request.kind === "review") choose({ kind: "review" });
    else {
      setScope({ kind: "all" });
      setTags([]);
      setView("library");
      setSelection([{ kind: "item", id: request.id }]);
    }
    onDone();
  }, [request, onDone]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!navOpen) return;
    const off = (e: MouseEvent) => !navRef.current?.contains(e.target as Node) && setNavOpen(false);
    window.addEventListener("mousedown", off);
    return () => window.removeEventListener("mousedown", off);
  }, [navOpen]);

  const scopeKey = scope.kind === "collection" ? `c${scope.id}` : scope.kind;
  const tabs = (
    <div className="tabs" style={{ overflow: "visible" }}>
      <button className={scope.kind === "all" && tags.length === 0 ? "is-on" : ""} onClick={() => choose({ kind: "all" })}>
        全部<span className="num">{o?.items ?? ""}</span>
      </button>
      <button className={scope.kind === "unfiled" ? "is-on" : ""} onClick={() => choose({ kind: "unfiled" })}>
        未分类<span className="num">{o?.unfiled ?? ""}</span>
      </button>
      {scope.kind === "collection" && <button className="is-on">{scope.name}</button>}
      {SPECIAL[scope.kind] && <button className="is-on">{SPECIAL[scope.kind]}</button>}
      {tags.length > 0 && (
        <button className="is-on" onClick={() => setTags([])} title="去掉标签筛选">
          # {tags.join("、")} <X size={12} className="inline" />
        </button>
      )}
      <div className="relative ml-auto" ref={navRef}>
        <button className={`act ${navOpen ? "" : ""}`} onClick={() => setNavOpen((v) => !v)}>
          集合与标签 <ChevronDown size={12} className="inline" />
        </button>
        {navOpen && (
          <div className="pop lib-nav right-0 mt-2">
            <Sidebar
              view={view}
              scope={scope}
              onScope={choose}
              onView={(v) => {
                setView(v);
                setNavOpen(false);
              }}
              tags={tags}
              onTags={(t) => {
                setTags(t);
                setView("library");
              }}
            />
          </div>
        )}
      </div>
      <button className="act" onClick={() => setView("import")}>
        导入
      </button>
    </div>
  );

  return (
    <div className={selection.length > 0 && view === "library" ? "has-panel" : ""}>
      <main className="page page-enter">
        {view === "import" ? (
          <>
            <p className="page-count">
              <button className="pith-link" onClick={() => setView("library")}>
                ← 文献库
              </button>
            </p>
            <h1 className="page-h1">导入</h1>
            <div className="page-body">
              <ImportPage onDone={() => choose({ kind: "all" })} />
            </div>
          </>
        ) : (
          <ItemList key={scopeKey} scope={scope} tags={tags} selection={selection} onSelection={setSelection} onImport={() => setView("import")} tabs={tabs} />
        )}
      </main>
      <aside className={`panel ${selection.length > 0 && view === "library" ? "is-open" : ""}`} aria-hidden={selection.length === 0}>
        <button className="x" onClick={() => setSelection([])} aria-label="关上">
          <X size={18} />
        </button>
        <div className="detail">{selection.length > 0 && <DetailPane selection={selection} onSelection={setSelection} scope={scope} />}</div>
      </aside>
    </div>
  );
}
