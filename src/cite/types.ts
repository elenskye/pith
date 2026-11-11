// 引文排版的类型：与 Rust（crates/docx-engine/src/cite）的 serde 结构一一对应（camelCase）。

export type CslName = { family?: string; given?: string; literal?: string };
export type CslDate = { "date-parts"?: number[][]; raw?: string; literal?: string };

export type CslItem = {
  id: string;
  type: string;
  title?: string;
  author?: CslName[];
  issued?: CslDate;
  language?: string;
  [key: string]: unknown;
};

/** 引文里的一条文献：uid + 可选页码 */
export type CiteItemRef = { id: string; locator?: string; label?: string; [key: string]: unknown };

/** 一段带格式的文字（Rust 的 `cite::Run`） */
export type Run = {
  text: string;
  italic?: boolean;
  bold?: boolean;
  sup?: boolean;
  sub?: boolean;
  smallCaps?: boolean;
  underline?: boolean;
};

/** Rust 发来的排版请求（`plan::RenderRequest`）：整篇文档的引文按出现顺序 */
export type RenderRequest = {
  style: string;
  items: Record<string, CslItem>;
  citations: { id: string; items: CiteItemRef[] }[];
};

/** 交回 Rust 的结果（`plan::Rendered`） */
export type Rendered = {
  citations: Record<string, Run[]>;
  bibliography: {
    entries: Run[][];
    hangingIndent: boolean;
    secondFieldAlign: false | "flush" | "margin";
    maxOffset: number;
  } | null;
};
