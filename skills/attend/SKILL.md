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

Send defaults to your current scope (own project + focus group). Always wrap the message in double quotes to prevent shell metacharacter expansion (`?`, `*`, `!`).

```bash
attend send "your message here"
attend send --broadcast "important announcement for all sessions"
attend send --to /home/user/other-project "directed message"
```

### Focus groups

A focus group is a set of peer project directories you listen to and send to. Send scope mirrors receive scope.

```bash
attend focus list                    # show current focus group
attend focus add ~/Projects/foo      # add a peer project
attend focus add ~/temp ~/bar        # add multiple at once
attend focus remove ~/temp           # remove a peer
attend focus clear                   # back to project-only mode
```

### Discovery and status

```bash
attend peers                         # list active Claude Code sessions
attend status                        # running instances, pending signals, focus state
```

### Stopping

Use TaskStop with the Monitor's task ID. Attend exits cleanly on signal.

## Arguments

- `/attend` — start the sensor loop via Monitor (default)
- `/attend status` — run `attend status` via Bash
