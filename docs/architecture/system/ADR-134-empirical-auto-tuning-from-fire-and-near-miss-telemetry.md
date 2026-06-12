---
status: Draft
date: 2026-06-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
  - ADR-125
  - ADR-130
  - ADR-135
---

# ADR-134: Empirical auto-tuning from fire and near-miss telemetry

## Context

The firing engine is tuned by hand. Every threshold, half-life, and vocabulary was set by authorial judgment, and the system collects no evidence that could revise them. Three gaps compound:

1. **ADR-123 Phase E was never built.** The planned `ways tune` cadence calibration (derive per-way `half_life` from observed fire deltas in `~/.claude/stats/events.jsonl`) remains open; `ways tune` today audits locale alias fidelity only (per ADR-125).
2. **Fires are logged; near-misses are discarded.** The matcher computes a score for every way on every prompt, then throws away everything below threshold. False silences — the way that *should* have fired — are structurally invisible, so the precision-first discipline (0 false positives as hard constraint) has no recall measurement to trade against.
3. **Fire relevance is unmeasured.** A 2026-06-09 session readout showed ~17 of 47 fires landing in a session whose work never touched their domain (`itops/incident`, `delivery/migrations`, `testing/mocking` firing into a docs-only session). Each costs an injection; collectively they erode the trust the emission discipline exists to protect. Nothing currently distinguishes a fire that shaped an action from one that was scrolled past.

Hand-tuning cannot close these gaps because the evidence doesn't exist to hand-tune *from*. A nervous system that cannot adjust its own sensitivities from experience is a reflex arc.

## Decision

Extend the telemetry surface and the `ways tune` subcommand into an empirical tuning loop with three measurements and a gated apply step.

1. **Near-miss logging.** The matcher logs, per prompt, the top-scoring below-threshold candidates (score within a margin of their threshold, e.g. 0.05) to `events.jsonl` as `way_nearmiss` events. The scores are already computed; this is persistence, not new computation. Volume is bounded by the margin.
2. **Cadence calibration (ADR-123 Phase E, as planned).** `ways tune --cadence` groups `way_fired` / `way_redisclosed` events by way, computes token-delta distributions between fires, and suggests `half_life` per the existing Phase E worksheet (rule of thumb: half_life ≈ median delta).
3. **Relevance signal.** `ways tune --precision` correlates each way's fires with the session's subsequent activity class, derived from data already in the event log (trigger channel, tool mix, domains of other fires). A way whose fires repeatedly land in sessions that never touch its domain is flagged with its observed irrelevance rate and suggested remedies: threshold raise, vocabulary narrowing, or trigger-channel change. This is a heuristic flag, not a verdict — the same contract as `ways tune`'s fidelity audit.
4. **Gated apply.** All three report by default; `--apply` rewrites frontmatter (`half_life`, `embed_threshold`) in place. Vocabulary changes are never auto-applied — they re-shape the embedding neighborhood and stay authorial. Applied changes are ordinary git diffs, reviewable and revertible.

The recall counterpart falls out of (1): near-miss data plus the existing fixture workflow lets `ways tune` report likely false silences (ways that consistently score just under threshold on prompts whose sessions then did that way's kind of work), giving the 0-FP discipline its first recall estimate.

## Consequences

### Positive

- Closes the open loop: telemetry that exists only as a record becomes input to calibration. Half-lives can be retuned per model generation (the maintenance posture ADR-301 documented).
- False silences become measurable for the first time; precision-first stops being precision-only.
- Per-session precision audits (`ways list` plus irrelevance rates) turn anecdotes like the 2026-06-09 readout into a tracked metric.

### Negative

- `events.jsonl` grows faster; near-miss margin needs a cap and the log needs rotation.
- The relevance signal is a proxy — activity-class correlation can mislabel legitimately cross-cutting ways (e.g. `meta/tracking`). Flags must stay diagnostic, never auto-applied to vocabulary.
- `--apply` writing frontmatter from observed behavior risks codifying one user's work distribution; suggested values should show the sample size they derive from.

### Neutral

- ADR-123 remains Accepted and unedited; this ADR absorbs and extends its open Phase E. Phase F (A/B validation) is unaffected.
- Fine-tuning the embedding model (deferred in ADR-108) becomes more attractive once near-miss data accumulates as training signal — out of scope here.

## Alternatives Considered

- **Edit ADR-123 to widen Phase E** — rejected: ADR-123 is an Accepted historical record; widening its scope post-acceptance hides when the precision dimension entered the design.
- **Model-graded relevance (LLM judges whether each fire influenced the session)** — rejected for now: highest-fidelity signal but adds inference cost to a system whose value is being cheap and ambient. Revisit if the activity-class proxy proves too coarse. Note that an *external* instance of this signal already arrives for free: the periodic Claude Code usage report is model-graded analysis of where a session actually went wrong, generated outside this loop at no inference cost to it. It does not measure way-fire relevance directly, but it measures the downstream thing ways exist to prevent — and a 2026-06 report (write-time friction: Buggy Code 40, Wrong Approach 37, Excessive Changes 7) is what empirically justified the content-level trigger in ADR-135, this design's first pattern-level consumer.
- **Manual periodic audits of `ways list` output** — rejected as the only mechanism: it found today's signal, but it doesn't scale and never sees near-misses.
