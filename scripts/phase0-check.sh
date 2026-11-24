#!/bin/bash
# Phase 0 一键自检：跑全部自动测试，再在临时目录走一遍「生成 → 填引文 → 排版 → 校验 → 切样式 → 校验」。
# 只读你的数据：Zotero 副本只在存在时以只读方式测试；不碰 ~/Zotero 与 ~/ResearchLab。
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
tmp="$(mktemp -d -t researchlab-phase0)"
step() { printf '\n== %s\n' "$1"; }

step "0-A Zotero 迁移：合成夹具测试"
cargo test -q -p zotero-import
if [ -d "$HOME/ResearchLab-test/zotero-copy" ]; then
  step "0-A Zotero 迁移：真实副本（只读）"
  ZOTERO_COPY_DIR="$HOME/ResearchLab-test/zotero-copy" cargo test -q -p zotero-import -- --ignored
fi

step "0-B Word 引文：类型检查 + 测试"
(cd poc/word-addin && pnpm install --frozen-lockfile > /dev/null && pnpm check)

step "0-C Word 排版：测试"
cargo test -q -p docx-engine

step "端到端（临时目录 $tmp）"
cargo run -q -p docx-engine -- fixture "$tmp/1-fixture.docx"
node poc/word-addin/docx/rl-docx.ts refresh "$tmp/1-fixture.docx" --out "$tmp/2-cited.docx"
cargo run -q -p docx-engine -- apply "$tmp/2-cited.docx" --spec crates/docx-engine/spec/thesis-sample.json --out "$tmp/3-formatted.docx" --report "$tmp/3-report.md" | head -5
node poc/word-addin/docx/rl-docx.ts verify "$tmp/3-formatted.docx" | head -1
node poc/word-addin/docx/rl-docx.ts refresh "$tmp/3-formatted.docx" --out "$tmp/4-gbt.docx" --style gbt7714-2015-numeric
node poc/word-addin/docx/rl-docx.ts verify "$tmp/4-gbt.docx" | head -1

step "全部通过。可以用 Word 打开 $tmp 里的文件看效果（排版前 2-cited、排版后 3-formatted、切 GB/T 后 4-gbt）"
