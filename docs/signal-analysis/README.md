# Signal Analysis

Empirical characterization of the ways matcher's score distributions. The
embedding models (EN-only 384-dim and multilingual 768-dim) produce scores
in different distributions; threshold calibration needs to see those
distributions, not guess at them.

## Generating a report

```bash
scripts/signal-report.py
```

Writes three artifacts to this directory:

- `scores.csv` — raw (prompt, lang, expected_way, way_id, model, score) rows
- `score-distributions.png` — histograms of signal vs noise per model,
  with the current thresholds drawn
- `per-prompt-gap.png` — per-prompt bar chart of the expected way's score
  vs the top competing way, for both models

Swap in your own battery with `--prompts file.jsonl` where each line is
`{"lang": "...", "expected_way": "...", "prompt": "..."}`.

## How to read the plots

**score-distributions.png** — two columns, EN and multi. Signal
(expected-way hits) in green, noise (everything else) in red. The black
dashed line is the current threshold. Good calibration puts the line in
the trough between the two distributions.

**per-prompt-gap.png** — one row per prompt. Blue bars are EN scores,
orange are multi. For each prompt, you see the expected way's score
and the top competing way. A healthy prompt has the expected way well
above any competitor AND above the per-model threshold (dotted line).

## What to look for

- **Signal peak below threshold**: the threshold is too high; real matches
  get cut. Lower it or sharpen the stubs.
- **Noise tail above threshold**: the threshold is too low; FPs fire.
  Raise it or sharpen the confuser stubs.
- **Signal and noise overlap significantly**: no threshold can cleanly
  separate them. The fix is stub-level, not threshold-level — use
  `ways tune` to find the confusers and re-author.
- **Multi-column dominance for English queries**: surprising. English
  queries should usually win in EN. If multi wins, either the English
  stub is weak, or the query has non-English content.

## Current baseline (2026-04-17)

From the default 16-prompt battery against the current corpus (ADR-125
matcher with per-model thresholds):

| model | signal min | signal mean | noise p95 | noise p99 | threshold |
|-------|-----------:|------------:|----------:|----------:|----------:|
| EN    | 0.01*      | 0.36        | 0.24      | 0.31      | 0.40      |
| multi | 0.54       | 0.67        | 0.45      | 0.59      | 0.55      |

\* signal_min 0.01 for EN comes from non-English prompts expecting a
match; the EN model doesn't understand them, which is correct — those
prompts are picked up by the multi path instead.

The thresholds sit in the gap between signal_min and noise_p95 for
both models. Multi has ~1% p99 leakage (top 1% of noise exceeds 0.55),
which is acceptable; most of those are genuinely-ambiguous stubs that
`ways tune` flags as discrimination problems.
