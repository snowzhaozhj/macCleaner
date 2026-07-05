#!/usr/bin/env bash
# PreToolUse hook: main 分支上禁止编辑源代码（docs/CLAUDE.md/.claude/ 除外）
set -euo pipefail

input=$(</dev/stdin)

project_dir="${CLAUDE_PROJECT_DIR:-$(pwd)}"

git_path="$project_dir/.git"
if [[ -f "$git_path" ]]; then
  read -r line < "$git_path"
  head_file="${line#gitdir: }/HEAD"
elif [[ -d "$git_path" ]]; then
  head_file="$git_path/HEAD"
else
  exit 0
fi

[[ -r "$head_file" ]] || exit 0

read -r head_content < "$head_file"
case "$head_content" in
  "ref: refs/heads/main"|"ref: refs/heads/master") ;;
  *) exit 0 ;;
esac

file_path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // ""' 2>/dev/null \
  || printf '%s' "$input" | sed -nE 's/.*"file_path"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/p' | head -1)

abs_root=$(cd "$project_dir" && pwd)
if [[ "$file_path" != "$abs_root"/* ]]; then
  exit 0
fi

rel="${file_path#"$abs_root"/}"

case "$rel" in
  CLAUDE.md|README.md|LICENSE|.claude/*|docs/*)
    exit 0 ;;
esac

cat >&2 <<EOF
BLOCKED: 禁止在 main 分支直接编辑源代码。

文件: $rel

请先创建 feature 分支：
  git checkout -b feat/<slug>
EOF
exit 2
