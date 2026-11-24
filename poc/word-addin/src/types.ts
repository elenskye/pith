// 引文 PoC 的共享类型。文献用 CSL-JSON，文档内数据见 store.ts。

export type CslName = { family?: string; given?: string; literal?: string };
export type CslDate = { "date-parts"?: number[][]; raw?: string; literal?: string };

export type CslItem = {
  id: string;
  type: string;
  title?: string;
  author?: CslName[];
  editor?: CslName[];
  issued?: CslDate;
  "container-title"?: string;
  language?: string;
  [key: string]: unknown;
};

/** 引文里的一条文献：文献 ID + 可选页码。 */
export type CiteItemRef = { id: string; locator?: string; label?: string };

/** 一处引文（一个内容控件）对应的数据。 */
export type CitationData = { items: CiteItemRef[]; created: string };

/** 一段带格式的文字。citeproc 输出的 HTML 会被拆成这种片段再写进 Word。 */
export type Run = {
  text: string;
  italic?: boolean;
  bold?: boolean;
  sup?: boolean;
  sub?: boolean;
  smallCaps?: boolean;
  underline?: boolean;
};
