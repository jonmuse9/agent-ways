#!/usr/bin/env bash
# Reactive firing for softwaredev/code/quality (ADR-123 Phase D).
#
# Exits 0 (requesting fire) when the just-completed Edit/Write touched
# a file that has grown past 500 lines. This is the metric predicate
# the quality way cares about — files this large are a concrete signal
# that maintenance cost is climbing, exactly the moment the quality
# way's guidance is load-bearing.
#
# Receives the full PostToolUse input JSON on stdin. Non-zero exit =
# "no match, skip me." No output is consumed by the caller, only the
# exit code. Keep side effects nil.

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')

case "$TOOL_NAME" in
  Edit|Write) ;;
  *) exit 1 ;;
esac

FP=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
[[ -z "$FP" ]] && exit 1
[[ -f "$FP" ]] || exit 1

LINES=$(wc -l < "$FP" 2>/dev/null)
[[ -z "$LINES" ]] && exit 1

if (( LINES >= 500 )); then
  exit 0
fi
exit 1
