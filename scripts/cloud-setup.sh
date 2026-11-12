#!/usr/bin/env bash
# 云端 Claude 的开发环境（CLAUDE.md 规则 7）。
# 用法：把这个文件的全部内容粘贴到 claude.ai/code 云端环境设置的 Setup script 里。
# 云端是 Ubuntu 24.04，脚本以 root 运行，必须以 0 退出。只装系统库和工具：不编译，不碰仓库。
# 哪一步失败都只打印提示、不中断，进会话后由云端 Claude 自己补装。
set -uo pipefail
export DEBIAN_FRONTEND=noninteractive

# Tauri 2 在 Linux 上编译要的系统库（WebKitGTK 等），加上 ureq 的 native-tls 要的 OpenSSL
apt-get update -y || echo "apt-get update 失败"
apt-get install -y --no-install-recommends \
  build-essential pkg-config curl wget file \
  libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  || echo "系统库没装全，进会话后再装"

# Rust：仓库用 edition 2024，Cargo.lock 是新版 cargo 写的
if command -v rustup >/dev/null 2>&1; then
  rustup update stable || echo "rustup update 失败"
  rustup default stable || true
fi

# pnpm 与本地一致（pnpm-lock.yaml 是 pnpm 12 写的）
npm install -g pnpm@12.6.0 || echo "pnpm 没装上"

exit 0
