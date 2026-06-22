---
description: when Claude Code is set to a non-English language but agent-ways is still running in English, surfacing the ways-localize option
vocabulary: localize localization translate language non-english multilingual locale internationalization i18n spanish french german japanese chinese
trigger: session-start
macro: prepend
scope: agent
refire: 0.15
requires: ["Bash(jq:*)", "Bash(tr:*)"]
---
