#!/usr/bin/env bash
# Precheck: building agent-ways from source needs Rust/Cargo >= 1.85.
#
# A transitive dependency (getrandom) uses Rust edition 2024, which was
# stabilized in 1.85. On older toolchains `cargo build` fails deep in dependency
# compilation with a cryptic "feature `edition2024` is required" error. Catch it
# up front with an actionable message instead.
#
# Called from the source-build branches of the Makefile. If cargo is absent the
# build target's own fallback handles it, so this exits 0 (don't double-report).

set -u
MIN_MAJOR=1
MIN_MINOR=85

command -v cargo >/dev/null 2>&1 || exit 0   # no cargo → build target reports it

ver=$(cargo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
[ -z "$ver" ] && exit 0                       # unparseable → don't block

maj=${ver%%.*}
min=${ver#*.}; min=${min%%.*}

if [ "$maj" -lt "$MIN_MAJOR" ] || { [ "$maj" -eq "$MIN_MAJOR" ] && [ "$min" -lt "$MIN_MINOR" ]; }; then
  cat >&2 <<EOF

  ──────────────────────────────────────────────────────────────────────────
  ERROR: agent-ways needs Rust/Cargo >= ${MIN_MAJOR}.${MIN_MINOR} to build from source.

    you have:  cargo ${ver}
    why:       a dependency (getrandom) uses Rust edition 2024 (stabilized in 1.85)

    fix:       rustup update            # if Rust came from rustup
               # or install rustup:     https://rustup.rs

    (A pre-built binary may also be available — see the releases page.)
  ──────────────────────────────────────────────────────────────────────────
EOF
  exit 1
fi
exit 0
