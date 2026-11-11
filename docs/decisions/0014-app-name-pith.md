# 0014 · App 名字：Pith

- 日期：2026-11-09
- 状态：已定（用户在 v0.1 正式版实测后选定）

## 决定

App 正式名 **Pith**（「要旨、精髓」）。「Research Lab」只是开发名，之前的文档、问卷里说的 Research Lab 就是 Pith。

| 地方 | 改成 |
| --- | --- |
| 菜单栏、窗口标题、`Pith.app` | Pith |
| Bundle ID | `com.elenskye.pith` |
| 正式 / 开发数据目录 | `~/Pith/`、`~/Pith Dev/`（原 `~/ResearchLab/`、`~/ResearchLab Dev/` 整个改名搬过来，库、备份、补全结果都保留） |
| 覆盖数据目录的环境变量 | `PITH_DATA_DIR` |
| 整库导出的文件夹 | `Pith 导出 <时间>/` |
| 联网补全的 User-Agent | `Pith/<版本>` |
| Rust 包 / 库 | `pith` / `pith_lib`（测试命令 `cargo test -p pith …`） |
| v0.2 Word 顶栏选项卡 | Pith |

## 不改的

- 整库导出的格式标识 `research-lab-library`：数据格式一旦定下就不改，旧导出、冻结的样本库都靠它识别。
- Phase 0 原型（`poc/word-addin`）里的「Research Lab」字样与引文命名空间 `urn:research-lab:citations:v1`：原型不动，v0.2 W1 把引文写入移进 App 时再定正式命名空间。
- 仓库目录 `personal-research-lab`、Zotero 副本目录 `~/ResearchLab-test/`、测试临时目录前缀 `researchlab-test-`：用户看不到或属于用户自己的文件夹。

## 影响

- Bundle ID 变了，macOS 把 Pith 当成新 App：WebView 里记的界面偏好（标签栏开合、「含子集合」）回到默认。
- 旧的 `Research Lab.app` 已删；用 `target/release/bundle/macos/Pith.app`。
