import { useState } from "react";
import { useMutation, useQueries, useQuery } from "@tanstack/react-query";
import { ArrowDown, ArrowUp, BookOpen, ExternalLink, FileText, Folder, Plus, RotateCcw, StickyNote, Trash2, X } from "lucide-react";
import {
  api,
  type AttachmentDetail,
  type ChildAttachment,
  type CollectionDetail,
  type CreatorInput,
  type ItemDetail,
  type NamedRef,
  type NoteDetail,
  type Parent,
  type Target,
} from "@/shared/ipc";
import { EditableText, TagInput, useMutate, useUi } from "@/shared/ui";
import { useRead } from "@/reader/useRead";
import { MetadataBox } from "@/metadata/MetadataBox";
import { useMetadata } from "@/metadata/MetadataProvider";
import { targetsOf, type Selection } from "./ItemList";
import type { Scope } from "./Sidebar";
import {
  ADDABLE_FIELDS,
  DELETE_FOREVER_NOTE,
  ITEM_TYPE_OPTIONS,
  ROLE_OPTIONS,
  attachmentKindLabel,
  containerLabel,
  contentTypeLabel,
  fieldLabel,
  formatSize,
  formatTime,
  itemTypeLabel,
  roleLabel,
  sourceLabel,
} from "./labels";

type Props = { selection: Selection[]; onSelection: (s: Selection[]) => void; scope: Scope };

/** 右侧：一项时显示（并可编辑）详情，多项时显示批量操作。只显示常用字段，其余收在「其他字段」。 */
export function DetailPane({ selection, onSelection, scope }: Props) {
  const one = selection.length === 1 ? selection[0] : null;
  const open = (s: Selection) => onSelection([s]);
  return (
    <aside className="flex w-[380px] shrink-0 flex-col border-l bg-card">
      {selection.length === 0 ? (
        <p className="m-auto text-muted-foreground">选择一条文献查看详情</p>
      ) : !one ? (
        <BatchPanel selection={selection} onSelection={onSelection} scope={scope} />
      ) : one.kind === "item" ? (
        <Load key={`i${one.id}`} query={{ queryKey: ["item", one.id], queryFn: () => api.itemsGet(one.id) }} render={(d) => <Item d={d} onSelect={open} onGone={() => onSelection([])} />} />
      ) : one.kind === "attachment" ? (
        <Load
          key={`a${one.id}`}
          query={{ queryKey: ["attachment", one.id], queryFn: () => api.attachmentsGet(one.id) }}
          render={(d) => <Attachment d={d} onSelect={open} onGone={() => onSelection([])} />}
        />
      ) : one.kind === "note" ? (
        <Load key={`n${one.id}`} query={{ queryKey: ["note", one.id], queryFn: () => api.notesGet(one.id) }} render={(d) => <Note d={d} onSelect={open} onGone={() => onSelection([])} />} />
      ) : (
        <Load key={`c${one.id}`} query={{ queryKey: ["collection", one.id], queryFn: () => api.collectionsGet(one.id) }} render={(d) => <Collection d={d} onGone={() => onSelection([])} />} />
      )}
    </aside>
  );
}

function Load<T>({ query, render }: { query: { queryKey: unknown[]; queryFn: () => Promise<T> }; render: (d: T) => React.ReactNode }) {
  const q = useQuery(query);
  if (q.error) return <p className="selectable p-5 text-destructive">{String(q.error)}</p>;
  return q.data ? <div className="selectable min-h-0 flex-1 overflow-y-auto p-5">{render(q.data)}</div> : null;
}

function Grid({ children }: { children: React.ReactNode }) {
  return <dl className="mt-4 grid grid-cols-[84px_1fr] items-baseline gap-x-3 gap-y-1.5">{children}</dl>;
}

function F({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="contents">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </div>
  );
}

function Section({ title, children, action }: { title: string; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <section className="mt-5">
      <div className="mb-1.5 flex items-center justify-between">
        <h3 className="text-xs text-muted-foreground">{title}</h3>
        {action}
      </div>
      {children}
    </section>
  );
}

function SmallButton({ onClick, children, danger, title }: { onClick: () => void; children: React.ReactNode; danger?: boolean; title?: string }) {
  return (
    <button onClick={onClick} title={title} className={`flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs hover:bg-muted ${danger ? "text-destructive" : ""}`}>
      {children}
    </button>
  );
}

/** 在回收站时的提示条 + 恢复 / 永久删除；不在时返回「移到回收站」按钮放在底部 */
function useTrashActions(target: Target, deletedAt: string | null, onGone: () => void) {
  const ui = useUi();
  const mutate = useMutate();
  const banner = deletedAt ? (
    <div className="mb-3 flex items-center gap-2 rounded bg-destructive/10 px-2 py-1 text-xs text-destructive">
      <span className="flex-1">在回收站（{formatTime(deletedAt)} 删除）</span>
      <SmallButton onClick={() => mutate(() => api.trashRestore([target]), "已恢复")}>
        <RotateCcw size={12} />
        恢复
      </SmallButton>
      <SmallButton
        danger
        onClick={async () => {
          const ok = await ui.confirm({ title: "永久删除", message: DELETE_FOREVER_NOTE, confirm: "永久删除", danger: true });
          if (ok && (await mutate(() => api.trashDelete([target]), "已永久删除"))) onGone();
        }}
      >
        永久删除
      </SmallButton>
    </div>
  ) : null;
  const footer = deletedAt ? null : (
    <div className="mt-8 border-t pt-3">
      <SmallButton danger onClick={() => mutate(() => api.trashMove([target]), "已移到回收站").then((r) => r !== undefined && onGone())}>
        <Trash2 size={12} />
        移到回收站
      </SmallButton>
    </div>
  );
  return { banner, footer };
}

function ParentLink({ parent, onSelect }: { parent: Parent | null; onSelect: (s: Selection) => void }) {
  if (!parent) return null;
  return (
    <button onClick={() => onSelect({ kind: parent.kind, id: parent.id })} className="mt-1 text-left text-xs text-muted-foreground hover:text-foreground hover:underline">
      所属{parent.kind === "item" ? "文献" : "笔记"}：{parent.title || "（无标题）"}
    </button>
  );
}

/** 在 App 的阅读器里打开 PDF */
function ReadButton({ id, primary }: { id: number; primary?: boolean }) {
  const { read } = useRead();
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        read(id);
      }}
      title="在阅读器里打开，可以高亮、摘录"
      className={`flex shrink-0 items-center gap-1 rounded-md border px-2 py-0.5 text-xs ${primary ? "border-primary bg-primary text-primary-foreground hover:opacity-90" : "hover:bg-muted"}`}
    >
      <BookOpen size={12} />
      阅读
    </button>
  );
}

function OpenButton({ id, label = "打开" }: { id: number; label?: string }) {
  const open = useMutation({ mutationFn: () => api.attachmentsOpen(id) });
  return (
    <span className="inline-flex flex-col items-end">
      <button
        onClick={(e) => {
          e.stopPropagation();
          open.mutate();
        }}
        className="flex shrink-0 items-center gap-1 rounded-md border px-2 py-0.5 text-xs hover:bg-muted"
      >
        <ExternalLink size={12} />
        {label}
      </button>
      {open.error && <span className="mt-0.5 text-[11px] text-destructive">{String(open.error)}</span>}
    </span>
  );
}

/** 标签：× 去掉，输入框回车添加（有已有标签的提示） */
function TagEditor({ tags, target, disabled }: { tags: string[]; target: Target; disabled?: boolean }) {
  const mutate = useMutate();
  const [draft, setDraft] = useState("");
  const add = async (raw: string = draft) => {
    const name = raw.trim();
    if (!name) return;
    if ((await mutate(() => api.tagsAdd([target], name))) !== undefined) setDraft("");
  };
  return (
    <div className="mt-4 flex flex-wrap items-center gap-1.5">
      {tags.map((t) => (
        <span key={t} className="flex items-center gap-0.5 rounded-full bg-muted py-0.5 pr-1 pl-2 text-xs">
          {t}
          {!disabled && (
            <button onClick={() => mutate(() => api.tagsRemove([target], t))} className="rounded-full text-muted-foreground hover:text-foreground" title="去掉这个标签">
              <X size={11} />
            </button>
          )}
        </span>
      ))}
      {!disabled && (
        <TagInput
          value={draft}
          onChange={setDraft}
          onEnter={(v) => void add(v)}
          onPick={(name) => void add(name)}
          onBlur={() => draft.trim() && void add()}
          exclude={tags}
          placeholder="+ 标签"
          className="selectable w-24 rounded-full border border-dashed bg-transparent px-2 py-0.5 text-xs outline-none focus:border-ring"
        />
      )}
    </div>
  );
}

/** 所在集合：× 移出，＋ 加入 */
function CollectionsEditor({ collections, target, disabled }: { collections: NamedRef[]; target: Target; disabled?: boolean }) {
  const ui = useUi();
  const mutate = useMutate();
  return (
    <span className="flex flex-wrap items-center gap-1">
      {collections.length === 0 && <span className="text-muted-foreground">（未分类）</span>}
      {collections.map((c) => (
        <span key={c.id} className="flex items-center gap-0.5 rounded bg-muted py-0.5 pr-1 pl-1.5 text-xs">
          <Folder size={11} className="text-muted-foreground" />
          {c.name}
          {!disabled && (
            <button onClick={() => mutate(() => api.collectionsRemove(c.id, [target]))} className="text-muted-foreground hover:text-foreground" title="移出这个集合">
              <X size={11} />
            </button>
          )}
        </span>
      ))}
      {!disabled && (
        <button
          onClick={async () => {
            const cid = await ui.pickCollection({ title: "添加到集合", confirm: "添加" });
            if (cid) await mutate(() => api.collectionsAdd(cid, [target]));
          }}
          className="rounded border border-dashed px-1 text-xs text-muted-foreground hover:text-foreground"
          title="添加到集合"
        >
          <Plus size={11} />
        </button>
      )}
    </span>
  );
}

type Draft = { role: string; single: boolean; family: string; given: string; literal: string };

/** 作者多于这个数时折叠（有的论文几十位作者，会占满详情栏） */
const AUTHORS_SHOWN = 10;

function CreatorsEditor({ d, disabled }: { d: ItemDetail; disabled: boolean }) {
  const mutate = useMutate();
  const [rows, setRows] = useState<Draft[] | null>(null);
  const [showAll, setShowAll] = useState(false);
  const start = () =>
    setRows(
      d.creators.length > 0
        ? d.creators.map((c) => ({ role: c.role, single: c.literal !== null, family: c.family ?? "", given: c.given ?? "", literal: c.literal ?? "" }))
        : [{ role: "author", single: false, family: "", given: "", literal: "" }],
    );
  if (!rows) {
    return (
      <div className="mt-2 flex items-start gap-2">
        <p className="min-w-0 flex-1 text-muted-foreground">
          {d.creators.length === 0 && <span>（没有作者）</span>}
          {(showAll ? d.creators : d.creators.slice(0, AUTHORS_SHOWN)).map((c, i) => (
            <span key={i}>
              {i > 0 && "；"}
              {c.name}
              {c.role !== "author" && <span className="text-xs">（{roleLabel(c.role)}）</span>}
            </span>
          ))}
          {d.creators.length > AUTHORS_SHOWN && (
            <button onClick={() => setShowAll(!showAll)} className="ml-1 text-xs hover:text-foreground">
              {showAll ? "收起" : `… 等 ${d.creators.length} 人，展开`}
            </button>
          )}
        </p>
        {!disabled && (
          <button onClick={start} className="shrink-0 text-xs text-muted-foreground hover:text-foreground">
            编辑作者
          </button>
        )}
      </div>
    );
  }
  const set = (i: number, patch: Partial<Draft>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const move = (i: number, by: number) => {
    const j = i + by;
    if (j < 0 || j >= rows.length) return;
    const next = [...rows];
    [next[i], next[j]] = [next[j], next[i]];
    setRows(next);
  };
  const save = async () => {
    const creators: CreatorInput[] = rows.map((r) => (r.single ? { role: r.role, literal: r.literal } : { role: r.role, family: r.family, given: r.given }));
    if ((await mutate(() => api.itemsUpdate(d.id, { creators }))) !== undefined) setRows(null);
  };
  const roles = Array.from(new Set([...ROLE_OPTIONS, ...rows.map((r) => r.role)]));
  const input = "selectable min-w-0 flex-1 rounded border bg-background px-1.5 py-0.5 text-xs outline-none focus:border-ring";
  return (
    <div className="mt-2 space-y-1.5 rounded-md border p-2">
      {rows.map((r, i) => (
        <div key={i} className="flex items-center gap-1">
          <select value={r.role} onChange={(e) => set(i, { role: e.target.value })} className="w-20 rounded border bg-background px-1 py-0.5 text-xs">
            {roles.map((x) => (
              <option key={x} value={x}>
                {roleLabel(x)}
              </option>
            ))}
          </select>
          {r.single ? (
            <input value={r.literal} onChange={(e) => set(i, { literal: e.target.value })} placeholder="全名（机构、中文姓名）" className={input} />
          ) : (
            <>
              <input value={r.family} onChange={(e) => set(i, { family: e.target.value })} placeholder="姓" className={input} />
              <input value={r.given} onChange={(e) => set(i, { given: e.target.value })} placeholder="名" className={input} />
            </>
          )}
          <button
            onClick={() => set(i, r.single ? { single: false, family: r.literal, literal: "" } : { single: true, literal: [r.family, r.given].filter(Boolean).join(" "), family: "", given: "" })}
            title={r.single ? "改成姓、名两栏" : "改成单栏（不拆姓名）"}
            className="w-7 shrink-0 rounded border px-0.5 text-[10px] text-muted-foreground hover:bg-muted"
          >
            {r.single ? "两栏" : "单栏"}
          </button>
          <button onClick={() => move(i, -1)} className="text-muted-foreground hover:text-foreground" title="上移">
            <ArrowUp size={12} />
          </button>
          <button onClick={() => move(i, 1)} className="text-muted-foreground hover:text-foreground" title="下移">
            <ArrowDown size={12} />
          </button>
          <button onClick={() => setRows(rows.filter((_, j) => j !== i))} className="text-muted-foreground hover:text-destructive" title="删除">
            <X size={12} />
          </button>
        </div>
      ))}
      <div className="flex items-center gap-2 pt-1">
        <button onClick={() => setRows([...rows, { role: "author", single: false, family: "", given: "", literal: "" }])} className="text-xs text-muted-foreground hover:text-foreground">
          + 添加作者
        </button>
        <span className="flex-1" />
        <button onClick={() => setRows(null)} className="rounded-md border px-2 py-0.5 text-xs hover:bg-muted">
          取消
        </button>
        <button onClick={save} className="rounded-md bg-primary px-2 py-0.5 text-xs text-primary-foreground">
          保存
        </button>
      </div>
    </div>
  );
}

function Item({ d, onSelect, onGone }: { d: ItemDetail; onSelect: (s: Selection) => void; onGone: () => void }) {
  const ui = useUi();
  const mutate = useMutate();
  const target: Target = { kind: "item", id: d.id };
  const { banner, footer } = useTrashActions(target, d.deletedAt, onGone);
  const locked = d.deletedAt !== null;
  const save = (field: string) => (v: string) => mutate(() => api.itemsUpdate(d.id, { fields: { [field]: v } }));
  const types = ITEM_TYPE_OPTIONS.includes(d.itemType) ? ITEM_TYPE_OPTIONS : [d.itemType, ...ITEM_TYPE_OPTIONS];

  const addField = async (name: string) => {
    const value = await ui.prompt({ title: `添加字段：${fieldLabel(name)}`, confirm: "添加" });
    if (value) await save(name)(value);
  };

  return (
    <>
      {banner}
      <EditableText value={d.title} onSave={save("title")} placeholder="（无标题，点击填写）" disabled={locked} className="text-base leading-snug font-semibold" />
      <CreatorsEditor d={d} disabled={locked} />
      <Grid>
        <F label="类型">
          <select
            value={d.itemType}
            disabled={locked}
            onChange={(e) => mutate(() => api.itemsUpdate(d.id, { itemType: e.target.value }))}
            className="-mx-1 rounded bg-transparent px-0.5 hover:bg-muted disabled:hover:bg-transparent"
          >
            {types.map((t) => (
              <option key={t} value={t}>
                {itemTypeLabel(t)}
              </option>
            ))}
          </select>
        </F>
        <F label="日期">
          <EditableText value={d.date} onSave={save("date")} placeholder="如 2019-03、March 2019、2025年6月" disabled={locked} />
        </F>
        {(d.containerField || d.containerTitle) && (
          <F label={containerLabel(d.containerField)}>
            <EditableText value={d.containerTitle} onSave={save("container")} disabled={locked} />
          </F>
        )}
        <F label="DOI">
          <EditableText value={d.doi} onSave={save("DOI")} disabled={locked} />
        </F>
        <F label="ISBN">
          <EditableText value={d.isbn} onSave={save("ISBN")} disabled={locked} />
        </F>
        <F label="语言">
          <EditableText value={d.language} onSave={save("language")} placeholder="（空：按标题自动判断）" disabled={locked} />
        </F>
        <F label="集合">
          <CollectionsEditor collections={d.collections} target={target} disabled={locked} />
        </F>
        {d.source && d.source !== "manual" && <F label="来源">{sourceLabel(d.source)} 导入</F>}
      </Grid>
      <TagEditor tags={d.tags} target={target} disabled={locked} />
      <MetadataBox itemId={d.id} hasIdentifier={!!(d.doi || d.isbn)} locked={locked} />

      <Section
        title={`附件（${d.attachments.length}）`}
        action={
          !locked && (
            <button
              onClick={async () => {
                const path = await api.pickAnyFile("选择要添加的文件（会复制进文献库，原文件不动）");
                if (path) await mutate(() => api.attachmentsAdd(d.id, path), "已添加附件");
              }}
              className="text-xs text-muted-foreground hover:text-foreground"
            >
              + 添加附件
            </button>
          )
        }
      >
        {d.attachments.length === 0 ? (
          <p className="text-xs text-muted-foreground">没有附件</p>
        ) : (
          <ul className="space-y-1">
            {d.attachments.map((a) => (
              <ChildRow key={a.id} a={a} onSelect={onSelect} />
            ))}
          </ul>
        )}
      </Section>
      {d.notes.length > 0 && (
        <Section title={`笔记（${d.notes.length}）`}>
          <ul className="space-y-1">
            {d.notes.map((n) => (
              <li key={n.id}>
                <button onClick={() => onSelect({ kind: "note", id: n.id })} className="flex w-full items-start gap-2 rounded-md px-1.5 py-1 text-left hover:bg-muted">
                  <StickyNote size={14} className="mt-0.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0">
                    <span className="block truncate">{n.title || "（无标题笔记）"}</span>
                    {n.preview && <span className="line-clamp-2 text-xs text-muted-foreground">{n.preview}</span>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
      <Section title="摘要">
        <EditableText value={d.abstract} onSave={save("abstractNote")} multiline placeholder="（没有摘要）" disabled={locked} className="leading-relaxed" />
      </Section>
      <details className="mt-5">
        <summary className="cursor-default text-xs text-muted-foreground">其他字段（{d.otherFields.length + (d.extra ? 1 : 0)}）</summary>
        <Grid>
          {d.otherFields.map((f) => (
            <F key={f.name} label={fieldLabel(f.name)}>
              <EditableText value={f.value} onSave={save(f.name)} disabled={locked} />
            </F>
          ))}
          <F label="其他信息">
            <EditableText value={d.extra} onSave={save("extra")} multiline disabled={locked} />
          </F>
        </Grid>
        {!locked && (
          <select
            value=""
            onChange={(e) => e.target.value && addField(e.target.value)}
            className="mt-3 rounded-md border bg-background px-1.5 py-0.5 text-xs text-muted-foreground"
          >
            <option value="">+ 添加字段…</option>
            {ADDABLE_FIELDS.filter((f) => !d.otherFields.some((o) => o.name === f)).map((f) => (
              <option key={f} value={f}>
                {fieldLabel(f)}
              </option>
            ))}
          </select>
        )}
      </details>
      <p className="mt-5 text-[11px] text-muted-foreground">
        加入于 {formatTime(d.createdAt)} · 修改于 {formatTime(d.updatedAt)}
      </p>
      {footer}
    </>
  );
}

function ChildRow({ a, onSelect }: { a: ChildAttachment; onSelect: (s: Selection) => void }) {
  const missing = a.status === "missing";
  return (
    <li className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-muted">
      <button onClick={() => onSelect({ kind: "attachment", id: a.id })} className="flex min-w-0 flex-1 items-center gap-2 text-left">
        <FileText size={14} className="shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">{a.title || contentTypeLabel(a.contentType)}</span>
        {a.annotationCount > 0 && <span className="shrink-0 text-xs text-muted-foreground tabular-nums" title="标注数">✎{a.annotationCount}</span>}
        {missing ? (
          <span className="shrink-0 rounded bg-destructive/10 px-1 text-[11px] text-destructive">文件缺失</span>
        ) : (
          a.size !== null && <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatSize(a.size)}</span>
        )}
      </button>
      {a.readable ? <ReadButton id={a.id} /> : !missing && <OpenButton id={a.id} label={a.kind === "linked_url" ? "网页" : "打开"} />}
    </li>
  );
}

function Attachment({ d, onSelect, onGone }: { d: AttachmentDetail; onSelect: (s: Selection) => void; onGone: () => void }) {
  const mutate = useMutate();
  const target: Target = { kind: "attachment", id: d.id };
  const { banner, footer } = useTrashActions(target, d.deletedAt, onGone);
  const missing = d.status === "missing";
  const locked = d.deletedAt !== null;
  return (
    <>
      {banner}
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <EditableText
            value={d.title || d.originalName}
            onSave={(v) => mutate(() => api.attachmentsRename(d.id, v))}
            disabled={locked}
            className="text-base leading-snug font-semibold"
          />
        </div>
        {d.readable && <ReadButton id={d.id} primary />}
        {!missing && <OpenButton id={d.id} label={d.kind === "linked_url" ? "网页" : d.readable ? "系统程序" : "打开"} />}
      </div>
      <ParentLink parent={d.parent} onSelect={onSelect} />
      {missing && <p className="mt-3 rounded-md bg-destructive/10 px-2 py-1.5 text-xs text-destructive">文件缺失：导入时没有找到这个文件。条目照常保留；需要的话在 Zotero 里下载后重新导入。</p>}
      <Grid>
        <F label="类型">{contentTypeLabel(d.contentType)}</F>
        <F label="存放">{attachmentKindLabel(d.kind)}</F>
        {d.originalName && <F label="原文件名">{d.originalName}</F>}
        {d.size !== null && <F label="大小">{formatSize(d.size)}</F>}
        {d.originalPath && <F label="原位置">{d.originalPath}</F>}
        {d.url && <F label="网址">{d.url}</F>}
        {d.annotationCount > 0 && <F label="标注">{d.annotationCount} 处</F>}
        {!d.parent && d.kind !== "embedded_image" && (
          <F label="集合">
            <CollectionsEditor collections={d.collections} target={target} disabled={locked} />
          </F>
        )}
        <F label="加入时间">{formatTime(d.createdAt)}</F>
      </Grid>
      <TagEditor tags={d.tags} target={target} disabled={locked} />
      {d.note && (
        <Section title="附件笔记">
          <p className="leading-relaxed whitespace-pre-wrap">{d.note}</p>
        </Section>
      )}
      {footer}
    </>
  );
}

function Note({ d, onSelect, onGone }: { d: NoteDetail; onSelect: (s: Selection) => void; onGone: () => void }) {
  const target: Target = { kind: "note", id: d.id };
  const { banner, footer } = useTrashActions(target, d.deletedAt, onGone);
  const locked = d.deletedAt !== null;
  return (
    <>
      {banner}
      <h2 className="text-base leading-snug font-semibold">{d.title || "（无标题笔记）"}</h2>
      <ParentLink parent={d.parent} onSelect={onSelect} />
      <Grid>
        {!d.parent && (
          <F label="集合">
            <CollectionsEditor collections={d.collections} target={target} disabled={locked} />
          </F>
        )}
        {d.imageCount > 0 && <F label="图片">{d.imageCount} 张（笔记编辑器在以后的版本）</F>}
        <F label="修改时间">{formatTime(d.updatedAt)}</F>
      </Grid>
      <TagEditor tags={d.tags} target={target} disabled={locked} />
      <Section title="内容（只读）">
        <p className="leading-relaxed whitespace-pre-wrap">{d.text || <span className="text-muted-foreground">（空）</span>}</p>
      </Section>
      {footer}
    </>
  );
}

function Collection({ d, onGone }: { d: CollectionDetail; onGone: () => void }) {
  const { banner } = useTrashActions({ kind: "collection", id: d.id }, d.deletedAt, onGone);
  return (
    <>
      {banner}
      <h2 className="flex items-center gap-2 text-base leading-snug font-semibold">
        <Folder size={16} className="text-muted-foreground" />
        {d.name}
      </h2>
      <Grid>
        {d.parentName && <F label="上级集合">{d.parentName}</F>}
        {d.children > 0 && <F label="下级集合">{d.children} 个（随它一起在回收站）</F>}
        <F label="直接成员">{d.members} 条</F>
      </Grid>
      <p className="mt-4 text-xs text-muted-foreground">集合在回收站时，里面的文献仍在「全部文献」里。恢复集合时，在回收站里的上级集合一起恢复。</p>
    </>
  );
}

// ---------------- 批量 ----------------

const BATCH_FIELDS: { field: string; label: string; options?: string[] }[] = [
  { field: "itemType", label: "类型", options: ITEM_TYPE_OPTIONS },
  { field: "language", label: "语言" },
  { field: "date", label: "日期" },
  { field: "container", label: "期刊 / 书名" },
  { field: "publisher", label: "出版者" },
  { field: "place", label: "出版地" },
];

function BatchPanel({ selection, onSelection, scope }: Props) {
  const ui = useUi();
  const mutate = useMutate();
  const metadata = useMetadata();
  const [tag, setTag] = useState("");
  const [field, setField] = useState(BATCH_FIELDS[0].field);
  const [value, setValue] = useState("");
  const trash = scope.kind === "trash";
  const items = selection.filter((s) => s.kind === "item");
  const taggable = selection.filter((s) => s.kind !== "collection");
  const count = (k: string) => selection.filter((s) => s.kind === k).length;
  const parts = [["item", "文献"], ["attachment", "附件"], ["note", "笔记"], ["collection", "集合"]]
    .map(([k, l]) => (count(k) ? `${l} ${count(k)}` : ""))
    .filter(Boolean)
    .join("、");
  const def = BATCH_FIELDS.find((f) => f.field === field)!;

  if (trash) {
    return (
      <div className="p-5">
        <p className="font-medium">已选 {selection.length} 项</p>
        <p className="mt-1 text-xs text-muted-foreground">{parts}</p>
        <div className="mt-4 flex gap-2">
          <SmallButton onClick={() => mutate(() => api.trashRestore(targetsOf(selection)), "已恢复").then((r) => r !== undefined && onSelection([]))}>
            <RotateCcw size={12} />
            恢复
          </SmallButton>
          <SmallButton
            danger
            onClick={async () => {
              const ok = await ui.confirm({ title: `永久删除 ${selection.length} 项`, message: DELETE_FOREVER_NOTE, confirm: "永久删除", danger: true });
              if (ok && (await mutate(() => api.trashDelete(targetsOf(selection)), "已永久删除"))) onSelection([]);
            }}
          >
            永久删除
          </SmallButton>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-5">
      <p className="font-medium">已选 {selection.length} 项</p>
      <p className="mt-1 text-xs text-muted-foreground">{parts}</p>

      {items.length >= 2 && <MergeBox ids={items.map((s) => s.id)} duplicates={scope.kind === "duplicates"} onDone={(master) => onSelection([{ kind: "item", id: master }])} />}

      <Section title="标签">
        <div className="flex gap-1.5">
          <TagInput
            value={tag}
            onChange={setTag}
            onEnter={() => {}}
            boxClassName="flex-1"
            placeholder="标签名"
            className="selectable w-full min-w-0 rounded-md border bg-background px-2 py-0.5 text-xs outline-none focus:border-ring"
          />
          <SmallButton onClick={() => tag.trim() && mutate(() => api.tagsAdd(targetsOf(taggable), tag), `已给 ${taggable.length} 项加上「${tag.trim()}」`)}>添加</SmallButton>
          <SmallButton onClick={() => tag.trim() && mutate(() => api.tagsRemove(targetsOf(taggable), tag.trim()), `已去掉「${tag.trim()}」`)}>去掉</SmallButton>
        </div>
      </Section>

      <Section title="集合">
        <div className="flex flex-wrap gap-1.5">
          <SmallButton
            onClick={async () => {
              const cid = await ui.pickCollection({ title: `把 ${selection.length} 项添加到集合`, confirm: "添加" });
              if (cid) await mutate(() => api.collectionsAdd(cid, targetsOf(selection)), (n) => `已添加 ${n} 项`);
            }}
          >
            添加到集合…
          </SmallButton>
          {scope.kind === "collection" && (
            <SmallButton onClick={() => mutate(() => api.collectionsRemove(scope.id, targetsOf(selection)), (n) => `已从「${scope.name}」移出 ${n} 项`)}>
              从「{scope.name}」移出
            </SmallButton>
          )}
        </div>
      </Section>

      {items.length > 0 && (
        <Section title="联网补全">
          <p className="mb-1.5 text-xs text-muted-foreground">按 DOI / ISBN 查，只补空着的字段；和现有不同的放进「待确认」。没有 DOI / ISBN 的跳过。</p>
          <SmallButton onClick={() => void metadata.lookup(items.map((s) => s.id))}>联网补全 {items.length} 条</SmallButton>
        </Section>
      )}

      {items.length > 0 && (
        <Section title={`修改字段（${items.length} 条文献）`}>
          <div className="flex gap-1.5">
            <select
              value={field}
              onChange={(e) => {
                setField(e.target.value);
                setValue("");
              }}
              className="rounded-md border bg-background px-1 py-0.5 text-xs"
            >
              {BATCH_FIELDS.map((f) => (
                <option key={f.field} value={f.field}>
                  {f.label}
                </option>
              ))}
            </select>
            {def.options ? (
              <select value={value} onChange={(e) => setValue(e.target.value)} className="min-w-0 flex-1 rounded-md border bg-background px-1 py-0.5 text-xs">
                <option value="">选择…</option>
                {def.options.map((o) => (
                  <option key={o} value={o}>
                    {itemTypeLabel(o)}
                  </option>
                ))}
              </select>
            ) : (
              <input
                value={value}
                onChange={(e) => setValue(e.target.value)}
                placeholder="新值（空 = 清空）"
                className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-0.5 text-xs outline-none focus:border-ring"
              />
            )}
            <SmallButton
              onClick={async () => {
                if (def.options && !value) return;
                const ok = await ui.confirm({ title: `把 ${items.length} 条文献的「${def.label}」改成「${def.options ? itemTypeLabel(value) : value || "（空）"}」？` });
                if (ok) await mutate(() => api.itemsSetField(items.map((s) => s.id), field, value), "已修改");
              }}
            >
              应用
            </SmallButton>
          </div>
        </Section>
      )}

      <div className="mt-8 border-t pt-3">
        <SmallButton danger onClick={() => mutate(() => api.trashMove(targetsOf(selection)), `${selection.length} 项已移到回收站`).then((r) => r !== undefined && onSelection([]))}>
          <Trash2 size={12} />
          移到回收站
        </SmallButton>
      </div>
    </div>
  );
}

/** 合并：选一条保留，其余的附件、笔记、标签、集合并过去，缺的字段补上，然后进回收站 */
function MergeBox({ ids, duplicates, onDone }: { ids: number[]; duplicates: boolean; onDone: (master: number) => void }) {
  const ui = useUi();
  const mutate = useMutate();
  const details = useQueries({ queries: ids.map((id) => ({ queryKey: ["item", id], queryFn: () => api.itemsGet(id) })) });
  const [master, setMaster] = useState(ids[0]);
  const loaded = details.map((d) => d.data).filter((d): d is ItemDetail => !!d);
  return (
    <Section title="合并为一条">
      <ul className="space-y-1 rounded-md border p-1.5">
        {loaded.map((d) => (
          <li key={d.id}>
            <label className="flex items-start gap-2 rounded px-1 py-0.5 hover:bg-muted">
              <input type="radio" checked={master === d.id} onChange={() => setMaster(d.id)} className="mt-1" />
              <span className="min-w-0 text-xs">
                <span className="block truncate font-medium">{d.title || "（无标题）"}</span>
                <span className="text-muted-foreground">
                  {[d.year, itemTypeLabel(d.itemType), `${d.attachments.length} 个附件`, d.doi].filter(Boolean).join(" · ")}
                </span>
              </span>
            </label>
          </li>
        ))}
      </ul>
      <p className="mt-1 text-[11px] text-muted-foreground">保留选中的这条；其余的附件、笔记、标签、集合并过来，缺的字段补上（不覆盖已有的），然后移进回收站。</p>
      <div className="mt-2 flex gap-1.5">
        <SmallButton
          onClick={async () => {
            const ok = await ui.confirm({ title: `把 ${ids.length} 条合并成一条？`, message: "被合并的条目会进回收站，需要时可以恢复。" });
            if (ok && (await mutate(() => api.itemsMerge(master, ids.filter((i) => i !== master)), "已合并")) !== undefined) onDone(master);
          }}
        >
          合并
        </SmallButton>
        {duplicates && <SmallButton onClick={() => mutate(() => api.duplicatesDismiss(ids), "已标记，不再提示")}>不是重复</SmallButton>}
      </div>
    </Section>
  );
}
