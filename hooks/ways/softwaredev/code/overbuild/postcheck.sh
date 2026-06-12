#!/usr/bin/env bash
# Postcheck for softwaredev/code/overbuild (ADR-135, Tier 2).
#
# Reads the PostToolUse input on stdin. Exit 0 = "freshly written code matches a
# seed over-build pattern — fire the way." Exit 1 = no match, which is the
# default and the correct outcome on anything unfamiliar (ADR-135: silence on
# the unrecognized beats a confident-wrong flag).
#
# The seed set is deliberately tiny and high-precision. Grow it only through the
# authoring loop (#7) from encounter telemetry — never by speculative addition.
set -euo pipefail

INPUT=$(cat)

# Only code writes. Ignore Bash/Task and any non-write tool.
tool=$(printf '%s' "$INPUT" | jq -r '.tool_name // empty')
case "$tool" in Write | Edit | MultiEdit) ;; *) exit 1 ;; esac

# Code files only — mirror code.check's allowlist so we never fire on docs,
# config, or a markdown file that merely contains an example.
path=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty')
[[ "$path" =~ \.(py|ts|tsx|js|jsx|mjs|vue|svelte|go|rs|rb|java|kt|scala|c|cc|cpp|h|hpp|cs|php|swift|sh|lua|ex|exs)$ ]] || exit 1

# The text actually being written: Write.content, Edit.new_string, or each
# MultiEdit edit's new_string.
content=$(printf '%s' "$INPUT" \
  | jq -r '[.tool_input.content, .tool_input.new_string, (.tool_input.edits[]?.new_string)]
           | map(select(. != null)) | join("\n")' 2>/dev/null || true)
[[ -n "$content" ]] || exit 1

# 1. Hand-rolled LRU/TTL cache — OrderedDict used for eviction.
if grep -qE 'OrderedDict' <<<"$content" \
   && grep -qE 'move_to_end|popitem\(\s*last\s*=\s*False' <<<"$content"; then
  exit 0
fi

# 2. Hand-written email-format regex — a regex constructed with an '@' in it.
if grep -qE '(re\.(match|compile|fullmatch|search)|new RegExp|MustCompile|Regex::new|regexp\.Compile)[^\n]*@' <<<"$content"; then
  exit 0
fi

# 3. Hand-rolled singleton — __new__ guarding a stored _instance.
if grep -qE '__new__' <<<"$content" && grep -qE '_instance' <<<"$content"; then
  exit 0
fi

exit 1
