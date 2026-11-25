// 首页（V1「生长」）：问候居中，对话框在中间；打开时一片叶子从对话框长出去——主脉从叶柄画到叶尖、穿过对话框，
// 侧脉依次长出去，最后勾叶缘；叶缘上五颗芽是最近照料的材料（悬停一颗，通到它的叶脉加深）。
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import leafSvg from "@/vendor/pith/marks/garden/leaf.svg?raw";
import { api } from "@/shared/ipc";
import { Leaf, KIND_NAMES } from "@/shared/marks";
import { useNav, dateOf } from "@/app/nav";
import { Composer, type Ask } from "@/chat/Composer";

// 叶子图里五颗芽的位置（viewBox 100 286 1032 324）；标签放哪：左、下、下、下、右（和 R1 一样）
const VIEW = { x: 100, y: 286, w: 1032, h: 324 };
const BUDS = [...leafSvg.matchAll(/class="bd bd(\d)"[^>]*? cx="([\d.]+)" cy="([\d.]+)"/g)].map((m) => ({ n: Number(m[1]), x: Number(m[2]), y: Number(m[3]) }));
const AT = ["l", "b", "b", "b", "r"] as const;

export function Home() {
  const nav = useNav();
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [typing, setTyping] = useState(false);
  const leaf = useRef<HTMLDivElement>(null);
  const recent = useQuery({ queryKey: ["materials", null], queryFn: () => api.kbMaterials(null) });

  // 侧脉一根根长：照 R1 的节奏写延时
  useEffect(() => {
    leaf.current?.querySelectorAll<SVGElement>(".vn").forEach((v, k) => v.style.setProperty("--d", `${(0.95 + Math.floor(k / 2) * 0.07).toFixed(2)}s`));
    leaf.current?.querySelectorAll<SVGElement>(".bd").forEach((b, n) => b.style.setProperty("--d", `${(2.35 + n * 0.1).toFixed(2)}s`));
  }, []);

  const send = async (a: Ask) => {
    setBusy(true);
    try {
      const chat = await api.chatStart(a.text, a.role, a.kbs, a.skill);
      qc.setQueryData(["chat", chat.uid], chat);
      void qc.invalidateQueries({ queryKey: ["chats"] });
      nav.openChat({ kind: "chat", uid: chat.uid });
    } catch (e) {
      nav.say(String(e));
    } finally {
      setBusy(false);
    }
  };

  const mats = (recent.data ?? []).slice(0, 5);
  return (
    <div className={`home ${typing ? "is-typing" : ""}`}>
      <div className="hello">
        <h1>今天想弄清什么？</h1>
        <p>问一个问题。/ 叫一个技能，@ 请一个角色，# 选知识库。</p>
      </div>
      <div className="home-leaf" ref={leaf}>
        <Leaf />
        <div className="home-buds">
          {mats.map((m, i) => {
            const b = BUDS[i];
            if (!b) return null;
            const left = ((b.x - VIEW.x) / VIEW.w) * 100;
            const top = ((b.y - VIEW.y) / VIEW.h) * 100;
            const at = AT[i];
            const style: React.CSSProperties = {
              left: `calc(${left}% + ${at === "l" ? -14 : at === "r" ? 14 : -10}px)`,
              top: `calc(${top}% + ${at === "b" ? 14 : -46}px)`,
              ["--d" as string]: `${(2.45 + i * 0.1).toFixed(2)}s`,
            };
            return (
              <button key={m.uid} className={`bud bud-${i} at-${at}`} style={style} onClick={() => nav.openKb(m.kb, m.uid)}>
                <b className={`w${m.stage}`}>{m.title}</b>
                <span>
                  {KIND_NAMES[m.kind]} · {dateOf(m.tendedAt)}
                </span>
              </button>
            );
          })}
        </div>
      </div>
      <div className="composer-wrap">
        <Composer placeholder="问 Pith……" busy={busy} onSend={send} onTyping={setTyping} />
      </div>
    </div>
  );
}
