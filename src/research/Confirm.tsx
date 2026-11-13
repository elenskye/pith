// 事实调查每次发送前的确认框（decisions/0020「流程」第 10 条）：计划前一次、确认计划后一次。
// 列出要发什么、发给谁、预计多少钱；用户点「发送」才联网。拆问题这一步在框里等结果（可以停止）；执行在后台做，框直接关掉。
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LoaderCircle, Send, Square } from "lucide-react";
import { api, type ResearchEstimate, type ResearchExecutePreview, type ResearchPlanPreview, type ResearchProgress, type ResearchRunView } from "@/shared/ipc";
import { Modal } from "@/shared/ui";
import { Notice } from "@/format/parts";
import { modelName } from "@/format/labels";
import { ENGINE, PROVIDER, STAGE, money } from "@/research/labels";

function EstimateBox({ estimate, mock }: { estimate: ResearchEstimate; mock: boolean }) {
  return (
    <div>
      <p className="mb-1 font-medium">费用</p>
      <table className="w-full text-left">
        <tbody>
          {estimate.lines.map((l) => (
            <tr key={l.label} className="border-b border-border/60">
              <td className="py-1 pr-2">{l.label}</td>
              <td className="py-1 pr-2 text-right text-muted-foreground tabular-nums">{l.count > 1 ? `${l.count} 次` : ""}</td>
              <td className="py-1 text-right whitespace-nowrap tabular-nums">
                约 {money(l.typicalUsd)} <span className="text-muted-foreground">/ 最多 {money(l.maxUsd)}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {mock ? (
        <p className="mt-1 text-muted-foreground">现在是模拟接口，不花钱。上面是换成真接口时的估计。</p>
      ) : (
        <p className="mt-1 tabular-nums">
          合计约 <span className="font-medium">{money(estimate.typicalUsd)}</span>，最多 {money(estimate.maxUsd)}
          <span className="ml-2 text-muted-foreground">
            本月已花 {money(estimate.monthSpentUsd)} / 上限 {money(estimate.budgetUsd)}（和排版 Agent 共用）
          </span>
        </p>
      )}
    </div>
  );
}

function MissingKeys({ keys, onSettings }: { keys: string[]; onSettings: () => void }) {
  if (keys.length === 0) return null;
  return (
    <Notice tone="danger" title={`还没有填 ${keys.join("、")} 的 API 密钥，发不了。`}>
      到「设置 › 密钥与费用」填写后再回来。
      <button onClick={onSettings} className="ml-1 text-primary hover:underline">
        去设置
      </button>
    </Notice>
  );
}

/** 新问题：确认后让模型拆成子问题（在框里等，几十秒到几分钟） */
export function PlanSend({ preview, onDone, onClose, onSettings }: { preview: ResearchPlanPreview; onDone: (v: ResearchRunView) => void; onClose: () => void; onSettings: () => void }) {
  const [phase, setPhase] = useState<"confirm" | "sending" | "error">("confirm");
  const [progress, setProgress] = useState<ResearchProgress | null>(null);
  const [seconds, setSeconds] = useState(0);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (phase !== "sending") return;
    const un = listen<ResearchProgress>("research-progress", (e) => e.payload.uid === null && setProgress(e.payload));
    const timer = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => {
      clearInterval(timer);
      un.then((f) => f());
    };
  }, [phase]);

  const blocked = preview.missingKeys.length > 0 || (!preview.mock && !preview.estimate.withinBudget);
  const send = async () => {
    setPhase("sending");
    try {
      const v = await api.researchPlanRun(preview.ticket);
      if (alive.current) onDone(v);
    } catch (e) {
      if (!alive.current) return;
      setError(String(e));
      setPhase("error");
    }
  };
  const who = `${PROVIDER[preview.provider] ?? preview.provider} 的 ${modelName(preview.model)}`;

  return (
    <Modal title={phase === "error" ? "拆问题没有完成" : preview.mock ? "用模拟接口拆问题" : `发给 ${who}？`} onClose={phase === "sending" ? () => {} : onClose} width="w-[34rem]">
      {phase === "error" ? (
        <>
          <p className="selectable text-xs leading-relaxed whitespace-pre-wrap text-destructive">{error}</p>
          <div className="mt-4 flex justify-end">
            <button onClick={onClose} className="rounded-md border px-3 py-1 text-xs hover:bg-muted">
              关闭
            </button>
          </div>
        </>
      ) : (
        <div className="space-y-3 text-xs">
          {preview.mock ? (
            <Notice tone="info" title="现在是模拟接口：不联网、不花钱，计划是虚构的「示例市」示例。" />
          ) : (
            <p className="text-muted-foreground">
              点「发送」后，下面的问题会发给 {who}，由它拆成可以核实的子问题{preview.webSearches > 0 ? `，并用 Claude 网页搜索摸底（最多 ${preview.webSearches} 次）` : ""}。它不回答问题、不给数字；拆好以后你先确认计划，才开始检索。
            </p>
          )}
          <div>
            <p className="mb-1 font-medium">要发的内容</p>
            <div className="selectable max-h-40 overflow-y-auto rounded-md border px-3 py-2 whitespace-pre-wrap">{preview.question}</div>
            <p className="mt-1 text-muted-foreground">问题 {preview.chars} 字，另有拆问题的说明（固定的提示词）。</p>
          </div>
          <EstimateBox estimate={preview.estimate} mock={preview.mock} />
          <MissingKeys keys={preview.missingKeys} onSettings={onSettings} />
          {!preview.mock && !preview.estimate.withinBudget && <Notice tone="danger" title="会超过月度上限，发不了。">可以在「设置 › 密钥与费用」调高上限（两边共用），或下个月再用。</Notice>}
          {phase === "sending" ? (
            <div className="flex items-center gap-2 rounded-md bg-muted px-3 py-2">
              <LoaderCircle size={14} className="shrink-0 animate-spin text-muted-foreground" />
              <span className="min-w-0 flex-1 tabular-nums">
                {stopping ? "正在停止…" : (progress?.message ?? "已发送，模型在想…")}
                <span className="ml-2 text-muted-foreground">{seconds} 秒</span>
              </span>
              <button
                onClick={() => {
                  setStopping(true);
                  void api.researchCancel();
                }}
                disabled={stopping}
                className="flex shrink-0 items-center gap-1 rounded-md border bg-background px-2.5 py-1 hover:bg-muted disabled:opacity-50"
              >
                <Square size={11} />
                停止
              </button>
            </div>
          ) : (
            <div className="flex justify-end gap-2 pt-1">
              <button onClick={onClose} className="rounded-md border px-3 py-1 hover:bg-muted">
                取消
              </button>
              <button onClick={send} disabled={blocked} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-40">
                <Send size={12} />
                发送
              </button>
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}

/** 确认计划后：开始（或接着）检索 → 下载 → 摘证据 → 核对 → 结论 → 报告。在后台做，框马上关 */
export function ExecuteSend({ preview, onStarted, onClose, onSettings }: { preview: ResearchExecutePreview; onStarted: () => void; onClose: () => void; onSettings: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const resume = preview.stage !== "search";
  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      await api.researchExecute(preview.uid, preview.ticket);
      onStarted();
    } catch (e) {
      setError(String(e));
      setStarting(false);
    }
  };
  return (
    <Modal title={resume ? `从「${STAGE[preview.stage]}」接着做？` : preview.mock ? "用模拟接口开始调查" : "开始调查？"} onClose={onClose} width="w-[34rem]">
      <div className="space-y-3 text-xs">
        {preview.mock ? (
          <Notice tone="info" title="现在是模拟接口：不联网、不花钱，来源和数字都是虚构的。" />
        ) : (
          <p className="text-muted-foreground">
            点「开始」后，Pith 会用 {ENGINE[preview.searchEngine] ?? preview.searchEngine} 搜索下面的搜索词，在这台电脑上逐个下载搜到的网页和文件（只取单页、不爬站），
            把正文发给 {modelName(preview.workerModel)} 摘证据，摘录和数字由 Pith 逐字核对，最后把核实过的事实发给 {modelName(preview.plannerModel)} 写结论。每次调用前都会检查月度上限。
          </p>
        )}
        <div>
          <p className="mb-1 font-medium">要做的</p>
          <ul className="list-disc space-y-0.5 rounded-md border py-2 pr-3 pl-7">
            {!resume && (
              <li>
                {preview.subQuestions} 个子问题，{preview.queries} 次搜索
              </li>
            )}
            <li>最多读 {preview.maxSources} 个来源{preview.manualSources > 0 ? `（含你手动加的 ${preview.manualSources} 个）` : ""}，只发正文，不发你的其他资料</li>
            <li>核实过的事实（不发原文全文）发去写结论；有对不上的数字会打回重写一次</li>
          </ul>
        </div>
        <EstimateBox estimate={preview.estimate} mock={preview.mock} />
        {preview.mayStop && <Notice title="最坏情况会超过月度上限。">不设单次上限：照常开始，花到上限时会停在当前这一步，调高上限后可以接着做。</Notice>}
        <MissingKeys keys={preview.missingKeys} onSettings={onSettings} />
        {error && <p className="selectable whitespace-pre-wrap text-destructive">{error}</p>}
        <div className="flex justify-end gap-2 pt-1">
          <button onClick={onClose} className="rounded-md border px-3 py-1 hover:bg-muted">
            取消
          </button>
          <button onClick={start} disabled={starting || preview.missingKeys.length > 0} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-40">
            {starting ? <LoaderCircle size={12} className="animate-spin" /> : <Send size={12} />}
            {resume ? "接着做" : "开始"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
