#!/usr/bin/env bash
# 用 Mac Word 把 .docx 导出成 PDF（Word 自己排版：页眉页脚、域、分页都按 Word 的结果），再逐页渲染、列出页眉页脚文字和空白页。
# 用法：bash scripts/word-pdf.sh <文件.docx> <输出目录>
# - 先把文档复制到输出目录再打开：原件不动、锁文件不落在原件旁边。关闭时不保存。
# - 第一次会请求「控制 Microsoft Word」的权限。Word 是沙盒应用：第一次往某个目录写 PDF 时会弹「Grant File Access」，
#   要人点「Select…」授权这个目录一次（Agent 不替用户点），之后同一目录不再问。所以验收时固定用一个输出目录。
#   没人点的话 osascript 会等到超时（300 秒）报 -1712。
# - 输出：<输出目录>/<名字>.pdf、<名字>-pages/pNNN.png、<名字>-pages.txt（每页一行：页眉区、页脚区文字；正文区没有墨迹的标 BLANK-BODY）
set -euo pipefail
src=$1
out=$(cd "$2" && pwd)
name=$(basename "${src%.docx}")
cp "$src" "$out/$name.docx"
open -a "Microsoft Word" "$out/$name.docx"
for _ in $(seq 1 60); do
  [ "$(osascript -e 'tell application "Microsoft Word" to count of documents' 2>/dev/null)" -ge 1 ] && break
  sleep 1
done
osascript -e "with timeout of 300 seconds
tell application \"Microsoft Word\"
save as active document file name \"$out/$name.pdf\" file format format PDF
close active document saving no
end tell
end timeout" >/dev/null
swift "$(dirname "$0")/pdf-pages.swift" "$out/$name.pdf" "$out/$name-pages" > "$out/$name-pages.txt"
head -1 "$out/$name-pages.txt"
grep BLANK "$out/$name-pages.txt" || true
