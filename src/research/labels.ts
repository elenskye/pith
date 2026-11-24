// 事实调查界面用的中文名与小工具。规则与核对都在 Rust（crates/research-engine、src-tauri/src/research/），这里只管显示。
import type { ResearchProducerKind, ResearchRunStatus, ResearchStage, ResearchTier, ResearchUncertainty } from "@/shared/ipc";

export const STAGES: ResearchStage[] = ["plan", "search", "download", "extract", "verify", "conclude", "report"];

export const STAGE: Record<ResearchStage, string> = {
  plan: "计划",
  search: "检索",
  download: "下载",
  extract: "摘证据",
  verify: "核对",
  conclude: "结论",
  report: "报告",
};

export const STATUS: Record<ResearchRunStatus, string> = {
  awaiting_plan: "等你确认计划",
  running: "进行中",
  cancelled: "已停止",
  failed: "没有做完",
  done: "做完了",
};

export const STATUS_TONE: Record<ResearchRunStatus, "plain" | "accent" | "warn" | "danger" | "ok"> = {
  awaiting_plan: "accent",
  running: "accent",
  cancelled: "warn",
  failed: "danger",
  done: "ok",
};

export const TIER: Record<ResearchTier, string> = { high: "高", medium: "中", low: "低" };
export const TIER_TONE: Record<ResearchTier, "ok" | "plain" | "warn"> = { high: "ok", medium: "plain", low: "warn" };
export const TIERS: ResearchTier[] = ["high", "medium", "low"];

export const UNCERTAINTY: Record<ResearchUncertainty, string> = {
  not_found: "找不到",
  conflicting: "互相矛盾",
  outdated: "已过时",
  unclear_definition: "口径没说清",
  low_trust_only: "只有低可信来源",
};

export const UNCERTAINTY_HINT: Record<ResearchUncertainty, string> = {
  not_found: "没有找到能回答这一点的数据",
  conflicting: "不同来源的数字对不上",
  outdated: "数据不是最新的",
  unclear_definition: "来源没有说清统计口径，或者口径不同",
  low_trust_only: "依据的事实全部来自可信度低的数据生产者（按等级表）",
};

export const KIND: Record<ResearchProducerKind, string> = {
  intl_org: "国际组织",
  foreign_gov_stats: "其他国家和地区的政府统计",
  academic: "学术研究",
  independent_survey: "独立调查机构、智库",
  platform_data: "招聘平台、企业的业务数据",
  industry_report: "企业、行业协会的报告",
  commercial_media: "市场化媒体自己的调查",
  cn_gov_stats: "中国政府官方统计",
  cn_state_media: "中国官方媒体",
  self_media: "自媒体、个人、论坛",
  unknown: "看不出是谁统计的",
};

export const ENGINE: Record<string, string> = {
  tavily: "Tavily",
  claude_web_search: "Claude 网页搜索",
  mock: "模拟搜索",
};

export const PROVIDER: Record<string, string> = {
  anthropic: "Anthropic",
  openai_compatible: "OpenAI 兼容（DeepSeek 等）",
  tavily: "Tavily",
  mock: "模拟",
};

export const PURPOSE: Record<string, string> = {
  research_plan: "拆问题",
  research_search: "检索",
  research_extract: "摘证据",
  research_conclude: "写结论",
  research_fetch: "网页抓取（下载失败的后备）",
};

export const SOURCE_STATE: Record<string, string> = { ok: "已读", failed: "下载失败", no_text: "取不出文字", pending: "还没下载" };

/** 美元：不到一分钱的写到 4 位，免得显示成 $0.00 */
export function money(n: number): string {
  if (n > 0 && n < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(2)}`;
}

export const hostOf = (url: string) => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

export const isWeb = (url: string) => /^https?:\/\//i.test(url);
