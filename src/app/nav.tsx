// 外框的状态：在哪一屏、哪段对话、设置的哪一页……几屏之间互相跳都经过这里（App.tsx 提供）。
import { createContext, useContext } from "react";
import type { SkillId } from "@/shared/ipc";

export type Screen = "chat" | "kb" | "lib" | "set";

/** 对话屏看的是什么：首页（null）、一段对话、还没有对话的一次调查或研究、一本书 */
export type ChatTarget =
  | null
  | { kind: "chat"; uid: string }
  | { kind: "ref"; skill: SkillId; ref: string; title: string }
  /** 打开一本解析好的书（概念研究）；在书里跑起一次研究时才建对话 */
  | { kind: "book"; text: string; title: string };

/** 设置的八页（R1 的七页 + 1.0.3 从文献库挪来的导入） */
export type SetSection = "roles" | "skills" | "kb" | "keys" | "word" | "import" | "data" | "look";

/** 从别处跳进文献库：看某一条、待确认的补全 */
export type LibRequest = { kind: "item"; id: number } | { kind: "review" };

export type Nav = {
  screen: Screen;
  go: (s: Screen) => void;
  chat: ChatTarget;
  openChat: (t: ChatTarget) => void;
  setSection: SetSection;
  openSettings: (s: SetSection) => void;
  /** 知识库：哪个库、哪份材料（空串：照旧） */
  kb: { uid: string | null; material: string | null };
  openKb: (uid: string | null, material?: string | null) => void;
  /** 文献库：选中一条、看待确认的补全 */
  openLibrary: (r: LibRequest) => void;
  openItem: (id: number) => void;
  /** 对话历史的枝桠 */
  side: boolean;
  setSide: (v: boolean) => void;
  /** 文献库的标签树（两指往右滑叫出来） */
  tags: boolean;
  openPalette: () => void;
  /** 存进知识库：种子从 from 飞到顶栏的知识库图标 */
  seed: (from: Element | null) => void;
  /** 换配色：湖面波纹从 (x, y) 漾开（颜色从同一点洇开在 shared/theme 的 saveLook） */
  lake: (x: number, y: number) => void;
  say: (text: string) => void;
};

export const NavContext = createContext<Nav | null>(null);

export function useNav(): Nav {
  const n = useContext(NavContext);
  if (!n) throw new Error("useNav 要在 App 里用");
  return n;
}

export const SKILLS: { id: SkillId; cmd: string; name: string; line: string }[] = [
  { id: "research", cmd: "/调查", name: "事实调查", line: "拆成能核实的子问题，检索、下载、逐字核对" },
  { id: "concept", cmd: "/概念", name: "概念研究", line: "读一本解析好的书：问这本书、追踪概念、整章分析" },
  { id: "format", cmd: "/排版", name: "排版", line: "照学校的规范排 Word 论文" },
];

export const skillName = (id: SkillId | null | undefined) => SKILLS.find((s) => s.id === id)?.name ?? "对话";

/** 角色头像的底色（R1：Pith 海蓝、伴读金、审稿人绯红、编辑珊瑚，其余轮着用） */
export const roleTone = (i: number) => ["var(--color-accent)", "var(--color-gold)", "var(--color-second)", "var(--color-coral)"][i % 4];

/** 「2 天前」「上周」这样的说法 */
export function when(iso: string): string {
  const d = new Date(iso);
  const days = Math.floor((Date.now() - d.getTime()) / 86400000);
  if (Number.isNaN(days)) return "";
  if (days <= 0) return "今天";
  if (days === 1) return "昨天";
  if (days < 7) return `${days} 天前`;
  if (days < 14) return "上周";
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
}

export function dateOf(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
}
