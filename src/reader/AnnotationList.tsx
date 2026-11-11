// 阅读器右侧的标注列表：按页内顺序；点一条跳回 PDF 原处；选中后可改颜色、评论、摘录文字，复制、删除。
import { Copy, Trash2 } from "lucide-react";
import { api, type Annotation } from "@/shared/ipc";
import { EditableText, useMutate } from "@/shared/ui";
import { formatTime } from "@/library/labels";
import { COLORS, kindLabel } from "./colors";

type Props = {
  list: Annotation[];
  readonly: string | null;
  selected: number | null;
  /** 刚放下的便签：评论框直接进入编辑 */
  fresh: number | null;
  onJump: (a: Annotation) => void;
  onDelete: (id: number) => void;
  onCopy: (a: Annotation) => void;
};

export function AnnotationList({ list, readonly, selected, fresh, onJump, onDelete, onCopy }: Props) {
  return (
    <aside className="flex w-80 shrink-0 flex-col border-l bg-card">
      <div className="flex h-9 shrink-0 items-center border-b px-3 text-xs font-medium">
        标注
        <span className="ml-1.5 text-muted-foreground tabular-nums">{list.length}</span>
      </div>
      {readonly && <p className="border-b bg-muted px-3 py-2 text-xs text-muted-foreground">{readonly}</p>}
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {list.length === 0 ? (
          <p className="px-2 py-6 text-center text-xs leading-relaxed text-muted-foreground">
            {readonly ? "没有标注" : "在页面上选中文字，点一个颜色就能高亮；点工具栏的便签按钮，再点页面，可以放一条便签。"}
          </p>
        ) : (
          <ul className="space-y-2">
            {list.map((a) => (
              <Card key={a.id} a={a} selected={a.id === selected} fresh={a.id === fresh} onJump={onJump} onDelete={onDelete} onCopy={onCopy} />
            ))}
          </ul>
        )}
      </div>
    </aside>
  );
}

function Card({ a, selected, fresh, onJump, onDelete, onCopy }: { a: Annotation; selected: boolean; fresh: boolean } & Pick<Props, "onJump" | "onDelete" | "onCopy">) {
  const mutate = useMutate();
  const color = a.color ?? "#ffd400";
  const page = a.pageLabel || String(a.pageIndex + 1);
  const update = (patch: Parameters<typeof api.annotationsUpdate>[1]) => mutate(() => api.annotationsUpdate(a.id, patch));
  return (
    <li
      id={`rl-card-${a.id}`}
      onClick={() => !selected && onJump(a)}
      className={`rounded-md border bg-background p-2 ${selected ? "border-ring ring-1 ring-ring" : "hover:border-muted-foreground/40"}`}
      style={{ borderLeft: `3px solid ${color}` }}
    >
      <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <button onClick={() => onJump(a)} className="hover:text-foreground hover:underline" title="跳到这里">
          第 {page} 页
        </button>
        <span>· {kindLabel(a.kind)}</span>
        {a.authorName && <span className="truncate">· {a.authorName}</span>}
        {a.isExternal && <span title="来自 PDF 文件本身，只读">· PDF 自带</span>}
      </div>
      {a.text !== null &&
        (selected && a.editable ? (
          <EditableText value={a.text} onSave={(v) => update({ text: v })} multiline className="mt-1 text-[12.5px] leading-relaxed" />
        ) : (
          <p className="selectable mt-1 line-clamp-6 text-[12.5px] leading-relaxed whitespace-pre-wrap">{a.text}</p>
        ))}
      <div className="mt-1 text-xs" onClick={(e) => selected && e.stopPropagation()}>
        {a.editable ? (
          <EditableText value={a.comment} onSave={(v) => update({ comment: v })} multiline placeholder="添加评论…" startEditing={fresh} className="leading-relaxed" />
        ) : (
          a.comment && <p className="selectable leading-relaxed whitespace-pre-wrap">{a.comment}</p>
        )}
      </div>
      {a.tags.length > 0 && (
        <div className="mt-1 flex flex-wrap gap-1">
          {a.tags.map((t) => (
            <span key={t} className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground">
              {t}
            </span>
          ))}
        </div>
      )}
      {selected && (
        <div className="mt-2 flex items-center gap-1 border-t pt-1.5" onClick={(e) => e.stopPropagation()}>
          {a.editable &&
            COLORS.map((c) => (
              <button
                key={c.value}
                onClick={() => c.value !== color && void update({ color: c.value })}
                title={`改成${c.label}色`}
                aria-label={`改成${c.label}色`}
                className={`size-3.5 rounded-full border border-black/10 ${c.value === color ? "ring-2 ring-ring ring-offset-1 ring-offset-background" : ""}`}
                style={{ background: c.value }}
              />
            ))}
          <span className="flex-1" />
          <span className="text-[11px] text-muted-foreground">{formatTime(a.updatedAt)}</span>
          {a.text && (
            <button onClick={() => onCopy(a)} title="复制摘录（带页码）" aria-label="复制摘录" className="rounded p-1 hover:bg-muted">
              <Copy size={13} />
            </button>
          )}
          {a.editable && (
            <button onClick={() => onDelete(a.id)} title="删除（可撤销）" aria-label="删除标注" className="rounded p-1 text-destructive hover:bg-muted">
              <Trash2 size={13} />
            </button>
          )}
        </div>
      )}
    </li>
  );
}
