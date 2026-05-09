#!/usr/bin/env bash
# Full check — runs once when Claude finishes its turn.
set -euo pipefail

SENTINEL=/tmp/linux-flow-rs-modified

# Bug fix 4: لا تُشغِّل إذا لم يُعدَّل أي ملف .rs في هذه الجلسة
if [[ ! -f "$SENTINEL" ]]; then
    exit 0
fi
rm -f "$SENTINEL"

CARGO=~/.cargo/bin/cargo
ROOT=/home/abdulrahmanchibon/Hani-pro/linux-flow

cd "$ROOT"

echo "=== forbidden-attrs ===" >&2
BAD=$(grep -rn 'allow(dead_code)' --include='*.rs' . | grep -v 'app/src/main\.rs' || true)
if [[ -n "$BAD" ]]; then
    echo "FORBIDDEN: allow(dead_code) outside app/src/main.rs:" >&2
    echo "$BAD" >&2
    exit 1
fi

echo "=== build ===" >&2
$CARGO build --workspace

echo "=== test ===" >&2
$CARGO test --workspace

# Bug fix 3: cargo doc بدون pipe — الفشل يوقف السلسلة فعلاً
echo "=== doc ===" >&2
$CARGO doc --workspace --no-deps

echo "=== audit ===" >&2
if command -v cargo-audit >/dev/null 2>&1; then
    $CARGO audit
else
    echo "cargo-audit not installed — skipping (run: cargo install cargo-audit)" >&2
fi

echo "=== deny ===" >&2
if command -v cargo-deny >/dev/null 2>&1; then
    $CARGO deny check
else
    echo "cargo-deny not installed — skipping (run: cargo install cargo-deny)" >&2
fi

echo "=== all checks passed ===" >&2
