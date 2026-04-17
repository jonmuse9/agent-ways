---
description: project-specific gated configuration
vocabulary: gated project specific configuration
pattern: (?i)(code|project|gated|configuration|quality)
scope: agent
when:
  project: /tmp/test-project-sim
curve:
  type: Exponential
  half_life: 30000
---
# Gated Way

Only fires when the project is /tmp/test-project-sim.
