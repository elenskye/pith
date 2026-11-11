// 文档内引文数据：一个 Custom XML Part，命名空间固定，内含一段 JSON。
// Word 内（Office.js / VBA）与 Word 外（docx 直写）读写同一份格式。

import type { CitationData, CslItem } from "./types.ts";

export const STORE_NS = "urn:research-lab:citations:v1";
export const STORE_VERSION = 1;

export type DocStore = {
  version: number;
  style: string;
  /** 文献快照：换电脑或文献库里没有这条时，仍能用文档自带数据刷新。 */
  items: Record<string, CslItem>;
  /** 引文 ID → 引文内容。文档里删掉的引文不立即清理（Word 撤销后还能恢复）。 */
  citations: Record<string, CitationData>;
  updated: string;
};

export class StoreError extends Error {}

export type XmlParser = (xml: string) => Document;

export function emptyStore(style: string, now: string): DocStore {
  return { version: STORE_VERSION, style, items: {}, citations: {}, updated: now };
}

function escapeXml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function serializeStore(store: DocStore): string {
  return (
    `<?xml version="1.0" encoding="UTF-8"?>` +
    `<rl:citations xmlns:rl="${STORE_NS}" version="${store.version}">` +
    `<rl:json>${escapeXml(JSON.stringify(store))}</rl:json>` +
    `</rl:citations>`
  );
}

/** 解析文档里的数据。Word 可能改写前缀、空白和转义，所以按命名空间取元素。 */
export function parseStore(xml: string, parse: XmlParser): DocStore {
  const doc = parse(xml);
  const root = doc.documentElement;
  if (!root || root.namespaceURI !== STORE_NS || root.localName !== "citations") {
    throw new StoreError("不是 Research Lab 引文数据");
  }
  const version = Number(root.getAttribute("version") ?? "0");
  if (!Number.isInteger(version) || version < 1) throw new StoreError("引文数据缺少版本号");
  if (version > STORE_VERSION) {
    throw new StoreError(`文档由更新版本的 Research Lab 写入（数据版本 ${version}），本版本只读不改`);
  }
  const node = root.getElementsByTagNameNS(STORE_NS, "json")[0];
  if (!node) throw new StoreError("引文数据缺少 json 节点");
  let data: unknown;
  try {
    data = JSON.parse(node.textContent ?? "");
  } catch {
    throw new StoreError("引文数据 JSON 无法解析");
  }
  const s = data as Partial<DocStore>;
  if (typeof s !== "object" || !s || typeof s.style !== "string" || typeof s.items !== "object" || typeof s.citations !== "object") {
    throw new StoreError("引文数据结构不完整");
  }
  return {
    version,
    style: s.style,
    items: s.items ?? {},
    citations: s.citations ?? {},
    updated: typeof s.updated === "string" ? s.updated : "",
  };
}
