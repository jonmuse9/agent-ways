---
status: Draft
date: 2026-04-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-104
  - ADR-105
  - ADR-108
  - ADR-112
  - ADR-113
---

# ADR-114: `attend` Events as an Insistent Way Trigger Type

## Context

[ADR-113](./ADR-113-attend-active-awareness-module.md) introduces `attend`, a sibling binary in the agent-ways workspace that implements the active awareness layer described in the [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md) design note. `attend` observes Claude Code session state and environmental signal, tracks approaching mechanical consequences (context pressure, reflection deferral, etc.), and produces emissions that need to reach Claude.

The question this ADR answers is: **how do those emissions become guidance Claude reads?**

### Current trigger types

The ways system supports a small set of trigger types, each keyed to a hook event in Claude Code's loop:

| Trigger | Fires on | Example |
|---|---|---|
| `UserPromptSubmit` | User sends a message | Intent classification ways |
| `PreToolUse` | Before Claude calls a tool | Tool guidance ways |
| `Stop` | After Claude's response | Reflection capture, cleanup |
| `SessionStart` | New session begins | Project pulse (ADR-106), orientation |
| `PostCompact` | After a compaction pass | Resume ways, ADR-112 |
| `context-threshold` | Context crosses a percentage boundary | Reflection, memory distillation (ADR-104, ADR-112) |

Every existing trigger is **reactive**: it keys off an event inside Claude's own loop (Claude did something, or Claude is about to do something, or Claude's context is changing). There is no trigger type for events *outside* Claude's loop — no way for a file change, an idle timeout, a context-pressure projection, or a user-requested timer to reach Claude through the same machinery.

### The gap and why reuse is the right answer

`attend` produces exactly that class of event: externally-observed signals that need to become injected guidance. A natural (and wrong) response would be to invent a new delivery channel for attend — a separate notification stream, a new hook class, a sidechannel that bypasses ways. The ways system already solves the hard parts: scoring observations against context, token-gated re-disclosure (ADR-104), progressive disclosure (ADR-105), and the shared vocabulary for guidance Claude reads. Building a parallel delivery system would duplicate that machinery and compromise the habituation properties that make the existing system work.

The right answer is to treat attend emissions as just another signal source that feeds the same pipeline. The ways matcher doesn't care whether a signal came from a user prompt or a sensor — it cares whether the signal matches any way in the corpus and whether those ways are eligible for disclosure under current rules. Adding attend as a signal source is a schema extension, not a new system.

### Why "insistent"

The trigger type is called `attend` by identity (the source) and carries an insistence property by nature (the emissions escalate based on unacted consequences; see ADR-113). Ways subscribing to this trigger type inherit that insistence behavior — their disclosure can escalate when attend's emission urgency rises, and the existing disclosure gate already supports weighted disclosure decisions.

## Decision

Extend the way trigger schema with a new type, `attend`, that subscribes a way to one or more signal types produced by the `attend` binary. Emissions from attend flow through the existing matcher and disclosure pipeline. No new hook events, no new delivery channel, no bypass of existing scoring.

### Schema extension

Ways declaring `trigger.type: attend` use the following frontmatter fields:

```yaml
---
name: reflect-on-context-pressure
description: Guide Claude through progressive ledger reflection as context approaches compaction
trigger:
  type: attend
  subscriptions:
    - context-pressure
  debounce_turns: 5           # minimum turns between disclosures (default: ADR-104 rules)
  salience_floor: 0.4         # suppress emissions below this weight (default: 0.3)
---

(way body — the guidance Claude reads when the signal fires and the way matches)
```

Field definitions:

- **`type: attend`** (required) — Marks this as a proactively-delivered way sourced from the attend binary.
- **`subscriptions`** (required) — List of signal types this way subscribes to. Matched against the `emits` field in sensor headers. A way can subscribe to multiple signal types, and one signal can match multiple ways.
- **`debounce_turns`** (optional) — Minimum turns between successive disclosures of this way. Defaults to ADR-104's standard re-disclosure rules if not specified.
- **`salience_floor`** (optional) — Emissions whose computed salience falls below this threshold are suppressed for this way. Allows ways to be selective about which emission modes (informational, affordance, insistent, critical) they want to engage with.

Standard fields (`name`, `description`, `embed_threshold`, way body, etc.) behave exactly as they do for other trigger types. The way body is still guidance Claude reads when the trigger fires and the way is selected.

### Subscription and matching

When attend emits a signal, the emission includes:

- **Signal type** (e.g., `context-pressure`, `file-churn`, `peer-session-active`)
- **Salience weight** (computed from insistence state)
- **Emission mode** (`informational`, `affordance`, `insistent`, `critical`)
- **Payload text** (the declarative observation — e.g., *"disclosed at turn 47, currently turn 52, projected critical at turn 58"*)
- **Metadata** (current turn, context state, consequence horizon, any sensor-specific fields)

The ways matcher receives the emission through attend's bridge to the `ways` CLI and:

1. Looks up all ways whose `trigger.type: attend` subscriptions include this signal type
2. For each candidate way, applies `salience_floor` to determine if the emission meets the way's selectivity threshold
3. Runs the emission payload through standard embedding/BM25 scoring against the way's corpus
4. Applies ADR-104 disclosure gating to check if the way has been disclosed recently
5. For surviving candidates, emits the way body as standard injection
6. Records the disclosure in the disclosure tracker (ADR-104) and notifies attend that the signal was surfaced so it can update its acknowledgment tracker

Steps 3 and 4 are the existing matcher and disclosure gate. Steps 1, 2, and 6 are new, but they are additions to the existing pipeline, not parallel infrastructure.

### Emission modes and way response

ADR-113 defines three emission modes (plus `silent`). Each mode produces a different interaction with the disclosure pipeline:

| Mode | Behavior |
|---|---|
| `silent` | No emission; ways never see it |
| `informational` | Standard matcher + disclosure gate; way fires if eligible, subject to normal habituation |
| `affordance` | Same as informational, but the emission payload includes a named tool or action Claude can invoke to investigate further |
| `insistent` | Elevated weight; disclosure gate is more willing to re-fire even if the way was recently disclosed; emission names stakes explicitly |
| `critical` | Maximum weight; disclosure gate bypasses normal cooldown if necessary; emission carries explicit consequence language |

Escalation between modes is handled entirely by attend (ADR-113 insistence emitter). The ways system honors the mode by letting higher-weight emissions re-surface ways that would otherwise be cooled down. This is a straightforward extension of the existing disclosure gate: it already computes an eligibility score; attend emissions contribute to the disclosure weight calculation.

The "acknowledged-but-silent" state from the design note is implemented by attend's sensor layer, not the ways system — it corresponds to observations attend holds below the salience floor until conditions warrant raising them. From the ways system's perspective, these observations simply never arrive.

### Unified disclosure and habituation

The critical property this ADR preserves: **ways triggered by attend go through the same disclosure gate as ways triggered by user prompts or tool use**. The disclosure tracker does not distinguish signal sources; it only tracks which ways have been disclosed and how recently. This means:

- A way that fires on both user prompts and attend signals (hypothetically, via two trigger declarations on the same way) would be subject to the same recent-disclosure suppression regardless of which source fired
- The ADR-104 token-gated re-disclosure rules apply uniformly
- Habituation works the same way: the first email arrival triggers full guidance, subsequent arrivals in a burst are suppressed to terse acks, extended silence eventually allows re-disclosure at full weight

This unified treatment is what makes the awareness layer compose cleanly with the rest of ways. Authors of attend-triggered ways don't need to learn new disclosure rules. Users don't need a separate mental model for reactive vs proactive ways. The underlying system is one system with two kinds of signal sources.

### Way authoring model

The author of an attend-triggered way writes it the same way they write any other way. They identify what Claude should know or do when a particular signal arrives, they write the guidance as the way body, they declare the subscription, and the system handles delivery. No knowledge of attend internals is required.

Example:

```yaml
---
name: note-build-completion
description: Surface a brief completion note and suggest test invocation when a long build finishes
trigger:
  type: attend
  subscriptions:
    - build-complete
  debounce_turns: 3
---

A background build has finished. Consider:

- Running the test suite to validate the build
- Checking output for warnings worth investigating
- Updating the working context with what changed

The user may already be aware via terminal output — only engage with this if
the build result is directly relevant to current work.
```

This way only fires when attend's sensor detects a build completion (presumably via a wall-clock observer monitoring a build process or checking exit codes on a known command). The author didn't write any sensor code, didn't touch attend's internals, and didn't need to know how the signal is delivered.

### Graceful no-op when attend is absent

If `attend` is not running, ways with `trigger.type: attend` are **inert, not broken**. The matcher knows these ways exist but receives no signals for them. They never fire. The rest of the ways system functions normally. When attend starts, the ways become live and begin receiving signals.

This is the "presence as additive" property from the design note, expressed at the ways layer. An agent-ways installation that never runs attend never experiences any downside from having attend-triggered ways in the corpus — they are dormant until someone wants them.

### Scope of this ADR

This ADR defines:

- The new `trigger.type: attend` schema
- The subscription and matching model
- How emissions flow through the existing matcher and disclosure pipeline
- Way authoring for this trigger type
- The graceful-no-op behavior when attend is not running

It explicitly does not define:

- The attend binary's internal architecture (that is ADR-113)
- The sensor catalog or specific signal types (those grow incrementally)
- How attend discovers which ways are subscribers (implementation detail — the ways CLI already knows how to query the corpus)
- Any changes to ADR-104's disclosure gate beyond accepting attend-sourced signal weights as input

## Consequences

### Positive

- **Unified delivery pipeline.** Ways system remains the single authority for guidance injection. No parallel channel, no new hook event class, no bypass.
- **Uniform habituation.** ADR-104's disclosure rules apply to proactive and reactive ways identically. Way authors don't need to learn new rules.
- **Separation of concerns.** `attend` produces signals; `ways` routes guidance. Each system does one thing and they meet at a well-defined interface (emission payloads with a known schema).
- **Incremental adoption.** Ways declaring `trigger.type: attend` can be added to the corpus before attend is even built — they simply remain dormant. This allows way authors to begin drafting ahead of the binary landing.
- **Graceful absence.** Sessions without attend running experience no breakage. The trigger type is inert, not error.

### Negative

- **Ways system gains a new input source.** The matcher and disclosure gate now need to accept emissions from a second source (attend, in addition to Claude Code hooks). This is a small addition but it does expand the ways system's API surface slightly.
- **Coordination between repos/crates.** Any change to the emission schema requires coordinated updates in both `attend` and `ways`. Mitigation: the schema is small, stable, and versioned if it needs to change.
- **Implicit dependency in way corpora.** A corpus that contains many attend-triggered ways effectively assumes attend is available in the target environment. Mitigation: ways are documented as "inert without attend" and do not fail when attend is absent; authors should ensure baseline reactive ways cover the essential behavior.

### Neutral

- **No change to existing trigger types.** This ADR adds a new type; it does not modify UserPromptSubmit, PreToolUse, Stop, SessionStart, PostCompact, or context-threshold. Existing ways continue to work exactly as before.
- **Way authoring model is unchanged for non-attend ways.** Only authors who want to write attend-triggered ways need to learn the new fields. Everyone else continues as before.
- **Scoring infrastructure is unchanged.** The embedding + BM25 + NCD tier (ADR-107, ADR-108) handles attend emission payloads the same way it handles any other text input.

## Alternatives Considered

- **New hook event type (e.g., `AttendEmission`).** Rejected. Duplicates the machinery ways already provides. Ways and hooks are different abstractions, and attend emissions are guidance signals, not lifecycle events — they belong in the ways pipeline, not a parallel hook class.
- **Direct injection bypassing the disclosure gate.** Rejected. Bypasses ADR-104's habituation rules. Would cause noisy ways to dominate Claude's context and violate the design note's "acknowledged silence" third state. The disclosure gate is exactly the right place for these decisions.
- **Attend has its own notification channel into Claude's context.** Rejected. Creates two competing attention surfaces and forces Claude to mentally merge them. One pipeline is simpler and the existing pipeline already handles all the hard parts.
- **A single catch-all "external" trigger type instead of `attend`-specific.** Considered and rejected. Tying the trigger type to a specific source (attend) is more honest — it documents what's producing the signals and prevents the trigger type from becoming a dumping ground for arbitrary external signal sources. If another source of proactive signal is ever needed, it can have its own trigger type and its own ADR.
- **Subscribe ways to sensors directly (skip the emission concept).** Rejected. The emission layer is where insistence lives. If ways subscribed to sensors directly, they would have to implement escalation logic themselves, which belongs in attend's insistence emitter for uniformity.
- **Require attend to be running for any way with `trigger.type: attend` to load.** Rejected. Violates the "additive, never required" invariant. Ways must load regardless of attend's state and simply become inert when attend is absent.

## References

- **Design note:** [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md)
- **Related ADRs:**
  - [ADR-104](./ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Disclosure gate that this ADR reuses
  - [ADR-105](./ADR-105-progressive-disclosure-for-way-trees.md) — Progressive disclosure model
  - [ADR-108](./ADR-108-embedding-based-way-matching-with-all-minilm-l6-v2.md) — Matcher that scores attend emission payloads
  - [ADR-112](./ADR-112-session-ledger-and-knowledge-graph-integration.md) — Reflection and ledger ways that will be the first consumers of attend signals
  - [ADR-113](./ADR-113-attend-active-awareness-module.md) — The attend binary whose emissions this ADR routes
