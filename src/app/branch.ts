// 枝桠的画法（R1 garden.mjs 的 branch）：一根从左边长进来的主枝，每个节点一根小枝，项目末端一颗节点。
// 对话历史（叫出来才长）、设置的导航（常驻，每次进设置长一遍）共用。

const r1 = (v: number) => Math.round(v * 10) / 10;
const bx = (y: number) => 56 + 2.6 * Math.sin(y / 64);

export type BranchNode = { y: number; kind: "g" | "i" | "new"; on?: boolean };
export type BranchPath = { d: string; cls: string; delay: number; dur?: number };
export type BranchDot = { cx: number; cy: number; on: boolean; delay: number };

/** 主枝从 y0 长到 y1；lead 是起头的延时，step 是一个节点接一个的间隔（秒） */
export function branch(y0: number, y1: number, nodes: BranchNode[], { lead = 0.05, step = 0.055 } = {}) {
  const top = y0 + 74;
  let main = `M-6 ${y0}C30 ${y0 + 4} ${r1(bx(top) - 2)} ${y0 + 30} ${r1(bx(top))} ${top}`;
  for (let y = top + 12; y <= y1; y += 12) main += `L${r1(bx(y))} ${y}`;
  const paths: BranchPath[] = [{ d: main, cls: "main", delay: 0, dur: 0.7 }];
  const dots: BranchDot[] = [];
  nodes.forEach((n, k) => {
    const d = lead + 0.18 + k * step;
    if (n.kind === "g") {
      const ya = n.y + 6;
      paths.push({ d: `M${r1(bx(ya))} ${ya}C${r1(bx(ya) + 8)} ${ya} 70 ${n.y} 80 ${n.y}`, cls: "tw g", delay: d });
      return;
    }
    const ya = n.y + 20;
    paths.push({ d: `M${r1(bx(ya))} ${ya}C${r1(bx(ya) + 16)} ${ya} 74 ${n.y + 1} 90 ${n.y}`, cls: `tw${n.on ? " on" : ""}`, delay: d });
    dots.push({ cx: 90, cy: n.y, on: !!n.on, delay: d + 0.3 });
  });
  return { paths, dots };
}

/** 标签出来的延时：比小枝晚一点 */
export const labelDelay = (k: number, lead = 0.05, step = 0.055) => lead + 0.3 + k * step;
