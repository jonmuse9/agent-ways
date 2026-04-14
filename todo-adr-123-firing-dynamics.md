# Implementation Plan: ADR-123 Firing Dynamics

**Branch:** `feat/firing-dynamics`
**ADR:** [`docs/architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md`](docs/architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md)
**Grounding:** [`docs/hooks-and-ways/observed-behavior.md`](docs/hooks-and-ways/observed-behavior.md)
**Theory:** [`docs/hooks-and-ways/context-decay.md`](docs/hooks-and-ways/context-decay.md)

## How to use this file

This is a sequenced task list for a multi-commit refactor. Work one phase at a time. Each task has a **What** (action) and a **Done when** (acceptance criterion). Phases are ordered by dependency — do not skip ahead. Commit at task boundaries so each step is independently reviewable and revertable.

When picking up work, find the first unchecked task in the earliest unfinished phase and start there. When a decision point arises that the plan does not specify, stop and check in with the operator rather than guessing.

Mark tasks complete with `[x]` as they land. Keep the file current — this *is* the task tracker.

---

## Phase A — Foundation: unit-agnostic engine + curves

The goal of this phase is to land the `Curve` enum and a refactored `EngagementState` in `sensor-trait` with full unit tests, **without touching attend's call sites yet**. Attend keeps working off its current `Instant`/`Duration`-based state until Phase B. This phase is pure addition: the new types and the new enum land alongside the old, so existing tests stay green.

- [x] **A1. Introduce progression-axis types in `sensor-trait`.**
  - **What:** Add `pub type Tick = u64;` and `pub type TickDelta = u64;` aliases to `tools/sensor-trait/src/lib.rs`. Additive only.
  - **Done when:** `cargo build -p sensor-trait` passes; the types are referenced by nothing yet.

- [x] **A2. Define the `Curve` enum.**
  - **What:** Add `pub enum Curve { Exponential, ActionPotential, ProgressiveStaircase, Flat }` with fields per ADR-123 Decision 2. All decay parameters use `half_life: TickDelta`. `ActionPotential` uses `burst_threshold: usize`, `peak_multiplier: f64`, `absolute_refractory: TickDelta`, `multiplier_half_life: TickDelta`. No `burst_window` as a tick span — burst window is implicit via `multiplier_half_life` decay.
  - **Done when:** Enum compiles with `#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]`, YAML round-trip works for all four variants.

- [x] **A3. Implement `Curve::salience_at(delta: TickDelta) -> f64`.**
  - **What:** `Exponential` returns `0.5_f64.powf(delta as f64 / half_life as f64)`. `ActionPotential` returns `1.0` (salience governed by multiplier, not curve shape). `ProgressiveStaircase` returns the salience of the most recent step whose delta is ≤ input. `Flat` returns `1.0` if `delta >= suppression`, else `0.0`.
  - **Done when:** Unit tests exist for each variant covering endpoint cases (delta=0, delta=half_life, delta=2×half_life, delta=huge) and values match by-hand calculation.

- [x] **A4. Implement `Curve::multiplier_at(delta, history, current) -> f64`.**
  - **What:** For `ActionPotential`: count history entries whose contribution hasn't decayed (entries within epsilon of non-trivial multiplier contribution), check against `burst_threshold`, compute `peak_multiplier × 0.5^(delta / multiplier_half_life)` in relative refractory, return `f64::INFINITY` in absolute refractory. For other variants: return `1.0`.
  - **Done when:** Unit tests cover: single fire (multiplier=1.0), burst triggered (>1.0), absolute refractory (effectively infinite), decay back toward 1.0, chunky-axis case (single event advancing tick by large delta — critical for ways).

- [x] **A5. Refactor `EngagementState` to own a `Curve` and tick-based history.**
  - **What:** New struct `EngagementState { curve: Curve, history: VecDeque<(Tick, f64)>, last_fire: Option<Tick> }`. Methods: `new(curve)`, `should_fire(current_tick, magnitude)`, `record_fire(tick, magnitude)`, `current_salience(current_tick)`, `current_multiplier(current_tick)`. Do not delete the old `EngagementState` yet — call the new one `EngagementStateV2` or put in a new module.
  - **Done when:** New state compiles, unit tests cover a full burst-decay cycle, old `EngagementState` still present and passing its own tests.

- [ ] **A6. Commit Phase A.**
  - **What:** Single commit: `feat(sensor-trait): progression-axis Curve enum and EngagementState (ADR-123 Phase A)`. Run `cargo test -p sensor-trait` before committing.
  - **Done when:** Commit lands, tests green, old code path untouched.

---

## Phase B — Attend migration

This phase swaps attend's internals to the new `EngagementState` and retires the old `Instant`/`Duration`-based state. Highest-risk phase because attend is already in production; parameter conversion must be numerically exact, not just syntactically correct.

- [ ] **B1. Compute half-life conversions for every existing attend parameter.**
  - **What:** Find every `decay_per_minute` usage in attend's config and code. For each, compute `half_life_seconds = (ln(0.5) / ln(1 - rate_per_minute)) × 60`. Write conversions into the worksheet below and double-check by plugging back.
  - **Done when:** Every existing parameter has a computed half-life equivalent, verified by round-tripping.
  - **Worksheet:**
    ```
    rate_per_minute → half_life_seconds
    0.10            → 395 s  (ln(0.5)/ln(0.9) × 60)
    (fill in the rest as discovered)
    ```

- [ ] **B2. Swap attend's sensor slots to the new `EngagementState`.**
  - **What:** Replace `SensorSlot::engagement`'s type with the new tick-based struct. Tick source: `SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()`. Wall-clock parameters (`absolute_refractory`, `multiplier_half_life`) are now in seconds. Each `EngagementState` constructed with a `Curve::ActionPotential` populated from the converted values in B1.
  - **Done when:** attend compiles, all existing attend tests pass without modification.

- [ ] **B3. Run a live attend parity check.**
  - **What:** Start attend with the new implementation. Trigger the same kinds of sensor fires the old implementation handled — peer messages, git events, build events. Watch the fire cadence against the old behavior.
  - **Done when:** No observable behavioral regression. If there is one, revisit B1 — conversion was probably wrong.

- [ ] **B4. Delete old `EngagementState` and its dependents.**
  - **What:** Remove the old `Instant`/`Duration`-based state, its unit tests, and any references. Rename `EngagementStateV2` → `EngagementState`.
  - **Done when:** No `Instant` or `Duration` references remain in the engagement path. Full test suite green.

- [ ] **B5. Commit Phase B.**
  - **What:** `feat(attend): migrate to progression-axis EngagementState (ADR-123 Phase B)`.
  - **Done when:** Commit lands. Attend is on the new engine.

---

## Phase C — Ways integration

Where the new engine starts doing actual work for ways. Phase C turns theoretical scaffolding from ADR-123 into executable code in the hot path.

- [ ] **C1. Add `curve:` parsing to `frontmatter.rs`.**
  - **What:** Extend `tools/ways-cli/src/frontmatter.rs` to recognize `curve:` blocks and produce a `Curve` value. Accept all four variants. Reject `redisclose:` at parse time — emit a clear error pointing to the migration (don't silently ignore).
  - **Done when:** Parser tests exist for all four curve variants, plus an explicit rejection test for `redisclose:`.

- [ ] **C2. Migrate every existing way from `redisclose:` to explicit `curve:`.**
  - **What:** Walk `hooks/ways/**/*.md`, convert `redisclose: N` to either `curve: { type: Exponential, half_life: <N converted to tokens> }` or `curve: { type: Flat, suppression: <N> }` depending on intended semantic (exponential as default for smooth fade; flat if step-function was genuinely wanted). `N` is currently a percentage of context window — convert using `REDISCLOSE_PCT` logic assuming 200k default, round to reasonable values.
  - **Done when:** Every way has an explicit `curve:` field. `rg 'redisclose:' hooks/ways/` returns nothing. `lint-ways` passes.

- [ ] **C3. Delete the `redisclose` parser and constant.**
  - **What:** Remove `REDISCLOSE_PCT` from `tools/ways-cli/src/session.rs`, `token_distance_exceeded`, and any `redisclose` reader in `frontmatter.rs`. Grep tree for `redisclose` to confirm no references remain.
  - **Done when:** `rg redisclose` returns nothing except historical mentions in docs/ADRs.

- [ ] **C4. Wire per-way `EngagementState` into `session.rs`.**
  - **What:** Each way gets a persistent `EngagementState` per session, keyed by way ID. Store in the same session-state directory structure as existing markers. Tick source: `get_token_position(session_id)`. On way match: `should_fire(current_tick, stimulus_magnitude)`; if passes, `record_fire(current_tick, magnitude)`. Start magnitude at `1.0`; tune later.
  - **Done when:** Ways fire through the engine instead of through the old step-function check. Existing session markers are migrated or cleared.

- [ ] **C5. Update hook scripts to call the new engine.**
  - **What:** `check-prompt.sh`, `check-task-pre.sh`, `check-file-pre.sh`, `check-bash-pre.sh` go through the new `ways` CLI path to query the engine. Hook scripts stay shell; they shell out to a ways subcommand. Exact subcommand shape is a decision point — if the current `ways` binary doesn't have a match-and-fire command, propose one before implementing.
  - **Done when:** All four hook scripts are on the new path, and a manual test session fires ways correctly through each hook.

- [ ] **C6. Commit Phase C.**
  - **What:** Can be multiple commits if C2 is large — one for "migrate ways frontmatter" and another for "wire ways-cli to engine". Otherwise single commit.
  - **Done when:** Commits land. Ways firing dynamics are on the new engine end-to-end for predictive firing.

---

## Phase D — Reactive firing

Predictive firing is working on the new engine. This phase adds the reactive-firing surface — `PostToolUse` and `PostToolUseFailure` with `postcheck.sh` per way.

- [ ] **D1. Add `PostToolUse` and `PostToolUseFailure` hook matchers to `settings.json`.**
  - **What:** Two new hook entries pointing to `hooks/ways/check-post.sh`. Matchers: `Edit|Write|Bash|Task`.
  - **Done when:** `settings.json` is valid, Claude Code reloads cleanly.

- [ ] **D2. Write `hooks/ways/check-post.sh`.**
  - **What:** Reads `tool_response` from stdin. Walks all ways with a `postcheck.sh` file. For each, runs `postcheck.sh` with `tool_response` as stdin. Exit 0 requests firing (consults engine's inward gate same as predictive). Injects matched ways via `hookSpecificOutput.additionalContext`.
  - **Done when:** Script handles happy path and error cases (missing postcheck, failing postcheck, engine refuses fire). Manual test with a hand-written `postcheck.sh` confirms the pipeline.

- [ ] **D3. Write a real `postcheck.sh` for `softwaredev/code/quality`.**
  - **What:** Reads `tool_response.filePath`, checks file size, exits 0 if over 500 lines. This is the load-bearing demo: reactive firing catches things predictive firing cannot.
  - **Done when:** Writing a 600-line file triggers the quality way via reactive firing — verified by hand.

- [ ] **D4. Commit Phase D.**
  - **What:** `feat(ways): reactive firing via PostToolUse + postcheck.sh (ADR-123 Phase D)`.
  - **Done when:** Commit lands. Reactive firing works end-to-end.

---

## Phase E — Tunability: `ways tune`

The existing `ways` CLI gets a `tune` subcommand that surveys `~/.claude/stats/events.jsonl`, computes per-way firing cadence, and suggests calibrated curve parameters.

- [ ] **E1. Add `ways tune` subcommand skeleton.**
  - **What:** New subcommand in `tools/ways-cli/src/cmd/`. Parses `events.jsonl`, groups by way ID, computes basic statistics (mean token delta between fires, p50, p75, p90, max).
  - **Done when:** `ways tune` prints a table of cadence per way.

- [ ] **E2. Derive curve parameters from cadence.**
  - **What:** For each way, suggest a `half_life` based on observed cadence (rule of thumb: half-life ≈ median delta between fires). For bursty patterns, also suggest `ActionPotential` parameters. Output as suggested frontmatter diffs, not applied yet.
  - **Done when:** `ways tune` outputs actionable suggestions.

- [ ] **E3. Add `--apply` flag.**
  - **What:** `ways tune --apply` rewrites the relevant `curve:` entries in each way's frontmatter in place. Dry-run is default.
  - **Done when:** Dry run shows what would change; `--apply` on a test way produces the expected edit.

- [ ] **E4. Commit Phase E.**
  - **What:** `feat(ways): ways tune subcommand for empirical curve calibration (ADR-123 Phase E)`.
  - **Done when:** Commit lands.

---

## Phase F — Validation

Confirm the refactor actually preserves or improves the observed behavior that motivated it.

- [ ] **F1. Reproduce the 2026-03-17 A/B observation against the new implementation.**
  - **What:** Run the same kind of code-review-into-release task with the new firing-dynamics stack. Compare token cost, redirection count, and output quality to the original observation in [`docs/hooks-and-ways/observed-behavior.md`](docs/hooks-and-ways/observed-behavior.md).
  - **Done when:** New implementation is at least as good as the old one on the same task shape. Write results into a follow-up section of `observed-behavior.md`.

- [ ] **F2. Run `ways tune` against real session data and commit calibrated defaults.**
  - **What:** After a week or two of real usage, run `ways tune --apply` to lock in empirically-grounded curve parameters. Review diff before committing.
  - **Done when:** Calibrated parameters are committed, `observed-behavior.md` gains a second observation entry documenting post-tuning behavior.

- [ ] **F3. Update ADR-123 status from Draft to Accepted.**
  - **What:** Edit ADR-123 frontmatter: `status: Draft` → `status: Accepted`. Add an "Implementation Status" section at the bottom noting shipped phases.
  - **Done when:** ADR reflects shipped state. Last action of the whole plan.

---

## Decision points (stop and ask)

The plan covers the steady path. Stop and check in with the operator when any of these come up:

- **C1–C3 parser/schema errors.** If `cargo test` or `lint-ways` hits a migration edge case that can't be cleanly resolved (e.g., a way where intended semantic is unclear from `redisclose: N` alone), ask — don't guess.
- **C5 subcommand shape.** If `ways-cli` doesn't already have a "match and fire" command that hook scripts can shell out to, the shape of the new subcommand is a design decision — propose options and ask.
- **B3 parity check fails.** If attend behaves differently after the migration, back out and re-check B1's conversions before touching anything else.
- **D3 reactive firing demo doesn't work.** If the postcheck path doesn't trigger on a 600-line file, hook wiring has a bug — diagnose rather than hack around.
- **F1 A/B parity check fails.** If new implementation is measurably worse than old on the same task, the refactor has lost something load-bearing. Do not ship. Investigate.

## Rollback plan

Each phase is a separate commit (or small batch). If a phase breaks something that only surfaces later, `git revert` the phase-ending commit to return to a known-working state. Phases are ordered so earlier phases don't depend on later ones for correctness — A is pure addition, B preserves semantics, C migrates surface-by-surface, D is purely additive on top of C, E is tooling, F is validation.
