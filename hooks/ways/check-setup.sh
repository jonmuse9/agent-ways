#!/bin/bash
# SessionStart: Check if ways installation is complete.
# Runs as the first startup hook. If setup is incomplete, emits a
# one-time diagnostic and exits cleanly so other hooks don't error.
#
# Checks: ways binary → embedding model (optional) → corpus

WAYS_BIN="${HOME}/.claude/bin/ways"
XDG_WAY="${XDG_CACHE_HOME:-$HOME/.cache}/claude-ways/user"

# Nothing to check if this isn't a ways-enabled install
[[ ! -d "${HOME}/.claude/hooks/ways" ]] && exit 0

if [[ ! -x "$WAYS_BIN" ]]; then
  cat <<'MSG'

⚠️  Ways setup incomplete — the `ways` binary is not installed.

Hooks will be inactive until setup completes. Run:

    cd ~/.claude && make setup

This downloads the ways binary, embedding model, and generates
the matching corpus. If you don't have a Rust toolchain, pre-built
binaries are downloaded automatically.

MSG
  exit 0
fi

# Binary exists — check corpus
CORPUS="${XDG_WAY}/ways-corpus.jsonl"
if [[ ! -f "$CORPUS" ]]; then
  cat <<'MSG'

⚠️  Ways corpus not generated — semantic matching is inactive.

Run:

    cd ~/.claude && make setup

MSG
  exit 0
fi

# Warn loudly if embedding model is missing — ADR-125 made it a hard dependency
MODEL="${XDG_WAY}/minilm-l6-v2.gguf"
EMBED_BIN="${XDG_WAY}/way-embed"
if [[ ! -f "$MODEL" ]] || [[ ! -x "$EMBED_BIN" ]]; then
  # Only mention this once per day (rate limit via marker file)
  MARKER="/tmp/.claude-embed-notice-$(date +%Y%m%d)"
  if [[ ! -f "$MARKER" ]]; then
    cat <<'MSG'

⚠  Embedding engine not installed — semantic way matching is unavailable.
   Only explicit pattern:/commands:/files: triggers will fire.

To install:

    cd ~/.claude && make setup

MSG
    touch "$MARKER" 2>/dev/null
  fi
fi
