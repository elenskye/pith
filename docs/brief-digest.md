# Research Lab 项目 Brief（压缩版）

> 原始文件：`Research_Lab_Project_Brief.md` v0.1（2026-11-06）与 `Research_Lab_Agent_Kickoff.md`，已于 2026-11-07 删除，本文保留其精华。原文是初稿，不是定死的规矩；具体方案以问卷回答和 `docs/decisions/` 为准。

## 1. 定位与边界

本地优先、个人所有、以证据为核心的研究工作台。与已有产品的边界：

- **Pomelo**：生活与项目数据管理 → Research Lab 不做待办、记账、生活笔记
- **Visual Assets**：个人视觉语言 → Research Lab 的可视化以知识真实性与清晰度为先，不强制借用
- **Personal Website**：对外发布出口 → Research Lab 只做可选导出，不自动公开
- **Zotero**：只需保留 / 替代其**核心文献管理与 Word 引用能力**，不复刻全部产品与云同步

## 2. 核心原则

1. Local-first，用户拥有数据；云端模型 / 联网 / 上传均需明确提示与范围
2. 先证据后答案：每个重要事实能定位到来源、页码 / 段落、查询时刻；缺证据就说不知道
3. 先短后深：默认一屏内 3–5 条发现，展开才看研究路径与完整报告
4. AI 执行，人作判断：Agent 不得篡改结论、伪造数据、静默覆盖源文件、发表成果
5. 确定性组件优先：CSL 引擎管引用、代码管计算、结构化规则管 Word 样式；LLM 管意图、计划、解释、候选
6. 逐阶段可用：每版本有实际功能和验收样例
7. 开源优先、禁止盲目复制：逐项记录许可证、维护、兼容性、取舍理由

**第一期不做**：Zotero 云同步复刻、多人协作、自动发表、未授权爬取、训练大模型、默认上云、通用 Office、通用 Agent 平台。

## 3. 功能总图

### 3.1 Library（Zotero 核心能力）
导入迁移（Zotero API / 导出格式、RIS / BibTeX / CSL JSON、DOI / ISBN / PMID、PDF / EPUB / Word / 网页存档）；完整元数据与自定义字段；搜索 / 筛选 / 批量编辑 / 去重 / 附件管理；PDF 高亮批注摘录（精确锚点，可跳回源文件页码）；Zotero 迁移首轮只读 + 映射报告 + 回滚；"版本 → 章节 → 小节 → 片段 → 证据锚点"关系。

### 3.2 Word Studio
**Reference Engine**：Word 侧边栏搜索本地文献、插入 / 编辑 / 移除引文、刷新书目；稳定文献 ID + 结构化可更新对象（不得以纯文本替换冒充动态引用）；CSL 至少 APA 7、Chicago、GB/T 7714；与 Zotero 既有 Word 字段的兼容性分别测试，不能安全转换的保留原样。
**Document Agent**：自然语言规范 → Formatting Plan → 预览差异 → 对副本应用；覆盖中西文字体、代码字体、标题、行距段距、表格、脚注尾注、题注、页边距、分节、页码、页眉页脚、目录、交叉引用；"只排版"不改正文实词；"润色"独立开关 + 逐处差异 + 可撤销；不支持的结构先报告不静默删除。
**可行性关卡**：先在真实 Word for Mac 跑通插入 → 保存 → 重开 → 编辑 → 切 CSL → 刷新书目闭环。

### 3.3 三种研究模式
- **Fact Research**：问题重写为可验证子问题；优先官方 / 一手来源；多轮检索、冲突 / 反例检索；默认短答（3–5 条结论 + 数据 + 证据链接 + 不确定性）；区分"找不到 / 冲突 / 过时 / 口径未定义"；保留检索时间与快照
- **Concept Research**：从章节 / 摘录 / 论点出发识别概念、前提、因果、反方；概念卡、论证卡、知识关系图；区分"原文说的 / AI 解释的 / 假设推演的"
- **Academic Research**：项目模板（问题 → 综述 → 假设 → 操作化 → 方法 → 数据 → 统计 → 稳健性 → 论文）；CSV / XLSX、字段字典、可复现脚本（参数、库版本、随机种子）；示例课题"政治效能感影响因素"；模拟数据不得冒充实证

### 3.4 Evidence Engine（共享证据层，最重要的公共数据结构）
`Source`、`Document` / `Fragment`、`Claim`、`EvidenceLink`、`ResearchRun`、`Artifact`。需要关键词 + 语义混合检索、跨语言、去重、精确片段定位、PDF 页码与版本追踪、反向索引"这条结论依赖哪些来源"。

### 3.5 WonderLab
概念卡 / 图表时间轴网络图 / 带参数的交互模型。原著事实与教学参数模型分别标注；交互代码在隔离环境运行；示例《国富论》"分工"章节。

### 3.6 Adaptive Agent
三层记忆：User Preferences / Verified Knowledge / Workflow-Skill Memory。自我改进闭环：观察纠错 → 候选 Skill 改动 → 固定测试集对比 → 低风险自动部署 → 可回滚；权限 / 隐私 / 引用规则 / 核心代码改动需额外审批。Skills 用 `SKILL.md` + 资源 + 版本号 + 测试用例目录。

### 3.7 页面
Home / Library（三栏）/ Research Workspace / Academic Workspace / Word Studio / WonderLab / Settings-Agent Lab。UI 克制、专业、信息密度可控、来源随手可查。

## 4. 建议技术架构（允许验证后调整）

| 层 | 首选 | 说明 |
| --- | --- | --- |
| macOS 桌面 | Tauri 2 + React + TS | 与 Pomelo 独立代码 / 数据库 / 配置 |
| 主数据 | SQLite + FTS5 | 附件独立存储，备份迁移明确 |
| 向量 | sqlite-vec（可选，pre-v1） | 先 FTS5 |
| 计算 | Python 3.11+ Worker 按需启动 | PDF 解析、RAG、数据分析 |
| 工作流 | 轻量状态机，复杂后评估 LangGraph | 可取消 / 恢复 / 重试 |
| LLM | 可替换 provider adapter | 预算硬上限；消费者订阅 ≠ API 额度 |
| 文档引用 | Office.js Add-in + CSL + OOXML / docx 引擎 | Word 内与脱离 Word 分开 |
| 可视化 | Vega-Lite / React Flow / 按需 Three.js | 声明式优先，自包含导出 |
| 机密 | macOS Keychain | 密钥不落 Git |

候选目录：`app/`、`core/`、`workers/`、`word-addin/`、`packages/citation/`、`packages/evidence/`、`skills/`、`tests/fixtures/`、`docs/`。不为提前模块化造复杂 monorepo。资源：M3 / 18 GB，先测 20–100 篇 PDF 的内存 / 延迟 / 磁盘。

## 5. 安全、可信、许可证

原始文件默认只读；所有外部文本不可信（不执行夹带指令）；网络来源存 URL / 访问时间 / 哈希；代码在受限目录运行；版权书籍仅处理合法持有内容；AGPL / GPL / 字体 / CSL 许可证逐项审查，依赖登记 `THIRD_PARTY.md`。需单独授权：删除用户数据、付费超预算、外部 PR、push、部署、购买、改其他仓库或系统设置。

## 6. 阶段与验收

原 Brief 的顺序：Phase 0（Zotero 迁移 / Word 动态引用 / Word 排版保真三项验证）→ v0.1 Library → v0.2 Word Studio → v0.3 Fact Research → v0.4 Academic Research → v0.5 WonderLab → v0.6 Adaptive Agent。现行顺序按用户排名改过（Concept Research 插到 v0.4，Academic 后移到 v0.6），见 `decisions/0005` 与 `ROADMAP.md`。

首批端到端验收场景：真实毕业论文重排；事实研究（如"津京就业指标可否直接比较"）；学术研究（政治效能感）；阅读复习（《国富论》分工章节）；自我改进（两次删长背景段 → Skill patch → 可撤销）。

## 7. 开源候选矩阵（技术候选与参考，不是全部安装或 fork）

| 需求能力 | 候选仓库 | 借用范围与提醒 |
| --- | --- | --- |
| Zotero 数据模型 | zotero/zotero | 数据模型与迁移兼容；AGPLv3 |
| 浏览器元数据采集 | zotero/zotero-connectors、zotero/translators | 网页识别规则 |
| RIS/BibTeX/DOI/ISBN 导入 | zotero/translation-server | Node/Docker，按需启动 |
| 学术 PDF 结构与引文 | grobidOrg/grobid | 资源成本单测 |
| 多格式解析 | docling-project/docling | 先验证中文效果 |
| 本地向量 | asg017/sqlite-vec | pre-v1，可选 |
| Word 内 UI | OfficeDev/Office-Add-in-samples | 官方示例 |
| CSL 处理器 | Juris-M/citeproc-js | 保留动态 citation ID |
| 样式/语言资源 | citation-style-language/styles、/locales | APA/Chicago/GB/T 具体版本待验收 |
| DOCX 离线修改 | python-openxml/python-docx | 非布局引擎 |
| Zotero Word 兼容研究 | zotero/zotero-word-for-mac-integration、-windows- | 仅研究，旧架构 |
| Web 深度调查 | assafelovic/gpt-researcher | 规划采集；UI 重做 |
| 科研证据检索 | Future-House/paper-qa | 文献 RAG、引用片段 |
| 研究提纲 | stanford-oval/storm | 参考方法，不强制长篇 |
| 可恢复工作流 | langchain-ai/langgraph | 复杂时用 |
| 数据与统计 | pandas、statsmodels、scipy | 代码可复现 |
| Notebook | jupyterlab | 导出 / 重跑 |
| 概念关系图 | xyflow/xyflow | 交互节点 |
| 轻量图 | mermaid-js/mermaid | 静态或轻交互 |
| 规范图表 | vega/vega-lite、plotly/plotly.js | 声明式 |
| 3D | mrdoob/three.js | 仅确有价值时 |
| Agent/RAG 回归 | promptfoo/promptfoo | 固定数据集评测 |
| Trace/Skill 版本 | langfuse/langfuse | 功能参考，优先自建轻量 |

官方核验链接：zotero.org/support/dev/source_code、zotero.org/support/dev/citation_styles、learn.microsoft.com/office/dev/add-ins/word/。以上以 2026-11-06 调研为基准，正式依赖需再次审计。
