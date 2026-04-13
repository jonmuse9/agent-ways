---
status: Draft
date: 2026-04-12
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-114
  - ADR-119
---

# ADR-121: Salience decay for signal presentation — turn-based exponential

## Context

ADR-119 (action potential engagement model) governs *when new stimuli break through* to fire a disclosure. Disk retention (default 30 days, per the cleanup work on `fix/attend-awareness-stabilization`) governs *how long signal files live on disk*. Neither mechanism addresses the middle case: a signal that has already been disclosed, is still on disk, and is still within the "present to the agent" set at every turn regardless of whether it's still load-bearing.

In a long session, this manifests as presentation bloat — signals from the first few turns keep appearing in notifications 40, 60, 80 turns later, long after their context has been overtaken. A survey of 18 recent active sessions shows the distribution is long-tailed: median 12 turns, p75 45, p90 84, max 133. Short sessions never see the problem; long sessions see it badly.

Time-based aging is tempting but wrong at this scale. Turn pacing varies by an order of magnitude — a 10-minute think turn and a 15-second reply both count as "one turn" and should age the same. A time-based window would wobble with pacing; a turn-based window tracks the actual thing we care about: how many rounds of decision-making have passed since this signal was useful.

Disk retention (30 days) is intentionally time-based because at that horizon time is a fine proxy for "bulk turns" — variance averages out across thousands of turns. Presentation aging operates at a much finer grain where the turn/time distinction matters, so the two mechanisms use different units.

## Decision

Signals carry a **salience** that starts at 1.0 when they arrive and decays exponentially with turns elapsed since arrival. Below a configurable floor, a signal stops appearing in notifications — but the file stays on disk until the 30-day cleanup removes it. Salience is reset to 1.0 whenever a signal is re-engaged (replied to, referenced).

The decay is parameterized by a **half-life in turns** — the number of turns after which an un-engaged signal has dropped to 50% salience. Default: 20 turns. Presentation floor: 0.3.

Re-engagement semantics preserve the "keep the active thread alive" behavior: an ongoing peer conversation keeps its signals hot as long as either side is replying.

Salience decay is a presentation-layer mechanism. It does not delete, mutate, or hide signals at the storage layer. It only gates what the peer sensor emits into notifications.

## Consequences

### Positive

- **Graceful fade, not a cliff.** Exponential decay means no hard "at turn 45 everything drops" behavior. The curve tracks declining relevance smoothly.
- **Short sessions unaffected.** With half-life 20 and median session 12 turns, the typical session sees no aging — first-turn signals are still ~66% salient at session end.
- **Long sessions see natural pruning.** In a p75 (45-turn) session, signals from the opening are below the 0.3 floor by the end; in a p90 (84-turn) session they're gone well before the end.
- **Symmetric with ADR-119.** Action potential handles engagement (when to fire); salience decay handles presentation (when to fade). Together they form an inward-outward pair.
- **Turn-based matches mental model.** Users think in conversational turns, not wall clock. The unit matches the intuition.

### Negative

- **Mechanism complexity.** Every signal now needs an `arrival_turn` field and the peer sensor gains a per-signal salience computation on every scan. Modest, but not free.
- **Turn counter dependency.** Requires a reliable source of "current turn" — tying attend's presentation to the session transcript couples it to Claude Code's JSONL format. If the format changes, attend breaks. ADR-119's tune command already has this coupling; extending it is not a net-new risk.
- **Re-engagement detection is heuristic.** "Did this signal get re-engaged?" requires matching replies or references against earlier signals. Imperfect matching will leak some aging through; accepting that as cost.
- **Threshold tuning.** The half-life and presentation floor are empirically chosen. The 20-turn default is grounded in real session data, but the right values on someone else's usage may differ. Config overrides handle this, but there's no automatic tuning yet.

### Neutral

- **Orthogonal to disk retention.** The 30-day disk cleanup is unchanged. Salience decay sits entirely above it, in the presentation path.
- **Replaces nothing.** ADR-119's engagement model is untouched. This layers on top.
- **Configurable.** All parameters live in attend config following the ADR-115 overlay pattern.

## Alternatives Considered

### Hard cutoff at N turns

Reject signals older than turn `current - N`. Simpler to implement (no per-signal salience), but produces a cliff — at turn 45, everything from before turn 0 vanishes abruptly. The user's framing ("could be exponential") explicitly argued against this. Exponential matches the intuition that relevance declines gradually, not abruptly.

### Time-based aging (minutes or hours)

Use `u2u_median` (~82s) as one-turn-equivalent and compute salience from wall clock. Cheaper because no turn counter needed. Rejected because turn pacing varies too much at this granularity — a single deep think turn can inflate apparent "age" and drop fresh signals below the floor incorrectly. At 30-day horizons time is fine (disk retention), but not at minutes-to-hours.

### Linear decay over N turns

`salience = max(0, 1 - (turns_since / N))`. Even simpler than exponential. Rejected because linear decay implies "every turn contributes equally to aging," which misrepresents the real curve: the first few turns of irrelevance matter more than the last few. Exponential naturally models "relevance halves repeatedly," which is closer to how attention actually works.

### Per-category half-lives

Different signal types (peer message vs. build event vs. git change) could age at different rates. Tempting but premature — start with one half-life across all signal types, add per-category overrides only if real use demands it. ADR-119 set the precedent of global defaults with per-sensor overrides available; follow the same pattern.

### Subsume into ADR-119's refractory machinery

Action potential already tracks engagement dynamics. Why not extend the refractory curve to cover presentation aging too? Rejected because the two mechanisms answer different questions. Refractory asks "should this new stimulus fire?" — an inward gate. Salience asks "should this already-fired signal still be shown?" — an outward gate. Conflating them would force a single curve to serve two opposite purposes and make both harder to reason about.
