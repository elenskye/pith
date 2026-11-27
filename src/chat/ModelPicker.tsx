// 选模型（1.1.0 验收，decisions/0041）：两层——左边一列是供应商，右边是这一家的模型；点模型就选好。
// 输入框下面一行、设置 › 模型与费用 都用它。弹出层放在 body 上、贴着按钮（下面放不下就翻到上面），
// 左右、上下都收在窗口里，长的名字换行不截断；两列各自滚动。
// 思考不在这里选：每个模型想不想、想多深在设置里定一次（这里只显示）。
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ModelProvider, ModelProviderView, ModelStatus, ModelView } from "@/shared/ipc";
import { usePresence } from "@/shared/controls";

export const EFFORT: Record<string, string> = { minimal: "极浅", low: "浅", medium: "中", high: "深", xhigh: "更深", max: "最深" };
export const effortLabel = (e: string | null | undefined) => (e ? (EFFORT[e] ?? e) : "");

/** 「想 · 中」「不想」 */
export function thinkLabel(m: Pick<ModelView, "thinking" | "choice">) {
  if (m.thinking === "never" || !m.choice.on) return "不想";
  return m.choice.effort ? `想 · ${effortLabel(m.choice.effort)}` : "想";
}

export const price = (m: ModelView) => (m.priced ? `$${m.input} / $${m.output}` : "价格不明");

export function findModel(s: ModelStatus, id: string | null | undefined): { p: ModelProviderView; m: ModelView } | null {
  for (const p of s.providers) {
    const m = p.models.find((x) => x.id === id);
    if (m) return { p, m };
  }
  return null;
}

type Props = {
  status: ModelStatus;
  /** 选中的模型 id；null 是「自动」 */
  value: string | null;
  onPick: (id: string | null) => void;
  /** 有「自动」这一项（子代理：对话那一家最强的） */
  auto?: { label: string; hint: string };
  /** 只列这一家（子代理跟着对话那一家） */
  only?: ModelProvider;
  /** line：输入框下面一行的小字；field：设置里有框的 */
  variant?: "line" | "field";
  /** 弹出层底下的一行（「设置 › 模型与费用 →」） */
  footer?: React.ReactNode;
  title?: string;
  className?: string;
};

const POP_W = 520;

export function ModelPicker({ status, value, onPick, auto, only, variant = "field", footer, title, className = "" }: Props) {
  const btn = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const presence = usePresence(open, 150);
  const cur = findModel(status, value);
  const providers = status.providers.filter((p) => !only || p.id === only);
  const [side, setSide] = useState<ModelProvider>(cur?.p.id ?? providers[0]?.id ?? "anthropic");
  const [pos, setPos] = useState<{ left: number; top?: number; bottom?: number; width: number; maxHeight: number; up: boolean } | null>(null);
  const [hi, setHi] = useState(-1);
  const close = useCallback(() => {
    setOpen(false);
    btn.current?.focus({ preventScroll: true });
  }, []);

  // 打开时回到选中的那一家
  useEffect(() => {
    if (open) setSide(cur?.p.id ?? providers[0]?.id ?? "anthropic");
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  // 位置：贴着按钮，放不下就翻上去；宽、高都收在窗口里
  useLayoutEffect(() => {
    if (!open || !btn.current) return;
    const place = () => {
      const r = btn.current!.getBoundingClientRect();
      const width = Math.min(only ? 340 : POP_W, innerWidth - 16);
      const below = innerHeight - r.bottom - 14;
      const above = r.top - 14;
      const up = below < 320 && above > below;
      const maxHeight = Math.max(180, Math.min(440, up ? above : below));
      const left = Math.max(8, Math.min(r.left, innerWidth - width - 8));
      setPos(up ? { left, bottom: innerHeight - r.top + 6, width, maxHeight, up } : { left, top: r.bottom + 6, width, maxHeight, up });
    };
    place();
    addEventListener("resize", place);
    return () => removeEventListener("resize", place);
  }, [open, only]);

  // 点外面、窗口失焦收起
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const t = e.target as Node | null;
      if (t && (pop.current?.contains(t) || btn.current?.contains(t))) return;
      setOpen(false);
    };
    const blur = () => setOpen(false);
    addEventListener("mousedown", down, true);
    addEventListener("blur", blur);
    return () => {
      removeEventListener("mousedown", down, true);
      removeEventListener("blur", blur);
    };
  }, [open]);

  const sideP = providers.find((p) => p.id === side) ?? providers[0];
  const usable = (p: ModelProviderView) => p.ready || status.mock;
  // 右边一列能选的：「自动」+ 这一家的模型
  const rows: { id: string | null; m?: ModelView }[] = [...(auto ? [{ id: null }] : []), ...(sideP?.models ?? []).map((m) => ({ id: m.id, m }))];

  useEffect(() => {
    if (!open) return;
    const i = rows.findIndex((r) => r.id === value);
    setHi(i);
    pop.current?.focus({ preventScroll: true });
  }, [open, side]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (open) pop.current?.querySelector(`[data-row="${hi}"]`)?.scrollIntoView({ block: "nearest" });
  }, [hi, open]);

  const pick = (id: string | null) => {
    if (sideP && !usable(sideP) && id !== null) return;
    setOpen(false);
    onPick(id);
    btn.current?.focus({ preventScroll: true });
  };

  const onKey = (e: React.KeyboardEvent) => {
    const pi = providers.findIndex((p) => p.id === side);
    if (e.key === "Escape") return (e.preventDefault(), close());
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const d = e.key === "ArrowDown" ? 1 : -1;
      setHi((h) => (rows.length ? (h + d + rows.length) % rows.length : -1));
    } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && !only) {
      e.preventDefault();
      const n = providers[(pi + (e.key === "ArrowRight" ? 1 : -1) + providers.length) % providers.length];
      if (n) setSide(n.id);
    } else if ((e.key === "Enter" || e.key === " ") && hi >= 0 && rows[hi]) {
      e.preventDefault();
      pick(rows[hi].id);
    }
  };

  const label = value === null && auto ? auto.label : (cur?.m.label ?? "选一个模型");
  return (
    <>
      <button
        ref={btn}
        type="button"
        title={title ?? (cur ? `${cur.p.label} · ${cur.m.label}` : undefined)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`mp-btn ${variant === "line" ? "mp-btn--line" : "mp-btn--field"} ${open ? "is-open" : ""} ${className}`}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {variant === "field" && cur && !(value === null && auto) && <span className="mp-btn-p">{cur.p.label}</span>}
        <span className="mp-btn-m">{label}</span>
        {variant === "line" && cur && <em>{thinkLabel(cur.m)}</em>}
        {variant === "line" && status.mock && <em className="is-mock">模拟</em>}
      </button>
      {presence.mounted &&
        pos &&
        createPortal(
          <div
            ref={pop}
            className={`mp-pop ${only ? "is-one" : ""} ${pos.up ? "is-up" : ""} ${presence.leaving ? "is-out" : ""}`}
            style={{ left: pos.left, top: pos.top, bottom: pos.bottom, width: pos.width, maxHeight: pos.maxHeight }}
            role="dialog"
            aria-label="选模型"
            tabIndex={-1}
            onKeyDown={onKey}
          >
            <div className="mp-cols">
              {!only && (
                <ul className="mp-providers" role="listbox" aria-label="供应商">
                  {providers.map((p) => (
                    <li key={p.id}>
                      <button
                        type="button"
                        className={`mp-provider ${p.id === side ? "is-side" : ""} ${p.id === cur?.p.id ? "is-on" : ""}`}
                        onMouseEnter={() => setSide(p.id)}
                        onFocus={() => setSide(p.id)}
                        onClick={() => setSide(p.id)}
                      >
                        <i className={usable(p) ? "is-ready" : ""} />
                        <span>{p.label}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <ul className="mp-models" role="listbox" aria-label={sideP?.label}>
                {sideP && !usable(sideP) && (
                  <li className="mp-need">{sideP.id === "vertex" ? "先在设置里填项目 ID，并用 gcloud 登录" : `先在设置里填 ${sideP.label} 的密钥`}</li>
                )}
                {rows.map((r, i) => {
                  const on = r.id === value;
                  const disabled = !!r.m && !!sideP && !usable(sideP);
                  return (
                    <li key={r.id ?? "auto"}>
                      <button
                        type="button"
                        data-row={i}
                        role="option"
                        aria-selected={on}
                        disabled={disabled}
                        className={`mp-item ${on ? "is-on" : ""} ${i === hi ? "is-active" : ""}`}
                        onMouseEnter={() => setHi(i)}
                        onClick={() => pick(r.id)}
                      >
                        {r.m ? (
                          <>
                            <b>{r.m.label}</b>
                            <span>
                              {thinkLabel(r.m)}
                              <em className="num">{price(r.m)}</em>
                              {r.m.found && <em>{r.m.source === "api" ? "接口报的" : "推断的"}</em>}
                            </span>
                          </>
                        ) : (
                          <>
                            <b>{auto!.label}</b>
                            <span>{auto!.hint}</span>
                          </>
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
            {footer && <div className="mp-foot">{footer}</div>}
          </div>,
          document.body,
        )}
    </>
  );
}
