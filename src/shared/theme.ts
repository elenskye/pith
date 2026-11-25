// 外观：配色（data-palette）和明暗（data-theme）写在根上，vendor/pith/pith.css 照它换变量（Tailwind 的 dark: 也看 data-theme）。
// 设置 › 外观、⌘K、对话里都能换；记在这台电脑上（localStorage），跟随系统时听系统的明暗变化。
//
// 换的时候新的颜色从点的地方像墨一样洇开（palette-lake 的 ink）：用 View Transitions，浏览器先拍下旧的样子，
// 样式只重算一次，洇开是在两张图上做遮罩，不逐帧重算整页。1.0.2 用 @property 让十四个颜色变量过渡，
// 文献库一长（1500 行）每帧要重算全部元素，WebKit 里掉到一秒几帧；整页的水面扭曲更慢（每帧约 100 毫秒）。
// 没有 View Transitions、或者减弱动态效果时，直接换。
import { useEffect, useState } from "react";
import { flushSync } from "react-dom";
import pithCss from "@/vendor/pith/pith.css?raw";

export type Palette = "paper-sea" | "mist-ochre";
export type ThemePref = "system" | "light" | "dark";
export type Look = { palette: Palette; theme: ThemePref };

export const PALETTES: { id: Palette; name: string; line: string }[] = [
  { id: "paper-sea", name: "纸与海", line: "默认 · 暖白的纸、海蓝和品红" },
  { id: "mist-ochre", name: "雾与赭", line: "冷雾灰的纸、赭石和靛蓝" },
];

const KEY = "pith.look";
const EVENT = "pith:look";
const dark = () => window.matchMedia("(prefers-color-scheme: dark)");

export function loadLook(): Look {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Look>;
    return {
      palette: PALETTES.some((p) => p.id === v.palette) ? (v.palette as Palette) : "paper-sea",
      theme: v.theme === "light" || v.theme === "dark" ? v.theme : "system",
    };
  } catch {
    return { palette: "paper-sea", theme: "system" };
  }
}

const themeOf = (look: Look) => (look.theme === "system" ? (dark().matches ? "dark" : "light") : look.theme);

function apply(look: Look, root: HTMLElement = document.documentElement) {
  root.dataset.palette = look.palette;
  root.dataset.theme = themeOf(look);
}

type Vt = { ready: Promise<void>; finished: Promise<void> };

/** 洇开真的开始了（浏览器拍完旧画面、动画起步）：湖面波纹等这一刻才漾开，不然最浓的那段被拍照的那一下吃掉 */
export const INK_READY = "pith:ink-ready";
const inkReady = () => window.dispatchEvent(new Event(INK_READY));
type VtDoc = Document & { startViewTransition?: (cb: () => void) => Vt };

/** 照 look 换根上的配色、明暗；颜色真的变了就从 at（默认窗口正中）洇开 */
function inkTo(look: Look, at?: { x: number; y: number }, then?: () => void) {
  const root = document.documentElement;
  const doc = document as VtDoc;
  const same = root.dataset.palette === look.palette && root.dataset.theme === themeOf(look);
  if (same || !doc.startViewTransition || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    apply(look);
    then?.();
    inkReady();
    return;
  }
  const x = at?.x ?? innerWidth / 2;
  const y = at?.y ?? innerHeight / 2;
  root.style.setProperty("--ink-x", `${x}px`);
  root.style.setProperty("--ink-y", `${y}px`);
  // 洇到最远的那个角
  root.style.setProperty("--ink-r", `${Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)) + 160}px`);
  root.classList.add("is-inking");
  const t = doc.startViewTransition(() => {
    apply(look);
    if (then) flushSync(then);
  });
  t.ready.then(inkReady, inkReady);
  t.finished.finally(() => root.classList.remove("is-inking"));
}

/** 换外观并记下；at 是点的位置（颜色从这里洇开） */
export function saveLook(look: Look, at?: { x: number; y: number }) {
  localStorage.setItem(KEY, JSON.stringify(look));
  inkTo(look, at, () => window.dispatchEvent(new CustomEvent(EVENT, { detail: look })));
}

/** App 启动时调一次：照记下的外观写根，跟随系统时听系统的变化（从窗口正中洇开） */
export function startLook(): () => void {
  apply(loadLook());
  const mq = dark();
  const onSystem = () => inkTo(loadLook());
  mq.addEventListener("change", onSystem);
  return () => mq.removeEventListener("change", onSystem);
}

export function useLook(): [Look, (l: Look, at?: { x: number; y: number }) => void] {
  const [look, setLook] = useState(loadLook);
  useEffect(() => {
    const on = (e: Event) => setLook((e as CustomEvent<Look>).detail);
    window.addEventListener(EVENT, on);
    return () => window.removeEventListener(EVENT, on);
  }, []);
  return [look, saveLook];
}

// 外观页的色板预览要画「另一套」的颜色：vendor/pith/pith.css 的配色块只写在 :root 上（默认那套没有元素级的块），
// 所以从同步来的 pith.css 里读出每套、每种明暗的 --color-*，写在预览自己的 style 上（数值仍然只来自方案）。
const BLOCKS: { palette: Palette; theme: "light" | "dark"; vars: Record<string, string> }[] = [];
for (const m of pithCss.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
  const vars = Object.fromEntries([...m[2].matchAll(/(--color-[\w-]+)\s*:\s*([^;]+);/g)].map((v) => [v[1], v[2].trim()]));
  if (Object.keys(vars).length === 0) continue;
  for (const sel of m[1].split(",").map((s) => s.trim())) {
    const p = /data-palette='([\w-]+)'/.exec(sel)?.[1] ?? "paper-sea";
    BLOCKS.push({ palette: p as Palette, theme: sel.includes("data-theme='dark'") ? "dark" : "light", vars });
  }
}

export function paletteVars(palette: Palette, theme: "light" | "dark"): React.CSSProperties {
  return (BLOCKS.find((b) => b.palette === palette && b.theme === theme)?.vars ?? {}) as React.CSSProperties;
}
