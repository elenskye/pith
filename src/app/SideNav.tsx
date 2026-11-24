// 主页面左侧的侧栏外框和条目（排版、调查、设置共用；文献库的侧栏另有自己的写法）。
import { Plus } from "lucide-react";

export function SideNav({ title, children, footer }: { title: string; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <nav aria-label={title} className="flex w-60 shrink-0 flex-col border-r bg-sidebar">
      <div className="px-4 pt-4 pb-2 text-[11px] font-medium tracking-wide text-muted-foreground">{title}</div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">{children}</div>
      {footer && <div className="border-t px-2 py-2">{footer}</div>}
    </nav>
  );
}

export function NavLabel({ children }: { children: React.ReactNode }) {
  return <div className="px-2 pt-4 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground">{children}</div>;
}

/** 一行：主文字、可选的第二行、右侧小标签；indent 是子条目 */
export function NavItem({
  label,
  sub,
  icon,
  right,
  active,
  indent,
  onClick,
  onMenu,
  title,
}: {
  label: React.ReactNode;
  sub?: React.ReactNode;
  icon?: React.ReactNode;
  right?: React.ReactNode;
  active: boolean;
  indent?: boolean;
  onClick: () => void;
  onMenu?: (e: React.MouseEvent) => void;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      onContextMenu={onMenu}
      title={title}
      aria-label={typeof label === "string" ? label : undefined}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-start gap-2 rounded-md py-1.5 pr-2 text-left ${indent ? "pl-6" : "pl-2"} ${active ? "bg-accent text-accent-foreground" : "hover:bg-muted"}`}
    >
      {icon && <span className="mt-0.5 shrink-0 text-muted-foreground">{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className={`block ${indent ? "truncate text-xs" : "line-clamp-2 text-[13px]"}`}>{label}</span>
        {sub && <span className="block truncate text-[11px] text-muted-foreground tabular-nums">{sub}</span>}
      </span>
      {right && <span className="mt-0.5 shrink-0">{right}</span>}
    </button>
  );
}

/** 侧栏顶上的「新建」一行 */
export function NavNew({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return <NavItem icon={<Plus size={14} />} label={label} active={active} onClick={onClick} />;
}
