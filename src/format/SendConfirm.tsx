// 每次把内容发给模型之前的确认框（提炼要求、文档计划共用）：列出要发什么、发给谁、预计多少钱；
// 用户点「发送」才调用，发送中显示进度和「停止」，出错把后端的原因原样显示。
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { LoaderCircle, Send, Square } from "lucide-react";
import { api, type AgentEstimate, type AgentProgress, type FormatExtractPreview, type FormatPlanPreview, type FormatRulesetView, type FormatRunView } from "@/shared/ipc";
import { Modal } from "@/shared/ui";
import { count, MODE, MODE_HINT, modelName, usd } from "@/format/labels";
import { Notice } from "@/format/parts";

type Props<T> = {
  /** 这次调用做什么，如「提炼要求」 */
  purpose: string;
  estimate: AgentEstimate;
  mock: boolean;
  hasKey: boolean;
  /** 要发的内容 */
  children: React.ReactNode;
  run: () => Promise<T>;
  onDone: (result: T) => void;
  onClose: () => void;
  /** 去「设置 › 排版 Agent」 */
  onSettings: () => void;
};

function SendDialog<T>({ purpose, estimate, mock, hasKey, children, run, onDone, onClose, onSettings }: Props<T>) {
  const qc = useQueryClient();
  const [phase, setPhase] = useState<"confirm" | "sending" | "error">("confirm");
  const [progress, setProgress] = useState<AgentProgress | null>(null);
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

  // 发送中：听进度事件、数秒
  useEffect(() => {
    if (phase !== "sending") return;
    const un = listen<AgentProgress>("agent-progress", (e) => setProgress(e.payload));
    const timer = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => {
      clearInterval(timer);
      un.then((f) => f());
    };
  }, [phase]);

  const noKey = !mock && !hasKey;
  const overBudget = !mock && !estimate.withinBudget;
  const who = `Anthropic 的 ${modelName(estimate.model)}`;

  const send = async () => {
    setPhase("sending");
    try {
      const r = await run();
      // 账本多了一行
      qc.invalidateQueries({ queryKey: ["agentStatus"] });
      if (alive.current) onDone(r);
    } catch (e) {
      // 出错、中途停止也可能已经产生费用
      qc.invalidateQueries({ queryKey: ["agentStatus"] });
      if (!alive.current) return;
      setError(String(e));
      setPhase("error");
    }
  };

  const stop = () => {
    setStopping(true);
    void api.formatCancel();
  };

  const title = phase === "error" ? `${purpose}没有完成` : mock ? `用模拟模型${purpose}` : `发给 ${who}？`;

  return (
    // 发送中不能点外面或按 Esc 关掉：调用还在进行、还在花钱，只能点「停止」
    <Modal title={title} onClose={phase === "sending" ? () => {} : onClose} width="w-[32rem]">
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
          {mock ? (
            <Notice tone="info" title="现在是模拟模型：不联网、不花钱，结果是示例。" />
          ) : (
            <p className="text-muted-foreground">
              点「发送」后，下面列出的内容会发到 api.anthropic.com，由 {who} {purpose}。只有这一次，别的内容不会发。
            </p>
          )}

          <div>
            <p className="mb-1 font-medium">要发的内容</p>
            <div className="rounded-md border px-3 py-2">{children}</div>
          </div>

          <div>
            <p className="mb-1 font-medium">费用</p>
            {mock ? (
              <p className="text-muted-foreground">
                模拟模型不花钱。换成真模型的话，预计约 {usd(estimate.typicalUsd)}，最多 {usd(estimate.maxUsd)}。
              </p>
            ) : (
              <>
                <p>
                  预计约 <span className="font-medium tabular-nums">{usd(estimate.typicalUsd)}</span>，最多 <span className="tabular-nums">{usd(estimate.maxUsd)}</span>
                </p>
                <p className="text-muted-foreground tabular-nums">
                  本月已花 {usd(estimate.monthSpentUsd)} / 上限 {usd(estimate.budgetUsd)}
                </p>
              </>
            )}
          </div>

          {noKey && (
            <Notice tone="danger" title="还没有填 Anthropic API 密钥，发不了。">
              到「设置 › 排版 Agent」填写后再回来。
              <button onClick={onSettings} className="ml-1 text-primary hover:underline">
                去设置
              </button>
            </Notice>
          )}
          {overBudget && (
            <Notice tone="danger" title="会超过月度上限，发不了。">
              这次最多可能花 {usd(estimate.maxUsd)}，加上本月已花的 {usd(estimate.monthSpentUsd)} 会超过上限 {usd(estimate.budgetUsd)}。可以在「设置 › 排版 Agent」调高上限，或下个月再用。
              <button onClick={onSettings} className="ml-1 text-primary hover:underline">
                去设置
              </button>
            </Notice>
          )}

          {phase === "sending" ? (
            <div className="flex items-center gap-2 rounded-md bg-muted px-3 py-2">
              <LoaderCircle size={14} className="shrink-0 animate-spin text-muted-foreground" />
              <span className="min-w-0 flex-1 tabular-nums">
                {stopping ? "正在停止…" : progressText(progress, mock)}
                <span className="ml-2 text-muted-foreground">{seconds} 秒</span>
              </span>
              <button onClick={stop} disabled={stopping} className="flex shrink-0 items-center gap-1 rounded-md border bg-background px-2.5 py-1 hover:bg-muted disabled:opacity-50">
                <Square size={11} />
                停止
              </button>
            </div>
          ) : (
            <div className="flex justify-end gap-2 pt-1">
              <button onClick={onClose} className="rounded-md border px-3 py-1 hover:bg-muted">
                取消
              </button>
              <button onClick={send} disabled={noKey || overBudget} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-40">
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

function progressText(p: AgentProgress | null, mock: boolean): string {
  if (p && p.outputChars > 0) return `已收到 ${count(p.outputChars)} 字`;
  if (p && p.thinkingChars > 0) return `模型在思考…（${count(p.thinkingChars)} 字）`;
  return mock ? "模拟模型在准备示例…" : "已发送，模型在思考…";
}

/** 提炼要求：把选的规范文档发给模型 */
export function ExtractSend({ preview, onDone, onClose, onSettings }: { preview: FormatExtractPreview; onDone: (r: FormatRulesetView) => void; onClose: () => void; onSettings: () => void }) {
  const total = preview.docs.reduce((n, d) => n + d.chars, 0);
  return (
    <SendDialog purpose="提炼要求" estimate={preview.estimate} mock={preview.mock} hasKey={preview.hasKey} run={() => api.formatExtractRun(preview.ticket)} onDone={onDone} onClose={onClose} onSettings={onSettings}>
      <ul className="space-y-1">
        {preview.docs.map((d) => (
          <li key={d.label} className="flex items-baseline gap-2">
            <span className="w-6 shrink-0 font-mono text-muted-foreground">{d.label}</span>
            <span className="selectable min-w-0 flex-1 break-all">{d.name}</span>
            <span className="shrink-0 text-muted-foreground tabular-nums">{count(d.chars)} 字</span>
          </li>
        ))}
      </ul>
      <p className="mt-1.5 border-t pt-1.5 text-muted-foreground">
        {preview.docs.length} 份文档的全文，合计 <span className="tabular-nums">{count(total)}</span> 字。
      </p>
    </SendDialog>
  );
}

/** 文档计划：把论文按选的方式发给模型 */
export function PlanSend({ preview, onDone, onClose, onSettings }: { preview: FormatPlanPreview; onDone: (r: FormatRunView) => void; onClose: () => void; onSettings: () => void }) {
  return (
    <SendDialog purpose="生成文档计划" estimate={preview.estimate} mock={preview.mock} hasKey={preview.hasKey} run={() => api.formatPlanRun(preview.ticket)} onDone={onDone} onClose={onClose} onSettings={onSettings}>
      <dl className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-2 gap-y-1">
        <dt className="text-muted-foreground">论文</dt>
        <dd className="selectable break-all">{preview.thesisName}</dd>
        <dt className="text-muted-foreground">发送方式</dt>
        <dd>
          {MODE[preview.mode]}
          <span className="text-muted-foreground">：{MODE_HINT[preview.mode]}</span>
          <span className="block text-muted-foreground">想换成「{MODE[preview.mode === "brief" ? "full" : "brief"]}」：设置 › 排版 Agent › 论文发送方式。</span>
        </dd>
        <dt className="text-muted-foreground">论文大小</dt>
        <dd className="tabular-nums">
          {count(preview.paragraphs)} 个段落，共 {count(preview.totalChars)} 字
        </dd>
      </dl>
      <p className="mt-1.5 border-t pt-1.5">
        这种方式会发出论文里的 <span className="font-medium tabular-nums">{count(preview.sentChars)}</span> 字
        <span className="text-muted-foreground">，另有论文的样式名、分节情况，和这套规范要设置的格式。</span>
      </p>
    </SendDialog>
  );
}
