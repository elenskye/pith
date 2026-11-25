// 明暗：vendor/pith/pith.css 按根上的 data-theme 换（Tailwind 的 dark: 也看它，index.css）。
// 现在跟着系统，系统换了跟着换；以后设置 › 外观能选亮、暗、配色（data-palette），改这里。

export function followSystemTheme(root: HTMLElement = document.documentElement): () => void {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    root.dataset.theme = mq.matches ? "dark" : "light";
  };
  apply();
  mq.addEventListener("change", apply);
  return () => mq.removeEventListener("change", apply);
}
