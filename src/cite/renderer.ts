// 引文排版桥的前端一侧：Rust（Word 宏经本机服务、Word 外刷新）发来 cite-render 事件，
// 这里用 citeproc-js 排好，调用 cite_render_done 交回。挂好监听后告诉 Rust「就绪」。
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RenderRequest } from "./types.ts";

export async function startRenderer(): Promise<void> {
  await listen<{ id: number; request: RenderRequest }>("cite-render", async ({ payload }) => {
    try {
      // citeproc 与样式文件第一次排版时才加载，不拖慢启动
      const [{ render }, { locales, styleSource }] = await Promise.all([import("./engine.ts"), import("./resources.ts")]);
      const result = render(payload.request, await styleSource(payload.request.style), await locales());
      await invoke("cite_render_done", { id: payload.id, result, error: null });
    } catch (e) {
      await invoke("cite_render_done", { id: payload.id, result: null, error: e instanceof Error ? e.message : String(e) });
    }
  });
  await invoke("cite_renderer_ready");
}
