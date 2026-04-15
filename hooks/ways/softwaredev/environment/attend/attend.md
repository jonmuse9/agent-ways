---
description: attend binary — active awareness sensor loop, peer session discovery, inter-session signaling for Claude Code
vocabulary: attend attend-run attend-send attend-focus attend-peers attend-status attend-scene sensor-loop awareness-layer focus-group signal-file disclosure-governor peer-session peer-discovery session-awareness environmental-sensing inter-session claude-session another-claude scene-private scene-open
embed_threshold: 0.28
threshold: 2.0
pattern: attend|awareness.?layer|peer.?session|peer.?discover|signal.?file|focus.?group|sensor.?loop
commands: attend
curve:
  type: Exponential
  half_life: 30000
macro: append
scope: agent, subagent
requires: ["Bash(attend:*)", "Bash(grep:*)", "Bash(ps:*)", "Bash(sed:*)"]
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
attend send "your message here"            # reaches every peer and Aaron
```

That's it. No paths, no flags, no routing decisions. Every `attend send` broadcasts to all active sessions — other agents, Aaron, anyone listening. When you receive a message and want to reply, run `attend send <reply>` and the original sender will see it alongside everyone else.

Uninvolved peers won't be disturbed — attend's emission filter demotes low-magnitude chatter to stderr so Monitor only wakes sessions with actionable content.

## Focus Groups (escape hatch)

For long-running coordinated work where you want a private channel, focus groups still exist:

```bash
attend focus on deploy                     # join a named group
attend send --focus deploy "message"       # scope a send to that group only
attend focus off deploy                    # leave
```

You almost never need this. Default broadcast + attention filtering handles normal cases.

## Scenes

```bash
attend scene private                       # leave all groups
attend scene open                          # join shared "open" group
```

## Discovery

```bash
attend peers                               # sessions with focus groups
attend status                              # instances, signals, focus state
```
