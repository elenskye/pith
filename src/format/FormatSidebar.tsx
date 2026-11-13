// 排版的侧栏：新建规范、各套规范；选中的规范下面列它排过的论文（decisions/0023）。
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { NavItem, NavLabel, NavNew, SideNav } from "@/app/SideNav";
import { Badge } from "@/format/parts";
import { RUN_STATE, when } from "@/format/labels";
import type { FormatAt } from "@/format/FormatPage";

export function FormatSidebar({ at, onAt }: { at: FormatAt; onAt: (at: FormatAt) => void }) {
  const list = useQuery({ queryKey: ["formatRulesets"], queryFn: api.formatRulesetsList });
  const current = useQuery({ queryKey: ["formatRuleset", at.rulesetId], queryFn: () => api.formatRulesetGet(at.rulesetId as string), enabled: at.rulesetId !== null });
  const rulesets = list.data ?? [];
  return (
    <SideNav title="排版">
      <NavNew label="新建规范" active={at.rulesetId === null} onClick={() => onAt({ rulesetId: null, runId: null })} />
      <NavLabel>规范</NavLabel>
      {list.error && <p className="selectable px-2 text-xs text-destructive">{String(list.error)}</p>}
      {list.data && rulesets.length === 0 && <p className="px-2 py-1 text-xs text-muted-foreground">还没有规范</p>}
      {rulesets.map((r) => (
        <div key={r.id}>
          <NavItem
            label={r.name}
            title={r.docs.join("\n")}
            sub={`${when(r.createdAt)} · ${r.requirements} 条要求 · ${r.runs} 份论文`}
            right={r.mock ? <Badge tone="warn">模拟</Badge> : undefined}
            active={at.rulesetId === r.id && at.runId === null}
            onClick={() => onAt({ rulesetId: r.id, runId: null })}
          />
          {at.rulesetId === r.id &&
            (current.data?.runs ?? []).map((run) => (
              <NavItem
                key={run.id}
                indent
                label={run.thesisName}
                title={run.thesisPath}
                right={<Badge tone={run.state === "delivered" ? "ok" : run.state === "blocked" ? "danger" : "plain"}>{RUN_STATE[run.state] ?? run.state}</Badge>}
                active={at.runId === run.id}
                onClick={() => onAt({ rulesetId: r.id, runId: run.id })}
              />
            ))}
        </div>
      ))}
    </SideNav>
  );
}
