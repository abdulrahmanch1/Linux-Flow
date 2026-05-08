#!/usr/bin/env bash
# Fast check — runs on every .rs file save.
set -euo pipefail

INPUT=$(cat)
FILE=$(jq -r '.tool_input.file_path // empty' <<< "$INPUT")

# Bug fix 1: استبدال case/glob بفحص string بسيط يعمل بشكل موثوق
if [[ "$FILE" != *"/linux-flow/"* ]] || [[ "$FILE" != *.rs ]]; then
    exit 0
fi

CARGO=~/.cargo/bin/cargo
ROOT=/home/abdulrahmanchibon/Hani-pro/linux-flow

cd "$ROOT"

# Bug fix 2: لا plain text مع JSON — نفصلهم
# stderr للـ developer terminal، stdout محجوز لـ JSON systemMessage فقط
$CARGO fmt --all >&2
$CARGO clippy --workspace -- -D warnings >&2

# إشعار Claude بوجود sentinel للـ Stop hook (bug fix 4)
touch /tmp/linux-flow-rs-modified

# Bug fix 2: systemMessage فقط في stdout وبشكل منفصل
LINES=$(wc -l < "$FILE")
if (( LINES > 250 )); then
    printf '{"systemMessage": "🚨 %s = %d lines — hard cap exceeded. Split unless data table, state machine, or unsafe invariant block."}\n' \
        "$(basename "$FILE")" "$LINES"
elif (( LINES > 150 )); then
    printf '{"systemMessage": "⚠ %s = %d lines — exceeds 150-line target. Split if possible, or justify in commit message."}\n' \
        "$(basename "$FILE")" "$LINES"
fi
