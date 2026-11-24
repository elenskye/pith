// 界面用的中文名。Zotero 类型名保持原样存库，这里只管显示；没列到的显示原名。

const ITEM_TYPES: Record<string, string> = {
  journalArticle: "期刊论文",
  book: "图书",
  bookSection: "图书章节",
  thesis: "学位论文",
  conferencePaper: "会议论文",
  report: "报告",
  preprint: "预印本",
  webpage: "网页",
  newspaperArticle: "报纸文章",
  magazineArticle: "杂志文章",
  document: "文档",
  dataset: "数据集",
  manuscript: "手稿",
  patent: "专利",
  standard: "标准",
  encyclopediaArticle: "百科条目",
  dictionaryEntry: "词典条目",
  blogPost: "博客文章",
  presentation: "演示",
  statute: "法规",
  case: "案例",
  interview: "访谈",
  film: "影片",
  computerProgram: "软件",
};

const ROLES: Record<string, string> = {
  author: "作者",
  editor: "编者",
  translator: "译者",
  contributor: "贡献者",
  bookAuthor: "原书作者",
  seriesEditor: "丛书编者",
  reviewedAuthor: "被评作者",
  director: "导演",
  interviewee: "受访者",
  interviewer: "采访者",
};

export const itemTypeLabel = (t: string) => ITEM_TYPES[t] ?? t;
export const roleLabel = (r: string) => ROLES[r] ?? r;

export function backupLabel(label: string): string {
  const v = /^before-v(\d+)$/.exec(label);
  if (v) return `升级到第 ${v[1]} 版前`;
  return { daily: "每日自动", manual: "手动", "before-import": "导入前", "before-restore": "恢复前", "before-delete": "永久删除前", "before-metadata": "联网补全前" }[label] ?? label;
}

export function creatorsSummary(first: string | null, count: number): string {
  if (!first) return "";
  return count > 1 ? `${first} 等` : first;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const ATTACHMENT_KINDS: Record<string, string> = {
  stored: "存在本库",
  linked_file: "链接文件（已复制进本库）",
  linked_url: "网址链接",
  embedded_image: "笔记内嵌图片",
};
export const attachmentKindLabel = (k: string) => ATTACHMENT_KINDS[k] ?? k;

/** MIME 类型 → 简短的中文名 */
export function contentTypeLabel(t: string | null): string {
  if (!t) return "附件";
  if (t === "application/pdf") return "PDF";
  if (t === "text/html" || t === "application/xhtml+xml") return "网页快照";
  if (t.startsWith("image/")) return "图片";
  if (t === "application/epub+zip") return "EPUB";
  return t;
}

const SOURCES: Record<string, string> = { zotero: "Zotero", bibtex: "BibTeX", ris: "RIS", "csl-json": "CSL JSON", manual: "手动", lookup: "DOI / ISBN 联网" };
export const sourceLabel = (s: string | null) => (s ? (SOURCES[s] ?? s) : "");

/** 库里的 UTC 时间 "YYYY-MM-DDTHH:MM:SS.000Z" → 本地 "YYYY-MM-DD HH:MM" */
export function formatTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 编辑时可选的文献类型（常用的在前） */
export const ITEM_TYPE_OPTIONS = Object.keys(ITEM_TYPES);
/** 新建时菜单里的类型 */
export const NEW_ITEM_TYPES = ["journalArticle", "book", "bookSection", "thesis", "conferencePaper", "report", "webpage", "document"];
/** 编辑作者时可选的角色 */
export const ROLE_OPTIONS = Object.keys(ROLES);

const CONTAINERS: Record<string, string> = {
  publicationTitle: "期刊",
  bookTitle: "书名",
  proceedingsTitle: "会议论文集",
  websiteTitle: "网站",
  blogTitle: "博客",
  forumTitle: "论坛",
  encyclopediaTitle: "百科全书",
  dictionaryTitle: "词典",
  programTitle: "节目",
  seriesTitle: "系列",
  reporter: "判例汇编",
  code: "法典",
};
export const containerLabel = (field: string | null) => (field ? (CONTAINERS[field] ?? field) : "期刊 / 书名");

/** Zotero 字段名的中文名（「其他字段」里显示）；没列到的显示原名 */
const FIELDS: Record<string, string> = {
  title: "标题",
  abstractNote: "摘要",
  date: "日期",
  volume: "卷",
  issue: "期",
  pages: "页码",
  numPages: "总页数",
  edition: "版次",
  series: "丛书",
  seriesNumber: "丛书编号",
  seriesTitle: "系列标题",
  publisher: "出版者",
  place: "出版地",
  university: "学校",
  institution: "机构",
  thesisType: "学位类型",
  reportType: "报告类型",
  reportNumber: "报告编号",
  conferenceName: "会议名称",
  ISSN: "ISSN",
  url: "网址",
  accessDate: "访问时间",
  shortTitle: "短标题",
  journalAbbreviation: "期刊缩写",
  libraryCatalog: "来源数据库",
  archive: "档案馆",
  archiveLocation: "档案位置",
  callNumber: "索书号",
  rights: "版权",
  extra: "其他信息",
  citationKey: "引用键",
  repository: "仓储",
  archiveID: "存档编号",
  websiteType: "网站类型",
  versionNumber: "版本",
  medium: "媒介",
  numberOfVolumes: "总卷数",
  language: "语言",
  DOI: "DOI",
  ISBN: "ISBN",
};
export const fieldLabel = (f: string) => {
  if (f === "creators") return "作者";
  if (f === "itemType") return "类型";
  const m = /^(bibtex|ris|csl):(.+)$/.exec(f);
  if (m) return `${m[2]}（${m[1] === "csl" ? "CSL" : m[1] === "ris" ? "RIS" : "BibTeX"} 原字段）`;
  return FIELDS[f] ?? f;
};
/** 「添加字段」里可选的常用字段 */
export const ADDABLE_FIELDS = ["volume", "issue", "pages", "publisher", "place", "edition", "series", "url", "accessDate", "ISSN", "shortTitle", "numPages", "university", "institution", "reportNumber", "conferenceName", "callNumber"];

const SORTS: Record<string, string> = { added: "加入时间", title: "标题", creator: "作者", year: "年份", type: "类型", relevance: "相关度" };
export const sortLabel = (s: string) => SORTS[s] ?? s;

/** 永久删除 / 清空回收站的确认说明（decisions/0013：删除前备份，附件文件留到没有备份用到） */
export const DELETE_FOREVER_NOTE = "会从回收站里彻底去掉。删除前自动备份数据库，后悔了可以在「设置 → 备份」里恢复；附件文件等到没有任何备份用到时才从磁盘删除。";
