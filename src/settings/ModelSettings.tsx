// 设置 › 模型与费用（1.1.0 照 R1 重画，decisions/0036）：本月花了多少（大圈、每天的细柱、各家、缓存命中、月上限）、
// 默认模型（一家 + 一档）、各家的密钥（Vertex AI 填项目和区域、看 gcloud 登录了没有）、最近的几笔。
// 大留白、大字、少按钮：填过的密钥只露「换」「清掉」两个字。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type ApiKeyWhich, type ModelKeyView, type ModelProviderView, type ModelStatus, type UsageCall } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { money } from "@/research/labels";
import { UsageRing, DayBars } from "@/chat/Usage";

const PLACE: Record<string, string> = { keychain: "存在钥匙串", memory: "只在这次运行里", file: "来自密钥文件" };
const PURPOSE: Record<string, string> = { chat: "对话", chat_subagent: "子代理", extract: "排版", plan: "排版" };
const purpose = (p: string) => PURPOSE[p] ?? (p.startsWith("research") ? "事实调查" : p.startsWith("concept") ? "概念研究" : p);

export function ModelSettings() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["modelStatus"], queryFn: api.modelStatus, staleTime: 0 });
  const usage = useQuery({ queryKey: ["usage", null], queryFn: () => api.usageSummary(null), staleTime: 0 });
  const put = (s: ModelStatus) => {
    qc.setQueryData(["modelStatus"], s);
    void qc.invalidateQueries({ queryKey: ["researchStatus"] });
    void qc.invalidateQueries({ queryKey: ["agentStatus"] });
    void qc.invalidateQueries({ queryKey: ["conceptStatus"] });
  };
  const s = status.data;
  if (status.error) return <p className="set-line selectable">{String(status.error)}</p>;
  if (!s) return null;
  return (
    <div className="mdl">
      {s.mock && <p className="mdl-mock">开发版、测试用模拟模型：不联网、不花钱。下面的密钥只在换成真接口（正式版，或 PITH_REAL_MODEL=1）时用到。</p>}
      {usage.data && <Spend s={s} onBudget={() => void Promise.all([status.refetch(), usage.refetch()])} />}
      <p className="set-h3">默认模型</p>
      <Default s={s} onStatus={put} />
      <p className="set-h3">各家的密钥</p>
      {s.providers
        .filter((p) => p.key)
        .map((p) => (
          <KeyRow key={p.id} which={p.key!} label={p.label} has={p.ready} place={p.keyPlace ?? "memory"} note={tierLine(p)} onStatus={put} />
        ))}
      <Vertex s={s} onStatus={put} />
      <p className="set-h3">别的密钥</p>
      {s.otherKeys.map((k) => (
        <KeyRow key={k.which} which={k.which} label={k.label} has={k.has} place={k.place} note={k.note} onStatus={put} />
      ))}
      {usage.data && usage.data.recent.length > 0 && (
        <>
          <p className="set-h3">最近的几笔</p>
          <div className="mdl-calls">
            {usage.data.recent.slice(0, 12).map((c, i) => (
              <Call key={i} c={c} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

const tierLine = (p: ModelProviderView) => p.tiers.map((t) => t.modelLabel).filter((v, i, a) => a.indexOf(v) === i).join(" · ");

function Spend({ s, onBudget }: { s: ModelStatus; onBudget: () => void }) {
  const { notify } = useUi();
  const usage = useQuery({ queryKey: ["usage", null], queryFn: () => api.usageSummary(null) });
  const u = usage.data!;
  const [editing, setEditing] = useState(false);
  const [budget, setBudget] = useState(String(s.budgetUsd));
  useEffect(() => setBudget(String(s.budgetUsd)), [s.budgetUsd]);
  const save = async () => {
    const n = Number(budget);
    if (!Number.isFinite(n) || n < 1 || n > 50) return notify("月上限写一个 1 到 50 之间的数", "error");
    try {
      await api.agentBudgetSet(n);
      setEditing(false);
      onBudget();
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const total = Math.max(u.spentUsd, 0.0001);
  return (
    <section className="mdl-spend">
      <UsageRing spent={u.spentUsd} budget={u.budgetUsd} size={112} stroke={6} />
      <div className="mdl-spend-main">
        <p className="set-k">{u.month.replace("-", " 年 ")} 月</p>
        <p className="mdl-big num">{money(u.spentUsd)}</p>
        <p className="mdl-of">
          {editing ? (
            <form
              className="mdl-budget"
              onSubmit={(e) => {
                e.preventDefault();
                void save();
              }}
            >
              上限 $
              <input className="pith-field num" type="number" min={1} max={50} step={1} value={budget} onChange={(e) => setBudget(e.target.value)} autoFocus />
              <button className="pith-link" type="submit">
                存好
              </button>
              <button className="pith-link" type="button" onClick={() => setEditing(false)}>
                算了
              </button>
            </form>
          ) : (
            <>
              月上限 {money(u.budgetUsd)}，还剩 {money(Math.max(0, u.budgetUsd - u.spentUsd))}
              <button className="pith-link" onClick={() => setEditing(true)}>
                改
              </button>
            </>
          )}
        </p>
        <p className="mdl-hint">所有模型、搜索、转换接口合计；每次发送前按最坏情况估一遍，超了就不发，不逐次问你。</p>
        {u.ledgerError && <p className="mdl-warn selectable">{u.ledgerError}</p>}
      </div>
      <div className="mdl-spend-side">
        <DayBars days={u.days.length ? u.days : [0]} height={46} />
        <ul className="mdl-slices">
          {u.byProvider.map((p) => (
            <li key={p.key}>
              <span>{p.label}</span>
              <i style={{ width: `${(p.usd / total) * 100}%` }} />
              <b className="num">{money(p.usd)}</b>
            </li>
          ))}
          {u.byProvider.length === 0 && <li className="is-empty">这个月还没花钱</li>}
        </ul>
        {u.cache.read + u.cache.input > 0 && (
          <p className="mdl-cache">
            缓存命中 <b className="num">{Math.round(u.cache.hitRate * 100)}%</b>
            <span>命中的部分只收一成左右的钱</span>
          </p>
        )}
      </div>
    </section>
  );
}

function Default({ s, onStatus }: { s: ModelStatus; onStatus: (s: ModelStatus) => void }) {
  const { notify } = useUi();
  const set = async (prefs: ModelStatus["prefs"]) => {
    try {
      onStatus(await api.modelPrefsSet(prefs));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const p = s.providers.find((x) => x.id === s.prefs.provider) ?? s.providers[0];
  const t = p.tiers.find((x) => x.tier === s.prefs.tier) ?? p.tiers[1];
  return (
    <div className="mdl-default">
      <div className="mdl-providers">
        {s.providers.map((x) => (
          <button key={x.id} className={`mdl-provider ${x.id === p.id ? "is-on" : ""} ${x.ready ? "is-ready" : ""}`} onClick={() => void set({ provider: x.id, tier: s.prefs.tier })} disabled={!x.ready && !s.mock}>
            <b>{x.label}</b>
            <span>{x.ready ? "能用" : x.id === "vertex" ? "要登录" : "没填密钥"}</span>
          </button>
        ))}
      </div>
      <div className="seg mdl-seg">
        {p.tiers.map((x) => (
          <button key={x.tier} className={x.tier === t.tier ? "is-on" : ""} onClick={() => void set({ provider: p.id, tier: x.tier })}>
            {x.label}
          </button>
        ))}
      </div>
      <p className="mdl-pick">
        <b>{t.modelLabel}</b>
        {t.effort && <span>想得{t.effort === "low" ? "浅" : t.effort === "high" ? "深" : "适中"}</span>}
        <span className="num">
          每百万 token 输入 ${t.input} · 输出 ${t.output} · 缓存读 ${t.cacheRead}
        </span>
      </p>
      <p className="mdl-hint">对话都用这一档；读整章、写长东西时交给强一档的子代理，主对话不换模型（换来换去缓存会白付）。</p>
    </div>
  );
}

function KeyRow({ which, label, has, place, note, onStatus }: { which: ApiKeyWhich; label: string; has: boolean; place: ModelKeyView["place"]; note: string; onStatus: (s: ModelStatus) => void }) {
  const { notify } = useUi();
  const [editing, setEditing] = useState(!has);
  const [key, setKey] = useState("");
  const [appKey, setAppKey] = useState("");
  useEffect(() => setEditing(!has), [has]);
  const fromFile = place === "file";
  const save = async () => {
    try {
      onStatus(await api.modelKeySet(which, key, which === "mathpix" ? appKey : undefined));
      setKey("");
      setAppKey("");
      notify(`${label} 的密钥存好了`);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const clear = async () => {
    try {
      onStatus(await api.modelKeyClear(which));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  return (
    <div className="mdl-key">
      <div className="mdl-key-name">
        <i className={has ? "is-on" : ""} />
        <b>{label}</b>
        <span>{note}</span>
      </div>
      {has && !editing ? (
        <div className="mdl-key-act">
          <span>{PLACE[place]}</span>
          {!fromFile && (
            <>
              <button className="pith-link" onClick={() => setEditing(true)}>
                换
              </button>
              <button className="pith-link" onClick={() => void clear()}>
                清掉
              </button>
            </>
          )}
        </div>
      ) : fromFile ? (
        <span className="mdl-key-act">密钥文件里没有这一把</span>
      ) : (
        <form
          className={`mdl-key-form ${which === "mathpix" ? "is-pair" : ""}`}
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <input className="pith-field selectable" type="password" placeholder={which === "mathpix" ? "app_id" : "粘贴密钥"} value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
          {which === "mathpix" && <input className="pith-field selectable" type="password" placeholder="app_key" value={appKey} onChange={(e) => setAppKey(e.target.value)} autoComplete="off" />}
          <button className="pith-button pith-sweep" type="submit" disabled={!key.trim()}>
            存好
          </button>
          {has && (
            <button className="pith-link" type="button" onClick={() => setEditing(false)}>
              算了
            </button>
          )}
        </form>
      )}
    </div>
  );
}

function Vertex({ s, onStatus }: { s: ModelStatus; onStatus: (s: ModelStatus) => void }) {
  const { notify } = useUi();
  const [project, setProject] = useState(s.vertex.project);
  const [location, setLocation] = useState(s.vertex.location || "global");
  const ready = s.providers.find((p) => p.id === "vertex")?.ready ?? false;
  const dirty = project.trim() !== s.vertex.project || location.trim() !== (s.vertex.location || "global");
  const save = async () => {
    try {
      onStatus(await api.modelVertexSet({ project: project.trim(), location: location.trim() }));
      notify("Vertex AI 的项目存好了");
    } catch (e) {
      notify(String(e), "error");
    }
  };
  return (
    <div className="mdl-key is-vertex">
      <div className="mdl-key-name">
        <i className={ready ? "is-on" : ""} />
        <b>Vertex AI</b>
        <span>Gemini，用 Google Cloud 的项目计费；不用密钥，用 gcloud 登录</span>
      </div>
      <form
        className="mdl-key-form"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <input className="pith-field selectable" placeholder="项目 ID，例如 my-project-123" value={project} onChange={(e) => setProject(e.target.value)} />
        <input className="pith-field selectable mdl-loc" placeholder="区域" value={location} onChange={(e) => setLocation(e.target.value)} />
        <button className="pith-button pith-sweep" type="submit" disabled={!dirty}>
          存好
        </button>
      </form>
      <p className="mdl-adc">
        <span className={s.vertex.gcloud ? "is-on" : ""}>{s.vertex.gcloud ? "找到了 gcloud" : "没找到 gcloud"}</span>
        <span className={s.vertex.adc ? "is-on" : ""}>{s.vertex.adc ? "登录过了" : "还没登录"}</span>
        {!s.vertex.adc && (
          <>
            在终端里跑一次 <code className="selectable">gcloud auth application-default login</code>
          </>
        )}
      </p>
    </div>
  );
}

function Call({ c }: { c: UsageCall }) {
  const when = c.at.slice(5, 16).replace("T", " ");
  const tokens = c.tokens.input + c.tokens.cacheRead + c.tokens.cacheWrite;
  return (
    <div className={`mdl-call ${c.mock ? "is-mock" : ""}`}>
      <span className="num">{when}</span>
      <span>{purpose(c.purpose)}</span>
      <span className="mdl-call-model">{c.model}</span>
      <span className="num">
        {tokens > 0 ? `${(tokens / 1000).toFixed(1)}k → ${(c.tokens.output / 1000).toFixed(1)}k` : ""}
        {c.tokens.cacheRead > 0 && <em> · 缓存 {Math.round((c.tokens.cacheRead / tokens) * 100)}%</em>}
      </span>
      <b className="num">{c.mock ? "模拟" : `${c.estimated ? "≈" : ""}${money(c.costUsd)}`}</b>
    </div>
  );
}
