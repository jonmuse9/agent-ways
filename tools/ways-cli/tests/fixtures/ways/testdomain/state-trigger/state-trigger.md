---
description: session start state triggered way
vocabulary: state session startup
trigger: session-start
scope: agent
curve:
  type: Exponential
  half_life: 30000
---
# State Trigger Test Way

This way fires once per session via the session-start trigger.
