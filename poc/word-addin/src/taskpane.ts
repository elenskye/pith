// 任务窗格：搜索本地文献 → 插入引文 → 生成书目 → 切换样式 → 刷新。
// 计算全在 refresh.ts（与 docx 直写、VBA 宏共用），这里只管界面与 Word 读写。

import { planRefresh } from "./refresh.ts";
import { DEFAULT_STYLE, LOCALE_FILES, STYLES, styleById } from "./styles.ts";
import { emptyStore, type DocStore } from "./store.ts";
import type { CiteItemRef, CslItem } from "./types.ts";
import * as word from "./word.ts";

type State = {
  library: CslItem[];
  byId: Record<string, CslItem>;
  locales: Record<string, string>;
  styles: Map<string, string>;
  picked: CiteItemRef[];
  editing: string | null;
  smallCaps: boolean;
  busy: boolean;
};

const S: State = { library: [], byId: {}, locales: {}, styles: new Map(), picked: [], editing: null, smallCaps: false, busy: false };
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

function log(msg: string, error = false) {
  const li = document.createElement("li");
  li.textContent = `${new Date().toLocaleTimeString()} ${msg}`;
  if (error) li.className = "err";
  $("log").prepend(li);
}

function label(i: CslItem): string {
  const who = (i.author ?? []).map((a) => a.family ?? a.literal ?? "").filter(Boolean);
  const year = i.issued?.["date-parts"]?.[0]?.[0] ?? "";
  return `${who.length > 2 ? `${who[0]} 等` : who.join("、")} ${year} · ${i.title ?? ""}`;
}

async function styleXml(id: string): Promise<string> {
  if (!S.styles.has(id)) S.styles.set(id, await (await fetch(`/csl/${styleById(id).file}`)).text());
  return S.styles.get(id)!;
}

async function busy<T>(what: string, fn: () => Promise<T>): Promise<T | undefined> {
  if (S.busy) return;
  S.busy = true;
  document.body.classList.add("busy");
  try {
    return await fn();
  } catch (e) {
    const debug = (e as { debugInfo?: unknown }).debugInfo;
    log(`${what}失败：${e instanceof Error ? e.message : String(e)}${debug ? ` ${JSON.stringify(debug)}` : ""}`, true);
  } finally {
    S.busy = false;
    document.body.classList.remove("busy");
  }
}

async function currentStore(): Promise<{ snap: word.DocSnapshot; store: DocStore }> {
  const snap = await word.readDoc();
  if (snap.storeError) throw new Error(snap.storeError);
  return { snap, store: snap.store ?? emptyStore($<HTMLSelectElement>("style").value, new Date().toISOString()) };
}

async function refresh(style?: string) {
  const t0 = performance.now();
  const { snap, store } = await currentStore();
  if (style) store.style = style;
  const plan = planRefresh({
    doc: snap.tags.map((tag, i) => ({ tag, signature: snap.signatures[i] })),
    store,
    library: S.byId,
    styleXml: await styleXml(store.style),
    locales: S.locales,
    newId: () => crypto.randomUUID(),
    now: new Date().toISOString(),
  });
  await word.applyPlan(plan, snap.tags, S.smallCaps);
  $<HTMLSelectElement>("style").value = plan.store.style;
  for (const w of plan.warnings) log(w, true);
  for (const s of plan.skipped) log(`第 ${s.index + 1} 处跳过：${s.reason}`, true);
  const changed = plan.citations.filter((c) => c.changed).length;
  log(`已刷新：${plan.citations.length} 处引文（改写 ${changed}），书目 ${plan.bibliography.texts.length} 条，${Math.round(performance.now() - t0)} ms`);
  $("status").textContent = `样式 ${styleById(plan.store.style).label} · 引文 ${plan.citations.length} 处 · 书目 ${snap.bibliographyCount ? "已插入" : "未插入"}`;
}

function renderPicked() {
  const ul = $("picked");
  ul.innerHTML = "";
  S.picked.forEach((ref, i) => {
    const li = document.createElement("li");
    li.innerHTML = `<span></span><input placeholder="页码" size="4"><button title="移除">×</button>`;
    li.querySelector("span")!.textContent = label(S.byId[ref.id] ?? { id: ref.id, type: "?" });
    const input = li.querySelector("input")!;
    input.value = ref.locator ?? "";
    input.oninput = () => (ref.locator = input.value.trim() || undefined);
    li.querySelector("button")!.onclick = () => {
      S.picked.splice(i, 1);
      renderPicked();
    };
    ul.append(li);
  });
  $<HTMLButtonElement>("insert").disabled = !S.picked.length;
  $("insert").textContent = S.editing ? "更新当前引文" : "插入引文";
}

function renderResults() {
  const q = $<HTMLInputElement>("q").value.trim().toLowerCase();
  const ul = $("results");
  ul.innerHTML = "";
  for (const item of S.library.filter((i) => !q || label(i).toLowerCase().includes(q)).slice(0, 20)) {
    const li = document.createElement("li");
    li.textContent = label(item);
    li.onclick = () => {
      if (!S.picked.some((p) => p.id === item.id)) S.picked.push({ id: item.id });
      renderPicked();
    };
    ul.append(li);
  }
}

async function onSelectionChanged() {
  const id = await word.selectedCitationId().catch(() => null);
  if (id === S.editing) return;
  S.editing = id;
  $("current").hidden = !id;
  if (id) {
    const { store } = await currentStore();
    S.picked = structuredClone(store.citations[id]?.items ?? []);
  } else S.picked = [];
  renderPicked();
}

async function onInsert() {
  await busy("插入引文", async () => {
    const { store } = await currentStore();
    const items = structuredClone(S.picked);
    if (S.editing) {
      store.citations[S.editing] = { ...store.citations[S.editing], items };
      await word.updateStore(store);
    } else {
      const id = crypto.randomUUID();
      store.citations[id] = { items, created: new Date().toISOString() };
      await word.insertCitation(store, id);
      // 下次打开文档时自动显示本面板
      Office.context.document.settings.set("Office.AutoShowTaskpaneWithDocument", true);
      Office.context.document.settings.saveAsync();
    }
    S.picked = [];
    renderPicked();
    await refresh();
  });
}

function mount() {
  $("app").innerHTML = `
    <header><strong>Research Lab 引文</strong><span id="status">正在读取文档…</span></header>
    <section>
      <label>样式 <select id="style">${STYLES.map((s) => `<option value="${s.id}">${s.label}</option>`).join("")}</select></label>
      <div class="row"><button id="refresh">刷新全文</button><button id="bib">插入参考文献表</button></div>
    </section>
    <section>
      <input id="q" placeholder="搜索作者、标题、年份">
      <ul id="results" class="list"></ul>
      <h4>已选文献</h4>
      <ul id="picked" class="list"></ul>
      <div class="row"><button id="insert" disabled>插入引文</button></div>
      <div id="current" hidden><p>光标在一处引文里：可改上面的文献后点「更新当前引文」。</p><button id="del">删除当前引文</button></div>
    </section>
    <section><h4>诊断</h4><pre id="diag"></pre><ul id="log"></ul></section>`;
  $<HTMLSelectElement>("style").value = DEFAULT_STYLE;
  $("q").oninput = renderResults;
  $("refresh").onclick = () => busy("刷新", () => refresh());
  $("style").onchange = () => busy("切换样式", () => refresh($<HTMLSelectElement>("style").value));
  $("bib").onclick = () => busy("插入参考文献表", async () => {
    await word.insertBibliography();
    await refresh();
  });
  $("insert").onclick = onInsert;
  $("del").onclick = () => busy("删除引文", async () => {
    if (!S.editing) return;
    await word.deleteCitation(S.editing);
    S.editing = null;
    await refresh();
  });
}

Office.onReady(async (info) => {
  mount();
  if (info.host !== Office.HostType.Word) {
    $("status").textContent = "只支持 Word";
    return;
  }
  const env = word.envInfo();
  S.smallCaps = env.smallCaps;
  S.library = (await (await fetch("/refs.json")).json()) as CslItem[];
  S.byId = Object.fromEntries(S.library.map((i) => [i.id, i]));
  for (const [lang, file] of Object.entries(LOCALE_FILES)) S.locales[lang] = await (await fetch(`/locales/${file}`)).text();
  renderResults();
  const zotero = await word.scanZoteroFields().catch((e) => ({ error: String(e) }));
  $("diag").textContent = JSON.stringify({ ...env, zotero }, null, 2);
  Office.context.document.addHandlerAsync(Office.EventType.DocumentSelectionChanged, () => void onSelectionChanged());
  await busy("读取文档", () => refresh());
});
