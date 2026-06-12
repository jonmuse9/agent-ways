#!/usr/bin/env bash
# Per-user sessions root — shared by all hook scripts.
#
# MUST stay identical to session::sessions_root() in the ways binary
# (tools/ways-cli/src/session.rs). Both compute this independently; if they
# diverge, the binary and the hooks read/write session state in different
# directories and coordination silently breaks. Resolution order:
#   1. $XDG_RUNTIME_DIR/claude-sessions            (Linux/systemd — already per-user)
#   2. Windows: $LOCALAPPDATA/claude-ways/sessions (per-user; matches the .exe)
#   3. /tmp/.claude-sessions-<uid>                 (other Unix)
#
# Usage: source this file, then use $SESSIONS_ROOT
#   source "$(dirname "$0")/sessions-root.sh"

if [[ -n "${XDG_RUNTIME_DIR:-}" ]]; then
  SESSIONS_ROOT="${XDG_RUNTIME_DIR}/claude-sessions"
else
  case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*)
      # Windows (Git Bash): LOCALAPPDATA is per-user and matches the binary.
      SESSIONS_ROOT="${LOCALAPPDATA}/claude-ways/sessions"
      ;;
    *)
      SESSIONS_ROOT="/tmp/.claude-sessions-$(id -u)"
      ;;
  esac
fi
