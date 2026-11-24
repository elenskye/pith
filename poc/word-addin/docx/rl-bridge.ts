// Word 宏（VBA）的计算后端：AppleScriptTask → do shell script → 本脚本。
// 输入：argv[2] 一段 JSON；输出：按行的制表符分隔文本，VBA 好解析。
//   OK
//   C <序号> <新tag或空> <文字> <格式>      每处引文
//   B <悬挂缩进pt> <条数>
//   E <文字> <格式>                          每条书目
//   X <序号> <原因>                          跳过的控件
//   W <提示>
//   S <数据部件 XML>
// 文字字段转义：\\ \t \n \r。格式：「长度:标记」逗号分隔，标记 i 斜体 b 粗体 p 上标 d 下标 c 小型大写 u 下划线。
// 出错时只输出一行：ERR <原因>。

import { randomUUID } from "node:crypto";
import { DOMParser } from "@xmldom/xmldom";
import { planRefresh } from "../src/refresh.ts";
import { DEFAULT_STYLE, STYLES } from "../src/styles.ts";
import { emptyStore, parseStore, serializeStore, type DocStore } from "../src/store.ts";
import type { CiteItemRef, CslItem, Run } from "../src/types.ts";
import { library, libraryList, locales, styleXml } from "./resources.ts";

type Payload =
  | { cmd: "strings" }
  | { cmd: "styles" }
  | { cmd: "search"; q: string }
  | { cmd: "refresh"; style?: string; store?: string; tags: string[]; add?: { citationID: string; items: CiteItemRef[] } | null };

const esc = (s: string) => s.replace(/\\/g, "\\\\").replace(/\t/g, "\\t").replace(/\n/g, "\\n").replace(/\r/g, "\\r");

function fmt(runs: Run[]): string {
  return runs
    .map((r) => {
      let f = "";
      if (r.italic) f += "i";
      if (r.bold) f += "b";
      if (r.sup) f += "p";
      if (r.sub) f += "d";
      if (r.smallCaps) f += "c";
      if (r.underline) f += "u";
      return `${r.text.length}:${f}`;
    })
    .join(",");
}

function label(i: CslItem): string {
  const who = (i.author ?? []).map((a) => a.family ?? a.literal ?? "").filter(Boolean);
  const names = who.length > 2 ? `${who[0]} 等` : who.join("、");
  const year = i.issued?.["date-parts"]?.[0]?.[0] ?? "";
  return `${names} ${year}　${i.title ?? ""}`.trim();
}

// 宏里的中文提示从这里取：Mac 的 VBA 编辑器导入 .bas 时会把中文变成乱码
const MESSAGES: Record<string, string> = {
  search: "搜索文献：输入作者、标题或年份（留空列出全部）",
  pick: "输入编号；多条用逗号分隔；页码写在冒号后，例如 1,3:17",
  nohit: "没有找到匹配的文献。",
  incite: "光标在一处引文里。请先把光标移到引文外再插入。",
  hasbib: "文档里已经有参考文献表了，点「刷新」即可。",
  style: "选择引文样式（输入编号）：",
  done: "Research Lab：已刷新",
  badpick: "编号无效：",
};

const parseXml = (xml: string) => new DOMParser().parseFromString(xml, "application/xml") as unknown as Document;

function main(): string[] {
  const p = JSON.parse(process.argv[2] ?? "{}") as Payload;
  if (p.cmd === "strings") return ["OK", ...Object.entries(MESSAGES).map(([k, v]) => `M\t${k}\t${esc(v)}`)];
  if (p.cmd === "styles") return ["OK", ...STYLES.map((s) => `Y\t${s.id}\t${esc(s.label)}`)];
  if (p.cmd === "search") {
    const q = (p.q ?? "").trim().toLowerCase();
    const hits = libraryList().filter((i) => !q || JSON.stringify([i.title, i.author, i.issued, i["container-title"]]).toLowerCase().includes(q));
    return ["OK", ...hits.slice(0, 9).map((i) => `I\t${i.id}\t${esc(label(i))}`)];
  }
  if (p.cmd === "refresh") {
    const now = new Date().toISOString();
    let store: DocStore = p.store ? parseStore(p.store, parseXml) : emptyStore(p.style || DEFAULT_STYLE, now);
    if (p.style) store = { ...store, style: p.style };
    if (p.add) store.citations[p.add.citationID] = { items: p.add.items, created: now };
    const plan = planRefresh({
      // 宏每次全部改写，签名留空
      doc: p.tags.map((tag) => ({ tag, signature: "" })),
      store,
      library: library(),
      styleXml: styleXml(store.style),
      locales: locales(),
      newId: () => randomUUID(),
      now,
    });
    const out = ["OK"];
    for (const c of plan.citations) out.push(`C\t${c.index}\t${c.retag ?? ""}\t${esc(c.text)}\t${fmt(c.runs)}`);
    out.push(`B\t${plan.bibliography.format.hangingIndentPt}\t${plan.bibliography.entries.length}`);
    const entries = plan.bibliography.entries.length ? plan.bibliography.entries : [[{ text: "（暂无引文）" }]];
    for (const e of entries) out.push(`E\t${esc(e.map((r) => r.text).join(""))}\t${fmt(e)}`);
    for (const s of plan.skipped) out.push(`X\t${s.index}\t${esc(s.reason)}`);
    for (const w of plan.warnings) out.push(`W\t${esc(w)}`);
    // CustomXMLParts.Add 不接受 XML 声明里的 encoding，去掉声明
    out.push(`S\t${esc(serializeStore(plan.store).replace(/^<\?xml[^>]*\?>/, ""))}`);
    return out;
  }
  throw new Error(`未知命令 ${(p as { cmd: string }).cmd}`);
}

try {
  process.stdout.write(main().join("\n"));
} catch (e) {
  process.stdout.write(`ERR\t${esc(e instanceof Error ? e.message : String(e))}`);
}
