# HANDOFF — 交接记录

> 规则：每轮开发结束必须更新本文件（`CLAUDE.md` 规则 1）。只写当前状态，历史看 git。

## 现在：1.1.0 验收（2026-11-27），正式版重装成验收后的 1.1.0

你验收 1.1.0 时提的六条，都在 `decisions/0041`：

1. **模型设置**（不分便宜 / 均衡 / 强）：
   - 设置 › 模型与费用「默认模型」两行：**对话**、**读整章写长东西**（子代理，默认「自动：这一家最强的」）。都是两层的选框（左边一家、右边它的模型），弹出层贴着按钮、放不下往上翻、收在窗口里、名字换行不截断。
   - 「各家」每家一份模型清单。每个模型写着「一直想 / 能关 · 几档」、现在的设置（「想 · 中」）、价；「自定义」点开改想不想、想多深（立刻存，「恢复默认」回去）。对话、子代理、联网借的模型都照它。
   - 每家「刷新」问接口现在有哪些模型（不花钱）。Anthropic 的接口直接报思考能力；OpenAI 用一个必被拒收的请求读出每个模型认的档位（拒收不计费）；DeepSeek、Google 按同系列推断。新的放在「还有 N 个」里，标「接口报的 / 推断的」，没价的按最贵的估。核下来 **Haiku 5.5、Sonnet 5.5 的思考其实能关**，以前当成关不掉。自带清单加了 Fable 5.2、GPT-6.1 Terra、Gemini 4 Argon。
   - 输入框下面只换模型（两层选框），旁边显示「想 · 中」；你原来存的「一家 + 一档」自动换成那一档的模型。
2. **Google 免费额度**：429 是免费层的上限——**Gemini 3.8 Flash 每天 20 次**（每个模型分开算，问一句要调好几次）。现在不再白白重试，直接说「免费额度用完了，约 X 小时后恢复」。免费层**不给 Google 搜索**，联网加了备用：自带的搜不了就借 Claude 网页搜索（或 Tavily）再搜。设置里 Google、Vertex 有「用免费额度」，勾上账本记 0、不占月上限。
3. **Vertex**：你登录过了，但项目 `gen-lang-client-0389669195` **还没打开 Vertex AI API**（403 SERVICE_DISABLED），一个请求都发不出去，那一轮没跑成。另外修了：gcloud 装在 `.zshrc` 加的目录里，正式版看不到（「没找到 gcloud」），现在会读终端的 PATH。
4. **书架 → 暂存箱**：是的，它就是暂存的地方（1.1.0 升级时把还没种进任何库的书放进去，只为了能检索）。改名「暂存箱」，而且**放进别的知识库的书会自动从暂存箱里拿出来**，留下的就是还没归好的。你自己建的同名库不会被改。
5. **重排**：有，是本机的 Qwen3-Reranker-0.6B（`~/Documents/LLM`，127.0.0.1:8082，用到才启动）。检索先按向量和按词各捞一批，再用它排序；它起不来就只按向量相似度排并说一声。不用云端重排、不花钱。

### 正式版里验了什么（computer use，2026-11-27 14:53）

`pnpm app:install` 重装了 1.1.0（会先退出正在开的 Pith）。在正式版里点过：

- 输入框下面：「DeepSeek V4.1 Flash 想 · 深」——你原来的「DeepSeek · 均衡」自动换成了对的模型；点开两层选框正常（短的清单弹出层也短、贴着按钮往上翻）。
- 设置 › 模型与费用：默认模型两行、各家清单都在；Vertex 那行「找到了 gcloud · 登录过了」（以前是没找到）；Vertex「刷新」把没开 API 的原因和命令写出来了。**替你勾了 Google 的「用免费额度」**（你说那把 Key 是免费额度；不对就取消勾）。
- 知识库：「书架」已经改名「暂存箱」（3 份）。
- **Anthropic「刷新」报 401「invalid x-api-key」**：同样的请求用你密钥文件夹里那把是 200，所以是**钥匙串里存的 Anthropic 密钥不对了**（多半是旧的）。
- OpenAI「刷新」时 macOS 弹出钥匙串的询问（新装的 Pith 第一次读这把密钥），要你输登录密码、点「始终允许」——那个窗口我看不到也不该点，就停在那里。DeepSeek、真问一句的测试都要读钥匙串，所以没往下走。

### 改的文件

- 新：`src-tauri/src/gateway/discover.rs`（刷新清单、读接口的能力）、`src/chat/ModelPicker.tsx`（两层选框）、`docs/decisions/0041-models-per-model-thinking.md`
- Rust：`gateway/{catalog,commands,anthropic,openai,gemini,http,meter,vertex,mod,tests}.rs`、`chat/{run,acceptance}.rs`、`web/{mod,tests}.rs`（备用搜索）、`kb/{store,tests}.rs`（暂存箱）、`lib.rs`（三个新命令）
- 前端：`settings/ModelSettings.tsx`、`chat/Usage.tsx`、`shared/ipc.ts`、`dev/mockModel.ts`、`styles/app.css`（拿掉三档的样式）
- 文档：`ROADMAP.md`（验收清单）、`architecture.md`、`testing.md`、`requirements.md`、`decisions/0036`（状态）、决策索引

### 测试

- `pnpm check` 全过（Rust 主库 350 个，新增：清单和思考设置、刷新的解析、关思考的写法、免费额度记 0、免费层一天的额度不重试、备用搜索、暂存箱）。
- `?mock` 里走过：两层选框（窄窗口里收在窗口内、长清单在框里滚）、设置页自定义（改成「不想」、恢复默认）。
- 补跑真实验收（`local/acceptance-1.1.0/rounds-6…8-*.log`）：Google 问书 5/5 编号对上；Google 联网第一次被免费层拒（这就是加备用搜索的原因），加了以后借 Claude 搜索、2/2 对上。一共 $0.007（Google 那部分实际免费）。另外刷新清单时探测 OpenAI 的档位，都是被拒收的请求，不计费。

提交：
- `v1.1.0: per-model thinking settings, model picker, inbox, free tier`
- `v1.1.0: acceptance notes`（这份 HANDOFF）

## 等你

1. **钥匙串**：屏幕上那个钥匙串询问输登录密码、点「始终允许」（每把密钥第一次会问一次）。
2. **Anthropic 密钥**：设置 › 模型与费用 › Anthropic「换」，粘你现在用的那把，再点「刷新」应该就不报 401 了。
3. **Vertex**：要用的话，先给项目打开 Vertex AI API（会改你的 Google Cloud 项目，所以我没动）：
   ```bash
   gcloud services enable aiplatform.googleapis.com --project gen-lang-client-0389669195
   ```
   Vertex 一般还要项目开了计费。打开以后告诉我，我补跑 Vertex 那一轮（`PITH_VERTEX_PROJECT=… PITH_ROUNDS=vertex_lite,web_vertex`，几分钱）。
4. **推送**：Pith `main`（第 3、4、5 期和这次验收的两个提交）。
5. 正式版里点一遍 `ROADMAP.md`「用户验收清单」里 1.1.0-验收 那几行；英文版《国富论》重新解析一次（1.1.0-5，修页码）还没做的话也做一下。

## 已知的、没做的

- 回答里的 `###`、`####` 小标题、`---` 分隔线，轻量 Markdown 还没认（Gemini 爱写）。
- Google 的「用免费额度」只管对话；Gemini 的 Google 搜索在免费层本来就搜不了，会借别家的。
- 输入框下面换的是默认模型，还不能每段对话各选各的。
- 两层选框是在 Pith 里用方案 `pith` 现有的变量和动效拼的，还没进 Atelier 的 Design System（待批）。
- 子代理只有「读」一种、不能再派；后台任务、组队、插件库在后面的版本。
- 1.0.5 那些照旧：卡片滑的时候宽度一下变；洇开的圈里是静的；文献库表格没虚拟滚动；技能页、导入页、设置旧块的样子没重画。

## 下一轮

先把上面「等你」1–3 做了（尤其 Vertex 要不要开）。之后 1.1.0 就算验收完；下一版是 **1.2.0 PDF 交给 engine-pdf**（`0034`）：开工前先问开发程度；还要问你一件——PDF 的标注写进文件、另存一份，还是照现在留在库里；engine-pdf 的 tag 要先推到 GitHub。
