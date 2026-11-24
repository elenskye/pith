// 概念研究页的中文名和小工具（v0.4，decisions/0024）。
import type { ConceptCardKind, ConceptCategory, ConceptRelationKind, ConceptRole, ConceptRunStatus, ConceptSentence } from "@/shared/ipc";

export const ROLE: Record<ConceptRole, string> = { body: "正文", heading: "标题", sidenote: "旁注", note: "注释", guide: "导读", matter: "前后附" };

export const CATEGORY: Record<ConceptCategory, string> = { original: "原文", others: "他人", explanation: "解释", speculation: "推演" };
export const CATEGORY_HINT: Record<ConceptCategory, string> = {
  original: "原著正文，逐字核对过",
  others: "有出处的别人的话（后世评论、反方，译本的旁注、注释、导读），逐字核对过",
  explanation: "AI 的解释；尽量挂原文，挂不上的标出来",
  speculation: "假设推演，不是原著的观点",
};
/** 每个类别一个颜色（标签的底色） */
export const CATEGORY_TONE: Record<ConceptCategory, string> = {
  original: "bg-blue-600 text-white dark:bg-blue-500",
  others: "bg-amber-600 text-white dark:bg-amber-500",
  explanation: "bg-emerald-600 text-white dark:bg-emerald-500",
  speculation: "bg-violet-600 text-white dark:bg-violet-500",
};

export const CARD_KIND: Record<ConceptCardKind, string> = { concept: "概念卡", argument: "论证卡", comment: "评论卡", counterfactual: "推演卡", note: "笔记" };
export const CARD_KINDS: ConceptCardKind[] = ["concept", "argument", "comment", "counterfactual", "note"];

export const RELATION: Record<ConceptRelationKind, string> = { cause: "因果", contains: "包含", opposes: "对立", example: "举例", premise: "前提", revises: "修正" };

export const RUN_STATUS: Record<ConceptRunStatus, string> = { ready: "待开始", running: "进行中", cancelled: "已停止", failed: "出错", done: "完成" };
export const RUN_TONE: Record<ConceptRunStatus, "plain" | "accent" | "warn" | "danger" | "ok"> = { ready: "plain", running: "accent", cancelled: "warn", failed: "danger", done: "ok" };

export const MODEL_NAME: Record<string, string> = { "gpt-6.1-sol": "GPT-6.1 Sol", "claude-opus-5-5": "Opus 5.5" };
export const EFFORTS = ["low", "medium", "high", "xhigh", "max"];
export const EFFORT: Record<string, string> = { low: "浅", medium: "中（默认）", high: "深", xhigh: "很深", max: "最深" };
export const SEARCH_ENGINE: Record<string, string> = { openai_web_search: "OpenAI 网页搜索", claude_web_search: "Claude 网页搜索", tavily: "Tavily", mock: "模拟" };

/** 锚点的写法：「I.i.3，第 110 页」 */
export function anchorLabel(s: ConceptSentence): string | null {
  const parts: string[] = [];
  if (s.paragraph) parts.push(s.paragraph);
  if (s.check.pages) parts.push(`第 ${s.check.pages} 页`);
  if (s.source) parts.push(s.source);
  return parts.length ? parts.join("，") : null;
}

/** 句子显示的文字：原文、他人加引号 */
export function shown(s: ConceptSentence): string {
  return s.category === "original" || s.category === "others" ? `“${s.quote ?? s.text}”` : s.text;
}

export const TRANSLATION_PREFIX = "译:";
