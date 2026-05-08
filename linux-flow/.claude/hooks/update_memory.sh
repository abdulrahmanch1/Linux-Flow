#!/usr/bin/env bash
set -euo pipefail

MEMORY_DIR="$HOME/.claude/projects/-home-abdulrahmanchibon-Hani-pro/memory"
REPO="/home/abdulrahmanchibon/Hani-pro"

cat > "$MEMORY_DIR/task_log.md" <<EOF
---
name: task completion log
description: سجل المهام المنجزة — يُولَّد تلقائياً من git log عند نهاية كل جلسة
type: project
---

## المهام المنجزة (من git log)

$(git -C "$REPO" log --oneline --all \
    | grep -E "feat: complete TASK" \
    | sed 's/^/- /' \
    || echo "- لا توجد tasks مكتملة بعد")

## آخر 5 commits

$(git -C "$REPO" log --oneline -5 | sed 's/^/- /')

## المهمة التالية

$(grep -m1 '^\- \[ \]' "$REPO/TASKS.md" | sed 's/- \[ \] //')
EOF
