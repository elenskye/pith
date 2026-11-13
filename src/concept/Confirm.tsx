// 概念研究每次发送前的确认框（decisions/0024「一次概念研究」第 3 步）：要发的段落（段数、字数、语言）、步骤、按最坏情况算的预计费用。
// 可选的步骤可以去掉，「也上网找」这类默认不跑的可以加上（改了重新估）。点「发送」后在后台跑，框关掉，结果页看进度。
import { useState } from "react";
import { LoaderCircle, Send } from "lucide-react";
import { api, type ConceptRunPreview, type ConceptRunRequest, type ConceptSkill } from "@/shared/ipc";
import { Modal } from "@/shared/ui";
import { Notice, Switch } from "@/format/parts";
import { money } from "@/research/labels";
import { MODEL_NAME, SEARCH_ENGINE } from "@/concept/labels";

export type Pending = { request: ConceptRunRequest | null; preview: ConceptRunPreview };

/** 发一个请求去估费用；出错交给调用方显示 */
export async function prepare(request: ConceptRunRequest): Promise<Pending> {
  return { request, preview: await api.conceptRunPrepare(request) };
}

export function RunConfirm({ pending, skill, onSent, onClose, onSettings }: { pending: Pending; skill?: ConceptSkill; onSent: (uid: string) => void; onClose: () => void; onSettings: () => void }) {
  const [p, setP] = useState(pending);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pv = p.preview;
  const blocked = pv.missingKeys.length > 0 || !pv.withinBudget;

  const toggle = async (id: string, kind: "skip" | "with", on: boolean) => {
    if (!p.request) return;
    const list = new Set(p.request[kind] ?? []);
    if (on) list.add(id);
    else list.delete(id);
    const request = { ...p.request, [kind]: [...list] };
    setBusy(true);
    setError(null);
    try {
      setP(await prepare(request));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const send = async () => {
    setBusy(true);
    setError(null);
    try {
      onSent(await api.conceptRunExecute(pv.ticket));
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const optional = skill?.steps.filter((s) => s.optional) ?? [];
  return (
    <Modal title={`${pv.resume ? "接着做" : "发送前确认"}：${pv.skillTitle}`} onClose={onClose} width="w-[640px]">
      <div className="space-y-3 text-xs">
        <div className="grid grid-cols-[88px_minmax(0,1fr)] gap-x-3 gap-y-1">
          <span className="text-muted-foreground">范围</span>
          <span>
            {pv.paragraphs > 0 ? (
              <>
                {pv.paragraphs} 段（{pv.first}
                {pv.last && pv.last !== pv.first ? ` … ${pv.last}` : ""}），{pv.chars.toLocaleString()} 字
              </>
            ) : (
              "没有原文段落"
            )}
            {pv.targets.length > 0 && <span className="block text-muted-foreground">卡片：{pv.targets.join("、")}</span>}
          </span>
          <span className="text-muted-foreground">语言</span>
          <span>{pv.languages.join(" + ")}</span>
          <span className="text-muted-foreground">模型</span>
          <span>
            {MODEL_NAME[pv.model] ?? pv.model}
            {pv.provider === "openai" && !pv.mock && <span className="text-muted-foreground">（推理深浅 {pv.effort}；store: false，不让 OpenAI 存对话）</span>}
          </span>
        </div>

        {(optional.length > 0 || skill?.steps.some((s) => s.offByDefault)) && p.request && (
          <div className="space-y-1 rounded-md border px-3 py-2">
            {skill?.steps
              .filter((s) => s.optional || s.offByDefault)
              .map((s) => {
                const on = s.offByDefault ? (p.request?.with ?? []).includes(s.id) : !(p.request?.skip ?? []).includes(s.id);
                return (
                  <label key={s.id} className="flex items-center gap-2">
                    <Switch on={on} disabled={busy} onChange={(v) => void toggle(s.id, s.offByDefault ? "with" : "skip", s.offByDefault ? v : !v)} />
                    <span>{s.title || s.id}</span>
                    {s.kind === "web" && <span className="text-muted-foreground">（网页检索：{SEARCH_ENGINE[pv.searchEngine] ?? pv.searchEngine}，本机下载 + 快照）</span>}
                  </label>
                );
              })}
          </div>
        )}

        <table className="w-full text-left">
          <thead className="text-muted-foreground">
            <tr className="border-b">
              <th className="py-1 pr-2 font-normal">步骤</th>
              <th className="py-1 pr-2 font-normal">发什么</th>
              <th className="py-1 text-right font-normal">预计 / 最多</th>
            </tr>
          </thead>
          <tbody>
            {pv.steps.map((s) => (
              <tr key={s.id} className="border-b border-border/60 align-top">
                <td className="py-1 pr-2 whitespace-nowrap">{s.title || s.id}</td>
                <td className="py-1 pr-2 text-muted-foreground">
                  {s.sends}
                  {s.kind === "model" && <span className="block tabular-nums">输入约 {s.inputTokens.toLocaleString()} token；输出最多 {s.maxOutputTokens.toLocaleString()} token（推理也算在内）</span>}
                </td>
                <td className="py-1 text-right whitespace-nowrap tabular-nums">{pv.mock ? "—" : `${money(s.typicalUsd)} / ${money(s.maxUsd)}`}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {pv.mock ? (
          <Notice tone="info" title="现在是模拟接口：不联网、不花钱，卡片来自录制的回复。" />
        ) : (
          <p className="tabular-nums">
            合计约 <span className="font-medium">{money(pv.typicalUsd)}</span>，最多 {money(pv.maxUsd)}
            <span className="ml-2 text-muted-foreground">
              本月已花 {money(pv.monthSpentUsd)} / 上限 {money(pv.budgetUsd)}（和排版、调查共用）
            </span>
          </p>
        )}
        {!pv.withinBudget && <Notice tone="danger" title="最坏情况会超过月度上限，发不了。可以少选几段、去掉几步，或在「设置 › 密钥与费用」调高上限。" />}
        {pv.missingKeys.length > 0 && (
          <Notice tone="danger" title={`还没有填 ${pv.missingKeys.join("、")} 的密钥，发不了。`}>
            <button onClick={onSettings} className="text-primary hover:underline">
              去「设置 › 密钥与费用」
            </button>
          </Notice>
        )}
        {error && <p className="selectable whitespace-pre-wrap text-destructive">{error}</p>}
        <div className="flex justify-end gap-2 pt-1">
          <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
            取消
          </button>
          <button onClick={() => void send()} disabled={busy || blocked} className="flex items-center gap-1 rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
            {busy ? <LoaderCircle size={13} className="animate-spin" /> : <Send size={13} />}
            {pv.mock ? "开始（模拟）" : "发送"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
