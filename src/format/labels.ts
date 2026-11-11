// 排版 Agent 界面用的中文名与小工具。对象 / 属性 / 角色的表照 Rust 抄（agent/extract.rs 的 TARGETS、PROPS，agent/planner.rs 的 ROLES），
// 只管显示；规则与校验在 Rust。没列到的显示原名。
import type { FormatCertainty, FormatCheckKind, FormatHandling, FormatMode } from "@/shared/ipc";
import { formatTime } from "@/library/labels";

const TARGETS: Record<string, string> = {
  page: "页面",
  body: "正文",
  heading1: "一级标题",
  heading2: "二级标题",
  heading3: "三级标题",
  heading4: "四级标题",
  caption: "图题、表题",
  table_text: "表内文字",
  figure: "图片所在段落",
  footnote: "脚注",
  bibliography: "参考文献表",
  header_footer: "页眉页脚文字",
  toc: "目录",
  header: "页眉内容",
  page_number: "页码",
  layout: "版式",
  table: "表格",
  footnote_number: "脚注编号",
};

const PROPS: Record<string, string> = {
  paper: "纸张",
  margin_top: "上边距",
  margin_bottom: "下边距",
  margin_left: "左边距",
  margin_right: "右边距",
  gutter: "装订线",
  header_distance: "页眉距边界",
  footer_distance: "页脚距边界",
  font_east_asia: "中文字体",
  font_latin: "西文字体",
  size: "字号",
  bold: "加粗",
  color: "颜色",
  align: "对齐",
  line_spacing: "行距（倍数）",
  line_spacing_exact: "行距（固定值）",
  space_before: "段前",
  space_after: "段后",
  first_line_indent: "首行缩进（字符）",
  page_break_before: "段前分页",
  tight_after_parent: "紧跟上一级标题时段前不空",
  single_left: "单面 · 左",
  single_center: "单面 · 中",
  single_right: "单面 · 右",
  odd_left: "双面奇数页 · 左",
  odd_center: "双面奇数页 · 中",
  odd_right: "双面奇数页 · 右",
  even_left: "双面偶数页 · 左",
  even_center: "双面偶数页 · 中",
  even_right: "双面偶数页 · 右",
  rule: "页眉下的横线",
  front_format: "前置部分的数字",
  body_format: "正文的数字",
  body_on_odd_page: "正文从奇数页开始",
  duplex_from_pages: "多少页起双面印制",
  three_line: "三线表",
  format: "编号形式",
  restart: "重新编号",
};

export const targetLabel = (key: string) => TARGETS[key] ?? key;
export const propLabel = (key: string) => PROPS[key] ?? key;

/** 计划里能用的目标样式（排版器按内置名认） */
export const ROLES: [string, string][] = [
  ["normal", "正文"],
  ["heading 1", "一级标题"],
  ["heading 2", "二级标题"],
  ["heading 3", "三级标题"],
  ["heading 4", "四级标题"],
  ["caption", "图题、表题"],
  ["bibliography", "参考文献"],
];

const YES_NO: [string, string][] = [
  ["true", "是"],
  ["false", "否"],
];
const PAGE_FORMATS: [string, string][] = [
  ["upper_roman", "大写罗马数字（Ⅰ、Ⅱ）"],
  ["lower_roman", "小写罗马数字（ⅰ、ⅱ）"],
  ["decimal", "阿拉伯数字（1、2）"],
];

/** 值只有固定几种的属性：下拉里显示中文。先按「对象.属性」找，再按属性找 */
const VALUE_CHOICES: Record<string, [string, string][]> = {
  bold: YES_NO,
  page_break_before: YES_NO,
  tight_after_parent: YES_NO,
  rule: YES_NO,
  body_on_odd_page: YES_NO,
  three_line: YES_NO,
  align: [
    ["left", "居左"],
    ["center", "居中"],
    ["right", "居右"],
    ["both", "两端对齐"],
  ],
  "page_number.align": [
    ["left", "居左"],
    ["center", "居中"],
    ["right", "居右"],
  ],
  front_format: PAGE_FORMATS,
  body_format: PAGE_FORMATS,
  format: [
    ["circled", "带圈数字（①②③）"],
    ["decimal", "阿拉伯数字"],
  ],
  restart: [
    ["each_page", "每页重新编号"],
    ["each_section", "每节重新编号"],
    ["continuous", "连续编号"],
  ],
};

export const valueChoices = (target: string, prop: string): [string, string][] | undefined => VALUE_CHOICES[`${target}.${prop}`] ?? VALUE_CHOICES[prop];

export const CERTAINTY: Record<FormatCertainty, string> = { explicit: "明文", template: "只在模板", inferred: "推断", unspecified: "未规定" };

export const CERTAINTY_HINT: Record<FormatCertainty, string> = {
  explicit: "规范正文写明了",
  template: "只在模板里出现，规范正文没写",
  inferred: "文档没写，为了让别的要求能执行而补上的",
  unspecified: "文档没有规定，保留原文档的样子",
};

export const HANDLING: Record<FormatHandling, string> = { auto: "自动", structure: "待做", check: "只检查", keep: "不动" };

export const HANDLING_HINT: Record<FormatHandling, string> = {
  auto: "排版程序直接设置",
  structure: "要改内容或结构才能满足，程序还不会做",
  check: "只检查、不改",
  keep: "排版程序不碰",
};

export const HANDLING_ORDER: FormatHandling[] = ["auto", "structure", "check", "keep"];

export const MODE: Record<FormatMode, string> = { brief: "结构和片段", full: "全文" };

export const MODE_HINT: Record<FormatMode, string> = {
  brief: "只发样式、标题、每段开头 30 字和表格概况，不发整篇正文",
  full: "每段全文都发（含表格里的文字）",
};

export const CHECK_KIND: Record<FormatCheckKind, string> = {
  abstract_zh_chars: "中文摘要字数",
  keywords_count: "关键词个数",
  references_count: "参考文献条数",
  caption_by_chapter: "图表按章编号",
};

export const PURPOSE: Record<string, string> = { extract: "提炼要求", plan: "文档计划" };

export const RUN_STATE: Record<string, string> = { none: "还没排", delivered: "已写出", blocked: "校验没过" };

/** 美元金额：两位小数；不到一分钱的写明，不显示成 $0.00 */
export function usd(n: number): string {
  if (n > 0 && n < 0.005) return "不到 $0.01";
  return `$${n.toFixed(2)}`;
}

export const count = (n: number) => n.toLocaleString("zh-CN");

/** claude-opus-5-5 → Claude Opus 5.5 */
export function modelName(id: string): string {
  const words: string[] = [];
  const nums: string[] = [];
  for (const part of id.split("-")) {
    if (/^\d+$/.test(part)) nums.push(part);
    else words.push(part.charAt(0).toUpperCase() + part.slice(1));
  }
  return [...words, nums.join(".")].filter(Boolean).join(" ");
}

/** 后端给的时间（UTC，2026-11-10T08:20:31.000Z）→ 本地的 2026-11-10 16:20 */
export const when = (iso: string) => formatTime(iso);

export const baseName = (path: string) => path.split("/").pop() ?? path;
