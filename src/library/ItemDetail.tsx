import { useState } from "react";
import { useMutation, useQueries, useQuery } from "@tanstack/react-query";
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
import { Select } from "@/shared/controls";
import { Mark } from "@/shared/marks";
import { useRead } from "@/reader/useRead";
import { MetadataBox } from "@/metadata/MetadataBox";
import { ParseDialog } from "@/concept/ParseDialog";
import { useMetadata } from "@/metadata/MetadataProvider";
import { targetsOf, type Selection } from "./ItemList";
import type { Scope } from "./scope";
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
    <div className="pd-wrap">
      {selection.length === 0 ? (
        <p className="pd-none">选择一条文献查看详情</p>
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
    </div>
  );
}

function Load<T>({ query, render }: { query: { queryKey: unknown[]; queryFn: () => Promise<T> }; render: (d: T) => React.ReactNode }) {
  const q = useQuery(query);
  if (q.error) return <p className="selectable pd-err">{String(q.error)}</p>;
  return q.data ? <div className="selectable pd">{render(q.data)}</div> : null;
}

function Grid({ children }: { children: React.ReactNode }) {
  return <dl className="pd-grid">{children}</dl>;
}

function F({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="contents">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function Section({ title, children, action }: { title: string; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <section className="pd-sec">
      <header>
        <h3>{title}</h3>
        {action}
      </header>
      {children}
    </section>
  );
}

function SmallButton({ onClick, children, danger, title }: { onClick: () => void; children: React.ReactNode; danger?: boolean; title?: string }) {
  return (
    <button onClick={onClick} title={title} className={`pd-btn ${danger ? "is-danger" : ""}`}>
      {children}
    </button>
  );
}

/** 在回收站时的提示条 + 恢复 / 永久删除；不在时返回「移到回收站」按钮放在底部 */
function useTrashActions(target: Target, deletedAt: string | null, onGone: () => void) {
  const ui = useUi();
  const mutate = useMutate();
  const banner = deletedAt ? (
    <div className="pd-banner">
      <span>在回收站（{formatTime(deletedAt)} 删除）</span>
      <SmallButton onClick={() => mutate(() => api.trashRestore([target]), "已恢复")}>恢复</SmallButton>
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
    <div className="pd-foot">
      <button className="pith-link is-danger" onClick={() => mutate(() => api.trashMove([target]), "已移到回收站").then((r) => r !== undefined && onGone())}>
        移到回收站
      </button>
    </div>
  );
  return { banner, footer };
}

function ParentLink({ parent, onSelect }: { parent: Parent | null; onSelect: (s: Selection) => void }) {
  if (!parent) return null;
  return (
    <button onClick={() => onSelect({ kind: parent.kind, id: parent.id })} className="pd-parent pith-link">
      所属{parent.kind === "item" ? "文献" : "笔记"}：{parent.title || "（无标题）"}
    </button>
  );
}

/** EPUB / PDF 附件：解析成文本（v0.4 概念研究）；解析过的显示「概念页」和「重新解析」 */
function ParseButton({ id, title, contentType, status }: { id: number; title: string; contentType: string | null; status: string }) {
  const [open, setOpen] = useState(false);
  const ok = status !== "missing" && (contentType === "application/epub+zip" || contentType === "application/pdf");
  const parsed = useQuery({ queryKey: ["conceptAttachment", id], queryFn: () => api.conceptAttachmentText(id), enabled: ok, staleTime: 0 });
  if (!ok) return null;
  const go = (uid: string) => window.dispatchEvent(new CustomEvent("pith:open-concept", { detail: uid }));
  return (
    <>
      {parsed.data ? (
        <>
          <button onClick={() => go(parsed.data!)} title="这个附件解析过了：在「概念」里读" className="pd-btn">
            概念页
          </button>
          <button onClick={() => setOpen(true)} title="再解析一次：替换文本，旧卡片按引文重新找锚点" className="pd-btn">
            重新解析
          </button>
        </>
      ) : (
        <button onClick={() => setOpen(true)} title="解析成带段落号的文本，在「概念」里读和分析" className="pd-btn">
          解析成文本
        </button>
      )}
      {open && <ParseDialog attachmentId={id} title={title} onClose={() => setOpen(false)} onSaved={go} />}
    </>
  );
}

function ReadButton({ id, primary }: { id: number; primary?: boolean }) {
  const { read } = useRead();
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        read(id);
      }}
      title="在阅读器里打开，可以高亮、摘录"
      className={`pd-btn ${primary ? "is-primary" : ""}`}
    >
      阅读
    </button>
  );
}

function OpenButton({ id, label = "打开" }: { id: number; label?: string }) {
  const open = useMutation({ mutationFn: () => api.attachmentsOpen(id) });
  return (
    <span className="pd-open">
      <button
        onClick={(e) => {
          e.stopPropagation();
          open.mutate();
        }}
        className="pd-btn"
      >
        {label}
      </button>
      {open.error && <span className="pd-err-s">{String(open.error)}</span>}
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
    <div className="pd-tags">
      {tags.map((t) => (
        <span key={t} className="pith-tag">
          {t}
          {!disabled && (
            <button onClick={() => mutate(() => api.tagsRemove([target], t))} className="pith-tag-x" title="去掉这个标签" aria-label={`去掉标签 ${t}`}>
              ×
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
          placeholder="＋ 标签"
          className="selectable pith-tag-add"
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
    <span className="pd-tags is-inline">
      {collections.length === 0 && <span className="pd-dim">未分类</span>}
      {collections.map((c) => (
        <span key={c.id} className="pith-tag is-col">
          {c.name}
          {!disabled && (
            <button onClick={() => mutate(() => api.collectionsRemove(c.id, [target]))} className="pith-tag-x" title="移出这个集合" aria-label={`移出集合 ${c.name}`}>
              ×
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
          className="pith-tag-add is-btn"
          title="添加到集合"
        >
          ＋
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
      <div className="pd-creators">
        <p>
          {d.creators.length === 0 && <span className="pd-dim">没有作者</span>}
          {(showAll ? d.creators : d.creators.slice(0, AUTHORS_SHOWN)).map((c, i) => (
            <span key={i}>
              {i > 0 && "；"}
              {c.name}
              {c.role !== "author" && <small>（{roleLabel(c.role)}）</small>}
            </span>
          ))}
          {d.creators.length > AUTHORS_SHOWN && (
            <button onClick={() => setShowAll(!showAll)} className="pith-link pd-more-link">
              {showAll ? "收起" : `… 等 ${d.creators.length} 人，展开`}
            </button>
          )}
        </p>
        {!disabled && (
          <button onClick={start} className="pith-link">
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
  const input = "selectable pith-field pith-field--s";
  return (
    <div className="pd-box pd-crows">
      {rows.map((r, i) => (
        <div key={i} className="pd-crow">
          <Select variant="field" className="pd-role" value={r.role} onChange={(v) => set(i, { role: v })}>
            {roles.map((x) => (
              <option key={x} value={x}>
                {roleLabel(x)}
              </option>
            ))}
          </Select>
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
            className="pd-btn pd-btn--s"
          >
            {r.single ? "两栏" : "单栏"}
          </button>
          <button onClick={() => move(i, -1)} className="pd-icon" title="上移" aria-label="上移">
            ↑
          </button>
          <button onClick={() => move(i, 1)} className="pd-icon" title="下移" aria-label="下移">
            ↓
          </button>
          <button onClick={() => setRows(rows.filter((_, j) => j !== i))} className="pd-icon is-danger" title="删掉这位" aria-label="删掉这位">
            ×
          </button>
        </div>
      ))}
      <div className="pd-box-acts">
        <button onClick={() => setRows([...rows, { role: "author", single: false, family: "", given: "", literal: "" }])} className="pith-link">
          ＋ 添加作者
        </button>
        <span className="flex-1" />
        <button onClick={() => setRows(null)} className="pith-button pith-button--quiet pith-button--s">
          算了
        </button>
        <button onClick={save} className="pith-button pith-button--s">
          存好
        </button>
      </div>
    </div>
  );
}

/** R1：面板头上一个「阅读」（有能读的 PDF 时） */
function ReadItem({ d }: { d: ItemDetail }) {
  const { openItem } = useRead();
  if (!d.attachments.some((a) => a.readable)) return null;
  return (
    <p className="pd-read">
      <button className="pith-button pith-sweep" onClick={() => void openItem(d.id)}>
        阅读 <i className="ar not-italic">→</i>
      </button>
    </p>
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
      <p className="panel-k">
        {itemTypeLabel(d.itemType)}
        {d.year ? ` · ${d.year}` : ""}
      </p>
      <EditableText value={d.title} onSave={save("title")} placeholder="（无标题，点击填写）" disabled={locked} className="panel-title" />
      <CreatorsEditor d={d} disabled={locked} />
      <ReadItem d={d} />
      <Grid>
        <F label="类型">
          <Select value={d.itemType} disabled={locked} onChange={(v) => mutate(() => api.itemsUpdate(d.id, { itemType: v }))} className="pd-inline-select" menuClassName="pith-menu--tall">
            {types.map((t) => (
              <option key={t} value={t}>
                {itemTypeLabel(t)}
              </option>
            ))}
          </Select>
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
              className="pith-link"
            >
              ＋ 添加附件
            </button>
          )
        }
      >
        {d.attachments.length === 0 ? (
          <p className="pd-dim">没有附件</p>
        ) : (
          <ul className="pd-files">
            {d.attachments.map((a) => (
              <ChildRow key={a.id} a={a} onSelect={onSelect} />
            ))}
          </ul>
        )}
      </Section>
      {d.notes.length > 0 && (
        <Section title={`笔记（${d.notes.length}）`}>
          <ul className="pd-files">
            {d.notes.map((n) => (
              <li key={n.id}>
                <button onClick={() => onSelect({ kind: "note", id: n.id })} className="pd-note pith-row">
                  <Mark path="types/note" className="tyi" />
                  <span>
                    <b>{n.title || "（无标题笔记）"}</b>
                    {n.preview && <small>{n.preview}</small>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
      <Section title="摘要">
        <EditableText value={d.abstract} onSave={save("abstractNote")} multiline placeholder="没有摘要，点一下写" disabled={locked} className="pd-abstract" />
      </Section>
      <details className="pd-more">
        <summary>其他字段 <span className="num">{d.otherFields.length + (d.extra ? 1 : 0)}</span></summary>
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
          <Select value="" onChange={(v) => v && addField(v)} placeholder="＋ 添加字段" className="pd-add-field" menuClassName="pith-menu--tall" render={() => "＋ 添加字段"}>
            {ADDABLE_FIELDS.filter((f) => !d.otherFields.some((o) => o.name === f)).map((f) => (
              <option key={f} value={f}>
                {fieldLabel(f)}
              </option>
            ))}
          </Select>
        )}
      </details>
      <p className="pd-time">
        加入于 {formatTime(d.createdAt)} · 修改于 {formatTime(d.updatedAt)}
      </p>
      {footer}
    </>
  );
}

function ChildRow({ a, onSelect }: { a: ChildAttachment; onSelect: (s: Selection) => void }) {
  const missing = a.status === "missing";
  return (
    <li className="pd-file">
      <button onClick={() => onSelect({ kind: "attachment", id: a.id })} className="pd-file-name pith-row">
        <Mark path="types/file" className="tyi" />
        <b>{a.title || contentTypeLabel(a.contentType)}</b>
        {a.annotationCount > 0 && <small title="标注数">{a.annotationCount} 处标注</small>}
        {missing ? <small className="is-warn">文件缺失</small> : a.size !== null && <small className="num">{formatSize(a.size)}</small>}
      </button>
      <span className="pd-file-acts">
      <ParseButton id={a.id} title={a.title || contentTypeLabel(a.contentType)} contentType={a.contentType} status={a.status} />
      {a.readable ? <ReadButton id={a.id} /> : !missing && <OpenButton id={a.id} label={a.kind === "linked_url" ? "网页" : "打开"} />}
      </span>
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
      <p className="panel-k">附件 · {contentTypeLabel(d.contentType)}</p>
      <EditableText value={d.title || d.originalName} onSave={(v) => mutate(() => api.attachmentsRename(d.id, v))} disabled={locked} className="panel-title" />
      <div className="pd-row-acts">
        <ParseButton id={d.id} title={d.title || d.originalName || ""} contentType={d.contentType} status={d.status} />
        {d.readable && <ReadButton id={d.id} primary />}
        {!missing && <OpenButton id={d.id} label={d.kind === "linked_url" ? "网页" : d.readable ? "系统程序" : "打开"} />}
      </div>
      <ParentLink parent={d.parent} onSelect={onSelect} />
      {missing && <p className="pd-banner">文件缺失：导入时没有找到这个文件。条目照常保留；需要的话在 Zotero 里下载后重新导入。</p>}
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
          <p className="pd-text">{d.note}</p>
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
      <p className="panel-k">笔记</p>
      <h2 className="panel-title">{d.title || "（无标题笔记）"}</h2>
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
        <p className="pd-text">{d.text || <span className="pd-dim">（空）</span>}</p>
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
      <p className="panel-k">集合</p>
      <h2 className="panel-title">{d.name}</h2>
      <Grid>
        {d.parentName && <F label="上级集合">{d.parentName}</F>}
        {d.children > 0 && <F label="下级集合">{d.children} 个（随它一起在回收站）</F>}
        <F label="直接成员">{d.members} 条</F>
      </Grid>
      <p className="pd-note-s">集合在回收站时，里面的文献仍在「全部文献」里。恢复集合时，在回收站里的上级集合一起恢复。</p>
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
      <div className="pd">
        <p className="panel-k">回收站</p>
        <h2 className="panel-title">已选 {selection.length} 项</h2>
        <p className="pd-sub">{parts}</p>
        <div className="pd-row-acts">
          <SmallButton onClick={() => mutate(() => api.trashRestore(targetsOf(selection)), "已恢复").then((r) => r !== undefined && onSelection([]))}>恢复</SmallButton>
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
    <div className="pd">
      <p className="panel-k">一起改</p>
      <h2 className="panel-title">已选 {selection.length} 项</h2>
      <p className="pd-sub">{parts}</p>

      {items.length >= 2 && <MergeBox ids={items.map((s) => s.id)} duplicates={scope.kind === "duplicates"} onDone={(master) => onSelection([{ kind: "item", id: master }])} />}

      <Section title="标签">
        <div className="pd-line">
          <TagInput value={tag} onChange={setTag} onEnter={() => {}} boxClassName="flex-1" placeholder="标签名" className="selectable pith-field pith-field--s w-full" />
          <SmallButton onClick={() => tag.trim() && mutate(() => api.tagsAdd(targetsOf(taggable), tag), `已给 ${taggable.length} 项加上「${tag.trim()}」`)}>添加</SmallButton>
          <SmallButton onClick={() => tag.trim() && mutate(() => api.tagsRemove(targetsOf(taggable), tag.trim()), `已去掉「${tag.trim()}」`)}>去掉</SmallButton>
        </div>
      </Section>

      <Section title="集合">
        <div className="pd-line">
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
          <p className="pd-note-s">按 DOI / ISBN 查，只补空着的字段；和现有不同的放进「待确认」。没有 DOI / ISBN 的跳过。</p>
          <SmallButton onClick={() => void metadata.lookup(items.map((s) => s.id))}>联网补全 {items.length} 条</SmallButton>
        </Section>
      )}

      {items.length > 0 && (
        <Section title={`修改字段（${items.length} 条文献）`}>
          <div className="pd-line">
            <Select
              variant="field"
              value={field}
              onChange={(v) => {
                setField(v);
                setValue("");
              }}
            >
              {BATCH_FIELDS.map((f) => (
                <option key={f.field} value={f.field}>
                  {f.label}
                </option>
              ))}
            </Select>
            {def.options ? (
              <Select variant="field" className="flex-1" value={value} onChange={setValue} placeholder="选择…" menuClassName="pith-menu--tall">
                {def.options.map((o) => (
                  <option key={o} value={o}>
                    {itemTypeLabel(o)}
                  </option>
                ))}
              </Select>
            ) : (
              <input value={value} onChange={(e) => setValue(e.target.value)} placeholder="新值（空 = 清空）" className="selectable pith-field pith-field--s flex-1" />
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

      <div className="pd-foot">
        <button className="pith-link is-danger" onClick={() => mutate(() => api.trashMove(targetsOf(selection)), `${selection.length} 项已移到回收站`).then((r) => r !== undefined && onSelection([]))}>
          移到回收站
        </button>
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
      <ul className="pd-box pd-merge">
        {loaded.map((d) => (
          <li key={d.id}>
            <label>
              <input type="radio" className="pith-radio" checked={master === d.id} onChange={() => setMaster(d.id)} />
              <span>
                <b>{d.title || "（无标题）"}</b>
                <small>
                  {[d.year, itemTypeLabel(d.itemType), `${d.attachments.length} 个附件`, d.doi].filter(Boolean).join(" · ")}
                </small>
              </span>
            </label>
          </li>
        ))}
      </ul>
      <p className="pd-note-s">保留选中的这条；其余的附件、笔记、标签、集合并过来，缺的字段补上（不覆盖已有的），然后移进回收站。</p>
      <div className="pd-line">
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
