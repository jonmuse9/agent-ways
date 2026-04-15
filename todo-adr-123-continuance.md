# ADR-123 Firing Dynamics — New Session Continuance

**PR:** https://github.com/aaronsb/agent-ways/pull/49 (draft)
**Branch:** `feat/firing-dynamics`
**Status at handoff (2026-04-14):** Architecture shipped end-to-end; Phase A–D + visualization + cross-tool lint + doc rewrites + ADR rewrites are all landed on the branch. The draft PR is open for review. Remaining work is mostly verification-shaped and fits a fresh session.

## What to do first

Start by reading — in this order — to load full context:

1. **The PR body** at #49. Top-to-bottom. It summarizes every commit, what shipped, the yaml-to-runtime mapping, and the explicit list of "still pending" work. The PR is the fastest way to rebuild the picture.
2. **[`todo-adr-123-firing-dynamics.md`](../../todo-adr-123-firing-dynamics.md)** on the branch. Same file from the original session. Phase A–D are marked `[x]`; Phase E, F, and sub-items within are still open.
3. **[`docs/architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md`](../architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md)** — the canonical architecture. Status is still `Draft` per plan task F3 ("last action of the whole plan"). Flipping to `Accepted` is part of this session's work.
4. **[`docs/hooks-and-ways/observed-behavior.md`](../hooks-and-ways/observed-behavior.md)** — the n=1 empirical baseline from 2026-03-17 that Phase F is meant to reproduce. Don't start Phase F without reading this; it's the target to preserve or improve.

Do not skim. The previous session compressed a lot into a small number of commits, and the PR body is the index; knowing the index well is what lets this session make local calls without guessing.

## Work items, in recommended order

### 1. Operator-level B3 attend parity check (highest priority, ~10 min)

The Phase B commit (`9340185`) migrated attend's engagement state from `Instant`/`Duration` linear-decay onto the progression-axis `Curve::ActionPotential` with exponential decay. Unit + integration tests pass, but the real verification is running attend in its normal environment and watching whether peer/git/build cadence matches pre-refactor behavior.

**What to do:**
- `attend run` in a normal dev session (or however the operator normally launches it — it's a persistent background process, likely via Monitor).
- Trigger the kinds of sensor events the operator usually sees: peer messages, git commits, build-complete signals.
- Watch whether the fire cadence feels right. Specifically: does absolute refractory still work (no fires in the first ~60 s after a burst)? Does relative refractory still work (high-magnitude events break through while low-magnitude ones get suppressed)?
- If any visible regression: `git revert 9340185` on the branch, push, re-open this task. The Phase B worksheet in `todo-adr-123-firing-dynamics.md` (B1) documents the linear-vs-exponential shape difference — if the regression is in the "tail" region (multi-minute post-burst), the caveat there is the first place to look.

**Success criterion:** no observable regression in one real-workload run. This is n=1; not a stats argument. If it feels right, note it in the PR comment thread and move on.

### 2. `ways list` + `ways rethink` operator walkthrough (~15 min)

Commit `47bc9de` converted both visualizations to per-way thresholds. The operator was watching `ways list` live during the previous session and confirmed they saw normal disclosure calculations, which is a positive signal. But a full walkthrough on a real completed session — especially one with heterogeneous curves — is worth doing before the PR merges.

**What to do:**
- `ways list` on the current session. Confirm the per-row `Re-disclosure` column shows sensible percentages, the bar positions cluster in the expected places, and the footer shows something like `"20–30K intervals"` (or whatever range the active way curves produce).
- `ways rethink --list` to pick a recent multi-event session. Replay it with the TUI. Watch whether per-way bar positions move with each frame as way re-fires get recorded.
- Edge case to test: a session that fires a way twice (first fire + re-fire) — confirm the second fire repositions the bar.
- Known limitation: `rethink` uses the *current* frontmatter to resolve curves, not whatever was in the file at the time the session was recorded. If a way's curve has been edited since the session, the replay shows the new curve. This is documented; not a bug.

**Success criterion:** visualizations render without crashes and the per-way intervals make intuitive sense. Note any rough edges in the PR thread.

### 3. Phase E — `ways tune` subcommand (~2–4 hours)

The original plan's Phase E is a new `ways tune` subcommand that surveys `~/.claude/stats/events.jsonl`, computes per-way cadence statistics, and suggests curve parameters grounded in real usage. Same discipline as `attend tune`.

**Plan reference:** `todo-adr-123-firing-dynamics.md` tasks E1–E4.

**What to build:**
- E1: New subcommand skeleton at `tools/ways-cli/src/cmd/tune_ways.rs` (or extend the existing `cmd/tune.rs`, which is currently locale-tuning scoped — check which split is cleaner). Parses `~/.claude/stats/events.jsonl`, groups `way_fired` and `way_redisclosed` events by way id, computes mean/p50/p75/p90/max token delta between fires.
- E2: Derive `half_life` from the cadence — rule of thumb `half_life ≈ median delta between fires`. For bursty patterns, also suggest `Curve::ActionPotential` parameters. Output as proposed frontmatter diffs.
- E3: `--apply` flag rewrites the relevant `curve:` entries in place. Dry-run by default. Preserve surrounding YAML verbatim (use the same line-surgery pattern as `config_lint::rewrite_without_lines`).
- E4: Commit as `feat(ways): ways tune subcommand for empirical curve calibration (ADR-123 Phase E)`.

**Gotchas:**
- Event log format: look at a few lines of `~/.claude/stats/events.jsonl` before assuming shape. The events are written by `session::log_event` and `hooks/ways/inject-subagent.sh` — the fields are `ts`, `event`, `way`, `domain`, `trigger`, `scope`, `project`, `session`, plus a handful of optional fields.
- Per-project vs global tuning: the operator has multiple project ways scopes. Start with global corpus tuning; per-project comes later.
- Not every way has enough fires to tune. Suggest parameters only when the way has at least N fires (pick a floor, maybe 3) in the survey window.
- The suggested curve may differ from what the way currently has. `--apply` needs to handle the existing `curve:` block cleanly — replace it, don't append a second one.

**Success criterion:** `ways tune` runs against the user's real events log, prints a reasonable table of suggested curves per way. `ways tune --apply` against a test way produces the expected frontmatter edit. Corpus rebuilds clean.

### 4. Phase F — A/B reproduction against observed-behavior.md (~1–3 hours of real work time)

The 2026-03-17 n=1 observation is the empirical baseline: vanilla Claude vs ways Claude on the same code-review-into-release task, with vanilla burning ~275k tokens and needing constant operator redirection while ways held steady at ~200k with minimal redirection. The Phase F ask is to reproduce this observation on the new ADR-123 stack and confirm the new implementation is at least as good as the pre-refactor one on the same task shape.

**Plan reference:** `todo-adr-123-firing-dynamics.md` task F1.

**What to do:**
- Pick a supertask + detour-work task that mirrors the 2026-03-17 shape. Ideally something the operator is actually trying to accomplish — not a contrived benchmark.
- Run it on the new stack. Track: total context tokens at end, number of redirections from operator to pull back to the supertask, qualitative sense of whether the supertask held or collapsed into "whatever the last user message asked for."
- Write results into a new section of `docs/hooks-and-ways/observed-behavior.md`, e.g. `## Re-observation — 2026-04-?? — post ADR-123`. Use the same table format as the original so comparison is direct.
- Call out any noteworthy behavioral differences, good or bad. Especially: did the new per-way curves change which ways fired and when, compared to the flat 25% default?

**Success criterion:** new observation is at least as good as the original on the same task shape. If it's measurably worse, stop — something load-bearing was lost in the refactor and the PR isn't ready to merge. The failure mode to watch for is vanilla-Claude-style flattening of the task stack under redirection.

### 5. `docs/attend-and-monitor/salience.md` rewrite (~30 min)

The previous session rewrote engagement.md, context-decay.md, loop.md, and configuration.md but skipped salience.md. It currently describes the attend-side signal-presentation aging as "designed, not yet implemented." That's still true for attend — but the inward/outward gate framing the page introduces is now shipping cross-tool via ADR-123, and the ways side implements the exact outward gate the page describes.

**What to do:**
- Rewrite in the same style as ADR-121's rewrite (which is the content source of truth for this doc).
- Keep the target audience: attend implementers who want to know when signal salience will be real for peer signals.
- Make the page accurate: the framing landed, the attend application is deferred, the ways side is the first concrete implementation and can be pointed at for the shape.
- Cross-reference: link to ADR-121 (for the decision), ADR-123 (for the unified engine), and the ways `session::way_fire_outcome` path (for the first concrete implementation).

**Success criterion:** the page accurately describes the current state without status notes or cross-references-to-git-history. A reader landing on it should understand (a) what the decision is, (b) what ships today, (c) what's still pending and where.

### 6. ADR-123 status transition (~5 min, only after F completes)

Plan task F3: flip `ADR-123-firing-dynamics-progression-axis-unification.md` frontmatter from `status: Draft` to `status: Accepted`. Add an "Implementation Status" section at the bottom noting the shipped phases and any deferred items. Run `adr index` to regenerate INDEX.md. Commit with message like `docs(adr): ADR-123 Draft → Accepted after Phase F validation`.

**Do NOT** flip this before Phase F validation passes. The plan is explicit that this is the last action. If Phase F surfaces a regression, ADR-123 stays Draft until the regression is resolved.

### 7. PR readiness checklist

Once items 1–5 land (item 6 is the explicit last action):

- [ ] All pending items in the PR body's "Operator-level verification" section are checked.
- [ ] Phase E commits are added to the branch.
- [ ] Phase F observation is in `observed-behavior.md` with a new dated section.
- [ ] salience.md rewrite is committed.
- [ ] ADR-123 is Accepted.
- [ ] `adr lint` passes (should still be clean).
- [ ] `cargo test --workspace` passes.
- [ ] `ways lint --global` and `attend config lint` both return 0 errors.
- [ ] PR body is updated to reflect completed items (move the "Operator-level verification" and "Deferred" lists into the "shipped" section).
- [ ] PR moved from Draft to Ready for Review.

## Deferred past this PR

These are noted in the PR body but are **not blocking**. Land them as separate follow-up PRs after merge:

- **`attend status` refractory state display** — ADR-119 step 7. Should show current multiplier and refractory state per sensor in the existing status table.
- **Motivation / reflection-overdue sensor wiring** — ADR-119 steps 8–9. A new sensor that emits time-based sub-threshold stimuli to drive intrinsic self-prompting.
- **`burst_window` yaml key removal** — once a few real usage cycles have run with `attend config lint --fix` removing it, the parser can stop accepting the key entirely.
- **Curves with both inward AND outward sides populated** — not currently used by any tool. A future way might want both a refractory gate AND a salience fade on the same state.

## Context that may save time

- **The `x-` escape hatch exists in both linters** — ways and attend. If you introduce a new field while prototyping, prefix with `x-` and the linter leaves it alone until you promote it to the schema.
- **`attend config lint --fix` has been run against the user's live config once already** — `burst_window` was removed in-place during the previous session's testing. The user's current config is clean against the ADR-123 schema.
- **The quality way fired on `lint.rs` in real time during the previous session** — at 938 lines. This is why the linter is now split. If you add more code and cross the threshold again, the same way will fire on your Edit. Listen to it.
- **`ways list` can be run mid-session** to check which ways have fired and when they're scheduled to re-fire. It's the best live debugger for the engine.
- **`ways show way <id> --session <sid> --trigger <channel>`** is the direct engine query. Shelling out to it is what the reactive firing path does; you can do the same to sanity-check any individual way's behavior without hooking Claude Code.
- **If `git reset --hard` is tempting** during Phase F regression diagnosis — don't. Use `git revert <sha>` to preserve the audit trail. The commit sequence is the PR's main artifact.

## Questions likely to come up

**Q: Should ADR-123 already be Accepted?**
No. Plan F3 is explicit: last action of the whole plan, after Phase F validates. Flipping it early is a paperwork move that lies about the validation state.

**Q: Is salience.md rewrite blocking the PR?**
Probably yes, for narrative coherence — every other firing-dynamics doc has been rewritten for ADR-123 and this one is a visible gap. But it's small enough (~30 min) that it's not worth gating on before the operator-level verification items.

**Q: What if Phase F shows a regression?**
Stop. Don't ship. Investigate whether the regression is (a) a curve-shape mismatch (fixable via per-way tuning), (b) a load-bearing behavior that was lost in the refactor (requires code fixes), or (c) a measurement artifact (the observation task differed in a relevant way). Document the finding regardless — it goes into observed-behavior.md as a second data point even if the PR doesn't merge.

**Q: Should Phase E come before or after Phase F?**
Plan says E before F — tune the curves from real data, then validate. This makes sense because Phase F is easier to interpret when the curves are empirically grounded rather than at their heuristic defaults. If time is tight, Phase F with default curves is still worth doing as a lower bound; Phase E after is the refinement.
