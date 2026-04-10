#!/bin/bash
# Dynamic context for attend way
# Checks if attend is installed and running, emits live state

# Not installed — nothing to add
if ! command -v attend &>/dev/null; then
  echo "**Note**: attend is not installed. Run \`make attend\` or \`make install\` to build it."
  exit 0
fi

# Check if attend is running
RUNNING=$(ps --no-headers -eo args 2>/dev/null | grep -c "attend run" | grep -v grep)
if [[ "$RUNNING" -gt 0 ]]; then
  echo "**Status**: attend is running"
else
  echo "**Status**: attend is not running — start with \`/attend\` or \`Monitor: attend run\`"
fi

# Show focus state
FOCUS_OUTPUT=$(attend focus list 2>/dev/null)
if [[ -n "$FOCUS_OUTPUT" ]]; then
  echo "**Focus**: $FOCUS_OUTPUT"
fi

# Show peer count
PEER_OUTPUT=$(attend peers 2>&1 | grep "baseline" | sed 's/\[attend\] peers: //')
if [[ -n "$PEER_OUTPUT" ]]; then
  echo "**Peers**: $PEER_OUTPUT"
fi

# Show pending signals
STATUS_OUTPUT=$(attend status 2>/dev/null)
PROJECT_SIGNALS=$(echo "$STATUS_OUTPUT" | grep "project:" | sed 's/.*project: *//')
BROADCAST_SIGNALS=$(echo "$STATUS_OUTPUT" | grep "broadcast:" | sed 's/.*broadcast: *//')
if [[ -n "$PROJECT_SIGNALS" ]] || [[ -n "$BROADCAST_SIGNALS" ]]; then
  echo "**Signals**: project: $PROJECT_SIGNALS | broadcast: $BROADCAST_SIGNALS"
fi
