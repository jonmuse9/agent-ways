#!/usr/bin/env bash
# State-based trigger evaluator — thin dispatcher to ways binary
#
# Evaluates: context-threshold, file-exists, session-start triggers.
# Also handles core guidance re-injection safety net.

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
TRANSCRIPT=$(echo "$INPUT" | jq -r '.transcript_path // empty')
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
HOOK_EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // "SessionStart"')

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"

ARGS=(--session "$SESSION_ID" --project "$PROJECT_DIR" --hook-event "$HOOK_EVENT")
[[ -n "$TRANSCRIPT" ]] && ARGS+=(--transcript "$TRANSCRIPT")

"${HOME}/.claude/bin/ways" scan state "${ARGS[@]}"
