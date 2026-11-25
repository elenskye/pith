// 顶栏：左边 Logo（点它回对话，打开时长一次、悬停再长），三个图标居中、不写字（悬停 0.35 秒后出名字）。
// 整条能拖动窗口（标题栏藏在它下面，tauri.conf.json 的 titleBarStyle: Overlay）。右上角不放 ⌘K 的提示（1.0.3）。
import { useEffect, useState } from "react";
import { Mark } from "@/shared/marks";
import { useNav, type Screen } from "@/app/nav";

const ICONS: { s: Exclude<Screen, "chat">; icon: string; name: string }[] = [
  { s: "kb", icon: "icons/kb", name: "知识库" },
  { s: "lib", icon: "icons/lib", name: "文献库" },
  { s: "set", icon: "icons/set", name: "设置" },
];

export function TopBar({ kbRef, fed }: { kbRef: React.RefObject<HTMLButtonElement | null>; fed: boolean }) {
  const nav = useNav();
  // App 打开时 Logo 长一次（.is-in，放完拿掉）
  const [intro, setIntro] = useState(true);
  useEffect(() => {
    const t = setTimeout(() => setIntro(false), 2000);
    return () => clearTimeout(t);
  }, []);

  return (
    <header className={`top ${nav.screen !== "chat" ? "is-lined" : ""}`} data-tauri-drag-region>
      <button
        className={`pith-brand ${intro ? "is-in" : ""}`}
        onClick={() => {
          if (nav.screen === "chat") nav.openChat(null);
          else nav.go("chat");
        }}
        aria-label="回到对话"
      >
        <Mark path="logo/pith-grow" />
        <span className="pith-wordmark">Pith</span>
      </button>
      <nav className="top-icons" data-tauri-drag-region>
        {ICONS.map(({ s, icon, name }) => (
          <button
            key={s}
            ref={s === "kb" ? kbRef : undefined}
            className={`pith-iconbtn pith-ink ${nav.screen === s ? "is-current" : ""} ${s === "kb" && fed ? "is-fed" : ""}`}
            onClick={() => nav.go(s)}
            aria-label={name}
          >
            <Mark path={icon} />
            <span className="pith-tip">{name}</span>
          </button>
        ))}
        <i className={`top-dot ${nav.screen !== "chat" ? `at-${nav.screen}` : ""}`} />
        <span className={`pith-badge ${fed ? "is-in" : ""}`}>+1</span>
      </nav>
    </header>
  );
}
