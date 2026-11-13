// Rust 命令（src-tauri/src/lib.rs）的类型化封装。界面只经这里调用后端，不碰 SQL；业务规则只写在 Rust。
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type AppInfo = {
  appVersion: string;
  isDev: boolean;
  dataDir: string;
  schemaVersion: number | null;
  latestSchema: number;
  startupError: string | null;
};

export type Overview = {
  items: number;
  trash: number;
  collections: number;
  tags: number;
  attachments: number;
  notes: number;
  missingFiles: number;
  /** 不在任何集合里的文献 */
  unfiled: number;
  /** 可能重复的组数 */
  duplicates: number;
  /** 联网补全后有待确认改动的文献 */
  review: number;
};

/** 列表看哪一部分 */
export type Scope =
  | { kind: "all" }
  | { kind: "unfiled" }
  | { kind: "collection"; id: number; includeChildren?: boolean }
  | { kind: "trash" }
  | { kind: "missing" }
  | { kind: "duplicates" }
  | { kind: "review" };

/** sort：added（回收站里是删除时间）| title | creator | year | type | relevance；不填 = 搜索时按相关度，否则加入时间新的在前 */
export type ListQuery = { scope: Scope; q?: string; tags?: string[]; itemType?: string | null; sort?: string | null; desc?: boolean };

/** 列表里的一行：文献，或独立附件 / 笔记；回收站里还有集合。 */
export type RowKind = "item" | "attachment" | "note" | "collection";

export type ListRow = {
  kind: RowKind;
  id: number;
  uid: string;
  /** 文献类型（Zotero 类型名）；附件为 MIME 类型 */
  itemType: string | null;
  title: string | null;
  firstCreator: string | null;
  creatorCount: number;
  year: number | null;
  containerTitle: string | null;
  createdAt: string;
  deletedAt: string | null;
  attachments: number;
  notes: number;
  missingFiles: number;
  /** 「可能重复」视图里的组号与原因 */
  group?: number;
  groupReason?: string;
};

export type Creator = { role: string; name: string; family: string | null; given: string | null; literal: string | null };
export type NamedRef = { id: number; name: string };
export type FieldValue = { name: string; value: string };
export type TagCount = { id: number; name: string; count: number };

/** kind：stored（存在本库）| linked_file | linked_url | embedded_image；status：ok | missing */
/** readable：能在 App 的阅读器里打开（PDF 且文件在） */
export type ChildAttachment = {
  id: number;
  title: string | null;
  kind: string;
  contentType: string | null;
  status: string;
  size: number | null;
  url: string | null;
  readable: boolean;
  annotationCount: number;
};
export type ChildNote = { id: number; title: string | null; preview: string };

export type ItemDetail = {
  id: number;
  uid: string;
  itemType: string;
  title: string | null;
  creators: Creator[];
  /** 日期原文 */
  date: string | null;
  year: number | null;
  containerTitle: string | null;
  /** 「期刊 / 书名」用的字段；为 null 时这种类型没有这一栏 */
  containerField: string | null;
  doi: string | null;
  isbn: string | null;
  language: string | null;
  abstract: string | null;
  extra: string | null;
  /** 常用字段以外的全部字段 */
  otherFields: FieldValue[];
  tags: string[];
  collections: NamedRef[];
  attachments: ChildAttachment[];
  notes: ChildNote[];
  source: string | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
};

export type Parent = { kind: "item" | "note"; id: number; title: string | null };

export type AttachmentDetail = {
  id: number;
  uid: string;
  kind: string;
  title: string | null;
  originalName: string | null;
  originalPath: string | null;
  url: string | null;
  contentType: string | null;
  status: string;
  size: number | null;
  sha256: string | null;
  parent: Parent | null;
  tags: string[];
  collections: NamedRef[];
  note: string | null;
  annotationCount: number;
  readable: boolean;
  createdAt: string;
  deletedAt: string | null;
};

export type NoteDetail = {
  id: number;
  uid: string;
  title: string | null;
  text: string;
  parent: Parent | null;
  tags: string[];
  collections: NamedRef[];
  imageCount: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
};

export type CollectionDetail = { id: number; name: string; parentName: string | null; children: number; members: number; createdAt: string; deletedAt: string | null };

export type CollectionNode = { id: number; parentId: number | null; name: string; itemCount: number };

/** label：daily | manual | before-import | before-restore | before-v<N>。createdAt：本地时间 "YYYY-MM-DD HH:MM:SS"。 */
export type BackupInfo = { fileName: string; label: string; createdAt: string; size: number };

// ---- 整库导出与恢复（decisions/0013） ----

/** 各表的行数（含回收站里的）；files / bytes：附件文件个数与总大小 */
export type LibraryCounts = { items: number; attachments: number; notes: number; annotations: number; collections: number; tags: number; files: number; bytes: number };

/** checks：自检，库里（source）↔ 按导出重建的库（stored），逐表 */
export type ExportReport = { path: string; counts: LibraryCounts; checks: Check[]; missingFiles: string[] };

/** exportedAt：UTC ISO；missingFiles：导出里没有、文件库里也没有的附件文件个数 */
export type ExportPreview = { path: string; exportedAt: string; appVersion: string; schemaVersion: number; counts: LibraryCounts; missingFiles: number };

/** checks：从导出恢复时逐表核对，导出（source）↔ 库里（stored）；从备份恢复为空。
 *  safetyBackup：恢复前自动做的备份；missingFiles：恢复后库里记着、文件库里没有的附件文件 */
export type RestoreReport = {
  source: string;
  safetyBackup: string | null;
  fromSchema: number;
  toSchema: number;
  counts: LibraryCounts;
  checks: Check[];
  filesCopied: number;
  missingFiles: number;
};

// ---- 导入 ----

export type FileIssue = { key: string | null; title: string | null; parentTitle: string | null; reason: string; inTrash: boolean };

export type ZoteroPreview = {
  dir: string;
  clientVersion: string | null;
  userdataVersion: number | null;
  versionTested: boolean;
  items: number;
  attachments: number;
  notes: number;
  annotations: number;
  collections: number;
  tags: number;
  trashItems: number;
  trashCollections: number;
  groupLibraries: number;
  files: number;
  bytes: number;
  missingFiles: FileIssue[];
  baseRelative: number;
  alreadyImported: number;
  zoteroRunning: boolean;
  warnings: string[];
  blocked: string | null;
};

export type FilePreview = {
  path: string;
  fileName: string;
  /** bibtex | ris | csl-json */
  format: string;
  records: number;
  byType: { itemType: string; count: number }[];
  duplicates: { index: number; title: string | null; reason: string }[];
  samples: string[];
  notes: number;
  warnings: string[];
};

export type ImportStats = {
  items: number;
  attachments: number;
  notes: number;
  annotations: number;
  collections: number;
  newTags: number;
  filesCopied: number;
  filesReused: number;
  bytesCopied: number;
  missingFiles: number;
  trash: number;
  skipped: number;
};

export type Check = { category: string; label: string; source: number; stored: number; ok: boolean; diff?: string[] };

export type ImportReport = {
  checks: Check[];
  sourceChecks: Check[];
  missingFiles: FileIssue[];
  fileWarnings: FileIssue[];
  notMigrated: { label: string; count: number }[];
  skipped: string[];
  warnings: string[];
};

/** source：zotero | bibtex | ris | csl-json */
export type RunSummary = { id: number; source: string; sourcePath: string | null; backupFile: string | null; startedAt: string; finishedAt: string | null; stats: ImportStats };
export type RunDetail = RunSummary & { report: ImportReport };

// ---- 编辑 ----

/** 操作对象：列表里的一行 */
export type Target = { kind: RowKind; id: number };
export type CreatorInput = { role: string; family?: string | null; given?: string | null; literal?: string | null };
/** fields：字段名 → 新值（空字符串 = 删除）；虚拟名 title / date / container 落到这条文献实际用的字段 */
export type ItemPatch = { itemType?: string; fields?: Record<string, string>; creators?: CreatorInput[] };
export type DeleteReport = { records: number; filesRemoved: number; backupFile: string | null };

// ---- 阅读 ----

/** 阅读位置，键与 Zotero 的阅读状态相同；top / left 为 PDF 坐标（pt） */
export type ReadingState = { pageIndex: number; scale?: number | string; top?: number; left?: number; rotation?: number };

/** readonly：不为空 = 只能看不能加标注，值是原因 */
export type ReaderDoc = { id: number; title: string; parent: Parent | null; size: number | null; readonly: string | null; state: ReadingState | null };

/** 位置与 Zotero 同格式：PDF 坐标（pt，原点左下），rects 每个 [x1, y1, x2, y2]；ink 为 paths */
export type AnnotationPosition = { pageIndex: number; rects?: number[][]; paths?: number[][]; width?: number };

export type Annotation = {
  id: number;
  uid: string;
  /** highlight | underline | note | image | ink | text */
  kind: string;
  pageIndex: number;
  pageLabel: string | null;
  position: AnnotationPosition | null;
  sortKey: string;
  text: string | null;
  comment: string | null;
  color: string | null;
  tags: string[];
  authorName: string | null;
  isExternal: boolean;
  editable: boolean;
  createdAt: string;
  updatedAt: string;
};

/** 新建标注：rects 是选区的原始矩形（PDF 坐标，未合并），合并、取整、排序键都由 Rust 做 */
export type NewAnnotation = {
  attachmentId: number;
  kind: "highlight" | "underline" | "note";
  pageIndex: number;
  pageLabel: string | null;
  pageBox: [number, number, number, number];
  rects: [number, number, number, number][];
  text?: string | null;
  comment?: string | null;
  color?: string | null;
  charOffset?: number | null;
};

export type AnnotationPatch = { color?: string; comment?: string; text?: string };

// ---- 联网补全 ----

/** enabled：null = 还没问过用户（第一次用前要先说明联网范围） */
export type MetadataSettings = { enabled: boolean | null; auto: boolean; email: string | null };
export type MetadataSettingsPatch = { enabled?: boolean; auto?: boolean; email?: string };

/** 后台队列的进度；run 每开始一轮加一 */
export type LookupProgress = {
  running: boolean;
  run: number;
  total: number;
  done: number;
  filledItems: number;
  filledFields: number;
  conflictItems: number;
  notFound: number;
  failed: number;
  skipped: number;
  stopped: string | null;
  finishedAt: string | null;
};

/** field：Zotero 字段名，另有 creators / itemType。kind：fill | conflict。
 *  state：applied（已自动填）| pending（待确认）| accepted | ignored | undone | kept（要撤销时已被改过） */
export type LookupChange = { field: string; current: string | null; found: string; kind: "fill" | "conflict"; state: string };

export type LookupView = {
  id: number;
  itemId: number;
  /** doi:… / isbn:… */
  identifier: string;
  /** import | edit | manual | add */
  trigger: string;
  /** found | not_found | failed */
  status: string;
  /** crossref / datacite / openlibrary … */
  source: string | null;
  message: string | null;
  changes: LookupChange[];
  createdAt: string;
  updatedAt: string;
};

export type AddReport = {
  created: number[];
  existing: { identifier: string; itemId: number; title: string | null }[];
  notFound: { identifier: string; message: string }[];
  invalid: string[];
  failed: string | null;
};

// ---------- Word 集成（v0.2，decisions/0016、0017） ----------

export type WordInstallRecord = { version: string; hash: string; template: string | null; installedAt: string };

export type WordInstallStatus = {
  record: WordInstallRecord | null;
  /** 已装的模板就是这个版本的 Pith 生成的 */
  upToDate: boolean;
  scriptInstalled: boolean;
  /** 已装的桥接脚本连的数据目录（开发版与正式版不同） */
  bridgeDataDir: string | null;
  /** 桥接脚本要启动的 Pith 已不在原处，要「更新」 */
  bridgeLaunchMissing: boolean;
  startupDir: string;
  scriptsDir: string;
  /** 写不进 Word 启动文件夹时，等用户手动放的模板 */
  manualTemplate: string | null;
};

export type CiteStyle = {
  id: string;
  /** 中文名；导入的样式是它自己的标题 */
  label: string;
  /** Word 里显示的名字 */
  en: string;
  file: string;
  /** CSL 文件里的 <id> */
  uri: string;
  kind: string;
  /** after-brackets：页码标在方括号外「[2]17」 */
  locator?: string;
  imported: boolean;
  /** 导入的依赖样式：母样式 ID */
  parent?: string;
};

export type ImportedStyle = { style: CiteStyle; replaced: boolean };

/** 置顶搜索窗里的一条文献 */
export type PickInfo = {
  uid: string;
  title: string;
  creators: string;
  year: number | null;
  container: string | null;
  itemType: string | null;
  inLibrary: boolean;
  inTrash: boolean;
};

/** 引文里的一条文献（Rust 的 CiteItemRef）：页码之外的字段（Zotero 转换来的前后缀等）原样带回 */
export type CiteRef = { id: string; locator?: string; label?: string; [key: string]: unknown };

export type PickerSession = { id: number; editing: boolean; items: { cite: CiteRef; info: PickInfo }[] };

export type PickerDone = { kind: "cancel" } | { kind: "remove" } | { kind: "items"; items: CiteRef[] };

export type ZoteroStats = { citations: number; converted: number; items: number; inLibrary: number; fromDocument: number; missing: number; edited: number };

export type ZoteroDocPreview = {
  found: { codes: string[]; bibliography: boolean; bibliographyTexts: string[]; fields: number; zoteroStyle: string | null; noteStyle: boolean; hasPith: boolean };
  stats: ZoteroStats;
  /** Zotero 的样式在 Pith 里对应的样式（中文名）；null：没有对应的 */
  style: string | null;
  out: string;
};

export type TextDiff = { index: number; zotero: string; pith: string };

export type ZoteroConvertReport = {
  stats: ZoteroStats;
  notes: string[];
  bibliography: boolean;
  zoteroStyle: string | null;
  style: string;
  styleNote: string | null;
  left: number;
  sameCitations: number;
  citationDiffs: TextDiff[];
  zoteroEntries: number;
  sameEntries: number;
  entryDiffs: TextDiff[];
};

export type WordStatus = {
  install: WordInstallStatus;
  wordInstalled: boolean;
  servicePort: number | null;
  serviceError: string | null;
  rendererReady: boolean;
  dataDir: string;
  styles: CiteStyle[];
};

export type WordInstallReport = { status: WordInstallStatus; templateError: string | null };

// ---------- 排版 Agent（v0.2 W3，decisions/0018；结构照 src-tauri/src/agent/） ----------

export type AgentTokens = { input: number; output: number; cacheRead: number; cacheWrite: number };

/** 用量账本里的一次模型调用。at：UTC ISO（与库里的时间同格式）；purpose：extract 提炼要求 | plan 文档计划 */
export type AgentCall = {
  at: string;
  purpose: string;
  model: string;
  tokens: AgentTokens;
  costUsd: number;
  /** 中断或出错时按估计记 */
  estimated: boolean;
  /** 模拟模型：不联网、不花钱 */
  mock: boolean;
  note: string | null;
  requestId: string | null;
};

/** keychain：macOS 钥匙串；memory：只在这次运行的内存里；file：PITH_API_KEY_FILE 指的文件（界面不能改） */
export type AgentKeyPlace = "keychain" | "memory" | "file";

export type AgentStatus = {
  mock: boolean;
  hasKey: boolean;
  keyPlace: AgentKeyPlace;
  model: string;
  /** 美元 / 百万 token */
  priceInput: number;
  priceOutput: number;
  budgetUsd: number;
  /** 论文发给模型的方式（默认结构和片段，设置里可换成全文） */
  sendMode: FormatMode;
  /** "2026-11" */
  month: string;
  monthSpentUsd: number;
  /** 最近的调用，新的在前 */
  calls: AgentCall[];
  ledgerError: string | null;
};

/** 发送前的费用估计。typicalUsd：一般情况；maxUsd：最坏情况；withinBudget：最坏情况也不超月度上限 */
export type AgentEstimate = {
  model: string;
  inputTokens: number;
  typicalUsd: number;
  maxUsd: number;
  budgetUsd: number;
  monthSpentUsd: number;
  withinBudget: boolean;
};

/** 模型调用进行中的进度（事件 agent-progress） */
export type AgentProgress = { thinkingChars: number; outputChars: number };

/** label：R1、R2……要求的出处用它指文档 */
export type FormatDocInfo = { label: string; name: string; chars: number };

/** 确定度：明文 / 只在模板 / 推断 / 未规定 */
export type FormatCertainty = "explicit" | "template" | "inferred" | "unspecified";
/** 处理方式：自动应用 / 要改内容或结构（这一版不做）/ 只检查 / 不动 */
export type FormatHandling = "auto" | "structure" | "check" | "keep";

/** verified：摘录在原文里原样找到了（Rust 核对） */
export type FormatSource = { doc: string; section: string; quote: string; verified: boolean };
/** error：值读不懂（不进规范） */
export type FormatSetting = { target: string; prop: string; value: string; error: string | null };
export type FormatSettingPatch = { target: string; prop: string; value: string };
export type FormatCheckKind = "abstract_zh_chars" | "keywords_count" | "references_count" | "caption_by_chapter";
export type FormatCheckSpec = { kind: FormatCheckKind; min: number | null; max: number | null };

export type FormatRequirement = {
  id: string;
  group: string;
  text: string;
  sources: FormatSource[];
  certainty: FormatCertainty;
  handling: FormatHandling;
  note: string;
  settings: FormatSetting[];
  check: FormatCheckSpec | null;
  /** 关掉的「自动」要求不进规范 */
  enabled: boolean;
};

/** 规范里的一个值和它的来历。requirement 为 null = 排版程序的默认做法 */
export type FormatSpecLine = {
  target: string;
  prop: string;
  targetLabel: string;
  propLabel: string;
  value: string;
  /** null = 排版程序的默认做法（规范没规定、但清掉手动格式后必须有确定值的地方） */
  requirement: string | null;
  certainty: FormatCertainty | null;
  /** 默认做法的理由 */
  note: string | null;
};

/** spec：交给排版器的规范（界面不用它的内容） */
export type FormatAssembled = { spec: unknown; lines: FormatSpecLine[]; problems: string[] };

/** brief：结构和片段；full：全文 */
export type FormatMode = "brief" | "full";

/** state：none 还没排 | delivered 已写出 | blocked 保真校验没过 */
export type FormatRunSummary = {
  id: string;
  thesisName: string;
  thesisPath: string;
  createdAt: string;
  mode: FormatMode;
  mock: boolean;
  costUsd: number;
  state: "none" | "delivered" | "blocked";
};

/** docs：文件名；requirements、runs：条数 */
export type FormatRulesetSummary = { id: string; name: string; createdAt: string; mock: boolean; docs: string[]; requirements: number; runs: number };

export type FormatRulesetView = {
  id: string;
  name: string;
  createdAt: string;
  mock: boolean;
  docs: FormatDocInfo[];
  requirements: FormatRequirement[];
  /** 读回模型输出时改正、丢掉的地方 */
  problems: string[];
  call: AgentCall | null;
  assembled: FormatAssembled;
  runs: FormatRunSummary[];
};

/** to / style 的取值：normal、heading 1…heading 4、caption、bibliography */
export type FormatStyleMapping = { from: string; to: string; reason: string; enabled: boolean };
/** maxChars：0 = 不限制 */
export type FormatRule = { pattern: string; maxChars: number; style: string; reason: string; enabled: boolean };
/** section 从 1 开始数 */
export type FormatProtect = { section: number; reason: string; enabled: boolean };
export type FormatObservation = { text: string; paragraphs: number[] };

/** 分界点：段落编号（从 1 起）+ 那一段开头的文字（Rust 用它核对文档没变） */
export type FormatAnchor = { paragraph: number; text: string };
/** 分区：前置部分（摘要）、正文、正文之后各从哪段开始；duplex = 双面印制 */
export type FormatLayout = {
  frontStart: FormatAnchor | null;
  bodyStart: FormatAnchor | null;
  backStart: FormatAnchor | null;
  reason: string;
  duplex: boolean;
  /** 单面 / 双面的默认值是怎么来的 */
  duplexNote: string;
};
/** table：文档里第几个表格（从 1 起） */
export type FormatTableSkip = { table: number; reason: string; enabled: boolean };
/** state：three_line 会排成三线表 | skipped 计划里说不排（可以改）| auto 程序自己跳过（不能改，why 是原因） */
export type FormatTable = { index: number; rows: number; cols: number; paragraph: number; firstRow: string; state: "three_line" | "skipped" | "auto"; why: string | null };

export type FormatDraft = {
  summary: string;
  styleMap: FormatStyleMapping[];
  paragraphRules: FormatRule[];
  protectSections: FormatProtect[];
  layout: FormatLayout;
  skipTables: FormatTableSkip[];
  observations: FormatObservation[];
  /** 模型给的计划里核对不过、被丢掉的项 */
  problems: string[];
};

/** ok：true 符合 | false 不符合 | null 需人工核对 */
export type FormatCheckLine = { requirement: string; text: string; ok: boolean | null; value: string; detail: string };

export type FormatRunResult = {
  appliedAt: string;
  /** 保真校验通过并写出了文件 */
  delivered: boolean;
  outputPath: string | null;
  reportPath: string | null;
  reportMd: string;
  fidelityProblems: number;
  specChecks: number;
  specFailed: number;
  checks: FormatCheckLine[];
};

export type FormatRun = {
  id: string;
  rulesetId: string;
  createdAt: string;
  thesisPath: string;
  thesisName: string;
  thesisSha256: string;
  mode: FormatMode;
  mock: boolean;
  /** 发给模型的论文原文字数 */
  sentChars: number;
  draft: FormatDraft;
  call: AgentCall | null;
  result: FormatRunResult | null;
};

/** 计划会改到的一批段落。how：rule 文字规则 | style 样式映射；from 是小写的样式名 */
export type FormatHit = { label: string; from: string; to: string; how: string; count: number; samples: string[] };
export type FormatSection = { index: number; paragraphs: number; chars: number; opening: string[] };

/** Run 的字段与预览平铺在一起（Rust 的 serde flatten）。thesisProblem：论文读不到 / 变了，预览算不出来的原因 */
export type FormatRunView = FormatRun & {
  hits: FormatHit[];
  sections: FormatSection[];
  styles: string[];
  /** 规范里有要分区处理的要求（页眉、页码、奇数页起）/ 要三线表：界面据此显示「版式」那几块 */
  needsZones: boolean;
  threeLine: boolean;
  /** 可以当分界点的段落（一级标题） */
  anchors: FormatAnchor[];
  tables: FormatTable[];
  thesisProblem: string | null;
};

export type FormatExtractPreview = { ticket: number; docs: FormatDocInfo[]; estimate: AgentEstimate; mock: boolean; hasKey: boolean };

/** totalChars：论文正文总字数；sentChars：这种方式会发出去的字数 */
export type FormatPlanPreview = {
  ticket: number;
  thesisName: string;
  mode: FormatMode;
  paragraphs: number;
  totalChars: number;
  sentChars: number;
  estimate: AgentEstimate;
  mock: boolean;
  hasKey: boolean;
};

// ---------- 事实调查（v0.3，decisions/0020；Rust：src-tauri/src/research/、crates/research-engine） ----------

export type ResearchStage = "plan" | "search" | "download" | "extract" | "verify" | "conclude" | "report";
/** awaiting_plan 计划出来了等确认 | running 后台在做 | cancelled 停下了（可以接着做）| failed 出错停下（可以接着做）| done 做完 */
export type ResearchRunStatus = "awaiting_plan" | "running" | "cancelled" | "failed" | "done";
export type ResearchTier = "high" | "medium" | "low";
export type ResearchProducerKind =
  | "intl_org"
  | "foreign_gov_stats"
  | "academic"
  | "independent_survey"
  | "platform_data"
  | "industry_report"
  | "commercial_media"
  | "cn_gov_stats"
  | "cn_state_media"
  | "self_media"
  | "unknown";
export type ResearchUncertainty = "not_found" | "conflicting" | "outdated" | "unclear_definition" | "low_trust_only";

export type ResearchSubQuestion = {
  id: string;
  text: string;
  metric: string;
  definition: string;
  population: string;
  region: string;
  period: string;
  /** official 官方统计 | alternative 官方统计之外的维度 */
  angle: "official" | "alternative";
  sourceTypes: string[];
  queries: string[];
  enabled: boolean;
};
export type ResearchDimension = { name: string; why: string; sources: string };
export type ResearchPlan = { summary: string; subQuestions: ResearchSubQuestion[]; dimensions: ResearchDimension[]; caveats: string[]; problems: string[] };
export type ResearchHit = { url: string; title: string; snippet: string; published: string | null };
export type ResearchSearch = { subQuestion: string; query: string; engine: string; hits: ResearchHit[]; error: string | null; at: string };

export type ResearchSource = {
  id: string;
  url: string;
  origin: "search" | "manual";
  subQuestions: string[];
  title: string;
  finalUrl: string | null;
  fetchedAt: string | null;
  sha256: string | null;
  kind: string | null;
  contentType: string | null;
  ext: string | null;
  bytes: number;
  textChars: number;
  fetchError: string | null;
  /** 不是本机直接下载到的：claude_web_fetch = 本机下载失败后改用 Claude 网页抓取 */
  fetchedVia: string | null;
  textError: string | null;
  extracted: boolean;
  readChars: number;
  truncated: boolean;
  extractError: string | null;
  publisher: string;
  published: string;
  tier: ResearchTier | null;
  itemId: number | null;
};

export type ResearchFact = {
  id: string;
  source: string;
  subQuestion: string;
  metric: string;
  value: string;
  unit: string;
  region: string;
  population: string;
  period: string;
  definition: string;
  producer: string;
  producerKind: ResearchProducerKind;
  relayed: boolean;
  quote: string;
  unitQuote: string;
  check: { status: "pending" | "verified" | "unverified"; reason: string | null; span: { start: number; end: number } | null; occurrences: number; parsed: unknown };
  tier: ResearchTier;
};

export type ResearchCalc = { id: string; op: "diff" | "ratio" | "pct_change" | "mean" | "sum"; inputs: string[]; label: string; decimals: number; result: string | null; shown: string; unit: string; steps: string; error: string | null };

export type ResearchSegment = { kind: "text"; text: string } | { kind: "fact"; id: string; text: string } | { kind: "calc"; id: string; text: string } | { kind: "unmatched"; text: string; why: string };

export type ResearchConclusion = { id: string; template: string; segments: ResearchSegment[]; facts: string[]; calcs: string[]; uncertainty: ResearchUncertainty[]; note: string; issues: string[] };

export type ResearchStep = {
  kind: ResearchStage;
  label: string;
  at: string;
  provider: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  searches: number;
  costUsd: number;
  mock: boolean;
  ok: boolean;
  note: string | null;
};

export type ResearchModelRef = { provider: string; model: string };

export type ResearchRun = {
  uid: string;
  question: string;
  seq: number;
  previous: string | null;
  createdAt: string;
  updatedAt: string;
  finishedAt: string | null;
  stage: ResearchStage;
  status: ResearchRunStatus;
  error: string | null;
  mock: boolean;
  config: { planner: ResearchModelRef; worker: ResearchModelRef; searchEngine: string; trust: { tiers: Partial<Record<ResearchProducerKind, ResearchTier>> }; limits: Record<string, number> };
  plan: ResearchPlan;
  searches: ResearchSearch[];
  sources: ResearchSource[];
  facts: ResearchFact[];
  calcs: ResearchCalc[];
  conclusions: ResearchConclusion[];
  gaps: string[];
  steps: ResearchStep[];
};

/** state：ok 可读并摘过 | failed 下载失败 | no_text 取不出文字 | pending 还没做到 */
export type ResearchSourceRow = ResearchSource & { tierLabel: string | null; facts: number; verified: number; producers: string[]; state: "ok" | "failed" | "no_text" | "pending" };
export type ResearchFactRow = ResearchFact & { producerKindLabel: string; tierLabel: string; sourceTitle: string; sourceUrl: string };
export type ResearchFactBrief = { id: string; metric: string; region: string; population: string; period: string; producer: string; value: string; source: string };
export type ResearchDiff = {
  previousUid: string;
  previousAt: string;
  changed: { old: ResearchFactBrief; new: ResearchFactBrief }[];
  added: ResearchFactBrief[];
  removed: ResearchFactBrief[];
  unchanged: number;
  sourcesAdded: { id: string; title: string; url: string }[];
  sourcesRemoved: { id: string; title: string; url: string }[];
  conclusionsAdded: string[];
  conclusionsRemoved: string[];
  conclusionsSame: number;
};

export type ResearchReport = {
  uid: string;
  question: string;
  seq: number;
  status: ResearchRunStatus;
  createdAt: string;
  finishedAt: string | null;
  mock: boolean;
  plannerModel: string;
  workerModel: string;
  searchEngine: string;
  conclusions: ResearchConclusion[];
  gaps: string[];
  tableName: string;
  table: { tier: ResearchTier; label: string; kinds: string[] }[];
  tableChanges: string[];
  summary: string;
  subQuestions: ResearchSubQuestion[];
  dimensions: ResearchDimension[];
  caveats: string[];
  planProblems: string[];
  searches: ResearchSearch[];
  sources: ResearchSourceRow[];
  facts: ResearchFactRow[];
  calcs: ResearchCalc[];
  steps: ResearchStep[];
  totals: {
    subQuestions: number;
    searches: number;
    sources: number;
    readable: number;
    downloadFailed: number;
    noText: number;
    facts: number;
    verified: number;
    unverified: number;
    conclusionIssues: number;
    costUsd: number;
    calls: number;
  };
  notes: string[];
  diff: ResearchDiff | null;
};

/** 审计：对存下来的调查从头再核一遍发现的问题 */
export type ResearchFinding = { kind: "quote" | "value" | "unit" | "text" | "tier" | "calc" | "conclusion" | "order"; target: string; message: string };

export type ResearchRunView = { questionId: number; run: ResearchRun; report: ResearchReport; findings: ResearchFinding[]; running: boolean; planEditable: boolean };

export type ResearchRunSummary = {
  uid: string;
  seq: number;
  stage: ResearchStage;
  status: ResearchRunStatus;
  mock: boolean;
  createdAt: string;
  finishedAt: string | null;
  sources: number;
  verifiedFacts: number;
  conclusions: number;
  costUsd: number;
};
export type ResearchQuestion = { id: number; uid: string; text: string; createdAt: string; updatedAt: string; runs: ResearchRunSummary[] };

export type ResearchEstimateLine = { label: string; count: number; typicalUsd: number; maxUsd: number };
export type ResearchEstimate = { typicalUsd: number; maxUsd: number; budgetUsd: number; monthSpentUsd: number; withinBudget: boolean; lines: ResearchEstimateLine[] };

export type ResearchPlanPreview = { ticket: number; question: string; chars: number; provider: string; model: string; webSearches: number; estimate: ResearchEstimate; mock: boolean; missingKeys: string[] };
export type ResearchExecutePreview = {
  ticket: number;
  uid: string;
  stage: ResearchStage;
  subQuestions: number;
  queries: number;
  searchEngine: string;
  maxSources: number;
  manualSources: number;
  workerModel: string;
  plannerModel: string;
  estimate: ResearchEstimate;
  mock: boolean;
  missingKeys: string[];
  /** 最坏情况会超过月度上限：做到一半可能停下 */
  mayStop: boolean;
};

export type ResearchProgress = { uid: string | null; stage: ResearchStage; done: number; total: number; message: string };
export type ResearchDone = { uid: string; status: ResearchRunStatus | null; error: string | null };

export type ResearchKeyWhich = "anthropic" | "deepseek" | "tavily";
export type ResearchEndpoint = { provider: "anthropic" | "openai_compatible"; model: string; baseUrl: string | null };
export type ResearchPrice = { input: number; output: number };
export type ResearchSettings = {
  planner: ResearchEndpoint;
  worker: ResearchEndpoint;
  /** 每个模型接口的专用搜索：claude_web_search | none（用 Tavily） */
  dedicatedSearch: Record<string, string>;
  tavilyDepth: "basic" | "advanced";
  planWebSearches: number;
  trust: { tiers: Partial<Record<ResearchProducerKind, ResearchTier>> };
  prices: { models: Record<string, ResearchPrice>; tavilyCreditUsd: number | null; claudeSearchUsd: number | null };
  /** 本机下载失败时改用 Claude 网页抓取（要 Anthropic 的密钥） */
  webFetchFallback: boolean;
  /** 网页抓取用的模型 */
  fetchModel: string;
};
export type ResearchStatus = {
  mock: boolean;
  settings: ResearchSettings;
  keys: { which: ResearchKeyWhich; label: string; has: boolean; place: AgentKeyPlace; needed: boolean }[];
  searchEngine: string;
  planWebSearches: number;
  prices: { role: "planner" | "worker"; model: string; input: number; output: number; known: boolean }[];
  tavilyCreditUsd: number;
  claudeSearchUsd: number;
  trust: { kind: ResearchProducerKind; label: string; hint: string; tier: ResearchTier; defaultTier: ResearchTier }[];
  trustName: string;
  budgetUsd: number;
  month: string;
  monthSpentUsd: number;
  calls: (AgentCall & { provider: string; kind: string; searches: number })[];
  ledgerError: string | null;
  running: string | null;
};
export type ResearchAdded = { key: string; itemId: number; created: boolean; attachmentId: number | null; attachmentError: string | null };
export type ResearchExportFormat = "markdown" | "html" | "docx";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  revealDataDir: () => invoke<void>("app_reveal_data_dir"),
  overview: () => invoke<Overview>("library_overview"),
  itemsList: (query: ListQuery) => invoke<ListRow[]>("items_list", { query }),
  tagsList: () => invoke<TagCount[]>("tags_list"),
  itemsCreate: (itemType: string, collectionId: number | null) => invoke<number>("items_create", { itemType, collectionId }),
  itemsUpdate: (id: number, patch: ItemPatch) => invoke<void>("items_update", { id, patch }),
  itemsSetField: (ids: number[], field: string, value: string) => invoke<void>("items_set_field", { ids, field, value }),
  itemsMerge: (masterId: number, otherIds: number[]) => invoke<void>("items_merge", { masterId, otherIds }),
  duplicatesDismiss: (ids: number[]) => invoke<void>("duplicates_dismiss", { ids }),
  attachmentsAdd: (itemId: number | null, path: string) => invoke<number>("attachments_add", { itemId, path }),
  attachmentsRename: (id: number, title: string) => invoke<void>("attachments_rename", { id, title }),
  tagsAdd: (targets: Target[], name: string) => invoke<void>("tags_add", { targets, name }),
  tagsRemove: (targets: Target[], name: string) => invoke<void>("tags_remove", { targets, name }),
  tagsRename: (id: number, name: string) => invoke<void>("tags_rename", { id, name }),
  tagsDelete: (id: number) => invoke<void>("tags_delete", { id }),
  collectionsCreate: (name: string, parentId: number | null) => invoke<number>("collections_create", { name, parentId }),
  collectionsRename: (id: number, name: string) => invoke<void>("collections_rename", { id, name }),
  collectionsMove: (id: number, parentId: number | null) => invoke<void>("collections_move", { id, parentId }),
  collectionsAdd: (id: number, targets: Target[]) => invoke<number>("collections_add", { id, targets }),
  collectionsRemove: (id: number, targets: Target[]) => invoke<number>("collections_remove", { id, targets }),
  trashMove: (targets: Target[]) => invoke<void>("trash_move", { targets }),
  trashRestore: (targets: Target[]) => invoke<void>("trash_restore", { targets }),
  trashDelete: (targets: Target[]) => invoke<DeleteReport>("trash_delete", { targets }),
  trashEmpty: () => invoke<DeleteReport>("trash_empty"),
  searchRebuild: () => invoke<number>("search_rebuild"),
  metadataSettingsGet: () => invoke<MetadataSettings>("metadata_settings_get"),
  metadataSettingsSet: (patch: MetadataSettingsPatch) => invoke<MetadataSettings>("metadata_settings_set", { patch }),
  metadataStatus: () => invoke<LookupProgress>("metadata_status"),
  metadataCancel: () => invoke<void>("metadata_cancel"),
  metadataLookup: (ids: number[]) => invoke<number>("metadata_lookup", { ids }),
  metadataLookupRun: (runId: number) => invoke<number>("metadata_lookup_run", { runId }),
  metadataLookupAll: () => invoke<number>("metadata_lookup_all"),
  metadataLatest: (itemId: number) => invoke<LookupView | null>("metadata_latest", { itemId }),
  metadataResolve: (lookupId: number, field: string, accept: boolean) => invoke<LookupView>("metadata_resolve", { lookupId, field, accept }),
  metadataUndo: (lookupId: number) => invoke<{ lookup: LookupView; reverted: number }>("metadata_undo", { lookupId }),
  metadataAdd: (text: string, collectionId: number | null) => invoke<AddReport>("metadata_add", { text, collectionId }),
  itemsGet: (id: number) => invoke<ItemDetail>("items_get", { id }),
  attachmentsGet: (id: number) => invoke<AttachmentDetail>("attachments_get", { id }),
  attachmentsOpen: (id: number) => invoke<void>("attachments_open", { id }),
  notesGet: (id: number) => invoke<NoteDetail>("notes_get", { id }),
  /** 文件库里的文件（rl-file 协议，按附件 id，不暴露本机路径） */
  fileUrl: (id: number) => convertFileSrc(String(id), "rl-file"),
  /** PDF 里的超链接交给系统浏览器（只认 http(s) / mailto） */
  openUrl: (url: string) => invoke<void>("app_open_url", { url }),
  readerGet: (id: number) => invoke<ReaderDoc>("reader_get", { id }),
  readerSaveState: (id: number, reading: ReadingState) => invoke<void>("reader_save_state", { id, reading }),
  annotationsList: (attachmentId: number) => invoke<Annotation[]>("annotations_list", { attachmentId }),
  annotationsCreate: (input: NewAnnotation) => invoke<Annotation>("annotations_create", { input }),
  annotationsUpdate: (id: number, patch: AnnotationPatch) => invoke<Annotation>("annotations_update", { id, patch }),
  annotationsDelete: (ids: number[]) => invoke<void>("annotations_delete", { ids }),
  annotationsRestore: (ids: number[]) => invoke<void>("annotations_restore", { ids }),
  collectionsList: () => invoke<CollectionNode[]>("collections_list"),
  collectionsGet: (id: number) => invoke<CollectionDetail>("collections_get", { id }),
  backupsList: () => invoke<BackupInfo[]>("backups_list"),
  backupsCreate: () => invoke<BackupInfo>("backups_create"),
  backupsRestore: (fileName: string) => invoke<RestoreReport>("backups_restore", { fileName }),
  exportCreate: (dir: string) => invoke<ExportReport>("export_create", { dir }),
  exportReveal: (path: string) => invoke<void>("export_reveal", { path }),
  exportInspect: (dir: string) => invoke<ExportPreview>("export_inspect", { dir }),
  exportRestore: (dir: string) => invoke<RestoreReport>("export_restore", { dir }),
  importZoteroPreview: (dir: string, baseDir: string | null) => invoke<ZoteroPreview>("import_zotero_preview", { dir, baseDir }),
  importZoteroRun: (dir: string, baseDir: string | null) => invoke<RunDetail>("import_zotero_run", { dir, baseDir }),
  importFilePreview: (path: string) => invoke<FilePreview>("import_file_preview", { path }),
  importFileRun: (path: string, skipDuplicates: boolean, intoCollection: boolean) => invoke<RunDetail>("import_file_run", { path, skipDuplicates, intoCollection }),
  importRunsList: () => invoke<RunSummary[]>("import_runs_list"),
  importRunsGet: (id: number) => invoke<RunDetail>("import_runs_get", { id }),
  wordStatus: () => invoke<WordStatus>("word_status"),
  wordInstall: () => invoke<WordInstallReport>("word_install"),
  /** 用户在「打开」对话框里选中了 Word 启动文件夹：把模板写进去 */
  wordInstallInto: (dir: string) => invoke<WordInstallReport>("word_install_into", { dir }),
  wordUninstall: () => invoke<WordInstallStatus>("word_uninstall"),
  /** startup：Word 启动文件夹；manual：等待手动安装的模板 */
  wordReveal: (which: "startup" | "manual") => invoke<void>("word_reveal", { which }),
  wordRevealFile: (path: string) => invoke<void>("word_reveal_file", { path }),
  /** Word 外把 Zotero 引文换成 Pith 引文：预览 / 转换（原文件不动，写到旁边的新文件） */
  wordZoteroInspect: (path: string) => invoke<ZoteroDocPreview>("word_zotero_inspect", { path }),
  wordZoteroConvert: (path: string) => invoke<{ out: string; report: ZoteroConvertReport }>("word_zotero_convert", { path }),
  citeStylesList: () => invoke<CiteStyle[]>("cite_styles_list"),
  citeStyleImport: (path: string) => invoke<ImportedStyle>("cite_style_import", { path }),
  citeStyleRemove: (id: string) => invoke<void>("cite_style_remove", { id }),
  /** 排版 Agent：状态、密钥、月度上限 */
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  agentKeySet: (key: string) => invoke<AgentStatus>("agent_key_set", { key }),
  agentKeyClear: () => invoke<AgentStatus>("agent_key_clear"),
  agentBudgetSet: (usd: number) => invoke<AgentStatus>("agent_budget_set", { usd }),
  agentSendModeSet: (mode: FormatMode) => invoke<AgentStatus>("agent_send_mode_set", { mode }),
  formatRulesetsList: () => invoke<FormatRulesetSummary[]>("format_rulesets_list"),
  formatRulesetGet: (id: string) => invoke<FormatRulesetView>("format_ruleset_get", { id }),
  formatRulesetDelete: (id: string) => invoke<void>("format_ruleset_delete", { id }),
  formatRulesetRename: (id: string, name: string) => invoke<FormatRulesetView>("format_ruleset_rename", { id, name }),
  /** prepare 不联网：读入文件，算出要发什么、大概多少钱；用户确认后拿 ticket 调 run（联网） */
  formatExtractPrepare: (paths: string[]) => invoke<FormatExtractPreview>("format_extract_prepare", { paths }),
  formatExtractRun: (ticket: number) => invoke<FormatRulesetView>("format_extract_run", { ticket }),
  formatRequirementSet: (rulesetId: string, requirementId: string, patch: { enabled?: boolean; settings?: FormatSettingPatch[] }) =>
    invoke<FormatRulesetView>("format_requirement_set", { rulesetId, requirementId, ...patch }),
  /** 发送方式用设置里的默认值；给了 mode 就用给的 */
  formatPlanPrepare: (rulesetId: string, path: string, mode?: FormatMode) => invoke<FormatPlanPreview>("format_plan_prepare", { rulesetId, path, mode }),
  formatPlanRun: (ticket: number) => invoke<FormatRunView>("format_plan_run", { ticket }),
  formatRunGet: (rulesetId: string, runId: string) => invoke<FormatRunView>("format_run_get", { rulesetId, runId }),
  formatRunDelete: (rulesetId: string, runId: string) => invoke<void>("format_run_delete", { rulesetId, runId }),
  /** 改计划（传整个 draft）；之前的排版结果作废 */
  formatPlanUpdate: (rulesetId: string, runId: string, draft: FormatDraft) => invoke<FormatRunView>("format_plan_update", { rulesetId, runId, draft }),
  /** 确定性排版，不联网 */
  formatApply: (rulesetId: string, runId: string) => invoke<FormatRunView>("format_apply", { rulesetId, runId }),
  /** 停止进行中的模型调用 */
  formatCancel: () => invoke<void>("format_cancel"),
  /** 事实调查：设置、密钥 */
  researchStatus: () => invoke<ResearchStatus>("research_status"),
  researchSettingsSet: (settings: ResearchSettings) => invoke<ResearchStatus>("research_settings_set", { settings }),
  researchKeySet: (which: ResearchKeyWhich, key: string) => invoke<ResearchStatus>("research_key_set", { which, key }),
  researchKeyClear: (which: ResearchKeyWhich) => invoke<ResearchStatus>("research_key_clear", { which }),
  researchQuestionsList: () => invoke<ResearchQuestion[]>("research_questions_list"),
  researchQuestionGet: (id: number) => invoke<ResearchQuestion>("research_question_get", { id }),
  /** prepare 不联网：算出要发什么、大概多少钱；用户确认后拿 ticket 调 run（联网） */
  researchPlanPrepare: (question: string) => invoke<ResearchPlanPreview>("research_plan_prepare", { question }),
  researchPlanRun: (ticket: number) => invoke<ResearchRunView>("research_plan_run", { ticket }),
  researchRunGet: (uid: string) => invoke<ResearchRunView>("research_run_get", { uid }),
  researchPlanUpdate: (uid: string, plan: ResearchPlan) => invoke<ResearchRunView>("research_plan_update", { uid, plan }),
  researchSourceAdd: (uid: string, url: string) => invoke<ResearchRunView>("research_source_add", { uid, url }),
  researchSourceRemove: (uid: string, key: string) => invoke<ResearchRunView>("research_source_remove", { uid, key }),
  researchExecutePrepare: (uid: string) => invoke<ResearchExecutePreview>("research_execute_prepare", { uid }),
  /** 在后台做；进度看 research-progress 事件，做完看 research-done */
  researchExecute: (uid: string, ticket: number) => invoke<void>("research_execute", { uid, ticket }),
  researchCancel: () => invoke<void>("research_cancel"),
  researchRerun: (uid: string) => invoke<ResearchRunView>("research_rerun", { uid }),
  researchRunDelete: (uid: string) => invoke<void>("research_run_delete", { uid }),
  researchQuestionDelete: (id: number) => invoke<void>("research_question_delete", { id }),
  /** 写到 dir 下的新文件（不覆盖），返回路径 */
  researchExport: (uid: string, format: ResearchExportFormat, dir: string) => invoke<string>("research_export", { uid, format, dir }),
  researchReveal: (path: string) => invoke<void>("research_reveal", { path }),
  researchAddToLibrary: (uid: string, keys: string[]) => invoke<{ added: ResearchAdded[]; view: ResearchRunView }>("research_add_to_library", { uid, keys }),
  /** 手动加的来源文件 */
  pickResearchFile: async () =>
    (await open({ directory: false, multiple: false, title: "选择要加进来的文件", filters: [{ name: "网页、PDF、表格、文档", extensions: ["pdf", "html", "htm", "xlsx", "xls", "csv", "tsv", "docx", "txt", "md"] }] })) as string | null,
  /** 置顶搜索窗 */
  pickerSession: () => invoke<PickerSession | null>("picker_session"),
  pickerSearch: (q: string) => invoke<PickInfo[]>("picker_search", { q }),
  pickerDone: (session: number, done: PickerDone) => invoke<void>("picker_done", { session, done }),
  /** 系统的选择文件夹对话框；取消返回 null */
  pickFolder: async (title: string, defaultPath?: string) => (await open({ directory: true, multiple: false, title, defaultPath })) as string | null,
  /** 任意文件（添加附件用） */
  pickAnyFile: async (title: string) => (await open({ directory: false, multiple: false, title })) as string | null,
  pickFile: async (title: string) =>
    (await open({ directory: false, multiple: false, title, filters: [{ name: "文献数据（BibTeX、RIS、CSL JSON）", extensions: ["bib", "bibtex", "ris", "json"] }] })) as string | null,
  pickCsl: async () => (await open({ directory: false, multiple: false, title: "选择 CSL 样式文件", filters: [{ name: "CSL 样式", extensions: ["csl", "xml"] }] })) as string | null,
  /** 学校的规范、模板、附件（可多选） */
  pickFormatDocs: async () => {
    const r = await open({ directory: false, multiple: true, title: "选择学校的规范文档（可多选）", filters: [{ name: "规范文档", extensions: ["docx", "doc", "rtf", "txt", "md"] }] });
    return r === null ? [] : Array.isArray(r) ? r : [r];
  },
  pickThesis: async () => (await open({ directory: false, multiple: false, title: "选择要排版的论文", filters: [{ name: "Word 文档", extensions: ["docx"] }] })) as string | null,
  pickDocx: async () => (await open({ directory: false, multiple: false, title: "选择有 Zotero 引文的 Word 文档", filters: [{ name: "Word 文档", extensions: ["docx"] }] })) as string | null,
};
