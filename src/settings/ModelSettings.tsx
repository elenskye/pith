// 设置 › 模型与费用（1.1.0 照 R1 重画，decisions/0036；验收时改，decisions/0041）：本月花了多少（大圈、每天的细柱、各家、缓存命中、月上限）、
// 默认模型（对话用的、子代理用的，两层的选框）、各家（密钥、模型清单、每个模型想不想想多深、刷新、免费额度；
// Vertex AI 填项目和区域、看 gcloud 登录了没有）、最近的几笔。
// 大留白、大字、少按钮：填过的密钥只露「换」「清掉」；每个模型只露「自定义」，点开的改动立刻存。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type ApiKeyWhich, type ModelChoice, type ModelKeyView, type ModelProviderView, type ModelStatus, type ModelView, type UsageCall } from "@/shared/ipc";
import { EFFORT, ModelPicker, effortLabel, findModel, price, thinkLabel } from "@/chat/ModelPicker";
import { useUi } from "@/shared/ui";
import { money } from "@/research/labels";
import { UsageRing, DayBars } from "@/chat/Usage";

const PLACE: Record<string, string> = { keychain: "存在钥匙串", memory: "只在这次运行里", file: "来自密钥文件" };
const PURPOSE: Record<string, string> = { chat: "对话", chat_subagent: "子代理", chat_web: "联网", model_probe: "刷新模型清单", extract: "排版", plan: "排版" };
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
      <Defaults s={s} onStatus={put} />
      <p className="set-h3">各家</p>
      {s.providers.map((p) => (
        <ProviderBlock key={p.id} s={s} p={p} onStatus={put} />
      ))}
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

function useSet(onStatus: (s: ModelStatus) => void) {
  const { notify } = useUi();
  return async (f: () => Promise<ModelStatus>, ok?: string) => {
    try {
      onStatus(await f());
      if (ok) notify(ok);
    } catch (e) {
      notify(String(e), "error");
    }
  };
}

/** 对话用哪个模型、子代理用哪个（默认：对话那一家最强的） */
function Defaults({ s, onStatus }: { s: ModelStatus; onStatus: (s: ModelStatus) => void }) {
  const run = useSet(onStatus);
  const main = findModel(s, s.prefs.model);
  const sub = findModel(s, s.prefs.subagentUsed);
  const subSet = s.prefs.subagent && findModel(s, s.prefs.subagent)?.p.id === main?.p.id ? s.prefs.subagent : null;
  const setMain = (model: string | null) => {
    if (!model) return;
    const same = findModel(s, model)?.p.id === main?.p.id;
    void run(() => api.modelPrefsSet({ model, subagent: same ? subSet : null }));
  };
  const setSub = (subagent: string | null) => void run(() => api.modelPrefsSet({ model: s.prefs.model, subagent }));
  return (
    <div className="mdl-defaults">
      <div className="mdl-def">
        <span className="mdl-def-k">对话</span>
        <ModelPicker status={s} value={s.prefs.model} onPick={setMain} />
        {main && <Brief m={main.m} />}
      </div>
      <div className="mdl-def">
        <span className="mdl-def-k">读整章、写长东西</span>
        <ModelPicker status={s} value={subSet} onPick={setSub} only={main?.p.id} auto={{ label: `自动：${sub?.m.label ?? "这一家最强的"}`, hint: "跟着对话那一家，用它最强的" }} />
        {sub && <Brief m={sub.m} />}
      </div>
      <p className="mdl-hint">对话从头到尾用一个模型（中途换，缓存会白付）；读整章、写长东西交给子代理。每个模型想不想、想多深，在下面各家的清单里定，对话、子代理和别处都照它。</p>
    </div>
  );
}

function Brief({ m }: { m: ModelView }) {
  return (
    <span className="mdl-brief">
      <span>{thinkLabel(m)}</span>
      <span className="num">{m.priced ? `每百万 token 输入 $${m.input} · 输出 $${m.output} · 缓存读 $${m.cacheRead}` : "价格表里没有，按最贵的估"}</span>
    </span>
  );
}

const when = (iso: string | null) => (iso ? `${Number(iso.slice(5, 7))} 月 ${Number(iso.slice(8, 10))} 日` : "");

/** 一家：密钥（Vertex 是项目和登录）、免费额度、模型清单、刷新 */
function ProviderBlock({ s, p, onStatus }: { s: ModelStatus; p: ModelProviderView; onStatus: (s: ModelStatus) => void }) {
  const run = useSet(onStatus);
  const [refreshing, setRefreshing] = useState(false);
  const [more, setMore] = useState(false);
  const usable = p.ready || s.mock;
  const own = p.models.filter((m) => !m.found);
  const found = p.models.filter((m) => m.found);
  const refresh = async () => {
    setRefreshing(true);
    await run(() => api.modelRefresh(p.id));
    setRefreshing(false);
  };
  return (
    <section className={`mdl-prov ${usable ? "" : "is-off"}`}>
      {p.key ? (
        <KeyRow which={p.key} label={p.label} has={p.ready} place={p.keyPlace ?? "memory"} note={usable ? "" : own.map((m) => m.label).join(" · ")} onStatus={onStatus} />
      ) : (
        <Vertex s={s} onStatus={onStatus} />
      )}
      {(usable || p.refreshNote) && (
        <div className="mdl-prov-bar">
          <span>
            {p.models.length} 个模型
            {p.refreshedAt ? ` · ${when(p.refreshedAt)}刷新过` : " · Pith 自带的清单"}
          </span>
          {!s.mock && (p.ready || p.id === "vertex") && (
            <button className="pith-link" onClick={() => void refresh()} disabled={refreshing}>
              {refreshing ? "正在问…" : "刷新"}
            </button>
          )}
          {p.free !== null && (
            <label className="mdl-free">
              <input type="checkbox" className="pith-check" checked={p.free} onChange={(e) => void run(() => api.modelFreeSet(p.id, e.target.checked))} />
              用免费额度
              <em>账本记 0、不占月上限</em>
            </label>
          )}
        </div>
      )}
      {p.refreshNote && <p className="mdl-note selectable">上次刷新没成：{p.refreshNote}</p>}
      {usable && (
        <div className="mdl-models">
          {own.map((m) => (
            <ModelRow key={m.id} m={m} onStatus={onStatus} />
          ))}
          {found.length > 0 && (
            <button className="mdl-more" onClick={() => setMore(!more)} aria-expanded={more}>
              {more ? "收起" : `还有 ${found.length} 个`}
              <em>刷新时从接口拿到的</em>
            </button>
          )}
          {more && found.map((m) => <ModelRow key={m.id} m={m} onStatus={onStatus} />)}
        </div>
      )}
    </section>
  );
}

const CAP: Record<ModelView["thinking"], string> = { always: "一直想", switch: "能关", never: "不想" };

/** 一个模型：名字、想不想想多深、价；「自定义」点开改，改了立刻存 */
function ModelRow({ m, onStatus }: { m: ModelView; onStatus: (s: ModelStatus) => void }) {
  const run = useSet(onStatus);
  const [open, setOpen] = useState(false);
  const set = (choice: ModelChoice | null) => void run(() => api.modelThinkingSet(m.id, choice));
  const levels = m.efforts.length;
  return (
    <div className={`mdl-model ${open ? "is-open" : ""}`}>
      <div className="mdl-model-row">
        <b>{m.label}</b>
        <span className="mdl-cap">
          {CAP[m.thinking]}
          {levels > 0 && ` · ${levels} 档`}
          {m.source !== "builtin" && <em>{m.source === "api" ? "接口报的" : "推断的"}</em>}
        </span>
        <span className={`mdl-now ${m.custom ? "is-custom" : ""}`}>{thinkLabel(m)}</span>
        <span className="mdl-price num">{price(m)}</span>
        {m.thinking !== "never" ? (
          <button className="pith-link" onClick={() => setOpen(!open)} aria-expanded={open}>
            {open ? "收起" : "自定义"}
          </button>
        ) : (
          <span />
        )}
      </div>
      {open && (
        <div className="mdl-custom">
          {m.thinking === "switch" && (
            <div className="seg">
              <button className={m.choice.on ? "is-on" : ""} onClick={() => set({ on: true, effort: m.choice.effort ?? m.defaultChoice.effort })}>
                想
              </button>
              <button className={!m.choice.on ? "is-on" : ""} onClick={() => set({ on: false, effort: m.choice.effort })}>
                不想
              </button>
            </div>
          )}
          {levels > 0 && (
            <div className={`seg ${m.choice.on ? "" : "is-dim"}`} aria-label="想多深">
              {m.efforts.map((e) => (
                <button key={e} className={m.choice.on && m.choice.effort === e ? "is-on" : ""} onClick={() => set({ on: true, effort: e })} title={e}>
                  {EFFORT[e] ?? e}
                </button>
              ))}
            </div>
          )}
          <span className="mdl-custom-def">
            默认：{m.defaultChoice.on ? `想 · ${effortLabel(m.defaultChoice.effort)}` : "不想"}
            {m.custom && (
              <button className="pith-link" onClick={() => set(null)}>
                恢复默认
              </button>
            )}
          </span>
        </div>
      )}
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
