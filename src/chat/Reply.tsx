// 一条消息：你说的；Pith（或点名的角色）的——一行状态行（每一步一个 Agent 图标，子代理的步骤收在派它的那一格下面）、
// 正文（轻量的 Markdown：段落、列表、加粗、小标题；[n] 是编号，点开在右边展开页边注）、问题卡（点选项就当你这样回答）、
// 交付卡（「存进……」把它种进知识库，种子落到顶栏的知识库图标），和这一回怎么结束的（停了、月上限、拒答……）。
// 正在回答的那一回（LiveReply）：状态行一格格长出来，正文一边写一边出来。
import { Fragment, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type ChatAsk, type ChatBody, type ChatCite, type ChatLive, type ChatMessage, type ChatStep, type Role } from "@/shared/ipc";
import { Agent, Stage } from "@/shared/marks";
import { useNav, roleTone } from "@/app/nav";
import { money } from "@/research/labels";

/** 一行里的 **加粗** 和 [n] 编号 */
function Inline({ text, open, onRef }: { text: string; open: number | null; onRef: (n: number) => void }) {
  const parts = text.split(/(\[\d+\]|\*\*[^*]+\*\*)/g);
  return (
    <>
      {parts.map((p, i) => {
        const ref = /^\[(\d+)\]$/.exec(p);
        if (ref) {
          const n = Number(ref[1]);
          return (
            <b key={i} className={`pith-ref ${open === n ? "is-on" : ""}`} onClick={() => onRef(n)} role="button" aria-label={`引用 ${n}`}>
              {n}
            </b>
          );
        }
        const bold = /^\*\*([^*]+)\*\*$/.exec(p);
        if (bold) return <strong key={i}>{bold[1]}</strong>;
        return <Fragment key={i}>{p}</Fragment>;
      })}
    </>
  );
}

/** 一段：列表（- 或 1.）、小标题（#），其余是段落 */
function Block({ text, open, onRef, className, style }: { text: string; open: number | null; onRef: (n: number) => void; className: string; style?: React.CSSProperties }) {
  const lines = text.split("\n").filter((l) => l.trim());
  const list = lines.length > 0 && lines.every((l) => /^\s*([-*•]|\d+[.、)])\s+/.test(l));
  if (list) {
    const ordered = /^\s*\d/.test(lines[0]);
    const items = lines.map((l) => l.replace(/^\s*([-*•]|\d+[.、)])\s+/, ""));
    const Tag = ordered ? "ol" : "ul";
    return (
      <Tag className={`${className} pith-list`} style={style}>
        {items.map((t, i) => (
          <li key={i}>
            <Inline text={t} open={open} onRef={onRef} />
          </li>
        ))}
      </Tag>
    );
  }
  const head = /^#{1,4}\s+(.*)$/.exec(lines[0] ?? "");
  if (head && lines.length === 1) {
    return (
      <p className={`${className} pith-h`} style={style}>
        <Inline text={head[1]} open={open} onRef={onRef} />
      </p>
    );
  }
  return (
    <p className={className} style={style}>
      {lines.map((l, i) => (
        <Fragment key={i}>
          {i > 0 && <br />}
          <Inline text={l.replace(/^#{1,4}\s+/, "")} open={open} onRef={onRef} />
        </Fragment>
      ))}
    </p>
  );
}

export function YouLine({ body, roles }: { body: ChatBody; roles: Role[] }) {
  const r = roles.find((x) => x.uid === body.role);
  return (
    <p className="you selectable">
      你：{r && r.uid !== "pith" && <b>@{r.name} </b>}
      {body.text}
    </p>
  );
}

function Who({ author, roles }: { author: string; roles: Role[] }) {
  const role = roles.find((r) => r.uid === author);
  const i = roles.findIndex((r) => r.uid === author);
  if (!role || role.uid === "pith") return null;
  return (
    <div className="who">
      <i className="av" style={{ background: `color-mix(in srgb, ${roleTone(i)} 26%, var(--color-page))` }}>
        {role.mark}
      </i>
      <b>{role.name}</b>
      <span>{role.line}</span>
    </div>
  );
}

/** 状态行：主线的格子一格格排开；子代理的步骤收在派它的那一格后面，小一号。live 时最后一格在走 */
function Status({ steps, live }: { steps: ChatStep[]; live?: boolean }) {
  if (steps.length === 0) return null;
  const last = steps.length - 1;
  return (
    <p className="pith-status">
      {steps.map((s, i) => (
        <span key={i} className={`pith-status-step ${s.sub ? "is-sub" : ""} ${live && i === last ? "is-run" : "is-done"}`}>
          <Agent id={s.agent} className="pith-agent" />
          <span className={live && i === last ? "pith-shimmer" : ""}>{s.text}</span>
        </span>
      ))}
    </p>
  );
}

const ENDING: Partial<Record<NonNullable<ChatBody["ending"]>, string>> = {
  stopped: "停了",
  budget: "到了月上限",
  refusal: "模型没有答",
  truncated: "写到上限了",
  error: "出错了",
  limit: "先停在这里",
  looping: "先停在这里",
};

function AskCard({ ask, onAnswer, answered }: { ask: ChatAsk; onAnswer?: (text: string) => void; answered: boolean }) {
  if (typeof ask === "string") return <p className="ask-back">{ask}</p>;
  return (
    <div className={`ask-card ${answered ? "is-answered" : ""}`}>
      <Agent id="ask" className="pith-agent" once />
      <div>
        <p className="ask-q">{ask.question}</p>
        {ask.options.length > 0 && (
          <div className="ask-opts">
            {ask.options.map((o) => (
              <button key={o} className="ask-opt" disabled={answered || !onAnswer} onClick={() => onAnswer?.(o)}>
                {o}
              </button>
            ))}
          </div>
        )}
        {!answered && <p className="ask-hint">也可以直接在下面写你的回答。</p>}
      </div>
    </div>
  );
}

export function Reply({ msg, conv, roles, fresh, first, last, onAnswer }: { msg: ChatMessage; conv: string; roles: Role[]; fresh: boolean; first: boolean; last: boolean; onAnswer?: (text: string) => void }) {
  const nav = useNav();
  const qc = useQueryClient();
  const b = msg.body;
  const [open, setOpen] = useState<number | null>(null);
  const deliverRef = useRef<HTMLButtonElement>(null);
  const role = roles.find((r) => r.uid === msg.author);
  const sourceRef = `${conv}#${msg.position}`;
  const saved = useQuery({ queryKey: ["materials", null], queryFn: () => api.kbMaterials(null), select: (ms) => ms.find((m) => m.sourceKind === "conversation" && m.sourceRef === sourceRef) ?? null });
  const [saving, setSaving] = useState(false);
  const onRef = (n: number) => setOpen((o) => (o === n ? null : n));

  const save = async () => {
    const d = b.delivery;
    if (!d || saving) return;
    setSaving(true);
    try {
      const kind = (["answer", "note", "concept", "excerpt"].includes(d.kind) ? d.kind : "answer") as "answer";
      await api.kbMaterialAdd({ kb: d.kb?.uid ?? "", kind, title: d.title, sub: role ? role.name : "Pith", body: d.body?.trim() ? d.body : (b.paragraphs ?? []).join("\n\n"), sourceKind: "conversation", sourceRef, conversation: conv });
      nav.seed(deliverRef.current);
      await qc.invalidateQueries({ queryKey: ["materials"] });
    } catch (e) {
      nav.say(String(e));
    } finally {
      setSaving(false);
    }
  };

  const kbName = saved.data ? undefined : b.delivery?.kb?.name;
  const cite = (c: ChatCite) => (
    <span key={c.n} className="side-note pith-sidenote">
      <span className="pith-sidenote-head">
        <i>{c.n}</i>
        {c.kind}
      </span>
      <b>{c.src}</b>
      {c.loc && <span className="loc">{c.loc}</span>}
      <q className="selectable">{c.text}</q>
      <span className="pith-sidenote-go">
        {c.material && (
          <button className="pith-link" onClick={() => nav.openKb(c.kb ?? null, c.material)}>
            知识库里这一段
          </button>
        )}
      </span>
    </span>
  );
  const ending = b.ending && ENDING[b.ending];
  return (
    <div className="msg">
      <Who author={msg.author} roles={roles} />
      <Status steps={b.steps ?? []} />
      {(b.paragraphs ?? []).map((p, i) => (
        <div key={i} className="pith-block">
          <Block text={p} open={open} onRef={onRef} className={`pith-p selectable ${fresh ? "unfold" : ""} ${first && i === 0 ? "cap" : ""}`} style={fresh ? { animationDelay: `${i * 0.18}s` } : undefined} />
          {b.cites?.filter((c) => open === c.n && p.includes(`[${c.n}]`)).map(cite)}
        </div>
      ))}
      {b.ask && <AskCard ask={b.ask} onAnswer={onAnswer} answered={!last} />}
      {(ending || b.note) && (
        <p className={`reply-note ${b.ending === "done" || b.ending === "asked" ? "" : `is-${b.ending}`}`}>
          {ending && <b>{ending}</b>}
          {b.note}
        </p>
      )}
      {b.mock && <span className="mock-note">1.0 的演示回答</span>}
      {b.delivery && (
        <article className={`pith-deliver ${saved.data ? "is-saved" : ""}`}>
          <span className="deliver-grow">
            <Stage stage={0} className="g1" />
            <Stage stage={1} className="g2" />
          </span>
          <div>
            <b className="pith-deliver-title" style={{ fontWeight: saved.data ? 520 : 300 }}>
              {b.delivery.title}
            </b>
            <p className="pith-deliver-desc">{b.delivery.desc}</p>
            <p className="pith-deliver-meta">{b.delivery.meta}</p>
          </div>
          {saved.data ? (
            <span className="pith-deliver-ok">
              <Agent id="save" className="pith-agent" once />
              <span>
                已种下
                <br />
                <button className="pith-link" onClick={() => nav.openKb(saved.data!.kb, saved.data!.uid)}>
                  去看
                </button>
              </span>
            </span>
          ) : (
            <button ref={deliverRef} className="pith-link act" onClick={save} disabled={saving}>
              {kbName ? `存进「${kbName}」` : "存进知识库"} →
            </button>
          )}
        </article>
      )}
      {b.model && (
        <p className="reply-meta">
          {b.model}
          {typeof b.costUsd === "number" && b.costUsd > 0 && <span className="num"> · {money(b.costUsd)}</span>}
        </p>
      )}
    </div>
  );
}

/** 正在回答的那一回 */
export function LiveReply({ live, roles, onStop }: { live: ChatLive; roles: Role[]; onStop: () => void }) {
  const steps: ChatStep[] = live.steps.length ? live.steps : [{ agent: "think", text: "在想" }];
  const paragraphs = live.text.split(/\n{2,}/).filter((p) => p.trim());
  const writing = paragraphs.length > 0;
  const shown: ChatStep[] = writing ? [...steps, { agent: "write", text: "在写" }] : steps;
  return (
    <div className="msg is-live">
      <Who author={live.author} roles={roles} />
      <Status steps={shown} live />
      {paragraphs.map((p, i) => (
        <div key={i} className="pith-block">
          <Block text={p} open={null} onRef={() => {}} className={`pith-p selectable ${i === paragraphs.length - 1 ? "is-writing" : ""}`} />
        </div>
      ))}
      <button className="pith-link live-stop" onClick={onStop}>
        停
      </button>
    </div>
  );
}
