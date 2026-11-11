// pdf.js 的加载与文档打开。阅读器第一次打开时才加载（单独的代码块，不拖慢启动）。
import * as pdfjs from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import "pdfjs-dist/web/pdf_viewer.css";
import "./reader.css";

export type ViewerLib = typeof import("pdfjs-dist/web/pdf_viewer.mjs");
export type PdfLib = typeof pdfjs;

let loading: Promise<{ pdfjs: PdfLib; viewer: ViewerLib }> | null = null;

/** pdf_viewer.mjs 从 globalThis.pdfjsLib 取核心库，所以先挂上再动态导入它。 */
export function loadPdfjs() {
  loading ??= (async () => {
    (globalThis as unknown as { pdfjsLib: PdfLib }).pdfjsLib = pdfjs;
    pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
    const viewer = await import("pdfjs-dist/web/pdf_viewer.mjs");
    return { pdfjs, viewer };
  })();
  return loading;
}

/** pdf.js 运行时资源（vite.config.ts 的 pdfjsAssets 提供），相对当前页面 */
const asset = (dir: string) => new URL(`pdfjs/${dir}/`, document.baseURI).href;

/** 打开 PDF：不运行 PDF 里的脚本（不建脚本管理器、不加载 quickjs）；中文等没嵌入编码表的字体用 cmaps。 */
export function openDocument(lib: PdfLib, data: ArrayBuffer) {
  return lib.getDocument({
    data,
    cMapUrl: asset("cmaps"),
    cMapPacked: true,
    standardFontDataUrl: asset("standard_fonts"),
    wasmUrl: asset("wasm"),
    iccUrl: asset("iccs"),
    enableXfa: false,
  });
}

/** 从文件库取 PDF（rl-file 协议）。出错时把 Rust 给的中文原因抛出来。 */
export async function fetchPdf(url: string): Promise<ArrayBuffer> {
  const r = await fetch(url);
  if (!r.ok) throw new Error((await r.text()) || `读取文件失败（${r.status}）`);
  return r.arrayBuffer();
}
