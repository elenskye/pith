// 文献库看的是哪一部分：全部、未分类、几种特殊的范围，或者一个集合
export type Scope =
  | { kind: "all" }
  | { kind: "unfiled" }
  | { kind: "missing" }
  | { kind: "duplicates" }
  | { kind: "review" }
  | { kind: "trash" }
  | { kind: "collection"; id: number; name: string };

export const scopeKey = (s: Scope) => (s.kind === "collection" ? `c${s.id}` : s.kind);
