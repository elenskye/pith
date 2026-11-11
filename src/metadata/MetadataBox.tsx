// 文献详情里的「联网补全」：最近一次查到了什么、补了哪些（可撤销）、哪些和现有不同（采用 / 忽略），重新查。
import { useQuery } from "@tanstack/react-query";
import { Check, RefreshCw, Undo2, X } from "lucide-react";
import { api, type LookupChange, type LookupView } from "@/shared/ipc";
import { useMutate } from "@/shared/ui";
import { fieldLabel, formatTime, itemTypeLabel } from "@/library/labels";
import { useMetadata } from "./MetadataProvider";

const SOURCE: Record<string, string> = { crossref: "Crossref", datacite: "DataCite", openlibrary: "Open Library", medra: "mEDRA", jalc: "JaLC" };
export const sourceName = (s: string | null) => (s ? (SOURCE[s] ?? s) : "");

const shown = (c: LookupChange, v: string | null) => (v && c.field === "itemType" ? itemTypeLabel(v) : v);

export function MetadataBox({ itemId, hasIdentifier, locked }: { itemId: number; hasIdentifier: boolean; locked: boolean }) {
  const { lookup } = useMetadata();
  const latest = useQuery({ queryKey: ["metadata", itemId], queryFn: () => api.metadataLatest(itemId) });
  const status = useQuery({ queryKey: ["lookupStatus"], queryFn: api.metadataStatus });
  const l = latest.data;
  if (!hasIdentifier && !l) return null;
  const running = status.data?.running ?? false;

  return (
    <div className="mt-5">
      <div className="mb-1.5 flex items-center justify-between">
        <h3 className="text-xs font-medium text-muted-foreground">联网补全</h3>
        {hasIdentifier && !locked && (
          <button onClick={() => void lookup([itemId])} disabled={running} className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground disabled:opacity-50">
            <RefreshCw size={12} className={running ? "animate-spin" : ""} />
            {running ? "正在查…" : l ? "重新查" : "按 DOI / ISBN 查"}
          </button>
        )}
      </div>
      {!l ? (
        <p className="text-xs text-muted-foreground">还没有查过。查到的只补空着的字段，和现有不同的会请你确认。</p>
      ) : (
        <Lookup l={l} locked={locked} />
      )}
    </div>
  );
}

function Lookup({ l, locked }: { l: LookupView; locked: boolean }) {
  const mutate = useMutate();
  const when = formatTime(l.createdAt);
  if (l.status !== "found") {
    return (
      <p className="text-xs text-muted-foreground">
        {when} {l.status === "not_found" ? "没找到" : "联网失败"}：{l.message}
      </p>
    );
  }
  const applied = l.changes.filter((c) => c.state === "applied");
  const pending = l.changes.filter((c) => c.state === "pending");
  const decided = l.changes.filter((c) => ["accepted", "ignored", "undone", "kept"].includes(c.state));
  return (
    <div className="space-y-2 text-xs">
      <p className="text-muted-foreground">
        {when} 从 {sourceName(l.source)} 查到
        {applied.length > 0 ? `，补上了 ${applied.map((c) => fieldLabel(c.field)).join("、")}` : l.changes.length === 0 ? "，和现有一致，没有要改的" : ""}
        {applied.length > 0 && !locked && l.trigger !== "add" && (
          <button onClick={() => void mutate(() => api.metadataUndo(l.id), (r) => (r.reverted ? `已撤销 ${r.reverted} 处` : "之后都被改过了，没有撤销"))} className="ml-1.5 inline-flex items-center gap-0.5 text-foreground hover:underline">
            <Undo2 size={11} />
            撤销
          </button>
        )}
      </p>
      {pending.length > 0 && (
        <ul className="space-y-1.5 rounded-md border border-dashed p-2">
          <li className="text-[11px] text-muted-foreground">与现有不同，等你确认：</li>
          {pending.map((c) => (
            <li key={c.field} className="flex items-start gap-2">
              <div className="min-w-0 flex-1">
                <span className="text-muted-foreground">{fieldLabel(c.field)}：</span>
                <span className="selectable line-through decoration-muted-foreground/60">{shown(c, c.current)}</span>
                <span className="mx-1 text-muted-foreground">→</span>
                <span className="selectable">{shown(c, c.found)}</span>
              </div>
              {!locked && (
                <span className="flex shrink-0 gap-1">
                  <button onClick={() => void mutate(() => api.metadataResolve(l.id, c.field, true))} title="采用查到的" className="flex items-center gap-0.5 rounded border px-1.5 py-0.5 hover:bg-muted">
                    <Check size={11} />
                    采用
                  </button>
                  <button onClick={() => void mutate(() => api.metadataResolve(l.id, c.field, false))} title="保留现有的" className="flex items-center gap-0.5 rounded border px-1.5 py-0.5 text-muted-foreground hover:bg-muted">
                    <X size={11} />
                    忽略
                  </button>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {decided.length > 0 && (
        <p className="text-[11px] text-muted-foreground">
          {decided.map((c) => `${fieldLabel(c.field)}${{ accepted: "已采用", ignored: "已忽略", undone: "已撤销", kept: "改过，保留" }[c.state]}`).join("；")}
        </p>
      )}
    </div>
  );
}
