import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "@/app/App";
import { installPlainInputs } from "@/shared/plainInputs";
import { startRenderer } from "@/cite/renderer";
import { PickerApp } from "@/picker/Picker";
import "./index.css";

const params = new URLSearchParams(location.search);
// 置顶搜索窗（Word 里插入 / 编辑引文）与主窗口共用这一份前端，按 ?window=picker 区分
const isPicker = params.get("window") === "picker";

// 浏览器里预览界面：`pnpm dev` → http://localhost:1430/?mock（只在开发构建里，打包时剔除；搜索窗：?mock&window=picker）
if (import.meta.env.DEV && params.has("mock")) {
  await (await import("./dev/mockIpc")).installMockIpc();
} else if (!isPicker) {
  // Word 引文的排版器（citeproc-js）：Rust 经事件请求、这里排好交回（decisions/0016）。只在主窗口
  startRenderer().catch((e) => console.error("引文排版器没有启动", e));
}

installPlainInputs();

// 本机 IPC：不需要网络重试、切回窗口自动刷新
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isPicker ? (
      <PickerApp />
    ) : (
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    )}
  </React.StrictMode>,
);
