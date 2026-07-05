#!/usr/bin/env bash
# PostToolUse hook: 编辑 .rs 文件后对所属 crate 跑 clippy
set -euo pipefail

input=$(</dev/stdin)

case "$input" in
  *'.rs"'*) ;;
  *) exit 0 ;;
esac

file_path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // ""' 2>/dev/null \
  || printf '%s' "$input" | sed -nE 's/.*"file_path"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/p' | head -1)

if [[ -z "$file_path" || "$file_path" != *.rs ]]; then
  exit 0
fi

project_dir="${CLAUDE_PROJECT_DIR:-$(pwd)}"
rel="${file_path#"$project_dir/"}"

if [[ "$rel" != crates/* ]]; then
  exit 0
fi

crate=$(echo "$rel" | awk -F/ '{print $2}')
if [[ -z "$crate" ]]; then
  exit 0
fi

cd "$project_dir"
if ! output=$(cargo clippy -p "mc-$crate" --all-targets -- -D warnings 2>&1); then
  {
    echo "clippy failed for crate '$crate' after editing $rel:"
    echo "$output" | tail -40
  } >&2
  exit 2
fi

exit 0
