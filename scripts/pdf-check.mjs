// PDF 兼容性自检：用 App 里同一个 pdf.js 打开某个目录下的全部 PDF，只打印汇总数字（不打印标题、文件名、正文）。
// 用法：node scripts/pdf-check.mjs ~/ResearchLab-test/zotero-copy/storage
// 只读；PDF 里的脚本不运行（pdf.js 不启用脚本）。Node 里用 pdf.js 的 legacy 构建（官方建议），解析逻辑与 App 相同。
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const dist = fileURLToPath(new URL("../node_modules/pdfjs-dist/", import.meta.url));
const pdfjs = await import(new URL("../node_modules/pdfjs-dist/legacy/build/pdf.mjs", import.meta.url).href);
const root = process.argv[2];
if (!root) {
  console.error("用法：node scripts/pdf-check.mjs <目录>");
  process.exit(2);
}

const files = [];
const walk = (d) => {
  for (const n of readdirSync(d)) {
    const p = join(d, n);
    const s = statSync(p);
    if (s.isDirectory()) walk(p);
    else if (n.toLowerCase().endsWith(".pdf")) files.push([p, s.size]);
  }
};
walk(root);

// pdf.js 的警告按类别计数（数字抹掉），不打印原文
const warnings = new Map();
const count = (k) => warnings.set(k, (warnings.get(k) ?? 0) + 1);
const { warn, log } = console;
console.warn = (...a) => count(String(a[0]).replace(/[0-9]+/g, "#").slice(0, 70));
console.log = (...a) => count("log: " + String(a[0]).replace(/[0-9]+/g, "#").slice(0, 60));

const s = { pdfs: files.length, mb: 0, opened: 0, failed: {}, pages: 0, maxPages: 0, pageLabels: 0, labelsNotPageNumbers: 0, textOnFirstPages: 0, noText: 0, cjk: 0, docLanguage: 0, ms: [] };
for (const [p, size] of files) {
  s.mb += size / 1e6;
  const t0 = performance.now();
  try {
    const task = pdfjs.getDocument({
      data: new Uint8Array(readFileSync(p)),
      cMapUrl: dist + "cmaps/",
      cMapPacked: true,
      standardFontDataUrl: dist + "standard_fonts/",
      wasmUrl: dist + "wasm/",
      iccUrl: dist + "iccs/",
      enableXfa: false,
    });
    const doc = await task.promise;
    s.opened++;
    s.pages += doc.numPages;
    s.maxPages = Math.max(s.maxPages, doc.numPages);
    const labels = await doc.getPageLabels();
    if (labels) {
      s.pageLabels++;
      if (labels.some((l, i) => l !== String(i + 1))) s.labelsNotPageNumbers++;
    }
    if ((await doc.getMetadata().catch(() => null))?.info?.Language) s.docLanguage++;
    let chars = 0;
    let cjk = 0;
    for (let i = 1; i <= Math.min(3, doc.numPages); i++) {
      for (const it of (await (await doc.getPage(i)).getTextContent()).items) {
        if (!it.str) continue;
        chars += it.str.length;
        cjk += (it.str.match(/[㐀-鿿]/g) ?? []).length;
      }
    }
    if (chars > 50) s.textOnFirstPages++;
    else s.noText++;
    if (cjk > 20) s.cjk++;
    await task.destroy();
  } catch (e) {
    const k = e?.name ?? "Error";
    s.failed[k] = (s.failed[k] ?? 0) + 1;
  }
  s.ms.push(Math.round(performance.now() - t0));
}
console.warn = warn;
console.log = log;
const ms = s.ms.sort((a, b) => a - b);
s.ms = { median: ms[Math.floor(ms.length / 2)] ?? 0, max: ms.at(-1) ?? 0 };
s.mb = Math.round(s.mb * 10) / 10;
console.log(JSON.stringify(s, null, 1));
console.log("pdf.js 警告（按类别计数）：", JSON.stringify([...warnings].sort((a, b) => b[1] - a[1]).slice(0, 10)));
