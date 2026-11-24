#!/bin/bash
# 安装 / 卸载 Word 宏要用的 AppleScript（只写这一个文件，不动 Word 其他设置）。
#   bash vba/install-word-bridge.sh          安装
#   bash vba/install-word-bridge.sh remove   卸载
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
dest="$HOME/Library/Application Scripts/com.microsoft.Word/ResearchLab.scpt"
if [ "${1:-}" = "remove" ]; then
  rm -f "$dest" && echo "已卸载 $dest"
  exit 0
fi
node_bin="$(command -v node)"
src="$(mktemp -t rl-bridge).applescript"
sed -e "s|@NODE@|$node_bin|" -e "s|@BRIDGE@|$here/docx/rl-bridge.ts|" "$here/vba/ResearchLab.applescript.in" > "$src"
mkdir -p "$(dirname "$dest")"
osacompile -o "$dest" "$src"
rm -f "$src"
echo "已安装 $dest（node：$node_bin）"
