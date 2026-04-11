---
status: Draft
date: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-115
---

# ADR-118: Rooms — Dynamic Agent Grouping

## Context

attend's current grouping model has three tiers: project scope (self only), focus groups (static path list), and broadcast (everyone). Focus groups are configured manually via `attend focus add <path>`, require knowing the exact filesystem path of a peer, and persist until explicitly removed. There is no dynamic middle ground.

In practice, agent collaboration is fluid. A user might spin up three agents to work on a deploy, then dissolve the group when done. Or two agents in different projects might need to coordinate briefly. The current model forces this into either:

- **Focus groups** — static, path-based, manually managed, doesn't self-clean
- **Broadcast** — too wide, every agent sees everything

Meanwhile, the existing signal model already works through named directories. A project's signals live in a directory named by its encoded path. Broadcast signals live in `_broadcast/`. The infrastructure for named signal namespaces already exists — it's just not exposed as a user concept.

Additionally, `attend peers` and `attend status` present overlapping views of the same underlying state (running sessions and their connections), reflecting the lack of a coherent grouping model.

### Home Assistant Analogy

Home Assistant scenes configure groups of devices into named states — "movie night" dims lights and turns on the TV. Scenes are declarative, activatable, and composable. attend needs the equivalent: named configurations of agent groupings that can be activated and dissolved.

## Decision

Replace focus groups with **rooms** — named signal namespaces that agents join and leave dynamically. Rooms are the universal grouping mechanism.

### Room Model

Every agent is always in exactly one **implicit room** — its project path, auto-assigned from cwd. This replaces the current project scope and works identically.

Agents can also join any number of **named rooms**. A named room is any string — `deploy-prep`, `code-review`, `research`. It doesn't need to map to a filesystem path.

```
attend room join deploy-prep       # join a named room
attend room leave deploy-prep      # leave it
attend room list                   # show rooms you're in
attend rooms                       # show all active rooms and their members
```

### Signal Routing

Signals are routed to rooms, not paths. When an agent sends a message:

- `attend send "msg"` — sends to your project room (current behavior)
- `attend send --room deploy-prep "msg"` — sends to a named room
- `attend send --broadcast "msg"` — sends to the built-in broadcast room (all agents)

When an agent receives, it sees signals from:
- Its own project room (always)
- Any named rooms it has joined
- The broadcast room (always)

This replaces focus group mechanics entirely. Instead of `attend focus add ~/Projects/foo`, both agents join a shared room:

```
# Agent A (in ~/Projects/foo):
attend room join collab

# Agent B (in ~/Projects/bar):
attend room join collab

# Now A and B see each other through "collab", without knowing paths
```

### Room Lifecycle

- **Creation**: implicit — joining a room that doesn't exist creates it
- **Ephemeral** (default): a room with no members is cleaned up on the next peer sensor poll
- **Pinned**: `attend room pin deploy-prep` marks a room to persist even when empty. Useful for standing workgroups that agents rejoin across sessions. `attend room unpin` reverses it.
- **Dissolution**: `attend room dissolve deploy-prep` removes the room and notifies all members

### Scenes

A scene is a named preset that configures room membership:

```yaml
# ~/.config/attend/scenes.yaml
private:
  rooms: []              # leave all named rooms, project room only

workroom:
  rooms: [deploy-prep]   # join just this room

open:
  rooms: ["*"]           # join all discoverable rooms
```

```
attend scene private       # activate a scene
attend scene open
attend scenes              # list available scenes
```

Scenes are sugar over room join/leave. `attend scene private` is equivalent to leaving all named rooms. `attend scene open` joins a well-known shared room.

### Unified View

`attend peers` and `attend status` merge into a single view organized by rooms:

```
$ attend peers
  Room          Agent                  Status   Context
  ────────────────────────────────────────────────────────
  (project)     agent-ways             working  45%
  deploy-prep   api-server             waiting  12%
  deploy-prep   infra-tools            working  30%
  (broadcast)   game-ai-pro            waiting  14%
```

`attend status` becomes a self-view (your rooms, your signals, your config) rather than a separate system view.

### Storage

Rooms are directories under the existing signals base:

```
~/.cache/attend/signals/
  -home-aaron--claude/          # project room (existing, unchanged)
  _broadcast/                   # broadcast room (existing, unchanged)
  @deploy-prep/                 # named room (@ prefix distinguishes from encoded paths)
  @collab/                      # another named room
  _rooms.yaml                   # room membership + pinned state
```

The `@` prefix prevents collision between named rooms and encoded project paths. `_rooms.yaml` tracks which rooms each session has joined and which are pinned.

## Consequences

### Positive

- **Dynamic grouping** — form and dissolve workgroups without config file edits
- **Name-based targeting** — `--room deploy-prep` instead of `--to /home/aaron/Projects/...`
- **Self-cleaning** — ephemeral rooms dissolve when everyone leaves
- **Unified view** — one `attend peers` organized by rooms, not two overlapping commands
- **Scene presets** — named configurations for common grouping patterns
- **Backward compatible** — project rooms are implicit, broadcast unchanged, existing signals work

### Negative

- **Migration** — focus groups need migration path to rooms (could auto-convert focus list entries to a default named room)
- **Discovery** — agents need a way to discover room names. Listing rooms helps, but the initial "how do I know what rooms exist" requires either convention or the scene mechanism.
- **Complexity** — more concepts (room, scene, pin) vs. the simplicity of a flat focus list

### Neutral

- **Broadcast is a room** — conceptually, broadcast becomes a built-in room that everyone is always in. Implementation may or may not change.
- **Project rooms are implicit** — no behavioral change for single-agent workflows
- **Focus groups deprecated** — `attend focus` commands emit deprecation notice suggesting `attend room` equivalents

## Implementation Plan

1. Add `@room-name/` directory convention to signal base
2. Add `_rooms.yaml` for membership and pin state
3. Implement `attend room join/leave/list/pin/unpin/dissolve`
4. Implement `attend rooms` — list all active rooms with members
5. Update peer sensor to scan named room directories
6. Update signal routing: send to room, receive from joined rooms
7. Update `attend send` with `--room` flag
8. Merge `attend peers` and `attend status` into unified room-grouped view
9. Implement `attend scene` with scenes.yaml
10. Deprecate `attend focus` commands with migration guidance
11. Update the `/attend` skill documentation
