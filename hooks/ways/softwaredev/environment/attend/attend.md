---
description: Active awareness layer — environmental sensing and peer signaling for Claude Code sessions
vocabulary: attend awareness monitor sensor peer signal focus notify observe process watch session
threshold: 2.0
pattern: attend|awareness|peer.?session|peer.?message|signal|focus.?group
commands: attend
redisclose: 15
macro: append
scope: agent, subagent
---
<!-- epistemic: tool-knowledge -->
# Attend

`attend` is the active awareness module for Claude Code sessions. It runs as a persistent background process via Monitor, polling sensors on adaptive schedules and surfacing environmental changes as notifications.

## Starting

Use `/attend` or launch manually via Monitor with `attend run`.

## Sensors

- **git** — dirty files, branch changes, upstream divergence
- **peers** — discovers other Claude Code sessions, reads peer signals
- **processes** — application presence (not PID churn)

## Peer Messaging

```bash
attend send your message here              # send to focus scope
attend send --broadcast important news     # send to all sessions
attend send --to /path/to/project message  # directed to one project
```

Send scope mirrors receive scope — if you have a focus group, messages go to all group members.

## Focus Groups

```bash
attend focus add ~/Projects/foo            # join a focus group
attend focus list                          # show current group
attend focus clear                         # project-only mode
```

## Discovery

```bash
attend peers                               # list active Claude sessions
attend status                              # instances, signals, focus state
```
