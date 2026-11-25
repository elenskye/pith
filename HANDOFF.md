# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.0.1 做完（2026-11-25）

视觉方案 `pith` 1.0.1 同步进来、接上了变量、字、明暗和 App 图标。界面照新架构重做是 1.1.0 的事，开工前先合成 `0030`。

### 本轮：Pith 1.0.1（Atelier 不升版本，还是 1.0.1）

**Atelier**（提交 `e5f8fbd`，tag `pith-v1.0.1`）

1. 两条新规矩（用户定）：
   - 除非用户明确要求，画布和 Design System 不和云端同步（不拉回、不发回、不在云端新建或更新）：`CLAUDE.md`「画布与 bench」、`docs/cloud.md`、`skills/design-loop`。
   - 只有本体（`bench/`、共用的 `tooling/`、`skills/`、`docs/`、根目录）的开发才升仓库的版本号；方案、画布这类平时的更新不升，只升方案自己的：`CLAUDE.md`「版本与 Git」。
   - `ROADMAP.md`「已拍板的决定」记了；`build:ds` 和 bench 在「本地改了、云端没更新」时的提示跟着改了。
2. **TopBar 第二个图看不到**：`marks/icons/lib.svg` 的叶书签一个元素有两个 `style`，单独打开就解析失败。在 `tooling/pith/build.mjs` 里并成一个；以后生成的 SVG 有重复的属性就直接报错（拿旧文件试过，抓得到）。
3. **封面加叶子**：首页叶子的几何从画布挪进 `tooling/pith/glyphs.mjs`（`homeLeaf`），单独成一张 `marks/garden/leaf.svg`（新资产，几何就是你 11-24 验收的 EX11 首页那片，标成已批）；画布 R1 改成从这里取，6 张板逐字节不变。Design System 的 Cover：叶柄从「Pith」字标右上角长出去，主脉横过封面，副标题在句号处换行。
4. 方案 `pith` 1.0.0 → **1.0.1**；`DESIGN.md` 加第 9 节「在 Pith 里怎么接」（shadcn ↔ `--color-*` 的对照）、第 8 节加一条待确认（要不要危险色）；`sync.json` 加上叶子；`CHANGELOG.md`、`ROADMAP.md`、`docs/adoption.md` 跟着改。
5. 云端的 Design System **没更新**（新规矩），还是 1.0.0：TopBar 那张图在云端照旧看不到。

**Pith**

1. 照你这次说的，`pith-v1.0.1` 同步进 `src/vendor/pith/`（`npm run sync -- --scheme pith --to pith --tag pith-v1.0.1`，`--check` 一致）。
2. `src/index.css`：先引 `vendor/pith/pith.css`，shadcn 变量全指到 `--color-*`（对照在 `vendor/pith/DESIGN.md` 第 9 节）。注意 shadcn 的 `accent` 是淡底，Pith 的 `--color-accent` 是海蓝，`@theme inline` 让两边不打架。界面字号 13 → 13.5（`--text-ui`）。
3. 字：新依赖 `@fontsource/lato`、`@fontsource-variable/newsreader`（OFL，登记在 `THIRD_PARTY.md`），`src/fonts.css` 把 Newsreader 登记成 pith.css 认的名字；中文还是系统字体。
4. 明暗：`src/shared/theme.ts` 跟着系统写根上的 `data-theme`，Tailwind 的 `dark:` 也改成看它。
5. App 图标换成蕨芽：`scripts/app-icon.sh`（`pnpm app:icon`）把同步来的 1024 图缩到 macOS 图标网格（主体 824/1024，四周透明）再 `tauri icon`。
6. 版本 1.0.0 → 1.0.1（`package.json`、`Cargo.toml`、`tauri.conf.json`、`Cargo.lock`）。`.claude/launch.json` 加了 `web-mock-alt`（端口 1432）：1430 常被 Peck 的开发服务器占着。
7. 文档：`CLAUDE.md` 规则 9「现在」、`ROADMAP.md`（现在、已完成、验收清单）、`docs/architecture.md`、`THIRD_PARTY.md`。

### 测试

- Pith：`pnpm check` 全过：`pith` 281 通过、25 项默认忽略，其余 crate 全过，前端 17 + 29 通过。
- 浏览器 `?mock`（1440 宽，亮、暗都看了）：变量、Lato、明暗跟系统切换都对；文献库、详情、调查页的颜色正常。
- Atelier：`node tooling/pith/build.mjs --check`（37 个资产）；`npm run qa -- --scheme pith` 163 项全过；`check:ds` 12 个预览明暗都过，封面看过截图；`dw check pith-explore` 6 张全过；`check:canvases` 一致；`npm test` 37/37。没跑 `npm run all`、`build:motion`（动效没改）。

## 等你

1. **推送**：Pith `main`；Atelier `main` 和 tag（`pith-v1.0.1` 是新的，之前的几个也还没推）：
   `git -C ~/Documents/GitHub/visual-atelier push && git -C ~/Documents/GitHub/visual-atelier push --tags`
2. **看新样子**：`ROADMAP.md`「用户验收清单」1.0.1 的两条（正式版的配色、字、图标；bench 里的 Design System 封面和 TopBar）。
3. 云端的 Design System 要不要按 1.0.1 重发（新规矩下没让就不动）；Peck 的 Design System 要不要按 1.0.0 重发（共享给了组织外的人）。
4. **正式版**：第一次打开会先备份、把数据升到 v9（以前 0.4.1 就该做的）；之后 Word 模板点「更新」，两本《国富论》各「重新解析」一次。
5. **摘抄、笔记的导出文件夹**（1.1.0 做导入时要）：Flomo、Obsidian 导出的 Markdown，放到 `local/` 下，我只读副本。

## 已知的、没做的

- **文献库表格窄了标题列会塌**：窗口宽度在 1100 以下（最小 960）时标题列被挤没，作者压在图标上。以前就这样（换回旧字体量过，列宽一样），不是这轮改的。
- 琥珀、翠绿这些提示、完成的颜色还是 Tailwind 的色板（约 90 处），等 1.1.0 照 Pith 重做界面时一起换。
- `destructive` 借了 `color-second`（纸与海是绯红）；要不要一个危险色，记在方案 `DESIGN.md` 第 8 节。
- 中文字体先用系统的（宋体、苹方）；要不要随 App 带 Noto Serif SC、Noto Sans SC 待定（`DESIGN.md` 第 8 节）。
- 首页叶子打开时的生长动画还没做成方案的动效零件（现在只在画布 R1 里），做 1.1.0 首页时再做。
- 文档里引用的旧提交号、第五轮问卷的偏好只在旧 `.git` 备份里（11-24 重建 Git）。
- 11-15 那四个修复留下的没做的照旧（`decisions/0025`）。

## 下一轮

- **先问开发程度**（`CLAUDE.md` 规则 7）。
- 先把 `docs/atelier-handoff-2026-11-22.md` 和 `0028` 合成 `decisions/0030`，改 `CLAUDE.md` 规则 4、`requirements.md`、`ROADMAP.md`，删掉搬来的那份。
- 再写 1.1.0 的计划（新的决策文件、`ROADMAP.md` 分 Phase），版本号改 1.1.0，加 migration 0010 前先冻结 v9 样本库。大致顺序（待计划确认）：模型接口抽象 → Agent 循环和子任务 → 技能运行时 → 对话页和研究主题（首页的叶子、顶栏三个图标用 `vendor/pith/marks/`）→ 知识库 → 旧功能改成技能 → Word 润色。
- 界面只用 shadcn 变量和 `vendor/pith/bundle.css` 的 `pith-` 类；方案要改回 Atelier 改、发版、再同步。
