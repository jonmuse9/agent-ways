#!/bin/bash
# PreToolUse:Task — thin dispatcher to ways binary
#
# Phase 1 of two-phase subagent injection:
# 1. This script: ways scan task (matches ways, writes stash)
# 2. SubagentStart: inject-subagent.sh (reads stash, emits content)
#
# Skips dispatches to custom-defined agents (project, global, or plugin):
# their .md file IS the agent's constitution, so ways injection would be
# redundant — and their delegation prompts are large enough that embedding
# them against the corpus crashes the embedder on its position-embedding
# limit. The scan/embed exists for generic subagents that lack such a
# constitution (general-purpose, Explore, Plan, etc.).

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
TASK_PROMPT=$(echo "$INPUT" | jq -r '.tool_input.prompt // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
TEAM_NAME=$(echo "$INPUT" | jq -r '.tool_input.team_name // empty')
SUBAGENT_TYPE=$(echo "$INPUT" | jq -r '.tool_input.subagent_type // empty')

[[ -z "$TASK_PROMPT" || -z "$SESSION_ID" ]] && exit 0

# Skip custom agents — they have their own .md constitution.
if [[ -n "$SUBAGENT_TYPE" ]]; then
  if [[ -f "${PROJECT_DIR}/.claude/agents/${SUBAGENT_TYPE}.md" ]] \
     || [[ -f "${HOME}/.claude/agents/${SUBAGENT_TYPE}.md" ]]; then
    exit 0
  fi
  # Plugin-provided agents live under .../plugins/<plugin>/agents/.
  # Glob via bash so we don't pay for a find() walk on every dispatch.
  shopt -s nullglob
  plugin_hits=("${HOME}/.claude/plugins/marketplaces/"*"/plugins/"*"/agents/${SUBAGENT_TYPE}.md")
  shopt -u nullglob
  if (( ${#plugin_hits[@]} > 0 )); then
    exit 0
  fi
fi

ARGS=(--query "$TASK_PROMPT" --session "$SESSION_ID" --project "$PROJECT_DIR")
[[ -n "$TEAM_NAME" ]] && ARGS+=(--team "$TEAM_NAME")

"${HOME}/.claude/bin/ways" scan task "${ARGS[@]}"

# Never block Task creation
exit 0
