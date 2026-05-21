#!/bin/bash
# PreToolUse: Check bash commands against ways — thin dispatcher
#
# The ways binary handles: command pattern matching, semantic scoring,
# check curve scoring, session state, and content output.
#
# The command is truncated to its semantic prefix before being passed to
# `ways scan command`. Heredoc bodies (`gh pr create --body "$(cat <<EOF…)"`),
# JSON payloads (`curl -d '{…}'`), and other large argument bodies carry
# no signal for "what kind of command is this" — the program name and
# first few args do. The MiniLM embedding models cap at ~128 tokens of
# position embeddings; queries past that abort the embedder (ggml
# get_rows out-of-range). 256 chars ≈ 60 tokens, safely under the limit
# with headroom for the description that gets appended downstream.
#
# This truncation feeds both the embed query *and* the regex matcher in
# the ways binary. Every existing `commands:` pattern under hooks/ways/
# matches on the program name + first arg (≤106 chars), so cropping at
# 256 changes no current behavior. Future patterns that need to look
# past char 256 of a bash command would be misusing this trigger
# anyway — that signal belongs in `pattern:` against the description.

source "$(dirname "$0")/require-ways.sh"

readonly CMD_QUERY_MAX=256

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
DESC=$(echo "$INPUT" | jq -r '.tool_input.description // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"

CMD_QUERY="${CMD:0:$CMD_QUERY_MAX}"

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
"${HOME}/.claude/bin/ways" scan command \
  --command "$CMD_QUERY" \
  --description "$DESC" \
  --session "$SESSION_ID" \
  --project "$PROJECT_DIR"
