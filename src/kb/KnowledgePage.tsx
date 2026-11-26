// 知识库（R1，1.0.3 借文献库的两栏）：第一栏左边是一排知识库，右边是种类和搜索；第二栏是生长阶段；
// 下面是卡片墙（瀑布流，shared/masonry），十种记录各一种样子；标题的字重跟着生长阶段（字会长）。
// 点一张卡，右边滑出面板：正文（书是原文：一章一章读，能对照译文）、从哪来、在哪几个库（一份材料能进几个库），
// 能改、能照料（换阶段）、能打开原处。从对话的页边注点「知识库里这一段」过来时，读到那一段、它亮着。
// 「种进来」：解析好的书（全文）、做完的调查（调查报告）、概念研究的卡片（概念卡），或者自己写一条笔记、记一条摘抄。
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { api, type KnowledgeBase, type Material, type MaterialKind } from "@/shared/ipc";
import { Modal, useUi } from "@/shared/ui";
import { Select, useWheelX } from "@/shared/controls";
import { useMasonry } from "@/shared/masonry";
import { KindIcon, KIND_NAMES, Mark, Stage, STAGE_NAMES } from "@/shared/marks";
import { useNav, dateOf } from "@/app/nav";

/** 书封的颜色：从方案的几种颜色里轮着取（不写死色值） */
const COVERS = ["var(--color-accent-text)", "var(--color-second)", "var(--color-app-ink)", "color-mix(in srgb, var(--color-accent-text) 60%, var(--color-second))", "color-mix(in srgb, var(--color-app-ink) 70%, var(--color-accent))"];

export function KnowledgePage() {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const kbs = useQuery({ queryKey: ["kbs"], queryFn: api.kbList });
  const list = kbs.data ?? [];
  const sel = list.find((k) => k.uid === nav.kb.uid) ?? list[0] ?? null;
  const mats = useQuery({ queryKey: ["materials", sel?.uid ?? ""], queryFn: () => api.kbMaterials(sel!.uid), enabled: !!sel });
  const [open, setOpen] = useState<string | null>(nav.kb.material);
  const [planting, setPlanting] = useState(false);
  const tabsRef = useRef<HTMLDivElement>(null);
  useWheelX(tabsRef);
  const [kind, setKind] = useState("");
  const [stage, setStage] = useState(-1);
  const [q, setQ] = useState("");
  useEffect(() => setOpen(nav.kb.material), [nav.kb.material]);
  useEffect(() => {
    setKind("");
    setStage(-1);
    setQ("");
  }, [sel?.uid]);
  const current = mats.data?.find((m) => m.uid === open) ?? null;
  const all = mats.data ?? [];
  const term = q.trim().toLowerCase();
  const shown = all.filter((m) => (!kind || m.kind === kind) && (stage < 0 || m.stage === stage) && (!term || `${m.title} ${m.body} ${m.sub}`.toLowerCase().includes(term)));
  const kinds = (Object.keys(KIND_NAMES) as MaterialKind[]).filter((k) => all.some((m) => m.kind === k));
  const wallRef = useRef<HTMLDivElement>(null);
  const rulerRef = useRef<HTMLElement>(null);
  useMasonry(wallRef, shown.map((m) => m.uid).join(","), rulerRef);

  const create = async () => {
    const name = await ui.prompt({ title: "新建知识库", label: "名字", placeholder: "例如：读《国富论》、毕业论文", confirm: "建好" });
    if (!name) return;
    try {
      const uid = await api.kbCreate(name, "");
      await qc.invalidateQueries({ queryKey: ["kbs"] });
      nav.openKb(uid);
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };

  return (
    <div className={current ? "has-panel" : ""}>
      <main className="page page-enter">
        <i className="page-ruler" ref={rulerRef} aria-hidden="true" />
        <p className="page-count num">{sel ? (shown.length === all.length ? `${sel.count} 份` : `${shown.length} / ${all.length} 份`) : " "}</p>
        <div className="lib-head">
          <h1 className="page-h1 truncate">{sel?.name ?? "知识库"}</h1>
          <div className="lib-acts">
            {sel && (
              <button className="pith-link" onClick={() => setPlanting(true)}>
                ＋ 种进来
              </button>
            )}
          </div>
        </div>
        {sel?.note && <p className="page-line">{sel.note}</p>}
        <div className="lib-bar">
          <div className="tabs" ref={tabsRef}>
            {list.map((k) => (
              <button key={k.uid} className={k.uid === sel?.uid ? "is-on" : ""} onClick={() => nav.openKb(k.uid)}>
                {k.name}
                <span className="num">{k.count}</span>
              </button>
            ))}
            <button className="tabs-add" onClick={create}>
              ＋ 知识库
            </button>
          </div>
          {sel && all.length > 0 && (
            <div className="lib-find-row">
              <Select value={kind} onChange={setKind} title="按种类筛选">
                <option value="">全部种类</option>
                {kinds.map((k) => (
                  <option key={k} value={k}>
                    {KIND_NAMES[k]}
                  </option>
                ))}
              </Select>
              <label className="pith-search">
                <Mark path="icons/search" />
                <input value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.key === "Escape" && setQ("")} placeholder="搜索标题、正文" className="selectable" />
                {q && (
                  <button onClick={() => setQ("")} aria-label="清空" className="pith-search-x">
                    ×
                  </button>
                )}
              </label>
            </div>
          )}
        </div>
        {sel && all.length > 0 && (
          <div className="lib-cols">
            <div className="pith-strip" role="toolbar" aria-label="生长阶段">
              <button className={`pith-strip-item pith-ink ${stage < 0 ? "is-on" : ""}`} onClick={() => setStage(-1)}>
                全部
              </button>
              {STAGE_NAMES.map((n, i) => {
                const c = all.filter((m) => m.stage === i).length;
                return (
                  <button key={n} className={`pith-strip-item pith-ink kb-stage ${stage === i ? "is-on" : ""}`} onClick={() => setStage(stage === i ? -1 : i)} disabled={c === 0}>
                    <Stage stage={i} />
                    {n}
                    <em className="num">{c}</em>
                  </button>
                );
              })}
            </div>
          </div>
        )}
        <div className="page-body">
          {kbs.data && list.length === 0 ? (
            <div className="empty">
              <p>还没有知识库。知识库是文献库和对话之间的一层：读过的书、做完的调查、对话里存下的回答都种在这里，回答时 Pith 在你勾的库里找。</p>
              <button className="pith-button pith-sweep" onClick={create}>
                建第一个知识库 <i className="ar not-italic">→</i>
              </button>
            </div>
          ) : mats.data && mats.data.length === 0 ? (
            <div className="empty">
              <p>「{sel?.name}」还是空的。把解析好的书、做完的调查种进来，或者在对话里把回答「存进」它。</p>
              <button className="pith-button pith-sweep" onClick={() => setPlanting(true)}>
                种进来 <i className="ar not-italic">→</i>
              </button>
            </div>
          ) : mats.data && shown.length === 0 ? (
            <div className="empty">
              <p>没有符合条件的</p>
              <p className="empty-sub">换个搜索词，或者去掉种类、阶段的筛选。</p>
            </div>
          ) : (
            <div className="wall" ref={wallRef}>
              {shown.map((m, i) => (
                <Card key={m.uid} m={m} i={i} on={m.uid === open} onClick={() => setOpen(m.uid === open ? null : m.uid)} />
              ))}
            </div>
          )}
        </div>
      </main>
      <MaterialPanel m={current} kb={sel} kbs={list} focus={current && current.uid === nav.kb.material ? nav.kb.passage : null} onClose={() => setOpen(null)} />
      {planting && sel && <Plant kb={sel} onClose={() => setPlanting(false)} />}
    </div>
  );
}

/** 一张卡片：按种类换样子 */
function Card({ m, i, on, onClick }: { m: Material; i: number; on: boolean; onClick: () => void }) {
  const lbl = (
    <span className="lbl">
      <KindIcon kind={m.kind} />
      {KIND_NAMES[m.kind]}
    </span>
  );
  const tt = <span className={`tt w${m.stage}`}>{m.title}</span>;
  const mt = (
    <span className="mt">
      <Stage stage={m.stage} />
      {m.sub ? `${m.sub} · ` : ""}
      {dateOf(m.tendedAt)}
    </span>
  );
  const tx = m.body ? <span className="tx">{clip(m.body, 120)}</span> : null;
  let inner: React.ReactNode;
  switch (m.kind) {
    case "full":
      inner = (
        <>
          <span className="cv" style={{ "--cover": COVERS[i % COVERS.length] } as React.CSSProperties}>
            <b>{m.title}</b>
            <span>{m.sub.split(" · ")[0]}</span>
          </span>
          <span className="bx">
            {lbl}
            <span className="tx">{m.sub}</span>
            <span className="chip">带段落号 · 能引用</span>
          </span>
        </>
      );
      break;
    case "excerpt":
      inner = (
        <span className="bx">
          <q>{clip(m.body || m.title, 160)}</q>
          {lbl}
          {mt}
        </span>
      );
      break;
    case "web":
      inner = (
        <>
          <span className="bar">
            <i />
            <i />
            <i />
            <span>{m.sub || "网页"}</span>
          </span>
          <span className="bx">
            {lbl}
            {tt}
            {tx}
            {mt}
          </span>
        </>
      );
      break;
    case "file": {
      const ext = (m.title.split(".").pop() || "FILE").toUpperCase().slice(0, 4);
      inner = (
        <span className="bx">
          <span className="doc">
            <span>{ext}</span>
          </span>
          <span>
            {lbl}
            {tt}
            <span className="path">{m.sub}</span>
            {mt}
          </span>
        </span>
      );
      break;
    }
    case "scan":
      inner = (
        <>
          <span className="img">
            <i />
            <b>{clip(m.body || m.title, 90)}</b>
            <i />
          </span>
          <span className="bx">
            {lbl}
            {tt}
            <span className="chip">已识别文字 · 能检索</span>
            {mt}
          </span>
        </>
      );
      break;
    case "concept":
      inner = (
        <span className="bx">
          {lbl}
          {tt}
          {tx}
          <span className="ft">
            <span>{m.sub}</span>
            <span>{dateOf(m.tendedAt)}</span>
          </span>
        </span>
      );
      break;
    case "report":
      inner = (
        <span className="bx">
          {lbl}
          {tt}
          <span className="vd">✓ 核对过</span>
          {tx}
          <span className="ev">
            <i />
            <i />
            <i />
            <i />
          </span>
          {mt}
        </span>
      );
      break;
    case "wonder":
      inner = (
        <>
          <span className="plot">
            <svg viewBox="0 0 240 150" aria-hidden="true">
              {Array.from({ length: 10 }, (_, c) => (
                <rect key={c} x={18 + c * 21} y={118 - (c + 1) * 9} width={7} height={(c + 1) * 9} rx={2} />
              ))}
            </svg>
          </span>
          <span className="bx">
            {lbl}
            {tt}
            {tx}
          </span>
        </>
      );
      break;
    case "answer":
      inner = (
        <span className="bx">
          {lbl}
          <span className="q">{m.title}</span>
          <span className="an">{m.body}</span>
          {mt}
        </span>
      );
      break;
    default:
      inner = (
        <span className="bx">
          {lbl}
          {tt}
          {tx}
          {mt}
        </span>
      );
  }
  return (
    <button className={`kc pith-lift t-${m.kind} ${on ? "is-on" : ""}`} style={{ "--i": Math.min(i, 16) } as React.CSSProperties} onClick={onClick}>
      {inner}
    </button>
  );
}

const clip = (s: string, n: number) => (s.length > n ? `${s.slice(0, n)}……` : s);

/** 右边的面板：一份材料 */
function MaterialPanel({ m, kb, kbs, focus, onClose }: { m: Material | null; kb: KnowledgeBase | null; kbs: KnowledgeBase[]; focus: number | null; onClose: () => void }) {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const [edit, setEdit] = useState(false);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  useEffect(() => {
    setEdit(false);
    setTitle(m?.title ?? "");
    setBody(m?.body ?? "");
  }, [m?.uid]); // eslint-disable-line react-hooks/exhaustive-deps

  const update = async (t: string, b: string, stage: number) => {
    if (!m) return;
    try {
      await api.kbMaterialUpdate(m.uid, t, b, stage);
      await qc.invalidateQueries({ queryKey: ["materials"] });
      setEdit(false);
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };
  const refresh = async () => {
    await qc.invalidateQueries({ queryKey: ["materials"] });
    void qc.invalidateQueries({ queryKey: ["kbs"] });
  };
  /** 从一个库里拿掉；别的库还有它就只是少一个库 */
  const removeFrom = async (from: KnowledgeBase | null | undefined) => {
    if (!m || !from) return;
    const last = m.kbs.length <= 1;
    const ok = await ui.confirm({
      title: `从「${from.name}」里拿掉？`,
      message: last ? "它只在这一个库里：拿掉就没了。原来的书、调查、对话都还在。" : "别的库里还有它，只是这个库里不再有。",
      confirm: "拿掉",
      danger: last,
    });
    if (!ok) return;
    await api.kbMaterialDelete(m.uid, from.uid).catch((e) => ui.notify(String(e), "error"));
    await refresh();
    if (last || from.uid === kb?.uid) onClose();
  };
  const link = async (to: string) => {
    if (!m || !to) return;
    await api.kbMaterialLink(m.uid, to).catch((e) => ui.notify(String(e), "error"));
    await refresh();
  };
  const others = kbs.filter((k) => !m?.kbs.includes(k.uid));
  const reads = !!m && m.passages > 0 && (m.kind === "full" || !m.body.trim());
  const origin = m ? originOf(m) : null;

  return (
    <aside className={`panel ${m ? "is-open" : ""}`} aria-hidden={!m}>
      {m && (
        <>
          <button className="x" onClick={onClose} aria-label="关上">
            <X size={18} />
          </button>
          <p className="k">
            <Stage stage={m.stage} />
            <span>{KIND_NAMES[m.kind]}</span>
          </p>
          {edit ? (
            <input className="pith-field kb-title-in" value={title} onChange={(e) => setTitle(e.target.value)} />
          ) : (
            <h2 className={`w${m.stage}`}>{m.title}</h2>
          )}
          {m.sub && <p className="s">{m.sub}</p>}
          {edit ? <textarea className="pith-field" value={body} onChange={(e) => setBody(e.target.value)} /> : reads ? <Original m={m} focus={focus} /> : m.body && <p className="t selectable">{m.body}</p>}
          <p className="acts">
            {edit ? (
              <>
                <button className="pith-button pith-sweep" onClick={() => void update(title, body, m.stage)}>
                  完成
                </button>
                <button className="pith-button pith-button--quiet" onClick={() => setEdit(false)}>
                  算了
                </button>
              </>
            ) : (
              <>
                {origin && (
                  <button
                    className="pith-button pith-sweep"
                    onClick={() => {
                      const { target } = origin;
                      nav.openChat(target);
                    }}
                  >
                    {origin.label} <i className="ar not-italic">→</i>
                  </button>
                )}
                <button className="pith-button pith-button--quiet" onClick={() => setEdit(true)}>
                  编辑
                </button>
                <button className="pith-button pith-button--quiet" onClick={() => void removeFrom(kb)}>
                  拿掉
                </button>
              </>
            )}
          </p>
          <dl>
            <dt>生长阶段</dt>
            <dd>
              <span className="seg">
                {STAGE_NAMES.map((n, s) => (
                  <button key={n} className={m.stage === s ? "is-on" : ""} onClick={() => void update(m.title, m.body, s)}>
                    {n}
                  </button>
                ))}
              </span>
            </dd>
            <dt>种下于</dt>
            <dd>{dateOf(m.createdAt)}</dd>
            <dt>最近照料</dt>
            <dd>{dateOf(m.tendedAt)}</dd>
            <dt>在这些知识库</dt>
            <dd className="kb-in">
              {m.kbs.map((u) => {
                const k = kbs.find((x) => x.uid === u);
                return (
                  k && (
                    <span key={u} className="kb-chip">
                      {k.name}
                      {m.kbs.length > 1 && (
                        <button aria-label={`从「${k.name}」里拿掉`} onClick={() => void removeFrom(k)}>
                          <X size={11} />
                        </button>
                      )}
                    </span>
                  )
                );
              })}
              {others.length > 0 && (
                <Select value="" onChange={(v) => void link(v)} title="再放进一个知识库" render={() => "＋ 也放进…"}>
                  {others.map((k) => (
                    <option key={k.uid} value={k.uid}>
                      {k.name}
                    </option>
                  ))}
                </Select>
              )}
            </dd>
            {m.passages > 0 && (
              <>
                <dt>检索</dt>
                <dd className="num">切成 {m.passages} 块，对话里能找到、能引用</dd>
              </>
            )}
          </dl>
        </>
      )}
    </aside>
  );
}

/** 原文：一章一章读（书有目录），从页边注过来时读到那一段、它亮着；书有对齐的译本时能对照 */
function Original({ m, focus }: { m: Material; focus: number | null }) {
  const [unit, setUnit] = useState<number | null>(null);
  const [pairs, setPairs] = useState(false);
  useEffect(() => setUnit(null), [m.uid, focus]);
  const q = useQuery({ queryKey: ["kbRead", m.uid, unit, unit === null ? focus : null, m.passages], queryFn: () => api.kbRead(m.uid, unit, unit === null ? focus : null) });
  const v = q.data;
  const focusRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    focusRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [v?.focus, v?.unit]);
  if (!v || v.passages.length === 0) return null;
  const hasPairs = Object.keys(v.pairs).length > 0;
  const name = (i: number) => {
    const u = v.units[i];
    return u.key && u.label && u.key !== u.label ? `${u.key}　${u.label}` : u.label || u.key || "正文";
  };
  return (
    <section className="kb-read">
      <div className="kb-read-bar">
        {v.units.length > 1 ? (
          <Select value={String(v.unit)} onChange={(x) => setUnit(Number(x))} title="换一章" className="min-w-0">
            {v.units.map((u, i) => (
              <option key={`${u.key}-${i}`} value={String(i)}>
                {name(i)}
              </option>
            ))}
          </Select>
        ) : (
          <span className="kb-read-name">{name(v.unit)}</span>
        )}
        {hasPairs && (
          <button className={`pith-link ${pairs ? "is-on" : ""}`} onClick={() => setPairs(!pairs)}>
            {pairs ? "收起对照" : `对照${v.pairTitle ? `《${v.pairTitle}》` : "译本"}`}
          </button>
        )}
      </div>
      {v.passages.map((p) =>
        p.role === "matter" ? null : p.role === "heading" ? (
          <p key={p.id} className="kb-read-h">
            {p.text}
          </p>
        ) : (
          <div key={p.id} ref={p.id === v.focus ? focusRef : undefined} className={`kb-read-p ${p.id === v.focus ? "is-focus" : ""} ${p.role !== "body" ? "is-aside" : ""}`}>
            {(p.key || p.page) && <span className="kb-read-loc num">{[p.key, p.page && `第 ${p.page} 页`].filter(Boolean).join(" · ")}</span>}
            <p className="selectable">{p.text}</p>
            {pairs && v.pairs[String(p.id)] && <p className="kb-read-zh selectable">{v.pairs[String(p.id)]}</p>}
          </div>
        ),
      )}
    </section>
  );
}

/** 一份材料从哪来、怎么打开 */
function originOf(m: Material): { label: string; target: Parameters<ReturnType<typeof useNav>["openChat"]>[0] } | null {
  const r = m.sourceRef;
  if (m.sourceKind === "concept_text" && r) return { label: "打开这本书", target: { kind: "book", text: r, title: `读《${m.title}》` } };
  if (m.sourceKind === "research_run" && r) return { label: "看调查", target: { kind: "ref", skill: "research", ref: r, title: m.title } };
  if (m.sourceKind === "concept_card" && r) return { label: "看这次研究", target: { kind: "ref", skill: "concept", ref: r.split("#")[0], title: m.title } };
  if (m.conversation) return { label: "回到那段对话", target: { kind: "chat", uid: m.conversation } };
  return null;
}

/** 种进来：能种的东西，或者自己写一条 */
function Plant({ kb, onClose }: { kb: KnowledgeBase; onClose: () => void }) {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const cand = useQuery({ queryKey: ["kbCandidates"], queryFn: api.kbCandidates, staleTime: 0 });
  const have = useQuery({ queryKey: ["materials", kb.uid], queryFn: () => api.kbMaterials(kb.uid) });
  const [write, setWrite] = useState<MaterialKind | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const planted = useMemo(() => new Set((have.data ?? []).map((m) => `${m.sourceKind}|${m.sourceRef}`)), [have.data]);

  const add = async (m: Partial<Material> & { kind: MaterialKind; title: string }, from?: Element | null) => {
    try {
      await api.kbMaterialAdd({ ...m, kb: kb.uid });
      nav.seed(from ?? null);
      await qc.invalidateQueries({ queryKey: ["materials"] });
      void qc.invalidateQueries({ queryKey: ["kbs"] });
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };

  const groups: [MaterialKind, string][] = [
    ["full", "解析好的书"],
    ["report", "做完的调查"],
    ["concept", "概念研究的卡片"],
  ];
  return (
    <Modal title={`种进「${kb.name}」`} onClose={onClose} width="w-[560px]">
      {write ? (
        <div className="space-y-3">
          <input className="pith-field" autoFocus placeholder={write === "excerpt" ? "出处，例如：Stigler 1951 的摘抄" : "标题"} value={title} onChange={(e) => setTitle(e.target.value)} />
          <textarea className="pith-field" placeholder={write === "excerpt" ? "摘下来的原话" : "写点什么"} value={body} onChange={(e) => setBody(e.target.value)} />
          <div className="flex justify-end gap-2">
            <button className="pith-button pith-button--quiet" onClick={() => setWrite(null)}>
              返回
            </button>
            <button
              className="pith-button pith-sweep"
              disabled={!title.trim()}
              onClick={async (e) => {
                await add({ kind: write, title, body, sub: write === "excerpt" ? "我的摘抄" : "我的笔记" }, e.currentTarget);
                onClose();
              }}
            >
              种下
            </button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="flex gap-2">
            <button className="pith-button pith-button--quiet" onClick={() => setWrite("note")}>
              写一条笔记
            </button>
            <button className="pith-button pith-button--quiet" onClick={() => setWrite("excerpt")}>
              记一条摘抄
            </button>
          </div>
          {groups.map(([kind, name]) => {
            const rows = (cand.data ?? []).filter((c) => c.kind === kind);
            return (
              <section key={kind}>
                <p className="set-h3 !mt-2">{name}</p>
                {rows.length === 0 ? (
                  <p className="text-xs text-muted-foreground">{kind === "full" ? "还没有。文献库里给书的附件点「解析成文本」。" : kind === "report" ? "还没有做完的调查。对话里用 /调查。" : "还没有。对话里用 /概念。"}</p>
                ) : (
                  <div className="qcard-list !mt-0 max-h-56 overflow-y-auto">
                    {rows.map((c) => {
                      const done = planted.has(`${c.sourceKind}|${c.sourceRef}`);
                      return (
                        <button key={`${c.sourceKind}-${c.sourceRef}`} disabled={done} onClick={(e) => void add(c, e.currentTarget)} className={done ? "opacity-50" : ""}>
                          <KindIcon kind={c.kind} />
                          {c.title}
                          <span>{done ? "已种下" : c.sub}</span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </section>
            );
          })}
        </div>
      )}
    </Modal>
  );
}
