// 技能放进对话以后，概念研究（书的目录）、排版（规范和排过的）左边那一列的外框和条目（app.css 的 .side-*）。
import { Plus } from "lucide-react";

export function SideNav({ title, children, footer }: { title: string; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <nav aria-label={title} className="side-nav">
      <div className="side-nav-title">{title}</div>
      <div>{children}</div>
      {footer && <div className="mt-2 border-t pt-2">{footer}</div>}
    </nav>
  );
}

export function NavLabel({ children }: { children: React.ReactNode }) {
  return <div className="side-nav-title !pt-4">{children}</div>;
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
      className={`side-item ${indent ? "is-sub" : ""} ${active ? "is-on" : ""}`}
    >
      {icon && <span className="mt-0.5 shrink-0 text-muted-foreground">{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className={`block ${indent ? "truncate" : "line-clamp-2"}`}>{label}</span>
        {sub && <span className="sub block truncate tabular-nums">{sub}</span>}
      </span>
      {right && <span className="mt-0.5 shrink-0">{right}</span>}
    </button>
  );
}

/** 侧栏顶上的「新建」一行 */
export function NavNew({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return <NavItem icon={<Plus size={14} />} label={label} active={active} onClick={onClick} />;
}
