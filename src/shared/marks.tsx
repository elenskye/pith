// Pith 的图形：同步来的 vendor/pith/marks/ 原样内联（类名留着，bundle.css 的动效零件照类名动）。
// 只在这里取 SVG，组件里不另画图标；界面里零碎的小图标（关闭、箭头）还用 Lucide。
import type { MaterialKind } from "@/shared/ipc";

const FILES = import.meta.glob("../vendor/pith/marks/**/*.svg", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

/** 去掉 xmlns，加上要的类名 */
function svgOf(path: string, cls?: string): string {
  const raw = FILES[`../vendor/pith/marks/${path}.svg`];
  if (!raw) return "";
  const svg = raw.replace(' xmlns="http://www.w3.org/2000/svg"', "").replace("<svg ", '<svg aria-hidden="true" ');
  return cls ? svg.replace('class="', `class="${cls} `) : svg;
}

/** 一个图形：path 是 marks/ 下去掉 .svg 的路径，如 "icons/kb"、"agents/think"、"stages/seed" */
export function Mark({ path, className, svgClass, style }: { path: string; className?: string; svgClass?: string; style?: React.CSSProperties }) {
  return <span className={`mark ${className ?? ""}`} style={style} dangerouslySetInnerHTML={{ __html: svgOf(path, svgClass) }} />;
}

const STAGES = ["seed", "sprout", "evergreen"];
export const STAGE_NAMES = ["刚种下", "长着", "常青"];

/** 生长阶段：种子 → 四瓣 → 八瓣 */
export function Stage({ stage, className }: { stage: number; className?: string }) {
  return <Mark path={`stages/${STAGES[Math.max(0, Math.min(2, stage))]}`} className={`stg ${className ?? ""}`} />;
}

/** Agent 图标（Pith 在想）：agents/<id>；once 只演一次 */
export function Agent({ id, className, once }: { id: string; className?: string; once?: boolean }) {
  return <Mark path={`agents/${FILES[`../vendor/pith/marks/agents/${id}.svg`] ? id : "think"}`} className={`ag ${className ?? ""}`} svgClass={once ? "once" : undefined} />;
}

export const KIND_NAMES: Record<MaterialKind, string> = {
  full: "全文",
  excerpt: "摘抄",
  note: "笔记",
  web: "网页快照",
  file: "本机文件",
  scan: "扫描件",
  concept: "概念卡",
  report: "调查报告",
  wonder: "WonderLab 作品",
  answer: "存下的回答",
};

/** 十种记录的类型图标 */
export function KindIcon({ kind, className }: { kind: MaterialKind; className?: string }) {
  return <Mark path={`types/${kind}`} className={`tyi ${className ?? ""}`} />;
}

/** 首页的叶子（marks/garden/leaf.svg） */
export function Leaf({ className }: { className?: string }) {
  return <Mark path="garden/leaf" className={className} />;
}

/** 字会长：标题的字重跟着生长阶段 */
export const STAGE_WEIGHT = [300, 430, 640];
