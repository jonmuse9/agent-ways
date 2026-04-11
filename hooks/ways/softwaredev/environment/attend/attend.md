---
description: attend binary — active awareness sensor loop, peer session discovery, inter-session signaling for Claude Code
vocabulary: attend attend-run attend-send attend-focus attend-peers attend-status attend-scene sensor-loop awareness-layer focus-group signal-file disclosure-governor peer-session peer-discovery session-awareness environmental-sensing inter-session claude-session another-claude scene-private scene-open
embed_threshold: 0.28
threshold: 2.0
pattern: attend|awareness.?layer|peer.?session|peer.?discover|signal.?file|focus.?group|sensor.?loop
commands: attend
redisclose: 15
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
attend send "your message here"            # send to project + focus groups
attend send --focus deploy "message"       # send to a focus group
attend send --broadcast "important news"   # send to all sessions
```

## Focus Groups

Named groups for shared signal routing. Dynamic — join and leave as needed.

```bash
attend focus on deploy                     # focus on a named group
attend focus on infra --pin                # focus + persist when empty
attend focus off deploy                    # release focus
attend focus list                          # show your groups
attend focus clear                         # project-only mode
```

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
