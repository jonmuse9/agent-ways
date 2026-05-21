#!/bin/bash
# Check user prompts against ways — thin dispatcher to ways binary
#
# The ways binary handles: file walking, frontmatter extraction, pattern
# + semantic matching, scope/precondition gating, parent threshold
# lowering, session markers, macro dispatch, and content output.
#
# UserPromptSubmit doesn't just carry the user's typed message — the
# harness also injects structured content here: <task-notification>
# blobs from completed background agents, <persisted-output> pointers
# for tool results that exceed inline budget, and other system-reminder
# envelopes. Any of those can run multiple KB, and embedding that
# overruns the MiniLM model's position-embedding table (SIGABRT in
# ggml_compute_forward_get_rows). Cap the embed query at 1024 chars
# (~240 tokens — generous because real user prompts can legitimately
# be paragraphs of context, unlike bash commands). Anything past 1024
# in a prompt is system-injected envelope content that carries no
# additional signal for matching the user's *intent* against ways.

source "$(dirname "$0")/require-ways.sh"

readonly PROMPT_QUERY_MAX=1024

INPUT=$(cat)
PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"

# Read response topics from Stop hook (if available).
# Path resolves through the binary so the writer (check-response.sh),
# the consumer (here), and `ways reset` cannot drift.
RESPONSE_STATE=$("${HOME}/.claude/bin/ways" response-topics-path "$SESSION_ID")
RESPONSE_TOPICS=""
if [[ -f "$RESPONSE_STATE" ]]; then
  RESPONSE_TOPICS=$(jq -r '.topics // empty' "$RESPONSE_STATE" 2>/dev/null)
fi

# Combined context: user prompt + Claude's recent topics, capped.
# RESPONSE_TOPICS is bounded by check-response.sh's extraction (~50 chars
# of keywords) so the cap is effectively a guard on PROMPT itself.
COMBINED="${PROMPT} ${RESPONSE_TOPICS}"
COMBINED="${COMBINED:0:$PROMPT_QUERY_MAX}"

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"
"${HOME}/.claude/bin/ways" scan prompt \
  --query "$COMBINED" \
  --session "$SESSION_ID" \
  --project "$PROJECT_DIR"
