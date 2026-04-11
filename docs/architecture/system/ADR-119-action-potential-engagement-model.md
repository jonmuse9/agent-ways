---
status: Draft
date: 2026-04-11
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-118
---

# ADR-119: Action Potential Engagement Model

## Context

attend's disclosure governor uses linear cooldowns and rate windows to manage notification frequency. This prevents flooding but doesn't model the natural dynamics of productive engagement. An agent responding to peer messages has no signal that engagement value is declining — it will keep responding at the same threshold indefinitely, leading to unbounded context spend on diminishing-value conversations.

The current model:
- **Flat threshold**: a stimulus either meets the emission threshold or doesn't, regardless of recent activity
- **Linear cooldown**: fixed time between disclosures, no relationship to engagement history
- **No refractory period**: an agent that just finished a burst of peer messaging can immediately start another

This creates the "party problem" — agents can burn context on extended peer conversations with no natural disengagement signal. The human has to intervene or the context window runs out.

## Decision

Model agent engagement after the neuronal action potential. The biological signal has properties that map directly to productive agent behavior:

### The Action Potential Shape

```
    Engagement
    (magnitude)
        ^
   +30  |        * peak
        |       / \
        |      /   \
        |     /     \
    0   |    /       \
        |   /         \
  -55   |--*           \          threshold
        | stimulus      \
  -70   |.................\___*___........  resting
        |                refractory
        +--------------------------------> time
```

### Phases

1. **Resting state** — agent at baseline awareness. Sensors poll, observations accumulate. No urgency.

2. **Stimulus & threshold** — an observation (peer message, git change, build event) accumulates magnitude. Sub-threshold stimuli decay without triggering engagement. Only stimuli crossing threshold fire a response. This is the existing emission threshold, unchanged.

3. **Depolarization** — rapid engagement. The agent is actively responding — reading messages, replying, acting on observations. Context flows in quickly. This phase is productive and should not be suppressed.

4. **Peak** — maximum engagement value. The first response to a peer message, the first reaction to a build failure. Highest information density.

5. **Repolarization** — diminishing returns. Continued engagement on the same topic yields less new information per context token spent. The agent should notice this decline.

6. **Refractory period** — after a burst of engagement, the threshold temporarily rises. The agent resists re-engaging with the same stimulus category. Casual follow-ups fall below the elevated threshold. Urgent new stimuli (different category, high magnitude) can still break through.

### Implementation

#### Engagement tracker (per sensor)

```rust
struct EngagementState {
    /// Recent engagement events (timestamp, magnitude)
    history: VecDeque<(Instant, f64)>,
    /// Current refractory multiplier (1.0 = normal, >1.0 = elevated threshold)
    refractory: f64,
    /// Time of last engagement burst
    last_burst: Option<Instant>,
}
```

#### Threshold elevation

After a burst of N engagements within a time window:
- Threshold multiplies by `refractory` factor (e.g., 1.5x after 3 engagements, 2.0x after 5)
- Multiplier decays over time back toward 1.0 (the relative refractory period)
- Decay rate is configurable per sensor — peer messages might have a 5-minute refractory, git observations might have 30 seconds

#### All-or-nothing firing

Below threshold: observation accumulates silently in the delta accumulator. No notification.
Above threshold: full disclosure — the observation is emitted, the refractory clock starts.

This is already how the emission threshold works. The new behavior is that the threshold *moves* based on recent engagement history.

#### Absolute vs relative refractory

- **Absolute** (first 30s after burst): no disclosure from this sensor regardless of magnitude. The agent is processing what it just received.
- **Relative** (30s–5min): disclosure possible but requires elevated magnitude. A casual "thanks" from a peer won't fire. A "HELP: production is down" will.

### Configuration

```yaml
# in attend config
engagement:
  burst_window: 120        # seconds — engagements within this window count as a burst
  burst_threshold: 3       # engagements before refractory kicks in
  refractory_multiplier: 1.5  # threshold multiplier per burst level
  absolute_refractory: 30  # seconds of complete suppression after burst
  decay_rate: 0.1          # refractory multiplier decay per minute
```

### Interaction with focus groups

Focus groups scope which stimuli reach an agent. The action potential model governs how the agent responds once stimuli arrive. They compose naturally:

- `attend focus on deploy` — agent receives deploy group signals
- Signals accumulate normally against the (possibly elevated) threshold
- If the agent just finished a burst of deploy conversation, the threshold is elevated
- Only high-magnitude deploy signals (urgent) break through the refractory period
- The agent naturally disengages from low-value follow-ups

### Motivation signal

The resting state isn't truly idle — the action potential model includes a baseline "resting potential" that represents ambient awareness. A time-based sensor can emit sub-threshold stimuli that accumulate slowly:

- After 5 minutes idle: small stimulus ("time passing, anything to attend to?")
- After 15 minutes: larger stimulus (crosses threshold → agent evaluates its situation)
- After 30 minutes: significant stimulus → agent checks inbox, peers, pending work

This provides intrinsic motivation without task assignment. The agent periodically evaluates whether there's something worth its attention, with frequency determined by the same threshold mechanics as everything else.

## Consequences

### Positive

- **Natural disengagement** — agents stop engaging when returns diminish, without rules
- **Burst tolerance** — rapid-fire engagement during productive phases is not suppressed
- **Urgency discrimination** — truly urgent signals break through refractory period
- **Self-regulating** — no human intervention needed for the "party problem"
- **Biologically grounded** — the model is well-studied, predictable, and intuitive
- **Intrinsic motivation** — idle agents self-prompt without external task assignment

### Negative

- **Complexity** — more state per sensor (engagement history, refractory multiplier)
- **Tuning** — refractory parameters need empirical calibration
- **Opaque** — harder for users to understand why an agent isn't responding to a message (elevated threshold)

### Neutral

- **Replaces nothing** — layers on top of existing disclosure governor, doesn't remove it
- **Configurable** — all parameters in attend config, per-sensor overrides possible
- **Observable** — `attend status` can show current refractory state per sensor

## Implementation Plan

1. Add `EngagementState` to sensor-trait alongside `DeltaAccumulator`
2. Track engagement history in `SensorSlot` — record each disclosure timestamp + magnitude
3. Implement threshold elevation: `effective_threshold = base_threshold * refractory_multiplier`
4. Implement absolute refractory: suppress all disclosures for N seconds after burst
5. Implement relative refractory: decay multiplier over time
6. Add engagement config to attend config
7. Add refractory state to `attend status` output
8. Add idle/motivation sensor as a new sensor crate (sensor-motivation or sensor-idle)
9. Wire motivation sensor to reflection-overdue way
10. Tune parameters empirically through multi-agent conversation sessions

## References

- Game AI Pro Chapter 2: "Informing Game AI Through the Study of Neurology" — action potential diagram and neurological grounding
- ADR-113: attend active awareness module — disclosure governor, emission thresholds
- ADR-118: focus groups — scoping which stimuli reach an agent
- Cognitive Frameworks paper — cognitive economics, cheapest path = correct path
