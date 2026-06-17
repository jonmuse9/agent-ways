#!/usr/bin/env bash
# Sync this agent-ways clone into ~/.claude WITHOUT making ~/.claude the repo.
#
# The canonical install (scripts/install.sh) clones agent-ways directly into
# ~/.claude so its skills/agents/commands/hooks/settings.json *are* the
# user-level config Claude Code reads. This script is for the other topology:
# the repo lives in a subdirectory (e.g. ~/.claude/agent-ways) while ~/.claude
# is an established config dir with the user's own plugins/credentials/sessions.
#
# It copies the repo's outputs into ~/.claude and merges only the `hooks` block
# (plus a few ways permissions) into the existing settings.json, leaving the
# user's model/theme/statusLine/plugins untouched. Re-runnable and idempotent;
# every changed target is backed up first.
#
# Cross-platform: pure POSIX bash + portable primitives (cp -r, mkdir -p, jq).
# No symlinks (Windows needs Developer Mode), no PowerShell/cmd syntax. Runs
# identically under Git Bash on Windows, macOS, and Linux.
#
# Usage:
#   scripts/sync-to-home.sh             # build binaries (if cargo present), sync, merge settings
#   scripts/sync-to-home.sh --no-build  # skip the binary rebuild; copy whatever is in bin/

set -euo pipefail

# --- Colors ---
if [[ -t 1 ]]; then
  GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'
  BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'
else
  GREEN='' YELLOW='' CYAN='' BOLD='' DIM='' RESET=''
fi

info()  { echo -e "${CYAN}::${RESET} $*"; }
ok()    { echo -e "${GREEN}ok${RESET} $*"; }
warn()  { echo -e "${YELLOW}warn${RESET} $*"; }

# --- Resolve source (repo root) and destination (~/.claude) ---
SRC="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${HOME}/.claude"

NO_BUILD=false
for arg in "$@"; do
  case "$arg" in
    --no-build) NO_BUILD=true ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
  esac
done

# Guard: never run in the canonical topology (would copy a dir onto itself).
if [[ "$SRC" == "$DEST" ]]; then
  warn "Source is ~/.claude itself — this is the canonical install; nothing to sync."
  exit 0
fi

# Sanity: make sure SRC really is an agent-ways clone.
if [[ ! -f "$SRC/hooks/check-config-updates.sh" ]] || [[ ! -d "$SRC/skills/attend" ]]; then
  echo "error: $SRC does not look like an agent-ways repo." >&2
  exit 1
fi

# jq is required for the settings merge.
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required (used to merge settings.json safely)." >&2
  echo "       Install jq, then re-run." >&2
  exit 1
fi

echo ""
echo -e "${BOLD}agent-ways → ~/.claude sync${RESET}"
echo -e "Source: ${CYAN}${SRC}${RESET}"
echo -e "Target: ${CYAN}${DEST}${RESET}"
echo ""

# --- 0. Backup everything we are about to touch ---
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="${DEST}/backups/sync-${STAMP}"
mkdir -p "$BACKUP"
for item in settings.json commands hooks bin; do
  if [[ -e "$DEST/$item" ]]; then
    cp -r "$DEST/$item" "$BACKUP/"
  fi
done
ok "Backed up settings.json, commands/, hooks/, bin/ → ${DIM}${BACKUP}${RESET}"

# --- 1. Skills (additive; agent-ways skill names don't collide with the user's) ---
info "Syncing skills/"
mkdir -p "$DEST/skills"
cp -r "$SRC/skills/." "$DEST/skills/"
ok "skills/ synced"

# --- 2. Agents ---
info "Syncing agents/"
mkdir -p "$DEST/agents"
cp -r "$SRC/agents/." "$DEST/agents/"
ok "agents/ synced"

# --- 3. Commands (refresh stale copies) ---
info "Syncing commands/"
mkdir -p "$DEST/commands"
cp -r "$SRC/commands/." "$DEST/commands/"
ok "commands/ refreshed"

# --- 4. Hooks (ways tree + top-level scripts referenced by settings) ---
info "Syncing hooks/"
mkdir -p "$DEST/hooks/ways"
cp -r "$SRC/hooks/ways/." "$DEST/hooks/ways/"
# Top-level hook scripts the wired settings reference by absolute path.
for h in check-config-updates.sh refresh-claude-md.sh; do
  [[ -f "$SRC/hooks/$h" ]] && cp -f "$SRC/hooks/$h" "$DEST/hooks/$h"
done
# Make every hook executable (matters on macOS/Linux).
find "$DEST/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true
ok "hooks/ synced"

# --- 5. Binaries ---
if [[ "$NO_BUILD" == "false" ]] && command -v cargo >/dev/null 2>&1 && command -v make >/dev/null 2>&1; then
  info "Rebuilding binaries (make update-binaries)"
  make -C "$SRC" update-binaries || warn "binary rebuild had issues — copying whatever is in $SRC/bin"
else
  [[ "$NO_BUILD" == "true" ]] && info "Skipping rebuild (--no-build)" || warn "cargo/make not found — copying prebuilt binaries from $SRC/bin"
fi
info "Copying binaries → $DEST/bin"
mkdir -p "$DEST/bin"
for b in ways attend attend-chat way-embed; do
  if [[ -f "$SRC/bin/$b" ]]; then
    cp -f "$SRC/bin/$b" "$DEST/bin/$b"
    chmod +x "$DEST/bin/$b" 2>/dev/null || true
  fi
done
ok "binaries copied"
# Best-effort corpus build for the new location's semantic matching.
"$DEST/bin/ways" corpus --quiet 2>/dev/null || warn "ways corpus not built (model may be missing) — matching falls back to pattern/command triggers"

# --- 6. Activate the ways layer: merge hooks + ways permissions into settings.json ---
info "Merging settings.json (hooks block + ways permissions)"
SETTINGS="$DEST/settings.json"
[[ -f "$SETTINGS" ]] || echo '{}' > "$SETTINGS"

ADD_PERMS='["Bash(ways:*)","Bash(attend:*)","Bash(attend-chat:*)","Bash(way-embed:*)","Edit(~/.claude/**)","Write(~/.claude/**)"]'

TMP="${SETTINGS}.tmp.$$"
TMP2="${SETTINGS}.tmp2.$$"
# Pass 1: set the hooks block + union the ways permissions, preserving all
# existing keys (model/theme/statusLine/plugins/...).
jq --slurpfile src "$SRC/settings.json" --argjson add "$ADD_PERMS" '
  .hooks = $src[0].hooks
  | .permissions = ((.permissions // {})
      | .allow = ((.allow // []) + ($add - (.allow // []))))
' "$SETTINGS" > "$TMP"
# Pass 2: quote-safe the hook command paths so they survive a $HOME containing
# spaces (e.g. C:\Users\Jonathan Muse). Without this, the shell that runs each
# hook splits the path at the space and executes the wrong target. Single
# source of truth: scripts/quote-hook-paths.jq.
jq -f "$SRC/scripts/quote-hook-paths.jq" "$TMP" > "$TMP2"
rm -f "$TMP"
mv -f "$TMP2" "$SETTINGS"
ok "settings.json updated (hooks quote-safe; existing model/theme/statusLine/plugins preserved)"

echo ""
echo -e "${GREEN}${BOLD}Sync complete.${RESET}"
echo -e "Restart Claude Code, then ${CYAN}/attend${RESET} and the other ways skills will appear."
echo ""
echo -e "${DIM}Note: ~/.claude is not a git repo in this topology, so the upstream${RESET}"
echo -e "${DIM}update-check is a no-op. To update: pull this clone, then re-run this script.${RESET}"
echo ""
