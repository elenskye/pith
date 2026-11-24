// 调查的侧栏：新调查、问过的问题；每个问题下面是它的各次调查（decisions/0023）。右键问题可以删除。
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { NavItem, NavLabel, NavNew, SideNav } from "@/app/SideNav";
import { Badge } from "@/format/parts";
import { when } from "@/format/labels";
import { STATUS, STATUS_TONE } from "@/research/labels";
import type { ResearchAt } from "@/research/ResearchPage";

export function ResearchSidebar({ at, onAt }: { at: ResearchAt; onAt: (at: ResearchAt) => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const list = useQuery({ queryKey: ["researchQuestions"], queryFn: api.researchQuestionsList, staleTime: 0 });
  const questions = list.data ?? [];

  const remove = async (id: number, text: string, uids: string[]) => {
    const ok = await ui.confirm({ title: "删除这个问题和它的所有调查？", message: `「${text.slice(0, 40)}」\n已经加入文献库的来源不受影响。`, confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.researchQuestionDelete(id);
      qc.invalidateQueries({ queryKey: ["researchQuestions"] });
      if (at.uid && uids.includes(at.uid)) onAt({ uid: null });
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };

  return (
    <SideNav title="调查">
      <NavNew label="新调查" active={at.uid === null} onClick={() => onAt({ uid: null })} />
      <NavLabel>问过的问题</NavLabel>
      {list.error && <p className="selectable px-2 text-xs text-destructive">{String(list.error)}</p>}
      {list.data && questions.length === 0 && <p className="px-2 py-1 text-xs text-muted-foreground">还没有调查过</p>}
      {questions.map((q) => {
        const uids = q.runs.map((r) => r.uid);
        const latest = q.runs[q.runs.length - 1];
        const menu = (e: React.MouseEvent) => ui.menu(e, [{ label: "删除这个问题…", danger: true, onClick: () => void remove(q.id, q.text, uids) }]);
        return (
          <div key={q.id} className="mb-1">
            <NavItem label={q.text} title={q.text} sub={when(q.updatedAt)} active={false} onClick={() => latest && onAt({ uid: latest.uid })} onMenu={menu} />
            {q.runs.map((r) => (
                <NavItem
                  key={r.uid}
                  indent
                  label={`第 ${r.seq} 次`}
                  right={
                    <span className="flex gap-1">
                      {r.mock && <Badge tone="warn">模拟</Badge>}
                      <Badge tone={STATUS_TONE[r.status]}>{STATUS[r.status]}</Badge>
                    </span>
                  }
                  active={at.uid === r.uid}
                  onClick={() => onAt({ uid: r.uid })}
                  onMenu={menu}
                />
            ))}
          </div>
        );
      })}
    </SideNav>
  );
}
