// 四种入口里要先选段落的两种（decisions/0024「一次概念研究」第 1 步）：
// - 问这本书：本地搜出候选段落，用户勾选，再交给「问这本书」技能；
// - 追踪概念：中英两种写法在全书里检索，按章列出命中，用户勾选，再交给「追踪概念」；
// 以及「自由」：任意范围 + 一句要求。
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LoaderCircle, Search } from "lucide-react";
import { api, type ConceptAskResult, type ConceptIndexStatus, type ConceptRunRequest, type ConceptSearchHit } from "@/shared/ipc";
import { Modal } from "@/shared/ui";
import { ROLE } from "@/concept/labels";

type Hit = ConceptSearchHit & { score?: number | null; via?: "semantic" | "keyword" | "both" };

function Hits({ hits, chosen, onChosen, grouped }: { hits: Hit[]; chosen: Set<string>; onChosen: (s: Set<string>) => void; grouped?: boolean }) {
  const toggle = (k: string) => {
    const n = new Set(chosen);
    if (n.has(k)) n.delete(k);
    else n.add(k);
    onChosen(n);
  };
  const groups: [string, Hit[]][] = [];
  for (const h of hits) {
    const g = grouped ? `${h.unitKey} ${h.unitLabel}` : "";
    const last = groups[groups.length - 1];
    if (last && last[0] === g) last[1].push(h);
    else groups.push([g, [h]]);
  }
  if (hits.length === 0) return <p className="py-4 text-center text-xs text-muted-foreground">没有搜到。换个说法，或用英文、中文各写一个词。</p>;
  return (
    <div className="max-h-[50vh] space-y-2 overflow-y-auto pr-1">
      {groups.map(([g, list]) => (
        <div key={g}>
          {g && <p className="sticky top-0 bg-card py-0.5 text-[11px] font-medium text-muted-foreground">{g}（{list.length}）</p>}
          {list.map((h) => (
            <label key={h.key} className="flex cursor-pointer gap-2 rounded-md px-1 py-1 text-xs hover:bg-muted">
              <input type="checkbox" checked={chosen.has(h.key)} onChange={() => toggle(h.key)} className="mt-0.5" />
              <span className="min-w-0 flex-1">
                <span className="font-medium tabular-nums">{h.key}</span>
                {h.role !== "body" && <span className="ml-1 text-muted-foreground">（{ROLE[h.role]}）</span>}
                {h.viaTranslation && <span className="ml-1 text-amber-700 dark:text-amber-400">在中译里搜到</span>}
                {typeof h.score === "number" && (
                  <span className="ml-1 rounded bg-muted px-1 text-[10px] text-muted-foreground tabular-nums" title="相关度（本机的重排模型打的分，0–100）">
                    {Math.round(h.score * 100)}
                  </span>
                )}
                {h.via === "keyword" && <span className="ml-1 text-[10px] text-muted-foreground">按词搜到</span>}
                <span className="line-clamp-2 text-muted-foreground">{h.text}</span>
                {h.viaTranslation && h.translationText && <span className="line-clamp-1 text-muted-foreground">中译：{h.translationText}</span>}
              </span>
            </label>
          ))}
        </div>
      ))}
    </div>
  );
}

function useSearch(textUid: string) {
  const [hits, setHits] = useState<ConceptSearchHit[] | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (q: string, preselect: number) => {
    setBusy(true);
    setError(null);
    try {
      const r = (await api.conceptSearch(textUid, q, 80)).filter((h) => h.role === "body");
      setHits(r);
      setChosen(new Set(r.slice(0, preselect).map((h) => h.key)));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return { hits, chosen, setChosen, busy, error, run };
}

/** 「问这本书」的检索：按意思找（本机的向量和重排模型）；用不了时后端退回按词，并在 note 里说明 */
function useAsk(textUid: string) {
  const [result, setResult] = useState<ConceptAskResult | null>(null);
  const [index, setIndex] = useState<ConceptIndexStatus | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 打开就在后台建语义索引（已经建好的什么都不做），进度跟着事件走
  useEffect(() => {
    let alive = true;
    api.conceptIndexStart(textUid).then((st) => alive && setIndex(st), () => {});
    const un = listen<{ textUid: string; done: number; total: number; running: boolean; error: string | null }>("concept-index", (e) => {
      if (e.payload.textUid === textUid) setIndex((old) => (old ? { ...old, ...e.payload } : old));
    }).catch(() => null); // 浏览器预览里没有事件
    return () => {
      alive = false;
      void un.then((f) => f?.());
    };
  }, [textUid]);
  const run = async (q: string, preselect: number) => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.conceptAskSearch(textUid, q, 16);
      const hits = r.hits.filter((h) => h.role === "body");
      setResult({ ...r, hits });
      if (!r.index.running) setIndex(r.index);
      setChosen(new Set(hits.slice(0, preselect).map((h) => h.key)));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return { result, index, chosen, setChosen, busy, error, run };
}

function IndexLine({ index }: { index: ConceptIndexStatus | null }) {
  if (!index) return null;
  if (index.issue) return <p className="text-muted-foreground">{index.issue}现在按词找：长句子可以只写关键词（用「；」分开几个词）。</p>;
  if (index.error && !index.running) return <p className="selectable text-destructive">语义索引没建成：{index.error}</p>;
  if (index.done < index.total)
    return (
      <p className="flex items-center gap-1.5 text-muted-foreground">
        {index.running && <LoaderCircle size={12} className="animate-spin" />}
        正在给这本书建语义索引：{index.done} / {index.total} 段（{index.model}，在本机算，不联网）。建好之前按词找。
      </p>
    );
  return <p className="text-muted-foreground">按意思找：直接写问题就行，中文、英文都可以（{index.model}，在本机算，不联网、不花钱）。</p>;
}

export function AskDialog({ textUid, onRequest, onClose }: { textUid: string; onRequest: (r: ConceptRunRequest) => void; onClose: () => void }) {
  const [question, setQuestion] = useState("");
  const s = useAsk(textUid);
  return (
    <Modal title="问这本书" onClose={onClose} width="w-[640px]">
      <div className="space-y-3 text-xs">
        <p className="text-muted-foreground">先在这本书里找出相关的段落，勾选以后再问模型。找段落在本机做；勾选的段落确认后才发给模型。</p>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (question.trim()) void s.run(question, 6);
          }}
        >
          <input value={question} onChange={(e) => setQuestion(e.target.value)} placeholder="例如：分工为什么受市场范围的限制？" className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1.5 outline-none focus:border-ring" />
          <button type="submit" disabled={!question.trim() || s.busy} className="flex items-center gap-1 rounded-md border px-3 py-1.5 hover:bg-muted disabled:opacity-50">
            {s.busy ? <LoaderCircle size={13} className="animate-spin" /> : <Search size={13} />}
            找段落
          </button>
        </form>
        <IndexLine index={s.index} />
        {s.busy && <p className="text-muted-foreground">第一次找要先启动本机的模型，可能要等十几秒。</p>}
        {s.error && <p className="selectable text-destructive">{s.error}</p>}
        {s.result?.note && <p className="selectable text-amber-700 dark:text-amber-400">{s.result.note}</p>}
        {s.result && s.result.mode === "semantic" && s.result.hits.length > 0 && <p className="text-muted-foreground">按相关度排，数字是相关度（0–100）；前 6 段已经勾上。</p>}
        {s.result && <Hits hits={s.result.hits} chosen={s.chosen} onChosen={s.setChosen} />}
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
            取消
          </button>
          <button
            disabled={!question.trim() || s.chosen.size === 0}
            onClick={() => onRequest({ textUid, skill: "ask", entry: "question", question, paragraphs: [...s.chosen] })}
            className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50"
          >
            用勾选的 {s.chosen.size} 段提问…
          </button>
        </div>
      </div>
    </Modal>
  );
}

export function TraceDialog({ textUid, onRequest, onClose }: { textUid: string; onRequest: (r: ConceptRunRequest) => void; onClose: () => void }) {
  const [en, setEn] = useState("");
  const [zh, setZh] = useState("");
  const s = useSearch(textUid);
  const terms = [en.trim(), zh.trim()].filter(Boolean);
  return (
    <Modal title="追踪概念" onClose={onClose} width="w-[640px]">
      <div className="space-y-3 text-xs">
        <p className="text-muted-foreground">写下这个概念的英文和中文写法，在全书里检索，按章列出命中；勾选要看的段落，再让模型按章列出用法与变化。</p>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (terms.length) void s.run(terms.join("；"), 40);
          }}
        >
          <input value={en} onChange={(e) => setEn(e.target.value)} placeholder="英文，如 division of labour" className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1.5 outline-none focus:border-ring" />
          <input value={zh} onChange={(e) => setZh(e.target.value)} placeholder="中文，如 分工" className="selectable w-40 rounded-md border bg-background px-2 py-1.5 outline-none focus:border-ring" />
          <button type="submit" disabled={!terms.length || s.busy} className="flex items-center gap-1 rounded-md border px-3 py-1.5 hover:bg-muted disabled:opacity-50">
            {s.busy ? <LoaderCircle size={13} className="animate-spin" /> : <Search size={13} />}
            检索
          </button>
        </form>
        {s.error && <p className="selectable text-destructive">{s.error}</p>}
        {s.hits && <Hits hits={s.hits} chosen={s.chosen} onChosen={s.setChosen} grouped />}
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
            取消
          </button>
          <button disabled={!terms.length || s.chosen.size === 0} onClick={() => onRequest({ textUid, skill: "trace", entry: "concept", terms, paragraphs: [...s.chosen] })} className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
            追踪勾选的 {s.chosen.size} 段…
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** 自由：一句要求 + 现在的范围（划选的段落或这一章） */
export function FreeDialog({ base, label, onRequest, onClose }: { base: ConceptRunRequest; label: string; onRequest: (r: ConceptRunRequest) => void; onClose: () => void }) {
  const [instruction, setInstruction] = useState("");
  const examples = ["做一份术语表", "每段写一句提要", "出五道复习题，附答案和出处"];
  return (
    <Modal title="自由" onClose={onClose} width="w-[520px]">
      <div className="space-y-3 text-xs">
        <p className="text-muted-foreground">范围：{label}。写一句要求，结果做成笔记；好用的可以固化成技能（放进技能文件夹）。</p>
        <textarea value={instruction} onChange={(e) => setInstruction(e.target.value)} rows={3} placeholder="例如：做一份术语表" className="selectable w-full resize-y rounded-md border bg-background px-2 py-1.5 outline-none focus:border-ring" />
        <div className="flex flex-wrap gap-1">
          {examples.map((x) => (
            <button key={x} onClick={() => setInstruction(x)} className="rounded-full border px-2 py-0.5 text-muted-foreground hover:bg-muted">
              {x}
            </button>
          ))}
        </div>
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="rounded-md border px-3 py-1.5 hover:bg-muted">
            取消
          </button>
          <button disabled={!instruction.trim()} onClick={() => onRequest({ ...base, skill: "free", instruction })} className="rounded-md bg-primary px-3 py-1.5 text-primary-foreground disabled:opacity-50">
            下一步…
          </button>
        </div>
      </div>
    </Modal>
  );
}
