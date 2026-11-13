// 概念页的侧栏：书 → 章 → 卡片（decisions/0024 第 10 条）。
// 书是解析过的原著（译本挂在原著下面，不单列）；选中一本书，下面是它的各章，每章下面是对这一章做过的研究和卡片；
// 不属于某一章的（问题、追踪、反方、推演）放在「其他」。右键研究可以删除。
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpenText } from "lucide-react";
import { api, type ConceptRunSummary } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { NavItem, NavLabel, SideNav } from "@/app/SideNav";
import { Badge } from "@/format/parts";
import { CARD_KIND, RUN_STATUS, RUN_TONE } from "@/concept/labels";
import type { ConceptAt } from "@/concept/ConceptPage";

/** 一次研究算在哪一章：整章入口的章；其他按第一段的段落号前缀（I.i.3 → I.i） */
export function unitOfRun(r: ConceptRunSummary): string | null {
  if (r.unit) return r.unit;
  if (r.skill === "passage" && r.firstParagraph) return r.firstParagraph.split(".").slice(0, -1).join(".") || null;
  return null;
}

export function ConceptSidebar({ at, onAt }: { at: ConceptAt; onAt: (at: ConceptAt) => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const texts = useQuery({ queryKey: ["conceptTexts"], queryFn: api.conceptTextsList, staleTime: 0 });
  const books = (texts.data ?? []).filter((t) => !t.originalUid);
  const view = useQuery({ queryKey: ["conceptText", at.text], queryFn: () => api.conceptTextGet(at.text!), enabled: !!at.text });
  const runs = useQuery({ queryKey: ["conceptRuns", at.text], queryFn: () => api.conceptRunsList(at.text!), enabled: !!at.text, staleTime: 0 });

  const remove = async (r: ConceptRunSummary) => {
    const ok = await ui.confirm({ title: "删除这次研究？", message: `「${r.skillTitle}：${r.label.slice(0, 40)}」和它的 ${r.cards.length} 张卡片。已经加入文献库的来源不受影响。`, confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.conceptRunDelete(r.uid);
      qc.invalidateQueries({ queryKey: ["conceptRuns"] });
      if (at.run === r.uid) onAt({ ...at, run: null, card: null });
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };

  const chapters = (view.data?.units ?? []).filter((u) => u.body > 0);
  const all = runs.data ?? [];
  const byUnit = (key: string) => all.filter((r) => unitOfRun(r) === key);
  const others = all.filter((r) => !unitOfRun(r) || !chapters.some((u) => u.key === unitOfRun(r)));

  const runItems = (list: ConceptRunSummary[]) =>
    list.map((r) => {
      const menu = (e: React.MouseEvent) => ui.menu(e, [{ label: "删除这次研究…", danger: true, onClick: () => void remove(r) }]);
      return (
        <div key={r.uid}>
          <NavItem
            indent
            label={r.skillTitle}
            sub={r.unit ? `${r.cards.length} 张卡片` : r.label}
            right={
              <span className="flex gap-1">
                {r.mock && <Badge tone="warn">模拟</Badge>}
                {r.status !== "done" && <Badge tone={RUN_TONE[r.status]}>{RUN_STATUS[r.status]}</Badge>}
              </span>
            }
            active={at.run === r.uid && !at.card}
            onClick={() => onAt({ ...at, run: r.uid, card: null })}
            onMenu={menu}
          />
          {at.run === r.uid &&
            r.cards.map((c) => (
              <button
                key={c.key}
                onClick={() => onAt({ ...at, run: r.uid, card: c.key })}
                className={`block w-full truncate rounded-md py-0.5 pr-2 pl-10 text-left text-[11px] ${at.card === c.key ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted"}`}
                title={`${CARD_KIND[c.kind]}：${c.title}`}
              >
                {c.key} {c.title}
              </button>
            ))}
        </div>
      );
    });

  return (
    <SideNav title="概念">
      <NavItem icon={<BookOpenText size={14} />} label="书" active={!at.text} onClick={() => onAt({ text: null, unit: null, run: null, card: null })} />
      {texts.error && <p className="selectable px-2 text-xs text-destructive">{String(texts.error)}</p>}
      {texts.data && books.length === 0 && <p className="px-2 py-1 text-xs text-muted-foreground">还没有解析过的书。在文献库里选一条有 EPUB / PDF 附件的文献，点附件上的「解析成文本」。</p>}
      {books.map((b) => (
        <div key={b.uid} className="mt-1">
          <NavItem
            label={b.title || b.itemTitle || "（无标题）"}
            sub={[b.language === "zh" ? "中文" : "英文", b.translationUid ? "有对齐的中译" : null, b.hasPages ? "有页码" : null].filter(Boolean).join(" · ")}
            active={at.text === b.uid && at.unit === null && !at.run}
            onClick={() => onAt({ text: b.uid, unit: null, run: null, card: null })}
          />
          {at.text === b.uid && (
            <>
              <NavLabel>章</NavLabel>
              {chapters.map((u) => (
                <div key={u.index}>
                  <NavItem
                    indent
                    label={`${u.key} ${u.label}`}
                    title={u.label}
                    right={u.aligned === false ? <Badge tone="warn" title="这一章中英正文段数不同，没有对齐">未对齐</Badge> : undefined}
                    active={at.unit === u.index && !at.run}
                    onClick={() => onAt({ text: b.uid, unit: u.index, run: null, card: null })}
                  />
                  {runItems(byUnit(u.key))}
                </div>
              ))}
              {others.length > 0 && (
                <>
                  <NavLabel>其他（问题、追踪、反方、推演）</NavLabel>
                  {runItems(others)}
                </>
              )}
            </>
          )}
        </div>
      ))}
    </SideNav>
  );
}
