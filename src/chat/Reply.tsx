// 一条消息：你说的；Pith（或点名的角色）的——一行状态行（每一步一个 Agent 图标）、正文带编号 [n]（点开在右边展开页边注）、
// 反问、交付卡（「存进……」把它种进知识库，种子落到顶栏的知识库图标）。刚来的那条照真的节奏一步步演出来。
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type ChatBody, type ChatMessage, type Role } from "@/shared/ipc";
import { Agent, Stage } from "@/shared/marks";
import { useNav, roleTone } from "@/app/nav";

/** 正文里的 [n] 换成编号按钮 */
function Cited({ text, open, onRef }: { text: string; open: number | null; onRef: (n: number) => void }) {
  const parts = text.split(/(\[\d+\])/g);
  return (
    <>
      {parts.map((p, i) => {
        const m = /^\[(\d+)\]$/.exec(p);
        if (!m) return <span key={i}>{p}</span>;
        const n = Number(m[1]);
        return (
          <b key={i} className={`pith-ref ${open === n ? "is-on" : ""}`} onClick={() => onRef(n)} role="button" aria-label={`引用 ${n}`}>
            {n}
          </b>
        );
      })}
    </>
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

export function Reply({ msg, conv, roles, fresh, first }: { msg: ChatMessage; conv: string; roles: Role[]; fresh: boolean; first: boolean }) {
  const nav = useNav();
  const qc = useQueryClient();
  const b = msg.body;
  const steps = b.steps ?? [];
  // 刚来的：状态行一步步走完（每步 0.55 秒），正文再展开
  const [at, setAt] = useState(fresh ? 0 : steps.length);
  const [open, setOpen] = useState<number | null>(null);
  const deliverRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (at >= steps.length) return;
    const t = setTimeout(() => setAt((a) => a + 1), 550);
    return () => clearTimeout(t);
  }, [at, steps.length]);
  const done = at >= steps.length;
  const role = roles.find((r) => r.uid === msg.author);
  const roleIndex = roles.findIndex((r) => r.uid === msg.author);
  const sourceRef = `${conv}#${msg.position}`;
  const saved = useQuery({ queryKey: ["materials", null], queryFn: () => api.kbMaterials(null), select: (ms) => ms.find((m) => m.sourceKind === "conversation" && m.sourceRef === sourceRef) ?? null });
  const [saving, setSaving] = useState(false);

  const save = async () => {
    const d = b.delivery;
    if (!d || saving) return;
    setSaving(true);
    try {
      await api.kbMaterialAdd({ kb: d.kb?.uid ?? "", kind: "answer", title: d.title, sub: role ? role.name : "Pith", body: (b.paragraphs ?? []).join("\n\n"), sourceKind: "conversation", sourceRef, conversation: conv });
      nav.seed(deliverRef.current);
      await qc.invalidateQueries({ queryKey: ["materials"] });
    } catch (e) {
      nav.say(String(e));
    } finally {
      setSaving(false);
    }
  };

  const kbName = saved.data ? undefined : b.delivery?.kb?.name;
  return (
    <div className="msg">
      {role && role.uid !== "pith" && (
        <div className="who">
          <i className="av" style={{ background: `color-mix(in srgb, ${roleTone(roleIndex)} 26%, var(--color-page))` }}>
            {role.mark}
          </i>
          <b>{role.name}</b>
          <span>{role.line}</span>
        </div>
      )}
      {steps.length > 0 && (
        <p className="pith-status">
          {steps.map((s, i) => (
            <span key={i} className={`pith-status-step ${i < at ? "is-done" : i === at ? "is-run" : ""}`} style={i > at ? { display: "none" } : undefined}>
              <Agent id={s.agent} className="pith-agent" />
              <span className={i === at ? "pith-shimmer" : ""}>{s.text}</span>
            </span>
          ))}
        </p>
      )}
      {done && (
        <>
          {(b.paragraphs ?? []).map((p, i) => (
            <p key={i} className={`pith-p selectable ${fresh ? "unfold" : ""} ${first && i === 0 ? "cap" : ""}`} style={fresh ? { animationDelay: `${i * 0.25}s` } : undefined}>
              <Cited text={p} open={open} onRef={(n) => setOpen((o) => (o === n ? null : n))} />
              {b.cites
                ?.filter((c) => open === c.n && p.includes(`[${c.n}]`))
                .map((c) => (
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
                ))}
            </p>
          ))}
          {b.ask && <p className={`ask-back ${fresh ? "unfold" : ""}`}>{b.ask}</p>}
          {b.mock && <span className="mock-note">演示回答 · 还没接上模型</span>}
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
        </>
      )}
    </div>
  );
}
