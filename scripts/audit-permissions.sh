#!/usr/bin/env bash
# Scan settings.json files for credential-shaped permission entries.
#
# Catches the failure mode where `Always allow` on a `curl -u user:token`
# (or `Authorization: Bearer ...`) pins the secret into the permission
# string verbatim. Run periodically; wire into a hook if you want
# enforcement at commit time.
#
# Usage:
#   scripts/audit-permissions.sh                # scan default files
#   scripts/audit-permissions.sh path/to/x.json # scan specific files
#
# Exit codes:
#   0 — no credential shapes detected
#   1 — findings to review
#   2 — bad invocation

set -u

# What we look for. Conservative: each pattern is a strong credential signal
# on its own, so a hit is worth a human eyeball.
PATTERNS=(
  '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+:[A-Za-z0-9+/_-]{30,}=?[A-Za-z0-9]*' # user:token in basic-auth
  'Bearer +[A-Za-z0-9._~+/=-]{30,}'                                       # OAuth bearer
  '\bsk-[A-Za-z0-9_-]{20,}'                                                # OpenAI-style API key
  '\bsk-ant-[A-Za-z0-9_-]{20,}'                                            # Anthropic key
  '\bghp_[A-Za-z0-9]{30,}|\bghs_[A-Za-z0-9]{30,}|\bgho_[A-Za-z0-9]{30,}'   # GitHub PATs
  '\bxox[baprs]-[A-Za-z0-9-]{20,}'                                         # Slack tokens
  '\bAKIA[0-9A-Z]{16}'                                                     # AWS access keys
  '\bATATT3xFf[A-Za-z0-9_-]{20,}'                                          # Atlassian API tokens
  'AUTH=["'\''"]?[^"'\'' ]+:[A-Za-z0-9+/_-]{30,}'                          # AUTH=user:secret env-var capture
  '(?i)(api[_-]?key|access[_-]?token|secret|passwd|password)["'\''=: ]+[A-Za-z0-9+/_-]{20,}'
)

TARGETS=("$@")
if [[ ${#TARGETS[@]} -eq 0 ]]; then
  # Default scan: this repo's settings, plus a sibling .local if present.
  REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  for f in "$REPO_ROOT/settings.json" "$REPO_ROOT/settings.local.json"; do
    [[ -f "$f" ]] && TARGETS+=("$f")
  done
fi

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  echo "audit-permissions: no settings files to scan" >&2
  exit 2
fi

hits=0
for f in "${TARGETS[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "audit-permissions: skip (not found): $f" >&2
    continue
  fi
  for pat in "${PATTERNS[@]}"; do
    if grep -nP --color=never "$pat" "$f" >/dev/null 2>&1; then
      while IFS= read -r line; do
        printf '%s:%s\n' "$f" "$line"
        hits=$((hits + 1))
      done < <(grep -nP --color=never "$pat" "$f")
    fi
  done
done

if [[ $hits -gt 0 ]]; then
  echo "" >&2
  echo "audit-permissions: $hits credential-shaped entries found." >&2
  echo "Edit the file(s) above to remove the offending entries, and rotate" >&2
  echo "any token whose suffix appears in the matches." >&2
  exit 1
fi

echo "audit-permissions: clean."
exit 0
