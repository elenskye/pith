// 量换配色的帧（只在开发构建里，?perf 打开）：设置 › 外观点两次色板，再到文献库用 ⌘K 换一次，每次记 2.4 秒里的帧。
// ?perf=big 先把文献库的表格复制到 1500 行再量；默认 small、big 各一遍。结果 POST 给本机 7788（有的话），并交给 WKWebView 的 done 消息。
// 在真 WebKit 里量：scripts/webkit-perf.swift（docs/testing.md「换配色的帧」）。
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function frames(trigger: () => void) {
  const times: number[] = [];
  let start = 0;
  await new Promise<void>((res) => requestAnimationFrame((t) => ((start = t), res())));
  trigger();
  await new Promise<void>((res) => {
    const loop = (t: number) => {
      times.push(t);
      if (t - start < 2400) requestAnimationFrame(loop);
      else res();
    };
    requestAnimationFrame(loop);
  });
  const d = times.slice(1).map((t, i) => t - times[i]).sort((a, b) => a - b);
  return { frames: times.length, first: +(times[0] - start).toFixed(1), p50: +d[Math.floor(d.length * 0.5)].toFixed(1), p95: +d[Math.floor(d.length * 0.95)].toFixed(1), max: +d[d.length - 1].toFixed(1), over34: d.filter((x) => x > 34).length };
}

function clickAt(el: Element) {
  const r = el.getBoundingClientRect();
  el.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: r.left + 40, clientY: r.top + 12 }));
}

const post = (body: string) => fetch("http://127.0.0.1:7788/", { method: "POST", body }).catch(() => {});

export async function runPerfProbe(fromEnv?: string) {
  const variants = (new URLSearchParams(location.search).get("perf") || fromEnv || "small,big").split(",");
  await wait(4000);
  const out: unknown[] = [{ viewTransitions: "startViewTransition" in document, w: innerWidth, h: innerHeight, dpr: devicePixelRatio }];
  for (const v of variants) {
    (document.querySelector('[aria-label="设置"]') as HTMLElement).click();
    await wait(800);
    ([...document.querySelectorAll(".br-st")].find((b) => b.textContent?.includes("外观")) as HTMLElement)?.click();
    await wait(1500);
    const card = () => [...document.querySelectorAll(".pal-card")].find((c) => !c.classList.contains("is-on"))!;
    await post("go look1");
    await wait(300);
    const look1 = { ...(await frames(() => clickAt(card()))), water: (window as unknown as { __water?: unknown }).__water };
    await wait(1500);
    await post("go look2");
    await wait(300);
    const look2 = { ...(await frames(() => clickAt(card()))), water: (window as unknown as { __water?: unknown }).__water };
    if (v === "water") {
      out.push({ v, look1, look2 });
      continue;
    }
    (document.querySelector('[aria-label="文献库"]') as HTMLElement).click();
    await wait(2000);
    if (v === "big") {
      const tb = document.querySelector(".lib-table tbody")!;
      const rows = [...tb.children];
      while (tb.children.length < 1500) for (const r of rows) tb.append(r.cloneNode(true));
      await wait(500);
    }
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }));
    await wait(500);
    const row = [...document.querySelectorAll(".palette-row")].find((b) => b.textContent?.includes("换成"))!;
    const lib = await frames(() => clickAt(row));
    await wait(800);
    out.push({ v, look1, look2, lib });
  }
  const body = JSON.stringify(out);
  await post(body);
  (window as unknown as { webkit?: { messageHandlers?: { done?: { postMessage: (b: string) => void } } } }).webkit?.messageHandlers?.done?.postMessage(body);
}
