// 设置 › 概念研究（v0.4，decisions/0024）：模型（默认 GPT-6.1 Sol，可选 Opus 5.5）、推理深浅、专用搜索、PDF 页数上限、单价，
// 技能列表（内置的、用户的、覆盖内置的；在访达中打开技能文件夹），最近的调用。
// 「密钥与费用」里的 OpenAI、Mathpix 两行也在这里（ConceptKeyRows）。
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, FolderOpen, LoaderCircle } from "lucide-react";
import { api, type ConceptSettings, type ConceptStatus } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { Badge, Notice, SectionTitle, Switch } from "@/format/parts";
import { money } from "@/research/labels";
import { EFFORT, EFFORTS, MODEL_NAME, SEARCH_ENGINE } from "@/concept/labels";

const PLACE: Record<string, string> = { keychain: "钥匙串", memory: "只在内存里，退出就没了", file: "来自 PITH_API_KEY_FILE 指的文件" };
const ENTRY: Record<string, string> = { chapter: "整章", selection: "划选", question: "问题", concept: "追踪", card: "卡片" };

/** 密钥与费用页里的 OpenAI、Mathpix */
export function ConceptKeyRows() {
  const qc = useQueryClient();
  const { confirm, notify } = useUi();
  const status = useQuery({ queryKey: ["conceptStatus"], queryFn: api.conceptStatus, staleTime: 0 });
  const [openai, setOpenai] = useState("");
  const [appId, setAppId] = useState("");
  const [appKey, setAppKey] = useState("");
  const s = status.data;
  if (!s) return null;
  const put = (next: ConceptStatus) => qc.setQueryData(["conceptStatus"], next);
  const save = async (which: "openai" | "mathpix") => {
    try {
      put(which === "openai" ? await api.conceptKeySet("openai", openai.trim()) : await api.conceptKeySet("mathpix", appId.trim(), appKey.trim()));
      setOpenai("");
      setAppId("");
      setAppKey("");
      notify(`${which === "openai" ? "OpenAI" : "Mathpix"} 的密钥已保存`);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const clear = async (which: "openai" | "mathpix", label: string) => {
    if (!(await confirm({ title: `清除 ${label} 的密钥？`, message: "清除后用到它的概念研究发不了，要用时再填。", confirm: "清除", danger: true }))) return;
    try {
      put(await api.conceptKeyClear(which));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  return (
    <>
      {s.keys.map((k) => (
        <div key={k.which} className="contents">
          <dt className="pt-0.5 text-muted-foreground">{k.label}</dt>
          <dd className="min-w-0">
            <p className="flex items-center gap-1.5">
              {k.has ? <CircleCheck size={13} className="shrink-0 text-emerald-600" /> : <CircleAlert size={13} className={`shrink-0 ${k.needed && k.which === "openai" ? "text-amber-600" : "text-muted-foreground"}`} />}
              <span>{k.has ? "已填" : "还没有填"}</span>
              <span className="text-xs text-muted-foreground">（{PLACE[k.place]}）</span>
              <Badge>概念研究</Badge>
            </p>
            {k.place !== "file" && (
              <form
                className="mt-1 flex gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  void save(k.which);
                }}
              >
                {k.which === "openai" ? (
                  <input type="password" value={openai} onChange={(e) => setOpenai(e.target.value)} placeholder={k.has ? "填新的密钥来替换" : "sk-…"} autoComplete="off" className="min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring" />
                ) : (
                  <>
                    <input value={appId} onChange={(e) => setAppId(e.target.value)} placeholder="app_id" autoComplete="off" className="w-36 min-w-0 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring" />
                    <input type="password" value={appKey} onChange={(e) => setAppKey(e.target.value)} placeholder={k.has ? "填新的 app_key 来替换" : "app_key"} autoComplete="off" className="min-w-0 flex-1 rounded-md border bg-background px-2 py-1 font-mono text-xs outline-none focus:border-ring" />
                  </>
                )}
                <button type="submit" disabled={k.which === "openai" ? !openai.trim() : !appId.trim() || !appKey.trim()} className="shrink-0 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-50">
                  保存
                </button>
                <button type="button" onClick={() => void clear(k.which, k.label)} disabled={!k.has} className="shrink-0 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50">
                  清除
                </button>
              </form>
            )}
            <p className="mt-0.5 text-xs text-muted-foreground">
              {k.which === "openai"
                ? "platform.openai.com › API keys（账号要能用 gpt-6.1-sol）。概念研究的默认模型和它的网页搜索用这一把。"
                : "console.mathpix.com 的 app_id 和 app_key：短 PDF 转文字用，每页 $0.005（开通第一个 API 密钥要一次性付 $19.99）。不填就用本机取文字。"}
            </p>
          </dd>
        </div>
      ))}
    </>
  );
}

export function ConceptSettingsSection({ onKeys }: { onKeys: () => void }) {
  const qc = useQueryClient();
  const ui = useUi();
  const status = useQuery({ queryKey: ["conceptStatus"], queryFn: api.conceptStatus, staleTime: 0 });
  const [saving, setSaving] = useState(false);
  const s = status.data;
  if (status.isLoading) return <LoaderCircle size={14} className="animate-spin text-muted-foreground" />;
  if (status.error) return <p className="selectable text-xs text-destructive">{String(status.error)}</p>;
  if (!s) return null;
  const set = async (patch: Partial<ConceptSettings>) => {
    setSaving(true);
    try {
      qc.setQueryData(["conceptStatus"], await api.conceptSettingsSet({ ...s.settings, ...patch }));
    } catch (e) {
      ui.notify(String(e), "error");
    } finally {
      setSaving(false);
    }
  };
  const missing = s.keys.filter((k) => k.needed && !k.has && k.which === "openai");
  return (
    <div className="space-y-6">
      <p className="text-xs text-muted-foreground">
        读书、做卡片用的模型和联网。每次发送前都会列出要发的段落、步骤和按最坏情况算的费用，确认后才发；费用和排版、调查共用月度上限。
      </p>
      {s.mock && <Notice tone="info" title="开发版和测试用模拟接口：不联网、不花钱。下面的设置在换成真接口时用到。" />}
      {!s.mock && missing.length > 0 && s.provider === "openai" && (
        <Notice title="还没有填 OpenAI 的密钥。">
          <button onClick={onKeys} className="text-primary hover:underline">
            去「密钥与费用」
          </button>
        </Notice>
      )}
      <dl className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-x-3 gap-y-3 text-sm">
        <dt className="pt-1 text-muted-foreground">模型</dt>
        <dd className="text-xs">
          <div className="flex gap-1">
            {["gpt-6.1-sol", "claude-opus-5-5"].map((m) => (
              <button key={m} disabled={saving} onClick={() => void set({ model: m })} className={`rounded-md border px-2.5 py-1 ${s.settings.model === m ? "border-primary text-primary" : "hover:bg-muted"}`}>
                {MODEL_NAME[m]}
                {m === "gpt-6.1-sol" && "（默认）"}
              </button>
            ))}
          </div>
          <p className="mt-1 text-muted-foreground tabular-nums">
            单价：输入 ${s.priceInput} / 缓存输入 ${s.priceCached} / 输出 ${s.priceOutput}（每百万 token）
            {s.settings.model === "gpt-6.1-sol" && `；单次输入超过 ${(s.longContextTokens / 1000).toFixed(0)}K 时整次按输入 2 倍、输出 1.5 倍`}
          </p>
        </dd>
        <dt className="pt-1 text-muted-foreground">推理深浅</dt>
        <dd className="text-xs">
          <div className="flex flex-wrap gap-1">
            {EFFORTS.map((e) => (
              <button key={e} disabled={saving || s.provider !== "openai"} onClick={() => void set({ effort: e })} className={`rounded-md border px-2 py-1 disabled:opacity-50 ${s.settings.effort === e ? "border-primary text-primary" : "hover:bg-muted"}`}>
                {EFFORT[e]}
              </button>
            ))}
          </div>
          <p className="mt-1 text-muted-foreground">OpenAI 的 reasoning.effort。推理的 token 也算输出、占每步的输出上限：越深越贵，太深可能写到上限被截断。Opus 5.5 固定用 medium。</p>
        </dd>
        <dt className="pt-1 text-muted-foreground">专用搜索</dt>
        <dd className="text-xs">
          <label className="flex items-center gap-2">
            <Switch on={s.settings.dedicatedSearch} disabled={saving} onChange={(v) => void set({ dedicatedSearch: v })} />
            <span>反方「也上网找」时用模型自带的网页搜索（现在：{SEARCH_ENGINE[s.searchEngine] ?? s.searchEngine}）</span>
          </label>
          <p className="mt-1 text-muted-foreground tabular-nums">OpenAI 网页搜索每次 ${s.webSearchUsd}，搜到的内容按输入 token 计；关掉就用 Tavily（要填 Tavily 的密钥）。下载网页照旧在这台电脑上做，存快照。</p>
        </dd>
        <dt className="pt-1 text-muted-foreground">PDF 页数上限</dt>
        <dd className="text-xs">
          <form
            className="flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              const v = Number(new FormData(e.currentTarget).get("limit"));
              if (Number.isFinite(v)) void set({ pdfPageLimit: Math.round(v) });
            }}
          >
            <input name="limit" type="number" min={1} max={2000} defaultValue={s.settings.pdfPageLimit} className="w-20 rounded-md border bg-background px-2 py-1 tabular-nums outline-none focus:border-ring" />
            <button type="submit" className="rounded-md border px-2.5 py-1 hover:bg-muted">
              保存
            </button>
            <span className="text-muted-foreground tabular-nums">超过就提示改用 EPUB；Mathpix 每页 ${s.mathpixPageUsd}</span>
          </form>
        </dd>
      </dl>

      <section>
        <SectionTitle
          right={
            <button onClick={() => void api.conceptSkillsReveal().catch((e) => ui.notify(String(e), "error"))} className="flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs hover:bg-muted">
              <FolderOpen size={12} />
              在访达中打开技能文件夹
            </button>
          }
        >
          技能（{s.skills.length}）
        </SectionTitle>
        <p className="mb-2 text-xs text-muted-foreground">
          技能是一个文件夹里的 SKILL.md：开头写名字、入口、产出和步骤，正文写每步的做法。自己的技能放在 <span className="selectable font-mono">{s.skillsDir}</span>；和内置技能同名的覆盖内置的。
        </p>
        <ul className="space-y-1.5 text-xs">
          {s.skills.map((k) => (
            <li key={k.name} className="rounded-md border px-3 py-2">
              <p className="flex items-center gap-2">
                <span className="font-medium">{k.title}</span>
                <span className="font-mono text-muted-foreground">{k.name}</span>
                <Badge tone={k.origin === "user" ? "accent" : "plain"}>{k.overridesBuiltin ? "用户的（覆盖内置的）" : k.origin === "user" ? "用户的" : "内置"}</Badge>
                <span className="ml-auto text-muted-foreground">入口：{k.entries.map((e) => ENTRY[e]).join("、")}</span>
              </p>
              <p className="text-muted-foreground">{k.description}</p>
              <p className="text-muted-foreground">
                步骤：
                {k.steps.map((st) => `${st.title || st.id}${st.kind === "library" ? "（查文献库）" : st.kind === "web" ? "（网页检索）" : ""}${st.optional ? "（可选）" : ""}`).join(" → ")}
              </p>
            </li>
          ))}
        </ul>
        {s.brokenSkills.length > 0 && <Notice tone="danger" title="这些技能读不了（格式不对），没有列进来：" items={s.brokenSkills.map((b) => `${b.folder}：${b.error}`)} />}
      </section>

      {s.calls.length > 0 && (
        <section>
          <SectionTitle>最近的调用</SectionTitle>
          <table className="w-full text-left text-xs">
            <tbody>
              {s.calls.map((c, i) => (
                <tr key={i} className="border-b border-border/60">
                  <td className="py-1 pr-2 text-muted-foreground tabular-nums">{c.at.replace("T", " ").slice(0, 16)}</td>
                  <td className="py-1 pr-2">{c.provider}</td>
                  <td className="py-1 pr-2">{c.model}</td>
                  <td className="py-1 pr-2 text-muted-foreground">{c.note}</td>
                  <td className="py-1 text-right tabular-nums">{c.mock ? "模拟" : money(c.costUsd)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}
