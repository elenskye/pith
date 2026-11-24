// citeproc（citeproc-js 的 CommonJS 包）没有官方类型，这里只声明 PoC 用到的部分。
declare module "citeproc" {
  export interface CslSys {
    retrieveLocale(lang: string): string | undefined;
    retrieveItem(id: string): unknown;
  }
  export interface CslCitation {
    citationID: string;
    citationItems: { id: string; locator?: string; label?: string; prefix?: string; suffix?: string; "suppress-author"?: boolean; "author-only"?: boolean }[];
    properties: { noteIndex: number };
  }
  export interface CslBibParams {
    maxoffset: number;
    entryspacing: number;
    linespacing: number;
    hangingindent?: boolean;
    "second-field-align": false | "flush" | "margin";
    entry_ids: string[][];
    bibliography_errors: unknown[];
  }
  export interface CslEngine {
    /** 内部登记表：顺序编码样式里 seq 就是引文编号 */
    registry: { registry: Record<string, { seq: number } | undefined> };
    rebuildProcessorState(citations: CslCitation[], mode?: string): [string, number, string][];
    makeBibliography(): [CslBibParams, string[]] | false;
  }
  const CSL: {
    Engine: new (sys: CslSys, style: string, lang?: string, forceLang?: boolean) => CslEngine;
    PROCESSOR_VERSION: string;
  };
  export default CSL;
}
