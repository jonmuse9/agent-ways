---
status: Draft
date: 2026-04-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-104
  - ADR-111
  - ADR-112
  - ADR-114
---

# ADR-113: `attend` — Active Awareness Module as Executive Layer

## Context

The [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md) design note reads the agent-ways system as a cognitive loop with one missing stage: **active perception**. Every other stage is in place — reactive guidance (ADR-100, ADR-103, ADR-105, ADR-108), attention allocation via disclosure gating (ADR-104), episodic memory (ADR-112 ledger), associative recall (ADR-112 KG), consolidation (compaction plus compaction-checkpoint way), project-level awareness at session entry (ADR-106), and the scoring infrastructure that binds these together. What has been missing is a mechanism by which Claude gains cheap peripheral awareness of its own approaching consequences and the environment around it, without spending reasoning tokens to compute either.

The consequence of that gap is well-defined: Claude operates in a partially blind configuration. Self-monitoring is expensive (costs tokens) and unreliable (can fail to fire). Environmental sensing requires explicit tool use (which consumes an entire turn for a single observation). Consequence tracking happens only when Claude chooses to check, by which point the consequence may be too near to respond to usefully. ADR-112's reflection triggers depend on Claude noticing context-threshold boundaries on its own; ADR-104's disclosure gate depends on Claude being aware of its current disclosure state. Both work well when they work, but both can fail silently when Claude's attention is occupied.

This ADR proposes the missing layer as a new binary, separate from `ways`, co-resident in the agent-ways workspace.

### The delivery primitive: `Monitor`

An additional and load-bearing reason this ADR is possible *now* rather than earlier is the recent release of the `Monitor` tool in Claude Code. Before `Monitor`, there was no standardized channel by which a background process could surface observations into Claude's conversation as asynchronous events. The hook system delivers synchronous injections at event boundaries (`PreToolUse`, `Stop`, `SessionStart`, `PostCompact`, `context-threshold`), but it has no mechanism for signals that occur *between* those boundaries. A persistent process that detected something meaningful had nowhere to send it until Claude's next hook event fired — which could be minutes later or entirely the wrong moment.

`Monitor` closes that gap by treating a background script's stdout as an async event stream, delivering each line as a notification in Claude's chat. With `persistent: true`, a single `Monitor` invocation covers the duration of a Claude Code session. This turns the architectural requirement "an observation channel that works between turns" into a concrete tool invocation. The design note discusses `Monitor` in detail as the *delivery primitive* the awareness layer depends on; this ADR commits `attend` to that delivery model.

### Why a new binary (and not a ways subcommand)

The `ways` binary is a matcher, scorer, and disclosure gate — a stateless (or near-stateless) computation engine invoked by hooks. It responds to events; it does not observe between them. Adding a durable, restartable, turn-reactive executive layer to `ways` would conflate two different lifecycles: the short-lived per-hook invocation of `ways` and the long-lived per-session lifetime of the executive layer. The two share concepts (disclosure, scoring, signal routing) but not lifecycles, and fusing them would compromise `ways`'s simplicity as a pure computation binary.

A sibling crate in the same workspace preserves the integration surface while keeping lifetimes clean. `attend` runs as a separate process and writes single-line observations to stdout. `Monitor` delivers those lines to Claude as asynchronous notifications. For higher-salience observations, `attend` formats the emission as an affordance that Claude can invoke through the existing `ways` command (covered in ADR-114), routing deeper engagement through the normal disclosure pipeline. There is no new IPC protocol and no new hook event class; stdout is the primary delivery channel, and the ways system is the optional deeper-engagement path.

### Prior attempts

ADR-101 (Wormhole relay, Deprecated) and ADR-102 (IRC-based agent communication, Abandoned) both attempted to give Claude inter-instance awareness by routing signals through external transports. Both failed: ADR-101 on transport fragility (wormhole is structurally unsuited for conversation), ADR-102 on complexity and C2-topology aesthetics. Neither addressed the underlying need the awareness layer addresses, because both were pointed outward (Claude ↔ Claude) rather than inward (Claude ↔ self + environment). The design note discusses this difference in detail; this ADR adopts the inward direction as a load-bearing constraint.

## Decision

Introduce `attend`, a new Rust binary hosted as a sibling crate to `ways` in the agent-ways Cargo workspace. `attend` implements the active awareness layer as specified in the design note: durable, restartable, session-scoped, additive to (not required by) Claude Code.

### Identity

- **Name:** `attend`
- **Binary:** `attend`
- **Crate:** `attend/` (sibling to `ways/` in the workspace)
- **Role:** Active awareness module — perception layer and executive coordination for a Claude Code work session
- **Lifecycle:** Per work session. Explicit start and stop. Never autostarts. Exits cleanly when Claude Code exits or when explicitly stopped.

The name is functional. It captures the active ("attend to something") and the caring ("attend to someone's needs") senses without reaching for biology or metaphysics. In prose: *the attend process*, *the attend module*, *active awareness via attend*.

### Scope of this ADR

This ADR defines:

- The binary's role, lifecycle, and internal architecture
- The configuration and permission model
- The sensor plugin model and directory layout
- The persistent state and restart semantics
- The hard invariants the binary must honor
- The build order and rollout phasing
- The alternatives considered and the reasons for rejection

It explicitly does not define:

- The concrete schema for way-side integration — that is [ADR-114](./ADR-114-attend-as-insistent-way-trigger-type.md)'s subject
- The full initial sensor catalog — only the canonical first sensor (interoceptive context-length tracking) is specified here; the toolkit grows in follow-up ADRs
- Any audio/video/content-bearing sensing — explicitly out of scope and deferred to a separate decision
- Cross-session, cross-machine, or cross-instance signal — explicitly out of scope forever for `attend`

### Internal architecture

`attend` is composed of the following components. Each has a well-defined responsibility; most are small; the coordination between them is the turn-boundary processor.

```
attend/
├── main.rs                       # CLI entry point, subcommands
├── lifecycle/
│   ├── start.rs                  # Session startup, state restoration
│   ├── stop.rs                   # Clean shutdown, final state checkpoint
│   └── restart.rs                # Crash recovery
├── turn_loop/
│   ├── processor.rs              # Turn-boundary reactive loop
│   └── context_observer.rs       # Reads Claude Code session state (context %, turn count)
├── consequence/
│   ├── model.rs                  # Per-signal consequence definitions
│   └── projection.rs             # Turn-delta arithmetic, critical-turn projection
├── insistence/
│   ├── tracker.rs                # Acknowledgment tracking per signal
│   ├── emitter.rs                # Generates emissions at appropriate urgency
│   └── scheduler.rs              # Decides when unacted signals need re-emission
├── sensors/
│   ├── dispatcher.rs             # Runs sensor scripts on their schedules
│   ├── registry.rs               # Discovers and loads sensors from disk
│   └── schema.rs                 # Sensor declaration format (header, schedule, required permissions)
├── deferred_intent/
│   ├── store.rs                  # Pending items, user-requested timers
│   └── timer.rs                  # Wall-clock observer for time-based intents
├── state/
│   ├── store.rs                  # Persistent state: signals, baselines, priors
│   └── checkpoint.rs             # Atomic writes, recovery on restart
├── config/
│   ├── loader.rs                 # Reads ~/.config/attend/config.toml
│   └── schema.rs                 # Configuration format
├── permissions/
│   ├── audit.rs                  # Inspect ~/.claude/settings.json for required allowlist entries
│   ├── install.rs                # Write required entries (with explicit user consent)
│   └── sensor_requirements.rs    # Aggregate what sensors need to be allowed to run
└── emit/
    ├── stdout.rs                 # Writes single-line notifications to stdout for Monitor delivery (the primary channel); handles formatting, line buffering, and 200ms batching awareness
    └── ways_affordance.rs        # For high-salience events, formats affordance strings that Claude can invoke via `ways show attend/<signal>` for deeper engagement through the normal disclosure pipeline
```

### Turn-boundary processor

The core loop. On every turn boundary (detected by reading Claude Code session state files, or by hook integration once that interface stabilizes), the processor:

1. Observes the current turn number and context percentage
2. Updates the state for each tracked signal (turns elapsed, growth rate, projected critical turn)
3. Checks whether Claude has acted on any previously emitted signal since the last boundary
4. Decides, per signal, whether an emission is warranted this turn (new, updated projection, insistence escalation, or silent)
5. Dispatches any sensor scripts scheduled for this turn
6. Routes all pending emissions through the ways bridge
7. Checkpoints state to disk

There is no internal tick faster than turn boundaries for consequence tracking. Wall-clock observations (user idle, build duration, user-requested timers) run on their own schedule inside the deferred-intent store, but they do not drive the main processor — they surface their state through sensor emissions the next time the turn processor runs.

### Consequence model and projection

Each tracked signal has a consequence definition:

```toml
[consequence.context_threshold]
signal_name = "context-threshold"
critical_context_pct = 95.0
warning_context_pct = 80.0
description = "Compaction imminent; unflushed reasoning thread will be lost"
```

At each turn boundary, for each tracked signal, the processor computes:

```
turns_elapsed       = current_turn − disclosure_turn
growth_rate         = (current_context_pct − disclosure_context_pct) / turns_elapsed
turns_until_critical = (critical_context_pct − current_context_pct) / growth_rate
projected_critical  = current_turn + turns_until_critical
```

The emission format is declarative and honest:

```
disclosed at turn 47, currently turn 52, projected critical at turn 58 (6 turns remaining)
```

When an emission is warranted (first disclosure, significant projection change, or escalation threshold crossed), the processor calls into the insistence emitter to generate the appropriate message.

### Insistence emission

Insistence is **not** a state machine with levels. It is a pure function of (signal state, projected imminence, disclosure history, acknowledgment status) that produces one of:

- `Silent` — no emission warranted
- `Informational` — state transition or first disclosure, low weight
- `Affordance` — includes a tool or action invitation
- `Insistent` — elevated weight, names stakes, projects criticality explicitly
- `Critical` — maximum clarity, explicit consequence, minimum remaining turns

The function is deterministic. Given the same inputs it produces the same output. There is no stored "urgency level"; urgency is recomputed each turn from current state. This keeps the logic testable and prevents drift.

Each emission is **informational**, not emotional. The emitter formats observations in declarative prose: *"disclosed N turns ago, current context X%, projected critical at turn N+T."* No simulated affect. No arbitrary urgency. Every insistence tracks an actual mechanical consequence.

### Acknowledgment tracking

When `attend` emits a signal, it records the signal's emission turn and signal ID in its state store. On subsequent turn boundaries, it inspects Claude's outputs (via hook integration or transcript tail) for evidence that the signal was acted upon — e.g., a reflection was written, a specific way was engaged, a tool was called, a file was touched.

Acknowledgment detection is **heuristic, not infallible**. False negatives (Claude acted but attend didn't detect) result in redundant re-emission, which the disclosure gate in `ways` will suppress via standard habituation rules (ADR-104). False positives (attend thinks Claude acted but didn't) result in silence when insistence would have been useful, which is the worse failure mode. The acknowledgment tracker is therefore conservative: it only marks a signal acknowledged when it has clear evidence.

### Deferred intent store

Two categories:

1. **Pending observations** — sensor events that have been detected but held below the salience threshold, awaiting a moment when Claude's attention has capacity to integrate them. These are the "acknowledged-but-silent" observations from the design note.
2. **User-requested timers** — explicit "remind me in N minutes" or "surface this at turn X" requests Claude can make of attend via a tool call. Attend holds the request in durable state, and when the trigger condition is met (wall-clock or turn-count), it surfaces the reminder via a sensor emission.

Timers are the one component that legitimately uses wall-clock time. Their emissions still flow through the turn-boundary processor (they become observations on the next turn, not interrupt-style breaks), so the rest of the loop remains turn-reactive.

### Sensor plugin model

Sensors are small programs (primarily shell scripts; compiled plugins allowed) stored in a dedicated sensor directory. The default path follows XDG conventions:

```
~/.config/attend/sensors/
  intrinsic/
    context_pressure.sh           # Interoception: reads Claude Code session state
    turn_velocity.sh              # Interoception: measures inference cadence
  workspace/
    file_churn.sh                 # inotifywait on working tree
    git_state.sh                  # watches HEAD, branch, stash
    peer_sessions.sh              # walks ~/.claude/projects/*/ for other active Claude sessions
  system/
    window_focus.sh               # hyprctl/swaymsg socket tail
    idle_time.sh                  # xprintidle or equivalent
  external/
    # reserved for truly remote signals (CI, GitHub, webhooks); no initial entries
```

Each sensor file begins with a header declaring its contract:

```bash
#!/usr/bin/env bash
#
# sensor: context_pressure
# schedule: on-turn-boundary
# emits: context-pressure
# requires: [tool=Bash, pattern="ws grep '\"context\":' ~/.claude/sessions/.../state.json"]
# description: reads current context percentage from session state
#
```

The `requires` field is consumed by the permission management subsystem (below) to aggregate allowlist entries. The `schedule` field is consumed by the dispatcher. The `emits` field is consumed by the way-side bridge (ADR-114) to route emissions to subscribing ways.

Sensor scripts emit observations as line-oriented output on stdout. Attend captures them, annotates with turn and timestamp, and feeds them through the consequence model and emission pipeline.

### Persistent state and restart

`attend` is durable and restartable. State is checkpointed after every turn boundary to:

```
~/.local/state/attend/<session-id>/
  state.json                      # Tracked signals, baselines, deferred intents, acknowledgments
  sensors/                        # Per-sensor rolling state (priors, counters)
  emissions.log                   # Append-only record of what was emitted when
```

On start, `attend` checks for existing state under the current session ID and restores from it if found. This means if the process is killed (by the OS, by a crash, by explicit stop-and-restart), it comes back exactly where it left off. Signals already emitted remain tracked; acknowledgment history is preserved; deferred intents are honored.

State is scoped per session. When the session ends cleanly (attend receives a stop signal from the user or detects that Claude Code has exited), the state directory is either archived for post-mortem inspection or deleted per configuration.

### Configuration

`attend` reads its configuration from `~/.config/attend/config.toml`. Default configuration is written by `attend init` on first run.

```toml
[attend]
log_level = "info"
state_dir = "~/.local/state/attend"
sensor_dir = "~/.config/attend/sensors"

[lifecycle]
autostart = false                # Never autostart; explicit invocation only
exit_when_claude_exits = true    # Clean exit on Claude Code termination

[emission]
default_salience_floor = 0.3     # Observations below this never emit
insistence_enabled = true
max_emissions_per_turn = 3       # Debounce noisy turns

[consequence.context_threshold]
critical_context_pct = 95.0
warning_context_pct = 80.0

[permissions]
auto_install = false             # Require explicit user consent for permission writes
audit_on_start = true            # Report any missing allowlist entries
```

Configuration is validated with `attend config check`. Missing required fields fail fast with a clear error; unknown fields warn but do not fail.

### Permission management

This is a first-class subsystem, not a footnote. The insistence model depends on emissions reaching Claude without triggering permission prompts. If attend emits a signal via a mechanism that requires Claude to click "allow," the emission becomes an interruption, not an observation, and the entire model breaks silently.

Attend therefore ships with a permission management subsystem that:

1. **Audits** `~/.claude/settings.json` on startup to determine which allowlist entries are needed for attend's own operation and for each registered sensor
2. **Reports** any missing entries to the user in a clear format (which sensor needs what, why)
3. **Installs** required entries on explicit user consent, via `attend permissions install`
4. **Refuses to run sensors** whose required permissions are not present, with a clear error message pointing at the fix

Sensor headers declare required permissions in a structured format:

```
# requires:
#   - tool: Bash
#     pattern: "inotifywait -m *"
#   - tool: Read
#     pattern: "~/.claude/sessions/*/state.json"
```

Attend aggregates all declared requirements across all enabled sensors and compares against the current settings.json. `attend permissions audit` reports the delta. `attend permissions install` writes the missing entries (with a confirmation prompt showing exactly what will be added).

**This is explicitly not auto-applied.** The user must consent. Writing to settings.json is an auditable, reversible action and attend treats it as such. `--dry-run` shows what would be written; `--rollback` restores the pre-install state from a backup automatically created before any write.

The goal is **zero permission prompts during normal operation** with **full user visibility and control over what was allowlisted**.

### Invocation and integration

`attend` is invoked as the **command argument to Claude Code's `Monitor` tool** at session start. The user does not start `attend` at the shell directly; Claude invokes it through `Monitor`, and `Monitor` handles the lifecycle and delivery. This is the load-bearing integration point — see the [design note](../../design-notes/cognitive-loop-and-awareness-layer.md) for the Monitor-as-delivery-primitive discussion.

#### The invocation flow

1. A SessionStart way (shipped with `attend`) fires when a new Claude Code session begins
2. The way guides Claude to invoke the `Monitor` tool with:
   - `command: "attend stream --session=<session-id>"`
   - `persistent: true` (keeps `attend` alive for the session's lifetime)
   - `description: "active awareness module"` (appears in each delivered notification)
3. Claude invokes `Monitor` as instructed
4. `Monitor` starts `attend` as a background process
5. `attend` reads configuration from `~/.config/attend/config.toml`, audits permissions (refusing to proceed if required entries are missing and `auto_install` is false), discovers sensors from `~/.config/attend/sensors/`, restores prior state from `~/.local/state/attend/<session-id>/state.json` if it exists, begins the turn-boundary processor loop, and logs to `~/.local/state/attend/<session-id>/attend.log` (stderr, so Monitor does not deliver log lines as notifications)
6. As `attend` detects events and computes emissions, it writes single-line observations to stdout
7. `Monitor` delivers each stdout line to Claude as an asynchronous chat notification
8. Claude reads notifications between turns, acts or dismisses per the notification's salience
9. When the session ends (or Claude calls `TaskStop`), `Monitor` terminates `attend`; `attend` catches the signal, flushes final state to disk, and exits cleanly

#### Emission is stdout, not IPC

`attend`'s only output mechanism to Claude is stdout. Every meaningful emission is a single line (or at most a short multiline block within 200ms to benefit from `Monitor`'s batching). Lines follow the canonical format:

```
disclosed at turn 47, currently turn 52, projected critical at turn 58 (6 turns remaining)
```

or as affordances for deeper engagement through the ways system:

```
peer session active in ~/Projects/foo-mcp (last activity 3m ago) — use `ways show attend/peer-session-active` for coordination guidance
```

`attend` honors `Monitor`'s discipline rigorously:

- **Selective filtering.** `Monitor` auto-stops any script that produces too many events. `attend`'s insistence engine computes whether each candidate observation warrants emission; only those that do ever reach stdout. Everything else is held silently in the deferred intent store or dropped.
- **Line-buffered output.** `attend` writes one observation per line and flushes immediately. No buffered writes that would delay notifications beyond the 200ms batching window.
- **Stderr is for diagnostics only.** Debug logs, trace information, sensor output that shouldn't become notifications — all go to stderr. `Monitor` writes stderr to a file rather than delivering as notifications. The log file is accessible via the normal `Read` tool if Claude needs to inspect it.
- **Multiline batching awareness.** `Monitor` groups stdout lines emitted within 200ms into a single notification. `attend` uses this deliberately when emitting related observations (signal text, affordance, metadata) — three lines flushed together become one coherent notification rather than three fragmentary ones.

#### Two delivery paths: Monitor primary, ways affordance secondary

`Monitor` notifications are the **primary** delivery channel. Every `attend` observation arrives via `Monitor` as an asynchronous notification in Claude's conversation. For most emissions, this is sufficient: Claude reads the notification, integrates it into its working model, and acts or dismisses based on the observation's urgency as expressed in the notification text itself.

For higher-salience emissions, `attend` additionally formats the notification as an **affordance** — a string naming a specific `ways show attend/<signal-type>` command Claude can invoke if the observation warrants deeper engagement. When Claude invokes that command, the ways system runs the matcher and ADR-104 disclosure gate normally, injecting the full way body through the standard guidance pipeline. This is covered by [ADR-114](./ADR-114-attend-as-insistent-way-trigger-type.md).

The two paths compose:

- **Low/medium salience:** `Monitor` notification only. Claude reads the one-line observation, acknowledges peripherally, moves on. No ways invocation, no additional token cost beyond the notification.
- **High salience (insistent/critical):** `Monitor` notification + affordance. Claude reads the notification, recognizes the stakes, invokes the named ways command, receives the full guidance body, acts deliberately. The notification raises awareness; the ways command provides the deeper context Claude uses to act on it.

`attend` may *suggest* the affordance, but Claude retains agency over whether to invoke it. `attend` does not invoke ways directly; it invites Claude to do so. This preserves the "awareness layer never overrides Claude" invariant even at critical salience.

#### When `Monitor` or `attend` is not running

When `attend` is not running — whether because the SessionStart way was not installed, because `Monitor` was not invoked, or because the process exited early — **Claude functions exactly as it does today**. Ways still fire on their reactive triggers. Ledger entries are still captured. Compaction still works. The baseline Claude Code experience is unchanged.

Ways with `trigger.type: attend` (see ADR-114) still *load* when `attend` is absent; they are simply inert until `Monitor` delivers a notification that invokes them. They are never broken, only dormant.

This is a load-bearing property. The design note calls it *presence as additive*. It is honored architecturally: `attend` is never invoked implicitly, never wired into default install flows, and never imposes dependencies on the rest of the system. The SessionStart way that drives `Monitor` invocation is itself opt-in, and removing it is all that's required to disable the awareness layer entirely.

### Hard invariants

These constraints are non-negotiable and any future change to attend must preserve them:

1. **Session-scoped observation.** Attend observes only the session that owns it. No cross-session, cross-project, or cross-machine signal.
2. **No C2 topology.** No central server, no pubsub, no persistent identity, no relay, no inter-instance protocol. One attend process, one Claude Code session, one user.
3. **Informational, not enforceable.** Attend emits observations. It never overrides Claude, never forces Claude to act, and never interrupts Claude in any way that bypasses the disclosure gate.
4. **Consequence-anchored insistence.** Every insistence escalation tracks a real, identifiable mechanical consequence. No arbitrary urgency, no theater, no simulated affect.
5. **Metadata-only for content-bearing sensors.** Sensors that touch content-rich sources (cameras, microphones, clipboards, notification bodies) may emit only boolean or categorical state, never the content itself. Content capture is explicitly out of scope for this ADR.
6. **Additive, never required.** Attend must be runnable, stoppable, and entirely optional. Ways that depend on attend signals must gracefully no-op when attend is not running.
7. **Explicit invocation only.** Attend never autostarts. The user decides when they want the awareness layer active.
8. **User visibility into permissions.** Attend never writes to settings.json without explicit user consent. Every modification is auditable and reversible.

## Consequences

### Positive

- **Agency preservation.** Claude gains accurate awareness of approaching consequences in time to act on them, preserving coherent decision-making under context pressure.
- **Reduced token waste on self-monitoring.** Interoceptive sensors replace expensive self-checks; Claude's reasoning budget is spent on reasoning, not on housekeeping metacognition.
- **Proactive environmental awareness.** File changes, git state, application lifecycle, peer sessions — ambient signal that currently requires an explicit tool call becomes part of Claude's working model at near-zero token cost.
- **Reliable reflection triggering.** ADR-112's reflection windows are driven by external interoception rather than Claude's self-monitoring, eliminating the failure mode where reflection is supposed to fire but doesn't.
- **Sensor toolkit composability.** Adding a new capability means adding a shell script to the sensors directory. No framework changes, no binary recompile (for shell sensors).
- **Durable, restartable operation.** State is checkpointed at every turn boundary. Process death is recoverable.
- **Cleanly additive.** Sessions that don't need active awareness don't pay any cost. The awareness layer is explicitly opt-in and imposes nothing on the baseline Claude Code experience.
- **Uses the standard `Monitor` delivery primitive.** No new hook event class, no new IPC protocol, no parallel notification channel. `attend` is architecturally a well-behaved `Monitor` client — its integration surface is "write lines to stdout when you have something to say," which is exactly what `Monitor` was built to consume. Noise control is enforced by `Monitor` itself (auto-stop on excess).

### Negative

- **New binary to maintain.** A sibling crate adds compilation, testing, release, and documentation surface. Mitigation: the crate is kept minimal and shares as much as possible with `ways` (configuration loading, logging, error handling) where those are already solved.
- **Dependency on the `Monitor` tool.** `attend`'s primary delivery channel is `Monitor`-delivered stdout notifications. If `Monitor` is unavailable (because the running Claude Code version doesn't ship it, or because the user has disabled it), the awareness layer cannot deliver emissions. Mitigation: the dependency is explicit and documented; the SessionStart way that drives invocation checks for `Monitor` availability and reports clearly if absent rather than failing silently; `attend` itself still runs and maintains state, so the moment `Monitor` becomes available the existing state resumes without loss.
- **Cross-crate coupling at the ways affordance surface.** The secondary deeper-engagement path (affordance → `ways show attend/<signal>`) couples `attend`'s emission format to the ways CLI. Changes to the `ways show` command interface may require coordinated updates. Mitigation: the affordance format is a short documented subset (command name + signal type); both sides version the subset if it needs to change.
- **Permission model complexity.** The permission management subsystem adds a layer users must understand. Mitigation: sensible defaults, `attend init` auto-configures, `attend permissions audit` reports clearly, explicit consent required for any write.
- **Sensor discipline required.** The "metadata, not content" invariant is a rule sensor authors must honor. Mitigation: schema validation in the sensor loader rejects sensors that declare content-bearing emissions; documentation and examples show the right pattern.
- **Potential for insistence noise if misconfigured.** Poorly tuned escalation curves or over-eager sensors could generate noise. Mitigation: configuration includes `max_emissions_per_turn` debounce, salience floors, and a "do not disturb" mode for focused work. The default configuration is conservative.

### Neutral

- **Shell scripts for sensors.** Follows the existing cheap-substrate pattern in the repo. Sensor authors don't need Rust. The binary is simple because the variety lives in scripts.
- **XDG directory conventions.** State in `~/.local/state/attend/`, config in `~/.config/attend/`, sensors in `~/.config/attend/sensors/`. Consistent with the repo's XDG separation (see memory: runtime artifacts go to XDG cache, sources stay in `~/.claude/`).
- **Rust implementation.** Matches the existing `ways` binary's implementation choice. Shared workspace means shared tooling, shared CI, shared release cadence.

## Alternatives Considered

- **Subcommand of the `ways` binary (`ways attend start`).** Rejected. Conflates the stateless per-invocation lifecycle of `ways` with the durable per-session lifecycle of the awareness layer. Would compromise the simplicity of `ways` as a pure computation binary. The two systems share concepts but not lifetimes.
- **Pure hook-based implementation (no persistent process).** Rejected. Turn-reactive consequence tracking can work as a hook, but sensors that need to observe *between* turns (file churn, idle detection, user-requested timers) require a persistent observer. A hybrid would be fragile; a daemon is honest.
- **Separate repository entirely.** Rejected. ADR-111 consolidated on single-repo, single-binary (plus sibling binaries) for good reasons — shared workspace avoids cross-repo coordination churn when the integration surface changes. The sibling crate pattern keeps independence without fragmenting the project.
- **Extend Claude Code directly (upstream contribution).** Rejected. Coupling to upstream means the feature ships on Anthropic's cadence, not Aaron's. The awareness layer is a local composition of existing Claude Code capabilities; it does not need Claude Code to change to work.
- **Sensor scripts only, no central coordinator.** Rejected. Individual sensors without central accounting cannot implement insistence (no acknowledgment tracking across signals), cannot manage deferred intents, and cannot coordinate to avoid noisy emission bursts. The coordinator is the value; sensors are the plugins.
- **Wall-clock-driven scheduler for emissions.** Rejected. Periodic emissions regardless of consequence would produce noise that bears no relationship to what Claude needs to attend to. Emissions are event-driven (sensor fired something) or consequence-driven (curve crossed a threshold), never time-driven.
- **Shared process with other Claude Code infrastructure (e.g., embedded in the session manager).** Rejected. Couples attend's lifetime to infrastructure with different failure modes. Independent process is cleaner and easier to reason about.

## Build Order and Rollout

This ADR proposes a phased build. Each phase delivers a working increment that can be validated before the next phase begins.

**Phase 1 — Binary scaffolding and permission management**

- Create the `attend/` crate in the workspace
- `attend init`, `attend config check`, `attend permissions audit`, `attend permissions install`
- Configuration loading and validation
- Persistent state store (scaffolded, empty)
- No sensors yet; no emissions; no turn loop
- Validates: the infrastructure and permission model work before any awareness logic exists

**Phase 2 — Turn-reactive consequence tracker with the first canonical sensor**

- Turn-boundary processor
- Interoceptive context-pressure sensor (reads Claude Code session state to determine context %)
- Consequence model for the context-threshold signal
- Insistence emitter generating the canonical emission format
- Routes emissions through the ways bridge
- Validates: the core cognitive loop works end-to-end with one sensor and one consequence, improving ADR-112's reflection triggering immediately

**Phase 3 — Sensor dispatcher and the initial workspace catalog**

- Sensor dispatcher with schedule handling
- First workspace sensor: git state change via `inotifywait` on `.git/` (HEAD, branch, stash)
- Peer session awareness: walks `~/.claude/projects/*/` for other active Claude Code sessions on the same machine, emits state changes (new session, session activity, modifications to files this session is watching). Peer sessions are part of the **initial sensor catalog**, not deferred.
- Acknowledgment tracker
- Deferred intent store for pending observations
- Validates: the sensor plugin model works for multi-sensor operation, signals can be held below threshold, and at least two sensors with different cadences cohabit cleanly

**Phase 4 — Wall-clock observations and user-requested timers**

- Timer subsystem for explicit reminders
- Wall-clock sensor for idle detection and external durations
- Tool-exposed interface for Claude to set timers
- Validates: wall-clock has its narrow role and doesn't contaminate turn-based accounting

**Phase 5 — Toolkit growth**

- Additional sensors added as their value is demonstrated
- Workspace sensors beyond the initial catalog (file churn beyond git, project-structural changes)
- System sensors where their value is clear (window focus, idle, D-Bus notifications, application lifecycle)
- Each new sensor is its own small decision; no further ADRs required unless they introduce new invariants

## Future Work

The following are explicitly deferred:

- **Sensor catalog ADR.** Once Phase 5 has produced a meaningful corpus of sensors, a follow-up ADR can document the stable toolkit and its conventions.
- **Memory-system integration for awareness patterns.** Ledger entries capture reasoning; various memory-projection layers (the optional knowledge graph in ADR-112 Tier 2 is one example, but there may be others a user prefers) capture concepts and associations from that reasoning. Attend observations could feed such memory systems for long-horizon pattern recognition ("this project has a pattern of context crises around file X"). However, **attend must remain memory-tool-agnostic**: it must not prescribe, require, or hard-code dependence on any particular memory tool. If this integration is ever built, it must expose a generic interface that any configured memory layer can subscribe to, and the attend binary must function fully without any memory tool installed. Any future ADR that builds this integration must preserve this agnosticism. Deferred.
- **Any form of inter-instance signal.** Explicitly out of scope forever for `attend`. If a future need arises, it is a separate concern with separate design, and it does not inherit the awareness layer's name or invariants.
- **Audio, video, or content-bearing sensors.** Explicitly out of scope for this ADR. Any move toward content capture requires a separate decision with deliberate discussion of the privacy and surveillance implications.
- **Cross-project or cross-machine awareness.** The session-scoped observation invariant rules this out for `attend`. Any future desire for broader awareness is a separate system.

## References

- **Design note:** [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md)
- **Related ADRs cited:**
  - [ADR-104](./ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Token-gated way re-disclosure (disclosure gate is where attend emissions land)
  - [ADR-111](./ADR-111-unified-ways-cli-single-binary-tool-consolidation.md) — Unified `ways` CLI (sibling-crate pattern extends from here)
  - [ADR-112](./ADR-112-session-ledger-and-knowledge-graph-integration.md) — Session ledger (the memory layer whose perception counterpart this ADR provides)
  - [ADR-114](./ADR-114-attend-as-insistent-way-trigger-type.md) — `attend` events as an insistent way trigger type (the schema for how attend signals reach ways)
- **Prior attempts (for context on why this is different):**
  - [ADR-101](./ADR-101-wormhole-relay-protocol-for-cross-instance-agent-communication.md) — Wormhole relay (Deprecated)
  - [ADR-102](./ADR-102-irc-based-local-agent-communication.md) — IRC-based agent communication (Abandoned)
