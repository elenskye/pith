// 仅开发用：排版 Agent 各命令在浏览器预览（?mock）里的模拟实现，由 mockIpc.ts 并进命令表。
// 一个内存里的小模型，大致模仿 Rust（src-tauri/src/agent/），不是规则的来源。数据全部是合成的「某某大学」示例。
// 预览别的状态：
//   ?mock&agent=real   假装用真模型：要先在设置里填密钥，调用有费用
//   ?mock&agent=over   同上，并且本月已经快花到上限（发送会被拦下）
//   ?mock&apply=blocked 排版时保真校验不通过（不写出文件）
import { emit } from "@tauri-apps/api/event";
import type {
  AgentCall,
  AgentEstimate,
  AgentStatus,
  FormatAnchor,
  FormatAssembled,
  FormatCertainty,
  FormatDraft,
  FormatHit,
  FormatMode,
  FormatRequirement,
  FormatRulesetSummary,
  FormatRulesetView,
  FormatRun,
  FormatRunResult,
  FormatRunSummary,
  FormatRunView,
  FormatSection,
  FormatSettingPatch,
  FormatSpecLine,
  FormatTable,
} from "@/shared/ipc";

type Args = Record<string, unknown>;
const fail = (msg: string): never => {
  throw msg;
};
const params = new URLSearchParams(location.search);
const real = params.get("agent") === "real" || params.get("agent") === "over";
const blocked = params.get("apply") === "blocked";
const now = () => new Date().toISOString();
const MODEL = "claude-opus-5-5";
const PRICE = { input: 4, output: 20 };

// ---------- 状态、密钥、账本 ----------

let hasKey = false;
let budgetUsd = 20;
let sendMode: FormatMode = "brief";
/** 账本：旧的在前 */
const ledger: AgentCall[] = [];

const month = () => now().slice(0, 7);
const monthSpent = () => ledger.filter((c) => !c.mock && c.at.startsWith(month())).reduce((n, c) => n + c.costUsd, 0);

function status(): AgentStatus {
  return {
    mock: !real,
    hasKey,
    keyPlace: "memory",
    model: MODEL,
    priceInput: PRICE.input,
    priceOutput: PRICE.output,
    budgetUsd,
    sendMode,
    month: month(),
    monthSpentUsd: monthSpent(),
    calls: [...ledger].reverse().slice(0, 30),
    ledgerError: null,
  };
}

function estimate(inputTokens: number, typicalOutput: number, maxOutput: number): AgentEstimate {
  const typicalUsd = (inputTokens * PRICE.input + typicalOutput * PRICE.output) / 1e6;
  const maxUsd = (inputTokens * 1.1 * PRICE.input + maxOutput * PRICE.output) / 1e6;
  return { model: MODEL, inputTokens, typicalUsd, maxUsd, budgetUsd, monthSpentUsd: monthSpent(), withinBudget: monthSpent() + maxUsd <= budgetUsd };
}

function record(purpose: string, input: number, output: number): AgentCall {
  const call: AgentCall = {
    at: now(),
    purpose,
    model: MODEL,
    tokens: { input, output, cacheRead: 0, cacheWrite: 0 },
    costUsd: real ? (input * PRICE.input + output * PRICE.output) / 1e6 : 0,
    estimated: false,
    mock: !real,
    note: null,
    requestId: real ? `req_mock_${ledger.length + 1}` : null,
  };
  ledger.push(call);
  return call;
}

// ---------- 模型调用：约 1.5 秒，中间发进度事件，可以停 ----------

let cancelled = false;
let busy = false;

function modelCall<T>(purpose: string, done: () => T): Promise<T> {
  if (busy) fail("上一次调用还没有结束。");
  if (real && !hasKey) fail("还没有填 Anthropic API 密钥。到「设置 › 密钥与费用」填写后再试。");
  busy = true;
  cancelled = false;
  return new Promise<T>((resolve, reject) => {
    let step = 0;
    const timer = setInterval(() => {
      step++;
      if (cancelled) {
        clearInterval(timer);
        busy = false;
        if (real) {
          const c = record(purpose, 9000, 1200);
          c.estimated = true;
          c.note = "中途停止，按估计记";
        }
        reject("已停止。" + (real ? "停下之前产生的用量按估计记进了账本。" : ""));
        return;
      }
      void emit("agent-progress", step <= 3 ? { thinkingChars: step * 420, outputChars: 0 } : { thinkingChars: 1260, outputChars: (step - 3) * 900 });
      if (step >= 6) {
        clearInterval(timer);
        busy = false;
        try {
          resolve(done());
        } catch (e) {
          reject(e);
        }
      }
    }, 250);
  });
}

// ---------- 设置值的换算（照 extract.rs 的 parse_setting 写个大概） ----------

const TARGETS: [string, string][] = [
  ["page", "页面"],
  ["body", "正文"],
  ["heading1", "一级标题"],
  ["heading2", "二级标题"],
  ["heading3", "三级标题"],
  ["heading4", "四级标题"],
  ["caption", "图题、表题"],
  ["table_text", "表内文字"],
  ["figure", "图片所在段落"],
  ["footnote", "脚注"],
  ["bibliography", "参考文献表"],
  ["header_footer", "页眉页脚文字"],
  ["toc", "目录"],
  ["header", "页眉内容"],
  ["page_number", "页码"],
  ["layout", "版式"],
  ["table", "表格"],
  ["footnote_number", "脚注编号"],
];
const PROPS: [string, string][] = [
  ["paper", "纸张"],
  ["margin_top", "上边距"],
  ["margin_bottom", "下边距"],
  ["margin_left", "左边距"],
  ["margin_right", "右边距"],
  ["gutter", "装订线"],
  ["header_distance", "页眉距边界"],
  ["footer_distance", "页脚距边界"],
  ["font_east_asia", "中文字体"],
  ["font_latin", "西文字体"],
  ["size", "字号"],
  ["bold", "加粗"],
  ["color", "颜色"],
  ["align", "对齐"],
  ["line_spacing", "行距（倍数）"],
  ["line_spacing_exact", "行距（固定值）"],
  ["space_before", "段前"],
  ["space_after", "段后"],
  ["first_line_indent", "首行缩进（字符）"],
  ["page_break_before", "段前分页"],
  ["tight_after_parent", "紧跟上一级标题时段前不空"],
  ["single_left", "单面 · 左"],
  ["single_center", "单面 · 中"],
  ["single_right", "单面 · 右"],
  ["odd_center", "双面奇数页 · 中"],
  ["even_center", "双面偶数页 · 中"],
  ["rule", "页眉下的横线"],
  ["front_format", "前置部分的数字"],
  ["body_format", "正文的数字"],
  ["body_on_odd_page", "正文从奇数页开始"],
  ["duplex_from_pages", "多少页起双面印制"],
  ["three_line", "三线表"],
  ["format", "编号形式"],
  ["restart", "重新编号"],
];
/** 结构类的对象各有哪些属性（照 Rust 的 props_of） */
const STRUCT_PROPS: Record<string, string[]> = {
  header: ["single_left", "single_center", "single_right", "odd_center", "even_center", "rule"],
  page_number: ["align", "front_format", "body_format"],
  layout: ["body_on_odd_page", "duplex_from_pages"],
  table: ["three_line"],
  footnote_number: ["format", "restart"],
};
const label = (table: [string, string][], key: string) => table.find((x) => x[0] === key)?.[1] ?? key;
const PAGE_PROPS = PROPS.slice(0, 8).map((p) => p[0]);
const SIZES: Record<string, number> = { 二号: 22, 小二: 18, 三号: 16, 小三: 15, 四号: 14, 小四: 12, 五号: 10.5, 小五: 9 };
const num = (v: number) => String(Math.round(v * 1000) / 1000);

function quantity(s: string): [number, string] | null {
  const m = /^(\d+(?:\.\d+)?)\s*(mm|毫米|cm|厘米|pt|磅)$/.exec(s.trim().toLowerCase());
  if (!m) return null;
  return [Number(m[1]), m[2] === "毫米" ? "mm" : m[2] === "厘米" ? "cm" : m[2] === "磅" ? "pt" : m[2]];
}
function points(s: string): number {
  const q = quantity(s);
  if (q) return q[1] === "pt" ? q[0] : q[1] === "mm" ? (q[0] * 72) / 25.4 : (q[0] * 72) / 2.54;
  return /^\d+(\.\d+)?$/.test(s.trim()) ? Number(s) : fail("要写磅值，如 20pt");
}

/** 读懂一个设置值，返回给人看的写法；读不懂就抛出原因 */
function display(target: string, prop: string, value: string): string {
  const v = value.trim();
  const yesNo = () => (["true", "是"].includes(v.toLowerCase()) ? "是" : ["false", "否"].includes(v.toLowerCase()) ? "否" : fail("只能是 true 或 false"));
  const oneOf = (names: Record<string, string>, hint: string) => names[v.toLowerCase()] ?? fail(hint);
  if (STRUCT_PROPS[target]) {
    if (!STRUCT_PROPS[target].includes(prop)) fail(`「${label(TARGETS, target)}」没有「${label(PROPS, prop)}」这个属性`);
    if (prop === "rule" || prop === "body_on_odd_page" || prop === "three_line") return yesNo();
    if (prop === "align") return oneOf({ left: "居左", center: "居中", right: "居右" }, "页码位置只能是 left / center / right");
    if (prop === "front_format" || prop === "body_format") return oneOf({ upper_roman: "大写罗马数字（Ⅰ、Ⅱ）", lower_roman: "小写罗马数字（ⅰ、ⅱ）", decimal: "阿拉伯数字（1、2）" }, "页码的数字形式只能是 upper_roman / lower_roman / decimal");
    if (prop === "format") return oneOf({ circled: "带圈数字（①②③）", decimal: "阿拉伯数字" }, "脚注编号形式只能是 circled（①②③）/ decimal");
    if (prop === "restart") return oneOf({ each_page: "每页重新编号", each_section: "每节重新编号", continuous: "连续编号" }, "脚注重新编号只能是 each_page / each_section / continuous");
    if (prop === "duplex_from_pages") return /^\d+$/.test(v) ? `${v} 页` : fail("写页数，如 30");
    return v && v.length <= 80 ? v : fail("页眉文字空着、太长或带换行");
  }
  if (prop === "tight_after_parent") return /^heading[234]$/.test(target) ? yesNo() : fail(`「${label(TARGETS, target)}」没有「${label(PROPS, prop)}」这个属性`);
  if ((target === "page") !== PAGE_PROPS.includes(prop)) fail(target === "page" ? "页面没有这个属性" : "这个属性只属于页面");
  switch (prop) {
    case "paper":
      return v.toLowerCase() === "a4" ? "A4" : fail("目前只支持 A4");
    case "font_east_asia":
    case "font_latin":
      return v && v.length <= 40 ? v : fail("字体名空着或太长");
    case "size": {
      const pt = SIZES[v] ?? SIZES[`${v}号`] ?? (quantity(v)?.[1] === "pt" ? quantity(v)![0] : null);
      return pt !== null && pt >= 4 && pt <= 72 ? `${num(pt)} 磅` : fail("字号写成「小四」「三号」或「12pt」");
    }
    case "bold":
    case "page_break_before":
      return ["true", "是"].includes(v.toLowerCase()) ? "是" : ["false", "否"].includes(v.toLowerCase()) ? "否" : fail("只能是 true 或 false");
    case "color":
      return /^#?[0-9a-fA-F]{6}$/.test(v) ? v.replace("#", "").toUpperCase() : fail("颜色写六位十六进制，如 000000");
    case "align":
      return ({ left: "居左", center: "居中", right: "居右", both: "两端对齐", justify: "两端对齐" } as Record<string, string>)[v.toLowerCase()] ?? fail("对齐只能是 left / center / right / both");
    case "line_spacing": {
      const n = Number(v.replace(/倍$/, ""));
      return v && n >= 0.5 && n <= 5 ? `${num(n)} 倍` : fail("行距倍数写成 1.5 这样");
    }
    case "line_spacing_exact":
    case "space_before":
    case "space_after":
      return `${num(points(v))} 磅`;
    case "first_line_indent": {
      const n = Number(v.replace(/字符$/, ""));
      return v && n >= 0 && n <= 10 ? `${num(n)} 字符` : fail("首行缩进写字符数，如 2");
    }
    default: {
      const q = quantity(v);
      if (!q) fail("长度要带单位，如 30mm、2.5cm");
      const cm = q![1] === "mm" ? q![0] / 10 : q![1] === "cm" ? q![0] : (q![0] * 2.54) / 72;
      return cm <= 20 ? `${num(cm)} cm` : fail("长度超出页面");
    }
  }
}

const check = (target: string, prop: string, value: string): string | null => {
  try {
    display(target, prop, value);
    return null;
  } catch (e) {
    return String(e);
  }
};

function assemble(requirements: FormatRequirement[]): FormatAssembled {
  const chosen = new Map<string, { value: string; by: string; certainty: FormatCertainty; prop: string }>();
  const problems: string[] = [];
  const slot = (prop: string) => (prop === "line_spacing_exact" ? "line_spacing" : prop);
  for (const r of requirements.filter((x) => x.enabled && x.handling === "auto")) {
    for (const s of r.settings) {
      const err = check(s.target, s.prop, s.value);
      if (err) {
        problems.push(`${r.id}：「${label(TARGETS, s.target)} · ${label(PROPS, s.prop)} = ${s.value}」读不懂（${err}），没有进规范`);
        continue;
      }
      const key = `${s.target}.${slot(s.prop)}`;
      const value = display(s.target, s.prop, s.value);
      const old = chosen.get(key);
      if (!old) chosen.set(key, { value, by: r.id, certainty: r.certainty, prop: s.prop });
      else if (old.value !== value) problems.push(`${old.by} 与 ${r.id} 对「${label(TARGETS, s.target)} · ${label(PROPS, s.prop)}」给了不同的值，按 ${old.by} 的（确定度更高或在前）；${r.id} 的没有用上`);
    }
  }
  const lines: FormatSpecLine[] = [];
  for (const [target, targetLabel] of TARGETS)
    for (const [prop, propLabel] of PROPS) {
      const c = chosen.get(`${target}.${slot(prop)}`);
      if (c && c.prop === prop) lines.push({ target, prop, targetLabel, propLabel, value: c.value, requirement: c.by, certainty: c.certainty, note: null });
    }
  // 排版程序的默认做法：正文用固定行距时，图片所在段落改单倍行距
  if (chosen.get("body.line_spacing")?.prop === "line_spacing_exact" && !chosen.has("figure.line_spacing")) {
    lines.push({ target: "figure", prop: "line_spacing", targetLabel: label(TARGETS, "figure"), propLabel: label(PROPS, "line_spacing"), value: "1 倍", requirement: null, certainty: null, note: "固定行距会把图片裁掉，图片所在段落用单倍行距" });
    lines.push({ target: "figure", prop: "first_line_indent", targetLabel: label(TARGETS, "figure"), propLabel: label(PROPS, "first_line_indent"), value: "0 字符", requirement: null, certainty: null, note: "正文有首行缩进，这类段落不该跟着缩进" });
  }
  return { spec: {}, lines, problems };
}

// ---------- 示例规范 ----------

type Req = Omit<FormatRequirement, "settings" | "enabled" | "check" | "note" | "sources"> & {
  sources?: [string, string, string, boolean?][];
  note?: string;
  settings?: [string, string, string][];
  check?: FormatRequirement["check"];
};

const SAMPLE: Req[] = [
  { id: "A1", group: "页面与通用", text: "A4 纸", certainty: "explicit", handling: "auto", sources: [["R1", "1.1", "论文一律用 A4 纸打印"]], settings: [["page", "paper", "A4"]] },
  {
    id: "A2",
    group: "页面与通用",
    text: "页边距：上 30 mm、下 25 mm、左右各 25 mm，装订线 10 mm",
    certainty: "explicit",
    handling: "auto",
    sources: [["R1", "1.1", "页边距：上 30 mm，下 25 mm，左、右各 25 mm"]],
    settings: [
      ["page", "margin_top", "30mm"],
      ["page", "margin_bottom", "25mm"],
      ["page", "margin_left", "25mm"],
      ["page", "margin_right", "25mm"],
      ["page", "gutter", "10mm"],
    ],
  },
  {
    id: "B1",
    group: "标题与正文",
    text: "正文：宋体小四，西文与数字 Times New Roman；行距固定值 20 磅；首行缩进 2 字符，两端对齐",
    certainty: "explicit",
    handling: "auto",
    sources: [
      ["R1", "1.2", "正文用宋体小四号，西文与数字用 Times New Roman"],
      ["R1", "1.2", "行距为固定值 20 磅；段落首行缩进 2 个字符，两端对齐"],
    ],
    settings: [
      ["body", "font_east_asia", "宋体"],
      ["body", "font_latin", "Times New Roman"],
      ["body", "size", "小四"],
      ["body", "line_spacing_exact", "20pt"],
      ["body", "first_line_indent", "2"],
      ["body", "align", "both"],
    ],
  },
  {
    id: "B2",
    group: "标题与正文",
    text: "一级标题：黑体三号，居中，另起一页，段前、段后各空一行",
    certainty: "explicit",
    handling: "auto",
    sources: [["R1", "2.1", "一级标题用黑体三号，居中，另起一页，段前、段后各空一行"]],
    note: "「空一行」按正文行距 20 磅折算。首行缩进 0 是推断：正文规定了缩进，标题不该跟着缩。",
    settings: [
      ["heading1", "font_east_asia", "黑体"],
      ["heading1", "size", "三号"],
      ["heading1", "align", "center"],
      ["heading1", "space_before", "20pt"],
      ["heading1", "space_after", "20pt"],
      ["heading1", "first_line_indent", "0"],
      ["heading1", "page_break_before", "true"],
    ],
  },
  {
    id: "B3",
    group: "标题与正文",
    text: "二级标题：黑体小三，居左，段前空一行",
    certainty: "explicit",
    handling: "auto",
    sources: [["R1", "2.2", "二级标题采用黑体小三号字，靠左对齐", false]],
    settings: [
      ["heading2", "font_east_asia", "黑体"],
      ["heading2", "size", "小三"],
      ["heading2", "align", "left"],
      ["heading2", "space_before", "20pt"],
    ],
  },
  {
    id: "B4",
    group: "标题与正文",
    text: "三级标题：黑体四号，居左；模板里的三级标题不加粗",
    certainty: "template",
    handling: "auto",
    sources: [["R2", "模板·第 2 章", "2.1.1 三级标题示例（黑体四号）"]],
    note: "规范正文只写了字体字号，不加粗只在模板里看得出来。",
    settings: [
      ["heading3", "font_east_asia", "黑体"],
      ["heading3", "size", "四号"],
      ["heading3", "bold", "false"],
    ],
  },
  { id: "B5", group: "标题与正文", text: "各级标题用黑色", certainty: "inferred", handling: "auto", note: "文档没有写颜色；论文里常见蓝色的内置标题样式，定成黑色才能和明文要求一起执行。", settings: [["heading1", "color", "000000"], ["heading2", "color", "000000"]] },
  { id: "B6", group: "标题与正文", text: "一、二级标题是否加粗：文档没有规定，保留原文档的样子", certainty: "unspecified", handling: "keep" },
  { id: "C1", group: "前置部分", text: "中文摘要 300～500 字", certainty: "explicit", handling: "check", sources: [["R1", "3.1", "中文摘要 300～500 字"]], check: { kind: "abstract_zh_chars", min: 300, max: 500 } },
  { id: "C2", group: "前置部分", text: "关键词 3～5 个，用分号隔开，最后一个后面不加标点", certainty: "explicit", handling: "check", sources: [["R1", "3.2", "关键词 3～5 个，用分号隔开"]], check: { kind: "keywords_count", min: 3, max: 5 } },
  {
    id: "D1",
    group: "图表公式脚注",
    text: "图题在图下方、表题在表上方，宋体五号，居中，单倍行距",
    certainty: "explicit",
    handling: "auto",
    sources: [["R1", "4.1", "图题在图的下方，表题在表的上方，均用宋体五号，居中"]],
    settings: [
      ["caption", "font_east_asia", "宋体"],
      ["caption", "size", "五号"],
      ["caption", "align", "center"],
      ["caption", "line_spacing", "单倍"],
    ],
  },
  { id: "D2", group: "图表公式脚注", text: "图、表按章编号，如图 2.1、表 3.2", certainty: "explicit", handling: "check", sources: [["R1", "4.1", "图、表按章编号，如图 2.1、表 3.2"]], check: { kind: "caption_by_chapter", min: null, max: null } },
  { id: "D3", group: "图表公式脚注", text: "表格采用三线表", certainty: "explicit", handling: "auto", sources: [["R1", "4.2", "表格采用三线表"]], settings: [["table", "three_line", "true"]] },
  { id: "D4", group: "图表公式脚注", text: "脚注：宋体小五，每页单独编号", certainty: "explicit", handling: "auto", sources: [["R1", "4.3", "脚注用宋体小五号，每页单独编号"]], note: "「每页单独编号」要改脚注设置，这一版不做，只设置字体字号。", settings: [["footnote", "font_east_asia", "宋体"], ["footnote", "size", "小五"]] },
  { id: "E1", group: "页眉页脚", text: "页眉：单面印制左为校名、右为章名；双面印制左页居中校名、右页居中章名；下加横线", certainty: "explicit", handling: "auto", sources: [["R1", "5.1", "内容为「某某大学本科毕业论文」，下加横线"]], settings: [["header", "single_left", "某某大学本科毕业论文"], ["header", "single_right", "{章名}"], ["header", "odd_center", "{章名}"], ["header", "even_center", "某某大学本科毕业论文"], ["header", "rule", "true"]] },
  { id: "E2", group: "页眉页脚", text: "页码在页脚居中；摘要、目录用罗马数字，正文起用阿拉伯数字", certainty: "explicit", handling: "auto", sources: [["R1", "5.2", "摘要、目录用罗马数字，正文起用阿拉伯数字"]], settings: [["page_number", "align", "center"], ["page_number", "front_format", "upper_roman"], ["page_number", "body_format", "decimal"]] },
  { id: "E3", group: "页眉页脚", text: "30 页及以上双面打印；正文从奇数页开始", certainty: "explicit", handling: "auto", sources: [["R1", "1.3", "30 页及以上的论文双面打印。正文从奇数页开始"]], settings: [["layout", "duplex_from_pages", "30"], ["layout", "body_on_odd_page", "true"]] },
  { id: "E4", group: "图表公式脚注", text: "公式编号写在公式所在行的最右边", certainty: "explicit", handling: "structure", sources: [["R1", "4.4", "公式的编号写在公式所在行的最右边"]] },
  { id: "F1", group: "参考文献", text: "参考文献按 GB/T 7714—2015 顺序编码制著录，不少于 15 篇", certainty: "explicit", handling: "check", sources: [["R1", "6.1", "参考文献按 GB/T 7714—2015 的顺序编码制著录，不少于 15 篇"]], note: "著录格式由引文样式负责；这里只数条数。", check: { kind: "references_count", min: 15, max: null } },
  { id: "F2", group: "参考文献", text: "正文里的引文标注在句末标点之前", certainty: "template", handling: "check", sources: [["R2", "模板·第 1 章", "如文献所述[1]。"]] },
  { id: "G1", group: "附录致谢声明", text: "封面按学校统一格式填写，不得改动", certainty: "explicit", handling: "keep", sources: [["R1", "7.1", "封面按学校统一格式填写，不得改动"]] },
];

function sampleRequirements(): FormatRequirement[] {
  return SAMPLE.map((r) => ({
    id: r.id,
    group: r.group,
    text: r.text,
    certainty: r.certainty,
    handling: r.handling,
    note: r.note ?? "",
    sources: (r.sources ?? []).map(([doc, section, quote, verified]) => ({ doc, section, quote, verified: verified ?? true })),
    settings: (r.settings ?? []).map(([target, prop, value]) => ({ target, prop, value, error: check(target, prop, value) })),
    check: r.check ?? null,
    enabled: true,
  }));
}

type MRuleset = Omit<FormatRulesetView, "assembled" | "runs">;
type MRun = FormatRun;

let seq = 0;
/** 形状像 ULID 的编号（26 位），新的排在前 */
const uid = () => `01MOCK${String(Date.now()).slice(-8)}${String(++seq).padStart(4, "0")}`.padEnd(26, "0");

const rulesets: MRuleset[] = [];
const runs: MRun[] = [];

function newRuleset(docs: { label: string; name: string; chars: number }[], call: AgentCall | null, createdAt = now()): MRuleset {
  const requirements = sampleRequirements();
  const bad = requirements.flatMap((r) => r.settings.filter((s) => s.error).map((s) => `${r.id} 的设置「${label(TARGETS, s.target)} · ${label(PROPS, s.prop)} = ${s.value}」读不懂：${s.error}`));
  const rs: MRuleset = { id: uid(), name: "某某大学本科毕业论文撰写规范（示例，2026 年版）", createdAt, mock: !real, docs, requirements, problems: bad, call };
  rulesets.unshift(rs);
  return rs;
}

const findRuleset = (id: unknown) => rulesets.find((r) => r.id === id) ?? fail("这套规范不存在了");
const findRun = (rulesetId: unknown, id: unknown) => runs.find((r) => r.rulesetId === rulesetId && r.id === id) ?? fail("这份论文的计划不存在了");

function runSummary(r: MRun): FormatRunSummary {
  return { id: r.id, thesisName: r.thesisName, thesisPath: r.thesisPath, createdAt: r.createdAt, mode: r.mode, mock: r.mock, costUsd: r.call?.costUsd ?? 0, state: !r.result ? "none" : r.result.delivered ? "delivered" : "blocked" };
}

function rulesetView(r: MRuleset): FormatRulesetView {
  return structuredClone({ ...r, assembled: assemble(r.requirements), runs: runs.filter((x) => x.rulesetId === r.id).map(runSummary) });
}

// ---------- 示例论文 ----------

const THESIS = { paragraphs: 412, totalChars: 28_640, briefChars: 6_180 };
const STYLES = ["Normal", "Normal (Web)", "p1", "Heading 1", "Heading 2", "Heading 3", "List Paragraph", "Caption", "TOC 1", "TOC 2", "Footnote Text"];
/** 各样式的段落：用来算「命中」 */
const STYLE_PARAS: Record<string, { count: number; samples: string[] }> = {
  "normal (web)": { count: 137, samples: ["政治效能感指个人认为自己的政治行动能够影响政治过程的感受。", "已有研究多把它分成内部效能感和外部效能感两个维度。", "本文使用合成的问卷数据，仅用于演示排版流程。"] },
  p1: { count: 24, samples: ["从表 3.1 可以看出，各题项的均值都在中位数以上。", "这一结果与第 2 章的预期一致。"] },
  normal: { count: 96, samples: ["本章介绍研究设计与数据来源。"] },
  "heading 2": { count: 6, samples: ["第 1 章 绪论", "第 2 章 文献综述", "第 3 章 研究设计"] },
  "heading 3": { count: 19, samples: ["1.1 研究背景", "1.2 研究问题"] },
  "list paragraph": { count: 12, samples: ["第一，样本覆盖东、中、西部地区。"] },
  caption: { count: 3, samples: ["图 1.1 研究框架"] },
};
const CAPTIONS = ["图 2.1 政治效能感的两个维度", "表 3.1 变量的描述统计", "表 3.2 信度检验结果", "图 4.1 各组均值比较", "图 4 展示了三组样本在外部效能感上的差异，其中东部地区的均值最高，中部次之，西部最低。"];
const SECTIONS: FormatSection[] = [
  { index: 1, paragraphs: 31, chars: 486, opening: ["某某大学", "本科毕业论文", "题目：示例：公民政治效能感的测量与比较", "学院：示例学院"] },
  { index: 2, paragraphs: 381, chars: 28_154, opening: ["摘要", "政治效能感是政治心理研究的核心概念之一。", "关键词：政治效能感；测量；比较"] },
];

/** 可以当分界点的段落（一级标题，含映射成一级标题的） */
const ANCHORS: FormatAnchor[] = [
  { paragraph: 32, text: "摘要" },
  { paragraph: 36, text: "目录" },
  { paragraph: 40, text: "第 1 章 绪论" },
  { paragraph: 72, text: "第 2 章 文献综述" },
  { paragraph: 118, text: "第 3 章 研究设计" },
  { paragraph: 190, text: "第 4 章 结果" },
  { paragraph: 300, text: "第 5 章 结论" },
  { paragraph: 340, text: "参考文献" },
  { paragraph: 395, text: "致谢" },
  { paragraph: 401, text: "原创性声明" },
];
const TABLES: Omit<FormatTable, "state" | "why">[] = [
  { index: 1, rows: 4, cols: 2, paragraph: 9, firstRow: "学号 | 20220001" },
  { index: 2, rows: 6, cols: 4, paragraph: 132, firstRow: "变量 | 均值 | 标准差 | 样本量" },
  { index: 3, rows: 5, cols: 3, paragraph: 151, firstRow: "量表 | 题项数 | Cronbach's α" },
  { index: 4, rows: 2, cols: 2, paragraph: 210, firstRow: "（公式） | (4.1)" },
];

function tables(d: FormatDraft): FormatTable[] {
  return TABLES.map((t) => {
    if (t.index === 4) return { ...t, state: "auto" as const, why: "没有边框，多半是用来排版的表格" };
    if (d.protectSections.some((p) => p.enabled && p.section === 1) && t.index === 1) return { ...t, state: "auto" as const, why: "在受保护的分节里" };
    if (d.skipTables.some((x) => x.enabled && x.table === t.index)) return { ...t, state: "skipped" as const, why: "计划里说不排" };
    return { ...t, state: "three_line" as const, why: null };
  });
}

function sampleDraft(): FormatDraft {
  return {
    summary: "正文大多是从网页粘贴来的，用的是 Normal (Web) 和 p1 样式；章标题用了 Heading 2、节标题用了 Heading 3，整体低了一级；图题、表题是手动居中的普通段落。第 1 节是学校统一格式的封面。计划把粘贴样式映射到正文、标题逐级上调、用文字规则认出题注，并保护封面所在的分节。",
    styleMap: [
      { from: "Normal (Web)", to: "normal", reason: "137 段，都是正文叙述（如 ¶45、¶46），是粘贴来的样式。", enabled: true },
      { from: "p1", to: "normal", reason: "24 段正文，出现在第 3、4 章，同样是粘贴来的。", enabled: true },
      { from: "Heading 2", to: "heading 1", reason: "「第 1 章 绪论」等 6 个章标题用了 Heading 2，Heading 1 没有用到，层级整体低一级。", enabled: true },
      { from: "Heading 3", to: "heading 2", reason: "「1.1 研究背景」等节标题用了 Heading 3，随章标题上调一级。", enabled: true },
    ],
    paragraphRules: [{ pattern: "^(图|表)\\s*\\d+(\\.\\d+)?\\s+\\S", maxChars: 40, style: "caption", reason: "¶88「图 2.1 政治效能感的两个维度」、¶131「表 3.1 变量的描述统计」这类手动居中的短段落是题注；限 40 字以内，排除「图 4 展示了……」这样的正文句子。", enabled: true }],
    protectSections: [{ section: 1, reason: "第 1 节 31 段全是封面与题名页（学校统一格式，靠手动格式排版），整节保护。", enabled: true }],
    layout: {
      frontStart: ANCHORS[0],
      bodyStart: ANCHORS[2],
      backStart: ANCHORS[9],
      reason: "¶32「摘要」是前置部分的开头，¶40「第 1 章 绪论」是正文的开头，¶401「原创性声明」在致谢之后。",
      duplex: true,
      duplexNote: "规范要求 30 页及以上双面印制；Word 上次存盘时记的是 46 页（排版后页数会变，以实际为准）",
    },
    skipTables: [{ table: 1, reason: "封面上的信息栏（学号、姓名、专业），不是数据表。", enabled: true }],
    observations: [
      { text: "图 4 没有按章编号（别的图是「图 2.1」这样的写法）。", paragraphs: [236] },
      { text: "章标题里手打了「第 1 章」编号，没有用自动编号。", paragraphs: [40, 72, 118] },
      { text: "多处用空段落隔开段落，排版后段间距会偏大。", paragraphs: [47, 53, 90] },
    ],
    problems: ["样式映射「正文文本 → normal」：文档里没有叫「正文文本」的样式，已去掉"],
  };
}

function hits(d: FormatDraft): FormatHit[] {
  const out: FormatHit[] = [];
  const protectedBody = d.protectSections.some((p) => p.enabled && p.section === 2);
  if (protectedBody) return out;
  // 文字规则：按合成的题注段落真的去匹配
  for (const r of d.paragraphRules.filter((x) => x.enabled)) {
    let re: RegExp;
    try {
      re = new RegExp(r.pattern);
    } catch {
      continue;
    }
    const matched = CAPTIONS.filter((c) => re.test(c) && (r.maxChars === 0 || c.length <= r.maxChars));
    if (matched.length === 0) continue;
    const to = r.style.toLowerCase();
    const existing = out.find((h) => h.how === "rule" && h.to === to);
    if (existing) continue;
    out.push({ label: `normal → ${to}（文字规则）`, from: "normal", to, how: "rule", count: matched.length * 4, samples: matched });
  }
  for (const m of d.styleMap.filter((x) => x.enabled)) {
    const from = m.from.toLowerCase();
    const paras = STYLE_PARAS[from];
    if (!paras || from === m.to.toLowerCase()) continue;
    out.push({ label: `${from} → ${m.to}（样式映射）`, from, to: m.to, how: "style", count: paras.count, samples: paras.samples });
  }
  return out.sort((a, b) => a.label.localeCompare(b.label));
}

function runView(r: MRun): FormatRunView {
  return structuredClone({ ...r, hits: hits(r.draft), sections: SECTIONS, styles: STYLES, needsZones: true, threeLine: true, anchors: ANCHORS, tables: tables(r.draft), thesisProblem: null });
}

function result(run: MRun, ruleset: MRuleset): FormatRunResult {
  const stem = run.thesisPath.replace(/\.docx$/i, "");
  const reqs = ruleset.requirements;
  const auto = reqs.filter((r) => r.handling === "auto" && r.enabled);
  const off = reqs.filter((r) => r.handling === "auto" && !r.enabled);
  const checks = reqs
    .filter((r) => r.handling === "check")
    .map((r) => {
      switch (r.check?.kind) {
        case "abstract_zh_chars":
          return { requirement: r.id, text: r.text, ok: true, value: "412 字", detail: "要求 300–500 字" };
        case "keywords_count":
          return { requirement: r.id, text: r.text, ok: false, value: "6 个", detail: "要求 3–5 个；分隔符是分号，末尾没有标点" };
        case "references_count":
          return { requirement: r.id, text: r.text, ok: true, value: "23 条", detail: "要求不少于 15 条；其中外文 9 条" };
        case "caption_by_chapter":
          return { requirement: r.id, text: r.text, ok: false, value: "17 个题注，1 个没按章", detail: "「图 4 各组均值比较」" };
        default:
          return { requirement: r.id, text: r.text, ok: null, value: "—", detail: "程序不会检查这一项" };
      }
    });
  const h = hits(run.draft);
  const delivered = !blocked;
  const lines = assemble(reqs).lines;
  const specChecks = Math.min(12, lines.length);
  const specFailed = lines.length > 7 ? 1 : 0;
  const mark = (ok: boolean | null) => (ok === true ? "✓" : ok === false ? "✗" : "需人工核对");
  let md = `# 排版报告

**结论：${delivered && specFailed === 0 ? "通过" : "不通过"}**

- 正文保真：${delivered ? "通过（文字、域、公式、结构、其他部件均未变）" : "发现 2 处问题"}
- 规范生效：${specFailed === 0 ? `${specChecks} 项全部符合` : `${specFailed} 项不符合`}

## 改动

规范：${ruleset.name}

- 改动的部件：word/document.xml、word/styles.xml
- 修改的样式：Normal、Heading 1、Heading 2、Heading 3、Caption、Footnote Text
- 清理直接格式：run 1,284 处、段落 366 处
- 表格段落套用表格文字样式：58；图片 / 公式段落套用居中样式：9
- 分节页面设置：2 处

## 段落角色映射（排版计划）

${h.map((x) => `- ${x.label}：${x.count} 段`).join("\n") || "- （没有）"}

## 受保护区域

- 段落 ${run.draft.protectSections.some((p) => p.enabled) ? 31 : 0} 个保持原样；为抵消样式改动写回原外观 12 处

`;
  if (!delivered) md += `## 保真问题

| 部件 | 类型 | 说明 |
| --- | --- | --- |
| word/document.xml | 文字 | 第 88 段的文字变了：「图 2.1 政治效能感」→「图 2.1政治效能感」 |
| word/document.xml | 域 | 第 131 段少了一个 SEQ 域 |

`;
  md += `## 规范核对

| 项 | 结果 | 说明 |
| --- | --- | --- |
${lines
  .slice(0, 12)
  .map((l, i) => `| ${l.targetLabel} · ${l.propLabel} | ${i === 7 ? "✗" : "✓"} | ${i === 7 ? `要求 ${l.value}，第 2 节实际不同` : l.value} |`)
  .join("\n")}

## 要求清单的处理

- 自动应用：${auto.length} 条（${auto.map((r) => r.id).join("、")}）
- 其中不是规范明文的：${auto.filter((r) => r.certainty !== "explicit").map((r) => `${r.id}（${r.certainty === "template" ? "模板" : "推断"}）`).join("、") || "无"}
${off.length ? `- 你关掉、没有应用的：${off.map((r) => r.id).join("、")}\n` : ""}
### 只检查、不改的要求

统计由程序完成，不是模型数的。「需人工核对」的程序没有检查。

| 要求 | 结果 | 统计 | 说明 |
| --- | --- | --- | --- |
${checks.map((c) => `| ${c.requirement} ${c.text} | ${mark(c.ok)} | ${c.value} | ${c.detail} |`).join("\n")}

### 要改内容或结构、这一版没有做的要求

${reqs.filter((r) => r.handling === "structure").map((r) => `- ${r.id} ${r.text}`).join("\n")}

### 不动的

${reqs.filter((r) => r.handling === "keep").map((r) => `- ${r.id} ${r.text}`).join("\n")}

## 文档里的其他情况

分析论文时模型看到的、计划解决不了的情况，供参考（没有经过程序核对）。

${run.draft.observations.map((o) => `- ${o.text}（第 ${o.paragraphs.join("、")} 段）`).join("\n")}
`;
  if (!delivered) md = `> 保真校验没有通过，所以没有写出文件。原文件没有改动。\n\n${md}`;
  return {
    appliedAt: now(),
    delivered,
    outputPath: delivered ? `${stem}（Pith 排版）.docx` : null,
    reportPath: delivered ? `${stem}（Pith 排版） 报告.md` : null,
    reportMd: md,
    fidelityProblems: delivered ? 0 : 2,
    specChecks,
    specFailed,
    checks,
  };
}

// 预置一套规范和一份还没排的论文，打开「排版」就有东西看
{
  const call: AgentCall = { at: "2026-11-09T06:12:40.000Z", purpose: "extract", model: MODEL, tokens: { input: 1480, output: 5210, cacheRead: 0, cacheWrite: 0 }, costUsd: real ? 0.11 : 0, estimated: false, mock: !real, note: null, requestId: null };
  ledger.push(call);
  const rs = newRuleset([{ label: "R1", name: "某某大学本科毕业论文撰写规范（示例）.docx", chars: 1126 }, { label: "R2", name: "论文模板.docx", chars: 2380 }], call, "2026-11-09T06:12:40.000Z");
  const planCall: AgentCall = { at: "2026-11-09T06:30:05.000Z", purpose: "plan", model: MODEL, tokens: { input: 9120, output: 3890, cacheRead: 0, cacheWrite: 0 }, costUsd: real ? 0.11 : 0, estimated: false, mock: !real, note: null, requestId: null };
  ledger.push(planCall);
  if (params.get("agent") === "over") ledger.push({ at: now(), purpose: "plan", model: MODEL, tokens: { input: 4_600_000, output: 60_000, cacheRead: 0, cacheWrite: 0 }, costUsd: 19.6, estimated: false, mock: false, note: null, requestId: "req_mock_0" });
  runs.push({ id: uid(), rulesetId: rs.id, createdAt: planCall.at, thesisPath: "/Users/mock/Documents/毕业论文/论文初稿.docx", thesisName: "论文初稿.docx", thesisSha256: "mock", mode: "brief", mock: !real, sentChars: THESIS.briefChars, draft: sampleDraft(), call: planCall, result: null });
}

// ---------- 确认框的 ticket ----------

type Ticket = { kind: "extract"; docs: { label: string; name: string; chars: number }[] } | { kind: "plan"; rulesetId: string; path: string; mode: FormatMode };
const tickets = new Map<number, Ticket>();
let nextTicket = 1;
const take = (id: unknown) => {
  const t = tickets.get(id as number);
  tickets.delete(id as number);
  return t ?? fail("这次确认已经过期，请重新开始。");
};

const ROLE_KEYS = ["normal", "heading 1", "heading 2", "heading 3", "heading 4", "caption", "bibliography"];
const baseName = (p: string) => p.split("/").pop() ?? p;

export const agentHandlers: Record<string, (a: Args) => unknown> = {
  agent_status: () => status(),
  agent_key_set: ({ key }) => {
    const k = String(key).trim();
    if (!(k.startsWith("sk-ant-") && k.length >= 40 && /^[A-Za-z0-9_-]+$/.test(k))) fail("这看起来不是 Anthropic 的 API 密钥（应以 sk-ant- 开头）。到 console.anthropic.com 的 API Keys 里复制完整的一串。");
    // 模拟：只记「填过了」，不留密钥本身
    hasKey = true;
    return status();
  },
  agent_key_clear: () => {
    hasKey = false;
    return status();
  },
  agent_send_mode_set: ({ mode }) => {
    sendMode = mode === "full" ? "full" : "brief";
    return status();
  },
  agent_budget_set: ({ usd }) => {
    const n = usd as number;
    if (!Number.isFinite(n) || n < 1 || n > 50) fail("月度上限要在 $1 到 $50 之间。");
    budgetUsd = Math.round(n * 100) / 100;
    return status();
  },
  format_rulesets_list: (): FormatRulesetSummary[] =>
    rulesets.map((r) => ({ id: r.id, name: r.name, createdAt: r.createdAt, mock: r.mock, docs: r.docs.map((d) => d.name), requirements: r.requirements.length, runs: runs.filter((x) => x.rulesetId === r.id).length })),
  format_ruleset_get: ({ id }) => rulesetView(findRuleset(id)),
  format_ruleset_delete: ({ id }) => {
    const i = rulesets.findIndex((r) => r.id === id);
    if (i >= 0) rulesets.splice(i, 1);
    for (let j = runs.length - 1; j >= 0; j--) if (runs[j].rulesetId === id) runs.splice(j, 1);
    return null;
  },
  format_ruleset_rename: ({ id, name }) => {
    const r = findRuleset(id);
    const n = String(name).trim();
    if (!n || n.length > 80) fail("名称不能空着，也不要超过 80 个字。");
    r.name = n;
    return rulesetView(r);
  },
  format_extract_prepare: ({ paths }) => {
    const list = paths as string[];
    if (list.length === 0) fail("还没有选要求文档");
    const docs = list.map((p, i) => ({ label: `R${i + 1}`, name: baseName(p), chars: i === 0 ? 1126 : 2380 + i * 317 }));
    const ticket = nextTicket++;
    tickets.set(ticket, { kind: "extract", docs });
    const input = Math.ceil(docs.reduce((n, d) => n + d.chars, 0) * 1.1) + 2600;
    return { ticket, docs, estimate: estimate(input, 14_000, 32_000), mock: !real, hasKey };
  },
  format_extract_run: ({ ticket }) => {
    const t = take(ticket);
    if (t.kind !== "extract") return fail("这次确认已经过期，请重新开始。");
    return modelCall("extract", () => rulesetView(newRuleset(t.docs, record("extract", 6460, 11_300))));
  },
  format_requirement_set: ({ rulesetId, requirementId, enabled, settings }) => {
    const rs = findRuleset(rulesetId);
    const r = rs.requirements.find((x) => x.id === requirementId) ?? fail("这条要求不存在了");
    if (typeof enabled === "boolean") r.enabled = enabled;
    if (settings) {
      if (r.handling !== "auto") fail("只有「自动」的要求有设置。");
      const next = (settings as FormatSettingPatch[]).map((s) => {
        const err = check(s.target, s.prop, s.value);
        if (err) fail(`「${label(TARGETS, s.target)} · ${label(PROPS, s.prop)} = ${s.value}」：${err}`);
        return { target: s.target, prop: s.prop, value: s.value.trim(), error: null };
      });
      if (next.length === 0) fail("至少留一个设置；不想应用这条要求就把它关掉。");
      r.settings = next;
    }
    return rulesetView(rs);
  },
  format_plan_prepare: ({ rulesetId, path, mode }) => {
    const rs = findRuleset(rulesetId);
    const p = String(path);
    if (!/\.docx$/i.test(p)) fail(`${baseName(p)} 不是 .docx。旧版 .doc 请先在 Word 里另存为 .docx。`);
    if (assemble(rs.requirements).lines.every((l) => l.requirement === null)) fail("这套规范里没有能自动应用的设置，排不了。");
    const m = (mode as FormatMode | undefined) ?? sendMode;
    const sentChars = m === "brief" ? THESIS.briefChars : THESIS.totalChars;
    const ticket = nextTicket++;
    tickets.set(ticket, { kind: "plan", rulesetId: rs.id, path: p, mode: m });
    return { ticket, thesisName: baseName(p), mode: m, paragraphs: THESIS.paragraphs, totalChars: THESIS.totalChars, sentChars, estimate: estimate(Math.ceil(sentChars * 1.1) + 5200, 7000, 24_000), mock: !real, hasKey };
  },
  format_plan_run: ({ ticket }) => {
    const t = take(ticket);
    if (t.kind !== "plan") return fail("这次确认已经过期，请重新开始。");
    return modelCall("plan", () => {
      const sentChars = t.mode === "brief" ? THESIS.briefChars : THESIS.totalChars;
      const call = record("plan", Math.ceil(sentChars * 1.1) + 5200, 5400);
      const run: MRun = { id: uid(), rulesetId: t.rulesetId, createdAt: now(), thesisPath: t.path, thesisName: baseName(t.path), thesisSha256: "mock", mode: t.mode, mock: !real, sentChars, draft: sampleDraft(), call, result: null };
      runs.unshift(run);
      return runView(run);
    });
  },
  format_run_get: ({ rulesetId, runId }) => runView(findRun(rulesetId, runId)),
  format_run_delete: ({ rulesetId, runId }) => {
    const i = runs.findIndex((r) => r.rulesetId === rulesetId && r.id === runId);
    if (i >= 0) runs.splice(i, 1);
    return null;
  },
  format_plan_update: ({ rulesetId, runId, draft }) => {
    const run = findRun(rulesetId, runId);
    const d = draft as FormatDraft;
    for (const r of d.paragraphRules) {
      const bad = (why: string) => fail(`文字规则「${r.pattern}」：${why}`);
      if (!r.pattern.trim()) bad("正则空着");
      try {
        new RegExp(r.pattern);
      } catch {
        bad("正则写得不对：括号或转义没有配对");
      }
      if (!ROLE_KEYS.includes(r.style.toLowerCase())) bad(`目标样式只能是 ${ROLE_KEYS.join(" / ")}`);
    }
    for (const m of d.styleMap) if (!ROLE_KEYS.includes(m.to.toLowerCase()) || !m.from.trim()) fail(`样式映射「${m.from} → ${m.to}」不对。`);
    if (d.protectSections.some((p) => p.section === 0)) fail("分节从 1 开始数。");
    const order = [d.layout.frontStart, d.layout.bodyStart, d.layout.backStart].filter((a): a is FormatAnchor => !!a).map((a) => a.paragraph);
    if (order.some((n, i) => i > 0 && n <= order[i - 1])) fail("分区的起点要按「前置部分 → 正文 → 正文之后」的顺序，段落编号从 1 开始、不能重复");
    // 摘要、观察、丢掉的项是模型那次给的，不让界面改
    run.draft = { ...structuredClone(d), summary: run.draft.summary, observations: run.draft.observations, problems: run.draft.problems };
    run.result = null;
    return runView(run);
  },
  format_apply: ({ rulesetId, runId }) => {
    const rs = findRuleset(rulesetId);
    const run = findRun(rulesetId, runId);
    if (assemble(rs.requirements).lines.every((l) => l.requirement === null)) fail("要求清单里没有一条能自动应用的设置（都关掉了，或值读不懂），没有可排的。");
    return new Promise((resolve) =>
      setTimeout(() => {
        run.result = result(run, rs);
        resolve(runView(run));
      }, 700),
    );
  },
  format_cancel: () => {
    cancelled = true;
    return null;
  },
};
