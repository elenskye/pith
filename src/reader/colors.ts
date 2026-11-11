// 标注颜色与种类的显示名。颜色与 Zotero 相同，导入的标注和这里新建的混在一起不突兀。
export const COLORS: { value: string; label: string }[] = [
  { value: "#ffd400", label: "黄" },
  { value: "#ff6666", label: "红" },
  { value: "#5fb236", label: "绿" },
  { value: "#2ea8e5", label: "蓝" },
  { value: "#a28ae5", label: "紫" },
  { value: "#e56eee", label: "品红" },
  { value: "#f19837", label: "橙" },
  { value: "#aaaaaa", label: "灰" },
];

export const DEFAULT_COLOR = COLORS[0].value;

export function colorLabel(c: string | null): string {
  return COLORS.find((x) => x.value === c?.toLowerCase())?.label ?? c ?? "";
}

const KINDS: Record<string, string> = { highlight: "高亮", underline: "下划线", note: "便签", image: "区域", ink: "手绘", text: "文本框" };
export const kindLabel = (k: string) => KINDS[k] ?? k;

/** 复制摘录：原文 + 页码。有中文用中文引号与「第 N 页」，否则 "…" (p. N)。 */
export function excerptForCopy(text: string, pageLabel: string | null, pageIndex: number): string {
  const page = pageLabel || String(pageIndex + 1);
  return /[㐀-鿿]/.test(text) ? `“${text}”（第 ${page} 页）` : `"${text}" (p. ${page})`;
}
