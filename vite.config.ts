import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";
import { createReadStream, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

/**
 * pdf.js 运行时按需取的资源：CJK 等字体编码表（cmaps）、未嵌入字体的替代字体（standard_fonts）、
 * 图片解码与色彩管理（wasm、iccs）。开发时从 node_modules 直接给，打包时复制到 dist/pdfjs/。
 * 不带 PDF 内嵌脚本的解释器（quickjs）与无 wasm 时的后备脚本：App 不运行 PDF 里的脚本，WebKit 支持 wasm。
 */
function pdfjsAssets(): Plugin {
  const root = fileURLToPath(new URL("./node_modules/pdfjs-dist/", import.meta.url));
  const dirs = ["cmaps", "standard_fonts", "wasm", "iccs"];
  const wanted = (dir: string, name: string) => dir !== "wasm" || ["jbig2.wasm", "openjpeg.wasm", "qcms_bg.wasm"].includes(name) || name.startsWith("LICENSE");
  const types: Record<string, string> = { wasm: "application/wasm", ttf: "font/ttf" };
  return {
    name: "pdfjs-assets",
    configureServer(server) {
      server.middlewares.use("/pdfjs/", (req, res, next) => {
        const [dir, name, ...rest] = decodeURIComponent((req.url ?? "").split("?")[0]).replace(/^\/+/, "").split("/");
        if (!dirs.includes(dir) || !name || rest.length > 0 || name.includes("..") || !wanted(dir, name)) return next();
        const file = join(root, dir, name);
        try {
          if (!statSync(file).isFile()) return next();
        } catch {
          return next();
        }
        res.setHeader("Content-Type", types[name.split(".").pop() ?? ""] ?? "application/octet-stream");
        createReadStream(file).pipe(res);
      });
    },
    generateBundle() {
      for (const dir of dirs) {
        for (const name of readdirSync(join(root, dir)).filter((n) => wanted(dir, n))) {
          this.emitFile({ type: "asset", fileName: `pdfjs/${dir}/${name}`, source: readFileSync(join(root, dir, name)) });
        }
      }
    },
  };
}

// 固定端口：tauri.conf.json 的 devUrl 指向这里（避开 Pomelo 的 1420 与 Word PoC 的 3127）。
export default defineConfig({
  plugins: [react(), tailwindcss(), pdfjsAssets()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/poc/**", "**/local/**", "**/target/**"] },
  },
});
