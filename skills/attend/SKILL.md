---
name: attend
description: Start the active awareness layer. Launches attend as a persistent Monitor that surfaces environmental changes (git state, peer sessions, process activity) as notifications. Use when the user says "attend", "start awareness", "watch my session", or invokes /attend.
allowed-tools: Bash, Monitor, Read
---

# Attend — Active Awareness

## Step 1: Pre-flight

Check that `attend` is installed:

```bash
command -v attend
```

If not found, tell the user to run `make attend` or `make install` from the agent-ways repo. Stop here.

## Step 2: Launch via Monitor

**CRITICAL: You MUST use the Monitor tool, NOT Bash.** Running attend via Bash blocks the tool call and discards notifications. Monitor is the only correct way to launch attend.

Call the Monitor tool with exactly these parameters:

- **command**: `attend`
- **description**: `attend: git, peers, processes`
- **persistent**: `true`
- **timeout_ms**: `3600000`

Do NOT run `attend` with the Bash tool. Do NOT use `run_in_background`. Only Monitor delivers stdout lines as async notifications into the conversation.

## What happens next

Attend polls three sensors on adaptive schedules:

- **processes** — application presence (not PID churn)
- **git** — dirty files, branch changes, upstream divergence
- **peers** — other Claude Code sessions via `~/.claude/sessions/`

Baselines are established silently on first poll (stderr only, not surfaced). Notifications arrive only when sensors detect meaningful state changes, rate-limited to ~3 per 2-minute window by the disclosure governor.

Each notification is a single line: `[attend sensor=NAME priority=LEVEL] description`

## Stopping

Use TaskStop with the Monitor's task ID. Attend exits cleanly on signal.

## Arguments

- `/attend` — start attend (default)
- `/attend status` — check if attend is already running: `ps -eo pid,comm | grep attend`
