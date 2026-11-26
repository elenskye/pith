// 仅开发用：对话、角色、知识库的假后端（?mock）。内容照 Explore R1（visual-atelier canvases/pith-explore/src/content.mjs）：
// 四个知识库、十种记录、读《国富论》那段对话。规则在 Rust（src-tauri/src/chat/、src-tauri/src/kb/），这里只大致模仿：
// 一份材料能进几个库；《国富论》切成段落块（三章、带段落号和页码、中译对照）；回答在知识库里找、引用编号点开是那一段。
import type { Chat, ChatBody, ChatCite, ChatLive, ChatStep, ChatSummary, KbPassage, KbUnit, KnowledgeBase, Material, MaterialKind, Role } from "@/shared/ipc";

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
  uid: uid("m"), kb, kbs: kb ? [kb] : [], kind, title, sub, body, stage, sourceKind: null, sourceRef: null, conversation: null, createdAt: ago(days + 3), tendedAt: ago(days), passages: body ? 1 : 0, ...src,
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
// 两份材料都在「读《国富论》」和「毕业论文」里（一份材料能进几个库）
stigler.kbs.push("kb-thesis");

// 《国富论》的段落块：三章、带段落号和页码；中译对照（合成的，大意）
let pid = 5000;
type Para = KbPassage & { zh: string };
const para = (unitKey: string, unitLabel: string, n: number, page: string, text: string, zh: string): Para => ({ id: ++pid, seq: pid - 5001, role: "body", key: `${unitKey}.${n}`, unitKey, unitLabel, page, text, zh });
const head = (unitKey: string, unitLabel: string): Para => ({ id: ++pid, seq: pid - 5001, role: "heading", key: "", unitKey, unitLabel, page: null, text: unitLabel, zh: "" });
const book: Para[] = [
  head("I.i", "Of the Division of Labour"),
  para("I.i", "Of the Division of Labour", 1, "109", "The greatest improvement in the productive powers of labour, and the greater part of the skill, dexterity, and judgment with which it is any where directed, or applied, seem to have been the effects of the division of labour.", "劳动生产力最大的改进，以及劳动在任何地方运用时的技能、熟练和判断力的大部分，似乎都是分工的结果。"),
  para("I.i", "Of the Division of Labour", 3, "110", "To take an example, therefore, from a very trifling manufacture; but one in which the division of labour has been very often taken notice of, the trade of the pin-maker… ten persons, therefore, could make among them upwards of forty-eight thousand pins in a day.", "举一个很小的制造业做例子，而它的分工常常引人注意，那就是扣针制造业……十个人每天就能做四万八千多枚针。"),
  para("I.i", "Of the Division of Labour", 5, "112", "This great increase of the quantity of work which, in consequence of the division of labour, the same number of people are capable of performing, is owing to three different circumstances.", "由于分工，同样数量的人能完成的工作量大大增加，这归于三种不同的情况。"),
  head("I.ii", "Of the Principle which gives occasion to the Division of Labour"),
  para("I.ii", "Of the Principle which gives occasion to the Division of Labour", 1, "117", "This division of labour… is the necessary, though very slow and gradual consequence of a certain propensity in human nature… the propensity to truck, barter, and exchange one thing for another.", "这种分工……是人性中某种倾向的必然结果，虽然很缓慢、逐渐地发生……即互通有无、物物交换、以物易物的倾向。"),
  para("I.ii", "Of the Principle which gives occasion to the Division of Labour", 2, "118", "It is not from the benevolence of the butcher, the brewer, or the baker, that we expect our dinner, but from their regard to their own interest.", "我们期望的晚餐，不是来自屠户、酿酒师或面包师的恩惠，而是来自他们对自身利益的关注。"),
  head("I.iii", "That the Division of Labour is limited by the Extent of the Market"),
  para("I.iii", "That the Division of Labour is limited by the Extent of the Market", 1, "121", "As it is the power of exchanging that gives occasion to the division of labour, so the extent of this division must always be limited by the extent of that power, or, in other words, by the extent of the market.", "既然交换的能力引起分工，分工的程度就总要受交换能力大小的限制，换句话说，要受市场范围的限制。"),
  para("I.iii", "That the Division of Labour is limited by the Extent of the Market", 2, "121", "In the lone houses and very small villages which are scattered about in so desert a country as the Highlands of Scotland, every farmer must be butcher, baker and brewer for his own family.", "在苏格兰高地这样荒凉的地方，散居的孤零零的人家和很小的村落里，每个农夫都得为自己一家当屠夫、面包师和酿酒人。"),
  para("I.iii", "That the Division of Labour is limited by the Extent of the Market", 3, "122", "As by means of water-carriage a more extensive market is opened to every sort of industry than what land-carriage alone can afford it, so it is upon the sea-coast, and along the banks of navigable rivers, that industry of every kind naturally begins to subdivide and improve itself.", "水运为各种产业开辟的市场比陆运所能提供的更广，所以各种产业的分工和改良，自然最先出现在沿海和通航的河流两岸。"),
];
wnFull.passages = book.length;
wnFull.kbs.push("kb-thesis");
const byKey = (k: string) => book.find((p) => p.key === k)!;
const bookCite = (n: number, p: Para): ChatCite => ({ n, kind: "原文", src: "《The Wealth of Nations》 Smith 1776", loc: `${p.key} · 第 ${p.page} 页`, text: p.text, translation: p.zh, translationLoc: `${p.key} · 杨敬年译本`, material: wnFull.uid, kb: "kb-wn", passage: p.id, item: 1 });
const firstReply: ChatBody = {
  steps: [
    { agent: "search", text: "在知识库里找「分工 市场范围」：6 段" },
    { agent: "read", text: "读了 P5009 的上下文" },
    { agent: "search", text: "在知识库里找「Stigler 行业生命周期」：2 段" },
  ],
  paragraphs: [
    "斯密的推理只有一步：分工是因为能交换才出现的，所以分工能有多细，就看能交换多少，也就是市场有多大[1]。",
    "市场太小，一个人就没法只干一件事。他举苏格兰高地的散村：农夫得给自家当屠夫、面包师和酿酒人[2]。所以他紧接着讲水运：水路把市场一下子扩大，分工和改良最先出现在沿海和通航的河边[3]。",
    "一百多年后，Stigler 把这句话用到了企业上：行业刚起步、市场还小，企业只好什么都自己做；行业长大了，专门的环节才分出去，成了独立的厂家[4]。",
  ],
  cites: [bookCite(1, byKey("I.iii.1")), bookCite(2, byKey("I.iii.2")), bookCite(3, byKey("I.iii.3")), { n: 4, kind: "摘抄", src: "《Stigler 1951 的摘抄》 我的摘抄 · 8 条", loc: "", text: stigler.body, material: stigler.uid, kb: "kb-wn", passage: 9001 }],
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
  // 知识库里找：《国富论》在范围里就挑两段讲分工和市场的
  const found = picked.length === 0 || wnFull.kbs.some((k) => picked.includes(k)) ? [byKey("I.iii.1"), byKey("I.i.3")] : [];
  const steps: ChatStep[] = [
    { agent: "think", text: "想了想" },
    { agent: "search", text: `在知识库里找「${query}」：${found.length} 段` },
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
        found.length ? "知识库里最相关的是斯密讲市场范围的那一段[1]，和别针厂的例子[2]，点编号看原文和中译。" : "知识库里没有找到相关的段落。",
        hits.length ? `文献库里沾边的有：${hits.map((h) => `**${h.title}**`).join("、")}。` : "文献库里没有找到沾边的条目。",
        "- 真的回答在正式版里（或开发版设 PITH_REAL_MODEL=1）\n- 下面的交付卡点「存进」，种子会飞进知识库",
      ];
  const body: ChatBody = {
    steps,
    paragraphs,
    cites: ask ? [] : found.map((p, i) => bookCite(i + 1, p)),
    ask,
    delivery: ask ? null : { kind: "answer", title: q.slice(0, 40), desc: "模拟回答：查了什么、找到了什么。", meta: "存下的回答 • Claude Sonnet 5.5", kb: name ? { uid: picked[0], name } : null, body: "" },
    model: "Claude Sonnet 5.5",
    costUsd: 0.0123,
    ending: ask ? "asked" : "done",
    note: null,
  };
  const beats: Beat[] = [{ at: 0, step: steps[0] }, { at: 250, text: "我先在知识库里找找。" }, { at: 900, reset: true }, { at: 950, step: steps[1] }, { at: 1350, step: steps[2] }, { at: 1700, step: steps[3] }];
  let at = 2000;
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
  kb_list: () => kbs.map((k) => ({ ...k, count: materials.filter((x) => x.kbs.includes(k.uid)).length })),
  kb_create: ({ name, note }) => {
    const k = { uid: uid("kb"), name: String(name), note: String(note ?? ""), createdAt: now() };
    kbs.push(k);
    return k.uid;
  },
  kb_update: ({ uid: u, name, note }) => Object.assign(kbs.find((k) => k.uid === u)!, { name, note }),
  kb_delete: ({ uid: u }) => {
    kbs.splice(kbs.findIndex((k) => k.uid === u), 1);
    for (let i = materials.length - 1; i >= 0; i--) {
      const x = materials[i];
      x.kbs = x.kbs.filter((k) => k !== u);
      if (x.kbs.length === 0) materials.splice(i, 1);
      else if (x.kb === u) x.kb = x.kbs[0];
    }
  },
  kb_materials: ({ kb }) =>
    materials
      .filter((x) => !kb || x.kbs.includes(String(kb)))
      .map((x) => ({ ...x, kb: kb ? String(kb) : x.kb }))
      .sort((a, b) => b.tendedAt.localeCompare(a.tendedAt)),
  kb_material_add: ({ material }) => {
    const x = material as Material;
    const kb = x.kb || kbs[0]?.uid;
    const had = materials.find((y) => x.sourceKind && y.sourceKind === x.sourceKind && y.sourceRef === x.sourceRef);
    if (had) {
      if (!had.kbs.includes(kb)) had.kbs.push(kb);
      had.tendedAt = now();
      return had;
    }
    const made: Material = { ...x, uid: uid("m"), kb, kbs: [kb], createdAt: now(), tendedAt: now(), passages: x.body?.trim() ? Math.max(1, x.body.split(/\n{2,}/).length) : 0 };
    materials.push(made);
    return made;
  },
  kb_material_update: ({ uid: u, title, body, stage }) => Object.assign(materials.find((x) => x.uid === u)!, { title, body, stage: Math.max(0, Math.min(2, Number(stage))), tendedAt: now() }),
  kb_material_delete: ({ uid: u, kb }) => {
    const i = materials.findIndex((x) => x.uid === u);
    if (i < 0) return;
    const x = materials[i];
    x.kbs = kb ? x.kbs.filter((k) => k !== kb) : [];
    if (x.kbs.length === 0) materials.splice(i, 1);
    else if (x.kb === kb) x.kb = x.kbs[0];
  },
  kb_material_link: ({ uid: u, kb }) => {
    const x = materials.find((y) => y.uid === u)!;
    if (!x.kbs.includes(String(kb))) x.kbs.push(String(kb));
    x.tendedAt = now();
    return x;
  },
  kb_read: ({ material, unit, passage }) => {
    const x = materials.find((y) => y.uid === material);
    if (!x) throw "这份材料不存在了。";
    if (x !== wnFull) {
      const ps: KbPassage[] = x.body.split(/\n{2,}/).filter((t) => t.trim()).map((text, i) => ({ id: 9000 + i, seq: i, role: "body", key: "", unitKey: "", unitLabel: "", page: null, text }));
      return { units: [{ key: "", label: "", count: ps.length, chars: x.body.length, first: 9000 }], unit: 0, passages: ps, focus: null, pairs: {}, pairTitle: null };
    }
    const keys = [...new Set(book.map((p) => p.unitKey))];
    const units: KbUnit[] = keys.map((k) => {
      const ps = book.filter((p) => p.unitKey === k);
      return { key: k, label: ps[0].unitLabel, count: ps.filter((p) => p.role === "body").length, chars: ps.reduce((a, p) => a + p.text.length, 0), first: ps[0].id };
    });
    const focus = book.find((p) => p.id === passage) ?? null;
    const at = focus ? keys.indexOf(focus.unitKey) : Math.min(Number(unit ?? 0), keys.length - 1);
    const ps = book.filter((p) => p.unitKey === keys[at]);
    return { units, unit: at, passages: ps.map(({ zh: _zh, ...p }) => p), focus: focus?.id ?? null, pairs: Object.fromEntries(ps.filter((p) => p.zh).map((p) => [String(p.id), p.zh])), pairTitle: "国富论（杨敬年译）" };
  },
  kb_index_status: () => ({ total: 11_420, done: 11_420, running: false, error: null, model: "模拟向量模型", issue: null, materials: materials.length }),
  kb_index_start: () => ({ total: 11_420, done: 11_420, running: false, error: null, model: "模拟向量模型", issue: null, materials: materials.length }),
  kb_candidates: () => [
    { ...m("", "full", "The Wealth of Nations (fixture)", "Smith · 英文 · 带段落号", "", 2, 0), sourceKind: "concept_text", sourceRef: "T-en" },
    { ...m("", "full", "国富论（合成）", "亚当·斯密 · 中文 · 带段落号", "", 2, 0), sourceKind: "concept_text", sourceRef: "T-zh" },
    { ...m("", "report", "2022–2024 年示例市 16–24 岁青年的就业情况怎么样？", "事实调查 · 第 2 次", "", 1, 0), sourceKind: "research_run", sourceRef: "RUN2" },
  ],
};
