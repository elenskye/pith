// 在 Node 里排一次引文（与 App 的 WebView 同一份代码 src/cite/engine.ts）：标准输入是 Rust 的 RenderRequest（JSON），
// 标准输出是 Rendered（JSON）。给 docx-engine 的命令行与真实数据测试用（crates/docx-engine/src/cite/node.rs）。只认内置样式。
import { readFileSync } from "node:fs";
import { render } from "../src/cite/engine.ts";
import { locales, styleSource } from "../src/cite/testing.ts";
import type { RenderRequest } from "../src/cite/types.ts";

const req = JSON.parse(readFileSync(0, "utf8")) as RenderRequest;
process.stdout.write(JSON.stringify(render(req, styleSource(req.style), locales())));
