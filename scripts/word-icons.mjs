// Word 选项卡的图标：lucide 图标（ISC）→ SVG（64px，描边 2，蓝色）。用法：node scripts/word-icons.mjs <目录> && swift scripts/word-icons.swift <目录>/*.svg
// 生成的 PNG 放进 src-tauri/word/images/（提交进 Git），模板生成时打包进 Pith.dotm。
import { writeFileSync } from "node:fs";
const root = new URL("../node_modules/lucide-react/dist/esm/icons/", import.meta.url).href;
const map = { citation: "quote", bibliography: "book-open-text", refresh: "refresh-cw", style: "swatch-book", zotero: "arrow-right-left", open: "app-window" };
const out = process.argv[2];
for (const [name, icon] of Object.entries(map)) {
  const { __iconData } = await import(root + icon + ".mjs");
  const body = __iconData.node.map(([tag, attrs]) => `<${tag} ${Object.entries(attrs).filter(([k]) => k !== "key").map(([k, v]) => `${k}="${v}"`).join(" ")}/>`).join("");
  writeFileSync(`${out}/${name}.svg`, `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="#3D6FD9" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`);
}
