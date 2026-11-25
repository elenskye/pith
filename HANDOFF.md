# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.0.3 做完（2026-11-25）

这一轮只做界面优化（你说问卷先不填）：控件都换成 Pith 的样子、文献库两栏和标签树、导入挪进设置、知识库借了文献库的两栏、换配色不卡。为什么这么做、量到的数字在 `docs/decisions/0032`。

### 本轮做了什么（照你的十条）

1. **Logo 对齐**：按基线排，蕨芽往下挪 6/48，墨的下沿和「Pith」的基线差 0.25 像素（量过）。
2. **右上角的 ⌘K 删了**（`app/TopBar.tsx`）。
3. **⌘K 有退场**：往上收、淡出（`usePresence` 留 0.18 秒）；顺手修了出来时从右边滑进来的那一下（居中改用 `translate`）。
4. **换外观卡顿**：在 WKWebView 里量（`scripts/webkit-perf.swift` + `src/dev/perfProbe.ts`），卡的是整页的水面扭曲（SVG 滤镜，每帧约 100 毫秒，换一次掉到约 28 帧）。拿掉它；颜色改成 View Transitions 从点的地方洇开（`shared/theme.ts`）。你看到「波纹没了」是我改出来的毛病：拍旧画面那 0.2 秒吃掉了波纹最浓的一段，洇开又把它盖住——改成等洇开开始再起波纹、洇开跑在波纹前面，按时间截帧看过，五圈都在。现在外观页和 ⌘K 换都是满帧。
5. **下拉框、勾选框**：16 个系统下拉框全换成 `shared/controls.tsx` 的 `Select`（弹出的列表 `Menu` 也给右键菜单、「新建」用）；17 个勾选框、单选框加了 `pith-check`、`pith-radio`；弹窗、提示条、就地编辑也换了。新样子写进了 Atelier 的 Design System（七个组件，待批），发 `pith-v1.1.1`、同步进 `src/vendor/pith/`。
6. **文献库**（`library/`）：第一栏左边范围、右边类型和搜索；第二栏只放集合、左右滑，子集合点开接在后面（`Collections.tsx`）；标签树从左边长出来、只用两指滑（右滑出、左滑收，Esc 也收；`TagTree.tsx`）；右边面板换成 Pith 的字段；导入挪到 设置 › 导入（设置八页）。删了 `Sidebar.tsx`。标签树、面板都开着时窄，先藏类型列、再藏作者列。
7. **知识库**：第一栏一排知识库 | 种类、搜索；第二栏生长阶段；面板字段和文献库一套。
8. **文档**：新开 `decisions/0032`；`architecture.md`、`requirements.md`、`testing.md`（「换配色的帧」怎么量）、`README.md`、`ROADMAP.md` 跟着改。版本号三处改成 1.0.3。

### 测试

- `pnpm check` 全过（`pith` 288 通过、25 项默认忽略，其余 crate 全过，前端 17 + 29）。
- 帧（WKWebView，1280 × 800，Retina）：外观页点色板 144–145 帧、0 个慢帧；文献库 ⌘K 换 143 帧、0 个慢帧；文献库 1500 行时 118 帧、4 个慢帧（最长 149 毫秒，之前 76 帧、最长 280 毫秒）。
- 本地窗口：另一个 Claude 会话占着电脑操作工具，没法在 App 窗口里点。做了两样代替：打了一个临时的「Pith Check」包（别的包名，数据是你给的 Zotero 副本导进临时目录），只截它那一个窗口看了首页（Logo、右上角）；交互用同一个 WebKit 的小窗按脚本走了一遍文献库（标签树、集合展开、下拉框、面板、右键菜单）、知识库、设置 › 导入、⌘K 的退场，逐张截图看过。临时包用完删了。
- Atelier：`qa` 163 项、`check:ds` 19 个预览明暗、`build:motion`（palette-lake）、`dw check pith-explore` 6 张、`check:canvases` 都过。

### 提交

- Pith：这一轮一个提交（见 `git log`，`v1.0.3: polish ui, fix palette switch lag`），正式版装了 1.0.3。
- Atelier：`e75669f v1.0.1: pith 1.1.0, controls and ink palette switch`、`6ba502d v1.0.1: pith 1.1.1, keep lake ripples visible`，tag `pith-v1.1.0`、`pith-v1.1.1`。

## 等你

1. **推送**：Pith `main`；Atelier `main` 和两个 tag（`pith-v1.1.0`、`pith-v1.1.1`）。
2. **看新样子**（`ROADMAP.md`「用户验收清单」1.0.3 那几条）：换配色的波纹和洇开、文献库两指右滑、下拉框；Design System 七个新组件和 palette-lake 在 Atelier 里是待批，看了不满意直接打回。
3. **规划问卷**还等你填：https://claude.ai/artifact/HG1MF86byAvZQZVTojni68 。
4. 之前留下的：Peck 的 Design System 云端要不要重发；Flomo、Obsidian 的导出（等问卷）。

## 已知的、没做的

- 整页的水面晃动拿掉了，没有便宜的替代（WebKit 只能用 CPU 上的 SVG 滤镜逐像素扭曲，见 `0032`）。想要的话可以试「只晃点附近一小块、0.5 秒」，要先量。
- 文献库表格没做虚拟滚动：上千条时换配色、切范围还有一次约 0.2 秒的重算。
- 技能里面那几页（调查、概念、排版）、导入页的内容、设置里原来那几块的按钮和小字还是旧样子，只换了下拉框和勾选框。
- R1 画布里还留着水面扭曲（原型）。
- 回答还是演示的（1.1.0 接 Agent 循环）；知识库的材料先一库一份；角色的「偏好模型」「默认知识库」只是记下来。

## 下一轮

- 你填了问卷：读回答案（`db_get` `survey/answers` 的 `summary`），写进 `ROADMAP.md`「到 2.0.0」和一份新决策，再开 1.1.0（升版本号、分 Phase；加 migration 前冻结 v10 样本库）。
- 还是界面：把技能那几页和设置里的旧块照 Pith 重画；文献库表格的虚拟滚动。
