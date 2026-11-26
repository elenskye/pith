// 仅开发用：对话、角色、知识库的假后端（?mock）。内容照 Explore R1（visual-atelier canvases/pith-explore/src/content.mjs）：
// 四个知识库、十种记录、读《国富论》那段对话。规则在 Rust（src-tauri/src/chat/），这里只大致模仿。
import type { Chat, ChatBody, ChatLive, ChatStep, ChatSummary, KnowledgeBase, Material, MaterialKind, Role } from "@/shared/ipc";

type Args = Record<string, unknown>;
const now = () => new Date().toISOString();
const ago = (days: number) => new Date(Date.now() - days * 86400000).toISOString();
let seq = 100;
const uid = (p: string) => `${p}${++seq}`;


const roles: Role[] = [
  { uid: "pith", name: "Pith", mark: "P", line: "研究助手 · 默认", prompt: "你是站主的研究助手。什么都能问：先给结论，再给依据和编号；该用哪个技能自己挑。", kb: "上一次勾的", model: "", skills: [], builtin: true, edited: false },
  { uid: "companion", name: "伴读", mark: "伴", line: "读一本书的时候", prompt: "你陪站主读书：讲清一个概念的来龙去脉，反过来问他，帮他整理摘抄、做概念卡。不急着替他下结论。", kb: "在读的那本书", model: "", skills: ["问这本书", "整章分析", "追踪概念"], builtin: true, edited: false },
  { uid: "reviewer", name: "审稿人", mark: "审", line: "专挑刺", prompt: "你是站主的审稿人。先找这个判断最弱的地方，再找反例和后世的批评；有出处就给编号。", kb: "跟着对话", model: "", skills: ["事实调查", "反方与后世评论", "假设推演"], builtin: true, edited: false },
  { uid: "editor", name: "编辑", mark: "编", line: "写论文的时候", prompt: "你是站主的编辑：守学校的规范和引文格式，润色时保留他的语气；改 Word 之前先把修订给他看。", kb: "毕业论文", model: "", skills: ["排版", "插入引文"], builtin: true, edited: false },
];

const kbs: (Omit<KnowledgeBase, "count">)[] = [
  { uid: "kb-thesis", name: "毕业论文", note: "政治效能感：概念、量表、测量", createdAt: ago(40) },
  { uid: "kb-wn", name: "读《国富论》", note: "斯密 1776，中英对齐。全文、摘抄、笔记，和对话里长出来的东西", createdAt: ago(30) },
  { uid: "kb-gh", name: "读《全球通史》", note: "Stavrianos，在读第一编", createdAt: ago(20) },
  { uid: "kb-misc", name: "随手存", note: "网页和对话里的好回答", createdAt: ago(50) },
];

const m = (kb: string, kind: MaterialKind, title: string, sub: string, body: string, stage: number, days: number, src: Partial<Material> = {}): Material => ({
  uid: uid("m"), kb, kind, title, sub, body, stage, sourceKind: null, sourceRef: null, conversation: null, createdAt: ago(days + 3), tendedAt: ago(days), ...src,
});

const materials: Material[] = [
  m("kb-thesis", "full", "The Voter Decides", "Campbell 等 1954 · 第 11 章", "第 11 章把「政治效能感」定义成一种感觉：个人的政治行动对政治过程有影响，或者能有影响。", 2, 38),
  m("kb-thesis", "full", "Measuring Internal Political Efficacy", "Niemi, Craig & Mattei 1991 · APSR", "用 1988 年全国选举研究的数据，检验内在效能感的几道题。", 2, 36),
  m("kb-thesis", "excerpt", "量表的题目怎么分内外", "我的摘抄 · 12 条", "内在效能：觉得自己懂、能参与；外在效能：觉得政府会回应。", 1, 22),
  m("kb-thesis", "note", "第 2 章的结构", "我的笔记", "先讲概念的来历，再讲两种测量，最后讲中国的数据。", 0, 6),
  m("kb-wn", "full", "The Wealth of Nations", "Smith 1776 · 中英对齐，带段落号", "As it is the power of exchanging that gives occasion to the division of labour, so the extent of this division must always be limited by the extent of that power…", 2, 26, { sourceKind: "concept_text", sourceRef: "T-en" }),
  m("kb-wn", "full", "The Division of Labor is Limited by the Extent of the Market", "Stigler 1951 · JPE 59(3)", "把斯密的定理用到行业的一生：纵向一体化先降后升。", 2, 23),
  m("kb-wn", "excerpt", "Stigler 1951 的摘抄", "我的摘抄 · 8 条", "行业年轻时，企业什么都自己做；行业长大，专门的环节分出去；行业衰退，又收回来自己做。", 1, 21),
  m("kb-wn", "note", "分工、市场和城市", "我的笔记", "为什么搬运工只在大城市里有活干（I.iii.2）。小镇养不起只干一件事的人。", 0, 13),
  m("kb-wn", "web", "Adam Smith’s Moral and Political Philosophy", "plato.stanford.edu", "斯坦福哲学百科里讲斯密的那一条。", 1, 17),
  m("kb-wn", "concept", "看不见的手", "概念卡 · 2 处原文", "只在 IV.ii.9 出现过一次，后人讲得比斯密多。", 1, 10),
  m("kb-wn", "file", "读书报告：分工与市场.docx", "~/Documents/读书报告", "1,800 字的草稿，还没写结论。", 0, 7),
  m("kb-wn", "scan", "第一卷扉页", "1776 年初版影印本 · 已识别文字", "AN INQUIRY INTO THE NATURE AND CAUSES OF THE WEALTH OF NATIONS. IN TWO VOLUMES. VOL. I.", 1, 16),
  m("kb-wn", "report", "斯密怎么评价东印度公司", "事实调查 · 3 条证据", "批评它的垄断：IV.vii 讲殖民地，V.i.e 讲股份公司。", 1, 14),
  m("kb-wn", "wonder", "别针工厂：分工让产量翻几倍", "能拖动的图解 · 三幕", "十个人分十八道工序，一天四万八千枚（I.i.3）。拖动人数，看产量怎么变。", 0, 9),
  m("kb-wn", "answer", "为什么搬运工只在大城市有活干", "伴读 · 2 处原文", "小村子里交换的机会太少，一个人没法只靠搬运过活（I.iii.2）。", 1, 12),
  m("kb-gh", "full", "A Global History", "Stavrianos · 中译本", "第一编：人类在史前的世界。", 1, 15),
  m("kb-gh", "excerpt", "四大古文明的地理条件", "我的摘抄 · 5 条", "大河流域，土地肥，也要治水。", 0, 9),
  m("kb-gh", "answer", "越丰裕越好吗", "伴读", "太丰裕的地方反而不急着组织起来。", 0, 8),
  m("kb-misc", "web", "What is a digital garden", "网页", "笔记不是写完就发，是一直照料着长。", 1, 24),
  m("kb-misc", "answer", "引文格式：APA 第 7 版", "编辑", "学校规范和 APA 第 7 版不一样的三处。", 2, 31),
  m("kb-misc", "web", "Zotero 的八种标注颜色", "网页", "黄、红、绿、蓝、紫、品红、橙、灰。", 0, 4),
];

const Q = "斯密为什么说分工受市场范围限制？";
const wnFull = materials.find((x) => x.title === "The Wealth of Nations")!;
const stigler = materials.find((x) => x.title === "Stigler 1951 的摘抄")!;
const firstReply: ChatBody = {
  steps: [
    { agent: "search", text: "在「读《国富论》」里找了 6 段" },
    { agent: "read", text: "读了第一篇第三章的 3 段" },
    { agent: "read", text: "又看了你记的 Stigler 摘抄" },
  ],
  paragraphs: [
    "斯密的推理只有一步：分工是因为能交换才出现的，所以分工能有多细，就看能交换多少，也就是市场有多大[1]。",
    "市场太小，一个人就没法只干一件事。他举苏格兰高地的散村：农夫得给自家当屠夫、面包师和酿酒人[2]。所以他紧接着讲水运：水路把市场一下子扩大，分工和改良最先出现在沿海和通航的河边[3]。",
    "一百多年后，Stigler 把这句话用到了企业上：行业刚起步、市场还小，企业只好什么都自己做；行业长大了，专门的环节才分出去，成了独立的厂家[4]。",
  ],
  cites: [
    { n: 1, kind: "全文", src: "《国富论》", loc: "I.iii.1", text: "As it is the power of exchanging that gives occasion to the division of labour, so the extent of this division must always be limited by the extent of that power, or, in other words, by the extent of the market.", material: wnFull.uid, kb: "kb-wn" },
    { n: 2, kind: "全文", src: "《国富论》", loc: "I.iii.2", text: "In the lone houses and very small villages which are scattered about in so desert a country as the Highlands of Scotland, every farmer must be butcher, baker and brewer for his own family.", material: wnFull.uid, kb: "kb-wn" },
    { n: 3, kind: "全文", src: "《国富论》", loc: "I.iii.3", text: "…so it is upon the sea-coast, and along the banks of navigable rivers, that industry of every kind naturally begins to subdivide and improve itself…", material: wnFull.uid, kb: "kb-wn" },
    { n: 4, kind: "摘抄", src: "Stigler 1951 的摘抄", loc: "我的摘抄", text: stigler.body, material: stigler.uid, kb: "kb-wn" },
  ],
  ask: { question: "想一想：今天的网购，算不算又把「市场」扩大了一次？", options: ["算，市场变大了", "不算，交换没变", "说不清，想听你讲"] },
  delivery: { kind: "concept", title: "分工受市场范围限制", desc: "分工因交换而起，所以受市场范围限制；高地散村和水运两个例子；Stigler 用到行业的一生。", meta: "概念卡 • Claude Sonnet 5.5", kb: { uid: "kb-wn", name: "读《国富论》" } },
  model: "Claude Sonnet 5.5",
  costUsd: 0.0186,
  ending: "asked",
};

const chats: Chat[] = [
  { uid: "c-wn", title: Q, role: "companion", kbs: ["kb-wn"], skill: null, refUid: null, stage: 0, createdAt: ago(2), updatedAt: ago(0.01), running: false, messages: [
    { position: 1, author: "you", body: { text: Q, role: "companion" }, createdAt: ago(0.02) },
    { position: 2, author: "companion", body: firstReply, createdAt: ago(0.01) },
  ] },
  ...[
    { t: "Stigler 的生命周期论站得住吗", role: "reviewer", days: 2, stage: 1, kbs: ["kb-wn"] },
    { t: "四大古文明发源的地理条件", role: "companion", days: 5, stage: 0, kbs: ["kb-gh"] },
    { t: "第 2 章的引文格式", role: "editor", days: 8, stage: 2, kbs: ["kb-thesis"] },
    { t: "政治效能感最早怎么定义", role: "pith", days: 30, stage: 2, kbs: ["kb-thesis"] },
  ].map((c, i) => ({ uid: `c-old${i}`, title: c.t, role: c.role, kbs: c.kbs, skill: null, refUid: null, stage: c.stage, createdAt: ago(c.days + 1), updatedAt: ago(c.days), running: false, messages: [{ position: 1, author: "you", body: { text: c.t }, createdAt: ago(c.days) }, { position: 2, author: c.role, body: reply(c.t, c.kbs).body, createdAt: ago(c.days) }] })),
];

// ---------- 一回回答（1.1.0）：照 Agent 循环的样子，几秒里一格格出来、一个字一个字写；能停 ----------

type Beat = { at: number; step?: ChatStep; text?: string; reset?: true };
type Sim = { conv: string; author: string; run: string; started: number; beats: Beat[]; end: number; reply: ChatBody };
const sims = new Map<string, Sim>();

/** 这一问的回答：先查、再做交付卡、再写；问题里有「还是」就给一张问题卡 */
function reply(q: string, picked: string[]): { beats: Beat[]; body: ChatBody } {
  const pool = materials.filter((x) => picked.length === 0 || picked.includes(x.kb));
  const hits = pool.filter((x) => [...q].some((ch) => ch.trim() && `${x.title}${x.body}`.includes(ch))).slice(0, 3);
  const name = kbs.find((k) => k.uid === picked[0])?.name;
  const query = q.replace(/[？?。，,！!]/g, "").slice(0, 16);
  const steps: ChatStep[] = [
    { agent: "think", text: "想了想" },
    { agent: "lib", text: `在文献库里找「${query}」：${hits.length} 条` },
  ];
  const either = q.split("还是");
  const ask = either.length === 2 ? { question: "你想先从哪一边看？", options: [either[0].replace(/^.*[，,]/, "").trim() || "前一个", either[1].replace(/[？?]/g, "").trim() || "后一个"] } : null;
  if (!ask) steps.push({ agent: "save", text: `做了一张交付卡：${q.slice(0, 14)}` });
  else steps.push({ agent: "ask", text: "问你一句" });
  steps.push({ agent: "write", text: "写好了" });
  const paragraphs = ask
    ? ["这个问题两头都能讲，先问你一句：你更想先看哪一边？选一个，我接着往下查。"]
    : [
        "这是 ?mock 里的模拟回答：照 Agent 循环的样子走一遍——想、查、做交付卡、写——问题本身没有被回答。",
        hits.length ? `文献库里沾边的有：${hits.map((h) => `**${h.title}**`).join("、")}。` : "文献库里没有找到沾边的条目。",
        "- 真的回答在正式版里（或开发版设 PITH_REAL_MODEL=1）\n- 下面的交付卡点「存进」，种子会飞进知识库",
      ];
  const body: ChatBody = {
    steps,
    paragraphs,
    cites: [],
    ask,
    delivery: ask ? null : { kind: "answer", title: q.slice(0, 40), desc: "模拟回答：查了什么、找到了什么。", meta: "存下的回答 • Claude Sonnet 5.5", kb: name ? { uid: picked[0], name } : null, body: "" },
    model: "Claude Sonnet 5.5",
    costUsd: 0.0123,
    ending: ask ? "asked" : "done",
    note: null,
  };
  const beats: Beat[] = [{ at: 0, step: steps[0] }, { at: 250, text: "我先在文献库里找找。" }, { at: 1000, reset: true }, { at: 1050, step: steps[1] }, { at: 1500, step: steps[2] }];
  let at = 1800;
  const full = paragraphs.join("\n\n");
  for (let i = 0; i < full.length; i += 3) beats.push({ at: (at += 40), text: full.slice(i, i + 3) });
  return { beats, body };
}

function liveOf(sim: Sim, t: number): ChatLive {
  let text = "";
  const steps: ChatStep[] = [];
  for (const b of sim.beats) {
    if (b.at > t) break;
    if (b.step) steps.push(b.step);
    if (b.text) text += b.text;
    if (b.reset) text = "";
  }
  return { run: sim.run, author: sim.author, steps, text, startedAt: new Date(sim.started).toISOString() };
}

/** 答完（或停了）：存成一条消息 */
function finish(sim: Sim, stopped = false) {
  sims.delete(sim.conv);
  const c = chats.find((x) => x.uid === sim.conv);
  if (!c) return;
  let body = sim.reply;
  if (stopped) {
    const l = liveOf(sim, Date.now() - sim.started);
    body = { steps: l.steps, paragraphs: l.text.split(/\n{2,}/).filter((p) => p.trim()), cites: [], model: sim.reply.model, costUsd: 0.004, ending: "stopped", note: "已经写出来的留着。" };
  }
  c.messages.push({ position: c.messages.length + 1, author: sim.author, body, createdAt: now() });
  c.running = false;
  c.updatedAt = now();
}

function tick(conv: unknown) {
  const sim = sims.get(String(conv));
  if (sim && Date.now() - sim.started > sim.end) finish(sim);
}

const summary = (c: Chat): ChatSummary => ({ uid: c.uid, title: c.title, role: c.role, skill: c.skill, refUid: c.refUid, stage: c.stage, createdAt: c.createdAt, updatedAt: c.updatedAt });
const find = (u: unknown) => chats.find((c) => c.uid === u) ?? (() => { throw "这段对话不存在了。"; })();
const say = (c: Chat, text: string, role: string) => {
  if (sims.has(c.uid)) throw "上一句还在回答。等它答完，或者先点停。";
  c.messages.push({ position: c.messages.length + 1, author: "you", body: { text, role }, createdAt: now() });
  const { beats, body } = reply(text, c.kbs);
  sims.set(c.uid, { conv: c.uid, author: role, run: uid("run"), started: Date.now(), beats, end: beats[beats.length - 1].at + 300, reply: body });
  c.running = true;
  c.updatedAt = now();
};

export const chatHandlers: Record<string, (a: Args) => unknown> = {
  chat_list: () => [...chats].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).map(summary),
  chat_get: ({ uid: u }) => (tick(u), structuredClone(find(u))),
  chat_live: ({ uid: u }) => {
    tick(u);
    const sim = sims.get(String(u));
    return sim ? liveOf(sim, Date.now() - sim.started) : null;
  },
  chat_stop: ({ uid: u }) => {
    const sim = sims.get(String(u));
    if (sim) finish(sim, true);
  },
  chat_start: ({ text, role, kbs: picked, skill }) => {
    const c: Chat = { uid: uid("c"), title: String(text).slice(0, 40), role: String(role), kbs: picked as string[], skill: (skill as Chat["skill"]) ?? null, refUid: null, stage: 0, createdAt: now(), updatedAt: now(), running: false, messages: [] };
    chats.push(c);
    if (skill) c.messages.push({ position: 1, author: "you", body: { text: String(text), role: String(role) }, createdAt: now() });
    else say(c, String(text), String(role));
    return structuredClone(c);
  },
  chat_send: ({ uid: u, text, role }) => {
    const c = find(u);
    say(c, String(text), (role as string | null) ?? c.role);
    return structuredClone(c);
  },
  chat_open_ref: ({ skill, refUid, title }) => {
    const had = chats.find((c) => c.skill === skill && c.refUid === refUid);
    if (had) return had.uid;
    const c: Chat = { uid: uid("c"), title: String(title), role: "pith", kbs: [], skill: skill as Chat["skill"], refUid: String(refUid), stage: 0, createdAt: now(), updatedAt: now(), running: false, messages: [] };
    chats.push(c);
    return c.uid;
  },
  chat_link: ({ uid: u, skill, refUid }) => {
    const c = find(u);
    c.skill = skill as Chat["skill"];
    c.refUid = String(refUid);
  },
  chat_rename: ({ uid: u, title }) => void (find(u).title = String(title)),
  chat_delete: ({ uid: u }) => void chats.splice(chats.indexOf(find(u)), 1),
  role_list: () => roles,
  role_save: ({ role }) => {
    const r = role as Role;
    const i = roles.findIndex((x) => x.uid === r.uid);
    const saved = { ...r, uid: r.uid || uid("r"), mark: r.mark || r.name.slice(0, 1), edited: r.builtin };
    if (i >= 0) roles[i] = saved;
    else roles.push(saved);
    return saved;
  },
  role_delete: ({ uid: u }) => {
    const i = roles.findIndex((x) => x.uid === u);
    if (i >= 0 && !roles[i].builtin) roles.splice(i, 1);
    else if (i >= 0) roles[i] = { ...roles[i], edited: false };
  },
  kb_list: () => kbs.map((k) => ({ ...k, count: materials.filter((x) => x.kb === k.uid).length })),
  kb_create: ({ name, note }) => {
    const k = { uid: uid("kb"), name: String(name), note: String(note ?? ""), createdAt: now() };
    kbs.push(k);
    return k.uid;
  },
  kb_update: ({ uid: u, name, note }) => Object.assign(kbs.find((k) => k.uid === u)!, { name, note }),
  kb_delete: ({ uid: u }) => {
    kbs.splice(kbs.findIndex((k) => k.uid === u), 1);
    for (let i = materials.length - 1; i >= 0; i--) if (materials[i].kb === u) materials.splice(i, 1);
  },
  kb_materials: ({ kb }) => materials.filter((x) => !kb || x.kb === kb).sort((a, b) => b.tendedAt.localeCompare(a.tendedAt)),
  kb_material_add: ({ material }) => {
    const x = material as Material;
    const kb = x.kb || kbs[0]?.uid;
    const had = materials.find((y) => y.kb === kb && x.sourceKind && y.sourceKind === x.sourceKind && y.sourceRef === x.sourceRef);
    if (had) return (had.tendedAt = now()), had;
    const made = { ...x, uid: uid("m"), kb, createdAt: now(), tendedAt: now() };
    materials.push(made);
    return made;
  },
  kb_material_update: ({ uid: u, title, body, stage }) => Object.assign(materials.find((x) => x.uid === u)!, { title, body, stage: Math.max(0, Math.min(2, Number(stage))), tendedAt: now() }),
  kb_material_delete: ({ uid: u }) => void materials.splice(materials.findIndex((x) => x.uid === u), 1),
  kb_candidates: () => [
    { ...m("", "full", "The Wealth of Nations (fixture)", "Smith · 英文 · 带段落号", "", 2, 0), sourceKind: "concept_text", sourceRef: "T-en" },
    { ...m("", "full", "国富论（合成）", "亚当·斯密 · 中文 · 带段落号", "", 2, 0), sourceKind: "concept_text", sourceRef: "T-zh" },
    { ...m("", "report", "2022–2024 年示例市 16–24 岁青年的就业情况怎么样？", "事实调查 · 第 2 次", "", 1, 0), sourceKind: "research_run", sourceRef: "RUN2" },
  ],
};
