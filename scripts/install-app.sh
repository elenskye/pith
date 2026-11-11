#!/usr/bin/env bash
# Build the release .app and install it to /Applications (replacing the old one).
# The installed app is the only one that uses the real data dir (~/Pith); its own startup
# migration backs up data first.
set -euo pipefail
cd "$(dirname "$0")/.."

# Signed with the fixed self-signed certificate (tauri.conf.json bundle.macOS.signingIdentity) so
# macOS sees every update as the same app: the keychain item (the API key) stays readable.
# The certificate does not have to be trusted for this (hence no -v): signing works and the app's
# designated requirement is "identifier + this certificate" either way.
if ! security find-identity -p codesigning | grep -q "Pith Self-Signed"; then
  echo "缺少签名证书 Pith Self-Signed（钥匙串访问 → 证书助理 → 创建证书：自签名根证书、代码签名，见 docs/architecture.md）" >&2
  exit 1
fi

bash scripts/clean-build.sh --auto
pnpm tauri build --bundles app

SRC="target/release/bundle/macos/Pith.app"
DST="/Applications/Pith.app"
RUNNING="$DST/Contents/MacOS/"

if pgrep -f "$RUNNING" >/dev/null; then
  osascript -e "quit app \"$DST\"" >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    pgrep -f "$RUNNING" >/dev/null || break
    sleep 0.5
  done
  if pgrep -f "$RUNNING" >/dev/null; then
    echo "「应用程序」里的 Pith 还在运行，请先退出它再装" >&2
    exit 1
  fi
fi
# Move (not copy): leaving the build output behind makes macOS list a second, real-data-using app.
if [ -e "$DST" ]; then
  OLD="$(mktemp -d)/Pith.app"
  mv "$DST" "$OLD"
fi
mv "$SRC" "$DST"
codesign --verify --strict "$DST"
echo "已安装：$DST"
