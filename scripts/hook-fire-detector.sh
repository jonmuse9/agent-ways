#!/usr/bin/env bash
# Writes a line to a log file every time this hook fires.
# Used to detect whether hooks fire during Monitor-triggered inference passes.
LOG="${TMPDIR:-${TEMP:-${TMP:-/tmp}}}/attend-hook-fire-log.txt"
echo "$(date +%H:%M:%S.%N) hook_event=$CLAUDE_HOOK_EVENT tool=$CLAUDE_TOOL_NAME" >> "$LOG"
