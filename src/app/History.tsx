// 对话历史：没有按钮，⌘\（Ctrl+\）或两指往右滑叫出来，往左滑、Esc、点空白处收起。
// 不是有底色的侧栏，是从左边长出来的枝桠，和对话页是同一张纸；叫出来时页面往右让 60、正文变淡。
import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type ChatSummary } from "@/shared/ipc";
import { Mark, STAGE_WEIGHT } from "@/shared/marks";
import { useNav, skillName, when } from "@/app/nav";
import { branch, labelDelay, type BranchNode } from "@/app/branch";

type Row = BranchNode & { t?: string; c?: ChatSummary };

function group(iso: string): string {
  const days = (Date.now() - new Date(iso).getTime()) / 86400000;
  if (days < 1 && new Date(iso).toDateString() === new Date().toDateString()) return "今天";
  if (days < 7) return "这周";
  return "更早";
}

export function History() {
  const nav = useNav();
  const list = useQuery({ queryKey: ["chats"], queryFn: api.chatList, enabled: nav.side, staleTime: 0 });
  const current = nav.chat?.kind === "chat" ? nav.chat.uid : nav.chat?.kind === "ref" ? nav.chat.ref : null;

  const rows = useMemo(() => {
    const out: Row[] = [{ kind: "new", y: 54 }];
    let y = 54;
    let last = "";
    for (const c of (list.data ?? []).slice(0, 60)) {
      const g = group(c.updatedAt);
      if (g !== last) {
        y += last ? 78 : 58;
        out.push({ kind: "g", y, t: g });
        y += 34;
        last = g;
      } else y += 56;
      out.push({ kind: "i", y, c, on: (c.uid ?? c.refUid) === current });
    }
    return out;
  }, [list.data, current]);

  const height = rows[rows.length - 1].y + 80;
  const { paths, dots } = branch(30, height - 40, rows);

  return (
    <>
      {nav.side && <button className="hist-scrim" onClick={() => nav.setSide(false)} aria-label="收起以前的对话" />}
      <nav className="hist" aria-label="以前的对话">
        <div style={{ position: "relative", height }}>
          <svg className="br-svg" width={340} height={height} viewBox={`0 0 340 ${height}`} aria-hidden="true">
            {paths.map((p, i) => (
              <path key={i} className={p.cls} pathLength={1} d={p.d} style={{ "--d": `${p.delay}s`, ...(p.dur ? { "--dur": `${p.dur}s` } : {}) } as React.CSSProperties} />
            ))}
            {dots.map((d, i) => (
              <circle key={i} className={d.on ? "on" : ""} cx={d.cx} cy={d.cy} r={2.8} style={{ "--d": `${d.delay}s` } as React.CSSProperties} />
            ))}
          </svg>
          {rows.map((r, k) => {
            const style = { "--d": `${labelDelay(k)}s` } as React.CSSProperties;
            if (r.kind === "new")
              return (
                <button key="new" className="br-lbl br-new pith-row" style={{ ...style, top: r.y - 12 }} onClick={() => nav.openChat(null)}>
                  <Mark path="icons/new" />
                  新对话<kbd>⌘N</kbd>
                </button>
              );
            if (r.kind === "g")
              return (
                <p key={`g${k}`} className="br-lbl br-g" style={{ ...style, top: r.y - 7 }}>
                  {r.t}
                </p>
              );
            const c = r.c!;
            return (
              <button
                key={c.uid ?? `${c.skill}-${c.refUid}`}
                className={`br-lbl br-it pith-row ${r.on ? "is-on" : ""}`}
                style={{ ...style, top: r.y - 11 }}
                onClick={() => nav.openChat(c.uid ? { kind: "chat", uid: c.uid } : { kind: "ref", skill: c.skill!, ref: c.refUid!, title: c.title })}
              >
                <b style={{ fontWeight: STAGE_WEIGHT[c.stage] }}>{c.title}</b>
                <span>
                  {c.skill ? skillName(c.skill) : roleName(c.role)} · {when(c.updatedAt)}
                </span>
              </button>
            );
          })}
          {list.data && list.data.length === 0 && <p className="br-empty br-lbl" style={{ "--d": ".4s" } as React.CSSProperties}>还没有以前的对话。问一个问题，它就从这里长出来。</p>}
        </div>
      </nav>
    </>
  );
}

const ROLE_NAMES: Record<string, string> = { pith: "Pith", companion: "伴读", reviewer: "审稿人", editor: "编辑" };
const roleName = (uid: string) => ROLE_NAMES[uid] ?? "角色";
