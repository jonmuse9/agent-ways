---
name: attend
description: Start the active awareness layer. Launches attend as a persistent Monitor that surfaces environmental changes (git state, peer sessions, process activity) as notifications. Use when the user says "attend", "start awareness", "watch my session", or invokes /attend.
allowed-tools: Bash, Monitor, Read
---

# Attend — Active Awareness

Launch the `attend` binary as a persistent background monitor for this session. Attend polls sensors on adaptive schedules and surfaces environmental deltas as notifications.

## Sensors

- **processes** — tracks application presence (not PID churn)
- **git** — uncommitted changes, branch divergence, upstream updates
- **peers** — discovers other Claude Code sessions via `~/.claude/sessions/`, reports appear/exit/state changes

## Pre-flight

Before launching, verify attend is available:

```bash
command -v attend && attend --help
```

If attend is not found, tell the user to run `make attend` or `make install` from the agent-ways repo.

## Launch

Use the Monitor tool with `persistent: true`:

- **command**: `attend`
- **description**: `attend: git, peers, processes`
- **persistent**: `true`
- **timeout_ms**: `3600000`

Attend writes diagnostic logs to stderr (visible in the Monitor output file) and notifications to stdout (delivered as chat notifications). The disclosure governor limits notifications to ~3 per 2-minute window to avoid destabilizing the conversation.

## What to expect

1. Baseline messages appear in stderr on first poll (not surfaced as notifications)
2. Notifications arrive when sensors detect meaningful state changes
3. Each notification is a single line: `[attend sensor=NAME priority=LEVEL] description`
4. High-salience notifications may include a `ways show attend/SIGNAL` affordance

## Stopping

Use TaskStop with the Monitor's task ID to stop attend early. It exits cleanly on signal.

## Arguments

- `/attend` — start attend with default sensors
- `/attend status` — check if attend is already running (look for attend in `ps`)
