#!/usr/bin/env bash
# Localization nudge (ADR-139, scenario 01.011.E).
#
# Fires only on the mismatch state: Claude Code is set to a non-English language
# but agent-ways is still in English mode (output_language en/auto — not localized).
# Then it recommends the ways-localize skill. Silent otherwise — English installs
# (the 99% case) and already-localized installs emit nothing.
#
# The body of localize.md is intentionally empty, so this macro's stdout is the
# way's entire output: silent here ⇒ the way is suppressed (no English-mode noise).
# CLI is the contract — ways' effective language comes from `ways language`, not a
# raw config file.

ROOT="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"

# Claude Code's own response language (settings.json `language`, a NAME like "spanish").
cc_lang=$(jq -r '.language // empty' "$ROOT/settings.json" 2>/dev/null | tr '[:upper:]' '[:lower:]')
case "$cc_lang" in
  ""|english|en) exit 0 ;;   # Claude Code is English (or unset) → nothing to offer
esac

# agent-ways' effective language (resolved across config layers). The CLI reports a
# NAME in `resolved_language` ("English", "Spanish", ...); English (en / auto) is the
# unlocalized state we nudge on.
ways_lang=$("$ROOT/bin/ways" language --json 2>/dev/null | jq -r '.resolved_language // "English"' | tr '[:upper:]' '[:lower:]')
case "$ways_lang" in
  english|en|auto|"") : ;;   # ways still English → the mismatch we nudge on
  *) exit 0 ;;               # ways already localized → satisfied, stay silent
esac

# Mismatch: Claude Code is non-English, ways is not. Nudge — the operator is already
# reading in their language, so this reaches them there.
echo "🌐 **Localization available.** Claude Code is set to **${cc_lang}**, but agent-ways is still running in English. With your consent, the **ways-localize** skill can translate every way into ${cc_lang} and tune it against the English source of truth. Ask before running it — it downloads a multilingual model and translates the whole corpus. This note stops once localization is done."
echo ""
