// 设置（R1）：左边是常驻的枝桠（每次进设置长一遍），八页：角色、技能、知识库与检索、模型与费用、Word 与引文、导入、数据、外观。
// 原来八页的内容（SettingsPage 的各块）分进这几页；角色、外观、知识库的管理是 1.0.2 新的，导入 1.0.3 从文献库挪过来。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type Role } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { PALETTES, paletteVars, useLook, type ThemePref } from "@/shared/theme";
import { useNav, roleTone, type SetSection } from "@/app/nav";
import { branch, labelDelay } from "@/app/branch";
import { SettingsPage, type SettingsSection } from "@/settings/SettingsPage";
import { ImportPage } from "@/import/ImportPage";
import { ModelSettings } from "@/settings/ModelSettings";
import { warmWater } from "@/app/water";

const PAGES: { id: SetSection; name: string; line: string; blocks?: [SettingsSection, string][] }[] = [
  { id: "roles", name: "角色", line: "请谁来答：每个角色有自己的说话方式、默认的知识库和常用的技能。" },
  { id: "skills", name: "技能", line: "对话里用 / 点名的技能：用哪个模型、想得多深、怎么检索。", blocks: [["research", "事实调查"], ["concept", "概念研究"], ["format", "排版"]] },
  { id: "kb", name: "知识库与检索", line: "知识库是文献库和对话之间的一层；回答时在你勾的库里找。" },
  { id: "keys", name: "模型与费用", line: "用哪家的模型、每个月最多花多少；花了多少一直看得见。" },
  { id: "word", name: "Word 与引文", line: "Word 里的 Pith 选项卡、引文样式、把 Zotero 的引文换过来。", blocks: [["word", ""]] },
  { id: "import", name: "导入", line: "把 Zotero 库、BibTeX、RIS 里的文献搬进文献库；原来的文件一个字也不动。" },
  { id: "data", name: "数据", line: "数据放在哪、备份和恢复、整库导出、联网补全。", blocks: [["general", "数据目录"], ["backup", "备份与导出"], ["library", "联网补全"]] },
  { id: "look", name: "外观", line: "两套配色，随时换；换的时候颜色像墨一样慢慢洇过去。" },
];

const STEP = 46;
const NODES = [{ kind: "g" as const, y: 36 }, ...PAGES.map((_, i) => ({ kind: "i" as const, y: 92 + i * STEP }))];

export function SettingsScreen({ onRestored }: { onRestored: () => void }) {
  const nav = useNav();
  const page = PAGES.find((p) => p.id === nav.setSection) ?? PAGES[0];
  const on = PAGES.indexOf(page);
  const { paths, dots } = branch(10, 470, NODES.map((n, k) => ({ ...n, on: k - 1 === on })), { lead: 0.1, step: 0.06 });

  return (
    <div className="set page-enter">
      <nav className="set-nav no-swipe" aria-label="设置">
        <svg className="br-svg" width={280} height={560} viewBox="0 0 280 560" style={{ top: 20 }} aria-hidden="true">
          {paths.map((p, i) => (
            <path key={i} className={p.cls} pathLength={1} d={p.d} style={{ "--d": `${p.delay}s` } as React.CSSProperties} />
          ))}
          {dots.map((d, i) => (
            <circle key={i} className={d.on ? "on" : ""} cx={d.cx} cy={d.cy} r={2.8} style={{ "--d": `${d.delay}s` } as React.CSSProperties} />
          ))}
        </svg>
        <p className="br-h" style={{ top: 36 - 12 + 20, "--d": `${labelDelay(0, 0.1, 0.06)}s` } as React.CSSProperties}>
          设置
        </p>
        {PAGES.map((p, i) => (
          <button
            key={p.id}
            className={`br-st pith-row ${p.id === page.id ? "is-on" : ""}`}
            style={{ top: 92 + i * STEP - 11 + 20, "--d": `${labelDelay(i + 1, 0.1, 0.06)}s` } as React.CSSProperties}
            onClick={() => nav.openSettings(p.id)}
          >
            {p.name}
          </button>
        ))}
      </nav>
      <div className="set-main">
        <div className="inner" key={page.id}>
          <p className="set-k">设置</p>
          <h2 className="set-h2">{page.name}</h2>
          <p className="set-line">{page.line}</p>
          {page.id === "roles" && <Roles />}
          {page.id === "kb" && <KnowledgeBases />}
          {page.id === "look" && <Appearance />}
          {page.id === "keys" && <ModelSettings />}
          {page.id === "import" && <ImportPage onDone={() => nav.go("lib")} />}
          {page.blocks?.map(([s, title]) => (
            <section key={s} className="set-block">
              {title && <h3 className="set-block-title">{title}</h3>}
              <SettingsPage section={s} onKeys={() => nav.openSettings("keys")} onRestored={onRestored} />
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}

// ---------- 角色 ----------

const EMPTY: Role = { uid: "", name: "", mark: "", line: "", prompt: "", kb: "", model: "", skills: [], builtin: false, edited: false };

function Roles() {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const roles = useQuery({ queryKey: ["roles"], queryFn: api.roleList });
  const list = roles.data ?? [];
  const [sel, setSel] = useState<string>("pith");
  const [draft, setDraft] = useState<Role | null>(null);
  const r = list.find((x) => x.uid === sel) ?? list[0];

  const save = async () => {
    if (!draft) return;
    try {
      const saved = await api.roleSave(draft);
      await qc.invalidateQueries({ queryKey: ["roles"] });
      setSel(saved.uid);
      setDraft(null);
    } catch (e) {
      ui.notify(String(e), "error");
    }
  };
  const remove = async () => {
    if (!r) return;
    const ok = await ui.confirm(
      r.builtin
        ? { title: `把「${r.name}」改回默认？`, message: "你对它的修改会去掉。", confirm: "改回默认" }
        : { title: `删掉「${r.name}」？`, message: "用过它的对话还在。", confirm: "删掉", danger: true },
    );
    if (!ok) return;
    await api.roleDelete(r.uid);
    await qc.invalidateQueries({ queryKey: ["roles"] });
    if (!r.builtin) setSel("pith");
  };

  return (
    <>
      <div className="people">
        {list.map((x, i) => (
          <button key={x.uid} className={`person ${x.uid === r?.uid && !draft ? "is-on" : ""}`} onClick={() => (setSel(x.uid), setDraft(null))} style={{ "--tone": roleTone(i) } as React.CSSProperties}>
            <i>{x.mark}</i>
            {x.name}
          </button>
        ))}
        <button className={`person add ${draft && !draft.uid ? "is-on" : ""}`} onClick={() => setDraft({ ...EMPTY })}>
          <i>＋</i>
          新角色
        </button>
      </div>
      {draft ? (
        <section className="mt-8 max-w-[720px] space-y-3">
          <p className="set-k">{draft.uid ? "改这个角色" : "新角色"}</p>
          <div className="grid grid-cols-[1fr_90px] gap-3">
            <input className="pith-field" placeholder="名字，例如：导师" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
            <input className="pith-field" placeholder="头像字" value={draft.mark} maxLength={2} onChange={(e) => setDraft({ ...draft, mark: e.target.value })} />
          </div>
          <input className="pith-field" placeholder="一句话：什么时候请它" value={draft.line} onChange={(e) => setDraft({ ...draft, line: e.target.value })} />
          <textarea className="pith-field" placeholder="它是谁、怎么说话、先做什么后做什么（工具和边界由 Pith 管，这里只写做事方式）" value={draft.prompt} onChange={(e) => setDraft({ ...draft, prompt: e.target.value })} />
          <div className="grid grid-cols-2 gap-3">
            <input className="pith-field" placeholder="默认知识库，例如：在读的那本书" value={draft.kb} onChange={(e) => setDraft({ ...draft, kb: e.target.value })} />
            <input className="pith-field" placeholder="偏好的模型，例如 anthropic/claude-opus-5-5（空着跟设置）" value={draft.model} onChange={(e) => setDraft({ ...draft, model: e.target.value })} />
          </div>
          <input className="pith-field" placeholder="常用技能，用顿号隔开" value={draft.skills.join("、")} onChange={(e) => setDraft({ ...draft, skills: e.target.value.split(/[、,，]/).map((s) => s.trim()).filter(Boolean) })} />
          <div className="flex gap-2 pt-2">
            <button className="pith-button pith-sweep" onClick={save} disabled={!draft.name.trim()}>
              存好
            </button>
            <button className="pith-button pith-button--quiet" onClick={() => setDraft(null)}>
              算了
            </button>
          </div>
        </section>
      ) : (
        r && (
          <section className="max-w-[760px]">
            <p className="role-k">
              <span className="set-k">角色{r.edited ? " · 改过" : ""}</span>
              <button
                className="pith-link"
                onClick={() => {
                  nav.openChat(null);
                  setTimeout(() => window.dispatchEvent(new CustomEvent("pith:compose", { detail: { role: r.uid } })), 50);
                }}
              >
                和它开始对话 →
              </button>
            </p>
            <h3 className="set-h2">{r.name}</h3>
            <p className="set-line !mb-0">{r.line}</p>
            <p className="role-desc selectable">{r.prompt}</p>
            <dl className="kv">
              <dt>默认知识库</dt>
              <dd>{r.kb || "跟着对话"}</dd>
              <dt>偏好的模型</dt>
              <dd>{r.model || "跟随全局"}</dd>
              <dt>常用技能</dt>
              <dd>{r.skills.length ? r.skills.join(" · ") : "全部"}</dd>
            </dl>
            <div className="mt-5 flex gap-2">
              <button className="pith-button pith-button--quiet" onClick={() => setDraft({ ...r })}>
                改
              </button>
              {(!r.builtin || r.edited) && (
                <button className="pith-button pith-button--quiet" onClick={remove}>
                  {r.builtin ? "改回默认" : "删掉"}
                </button>
              )}
            </div>
          </section>
        )
      )}
    </>
  );
}

// ---------- 知识库 ----------

function KnowledgeBases() {
  const nav = useNav();
  const ui = useUi();
  const qc = useQueryClient();
  const kbs = useQuery({ queryKey: ["kbs"], queryFn: api.kbList });
  const refresh = () => qc.invalidateQueries({ queryKey: ["kbs"] });
  const create = async () => {
    const name = await ui.prompt({ title: "新建知识库", label: "名字", confirm: "建好" });
    if (!name) return;
    await api.kbCreate(name, "").catch((e) => ui.notify(String(e), "error"));
    void refresh();
  };
  return (
    <>
      {(kbs.data ?? []).map((k) => (
        <div key={k.uid} className="kb-row">
          <div>
            <b>{k.name}</b>
            <p>
              {k.count} 份{k.note ? ` · ${k.note}` : ""}
            </p>
          </div>
          <div className="flex gap-3">
            <button className="pith-link" onClick={() => nav.openKb(k.uid)}>
              打开
            </button>
            <button
              className="pith-link"
              onClick={async () => {
                const name = await ui.prompt({ title: "改名", initial: k.name, confirm: "改名" });
                if (name === null) return;
                const note = await ui.prompt({ title: "一句说明（可以空着）", initial: k.note, confirm: "存好" });
                await api.kbUpdate(k.uid, name, note ?? k.note).catch((e) => ui.notify(String(e), "error"));
                void refresh();
              }}
            >
              改名
            </button>
            <button
              className="pith-link"
              onClick={async () => {
                const ok = await ui.confirm({ title: `删掉「${k.name}」？`, message: `里面的 ${k.count} 份材料一起拿掉；原来的书、调查、对话都还在。`, confirm: "删掉", danger: true });
                if (!ok) return;
                await api.kbDelete(k.uid);
                void refresh();
              }}
            >
              删掉
            </button>
          </div>
        </div>
      ))}
      {kbs.data?.length === 0 && <p className="set-line">还没有知识库。</p>}
      <button className="pith-button pith-sweep mt-5" onClick={create}>
        ＋ 新知识库
      </button>
      <p className="set-h3">检索</p>
      <p className="text-sm text-muted-foreground">
        对话已经接上模型，但还不会在知识库里找（1.1.0 第 3 期接上：段落块、本机向量和重排、读原文、引用编号）。本机向量模型的设置在 设置 › 技能 › 概念研究。
      </p>
    </>
  );
}

// ---------- 外观 ----------

const LEAF_MINI = (
  <svg className="lv" viewBox="0 0 200 60" aria-hidden="true">
    <path d="M6 32Q14 30 22 30L190 30" className="m" />
    <path d="M22 30C60 6 140 4 196 30C140 56 60 54 22 30Z" className="o" />
    <path d="M40 30Q48 18 62 14M40 30Q48 42 62 46M70 30Q78 16 94 11M70 30Q78 44 94 49M100 30Q108 17 124 12M100 30Q108 43 124 48M130 30Q138 20 152 16M130 30Q138 40 152 44M158 30Q164 24 174 22M158 30Q164 36 174 38" className="v" />
  </svg>
);

function Appearance() {
  const nav = useNav();
  const [look, setLook] = useLook();
  useEffect(warmWater, []);
  const [theme, setTheme] = useState(document.documentElement.dataset.theme ?? "light");
  useEffect(() => setTheme(document.documentElement.dataset.theme ?? "light"), [look]);
  return (
    <>
      <div className="pals">
        {PALETTES.map((p) => (
          <button
            key={p.id}
            className={`pal-card ${look.palette === p.id ? "is-on" : ""}`}
            onClick={(e) => {
              if (look.palette === p.id) return;
              nav.lake(e.clientX, e.clientY);
              setLook({ ...look, palette: p.id }, { x: e.clientX, y: e.clientY });
            }}
            aria-label={`换成${p.name}`}
          >
            <span className="pv" style={paletteVars(p.id, theme === "dark" ? "dark" : "light")}>
              {LEAF_MINI}
              <b>今天想弄清什么？</b>
              <i className="in">
                问 Pith……<em>→</em>
              </i>
              <i className="bt">存进知识库</i>
            </span>
            <span className="nm">
              <b>{p.name}</b>
              <span>{p.line}</span>
              <i className="ck" />
            </span>
          </button>
        ))}
      </div>
      <p className="set-h3">明暗</p>
      <div className="seg">
        {(
          [
            ["system", "跟随系统"],
            ["light", "亮"],
            ["dark", "暗"],
          ] as [ThemePref, string][]
        ).map(([t, n]) => (
          <button
            key={t}
            className={look.theme === t ? "is-on" : ""}
            onClick={(e) => {
              if (look.theme === t) return;
              nav.lake(e.clientX, e.clientY);
              setLook({ ...look, theme: t }, { x: e.clientX, y: e.clientY });
            }}
          >
            {n}
          </button>
        ))}
      </div>
      <p className="mt-6 text-[13.5px] text-muted-foreground">也可以 ⌘K 里打「配色」「暗」。记在这台电脑上。</p>
    </>
  );
}
