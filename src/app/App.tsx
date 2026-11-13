import { Component, lazy, Suspense, useCallback, useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { api } from "@/shared/ipc";
import { UiProvider } from "@/shared/ui";
import { Sidebar, type Scope, type View } from "@/library/Sidebar";
import { Rail, type Module } from "@/app/Rail";
import { ItemList, type Selection } from "@/library/ItemList";
import { DetailPane } from "@/library/ItemDetail";
import { SettingsNav, SettingsPage, type SettingsSection } from "@/settings/SettingsPage";
import { ImportPage } from "@/import/ImportPage";
import { FormatPage, type FormatAt } from "@/format/FormatPage";
import { FormatSidebar } from "@/format/FormatSidebar";
import { ResearchPage, type ResearchAt } from "@/research/ResearchPage";
import { ResearchSidebar } from "@/research/ResearchSidebar";
import { ConceptPage, type ConceptAt } from "@/concept/ConceptPage";
import { ConceptSidebar } from "@/concept/ConceptSidebar";
import { ReadContext } from "@/reader/useRead";
import { MetadataProvider } from "@/metadata/MetadataProvider";

// 阅读器连同 pdf.js 单独打包，第一次阅读时才加载
const Reader = lazy(() => import("@/reader/Reader"));

/** 外框：最左一列切换主页面（文献库、排版、调查、概念）和设置，每个主页面有自己的侧栏（decisions/0023）。
 *  文献库是三栏：导航 / 列表 / 详情；导入页占中右两栏。排版、调查、设置是侧栏 + 一页。
 *  阅读 PDF 时阅读器占满窗口；文献库只是藏起来（搜索词、滚动位置都还在），返回就回到原样。 */
export function App() {
  return (
    <UiProvider>
      <Shell />
    </UiProvider>
  );
}

function Shell() {
  const [module, setModule] = useState<Module>("library");
  // 文献库里看的是列表还是导入页
  const [view, setView] = useState<View>("library");
  const [settingsAt, setSettingsAt] = useState<SettingsSection>("general");
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [tags, setTags] = useState<string[]>([]);
  const [selection, setSelection] = useState<Selection[]>([]);
  const [reading, setReading] = useState<number | null>(null);
  // 排版页看到哪一层：记在外框，去设置填完密钥再回来还在原处
  const [formatAt, setFormatAt] = useState<FormatAt>({ rulesetId: null, runId: null });
  // 调查页看到哪一次：同上
  const [researchAt, setResearchAt] = useState<ResearchAt>({ uid: null });
  // 概念页看到哪：同上
  const [conceptAt, setConceptAt] = useState<ConceptAt>({ text: null, unit: null, run: null, card: null });
  // 文献库附件上「解析成文本」存好后、或点「概念页」：跳到那本书；删掉一本书：回到书架
  useEffect(() => {
    const open = (e: Event) => {
      const uid = (e as CustomEvent<string>).detail;
      setConceptAt({ text: uid, unit: null, run: null, card: null });
      setReading(null);
      setModule("concept");
    };
    const home = () => setConceptAt({ text: null, unit: null, run: null, card: null });
    window.addEventListener("pith:open-concept", open);
    window.addEventListener("pith:concept-home", home);
    return () => {
      window.removeEventListener("pith:open-concept", open);
      window.removeEventListener("pith:concept-home", home);
    };
  }, []);
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo });

  // 换范围：清空选择与标签筛选；列表用 key 重建，搜索词、类型筛选、排序也清空
  const choose = (s: Scope) => {
    setScope(s);
    setSelection([]);
    setTags([]);
    setView("library");
    setModule("library");
  };
  // 排版、调查里「去填密钥」
  const toKeys = () => {
    setSettingsAt("keys");
    setModule("settings");
  };
  // 点最左一列：换主页面；点的就是现在这一页，回到它的起点
  const pickModule = (m: Module) => {
    if (m === module) {
      if (m === "format") setFormatAt({ rulesetId: null, runId: null });
      if (m === "research") setResearchAt({ uid: null });
      if (m === "concept") setConceptAt((a) => ({ text: a.text, unit: null, run: null, card: null }));
      if (m === "library") setView("library");
    }
    setModule(m);
  };
  const scopeKey = scope.kind === "collection" ? `c${scope.id}` : scope.kind;
  // 联网补全一轮结束后「去确认」：打开「待确认的补全」（只用到 setState，引用稳定）
  const toReview = useCallback(() => {
    setReading(null);
    setScope({ kind: "review" });
    setSelection([]);
    setTags([]);
    setView("library");
    setModule("library");
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
                    setModule("library");
                    setSelection([{ kind: "item", id }]);
                  }}
                />
              </Suspense>
            </ReaderBoundary>
          )}
          <div className={`flex min-h-0 flex-1 ${reading !== null ? "hidden" : ""}`}>
            <Rail module={module} onModule={pickModule} />
            {module === "library" && (
              <>
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
                {view === "import" ? (
                  <ImportPage onDone={() => choose({ kind: "all" })} />
                ) : (
                  <>
                    <ItemList key={scopeKey} scope={scope} tags={tags} selection={selection} onSelection={setSelection} onImport={() => setView("import")} />
                    <DetailPane selection={selection} onSelection={setSelection} scope={scope} />
                  </>
                )}
              </>
            )}
            {module === "format" && (
              <>
                <FormatSidebar at={formatAt} onAt={setFormatAt} />
                <FormatPage at={formatAt} onAt={setFormatAt} onSettings={toKeys} />
              </>
            )}
            {module === "research" && (
              <>
                <ResearchSidebar at={researchAt} onAt={setResearchAt} />
                <ResearchPage at={researchAt} onAt={setResearchAt} onSettings={toKeys} />
              </>
            )}
            {module === "concept" && (
              <>
                <ConceptSidebar at={conceptAt} onAt={setConceptAt} />
                <ConceptPage at={conceptAt} onAt={setConceptAt} onSettings={toKeys} />
              </>
            )}
            {module === "settings" && (
              <>
                <SettingsNav section={settingsAt} onSection={setSettingsAt} />
                <SettingsPage section={settingsAt} onSection={setSettingsAt} onRestored={afterRestore} />
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
