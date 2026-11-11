// 排版 Agent（v0.2 W3，decisions/0018）：选学校的规范文档 → 模型提炼要求清单 → 逐条确认 → 选论文 → 模型出文档计划 → 确认 → 确定性排版 → 报告。
// 联网只有「提炼」「出计划」两步，每次发送前都弹确认框（SendConfirm）。规则都在 Rust（src-tauri/src/agent/）。
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle, Plus } from "lucide-react";
import { api, type FormatExtractPreview } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { usd, when } from "@/format/labels";
import { Badge, Notice, Page } from "@/format/parts";
import { ExtractSend } from "@/format/SendConfirm";
import { RulesetView } from "@/format/RulesetView";
import { RunView } from "@/format/RunView";

/** 排版页现在看的是哪一层：规范列表 / 一套规范 / 一份论文的计划 */
export type FormatAt = { rulesetId: string | null; runId: string | null };

export function FormatPage({ at, onAt, onSettings }: { at: FormatAt; onAt: (at: FormatAt) => void; onSettings: () => void }) {
  const root = () => onAt({ rulesetId: null, runId: null });
  if (at.rulesetId && at.runId) return <RunView key={at.runId} rulesetId={at.rulesetId} runId={at.runId} onRoot={root} onBack={() => onAt({ rulesetId: at.rulesetId, runId: null })} />;
  if (at.rulesetId) return <RulesetView key={at.rulesetId} id={at.rulesetId} onBack={root} onRun={(runId) => onAt({ rulesetId: at.rulesetId, runId })} onSettings={onSettings} />;
  return <RulesetList onOpen={(rulesetId) => onAt({ rulesetId, runId: null })} onSettings={onSettings} />;
}

function RulesetList({ onOpen, onSettings }: { onOpen: (id: string) => void; onSettings: () => void }) {
  const qc = useQueryClient();
  const { notify } = useUi();
  const list = useQuery({ queryKey: ["formatRulesets"], queryFn: api.formatRulesetsList });
  const status = useQuery({ queryKey: ["agentStatus"], queryFn: api.agentStatus, staleTime: 0 });
  const [preparing, setPreparing] = useState(false);
  const [preview, setPreview] = useState<FormatExtractPreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setError(null);
    try {
      const paths = await api.pickFormatDocs();
      if (paths.length === 0) return;
      setPreparing(true);
      setPreview(await api.formatExtractPrepare(paths));
    } catch (e) {
      setError(String(e));
    } finally {
      setPreparing(false);
    }
  };

  const s = status.data;
  return (
    <Page
      crumbs={[{ label: "排版" }]}
      actions={
        <button onClick={create} disabled={preparing} className="flex items-center gap-1 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50">
          {preparing ? <LoaderCircle size={13} className="animate-spin" /> : <Plus size={13} />}
          新建规范…
        </button>
      }
    >
      <div className="space-y-2 text-xs text-muted-foreground">
        <p>
          选学校的论文规范、模板、附件，模型提炼成「要求清单」，你逐条确认；再选一份论文，模型给出「文档计划」，你确认后由排版程序改一份副本、核对、出报告。模型只出清单和计划，不碰论文；原文件从不改写。
        </p>
        <p>
          只有提炼清单、出计划这两步联网，每次发送前都会列出要发的内容和预计费用。
          {s && !s.mock && (
            <span className="tabular-nums">
              {" "}
              本月已花 {usd(s.monthSpentUsd)} / 上限 {usd(s.budgetUsd)}。
            </span>
          )}
        </p>
      </div>

      {s?.mock && <Notice tone="info" title="现在是模拟模型：不联网、不花钱，结果是示例。" />}
      {s && !s.mock && !s.hasKey && (
        <Notice title="还没有填 Anthropic API 密钥。">
          提炼要求、生成计划要用它。
          <button onClick={onSettings} className="ml-1 text-primary hover:underline">
            去「设置 › 排版 Agent」填写
          </button>
        </Notice>
      )}
      {error && <p className="selectable text-xs whitespace-pre-wrap text-destructive">{error}</p>}
      {list.error && <p className="selectable text-xs text-destructive">{String(list.error)}</p>}

      {list.data && list.data.length === 0 ? (
        <div className="rounded-md border border-dashed px-4 py-8 text-center text-xs text-muted-foreground">
          <p>还没有规范。</p>
          <p className="mt-1">点「新建规范」，选学校发的规范文档（.docx、.doc、.rtf、.txt、.md，可多选；PDF 请先另存为 Word 或文本）。</p>
        </div>
      ) : (
        <ul className="divide-y rounded-md border">
          {(list.data ?? []).map((r) => (
            <li key={r.id}>
              <button onClick={() => onOpen(r.id)} className="flex w-full items-center gap-3 px-3 py-2.5 text-left hover:bg-muted/60">
                <div className="min-w-0 flex-1">
                  <p className="truncate font-medium" title={r.name}>
                    {r.name}
                  </p>
                  <p className="truncate text-xs text-muted-foreground" title={r.docs.join("\n")}>
                    {r.docs.join("、")}
                  </p>
                  <p className="text-xs text-muted-foreground tabular-nums">
                    {when(r.createdAt)} · {r.requirements} 条要求 · {r.runs} 份论文
                  </p>
                </div>
                {r.mock && (
                  <Badge tone="warn" title="模拟模型给的示例，不是真的提炼结果">
                    模拟
                  </Badge>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}

      {preview && (
        <ExtractSend
          preview={preview}
          onSettings={onSettings}
          onClose={() => setPreview(null)}
          onDone={(r) => {
            setPreview(null);
            qc.setQueryData(["formatRuleset", r.id], r);
            qc.invalidateQueries({ queryKey: ["formatRulesets"] });
            notify(r.mock ? `提炼出 ${r.requirements.length} 条要求（模拟模型，没有花钱）` : `提炼出 ${r.requirements.length} 条要求，实际费用 ${usd(r.call?.costUsd ?? 0)}`);
            onOpen(r.id);
          }}
        />
      )}
    </Page>
  );
}
