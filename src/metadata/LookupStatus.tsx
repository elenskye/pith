// 文献库页头右边：后台联网补全的进度和「停止」（一行小字，下面一道海蓝的进度）
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/ipc";

export function LookupStatus() {
  const status = useQuery({ queryKey: ["lookupStatus"], queryFn: api.metadataStatus });
  const p = status.data;
  if (!p?.running) return null;
  const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
  return (
    <div className="lookup" style={{ "--p": `${pct}%` } as React.CSSProperties}>
      <span>
        联网补全{" "}
        <b className="num">
          {p.done} / {p.total}
        </b>
      </span>
      <button className="pith-link" onClick={() => void api.metadataCancel().then(() => status.refetch())} title="停止（正在查的这条查完就停）">
        停止
      </button>
      <i aria-hidden="true" />
    </div>
  );
}
