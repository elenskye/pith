import { Component, lazy, Suspense, useCallback, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { api } from "@/shared/ipc";
import { UiProvider } from "@/shared/ui";
import { Sidebar, type Scope, type View } from "@/library/Sidebar";
import { ItemList, type Selection } from "@/library/ItemList";
import { DetailPane } from "@/library/ItemDetail";
import { SettingsPage } from "@/settings/SettingsPage";
import { ImportPage } from "@/import/ImportPage";
import { FormatPage, type FormatAt } from "@/format/FormatPage";
import { ReadContext } from "@/reader/useRead";
import { MetadataProvider } from "@/metadata/MetadataProvider";

// 阅读器连同 pdf.js 单独打包，第一次阅读时才加载
const Reader = lazy(() => import("@/reader/Reader"));

/** 三栏外框：左侧文献库导航，中间列表，右侧详情；设置页、导入页、排版页占中右两栏。
 *  阅读 PDF 时阅读器占满窗口；文献库只是藏起来（搜索词、滚动位置都还在），返回就回到原样。 */
export function App() {
  return (
    <UiProvider>
      <Shell />
    </UiProvider>
  );
}

function Shell() {
  const [view, setView] = useState<View>("library");
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [tags, setTags] = useState<string[]>([]);
  const [selection, setSelection] = useState<Selection[]>([]);
  const [reading, setReading] = useState<number | null>(null);
  // 排版页看到哪一层：记在外框，去设置填完密钥再回来还在原处
  const [formatAt, setFormatAt] = useState<FormatAt>({ rulesetId: null, runId: null });
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo });

  // 换范围：清空选择与标签筛选；列表用 key 重建，搜索词、类型筛选、排序也清空
  const choose = (s: Scope) => {
    setScope(s);
    setSelection([]);
    setTags([]);
    setView("library");
  };
  const scopeKey = scope.kind === "collection" ? `c${scope.id}` : scope.kind;
  // 联网补全一轮结束后「去确认」：打开「待确认的补全」（只用到 setState，引用稳定）
  const toReview = useCallback(() => {
    setReading(null);
    setScope({ kind: "review" });
    setSelection([]);
    setTags([]);
    setView("library");
  }, []);

  // 库被整个换掉（恢复）：选择、筛选、范围回到初始，留在设置页看结果
  const afterRestore = useCallback(() => {
    setReading(null);
    setScope({ kind: "all" });
    setSelection([]);
    setTags([]);
  }, []);

  return (
    <MetadataProvider onReview={toReview}>
      <ReadContext.Provider value={setReading}>
        <div className="flex h-full flex-col">
          {info.data?.startupError && (
            <div className="flex items-start gap-2 border-b bg-destructive/10 px-4 py-2 text-destructive">
              <TriangleAlert size={16} className="mt-0.5 shrink-0" />
              <span className="selectable">数据库没有打开：{info.data.startupError}</span>
            </div>
          )}
          {reading !== null && (
            <ReaderBoundary key={reading} onClose={() => setReading(null)}>
              <Suspense fallback={<p className="m-auto text-muted-foreground">正在加载阅读器…</p>}>
                <Reader
                  attachmentId={reading}
                  onClose={() => setReading(null)}
                  onShowItem={(id) => {
                    setReading(null);
                    setView("library");
                    setSelection([{ kind: "item", id }]);
                  }}
                />
              </Suspense>
            </ReaderBoundary>
          )}
          <div className={`flex min-h-0 flex-1 ${reading !== null ? "hidden" : ""}`}>
            <Sidebar
              view={view}
              scope={scope}
              onScope={choose}
              onView={setView}
              tags={tags}
              onTags={(t) => {
                setTags(t);
                setView("library");
              }}
            />
            {view === "settings" ? (
              <SettingsPage onRestored={afterRestore} />
            ) : view === "import" ? (
              <ImportPage onDone={() => choose({ kind: "all" })} />
            ) : view === "format" ? (
              <FormatPage at={formatAt} onAt={setFormatAt} onSettings={() => setView("settings")} />
            ) : (
              <>
                <ItemList key={scopeKey} scope={scope} tags={tags} selection={selection} onSelection={setSelection} onImport={() => setView("import")} />
                <DetailPane selection={selection} onSelection={setSelection} scope={scope} />
              </>
            )}
          </div>
        </div>
      </ReadContext.Provider>
    </MetadataProvider>
  );
}

/** 阅读器出了意外错误时只关掉阅读器，文献库照常（不让整个窗口变空白） */
class ReaderBoundary extends Component<{ onClose: () => void; children: React.ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: String((e as Error)?.message ?? e) };
  }
  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="m-auto max-w-md rounded-lg border bg-card p-4 text-center">
        <p className="font-medium">阅读器出错了</p>
        <p className="selectable mt-1 text-xs text-muted-foreground">{this.state.error}</p>
        <button onClick={this.props.onClose} className="mt-3 rounded-md border px-3 py-1 text-xs hover:bg-muted">
          返回文献库
        </button>
      </div>
    );
  }
}
