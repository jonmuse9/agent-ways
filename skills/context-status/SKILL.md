---
name: context-status
description: Check how much context window remains in this session — token budget, usage, and room left before compaction. Use when the user asks how much context/room/budget is left, or wants a context-usage gauge.
allowed-tools: Bash
---

# Context Status

Run the context usage command and capture the JSON output:

```bash
ways context --json
```

Then render a visual gauge using the chart tool. Build the JSON and pipe it:

```bash
# Parse the values from the --json output, then:
echo '{"type":"hbar","data":{"Used (PCT%)":USED,"Free (RPCT%)":REMAINING},"title":"Context: USEDk / TOTALk tokens (MODEL)","width":60,"format":"human"}' \
  | ~/.claude/hooks/ways/softwaredev/visualization/charts/chart-tool
```

Replace `USED`, `REMAINING`, `TOTAL`, `PCT`, `RPCT`, and `MODEL` with actual values from the JSON output. Use `jq` to extract them.

The command auto-detects the context window size from the model in the transcript (it varies by model). Override with `CLAUDE_CONTEXT_WINDOW` env var.

If the remaining percentage is below 20%, mention that compaction is approaching and suggest wrapping up or prioritizing remaining work.

## Not for

- Changing the context window size or compaction threshold — that's `CLAUDE_CONTEXT_WINDOW` and the compaction settings, not this skill.
- General session status — it reports the context window only.
- A continuous watch — it's a one-shot snapshot; run it again for a fresh reading.
