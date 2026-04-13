# Salience decay

This page covers the **presentation-layer aging** mechanism: how a signal's visibility in the conversation fades over turns even while the signal file remains on disk. It's the turn-based cousin of the 30-day time-based disk cleanup described in [`signals.md`](signals.md).

**Status**: designed in ADR-121 (draft), not yet implemented at the time of writing. This page describes the decision, not a current behavior — the mechanism will land as a follow-up.

## The shape of the problem

Disk retention (30 days) addresses bulk accumulation — signal files shouldn't live forever on disk. The action potential model (ADR-119, see [`engagement.md`](engagement.md)) addresses engagement — a sensor shouldn't keep firing on diminishing-value stimuli. But neither of those addresses the middle case: a signal that *was* disclosed, *is* still on disk, and *keeps getting re-presented* every time the peer sensor scans, long after it has stopped being load-bearing.

In a 90-turn session, a signal that arrived at turn 3 is still being surfaced at turn 80 — even though by turn 80 that context has been overtaken by at least an order of magnitude of new events. The signal isn't stale in any disk-cleanup sense (it's hours old, not days), but it's stale in a *relevance* sense. The conversation has moved on.

ADR-121's framing: **signals need presentation-layer aging, measured in turns, decaying smoothly.**

## The decision

Every signal carries a **salience** that starts at 1.0 when it arrives and decays exponentially with turns elapsed since arrival. Below a configurable floor (default 0.3), the signal stops appearing in notifications — but the file stays on disk until the 30-day cleanup removes it. Salience is reset to 1.0 whenever a signal is re-engaged (replied to, referenced, or otherwise touched).

The parameter the user tunes is **half-life in turns**: the number of turns after which an un-engaged signal has dropped to 50% salience.

```
salience(t) = 0.5^((t - arrival_turn) / half_life)
```

Default half-life: **20 turns**. Default floor: **0.3**.

### Why these numbers

Half-life is grounded in survey data from real session transcripts. A sweep of 18 recent active sessions produced the distribution:

```
min=1  median=12  p75=45  p90=84  max=133  mean=32.6
```

With half-life 20 and floor 0.3, the behavior at each distribution point:

| Session length | Salience of a first-turn signal at the end |
|---|---|
| median (12 turns) | 0.66 — still visible, nothing faded |
| p75 (45 turns) | 0.21 — below floor, dropped from presentation |
| p90 (84 turns) | 0.06 — effectively gone |

**Read this as**: short sessions never see aging (nothing to prune), medium sessions see the oldest stuff drop out cleanly near the end, long sessions experience meaningful pruning. The curve is gentle enough that mid-session signals don't vanish too fast, and steep enough that long-tail signals actually go away.

## Re-engagement resets

A signal that's been replied to or referenced is a signal that's still relevant. The reset semantics:

- When an agent or human replies to a signal (using the `re:` threading field from ADR-120), the original signal's `arrival_turn` is updated to the current turn. Its salience returns to 1.0.
- When a message references a prior signal by content (best-effort matching, not implemented), the same reset happens.
- When a signal is re-disclosed for any reason (e.g., an agent that joins mid-conversation sees backlog), the arrival_turn for *that agent's view* resets — because the signal just entered their attention.

Re-engagement is the "conversation thread still alive" signal. As long as either side is responding, the thread stays hot. When neither side has touched it for ~20 turns, it fades.

## Why turn-based, not time-based

Short answer: **precision where it matters, convenience where it doesn't.**

Disk retention operates at 30-day scale, where turn pacing variance averages out across thousands of turns. "30 days" is a round human number everyone intuits. Time is a fine proxy for bulk turns at that horizon.

Attention window operates at 20-turn scale, where turn pacing variance is order-of-magnitude (a 10-minute think turn and a 15-second reply both count as "one turn"). Time-based aging at this grain would wobble with pacing — a conversation that happens to have long think turns would drop signals out of presentation based on wall clock, which has nothing to do with whether they're still relevant. Turn count tracks the actual thing we care about: how many rounds of decision-making have passed since this signal was useful.

Both mechanisms exist for legitimate reasons. They use different units because different scales warrant different units. Future-us should not "fix" the apparent inconsistency — the split is deliberate and called out in ADR-121.

## Where the turn counter comes from

Attend can count turns from authoritative sources, no estimation needed:

- **Primary source**: parse the active session's JSONL transcript at `~/.claude/projects/<encoded-cwd>/<session>.jsonl`. Count `"type":"user"` entries (excluding `tool_result`). Attend already reads these files in `attend tune`; the same parser can be used for the live turn counter.
- **Secondary source**: the context sensor already tracks context percentage across polls. Turn count correlates with context growth; it's a less-precise but more-available signal.

The primary source is exact. The secondary is a fallback if transcript reading fails for any reason. The implementation will try primary first.

## What's different from engagement

Salience decay and engagement (ADR-119) are two different mechanisms solving two different problems. They look similar on the surface — both involve curves, thresholds, decay — but they operate on opposite sides of the disclosure path:

| | Engagement (ADR-119) | Salience (ADR-121) |
|---|---|---|
| **Question** | "Should this new event fire?" | "Should this already-fired signal still be visible?" |
| **Direction** | Inward gate — stimulus → disclosure | Outward gate — disclosure → presentation |
| **Unit** | Time (for refractory) | Turns |
| **Scope** | Per sensor | Per signal |
| **Effect** | Suppresses low-magnitude follow-ups after a burst | Ages out stale signals from active notification set |
| **Resets on** | Decay over time | Re-engagement (reply, reference) |

They compose. A signal that fires in a hot conversation passes both gates: engagement approves it (the sensor is in relative refractory but the magnitude is high enough to break through) and salience is at 1.0 (just arrived). Later, that same signal might still exist on disk while salience has fallen to 0.15 and the sensor is back at baseline engagement. If a follow-up event references it, salience resets to 1.0 and it becomes visible again.

## Alternatives rejected in ADR-121

Documented here for posterity. Full rationale is in the ADR itself.

- **Hard cutoff at N turns.** Simpler than exponential, but produces a cliff — at turn 45, everything from before turn 0 vanishes abruptly. Exponential matches the "relevance declines gradually" intuition better.
- **Linear decay over N turns.** Treats every turn's contribution to aging as equal. But the first few turns of irrelevance matter more than the last few — exponential's "halves repeatedly" shape is closer to how attention actually works.
- **Time-based aging.** Wobbles with pacing, as described above.
- **Per-category half-lives** (different decay rates for different signal types). Premature — start with one half-life globally, add per-category overrides if real use demands it.
- **Subsume into engagement's refractory machinery.** Conflates the inward and outward gates, making both harder to reason about.

## Implementation notes (when it lands)

The mechanism needs three things:

1. **Per-signal `arrival_turn` metadata.** Either in the signal file (a new field alongside `from|project|cwd|re:|message`) or in a side table. Side table is probably cleaner — it keeps the wire format stable for readers that don't care about salience.
2. **A live turn counter.** Parse the active session's transcript; count user turns. Update on each peer sensor poll (cheap — the file is small and grows slowly).
3. **A presentation gate in `sensor-peers`.** Before emitting a signal as an observation, compute its salience. If below the floor, suppress. If above, pass through.

No changes to the engagement model, the disclosure governor, or the signal file format (assuming side-table storage for `arrival_turn`). The implementation is scoped to the peer sensor and a new small module for turn counting.

Configuration additions when implemented:

```yaml
attention:
  half_life_turns: 20      # default 20 — matches session distribution analysis
  presentation_floor: 0.3  # below this, signal is no longer presented
```

## Related

- **ADR-121** — the decision record (draft)
- [`signals.md`](signals.md) — disk-side retention (the time-based cousin)
- [`engagement.md`](engagement.md) — the inward-gate engagement model
- [`loop.md`](loop.md) — where the presentation gate will sit in the loop
