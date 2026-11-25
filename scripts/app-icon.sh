#!/usr/bin/env bash
# App 图标：从同步来的 src/vendor/pith/marks/logo/app-icon@1024.png 生成 src-tauri/icons/（pnpm app:icon）。
# 方案里的图标主体占画布的 90%（approved 的样子，不改）；macOS 的图标网格主体占 824/1024，
# 所以先缩到 916（916 × 0.9 ≈ 824），四周补透明到 1024，存成 source.png，再用 tauri icon 出各个尺寸。
# 只换 tauri.conf.json 用到的那几个文件。每次同步 pith 方案、App 图标变了就跑一次。
set -euo pipefail
cd "$(dirname "$0")/.."
src=src/vendor/pith/marks/logo/app-icon@1024.png
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sips -z 916 916 "$src" --out "$tmp/916.png" >/dev/null
sips -p 1024 1024 "$tmp/916.png" --out src-tauri/icons/source.png >/dev/null
pnpm tauri icon src-tauri/icons/source.png -o "$tmp/out" >/dev/null
for f in 32x32.png 128x128.png 128x128@2x.png icon.icns icon.png; do
  cp "$tmp/out/$f" src-tauri/icons/
done
echo "App 图标：从 $src 生成，src-tauri/icons/ 换好了"
