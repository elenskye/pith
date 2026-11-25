// Pith 的控件（方案 pith 的 Select、Menu；样式在 vendor/pith/bundle.css 的 pith-select、pith-menu）：
// 系统的 <select> 在 macOS 上弹出的是原生菜单，改不了样子，所以自己画；勾选框、单选框只靠样式（pith-check、pith-radio）。
import { Children, isValidElement, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** 出场、退场：open 变 false 后再留 ms 毫秒（加 .is-out 演退场），然后卸掉 */
export function usePresence(open: boolean, ms = 160) {
  const [mounted, setMounted] = useState(open);
  const [leaving, setLeaving] = useState(false);
  useEffect(() => {
    if (open) {
      setMounted(true);
      setLeaving(false);
      return;
    }
    if (!mounted) return;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setMounted(false);
      return;
    }
    setLeaving(true);
    const t = setTimeout(() => {
      setMounted(false);
      setLeaving(false);
    }, ms);
    return () => clearTimeout(t);
  }, [open, ms]); // eslint-disable-line react-hooks/exhaustive-deps
  return { mounted, leaving };
}

export type Option = { value: string; label: React.ReactNode; hint?: React.ReactNode; disabled?: boolean };

/** 从 <option> 子元素读出选项（这样换掉 <select> 时选项照原样写） */
function optionsOf(children: React.ReactNode): Option[] {
  const out: Option[] = [];
  Children.forEach(children, (c) => {
    if (!isValidElement(c)) return;
    const p = c.props as { value?: string | number; children?: React.ReactNode; disabled?: boolean };
    if (c.type === "option") out.push({ value: String(p.value ?? ""), label: p.children, disabled: p.disabled });
    else out.push(...optionsOf(p.children));
  });
  return out;
}

/**
 * 下拉框：按钮 + 弹出的列表（放在 body 上，面板、表格里不会被裁掉）。
 * ↑↓ 移动、Enter / 空格选、Esc 收起、打字跳到开头对得上的那项。
 * variant：quiet 是工具栏里的一行字（默认），field 是表单里有框的。
 */
export function Select({
  value,
  onChange,
  options,
  children,
  placeholder,
  disabled,
  title,
  variant = "quiet",
  className = "",
  menuClassName = "",
  render,
}: {
  value: string;
  onChange: (v: string) => void;
  options?: Option[];
  children?: React.ReactNode;
  placeholder?: React.ReactNode;
  disabled?: boolean;
  title?: string;
  variant?: "quiet" | "field";
  className?: string;
  menuClassName?: string;
  /** 按钮上怎么写选中的那项（默认写它的 label） */
  render?: (o: Option | undefined) => React.ReactNode;
}) {
  const opts = options ?? optionsOf(children);
  const cur = opts.find((o) => o.value === value);
  const [open, setOpen] = useState(false);
  const btn = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={btn}
        type="button"
        disabled={disabled}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`pith-select ${variant === "field" ? "pith-select--field" : ""} ${open ? "is-open" : ""} ${className}`}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span className="pith-select-v">{render ? render(cur) : (cur?.label ?? <span className="pith-select-ph">{placeholder ?? "选择…"}</span>)}</span>
      </button>
      <Menu
        open={open}
        anchor={btn}
        items={opts.map((o) => ({ key: o.value, label: o.label, hint: o.hint, on: o.value === value, disabled: o.disabled, onPick: () => onChange(o.value) }))}
        onClose={() => {
          setOpen(false);
          btn.current?.focus({ preventScroll: true });
        }}
        role="listbox"
        className={menuClassName}
      />
    </>
  );
}

export type MenuEntry =
  | { key: string; label: React.ReactNode; hint?: React.ReactNode; on?: boolean; danger?: boolean; disabled?: boolean; onPick: () => void }
  | { key: string; sep: true }
  | { key: string; group: React.ReactNode };

/**
 * 弹出的列表（下拉框、右键菜单、「新建」都用它）：贴着 anchor 的下沿（放不下就翻到上面），或者在 at 那个点。
 * 出来是从上往下展开一点，收起时淡出（pith-menu 的 .is-out）。
 */
export function Menu({
  open,
  anchor,
  at,
  items,
  onClose,
  role = "menu",
  className = "",
}: {
  open: boolean;
  anchor?: React.RefObject<HTMLElement | null>;
  at?: { x: number; y: number };
  items: MenuEntry[];
  onClose: () => void;
  role?: "menu" | "listbox";
  className?: string;
}) {
  const { mounted, leaving } = usePresence(open, 140);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; minWidth?: number; up: boolean } | null>(null);
  const picks = items.map((it, i) => ("onPick" in it && !it.disabled ? i : -1)).filter((i) => i >= 0);
  const [hi, setHi] = useState(-1);
  const typed = useRef({ s: "", t: 0 });

  // 位置：每次打开量一次（窗口、面板会变）
  useLayoutEffect(() => {
    if (!open) return;
    const m = ref.current?.getBoundingClientRect();
    const h = m?.height ?? 0;
    const w = m?.width ?? 0;
    if (anchor?.current) {
      const r = anchor.current.getBoundingClientRect();
      const up = r.bottom + 6 + h > innerHeight - 8 && r.top - 6 - h > 8;
      setPos({ left: Math.max(8, Math.min(r.left, innerWidth - Math.max(w, r.width) - 8)), top: up ? r.top - 6 - h : r.bottom + 6, minWidth: r.width, up });
    } else if (at) {
      setPos({ left: Math.max(8, Math.min(at.x, innerWidth - w - 8)), top: Math.max(8, Math.min(at.y, innerHeight - h - 8)), up: false });
    }
    const on = items.findIndex((it) => "onPick" in it && it.on);
    setHi(on >= 0 ? on : -1);
  }, [open, mounted]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!open) return;
    ref.current?.querySelector(".is-on")?.scrollIntoView({ block: "nearest" });
    ref.current?.focus({ preventScroll: true });
  }, [open, pos]);

  const pick = useCallback(
    (i: number) => {
      const it = items[i];
      if (!it || !("onPick" in it) || it.disabled) return;
      onClose();
      it.onPick();
    },
    [items, onClose],
  );

  // 点外面、窗口失焦、滚动别处时收起
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const t = e.target instanceof Node ? e.target : null;
      if (t && (ref.current?.contains(t) || anchor?.current?.contains(t))) return;
      onClose();
    };
    const blur = () => onClose();
    window.addEventListener("mousedown", down, true);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("blur", blur);
    };
  }, [open, onClose, anchor]);

  useEffect(() => {
    if (open) ref.current?.querySelector(`[data-i="${hi}"]`)?.scrollIntoView({ block: "nearest" });
  }, [hi, open]);

  const onKey = (e: React.KeyboardEvent) => {
    const k = picks.indexOf(hi);
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setHi(picks[Math.min(picks.length - 1, k + 1)] ?? -1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHi(picks[Math.max(0, k - 1)] ?? -1);
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      setHi(e.key === "Home" ? picks[0] : picks[picks.length - 1]);
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(hi);
    } else if (e.key === "Tab") {
      onClose();
    } else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey) {
      const now = Date.now();
      typed.current = { s: (now - typed.current.t < 700 ? typed.current.s : "") + e.key.toLowerCase(), t: now };
      const hit = picks.find((i) => {
        const it = items[i];
        return "onPick" in it && textOf(it.label).toLowerCase().startsWith(typed.current.s);
      });
      if (hit !== undefined) setHi(hit);
    }
  };

  if (!mounted) return null;
  return createPortal(
    <div
      ref={ref}
      role={role}
      tabIndex={-1}
      onKeyDown={onKey}
      onMouseDown={(e) => e.stopPropagation()}
      className={`pith-menu ${pos?.up ? "is-up" : ""} ${leaving ? "is-out" : ""} ${className}`}
      style={pos ? { left: pos.left, top: pos.top, minWidth: pos.minWidth } : { left: -9999, top: 0 }}
    >
      {items.map((it, i) => {
        if ("sep" in it) return <hr key={it.key} className="pith-menu-sep" />;
        if ("group" in it)
          return (
            <p key={it.key} className="pith-menu-group">
              {it.group}
            </p>
          );
        return (
          <button
            key={it.key}
            type="button"
            data-i={i}
            role={role === "listbox" ? "option" : "menuitem"}
            aria-selected={role === "listbox" ? !!it.on : undefined}
            disabled={it.disabled}
            className={`pith-menu-item pith-row ${it.on ? "is-on" : ""} ${i === hi ? "is-active" : ""} ${it.danger ? "is-danger" : ""}`}
            onMouseEnter={() => setHi(i)}
            onClick={() => pick(i)}
          >
            <span>{it.label}</span>
            {it.hint !== undefined && <em>{it.hint}</em>}
          </button>
        );
      })}
    </div>,
    document.body,
  );
}

function textOf(n: React.ReactNode): string {
  if (typeof n === "string" || typeof n === "number") return String(n);
  if (Array.isArray(n)) return n.map(textOf).join("");
  if (isValidElement(n)) return textOf((n.props as { children?: React.ReactNode }).children);
  return "";
}
