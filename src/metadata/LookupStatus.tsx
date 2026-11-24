// 侧栏底部：后台联网补全的进度条与「停止」
import { useQuery } from "@tanstack/react-query";
import { Globe, Square } from "lucide-react";
import { api } from "@/shared/ipc";

export function LookupStatus() {
  const status = useQuery({ queryKey: ["lookupStatus"], queryFn: api.metadataStatus });
  const p = status.data;
  if (!p?.running) return null;
  const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
  return (
    <div className="border-t px-3 py-2 text-xs">
      <div className="flex items-center gap-1.5">
        <Globe size={13} className="shrink-0 animate-pulse text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">
          联网补全 <span className="tabular-nums">{p.done} / {p.total}</span>
        </span>
        <button onClick={() => void api.metadataCancel().then(() => status.refetch())} title="停止（正在查的这条查完就停）" className="flex items-center gap-0.5 rounded px-1 text-muted-foreground hover:bg-muted hover:text-foreground">
          <Square size={10} />
          停止
        </button>
      </div>
      <div className="mt-1.5 h-1 overflow-hidden rounded bg-muted">
        <div className="h-full bg-primary transition-all" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
