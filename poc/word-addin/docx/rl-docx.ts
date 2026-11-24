// 命令行：在 Word 外处理 Research Lab 引文。永不改写输入文件。
//   node docx/rl-docx.ts seed <输出.docx> [--style apa] [--zotero]
//   node docx/rl-docx.ts refresh <输入.docx> --out <输出.docx> [--style gbt7714-2015-numeric]
//   node docx/rl-docx.ts verify <输入.docx> [--zotero-baseline <另一份.docx>] [--json]
//   node docx/rl-docx.ts inspect <输入.docx>

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { STYLES } from "../src/styles.ts";
import { runsToText } from "../src/runs.ts";
import { citationSdts, findStore, listFields, listSdts, openDocx, sdtRuns, zoteroFields } from "./docx.ts";
import { refreshDocx, seedDocx, verifyDocx } from "./ops.ts";
import { library } from "./resources.ts";

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
}
const flag = (name: string) => process.argv.includes(name);
const now = () => new Date().toISOString();

function checkStyle(id: string | undefined) {
  if (id && !STYLES.some((s) => s.id === id)) {
    console.error(`未知样式 ${id}。可选：${STYLES.map((s) => s.id).join("、")}`);
    process.exit(2);
  }
}

const [cmd, file] = process.argv.slice(2);
if (!cmd || !file) {
  console.error("用法见文件头注释");
  process.exit(2);
}

if (cmd === "seed") {
  checkStyle(arg("--style"));
  if (existsSync(file)) {
    console.error(`拒绝覆盖已存在的文件：${file}`);
    process.exit(2);
  }
  writeFileSync(file, seedDocx({ style: arg("--style") ?? "apa", zotero: flag("--zotero"), library: library(), now: now() }));
  console.log(`已生成 ${file}`);
} else if (cmd === "refresh") {
  const out = arg("--out");
  checkStyle(arg("--style"));
  if (!out || resolve(out) === resolve(file)) {
    console.error("refresh 必须用 --out 指定另一个输出文件（不改写输入）");
    process.exit(2);
  }
  const { bytes, plan } = refreshDocx(readFileSync(file), { style: arg("--style"), library: library(), now: now() });
  writeFileSync(out, bytes);
  console.log(`已刷新 → ${out}（样式 ${plan.store.style}，引文 ${plan.citations.length} 处，改写 ${plan.citations.filter((c) => c.changed).length} 处，书目 ${plan.bibliography.texts.length} 条）`);
  for (const w of plan.warnings) console.log(`  提示：${w}`);
  for (const s of plan.skipped) console.log(`  跳过第 ${s.index + 1} 处：${s.reason}`);
} else if (cmd === "verify") {
  const r = verifyDocx(readFileSync(file), { library: library() });
  const base = arg("--zotero-baseline");
  if (base) {
    const b = verifyDocx(readFileSync(base), { library: library() });
    if (b.zotero.digest !== r.zotero.digest) {
      r.ok = false;
      r.problems.push("Zotero 域与基线文档不一致（被改动了）");
    }
  }
  if (flag("--json")) console.log(JSON.stringify(r, null, 2));
  else {
    console.log(`${r.ok ? "通过" : "不通过"}：样式 ${r.style}，引文 ${r.citations.length} 处，书目 ${r.bibliography.actual.length} 条，Zotero 引文域 ${r.zotero.citations} 个`);
    for (const c of r.citations) console.log(`  [${c.ok ? "✓" : "✗"}] ${c.actual}`);
    for (const t of r.bibliography.actual) console.log(`  书目：${t.replace(/\t/g, " ⇥ ")}`);
    for (const p of r.problems) console.log(`  问题：${p}`);
  }
  process.exit(r.ok ? 0 : 1);
} else if (cmd === "inspect") {
  const d = openDocx(readFileSync(file));
  const loc = findStore(d.files);
  console.log("数据：", loc ? ("error" in loc ? `错误 ${loc.error}` : `${loc.path} 样式 ${loc.store.style}，引文数据 ${Object.keys(loc.store.citations).length} 条，文献快照 ${Object.keys(loc.store.items).length} 条`) : "无");
  console.log("内容控件：");
  for (const s of listSdts(d.doc)) console.log(`  ${s.block ? "块" : "行内"} ${s.tag || "(无 tag)"}`);
  console.log("引文：");
  for (const s of citationSdts(d.doc)) console.log(`  ${s.tag} → ${runsToText(sdtRuns(s))}`);
  const z = zoteroFields(listFields(d.doc));
  console.log(`域：共 ${listFields(d.doc).length} 个，其中 Zotero 引文 ${z.citations}、书目 ${z.bibliographies}`);
} else {
  console.error(`未知命令 ${cmd}`);
  process.exit(2);
}
