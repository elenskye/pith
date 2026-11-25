// 瀑布流：卡片一张张放进最矮的那一列。
// 不用 CSS 的 columns（1.0.5）：WebKit 里多栏的东西一抬起、一带影子，就会被画到旁边那一栏去（面板滑出来时卡片旁边一片怪影子），
// 宽度一变整墙重排、卡片原地跳。这里每张卡绝对定位：宽度变了（右边的面板滑出来）就算新位置，卡片从原处滑过去（FLIP），
// 不重建 DOM、不重演入场。
import { useLayoutEffect, useRef, type RefObject } from "react";

export type WallOptions = { min: number; max: number; gap: number; rowGap: number };
const WALL: WallOptions = { min: 240, max: 4, gap: 18, rowGap: 16 };

/** 这么宽放几列、每列多宽：每列至少 min，最多 max 列 */
export function columnsFor(width: number, o: Partial<WallOptions> = {}) {
  const { min, max, gap } = { ...WALL, ...o };
  const cols = Math.max(1, Math.min(max, Math.floor((width + gap) / (min + gap))));
  return { cols, colW: Math.max(0, (width - gap * (cols - 1)) / cols) };
}

/** 按顺序放进最矮的那一列（一样矮放左边）；返回每张的位置和整面墙的高 */
export function place(heights: number[], cols: number, colW: number, o: Partial<WallOptions> = {}) {
  const { gap, rowGap } = { ...WALL, ...o };
  const tops: number[] = new Array(cols).fill(0);
  const pos = heights.map((h) => {
    let c = 0;
    for (let k = 1; k < cols; k++) if (tops[k] < tops[c] - 0.5) c = k;
    const p = { x: Math.round(c * (colW + gap)), y: Math.round(tops[c]) };
    tops[c] += h + rowGap;
    return p;
  });
  return { pos, height: heights.length ? Math.max(...tops) - rowGap : 0 };
}

const MOVE_MS = 500;
const MOVE_EASE = "cubic-bezier(.23,1,.32,1)"; // 和面板滑出来的 --out 一样

/**
 * 给 ref 那面墙排版：它的每个子元素是一张卡。sig 是卡片清单（变了就重新挂观察）。
 * ruler 给了就按它的宽排（页面的 padding 在过渡时，墙的宽一帧帧在变，尺一下就到最后的宽：排一次，卡片滑过去），再减去滚动条。
 * 宽变了、卡片变高变矮都会重排；挪了位置的卡从原处滑过去，滑的时候墙上加 is-moving（卡不响应悬停，免得经过鼠标时一抬一落）。
 */
export function useMasonry(ref: RefObject<HTMLElement | null>, sig: string, ruler?: RefObject<HTMLElement | null>) {
  const seen = useRef(new WeakSet<Element>());
  useLayoutEffect(() => {
    const wall = ref.current;
    if (!wall) return;
    const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
    let moving = 0;
    const layout = () => {
      const cards = [...wall.children] as HTMLElement[];
      const box = wall.parentElement;
      const width = ruler?.current ? ruler.current.clientWidth - (box ? box.offsetWidth - box.clientWidth : 0) : wall.clientWidth;
      const { cols, colW } = columnsFor(width);
      const w = `${colW}px`;
      for (const c of cards) if (c.style.width !== w) c.style.width = w;
      const { pos, height } = place(
        cards.map((c) => c.offsetHeight),
        cols,
        colW,
      );
      const h = `${height}px`;
      if (wall.style.height !== h) wall.style.height = h;
      // 先量（含正在滑的那一段），再写，再量一次，最后从旧处滑过来
      const moves = cards.flatMap((c, i) => {
        const l = `${pos[i].x}px`;
        const t = `${pos[i].y}px`;
        if (c.style.left === l && c.style.top === t) return [];
        return [{ c, l, t, from: seen.current.has(c) && !still() ? c.getBoundingClientRect() : null }];
      });
      for (const m of moves) {
        if (m.from) for (const a of m.c.getAnimations()) if (a.id === "masonry") a.cancel();
        m.c.style.left = m.l;
        m.c.style.top = m.t;
      }
      for (const c of cards) seen.current.add(c);
      const slides = moves.flatMap(({ c, from }) => {
        if (!from) return [];
        const to = c.getBoundingClientRect();
        const dx = from.left - to.left;
        const dy = from.top - to.top;
        return Math.abs(dx) + Math.abs(dy) < 1 ? [] : [{ c, dx, dy }];
      });
      if (!slides.length) return;
      const run = ++moving;
      wall.classList.add("is-moving");
      const anims = slides.map(({ c, dx, dy }) => {
        const a = c.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: MOVE_MS, easing: MOVE_EASE });
        a.id = "masonry";
        return a.finished.catch(() => undefined);
      });
      void Promise.all(anims).then(() => {
        if (run === moving) wall.classList.remove("is-moving");
      });
    };
    layout();
    const ro = new ResizeObserver(layout);
    ro.observe(ruler?.current ?? wall);
    for (const c of wall.children) ro.observe(c);
    return () => ro.disconnect();
  }, [ref, sig, ruler]);
}
