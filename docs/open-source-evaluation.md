# 开源组件评估

> 逐项：仓库、许可证、维护、Mac 兼容、资源、结论（直接用 / 借鉴 / 不集成）。按 Phase 0 子项分节，用到再评。

## 0-A Zotero 迁移

| 组件 | 许可证 | 用途 | 结论 |
| --- | --- | --- | --- |
| [zotero/zotero](https://github.com/zotero/zotero) | AGPL-3.0 | 数据库 schema 与行为的参考 | **只读其数据库，不复制代码**。夹具的表结构按真实库的列名手写，不拷贝 Zotero 的 `userdata.sql`。读取 AGPL 软件生成的数据文件不构成衍生作品 |
| Zotero 本地 API（`localhost:23119`） | — | 另一种读取途径 | **不用**：要求 Zotero 运行；用户最终会卸载 Zotero |
| Zotero 导出（CSL JSON / BibTeX / RDF） | — | 另一种读取途径 | **不作主路径**：没有集合树、回收站、附件哈希、笔记内嵌图片关系。v0.1 的 L2（BibTeX / CSL JSON / RIS 导入）照常做，面向非 Zotero 来源 |
| [rusqlite](https://github.com/rusqlite/rusqlite) 0.41 | MIT | Rust 读写 SQLite | **直接用**。`bundled` 内置 SQLite 3.54.1，不依赖系统库；Pomelo 同款 |
| [serde](https://github.com/serde-rs/serde) / serde_json | MIT OR Apache-2.0 | 中间模型序列化 | **直接用** |
| [RustCrypto sha2 / md-5](https://github.com/RustCrypto/hashes) 0.11 | MIT OR Apache-2.0 | 文件 SHA-256 / MD5 | **直接用**，纯 Rust |

资源：读取 + 全量哈希 + 审计 84 条 / 62 MB 用时 0.53 s，峰值内存 8 MB。

## 0-B Word 动态引用

| 组件 | 许可证 | 用途 | 结论 |
| --- | --- | --- | --- |
| [Juris-M/citeproc-js](https://github.com/Juris-M/citeproc-js)（npm `citeproc` 2.4.63） | CPAL-1.0 OR AGPL-3.0（npm 元数据误写为 AGPL-1.0；上游仓库附 CPAL 与 AGPLv3 全文） | CSL 引擎，Zotero 同款 | **直接用（PoC）**。npm 包最后发布于 2023，上游仍在维护（2026-07 有提交，Zotero 从源码构建）；v0.2 可改为按提交固定上游构建。个人使用不分发无许可问题，分发前重评 |
| [CSL styles](https://github.com/citation-style-language/styles) / [locales](https://github.com/citation-style-language/locales) | CC BY-SA 3.0 | APA 7、GB/T 7714-2015 / 2025、Chicago 18 | **直接用**，按提交固定（`poc/word-addin/public/csl/SOURCES.md`）。中文论文可能需要社区中文适配样式，v0.2 用真实论文定 |
| Office.js（`appsforoffice.microsoft.com/lib/1/hosted/office.js`） | 微软服务条款 | Word 加载项 API | **保留代码，暂不能用**：本机 Word 16.114 侧载失效（见 feasibility 0-B） |
| [@types/office-js](https://www.npmjs.com/package/@types/office-js) 1.0.616 | MIT | 类型 | 开发依赖 |
| [zotero/zotero-word-for-mac-integration](https://github.com/zotero/zotero-word-for-mac-integration) | AGPL-3.0 | Zotero 的 Mac Word 集成 | **只研究不复制**：本机 `Zotero.scpt` 证实其做法是「全局模板 + AppleScriptTask + do shell script」，本 PoC 的宏路线独立实现同类机制 |
| [fflate](https://github.com/101arrowz/fflate) 0.8.3 | MIT | Node 端读写 .docx（zip） | **直接用**（PoC 开发依赖），纯 JS、无依赖 |
| [@xmldom/xmldom](https://github.com/xmldom/xmldom) 0.9.12 | MIT | Node 端解析 / 序列化 OOXML | **直接用**（PoC 开发依赖） |
| [Vite](https://vite.dev) 8 / TypeScript 6 | MIT / Apache-2.0 | 加载项开发服务器与构建 | 与 Pomelo 同款 |
| [typst/hayagriva](https://github.com/typst/hayagriva) | MIT OR Apache-2.0 | Rust CSL 引擎 | **备选**：若将来要分发或把引用计算搬进 Rust 核心再评估；CSL 覆盖度需实测 |
| citeproc-rs | MPL-2.0 | Rust CSL 引擎 | **不选**：多年未维护 |

资源：7 条文献，Word 外刷新一个文件约 0.12 s；宏一次刷新的计算往返约 0.2 s；citeproc-js 浏览器打包 390 KB（gzip 103 KB）。

## 0-C Word 排版保真

用同一个校验器（`crates/docx-engine/src/verify.rs`）对三份文档做「读入 → 不改 → 写出」往返，再比较内容与结构。细节见 feasibility 0-C。

| 组件 | 许可证 | 往返结果 | 结论 |
| --- | --- | --- | --- |
| [bokuweb/docx-rs](https://github.com/bokuweb/docx-rs) 0.4.22 | MIT | 三份文档读取时都内部崩溃（`unreachable`） | **不用于编辑**；可作生成新文档的候选 |
| [cstkingkey/docx-rust](https://github.com/cstkingkey/docx-rs) 0.1.11 | MIT | 丢目录域结果、公式、修订、批注扩展部件、正文 22–24 字；.docm 丢 9 个部件 | **不用** |
| [python-openxml/python-docx](https://github.com/python-openxml/python-docx) 1.2.0 | MIT | 内容无损（无关部件被重新序列化）；拒绝 .docm | **不用**：API 覆盖不了中文排版关键项，需 Python 运行时 |
| [KaiserY/ooxmlsdk](https://github.com/KaiserY/ooxmlsdk) 0.15 | MIT OR Apache-2.0 | 未评估 | 备选 |
| extend-hq/rust-docx 0.2.14 | MIT | 未评估（2026-11-04 才发布） | 观察 |
| [tafia/quick-xml](https://github.com/tafia/quick-xml) 0.42 | MIT | — | **直接用**：自研可编辑 XML 树的底层，未改节点按原字节写回 |
| [RazrFalcon/roxmltree](https://github.com/RazrFalcon/roxmltree) 0.21 | MIT OR Apache-2.0 | — | **直接用**：独立校验器的解析器（与编辑器不同，避免同一 bug 两边都看不见） |
| [zip-rs/zip2](https://github.com/zip-rs/zip2) 9.4 | MIT | — | **直接用**：未改部件按原压缩字节复制 |

资源：Word 存的 3 页复杂文档，排版 + 校验 + 规范核对约 19 ms（release）；release 可执行文件 1.8 MB。
