# 第三方依赖

> 所有引入的依赖都登记在这里（名称、版本、许可证、用途、链接）。版本以 `Cargo.lock` 为准。
> 本项目目前不分发二进制；分发前需复核各许可证的声明义务（MIT / Apache-2.0 需保留版权与许可声明）。

## Rust · src-tauri（App 后端，v0.1）

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| tauri / tauri-build | 2.12.5 / 2.7.4 | MIT OR Apache-2.0 | 桌面 App 框架 | https://github.com/tauri-apps/tauri |
| tauri-plugin-opener | 2.7.2 | MIT OR Apache-2.0 | 在访达中显示数据目录 | https://github.com/tauri-apps/plugins-workspace |
| rusqlite（features: bundled、backup） | 0.41.1 | MIT | 读写 library.db；backup：恢复时整库复制 | https://github.com/rusqlite/rusqlite |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 | 命令参数与返回值 | https://github.com/serde-rs |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | ULID 的随机部分 | https://github.com/rust-random/getrandom |
| tauri-plugin-dialog | 2.8.4 | Apache-2.0 OR MIT | 导入时选择文件夹 / 文件的系统对话框（Phase 2） | https://github.com/tauri-apps/plugins-workspace |
| biblatex | 0.12.0 | MIT OR Apache-2.0 | 解析 BibTeX / BibLaTeX（Phase 2） | https://github.com/typst/biblatex |
| sha2 / md-5 | 0.11.1 / 0.11.0 | MIT OR Apache-2.0 | 附件复制进文件库时边写边算 SHA-256 / MD5（Phase 2） | https://github.com/RustCrypto/hashes |
| ureq（features: native-tls、gzip） | 3.5.1 | MIT OR Apache-2.0 | 联网补全的 HTTP 请求（Phase 5，`decisions/0012`）；v0.2 W3 起也用来调用 Anthropic 的 Messages API（流式，`decisions/0018`） | https://github.com/algesten/ureq |
| security-framework | 3.7.0 | MIT OR Apache-2.0 | 正式版把 Anthropic API 密钥存进 macOS 钥匙串（v0.2 W3，`decisions/0018`；此前已是 native-tls 的间接依赖） | https://github.com/kornelski/rust-security-framework |
| regex | 1.13.1 | MIT OR Apache-2.0 | 排版 Agent：核对模型给的文字规则、模拟模型认题注（v0.2 W3；文档引擎已在用） | https://github.com/rust-lang/regex |
| zotero-import（本仓库 `crates/zotero-import`） | 0.0.1 | 本项目 | Zotero 只读读取与审计（Phase 2 起由 App 引用） | — |
| docx-engine（本仓库 `crates/docx-engine`） | 0.2.0 | 本项目 | Word 文档引擎：引文写入与核对、VBA 工程与 Word 模板生成（v0.2 起由 App 引用，`decisions/0016`） | — |
| roxmltree（开发） | 0.21.2 | MIT OR Apache-2.0 | 测试里读回 Word 模板的选项卡 XML | https://github.com/RazrFalcon/roxmltree |

Phase 2 新增的间接依赖：biblatex 带入 paste、unscanny、unicode-normalization（MIT OR Apache-2.0，后者另含 Unicode-3.0）、strum / strum_macros（MIT）、tinyvec（Zlib OR Apache-2.0 OR MIT）、roman-numerals-rs（0BSD OR CC0-1.0）；tauri-plugin-dialog 带入 tauri-plugin-fs（Apache-2.0 OR MIT）、rfd（MIT）。无 GPL / AGPL。

Phase 5 新增的间接依赖：ureq-proto、native-tls、security-framework / security-framework-sys（macOS 系统 TLS）、rustls-pki-types、der、pem-rfc7468、base64ct、zeroize、utf8-zero、foreign-types（MIT OR Apache-2.0）；webpki-root-certs（CDLA-Permissive-2.0，根证书数据；我们用系统证书，不用它）；openssl / openssl-sys / schannel（Apache-2.0 / MIT，只在 Linux / Windows 上编译，macOS 不用）。无 GPL / AGPL。

间接依赖（Tauri 带入，约 300 个）：绝大多数 MIT / Apache-2.0；另有 Unicode-3.0、Zlib、BSD-3-Clause（brotli 等）、MPL-2.0（cssparser、selectors、dtoa-short、option-ext，文件级著佐权，个人使用无影响）、CC0。无 GPL / AGPL。

## Node · 根目录（App 前端）

版本以 `pnpm-lock.yaml` 为准。

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| react / react-dom | 19.3.1 | MIT | 界面 | https://react.dev |
| @tanstack/react-query | 5.107.1 | MIT | 调用后端的数据缓存 | https://tanstack.com/query |
| @tauri-apps/api | 2.12.4 | MIT OR Apache-2.0 | 调用 Rust 命令、浏览器 mock | https://github.com/tauri-apps/tauri |
| lucide-react | 1.70.0 | ISC | 图标；Word 选项卡的 6 个图标也由它的图标数据生成 PNG（`scripts/word-icons.mjs`，打包进 Pith.dotm，v0.2 W2） | https://lucide.dev |
| @tauri-apps/plugin-dialog | 2.8.4 | MIT OR Apache-2.0 | 选择文件夹 / 文件（Phase 2） | https://github.com/tauri-apps/plugins-workspace |
| pdfjs-dist | 6.5.438 | Apache-2.0 | PDF 阅读器：解析、渲染、文字层、查找（Phase 4，`decisions/0011`） | https://github.com/mozilla/pdf.js |
| @tauri-apps/cli（开发） | 2.12.4 | MIT OR Apache-2.0 | `tauri dev / build / icon` | https://github.com/tauri-apps/tauri |
| vite / @vitejs/plugin-react（开发） | 8.3.6 / 6.2.1 | MIT | 开发服务器与打包 | https://vite.dev |
| tailwindcss / @tailwindcss/vite（开发） | 4.3.5 | MIT | 样式 | https://tailwindcss.com |
| typescript（开发） | 6.0.3 | Apache-2.0 | 类型检查 | https://www.typescriptlang.org |
| @types/react / @types/react-dom（开发） | 19.3.0 | MIT | 类型 | https://github.com/DefinitelyTyped/DefinitelyTyped |
| citeproc（citeproc-js） | 2.4.63 | CPAL-1.0 OR AGPL-1.0（包内声明） | Word 引文排版，在 WebView 里跑（v0.2，`decisions/0006`、`0016`）；个人使用不分发没有问题，分发前需复核 | https://github.com/Juris-M/citeproc-js |
| @types/node（开发） | 26.6.6 | MIT | 前端引文排版测试（node --test）的类型 | https://github.com/DefinitelyTyped/DefinitelyTyped |

引文样式与语言文件（`src/cite/csl/`、`src/cite/locales/`，打包进 App）：citation-style-language 官方仓库，CC BY-SA 3.0，按提交固定，来源与哈希见 `src/cite/SOURCES.md`。

pdfjs-dist 随包带的运行时资源（打包时复制到 `dist/pdfjs/`，各自的许可证文件一并复制）：

| 资源 | 许可证 | 用途 |
| --- | --- | --- |
| `cmaps/`（Adobe CMap） | BSD-3-Clause（Adobe） | 中日韩等没嵌入编码表的字体 |
| `standard_fonts/` Foxit 字体 | BSD-3-Clause（PDFium） | PDF 标准 14 种字体的替代 |
| `standard_fonts/` Liberation 字体 | GPL-2.0 + 字体例外（只用来显示 PDF，不影响 App 本身的许可） | Arial / Times 等没嵌入时的替代 |
| `wasm/` openjpeg、jbig2、qcms | BSD-2-Clause、BSD-3-Clause（PDFium）、MIT | JPEG 2000、JBIG2 图片解码，色彩管理 |
| `iccs/` | CC0-1.0 | CMYK 色彩配置 |

没带：PDF 脚本解释器 quickjs（App 不运行 PDF 里的脚本）、无 wasm 时的后备脚本。pdfjs-dist 的可选依赖 `@napi-rs/canvas`（MIT）只在 Node 里渲染时用，会装进 `node_modules`，不进 App。

App 图标（`src-tauri/icons/`）由一次性脚本绘制（三本书脊 + 书架），`tauri icon` 生成，属本项目。

## Rust · crates/zotero-import（Phase 0-A）

### 直接依赖

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| rusqlite（features: bundled） | 0.41.1 | MIT | 只读打开 Zotero 数据库 | https://github.com/rusqlite/rusqlite |
| SQLite（经 libsqlite3-sys 0.39.1 内置） | 3.54.1 | Public Domain | 数据库引擎 | https://sqlite.org/copyright.html |
| serde | 1.0.229 | MIT OR Apache-2.0 | 序列化 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | JSON 输出 | https://github.com/serde-rs/json |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | 文件 SHA-256 | https://github.com/RustCrypto/hashes |
| md-5 | 0.11.0 | MIT OR Apache-2.0 | 文件 MD5（对比 Zotero storageHash） | https://github.com/RustCrypto/hashes |

### 间接依赖（许可证汇总）

全部为宽松许可证，无 GPL / AGPL：

- MIT OR Apache-2.0：bitflags、block-buffer、cc、cfg-if、const-oid、cpufeatures、crypto-common、digest、fallible-iterator、fallible-streaming-iterator、find-msvc-tools、hashbrown、hashlink、hybrid-array、itoa、libc、pkg-config、proc-macro2、quote、serde_core、serde_derive、shlex、smallvec、syn、thiserror、thiserror-impl、typenum、vcpkg
- MIT：libsqlite3-sys、rsqlite-vfs、sqlite-wasm-rs、zmij
- Zlib：foldhash
- Unlicense OR MIT：memchr
- (MIT OR Apache-2.0) AND Unicode-3.0：unicode-ident

## Rust · crates/docx-engine（Phase 0-C 起；v0.2 由 App 引用）

### 直接依赖

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| zip（features: deflate） | 9.4.1 | MIT | 读写 .docx 包，未改部件原样复制 | https://github.com/zip-rs/zip2 |
| quick-xml | 0.42.0 | MIT | 可编辑 XML 树的底层（排版器） | https://github.com/tafia/quick-xml |
| roxmltree | 0.21.2 | MIT OR Apache-2.0 | 独立校验器的 XML 解析 | https://github.com/RazrFalcon/roxmltree |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 | 排版规范 JSON | https://github.com/serde-rs |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | 部件指纹 | https://github.com/RustCrypto/hashes |
| regex | 1.13.1 | MIT OR Apache-2.0 | 文档计划里按文字识别段落（如题注） | https://github.com/rust-lang/regex |
| cfb | 0.16.0 | MIT | 写 / 读复合文件（vbaProject.bin，v0.2 W1） | https://github.com/mdsteele/rust-cfb |
| docx-rs（开发，仅评估） | 0.4.22 | MIT | 往返评估 | https://github.com/bokuweb/docx-rs |
| docx-rust（开发，仅评估） | 0.1.11 | MIT | 往返评估 | https://github.com/cstkingkey/docx-rs |

间接依赖（正式依赖部分）：MIT OR Apache-2.0（flate2、crc32fast、indexmap、hashbrown、typed-path、getrandom、log；cfb 带入的 fnv、uuid、web-time 等）、MIT（nix、simd-adler32、zmij）、Zlib（zlib-rs）、Apache-2.0（zopfli）、Unlicense OR MIT（memchr）、Unicode-3.0（unicode-ident）。无 GPL / AGPL。

### 仅评估、不是项目依赖

| 名称 | 版本 | 许可证 | 说明 |
| --- | --- | --- | --- |
| python-docx（及 lxml） | 1.2.0（lxml 6.1.3） | MIT（lxml BSD-3-Clause） | 只用于 0-C 对比评估；评估用的虚拟环境已删除，复现方法见 `crates/docx-engine/README.md` |

## Node · poc/word-addin（Phase 0-B）

版本以 `poc/word-addin/pnpm-lock.yaml` 为准。PoC 不分发。

### 直接依赖

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| citeproc（citeproc-js） | 2.4.63 | CPAL-1.0 OR AGPL-3.0 | CSL 引用格式化引擎 | https://github.com/Juris-M/citeproc-js |
| fflate（开发） | 0.8.3 | MIT | 读写 .docx（zip） | https://github.com/101arrowz/fflate |
| @xmldom/xmldom（开发） | 0.9.12 | MIT | Node 端 XML 解析 | https://github.com/xmldom/xmldom |
| vite（开发） | 8.3.6 | MIT | 加载项开发服务器与构建 | https://vite.dev |
| typescript（开发） | 6.0.3 | Apache-2.0 | 类型检查 | https://www.typescriptlang.org |
| @types/office-js（开发） | 1.0.616 | MIT | Office.js 类型 | https://www.npmjs.com/package/@types/office-js |
| @types/node（开发） | 26.6.6 | MIT | Node 类型 | https://www.npmjs.com/package/@types/node |

间接依赖（均为开发依赖）：MIT（rolldown、postcss、nanoid、picomatch、tinyglobby、fdir、@oxc-project/types 等）、Apache-2.0（detect-libc）、BSD-3-Clause（source-map-js）、ISC（picocolors）、MPL-2.0（lightningcss）。无 GPL / AGPL。

### 数据文件

| 名称 | 许可证 | 用途 | 来源 |
| --- | --- | --- | --- |
| CSL 样式 5 个、语言文件 2 个 | CC BY-SA 3.0 | 引用格式 | citation-style-language/styles、/locales，按提交固定，见 `poc/word-addin/public/csl/SOURCES.md` |

### 运行时加载（不打包）

| 名称 | 条款 | 用途 |
| --- | --- | --- |
| Office.js（`appsforoffice.microsoft.com`） | 微软服务条款 | Word 加载项 API，微软要求从官方 CDN 加载 |

## Rust · v0.3 事实调查（src-tauri 的 `research` 模块与 crates/research-engine，云端 2026-11-12 加）

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| research-engine（本仓库 `crates/research-engine`） | 0.3.0 | 本项目 | 事实调查的纯逻辑：流程、核对、可信度、计算、新旧对比、渲染（只依赖 serde / serde_json） | — |
| scraper | 0.27.0 | ISC | 下载到的网页取正文（HTML 解析；和 Tauri 共用 html5ever，不多带一套） | https://github.com/rust-scraper/scraper |
| encoding_rs | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause | 网页、CSV 的编码识别（GBK / GB18030 等）；已是 Tauri 的间接依赖，现在直接用 | https://github.com/hsivonen/encoding_rs |
| calamine | 0.38.0 | MIT | 读 XLSX / XLS 统计表 | https://github.com/tafia/calamine |
| pdf-extract | 0.12.1 | MIT | PDF 取文字（取不出的标「取文字失败」） | https://github.com/jrmuizel/pdf-extract |

许可证评估：都是 MIT / ISC / Apache-2.0 一类的宽松许可证，没有 GPL / AGPL。

新增的间接依赖：
- calamine 带入 atoi_simd、debug_unsafe、fast-float2、codepage（MIT OR Apache-2.0）。
- pdf-extract 带入 lopdf、pom、adobe-cmap-parser、type1-encoding-parser（MIT），cff-parser、ttf-parser、postscript、euclid、unicode-bidi（MIT OR Apache-2.0）；
  lopdf 又带入 nom 8（MIT）、ecb（MIT）、aes / cbc / cipher / block-padding / inout / md-5 0.10 / rand 0.10 / rand_core / chacha20（RustCrypto 与 rust-random，MIT OR Apache-2.0，读加密 PDF 用）、rangemap（MIT/Apache-2.0）、stringprep（MIT/Apache-2.0）、unicode-properties（(MIT OR Apache-2.0) AND Unicode-3.0）。
- scraper 带入 ego-tree（ISC）；html5ever、selectors、cssparser 与 Tauri 共用同一个版本（MPL-2.0 的 selectors、cssparser 原来就在，见上）。

测试与假服务器只用标准库（`std::net::TcpListener`），没有新的开发依赖。

## Rust · v0.4 概念研究（src-tauri 的 `concept` 模块与 crates/concept-engine，云端 2026-11-13 加）

**没有新的第三方依赖**（Cargo.lock 只多了本仓库的 concept-engine）：

| 名称 | 版本 | 许可证 | 用途 | 链接 |
| --- | --- | --- | --- | --- |
| concept-engine（本仓库 `crates/concept-engine`） | 0.4.0 | 本项目 | 概念研究的纯逻辑：EPUB / Mathpix 文本层、段落号与页码、中英对齐、技能与工作流、核对与审计、关系图、渲染 | — |
| zip（features: deflate） | 9.4.1 | MIT | concept-engine 读 EPUB（压缩包）、测试里现拼夹具 EPUB；和 docx-engine 同一个版本 | https://github.com/zip-rs/zip2 |
| pdf-extract | 0.12.1 | MIT | 短 PDF 数页数、没有 Mathpix 密钥时按页取文字（和事实调查同一个库） | https://github.com/jrmuizel/pdf-extract |

- XHTML / OPF / NCX 用 concept-engine 自己的容错读取（`markup.rs`，只用标准库），没有用 roxmltree（遇到 HTML 实体报错）或 scraper（按 HTML 解析会把自闭合的锚点当成没关上的元素）。
- 夹具的英文文字来自 Project Gutenberg #3300（公有领域，去掉了 PG 的商标和许可说明），见 `crates/concept-engine/fixtures/wealth/README.md`。

## 参考但未引入

| 名称 | 许可证 | 关系 |
| --- | --- | --- |
| Zotero | AGPL-3.0 | 只读取其数据库文件，不复制代码 |
| zotero-word-for-mac-integration | AGPL-3.0 | 只研究其机制（全局模板 + AppleScriptTask），不复制代码 |
