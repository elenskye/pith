// 外观：配色（data-palette）和明暗（data-theme）写在根上，vendor/pith/pith.css 照它换变量（Tailwind 的 dark: 也看 data-theme）。
// 设置 › 外观、⌘K、对话里都能换；记在这台电脑上（localStorage），跟随系统时听系统的明暗变化。
import { useEffect, useState } from "react";
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

function apply(look: Look, root: HTMLElement = document.documentElement) {
  root.dataset.palette = look.palette;
  root.dataset.theme = look.theme === "system" ? (dark().matches ? "dark" : "light") : look.theme;
}

export function saveLook(look: Look) {
  localStorage.setItem(KEY, JSON.stringify(look));
  apply(look);
  window.dispatchEvent(new CustomEvent(EVENT, { detail: look }));
}

/** App 启动时调一次：照记下的外观写根，跟随系统时听系统的变化 */
export function startLook(): () => void {
  apply(loadLook());
  const mq = dark();
  const onSystem = () => apply(loadLook());
  mq.addEventListener("change", onSystem);
  return () => mq.removeEventListener("change", onSystem);
}

export function useLook(): [Look, (l: Look) => void] {
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
