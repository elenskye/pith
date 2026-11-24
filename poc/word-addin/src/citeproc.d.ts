// citeproc（citeproc-js 的 CommonJS 包）没有官方类型，这里只声明 PoC 用到的部分。
declare module "citeproc" {
  export interface CslSys {
    retrieveLocale(lang: string): string | undefined;
    retrieveItem(id: string): unknown;
  }
  export interface CslCitation {
    citationID: string;
    citationItems: { id: string; locator?: string; label?: string }[];
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
    rebuildProcessorState(citations: CslCitation[], mode?: string): [string, number, string][];
    makeBibliography(): [CslBibParams, string[]] | false;
  }
  const CSL: {
    Engine: new (sys: CslSys, style: string, lang?: string, forceLang?: boolean) => CslEngine;
    PROCESSOR_VERSION: string;
  };
  export default CSL;
}
