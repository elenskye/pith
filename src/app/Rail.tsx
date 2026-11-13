// 最左边一列：四个主页面（文献库、排版、调查、概念）和设置。每个主页面有自己的侧栏（decisions/0023、0024）。
import { Library, Lightbulb, PencilRuler, Settings, Telescope } from "lucide-react";

export type Module = "library" | "format" | "research" | "concept" | "settings";

const MAIN: { m: Module; label: string; Icon: typeof Library }[] = [
  { m: "library", label: "文献库", Icon: Library },
  { m: "format", label: "排版", Icon: PencilRuler },
  { m: "research", label: "调查", Icon: Telescope },
  { m: "concept", label: "概念", Icon: Lightbulb },
];

/** onModule：点的是现在这一页时，各页自己回到它的起点（列表 / 新建） */
export function Rail({ module, onModule }: { module: Module; onModule: (m: Module) => void }) {
  return (
    <nav aria-label="主页面" className="flex w-16 shrink-0 flex-col items-center gap-1 border-r bg-sidebar py-3">
      {MAIN.map(({ m, label, Icon }) => (
        <RailButton key={m} label={label} active={module === m} onClick={() => onModule(m)}>
          <Icon size={18} />
        </RailButton>
      ))}
      <div className="flex-1" />
      <RailButton label="设置" active={module === "settings"} onClick={() => onModule("settings")}>
        <Settings size={18} />
      </RailButton>
    </nav>
  );
}

function RailButton({ label, active, onClick, children }: { label: string; active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      title={label}
      aria-current={active ? "page" : undefined}
      className={`flex w-12 flex-col items-center gap-0.5 rounded-md py-1.5 text-[10px] ${active ? "bg-accent font-medium text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground"}`}
    >
      {children}
      <span>{label}</span>
    </button>
  );
}
