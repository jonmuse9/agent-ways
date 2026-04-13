# Attend and Monitor

This directory documents the active awareness layer — how `attend` and Claude Code's `Monitor` tool combine to surface environmental changes as async notifications into a running session.

## The pair

**Attend** is a sensor loop that watches the local environment (git state, peer sessions, context pressure, process activity, peer messages) and emits human-readable observations when something crosses a threshold. It runs as a single long-lived process inside the Claude Code session.

**Monitor** is the Claude Code tool feature that launches a command and delivers each line of its stdout as an asynchronous notification into the conversation. The conversation receives the observations as they happen instead of blocking on a foreground tool call.

Monitor is a general-purpose awareness channel, not an attend-specific thing. Its intended use is **any long-running process that produces unpredictable state updates** the agent needs to react to: a multi-minute build emitting compile errors, a watch-mode test runner flipping between green and red, a deploy pipeline reporting stage transitions, a custom event log tailing whatever the current task cares about. Anything that happens on its own timeline and matters mid-turn is a candidate.

The common thread across those cases is **sporadic unpredictable state** — changes the agent didn't cause, arriving at unknown times, that still need to land in the conversation in time to affect the next decision. Every Monitor use case fits this shape; they just source their state from different things. A build watcher sources from one process's stdout. A test runner sources from filesystem watches plus child-process exit codes.

**Attend is one more class of sporadic unpredictable state** — specifically, the one that comes from **complex interactions across peer agents plus ambient environmental awareness** (git, processes, context pressure, focus groups). The state isn't one process's output; it's an ecosystem's. Attend aggregates that ecosystem into a single stream of observations that Monitor can deliver.

Attend without Monitor is a CLI that prints to a dead terminal. Monitor without attend is a generic stream relay — useful in its own right for build watchers and log tails, but without attend it has no opinion about *what* multi-agent ecosystem state should stream. Together they form the **awareness channel** — the mechanism by which the agent learns about reality changes it didn't cause.

If you're coming from the hooks-and-ways world, the analogy is exact: `hooks` are Claude Code's harness mechanism for *synchronous* prompts against the model's decisions; `Monitor` is the harness mechanism for *asynchronous* observations about the world. `ways` are what runs synchronously through hooks; `attend` is what runs asynchronously through Monitor.

## What lives in this directory

| File | Purpose |
|---|---|
| [`README.md`](README.md) | Orientation — you are here |
| [`loop.md`](loop.md) | The sensor loop — state diagrams, timing, signal flow |
| [`sensors.md`](sensors.md) | What each built-in sensor observes and how it emits (planned) |
| [`signals.md`](signals.md) | Signal file format, storage layout, lifecycle (planned) |
| [`engagement.md`](engagement.md) | Action potential model in prose and diagrams (planned) |
| [`salience.md`](salience.md) | Turn-based presentation decay — the ADR-121 mechanism (planned) |
| [`focus-groups.md`](focus-groups.md) | Dynamic group membership and signal routing (planned) |
| [`configuration.md`](configuration.md) | Config schema, overlay semantics, tuning workflow (planned) |

Files marked **planned** are part of the ongoing documentation pass. Start with `loop.md` — everything else threads through it.

## Reading order

1. **[`loop.md`](loop.md)** — if you only read one file, read this. The sensor loop is the substrate that everything else rides on.
2. **`sensors.md`** — once you understand the loop, individual sensors make sense as pluggable units inside it.
3. **`engagement.md`** — the action potential model governs *when* sensors fire. Read this after sensors.
4. **`signals.md`** — the wire format and storage layout for peer messages and notifications.
5. **`focus-groups.md`** — how groups scope signal routing between agents.
6. **`salience.md`** — presentation-layer aging; orthogonal to all the above.
7. **`configuration.md`** — reference manual for the YAML surface.

## Related docs

- **ADR-113** (`docs/architecture/system/`) — the original decision to build attend as an active awareness module
- **ADR-114** — attend as an insistent trigger type for ways
- **ADR-115** — declarative config with project-scope overlay
- **ADR-116** — permission requirements
- **ADR-117** — sensor crate extraction
- **ADR-118** — focus groups, dynamic agent grouping
- **ADR-119** — action potential engagement model
- **ADR-121** — salience decay for signal presentation (draft)
- `docs/hooks-and-ways/` — sibling docs for the synchronous hook mechanism
- `docs/design-notes/cognitive-loop-and-awareness-layer.md` — earlier design exploration that informed ADR-113

## Where the code lives

- `tools/attend/` — orchestrator, config, groups, state, CLI
- `tools/sensor-trait/` — base `Sensor` trait, `SensorSlot`, engagement state
- `tools/sensor-context/`, `sensor-git/`, `sensor-peers/`, `sensor-processes/` — the built-in sensors
- `tools/agent-fmt/` — shared terminal formatting (banners, tables, commands)
- `skills/attend/SKILL.md` — the invocation skill the agent reads when the user asks for awareness
- `hooks/ways/softwaredev/environment/attend/` — the way that surfaces live attend state in the steering layer
