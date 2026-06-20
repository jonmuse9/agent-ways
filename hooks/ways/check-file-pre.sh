#!/usr/bin/env bash
# PreToolUse: Check file operations against ways — thin dispatcher
#
# The ways binary handles: file pattern matching, check scoring,
# session state, and content output.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
FP=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"

[[ -z "$FP" ]] && exit 0

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
# --opt=value form for consistency with the other scan hooks (binds values that
# could begin with '-' unambiguously).
"${HOME}/.claude/bin/ways" scan file \
  --path="$FP" \
  --session="$SESSION_ID" \
  --project="$PROJECT_DIR"
