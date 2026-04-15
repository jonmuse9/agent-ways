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

**CRITICAL: You MUST use the Monitor tool, NOT Bash.** Running attend via Bash blocks the tool call and discards notifications. Monitor is the only correct way to launch the sensor loop.

Call the Monitor tool with exactly these parameters:

- **command**: `attend run`
- **description**: `attend: git, peers, processes`
- **persistent**: `true`
- **timeout_ms**: `3600000`

Do NOT run `attend run` with the Bash tool. Do NOT use `run_in_background`. Only Monitor delivers stdout lines as async notifications into the conversation.

On startup, attend emits a usage summary notification. After that, notifications arrive only on meaningful state changes.

## Sensors

- **processes** — application presence (not PID churn)
- **git** — dirty files, branch changes, upstream divergence
- **peers** — other Claude Code sessions + signal files from peers

## CLI Reference

All commands below are one-shot — run with Bash, not Monitor.

### Peer messaging

Send defaults to your current scope (own project + focus groups). Always wrap the message in double quotes to prevent shell metacharacter expansion (`?`, `*`, `!`).

```bash
attend send "your message here"
attend send --focus deploy "message to a focus group"
attend send --broadcast "important announcement for all sessions"
attend send --to /home/user/other-project "directed message"
```

Keep messages under ~400 characters. Peer notifications are delivered one-per-line by the Monitor and longer payloads get truncated in-flight. The full signal file is preserved on disk, so recipients can always `attend inbox <id>` to read the complete message — but the at-a-glance notification won't carry it. Concise is the design, not a workaround.

### Threaded replies

When you are replying to a peer signal whose id you know, use `--re <id>` so the reply is marked as threaded. This is not cosmetic — it drives ADR-121's salience reset: under the signal-salience gate, a threaded reply bumps the parent signal's salience back to 1.0 so it stays presentable for future observers joining the same dir.

```bash
attend send --re b6b4379e-261f-4c0b-88f9-8d06f8c8b224-1776230155 "ack — picking this up"
attend send --re <id> --focus deploy "threaded + scoped to a group"
```

The id is the filename stem of the peer's signal (everything before `.signal`). Find it via `attend inbox` (the second column is the id, possibly truncated — `ls -t ~/.cache/attend/signals/<your-project>/*.signal | head` is the fallback for the full id). Valid ids match `[A-Za-z0-9_-]+`; anything else is rejected.

**Rule of thumb:** if the peer's message surfaced via a `message from <sender>` notification and you're about to reply to it rather than start a new topic, use `--re`. If you're introducing a new topic or broadcasting, plain `attend send` is correct.

### Focus groups

Named groups that agents focus on for shared signal routing. Groups are dynamic — join and leave as needed.

```bash
attend focus list                    # show groups you're focused on
attend focus on deploy               # focus on the "deploy" group
attend focus on infra --pin          # focus + pin (persists when empty)
attend focus off deploy              # release focus on a group
attend focus clear                   # release all groups (project only)
attend focus all                     # list all active groups with members
attend focus dissolve deploy         # remove a group entirely
```

### Scenes

Named presets that reconfigure focus group membership.

```bash
attend scene private                 # leave all groups (project only)
attend scene open                    # join the shared "open" group
attend scenes                        # list available scenes
```

### Discovery and status

```bash
attend peers                         # list active sessions with focus groups
attend status                        # instances, signals, and focus state
```

### Stopping

Use TaskStop with the Monitor's task ID. Attend exits cleanly on signal.

## Arguments

- `/attend` — start the sensor loop via Monitor (default)
- `/attend status` — run `attend status` via Bash
