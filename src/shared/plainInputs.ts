// macOS 的自动更正、自动大写、拼写检查、写作建议、自动填充会悄悄改写输入框里的字
// （实测：搜索框输入 risk 变成 Risk），它弹出的更正提示还会吞掉紧接着的一两次点击。
// 文献库里的作者名、标题、DOI 必须原样保存，所以 App 里所有文本框一律关掉这些功能。
// spellcheck / writingsuggestions 会从祖先继承，写在 index.html 的 <html> 上；其余逐个加。

const OFF: [string, string][] = [
  ["autocomplete", "off"],
  ["autocorrect", "off"],
  ["autocapitalize", "off"],
  ["spellcheck", "false"],
  ["writingsuggestions", "false"],
];

const NON_TEXT = new Set(["checkbox", "radio", "button", "submit", "reset", "file", "color", "range", "hidden", "image"]);

function quiet(el: Element) {
  const text = el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && !NON_TEXT.has(el.type));
  if (!text) return;
  for (const [k, v] of OFF) if (!el.hasAttribute(k)) el.setAttribute(k, v);
}

function scan(node: Node) {
  if (!(node instanceof Element)) return;
  quiet(node);
  node.querySelectorAll("input, textarea").forEach(quiet);
}

/** 启动时调用一次：现在和以后出现的文本框都关掉系统的输入辅助 */
export function installPlainInputs(root: HTMLElement = document.body) {
  scan(root);
  new MutationObserver((records) => {
    for (const r of records) r.addedNodes.forEach(scan);
  }).observe(root, { childList: true, subtree: true });
}
