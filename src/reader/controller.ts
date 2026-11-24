// 阅读器的 pdf.js 部分：建 PDFViewer、翻页缩放查找、记阅读位置，以及在页面上画标注、把选区换成 PDF 坐标。
// 只管显示与坐标换算；标注怎么合并、怎么排序、怎么存，都在 Rust（src-tauri/src/reader.rs）。
import type { PDFDocumentLoadingTask, PDFDocumentProxy } from "pdfjs-dist";
import type { Annotation, ReadingState } from "@/shared/ipc";
import { openDocument, type PdfLib, type ViewerLib } from "./pdfjs";

type Rect = [number, number, number, number];

/** pdf.js 的 PageViewport 里用到的部分 */
type Viewport = {
  width: number;
  height: number;
  scale: number;
  viewBox: number[];
  convertToViewportPoint(x: number, y: number): number[];
  convertToPdfPoint(x: number, y: number): number[];
};
type PageView = { id: number; div: HTMLDivElement; viewport: Viewport };

export type SelectionInfo = {
  pageIndex: number;
  pageLabel: string | null;
  pageBox: Rect;
  rects: Rect[];
  text: string;
  charOffset: number;
  /** 选区末尾（窗口坐标），浮动工具条放这里 */
  anchor: { x: number; y: number };
};

export type FindResult = { state: number; current: number; total: number };

export type Callbacks = {
  onPages: (count: number, labels: string[] | null) => void;
  onPage: (pageNumber: number) => void;
  onScale: (value: string) => void;
  onLocation: (s: ReadingState) => void;
  onFind: (r: FindResult) => void;
  onError: (msg: string) => void;
};

/** FindState：0 找到，1 没找到，2 从头再找，3 查找中 */
export const FIND_NOT_FOUND = 1;
export const FIND_PENDING = 3;

const NOTE_SIZE = 22; // 便签图标边长（pt），与 Zotero 相同

export class PdfController {
  private readonly viewer: InstanceType<ViewerLib["PDFViewer"]>;
  private readonly eventBus: InstanceType<ViewerLib["EventBus"]>;
  private readonly linkService: InstanceType<ViewerLib["PDFLinkService"]>;
  private task: PDFDocumentLoadingTask | null = null;
  private doc: PDFDocumentProxy | null = null;
  private labels: string[] | null = null;
  private annotations: Annotation[] = [];
  private selected: number | null = null;
  private restore: ReadingState | null = null;
  private destroyed = false;
  private readonly cleanup: (() => void)[] = [];

  constructor(
    private readonly lib: { pdfjs: PdfLib; viewer: ViewerLib },
    private readonly container: HTMLDivElement,
    private readonly cb: Callbacks,
  ) {
    const { EventBus, PDFLinkService, PDFFindController, PDFViewer, LinkTarget } = lib.viewer;
    // lang 置空：pdf.js 量文字宽度的画布不带语言；不置空，文字层会继承界面的 zh-CN，「sans-serif」换成苹方，
    // 比页面上的字宽（实测宽 8.6%），选区和高亮就对不齐。设在外层容器上（pdf.js 换文档时会清掉内层的 lang）；
    // PDF 自带语言时 pdf.js 给内层设上那个语言，量的时候也用它，两边仍一致。
    container.setAttribute("lang", "");
    const eventBus = new EventBus();
    const linkService = new PDFLinkService({ eventBus, externalLinkTarget: LinkTarget.BLANK, externalLinkRel: "noopener noreferrer nofollow" });
    const findController = new PDFFindController({ eventBus, linkService });
    this.viewer = new PDFViewer({
      container,
      viewer: container.firstElementChild as HTMLDivElement,
      eventBus,
      linkService,
      findController,
      // 只显示 PDF 自带的标注与链接，不做表单、不用 pdf.js 的标注编辑（标注存在本库，不写回 PDF）
      annotationMode: lib.pdfjs.AnnotationMode.ENABLE,
      annotationEditorMode: lib.pdfjs.AnnotationEditorType.DISABLE,
    });
    linkService.setViewer(this.viewer);
    this.eventBus = eventBus;
    this.linkService = linkService;

    eventBus.on("pagesinit", () => this.onPagesInit());
    // 缩放后 pdf.js 会清掉页面里不认识的子元素：每次画页都把标注层补回去
    const redraw = ({ pageNumber }: { pageNumber: number }) => this.drawPage(pageNumber - 1);
    eventBus.on("pagerender", redraw);
    eventBus.on("pagerendered", redraw);
    // 开发时自检：文字层（选区坐标的来源）必须和画布一样大，否则高亮会和正文错位（reader.css 的 box-sizing）
    if (import.meta.env.DEV) eventBus.on("textlayerrendered", ({ pageNumber }: { pageNumber: number }) => this.checkAlignment(pageNumber - 1));
    eventBus.on("pagechanging", ({ pageNumber }: { pageNumber: number }) => cb.onPage(pageNumber));
    eventBus.on("scalechanging", ({ scale, presetValue }: { scale: number; presetValue?: string }) => cb.onScale(presetValue ?? String(scale)));
    eventBus.on("updateviewarea", ({ location }: { location: { pageNumber: number; scale: number | string; top: number; left: number; rotation: number } }) => {
      const scale = typeof location.scale === "number" ? location.scale / 100 : location.scale;
      cb.onLocation({ pageIndex: location.pageNumber - 1, scale, top: location.top, left: location.left, rotation: location.rotation });
    });
    eventBus.on("updatefindmatchescount", ({ matchesCount }: { matchesCount: { current: number; total: number } }) =>
      cb.onFind({ state: 0, current: matchesCount.current, total: matchesCount.total }),
    );
    eventBus.on("updatefindcontrolstate", ({ state, matchesCount }: { state: number; matchesCount: { current: number; total: number } }) =>
      cb.onFind({ state, current: matchesCount?.current ?? 0, total: matchesCount?.total ?? 0 }),
    );

    // 触控板捏合 / ⌘+滚轮缩放（Chromium 发带 ctrlKey 的 wheel，WebKit 发 gesture 事件）
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      this.viewer.updateScale({ drawingDelay: 300, scaleFactor: Math.exp(-e.deltaY / 100), origin: [e.clientX, e.clientY] });
    };
    let gestureScale = 1;
    const onGestureStart = (e: Event) => {
      e.preventDefault();
      gestureScale = 1;
    };
    const onGestureChange = (e: Event) => {
      const g = e as Event & { scale: number; clientX: number; clientY: number };
      e.preventDefault();
      this.viewer.updateScale({ drawingDelay: 300, scaleFactor: g.scale / gestureScale, origin: [g.clientX, g.clientY] });
      gestureScale = g.scale;
    };
    // PDF 里的网址链接交给系统浏览器（WebView 里不跳走）
    const onClick = (e: MouseEvent) => {
      const a = (e.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
      if (a && /^(https?:|mailto:)/i.test(a.href)) {
        e.preventDefault();
        e.stopPropagation();
        void this.openExternal(a.href);
      }
    };
    container.addEventListener("wheel", onWheel, { passive: false });
    container.addEventListener("gesturestart", onGestureStart);
    container.addEventListener("gesturechange", onGestureChange);
    container.addEventListener("click", onClick, true);
    this.cleanup.push(() => {
      container.removeEventListener("wheel", onWheel);
      container.removeEventListener("gesturestart", onGestureStart);
      container.removeEventListener("gesturechange", onGestureChange);
      container.removeEventListener("click", onClick, true);
    });
  }

  /** 由界面设置：打开外部链接 */
  openExternal: (url: string) => Promise<void> = async () => {};

  async open(data: ArrayBuffer, state: ReadingState | null) {
    this.restore = state;
    this.task = openDocument(this.lib.pdfjs, data);
    let doc: PDFDocumentProxy;
    try {
      doc = await this.task.promise;
    } catch (e) {
      if (!this.destroyed) this.cb.onError(pdfError(e));
      return;
    }
    if (this.destroyed) return; // destroy() 已经关掉了 task
    this.doc = doc;
    this.labels = await doc.getPageLabels().catch(() => null);
    this.viewer.setDocument(doc);
    this.linkService.setDocument(doc, null);
    this.cb.onPages(doc.numPages, this.labels);
  }

  private onPagesInit() {
    const s = this.restore;
    this.restore = null;
    this.viewer.currentScaleValue = s?.scale !== undefined ? String(s.scale) : "auto";
    if (s && this.doc) {
      const pageNumber = Math.min(Math.max(1, s.pageIndex + 1), this.doc.numPages);
      this.viewer.scrollPageIntoView({ pageNumber, destArray: [null, { name: "XYZ" }, s.left ?? null, s.top ?? null, null], allowNegativeOffset: true });
    }
  }

  destroy() {
    this.destroyed = true;
    for (const f of this.cleanup) f();
    // 先让视图放下文档（停止渲染），再关掉解析 worker
    this.viewer.setDocument(null);
    this.linkService.setDocument(null);
    void this.task?.destroy();
  }

  // ---------------- 翻页、缩放、查找 ----------------

  get pageCount() {
    return this.doc?.numPages ?? 0;
  }
  pageLabel(index: number): string | null {
    return this.labels?.[index] ?? null;
  }
  goToPage(n: number) {
    if (n >= 1 && n <= this.pageCount) this.viewer.currentPageNumber = n;
  }
  /** 页码框里输入的是页面标签（如 xii、47）时按标签找，否则按第几页 */
  goToLabel(input: string): boolean {
    const t = input.trim();
    const byLabel = this.labels ? this.labels.indexOf(t) : -1;
    const n = byLabel >= 0 ? byLabel + 1 : Number(t);
    if (!Number.isInteger(n) || n < 1 || n > this.pageCount) return false;
    this.goToPage(n);
    return true;
  }
  next() {
    this.viewer.nextPage();
  }
  previous() {
    this.viewer.previousPage();
  }
  setScale(value: string) {
    this.viewer.currentScaleValue = value;
  }
  zoom(steps: number) {
    this.viewer.updateScale({ steps });
  }
  find(query: string, again: boolean, previous: boolean) {
    this.eventBus.dispatch("find", { source: this, type: again ? "again" : "", query, caseSensitive: false, entireWord: false, highlightAll: true, findPrevious: previous, matchDiacritics: false });
  }
  closeFind() {
    this.eventBus.dispatch("findbarclose", { source: this });
  }

  // ---------------- 标注 ----------------

  setAnnotations(list: Annotation[], selected: number | null) {
    this.annotations = list;
    this.selected = selected;
    for (let i = 0; i < this.pageCount; i++) this.drawPage(i);
  }

  select(id: number | null) {
    this.selected = id;
    for (const el of this.container.querySelectorAll<HTMLElement>(".rl-annots [data-id]")) {
      el.classList.toggle("rl-selected", Number(el.dataset.id) === id);
    }
  }

  /** 滚到标注所在位置（上沿留一点空） */
  scrollTo(a: Annotation) {
    const pv = this.pageView(a.pageIndex);
    const ys = [...(a.position?.rects ?? []).flatMap((r) => [r[1], r[3]]), ...(a.position?.paths ?? []).flatMap((p) => p.filter((_, i) => i % 2 === 1))];
    const pageTop = pv?.viewport.viewBox[3];
    const top = ys.length ? Math.max(...ys) + 40 : null;
    this.viewer.scrollPageIntoView({
      pageNumber: a.pageIndex + 1,
      destArray: top === null ? undefined : [null, { name: "XYZ" }, null, pageTop === undefined ? top : Math.min(top, pageTop), null],
      allowNegativeOffset: true,
    });
  }

  private pageView(index: number): PageView | null {
    return (this.viewer.getPageView(index) as PageView | undefined) ?? null;
  }

  private checkAlignment(index: number) {
    const div = this.pageView(index)?.div;
    const text = div?.querySelector(".textLayer")?.getBoundingClientRect();
    const canvas = div?.querySelector(".canvasWrapper")?.getBoundingClientRect();
    if (!text?.width || !canvas?.width) return;
    const off = Math.max(Math.abs(text.width - canvas.width), Math.abs(text.height - canvas.height), Math.abs(text.left - canvas.left), Math.abs(text.top - canvas.top));
    if (off > 1) console.warn(`阅读器：第 ${index + 1} 页文字层和画布差了 ${off.toFixed(1)}px，高亮会和正文错位`);
  }

  private drawPage(index: number) {
    const pv = this.pageView(index);
    if (!pv?.div) return;
    let layer = pv.div.querySelector<HTMLDivElement>(":scope > .rl-annots");
    if (!layer) {
      layer = document.createElement("div");
      layer.className = "rl-annots";
      pv.div.append(layer);
    }
    layer.replaceChildren();
    const vp = pv.viewport;
    const box = (r: number[]) => {
      const [x1, y1] = vp.convertToViewportPoint(r[0], r[1]);
      const [x2, y2] = vp.convertToViewportPoint(r[2], r[3]);
      return {
        left: `${(Math.min(x1, x2) / vp.width) * 100}%`,
        top: `${(Math.min(y1, y2) / vp.height) * 100}%`,
        width: `${(Math.abs(x2 - x1) / vp.width) * 100}%`,
        height: `${(Math.abs(y2 - y1) / vp.height) * 100}%`,
      };
    };
    for (const a of this.annotations) {
      if (a.pageIndex !== index || !a.position) continue;
      const color = a.color ?? "#ffd400";
      const add = (cls: string, r: number[]) => {
        const el = document.createElement("div");
        el.className = `rl-${cls}${a.id === this.selected ? " rl-selected" : ""}`;
        el.dataset.id = String(a.id);
        Object.assign(el.style, box(r));
        el.style.setProperty("--rl-color", color);
        layer.append(el);
      };
      const rects = a.position.rects ?? [];
      switch (a.kind) {
        case "highlight":
          rects.forEach((r) => add("hl", r));
          break;
        case "underline":
          rects.forEach((r) => add("ul", r));
          break;
        case "note":
          rects.slice(0, 1).forEach((r) => add("note", r));
          break;
        case "ink":
          layer.append(this.inkSvg(a, vp, color));
          break;
        default:
          rects.forEach((r) => add("box", r));
      }
    }
  }

  private inkSvg(a: Annotation, vp: Viewport, color: string) {
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("viewBox", `0 0 ${vp.width} ${vp.height}`);
    svg.setAttribute("preserveAspectRatio", "none");
    svg.setAttribute("class", `rl-ink${a.id === this.selected ? " rl-selected" : ""}`);
    svg.dataset.id = String(a.id);
    for (const path of a.position?.paths ?? []) {
      const pts: string[] = [];
      for (let i = 0; i + 1 < path.length; i += 2) pts.push(vp.convertToViewportPoint(path[i], path[i + 1]).join(","));
      const line = document.createElementNS(ns, "polyline");
      line.setAttribute("points", pts.join(" "));
      line.setAttribute("stroke", color);
      line.setAttribute("stroke-width", String((a.position?.width ?? 2) * vp.scale));
      svg.append(line);
    }
    return svg;
  }

  // ---------------- 坐标换算 ----------------

  /** 页面内容区（不含边框）在窗口里的位置 */
  private contentBox(div: HTMLDivElement) {
    const r = div.getBoundingClientRect();
    return { left: r.left + div.clientLeft, top: r.top + div.clientTop, width: div.clientWidth, height: div.clientHeight };
  }

  private pageIndexOf(node: Node | null): number | null {
    const el = node instanceof Element ? node : node?.parentElement;
    const page = el?.closest<HTMLDivElement>(".page[data-page-number]");
    return page && this.container.contains(page) ? Number(page.dataset.pageNumber) - 1 : null;
  }

  /** 窗口坐标 → 某页的 PDF 坐标 */
  private toPdf(pv: PageView, x: number, y: number): [number, number] {
    const b = this.contentBox(pv.div);
    const [px, py] = pv.viewport.convertToPdfPoint(((x - b.left) / b.width) * pv.viewport.width, ((y - b.top) / b.height) * pv.viewport.height);
    return [px, py];
  }

  /** 点到了哪条标注（同一点有几条时取面积最小的） */
  hitTest(x: number, y: number): number | null {
    const index = this.pageIndexOf(document.elementFromPoint(x, y));
    const pv = index === null ? null : this.pageView(index);
    if (index === null || !pv) return null;
    const [px, py] = this.toPdf(pv, x, y);
    let best: { id: number; area: number } | null = null;
    for (const a of this.annotations) {
      if (a.pageIndex !== index || !a.position) continue;
      const pad = a.kind === "ink" ? (a.position.width ?? 2) + 3 : 1.5;
      const boxes = a.kind === "ink" ? (a.position.paths ?? []).map(pathBox) : (a.position.rects ?? []);
      for (const r of boxes) {
        const [x1, x2] = [Math.min(r[0], r[2]) - pad, Math.max(r[0], r[2]) + pad];
        const [y1, y2] = [Math.min(r[1], r[3]) - pad, Math.max(r[1], r[3]) + pad];
        if (px >= x1 && px <= x2 && py >= y1 && py <= y2) {
          const area = (x2 - x1) * (y2 - y1);
          if (!best || area < best.area) best = { id: a.id, area };
        }
      }
    }
    return best?.id ?? null;
  }

  /** 在点击处放便签：返回该页信息与图标框（PDF 坐标） */
  notePlacement(x: number, y: number): { pageIndex: number; pageLabel: string | null; pageBox: Rect; rect: Rect } | null {
    const index = this.pageIndexOf(document.elementFromPoint(x, y));
    const pv = index === null ? null : this.pageView(index);
    if (index === null || !pv) return null;
    const [px, py] = this.toPdf(pv, x, y);
    const half = NOTE_SIZE / 2;
    return { pageIndex: index, pageLabel: this.pageLabel(index), pageBox: viewBox(pv), rect: [px - half, py - half, px + half, py + half] };
  }

  /** 当前选区 → PDF 坐标的矩形（一段文字一个，Rust 再合并成行）。跨页返回 "cross-page"。 */
  selection(): SelectionInfo | "cross-page" | null {
    const sel = document.getSelection();
    if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
    const range = sel.getRangeAt(0);
    if (!this.container.contains(range.commonAncestorContainer)) return null;
    const index = this.pageIndexOf(range.startContainer);
    const end = this.pageIndexOf(range.endContainer);
    if (index === null) return null;
    if (end !== null && end !== index) return "cross-page";
    const pv = this.pageView(index);
    const textLayer = pv?.div.querySelector(".textLayer");
    if (!pv || !textLayer) return null;

    const rects: Rect[] = [];
    let last: DOMRect | null = null;
    const walker = document.createTreeWalker(textLayer, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
      if (!range.intersectsNode(n) || !n.data.trim()) continue;
      const part = document.createRange();
      part.selectNodeContents(n);
      if (range.comparePoint(n, 0) < 0) part.setStart(range.startContainer, range.startOffset);
      if (range.comparePoint(n, n.length) > 0) part.setEnd(range.endContainer, range.endOffset);
      for (const cr of part.getClientRects()) {
        if (cr.width < 0.5 || cr.height < 0.5) continue;
        const [x1, y1] = this.toPdf(pv, cr.left, cr.bottom);
        const [x2, y2] = this.toPdf(pv, cr.right, cr.top);
        rects.push([Math.min(x1, x2), Math.min(y1, y2), Math.max(x1, x2), Math.max(y1, y2)]);
        last = cr;
      }
    }
    if (!rects.length || !last) return null;
    const before = document.createRange();
    before.setStart(textLayer, 0);
    before.setEnd(range.startContainer, range.startOffset);
    return {
      pageIndex: index,
      pageLabel: this.pageLabel(index),
      pageBox: viewBox(pv),
      rects,
      text: sel.toString(),
      charOffset: before.toString().length,
      anchor: { x: last.right, y: last.bottom },
    };
  }
}

function viewBox(pv: PageView): Rect {
  const [a, b, c, d] = pv.viewport.viewBox;
  return [a, b, c, d];
}

function pathBox(p: number[]): number[] {
  const xs = p.filter((_, i) => i % 2 === 0);
  const ys = p.filter((_, i) => i % 2 === 1);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/** pdf.js 的英文错误 → 给用户看的中文 */
function pdfError(e: unknown): string {
  const name = (e as { name?: string })?.name;
  if (name === "PasswordException") return "这个 PDF 有密码保护，App 里还不能打开；请用「打开」交给系统程序";
  if (name === "InvalidPDFException") return "文件不是有效的 PDF（可能损坏了）";
  return `PDF 打不开：${String((e as Error)?.message ?? e)}`;
}
