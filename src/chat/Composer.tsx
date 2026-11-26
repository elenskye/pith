// 输入框：/ 点名技能，@ 请一个角色，# 选知识库（选了留成小标签）。等回答时字段里是「Pith 在想……」。
// Enter 发送，Shift+Enter 换行。打 / @ # 作为第一个字也会弹出对应的单子。
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type SkillId } from "@/shared/ipc";
import { Agent, Mark } from "@/shared/marks";
import { SKILLS, roleTone } from "@/app/nav";

export type Ask = { text: string; role: string; kbs: string[]; skill: SkillId | null };

type Menu = "/" | "@" | "#" | null;

export function Composer({
  placeholder,
  busy,
  onSend,
  defaultKbs,
  allowSkill = true,
  onTyping,
  onStop,
}: {
  placeholder: string;
  busy?: boolean;
  onSend: (a: Ask) => void | Promise<void>;
  defaultKbs?: string[];
  allowSkill?: boolean;
  onTyping?: (typing: boolean) => void;
  /** 正在回答时给一个「停」 */
  onStop?: () => void;
}) {
  const [text, setText] = useState("");
  const [skill, setSkill] = useState<SkillId | null>(null);
  const [role, setRole] = useState<string | null>(null);
  const [kbs, setKbs] = useState<string[]>(defaultKbs ?? []);
  const [menu, setMenu] = useState<Menu>(null);
  const [active, setActive] = useState(0);
  const ref = useRef<HTMLTextAreaElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const roles = useQuery({ queryKey: ["roles"], queryFn: api.roleList });
  const kbList = useQuery({ queryKey: ["kbs"], queryFn: api.kbList });

  useEffect(() => onTyping?.(text.length > 0 || !!skill), [text, skill, onTyping]);
  // 设置 › 角色的「和它开始对话」
  useEffect(() => {
    const on = (e: Event) => {
      const r = (e as CustomEvent<{ role: string }>).detail.role;
      setRole(r === "pith" ? null : r);
      ref.current?.focus();
    };
    window.addEventListener("pith:compose", on);
    return () => window.removeEventListener("pith:compose", on);
  }, []);
  // 点外面收起单子
  useEffect(() => {
    if (!menu) return;
    const off = (e: MouseEvent) => !wrap.current?.contains(e.target as Node) && setMenu(null);
    window.addEventListener("mousedown", off);
    return () => window.removeEventListener("mousedown", off);
  }, [menu]);

  const options = menu === "/" ? SKILLS.length : menu === "@" ? (roles.data?.length ?? 0) : menu === "#" ? (kbList.data?.length ?? 0) : 0;
  const pick = (i: number) => {
    if (menu === "/") setSkill(SKILLS[i].id);
    else if (menu === "@" && roles.data) setRole(roles.data[i].uid === "pith" ? null : roles.data[i].uid);
    else if (menu === "#" && kbList.data) {
      const uid = kbList.data[i].uid;
      setKbs((k) => (k.includes(uid) ? k.filter((x) => x !== uid) : [...k, uid]));
      return; // 知识库能勾几个，单子不收
    }
    setMenu(null);
    ref.current?.focus();
  };

  const send = () => {
    const t = text.trim();
    if (busy || (!t && !skill)) return;
    void onSend({ text: t || SKILLS.find((s) => s.id === skill)!.name, role: role ?? "pith", kbs, skill });
    setText("");
    setSkill(null);
    setRole(null);
  };

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (menu && options > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : options - 1)) % options);
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        pick(active);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setMenu(null);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    } else if (e.key === "Backspace" && text === "") {
      if (skill) setSkill(null);
      else if (role) setRole(null);
    }
  };

  const onChange = (v: string) => {
    // 第一个字打 / @ #：弹出单子，不留在字段里
    if (text === "" && (v === "/" || v === "@" || v === "#") && (v !== "/" || allowSkill)) {
      setMenu(v as Menu);
      setActive(0);
      return;
    }
    setText(v);
  };

  const open = (m: Menu) => {
    setMenu(menu === m ? null : m);
    setActive(0);
  };
  const roleObj = roles.data?.find((r) => r.uid === role);
  const ready = !busy && (text.trim().length > 0 || !!skill);

  return (
    <div className="pith-composer" ref={wrap}>
      <div className="pith-composer-field" onClick={() => ref.current?.focus()}>
        {busy ? (
          <span className="pith-thinking">
            <Agent id="think" className="pith-agent" />
            <span className="pith-shimmer">Pith 在想……</span>
          </span>
        ) : (
          <>
            {skill && (
              <span className="pith-chip is-skill">
                {SKILLS.find((s) => s.id === skill)?.cmd}
                <button className="chip-x" onClick={() => setSkill(null)} aria-label="不用这个技能">
                  ×
                </button>
              </span>
            )}
            {roleObj && (
              <span className="pith-chip is-role">
                @{roleObj.name}
                <button className="chip-x" onClick={() => setRole(null)} aria-label="不请这个角色">
                  ×
                </button>
              </span>
            )}
            {kbList.data
              ?.filter((k) => kbs.includes(k.uid))
              .map((k) => (
                <span key={k.uid} className="pith-chip">
                  # {k.name}
                  <button className="chip-x" onClick={() => setKbs((x) => x.filter((u) => u !== k.uid))} aria-label="不在这个知识库里找">
                    ×
                  </button>
                </span>
              ))}
            <textarea ref={ref} rows={1} value={text} onChange={(e) => onChange(e.target.value)} onKeyDown={onKey} placeholder={skill ? SKILLS.find((s) => s.id === skill)?.line : placeholder} className="selectable" />
          </>
        )}
      </div>
      {!busy && (
        <div className="pith-sig">
          {allowSkill && (
            <button className={`pith-ink ${menu === "/" ? "is-open" : ""}`} onClick={() => open("/")} aria-label="技能">
              /
            </button>
          )}
          <button className={`pith-ink ${menu === "@" ? "is-open" : ""}`} onClick={() => open("@")} aria-label="角色">
            @
          </button>
          <button className={`pith-ink ${menu === "#" ? "is-open" : ""}`} onClick={() => open("#")} aria-label="知识库">
            #
          </button>
        </div>
      )}
      {busy && onStop ? (
        <button className="pith-send comp-stop" onClick={onStop} aria-label="停下这一回" title="停下（已经写出来的照样留着）">
          <i />
        </button>
      ) : (
        <button className="pith-send pith-sweep" onClick={send} disabled={!ready} aria-label="发送">
          <Mark path="icons/send" />
        </button>
      )}
      {menu === "/" && (
        <div className="comp-menu">
          <p>/ 技能：点名用一个，不点 Pith 自己挑</p>
          {SKILLS.map((s, i) => (
            <button key={s.id} className={i === active ? "is-active" : ""} onMouseEnter={() => setActive(i)} onClick={() => pick(i)}>
              <span className="k">{s.cmd}</span>
              {s.name}
            </button>
          ))}
        </div>
      )}
      {menu === "@" && (
        <div className="comp-menu">
          <p>@ 角色：请一位答这一回</p>
          {roles.data?.map((r, i) => (
            <button key={r.uid} className={i === active ? "is-active" : ""} onMouseEnter={() => setActive(i)} onClick={() => pick(i)}>
              <i className="av" style={{ background: `color-mix(in srgb, ${roleTone(i)} 26%, var(--color-page))` }}>
                {r.mark}
              </i>
              {r.name}
              <span className="r">{r.line}</span>
            </button>
          ))}
        </div>
      )}
      {menu === "#" && (
        <div className="comp-menu">
          <p># 知识库：这一次在哪几个里找</p>
          {kbList.data?.length === 0 && <p style={{ border: 0 }}>还没有知识库。到顶栏的知识库里建一个。</p>}
          {kbList.data?.map((k, i) => (
            <button key={k.uid} className={`${kbs.includes(k.uid) ? "is-on" : ""} ${i === active ? "is-active" : ""}`} onMouseEnter={() => setActive(i)} onClick={() => pick(i)}>
              <span className="ck" />
              {k.name}
              <span className="r num">{k.count}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
