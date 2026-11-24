// 测试用：复用 docx/resources.ts 读取打包资源。
import { DOMParser } from "@xmldom/xmldom";

export { library, locales, styleXml } from "../docx/resources.ts";

export const parseXml = (xml: string) =>
  new DOMParser().parseFromString(xml, "application/xml") as unknown as Document;

export function idGen(prefix = "dup"): () => string {
  let n = 0;
  return () => `${prefix}-${++n}`;
}
