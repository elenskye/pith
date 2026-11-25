// 对话屏：首页（叶子），或者一段对话。对话里点名的技能（事实调查、概念研究、排版）跑的还是原来那套流程，
// 那一页放进对话里；以前的调查、研究没有对话，照样能打开，接着问时再建一段对话连上它。
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { api, type Chat, type Role, type SkillId, type ResearchPlanPreview } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Stage } from "@/shared/marks";
import { useNav, skillName, dateOf, when } from "@/app/nav";
import { Home } from "@/chat/Home";
import { Composer, type Ask } from "@/chat/Composer";
import { Reply, YouLine } from "@/chat/Reply";
import { ResearchPage } from "@/research/ResearchPage";
import { PlanSend } from "@/research/Confirm";
import { ConceptPage, type ConceptAt } from "@/concept/ConceptPage";
import { ConceptSidebar } from "@/concept/ConceptSidebar";
import { FormatPage, type FormatAt } from "@/format/FormatPage";
import { FormatSidebar } from "@/format/FormatSidebar";

export function ChatScreen() {
  const nav = useNav();
  const t = nav.chat;
  if (!t) return <Home />;
  if (t.kind === "ref") return <RefThread key={`${t.skill}-${t.ref}`} skill={t.skill} refUid={t.ref} title={t.title} />;
  if (t.kind === "book") return <BookThread key={t.text} text={t.text} title={t.title} />;
  return <ConvThread key={t.uid} uid={t.uid} />;
}

/** 接着问：点名了技能就开一段新对话，否则在这段里问 */
function useFollowUp(uid: () => Promise<string>) {
  const nav = useNav();
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  const send = async (a: Ask) => {
    setBusy(true);
    try {
      if (a.skill) {
        const chat = await api.chatStart(a.text, a.role, a.kbs, a.skill);
        qc.setQueryData(["chat", chat.uid], chat);
        nav.openChat({ kind: "chat", uid: chat.uid });
      } else {
        const id = await uid();
        const chat = await api.chatSend(id, a.text, a.role === "pith" ? null : a.role);
        qc.setQueryData(["chat", chat.uid], chat);
        if (nav.chat?.kind !== "chat" || nav.chat.uid !== chat.uid) nav.openChat({ kind: "chat", uid: chat.uid });
      }
      void qc.invalidateQueries({ queryKey: ["chats"] });
    } catch (e) {
      nav.say(String(e));
    } finally {
      setBusy(false);
    }
  };
  return { busy, send };
}

function ConvThread({ uid }: { uid: string }) {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const chat = useQuery({ queryKey: ["chat", uid], queryFn: () => api.chatGet(uid) });
  const roles = useQuery({ queryKey: ["roles"], queryFn: api.roleList });
  const kbs = useQuery({ queryKey: ["kbs"], queryFn: api.kbList });
  const { busy, send } = useFollowUp(async () => uid);
  // 打开时已经有的消息不再演；之后来的（接着问的回答）照节奏演出来
  const seen = useRef<number | null>(null);
  if (seen.current === null && chat.data) seen.current = Math.max(0, ...chat.data.messages.map((m) => m.position));
  const scroller = useRef<HTMLDivElement>(null);
  const count = chat.data?.messages.length ?? 0;
  useEffect(() => {
    if (count > 2) scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: "smooth" });
  }, [count]);

  if (chat.error) return <p className="empty selectable">{String(chat.error)}</p>;
  const c = chat.data;
  if (!c) return null;
  const rs = roles.data ?? [];
  const role = rs.find((r) => r.uid === c.role);
  // 第一句就是标题（技能拿它当问题），不再重复
  const rest = c.messages.slice(c.messages[0]?.author === "you" ? 1 : 0);
  const relink = async (skill: SkillId, ref: string) => {
    await api.chatLink(uid, skill, ref);
    await qc.invalidateQueries({ queryKey: ["chat", uid] });
    void qc.invalidateQueries({ queryKey: ["chats"] });
  };
  const rename = async () => {
    const t = await ui.prompt({ title: "给这段对话改个名", initial: c.title, confirm: "改名" });
    if (!t) return;
    await api.chatRename(uid, t);
    void qc.invalidateQueries({ queryKey: ["chat", uid] });
    void qc.invalidateQueries({ queryKey: ["chats"] });
  };
  const remove = async () => {
    const ok = await ui.confirm({ title: "删掉这段对话？", message: c.skill ? "对话删掉以后，里面跑过的调查、研究还在，对话历史里照样能打开。存进知识库的东西也还在。" : "存进知识库的东西还在。", confirm: "删掉", danger: true });
    if (!ok) return;
    await api.chatDelete(uid);
    void qc.invalidateQueries({ queryKey: ["chats"] });
    nav.openChat(null);
  };

  return (
    <div className="thread-screen">
      <div className={`thread ${c.skill ? "is-wide" : ""}`} ref={scroller}>
        <div className="art">
          <Header
            kicker={c.skill ? skillName(c.skill) : "对话"}
            stage={c.stage}
            who={`${role?.name ?? "Pith"} · ${when(c.updatedAt)}`}
            title={c.title}
            tags={c.kbs.map((k) => kbs.data?.find((x) => x.uid === k)?.name).filter((x): x is string => !!x)}
            planted={c.createdAt}
            tended={c.updatedAt}
            actions={
              <>
                <button className="pith-link" onClick={rename}>
                  改名
                </button>
                <button className="pith-link" onClick={remove}>
                  删掉
                </button>
              </>
            }
          />
          {c.skill && (c.refUid ? <SkillSurface skill={c.skill} refUid={c.refUid} onRelink={(r) => void relink(c.skill!, r)} /> : <SkillStart chat={c} onLinked={(r) => void relink(c.skill!, r)} />)}
          {rest.map((m, i) =>
            m.author === "you" ? (
              <YouLine key={m.position} body={m.body} roles={rs} />
            ) : (
              <Reply key={m.position} msg={m} conv={uid} roles={rs} fresh={m.position > (seen.current ?? 0)} first={!c.skill && i === 0} />
            ),
          )}
        </div>
      </div>
      <div className="composer-wrap">
        <Composer placeholder={c.skill ? "接着问……（演示回答）" : "接着问……"} busy={busy} onSend={send} defaultKbs={c.kbs} />
      </div>
    </div>
  );
}

/** 以前的调查、研究（还没有对话）：照样打开；接着问时建一段对话连上它 */
function RefThread({ skill, refUid, title }: { skill: SkillId; refUid: string; title: string }) {
  const { busy, send } = useFollowUp(() => api.chatOpenRef(skill, refUid, title));
  return (
    <div className="thread-screen">
      <div className="thread is-wide">
        <div className="art">
          <Header kicker={skillName(skill)} stage={1} who="以前的" title={title} tags={[]} />
          <SkillSurface skill={skill} refUid={refUid} />
        </div>
      </div>
      <div className="composer-wrap">
        <Composer placeholder="接着问……（演示回答）" busy={busy} onSend={send} />
      </div>
    </div>
  );
}

/** 一本书：书的首页、目录、一章；在书里跑起一次研究时建一段对话连上它 */
function BookThread({ text, title }: { text: string; title: string }) {
  const nav = useNav();
  const qc = useQueryClient();
  const { busy, send } = useFollowUp(async () => (await api.chatStart(title, "companion", [], null)).uid);
  const onRun = async (run: string) => {
    const chat = await api.chatStart(title, "companion", [], "concept");
    await api.chatLink(chat.uid, "concept", run);
    void qc.invalidateQueries({ queryKey: ["chats"] });
    nav.openChat({ kind: "chat", uid: chat.uid });
  };
  return (
    <div className="thread-screen">
      <div className="thread is-wide">
        <div className="art">
          <Header kicker="概念研究" stage={2} who="伴读" title={title} tags={[]} />
          <ConceptHost initial={{ text, unit: null, run: null, card: null }} onRun={(r) => void onRun(r)} />
        </div>
      </div>
      <div className="composer-wrap">
        <Composer placeholder="问这本书……（演示回答）" busy={busy} onSend={send} />
      </div>
    </div>
  );
}

function Header({ kicker, stage, who, title, tags, planted, tended, actions }: { kicker: string; stage: number; who: string; title: string; tags: string[]; planted?: string; tended?: string; actions?: React.ReactNode }) {
  return (
    <>
      <p className="kick">
        <span className="crim">{kicker}</span>
        <Stage stage={stage} />
        <span>{who}</span>
      </p>
      <h1 className={`art-title selectable w${stage}`}>{title}</h1>
      <div className="art-meta">
        <span className="tags">
          {tags.map((t) => (
            <span key={t}># {t}</span>
          ))}
          {actions}
        </span>
        {planted && (
          <p>
            种下于 {dateOf(planted)}
            <br />
            {tended && `${when(tended)}照料`}
          </p>
        )}
      </div>
    </>
  );
}

// ---------- 技能：开始（还没有那次运行）、跑着或跑完（原来那一页放进对话里） ----------

function SkillStart({ chat, onLinked }: { chat: Chat; onLinked: (ref: string) => void }) {
  const question = chat.messages[0]?.body.text ?? chat.title;
  if (chat.skill === "research") return <ResearchStart question={question} onLinked={onLinked} />;
  if (chat.skill === "concept") return <ConceptHost initial={{ text: null, unit: null, run: null, card: null }} onRun={onLinked} />;
  return <FormatHost initial={{ rulesetId: null, runId: null }} onRun={(rs, run) => onLinked(`${rs}:${run}`)} />;
}

function SkillSurface({ skill, refUid, onRelink }: { skill: SkillId; refUid: string; onRelink?: (ref: string) => void }) {
  const nav = useNav();
  if (skill === "research")
    return (
      <div className="skill-surface">
        <ResearchPage at={{ uid: refUid }} onAt={(a) => (a.uid ? onRelink?.(a.uid) : nav.openChat(null))} onSettings={() => nav.openSettings("keys")} />
      </div>
    );
  if (skill === "concept") return <ConceptRef run={refUid} />;
  const [rulesetId, runId] = refUid.split(":");
  return <FormatHost initial={{ rulesetId, runId: runId ?? null }} />;
}

/** 事实调查：拿你问的那句当问题，先算要发什么、多少钱（不联网），你确认以后模型拆子问题 */
function ResearchStart({ question, onLinked }: { question: string; onLinked: (ref: string) => void }) {
  const nav = useNav();
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["researchStatus"], queryFn: api.researchStatus, staleTime: 0 });
  const [preview, setPreview] = useState<ResearchPlanPreview | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const prepare = async () => {
    setError(null);
    setPreparing(true);
    try {
      setPreview(await api.researchPlanPrepare(question));
    } catch (e) {
      setError(String(e));
    } finally {
      setPreparing(false);
    }
  };
  const s = status.data;
  const missing = s ? s.keys.filter((k) => k.needed && !k.has).map((k) => k.label) : [];
  return (
    <div className="qcard">
      <p>要查证这个问题吗？模型先把它拆成能核实的子问题，你确认计划以后 Pith 检索、下载、摘证据，每个数字都在保存下来的原文里逐字核对。</p>
      {s?.mock && <p className="mt-2 text-xs text-muted-foreground">现在是模拟接口：不联网、不花钱，来源和数字都是虚构的「示例市」。</p>}
      {s && !s.mock && missing.length > 0 && (
        <p className="mt-2 text-xs">
          还没有填 {missing.join("、")} 的密钥。
          <button className="pith-link" onClick={() => nav.openSettings("keys")}>
            去设置 › 模型与费用
          </button>
        </p>
      )}
      <div className="opts">
        <button className="pith-button pith-sweep" onClick={prepare} disabled={preparing || !!s?.running}>
          {preparing && <LoaderCircle size={14} className="animate-spin" />}
          开始调查 <i className="ar not-italic">→</i>
          <span className="rec">先看要花多少</span>
        </button>
      </div>
      {s?.running && <p className="mt-2 text-xs text-muted-foreground">有一次调查正在进行，做完或停止以后才能开始新的。</p>}
      {error && <p className="selectable mt-2 text-xs whitespace-pre-wrap text-destructive">{error}</p>}
      {preview && (
        <PlanSend
          preview={preview}
          onSettings={() => nav.openSettings("keys")}
          onClose={() => setPreview(null)}
          onDone={(v) => {
            setPreview(null);
            qc.setQueryData(["researchRun", v.run.uid], v);
            void qc.invalidateQueries({ queryKey: ["researchQuestions"] });
            onLinked(v.run.uid);
          }}
        />
      )}
    </div>
  );
}

/** 概念研究：书架 → 一本书 → 一章 → 研究；左边是这本书的目录（原来的侧栏）。第一次跑起来时把对话连上那次研究 */
function ConceptHost({ initial, onRun }: { initial: ConceptAt; onRun?: (run: string) => void }) {
  const nav = useNav();
  const [at, setAt] = useState<ConceptAt>(initial);
  const linked = useRef(!!initial.run);
  useEffect(() => {
    if (at.run && !linked.current && onRun) {
      linked.current = true;
      onRun(at.run);
    }
  }, [at.run, onRun]);
  // 删掉一本书：回到书架
  useEffect(() => {
    const home = () => setAt({ text: null, unit: null, run: null, card: null });
    window.addEventListener("pith:concept-home", home);
    return () => window.removeEventListener("pith:concept-home", home);
  }, []);
  return (
    <div className={`skill-surface ${at.text ? "has-nav" : ""}`}>
      {at.text && (
        <div className="skill-nav no-swipe">
          <ConceptSidebar at={at} onAt={setAt} />
        </div>
      )}
      <div className="min-w-0">
        <ConceptPage at={at} onAt={setAt} onSettings={() => nav.openSettings("keys")} />
      </div>
    </div>
  );
}

/** 以前的概念研究：先找到它是哪本书的，再打开 */
function ConceptRef({ run }: { run: string }) {
  const view = useQuery({ queryKey: ["conceptRun", run], queryFn: () => api.conceptRunGet(run) });
  if (view.error) return <p className="selectable mt-6 text-destructive">{String(view.error)}</p>;
  if (!view.data) return null;
  return <ConceptHost initial={{ text: view.data.textUid, unit: null, run, card: null }} />;
}

/** 排版：规范清单 → 一份规范 → 一次排版；左边是规范和排过的（原来的侧栏） */
function FormatHost({ initial, onRun }: { initial: FormatAt; onRun?: (ruleset: string, run: string) => void }) {
  const nav = useNav();
  const [at, setAt] = useState<FormatAt>(initial);
  const linked = useRef(!!initial.runId);
  useEffect(() => {
    if (at.rulesetId && at.runId && !linked.current && onRun) {
      linked.current = true;
      onRun(at.rulesetId, at.runId);
    }
  }, [at, onRun]);
  return (
    <div className="skill-surface has-nav">
      <div className="skill-nav no-swipe">
        <FormatSidebar at={at} onAt={setAt} />
      </div>
      <div className="min-w-0">
        <FormatPage at={at} onAt={setAt} onSettings={() => nav.openSettings("keys")} />
      </div>
    </div>
  );
}

export type { Role };
