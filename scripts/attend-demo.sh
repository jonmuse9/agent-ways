#!/usr/bin/env bash
# Naive attend experiment — tests Monitor delivery mechanics.
# Emits lines to stdout on a wall-clock schedule.
# Logs diagnostics to stderr (should NOT appear as notifications).
#
# Usage: attend-experiment.sh [--burst] [--quiet]
#   --burst   emit 5 lines rapidly to test batching + rate limits
#   --quiet   long intervals (30s) to test persistent mode patience

set -euo pipefail

MODE="${1:-normal}"
TICK=0

emit() {
  echo "$1"  # stdout → Monitor notification
}

log() {
  echo "[attend-exp] $1" >&2  # stderr → should go to output file, not notifications
}

log "starting in mode=$MODE, pid=$$"

case "$MODE" in
  --burst)
    log "burst mode: emitting 5 lines rapidly"
    for i in 1 2 3 4 5; do
      emit "burst line $i of 5 — testing batching"
      sleep 0.05  # within 200ms window, should batch
    done
    log "burst complete, now ticking normally"
    sleep 3
    emit "post-burst: settled into normal tick"
    sleep 5
    emit "tick 2 after burst — still alive?"
    sleep 5
    emit "final tick — exiting cleanly"
    ;;

  --quiet)
    log "quiet mode: 30s intervals"
    while true; do
      TICK=$((TICK + 1))
      emit "quiet tick $TICK (every 30s)"
      sleep 30
    done
    ;;

  *)
    # Normal mode: tick every 10s, emit observations
    log "normal mode: 10s intervals, simulating sensor observations"
    while true; do
      TICK=$((TICK + 1))

      # Simulate a sensor that mostly sees nothing
      if (( TICK % 3 == 0 )); then
        # Every 3rd tick, emit an observation
        emit "tick $TICK: state change detected (simulated file delta)"
      elif (( TICK % 7 == 0 )); then
        # Every 7th tick, emit an affordance-style notification
        emit "tick $TICK: context at 72% — projected critical at turn 58 (6 turns remaining). Use \`ways show attend/context-pressure\` for reflection guidance."
      else
        # Most ticks: nothing emitted, just log
        log "tick $TICK: no state change (silent)"
      fi

      sleep 10
    done
    ;;
esac

log "exiting cleanly"
