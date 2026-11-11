// 排版页几处共用的小部件：页面外框、标签、提示框、改完再提交的输入框、很小的 Markdown 渲染。
import { useEffect, useState } from "react";
import { ChevronRight, TriangleAlert } from "lucide-react";

/** 占中右两栏的一页：顶栏是面包屑（最后一项是当前页）和右侧按钮 */
export function Page({ crumbs, actions, children }: { crumbs: { label: string; onClick?: () => void }[]; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="min-w-0 flex-1 overflow-y-auto">
      <header className="sticky top-0 z-10 flex h-12 items-center gap-3 border-b bg-background px-6">
        <h1 className="flex min-w-0 flex-1 items-center gap-1 text-sm">
          {crumbs.map((c, i) => {
            const last = i === crumbs.length - 1;
            return (
              <span key={i} className={`flex items-center gap-1 ${last ? "min-w-0" : "shrink-0"}`}>
                {i > 0 && <ChevronRight size={13} className="shrink-0 text-muted-foreground" />}
                {c.onClick && !last ? (
                  <button onClick={c.onClick} className="max-w-40 truncate rounded px-1 text-muted-foreground hover:bg-muted hover:text-foreground">
                    {c.label}
                  </button>
                ) : (
                  <span className="truncate px-1 font-semibold" title={c.label}>
                    {c.label}
                  </span>
                )}
              </span>
            );
          })}
        </h1>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </header>
      <div className="max-w-3xl space-y-6 px-6 py-6">{children}</div>
    </section>
  );
}

type Tone = "plain" | "accent" | "warn" | "danger" | "ok";

const TONES: Record<Tone, string> = {
  plain: "bg-muted text-muted-foreground",
  accent: "bg-accent text-accent-foreground",
  warn: "bg-amber-100 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300",
  danger: "bg-destructive/10 text-destructive",
  ok: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300",
};

export function Badge({ tone = "plain", title, children }: { tone?: Tone; title?: string; children: React.ReactNode }) {
  return (
    <span title={title} className={`inline-block shrink-0 rounded px-1.5 py-px text-[11px] leading-relaxed whitespace-nowrap ${TONES[tone]}`}>
      {children}
    </span>
  );
}

/** 醒目的提示框：一句标题，下面可以列几条 */
export function Notice({ tone = "warn", title, items, children }: { tone?: "warn" | "danger" | "info"; title?: React.ReactNode; items?: string[]; children?: React.ReactNode }) {
  const box =
    tone === "danger" ? "border-destructive/40 bg-destructive/10" : tone === "warn" ? "border-amber-300/70 bg-amber-50 dark:border-amber-800/60 dark:bg-amber-950/30" : "bg-muted/60";
  return (
    <div className={`selectable rounded-md border px-3 py-2 text-xs ${box}`}>
      {title && (
        <p className={`flex items-start gap-1.5 font-medium ${tone === "danger" ? "text-destructive" : ""}`}>
          {tone !== "info" && <TriangleAlert size={13} className="mt-0.5 shrink-0" />}
          <span className="min-w-0">{title}</span>
        </p>
      )}
      {items && items.length > 0 && (
        <ul className={`list-disc space-y-0.5 pl-5 text-muted-foreground ${title ? "mt-1" : ""}`}>
          {items.map((x, i) => (
            <li key={i} className="break-words">
              {x}
            </li>
          ))}
        </ul>
      )}
      {children && <div className={`text-muted-foreground ${title ? "mt-1" : ""}`}>{children}</div>}
    </div>
  );
}

export function SectionTitle({ children, right }: { children: React.ReactNode; right?: React.ReactNode }) {
  return (
    <div className="mb-2 flex items-center justify-between gap-3">
      <h2 className="text-xs font-medium text-muted-foreground">{children}</h2>
      {right}
    </div>
  );
}

/** 改完（回车或离开）才提交的输入框；值没变不提交。外面的值变了跟着变 */
export function CommitInput({
  value,
  onCommit,
  disabled,
  className = "",
  placeholder,
  inputMode,
}: {
  value: string;
  onCommit: (v: string) => void;
  disabled?: boolean;
  className?: string;
  placeholder?: string;
  inputMode?: "numeric" | "text";
}) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const commit = () => {
    if (v.trim() !== value.trim()) onCommit(v.trim());
    else setV(value);
  };
  return (
    <input
      value={v}
      disabled={disabled}
      placeholder={placeholder}
      inputMode={inputMode}
      onChange={(e) => setV(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.nativeEvent.isComposing) return;
        if (e.key === "Enter") e.currentTarget.blur();
        else if (e.key === "Escape") {
          e.stopPropagation();
          setV(value);
        }
      }}
      className={`selectable rounded border bg-background px-1.5 py-0.5 outline-none focus:border-ring disabled:opacity-60 ${className}`}
    />
  );
}

export function Switch({ on, onChange, disabled, title }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; title?: string }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      title={title}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative h-4 w-7 shrink-0 rounded-full transition-colors disabled:opacity-50 ${on ? "bg-primary" : "bg-border"}`}
    >
      <span className={`absolute top-0.5 left-0.5 h-3 w-3 rounded-full bg-background shadow transition-transform ${on ? "translate-x-3" : ""}`} />
    </button>
  );
}

// ---------------- Markdown（只认排版报告用到的：标题、列表、表格、粗体、引用、分隔线） ----------------

/** 一行里的 **粗体** 与 `代码` */
function inline(text: string): React.ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**") && part.length > 4) return <strong key={i}>{part.slice(2, -2)}</strong>;
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2)
      return (
        <code key={i} className="rounded bg-muted px-1 font-mono text-[0.92em]">
          {part.slice(1, -1)}
        </code>
      );
    return part;
  });
}

/** 表格的一行 → 单元格（`\|` 是单元格里的竖线） */
function cells(line: string): string[] {
  const out: string[] = [];
  let cur = "";
  const t = line.trim();
  for (let i = 0; i < t.length; i++) {
    if (t[i] === "\\" && t[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (t[i] === "|") {
      out.push(cur.trim());
      cur = "";
    } else cur += t[i];
  }
  out.push(cur.trim());
  // 行首、行尾的竖线不算单元格
  if (t.startsWith("|")) out.shift();
  if (t.endsWith("|") && !t.endsWith("\\|")) out.pop();
  return out;
}

const isRow = (l: string) => l.trim().startsWith("|");
const isDivider = (l: string) => /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/.test(l);

export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const out: React.ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const key = out.length;
    if (!line.trim()) {
      i++;
      continue;
    }
    const h = /^(#{1,4})\s+(.*)$/.exec(line);
    if (h) {
      const level = h[1].length;
      const cls = level === 1 ? "text-sm font-semibold" : level === 2 ? "mt-4 border-b pb-1 text-[13px] font-semibold" : "mt-3 font-medium";
      out.push(
        <p key={key} role="heading" aria-level={level} className={cls}>
          {inline(h[2])}
        </p>,
      );
      i++;
      continue;
    }
    if (/^\s*-{3,}\s*$/.test(line)) {
      out.push(<hr key={key} className="my-3" />);
      i++;
      continue;
    }
    if (isRow(line) && i + 1 < lines.length && isDivider(lines[i + 1])) {
      const head = cells(line);
      const body: string[][] = [];
      i += 2;
      while (i < lines.length && isRow(lines[i])) body.push(cells(lines[i++]));
      out.push(
        <div key={key} className="overflow-x-auto">
          <table className="w-full text-left">
            <thead className="text-muted-foreground">
              <tr className="border-b">
                {head.map((c, j) => (
                  <th key={j} className="py-1 pr-3 font-normal whitespace-nowrap">
                    {inline(c)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {body.map((row, r) => (
                <tr key={r} className="border-b border-border/60 align-top">
                  {head.map((_, j) => (
                    <td key={j} className="py-1 pr-3 break-words">
                      {inline(row[j] ?? "")}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*[-*]\s+/, ""));
      out.push(
        <ul key={key} className="list-disc space-y-0.5 pl-5">
          {items.map((x, j) => (
            <li key={j} className="break-words">
              {inline(x)}
            </li>
          ))}
        </ul>,
      );
      continue;
    }
    if (line.startsWith(">")) {
      const quote: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) quote.push(lines[i++].replace(/^>\s?/, ""));
      out.push(
        <blockquote key={key} className="border-l-2 border-amber-400 bg-amber-50 px-3 py-1.5 dark:bg-amber-950/30">
          {inline(quote.join(" "))}
        </blockquote>,
      );
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(#{1,4}\s|\s*[-*]\s+|>|\s*-{3,}\s*$)/.test(lines[i]) && !isRow(lines[i])) para.push(lines[i++]);
    // 以 | 开头却不成表格的一行：照普通文字显示，别卡住
    if (para.length === 0) para.push(lines[i++]);
    out.push(
      <p key={key} className="break-words">
        {inline(para.join(" "))}
      </p>,
    );
  }
  return <div className="selectable space-y-2 text-xs leading-relaxed">{out}</div>;
}
