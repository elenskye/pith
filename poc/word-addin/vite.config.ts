import { existsSync, readFileSync } from "node:fs";
import { defineConfig } from "vite";

// Office 侧载清单写死了 localhost:3127，端口被占用时直接报错，不要换端口。
// Word for Mac 只接受 HTTPS 地址：证书放在仓库外的 local/certs（不进 Git），
// 生成方法见 README。没有证书时退回 HTTP，只能在浏览器 ?mock 模式下用。
const certDir = new URL("../../local/certs/", import.meta.url);
const key = new URL("localhost.key", certDir);
const cert = new URL("localhost.crt", certDir);
const https = existsSync(key) && existsSync(cert) ? { key: readFileSync(key), cert: readFileSync(cert) } : undefined;

export default defineConfig({
  server: { host: "localhost", port: 3127, strictPort: true, https },
  preview: { host: "localhost", port: 3127, strictPort: true, https },
  build: { rollupOptions: { input: { taskpane: "taskpane.html" } } },
});
