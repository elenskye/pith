// 事实调查（v0.3，decisions/0020）：问一个问题 → 确认要发什么、多少钱 → 模型拆成子问题 → 你确认计划 →
// Pith 检索、下载、摘证据、逐字核对、写结论 → 短答（数字点回原文）+ 全过程 + 导出 / 加入文献库 / 重查。
// 规则都在 Rust（crates/research-engine、src-tauri/src/research/）；这里只管界面。
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle, Telescope, Trash2 } from "lucide-react";
import { api, type ResearchPlanPreview } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Badge, Notice, Page } from "@/format/parts";
import { when } from "@/format/labels";
import { PlanSend } from "@/research/Confirm";
import { RunScreen } from "@/research/RunScreen";
import { STATUS, STATUS_TONE, money } from "@/research/labels";

/** 调查页现在看的是哪一次调查（null = 问题列表） */
export type ResearchAt = { uid: string | null };

export function ResearchPage({ at, onAt, onSettings }: { at: ResearchAt; onAt: (at: ResearchAt) => void; onSettings: () => void }) {
  if (at.uid) return <RunScreen key={at.uid} uid={at.uid} onRoot={() => onAt({ uid: null })} onOpen={(uid) => onAt({ uid })} onSettings={onSettings} />;
  return <Questions onOpen={(uid) => onAt({ uid })} onSettings={onSettings} />;
}

const EXAMPLE = "2022–2026 年北京、天津年轻人（应届生和非应届生）的就业情况怎么样？和其他一线城市、热门二线城市比呢？";

function Questions({ onOpen, onSettings }: { onOpen: (uid: string) => void; onSettings: () => void }) {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const list = useQuery({ queryKey: ["researchQuestions"], queryFn: api.researchQuestionsList, staleTime: 0 });
  const status = useQuery({ queryKey: ["researchStatus"], queryFn: api.researchStatus, staleTime: 0 });
  const [question, setQuestion] = useState("");
  const [preparing, setPreparing] = useState(false);
  const [preview, setPreview] = useState<ResearchPlanPreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ask = async () => {
    setError(null);
    setPreparing(true);
    try {
      setPreview(await api.researchPlanPrepare(question));
    } catch (e) {
      setError(String(e));
    } finally {
      setPreparing(false);
    }
  };

  const removeQuestion = async (id: number, text: string) => {
    const ok = await confirm({ title: "删除这个问题和它的所有调查？", message: `「${text.slice(0, 40)}」\n已经加入文献库的来源不受影响。`, confirm: "删除", danger: true });
    if (!ok) return;
    try {
      await api.researchQuestionDelete(id);
      qc.invalidateQueries({ queryKey: ["researchQuestions"] });
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const s = status.data;
  const missing = s ? s.keys.filter((k) => k.needed && !k.has).map((k) => k.label) : [];
  return (
    <Page crumbs={[{ label: "调查" }]}>
      <div className="space-y-2 text-xs text-muted-foreground">
        <p>
          问一个需要查证的问题。模型先把它拆成可以核实的子问题（包括官方统计之外的衡量维度），你确认后，Pith 搜索、在这台电脑上下载网页和文件、请便宜的模型摘出数字，
          <span className="text-foreground">每条摘录和数字都由 Pith 在保存下来的原文里逐字核对</span>，计算也由 Pith 做，最后只用核实过的事实写 3–5 条短答。
        </p>
        <p>
          来源按数据生产者的可信度排序；中国政府官方统计默认降权并标明（设置里可以改）。
          {s && !s.mock && (
            <span className="tabular-nums">
              {" "}
              本月已花 {money(s.monthSpentUsd)} / 上限 {money(s.budgetUsd)}（和排版 Agent 共用）。
            </span>
          )}
        </p>
      </div>

      {s?.mock && <Notice tone="info" title="现在是模拟接口：不联网、不花钱，来源和数字都是虚构的「示例市」示例。" />}
      {s && !s.mock && missing.length > 0 && (
        <Notice title={`还没有填 ${missing.join("、")} 的 API 密钥。`}>
          <button onClick={onSettings} className="text-primary hover:underline">
            去「设置 › 事实调查」填写
          </button>
        </Notice>
      )}

      <form
        className="space-y-2"
        onSubmit={(e) => {
          e.preventDefault();
          void ask();
        }}
      >
        <textarea
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          rows={3}
          placeholder={`要调查的问题，例如：${EXAMPLE}`}
          className="selectable w-full resize-y rounded-md border bg-background px-3 py-2 text-sm outline-none focus:border-ring"
        />
        <div className="flex items-center justify-between gap-2">
          <button type="button" onClick={() => setQuestion(EXAMPLE)} className="text-xs text-muted-foreground hover:text-foreground hover:underline">
            用验收问题
          </button>
          <button type="submit" disabled={!question.trim() || preparing || !!s?.running} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground disabled:opacity-50">
            {preparing ? <LoaderCircle size={13} className="animate-spin" /> : <Telescope size={13} />}
            开始调查…
          </button>
        </div>
        {s?.running && <p className="text-xs text-muted-foreground">有一次调查正在进行，做完或停止以后才能开始新的。</p>}
        {error && <p className="selectable text-xs whitespace-pre-wrap text-destructive">{error}</p>}
      </form>

      {list.error && <p className="selectable text-xs text-destructive">{String(list.error)}</p>}
      {list.data && list.data.length === 0 ? (
        <p className="rounded-md border border-dashed px-4 py-6 text-center text-xs text-muted-foreground">还没有调查过。</p>
      ) : (
        <ul className="space-y-2">
          {(list.data ?? []).map((q) => (
            <li key={q.id} className="rounded-md border">
              <div className="flex items-start gap-2 px-3 pt-2">
                <p className="min-w-0 flex-1 text-sm font-medium">{q.text}</p>
                <button onClick={() => void removeQuestion(q.id, q.text)} className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-destructive" title="删除这个问题">
                  <Trash2 size={13} />
                </button>
              </div>
              <ul className="divide-y px-1 pb-1">
                {q.runs.map((r) => (
                  <li key={r.uid}>
                    <button onClick={() => onOpen(r.uid)} className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-muted/60">
                      <span className="w-14 shrink-0 text-muted-foreground">第 {r.seq} 次</span>
                      <Badge tone={STATUS_TONE[r.status]}>{STATUS[r.status]}</Badge>
                      {r.mock && <Badge tone="warn">模拟</Badge>}
                      <span className="min-w-0 flex-1 truncate text-muted-foreground tabular-nums">
                        {when(r.finishedAt ?? r.createdAt)}
                        {r.status === "done" && ` · ${r.sources} 个来源 · ${r.verifiedFacts} 条核实的事实 · ${r.conclusions} 条结论`}
                      </span>
                      {!r.mock && r.costUsd > 0 && <span className="shrink-0 tabular-nums">{money(r.costUsd)}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}

      {preview && (
        <PlanSend
          preview={preview}
          onSettings={onSettings}
          onClose={() => setPreview(null)}
          onDone={(v) => {
            setPreview(null);
            setQuestion("");
            qc.setQueryData(["researchRun", v.run.uid], v);
            qc.invalidateQueries({ queryKey: ["researchQuestions"] });
            onOpen(v.run.uid);
          }}
        />
      )}
    </Page>
  );
}
