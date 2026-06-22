---
name: ways-tests
description: Score way matching via embedding similarity, analyze vocabulary, and validate frontmatter. Use when testing how well a way matches prompts, checking cosine similarity scores, inspecting the embedding engine status, or validating way files.
allowed-tools: Bash, Read, Glob, Grep, Edit
---

# ways-tests: Way Matching & Vocabulary Tool

Test how well a way matches sample prompts, analyze vocabulary for gaps, and validate frontmatter.

## Usage

```
/ways-tests score <way> "prompt"          # Score one way against a prompt
/ways-tests score-all "prompt"            # Rank all ways against a prompt
/ways-tests suggest <way>                 # Analyze vocabulary gaps
/ways-tests suggest <way> --apply         # Update vocabulary in-place
/ways-tests suggest --all [--apply]       # Analyze/update all ways
/ways-tests lint <way>                    # Validate frontmatter
/ways-tests lint --all                    # Validate all ways
/ways-tests check <check> "context"       # Test check scoring curve
/ways-tests check-all "context"           # Rank all checks against context
/ways-tests tree <path>                   # Analyze progressive disclosure tree structure
/ways-tests budget <path>                 # Token cost analysis for a way tree
/ways-tests jaccard <tree>                # Sibling vocabulary isolation for a tree
/ways-tests jaccard <way1> <way2>         # Vocabulary overlap between two specific ways
/ways-tests crowding "prompt"             # Detect vocabulary crowding across all ways
/ways-tests compare <path1> <path2>       # Side-by-side tree metrics comparison
/ways-tests metrics                       # Show tree disclosure metrics for current session
/ways-tests embed-status                  # Embedding engine health dashboard
/ways-tests embed-score <way> "prompt"   # Cosine similarity score for one way
/ways-tests embed-score-all "prompt"     # Cosine similarity ranking across all ways
```

## Not for

- Authoring or editing ways themselves — that's the **ways** skill (`/ways`).
- Updating the agent-ways install — that's **ways-update**.
- This skill only *measures and validates*: scoring, vocabulary analysis, frontmatter and tree lint.

## Engine

The matching pipeline uses embedding-based semantic scoring as the sole retrieval tier. The embedding model is a hard dependency of `ways`.

```
Embedding (~20ms batch, all-MiniLM-L6-v2)
  ways embed: cosine similarity, 0–1 scale
  Threshold field: embed_threshold (per-way, default: 0.35)
  Fires when: cosine(query, way) >= embed_threshold
```

To check engine health: `ways status`

## Embed-Status Mode

Show the embedding engine health dashboard:

```bash
ways status
```

Reports:
- Engine status (embedding healthy / degraded / missing)
- Embedding binary path and version
- Model path and size (`minilm-l6-v2.gguf`)
- Corpus state: total ways, how many have pre-computed embeddings, size
- Manifest freshness (staleness detection)
- Per-project: inclusion marker state, staleness, embedded count

**If the engine reports degraded or missing**, diagnose:
- Binary missing → `make setup` in `~/.claude`
- Model missing → `make setup` downloads it to `~/.cache/claude-ways/user/`
- Corpus missing or stale → `ways corpus` (or `make corpus`)

## Embedding Score Mode

Score a way using the embedding engine directly.

**Step 1 — Ensure corpus is fresh:**

```bash
ways corpus
```

This regenerates `~/.cache/claude-ways/user/ways-corpus.jsonl`. The corpus includes pre-computed embeddings for all semantic ways (`description` + `vocabulary` fields present). Run this after adding or editing any way.

**Step 2 — Score all ways against a prompt (batch, ~20ms):**

```bash
WAY_EMBED="${HOME}/.cache/claude-ways/user/way-embed"
CORPUS="${XDG_CACHE_HOME:-$HOME/.cache}/claude-ways/user/ways-corpus.jsonl"
MODEL="${XDG_CACHE_HOME:-$HOME/.cache}/claude-ways/user/minilm-l6-v2.gguf"

"$WAY_EMBED" match \
  --corpus "$CORPUS" \
  --model  "$MODEL" \
  --query  "your prompt here"
```

Output is `id<TAB>score` for each way whose cosine similarity meets its `embed_threshold`. Example:

```
softwaredev/security	0.6821
softwaredev/api	0.5104
itops/runbooks	0.4312
```

**Step 3 — Score a single way (look up by id):**

Ways are identified in the corpus by their path relative to the ways root (e.g., `softwaredev/security`). After running the batch command, grep for the target id:

```bash
"$WAY_EMBED" match \
  --corpus "$CORPUS" \
  --model  "$MODEL" \
  --query  "your prompt here" | grep -F "softwaredev/security"
```

**Overriding per-way threshold for exploration:**

```bash
"$WAY_EMBED" match \
  --corpus "$CORPUS" \
  --model  "$MODEL" \
  --query  "your prompt here" \
  --threshold 0.1    # show everything above 0.1 cosine similarity
```

Use `--threshold 0.1` to see the full similarity landscape when debugging misses.

### Interpreting Cosine Similarity Scores

| Range | Meaning |
|-------|---------|
| >= 0.7 | Strong match — semantically close |
| 0.5–0.7 | Moderate match — related domain |
| 0.35–0.5 | Weak match — at or near default threshold |
| < 0.35 | Below default threshold — no match |

The default `embed_threshold` is **0.35**. Per-way overrides live in the way's frontmatter:

```yaml
---
description: "..."
vocabulary: "..."
embed_threshold: 0.45   # raise to require stronger match
---
```

Raise `embed_threshold` to suppress weak false positives. Lower it (toward 0.25) for broader catch on niche topics.

**Parent-boost in sessions (ADR-125).** At scan time (not in this skill's scoring), a child way's effective threshold is multiplied by `config.parent_threshold_multiplier` (default 0.8) when any ancestor way has fired in the session. Children within active parent domains fire on weaker signal — the session-subgraph mechanism behind progressive disclosure. See [docs/hooks-and-ways/matching.md](../../docs/hooks-and-ways/matching.md).

**Locale alias audit.** For multilingual stubs, use `ways tune` to surface fidelity (do sibling translations agree?) and discrimination (is another way's alias winning against yours?) problems. Does not write thresholds — only reports. Full workflow in the [`knowledge/optimization/tuning`](../../hooks/ways/meta/knowledge/optimization/tuning/tuning.md) way.

### Embedding Cross-Way Ranking

When using `embed-score` on a single way, also run the full batch and present the top results as a ranked table:

```
=== "add a make target for linting" ===

Engine: embedding (auto)

Target: softwaredev/environment/makefile
  Cosine: 0.5821  embed_threshold: 0.35  Result: MATCH

Cross-way ranking (top by cosine similarity):
  Cosine  Thr   Match  Way
  ------  ----  -----  ---
  0.5821  0.35  YES    softwaredev/environment/makefile  ← target
  0.4102  0.35  YES    documentation/standards
  0.2891  0.35  no     softwaredev/environment/deps
  ...

Assessment: Clean win. Target leads by 0.17 cosine points.
```

Flag these patterns:
- **Overlap**: Two ways both match with cosine scores within 0.05 of each other → potential conflict
- **False dominance**: Another way has a higher cosine than the target → vocabulary or description may need tuning
- **Healthy co-fire**: Both match but serve complementary purposes → note as expected

## Resolving Way Paths

When the user gives a short name like "security" instead of a full path:
1. Check `$CLAUDE_PROJECT_DIR/.claude/ways/` first (project-local)
2. Then check `~/.claude/hooks/ways/` recursively for `*/security/security.md`
3. If multiple matches, list them and ask the user to pick

## Score Mode

Use the embedding engine for all scoring operations. Regenerate the corpus before scoring so changes to descriptions and vocabulary are reflected:

```bash
ways corpus
```

Use the `ways embed` subcommand to score a prompt against the full corpus:

```bash
ways embed --query "$prompt"
# Prints top-N ways with cosine similarity scores.
# A way fires when its score meets or exceeds its per-way embed_threshold.
```

### Cross-Way Context (automatic)

**When scoring a single way, always include cross-way context.** After showing the target way's score, automatically run a score-all for the same prompt and display the top 5-8 ways as a ranking table. This answers the real questions:

- Does this way **win** when it should?
- Does it **defer** to the right way when another is more specific?
- Are there **unhealthy overlaps** where two ways compete at similar scores?
- Do any **unexpected ways** fire that shouldn't?

Present as:

```
=== "add a make target for linting" ===

Target: softwaredev/environment/makefile
  Score: 0.6213  Threshold: 0.35  Result: MATCH

Cross-way ranking:
  Score   Thr    Match  Way
  ------  -----  -----  ---
  0.6213  0.35   YES    softwaredev/environment/makefile  ← target
  0.2904  0.35   no     documentation/standards
  0.1102  0.35   no     softwaredev/environment/deps
  ...

Assessment: Clean win. No competing ways above threshold.
```

Flag these patterns:
- **Overlap**: Two ways both match with scores within 0.05 cosine of each other → potential conflict
- **False dominance**: Another way scores higher than the target → the target may need vocabulary tuning
- **Healthy co-fire**: Both match but serve complementary purposes → note as expected

## Score-All Mode

Run `ways embed --query "$prompt"` once — it scores the query against all pre-computed corpus embeddings in a single batch call (~20ms). No per-way loop needed. Output is already ranked by cosine similarity:

```
Score   Threshold  Match  Way
------  ---------  -----  ---
0.5421  0.35       YES    softwaredev/security
0.4017  0.35       YES    softwaredev/api
0.2983  0.35       no     softwaredev/debugging
```

Include ways that have pattern matches too (mark those as "REGEX" in the Match column).

### Prompt Battery (automatic for score-all)

When running score-all without a specific prompt, or when the user asks for a broad evaluation, generate a battery of 8-12 diverse prompts that stress-test coverage:

- 2-3 prompts that should clearly match one specific way
- 2-3 prompts that should trigger healthy co-fires (multiple ways relevant)
- 2-3 prompts at the boundary (could go either way)
- 2-3 prompts that shouldn't match any way strongly

This gives a landscape view of how the way ecosystem behaves.

## Suggest Mode

Use the `ways suggest` subcommand:

```bash
ways suggest --file "$wayfile" --min-freq 2
```

Output is section-delimited (GAPS, COVERAGE, UNUSED, VOCABULARY). Parse and display readably:

```
=== Vocabulary Analysis: softwaredev/code/security ===

Gaps (body terms not in vocabulary, freq >= 2):
  parameterized  freq=3
  endpoints      freq=2

Coverage (vocabulary terms found in body):
  sql            freq=3
  secrets        freq=3

Unused (vocabulary terms not in body):
  owasp, csrf, cors   (catch user prompts, not body text — likely intentional)

Suggested vocabulary line:
  vocabulary: <current> <+ gaps>
```

The UNUSED section is informational — unused vocabulary terms are often intentional (they catch user query terms that don't appear in the way body). Don't automatically remove them.

### Suggest + Apply

When `--apply` is specified:

1. **Git safety check**: Verify the way file is inside a git worktree
2. **If NOT git-tracked**: Warn and refuse unless `--force` is also specified
3. **If git-tracked**: Replace the vocabulary line, show diff, report count
4. **For `--all --apply`**: Process each way that has gaps, showing progress

## Lint Mode

Validate way frontmatter against the official schema. Use the linter script for mechanical validation:

```bash
# Lint all ways (global + project-local)
ways lint

# Print the frontmatter schema
ways lint --schema

# Lint a specific directory
ways lint hooks/ways/meta/

# Exit non-zero on errors (for CI)
ways lint --check
```

The linter checks:
- Unknown fields (typos, deprecated fields)
- Invalid values (non-numeric threshold, bad scope values, bad macro values)
- Incomplete pairs (description without vocabulary, or vice versa)
- `when:` block validation (unknown sub-fields, path existence)
- `*.check.md` structure (`## anchor` and `## check` sections)
- Sibling vocabulary isolation (Jaccard > 0.15 warning, > 0.25 error)

The linter does NOT flag absence of optional fields. A way without `when:`, `macro:`, or `provenance:` is correct — these fields are additive. Only flag what's wrong, not what's missing-but-optional.

### Frontmatter Schema Reference

Run `ways lint --schema` for the full field reference. Key categories:

**Trigger fields**: `pattern`, `description`, `vocabulary`, `threshold`, `files`, `commands`, `trigger`
**Scope/preconditions**: `scope`, `when:` (with sub-field `project:`)
**Display**: `macro` (prepend/append)
**State**: `trigger`, `repeat`, `path`
**Governance**: `provenance:` (stripped before injection, zero context cost)
**Extended**: `scan_exclude` (macro-specific)

### Progressive Disclosure Validation (when `--all`)
When linting all ways, also check tree structural health:
- **Threshold progression**: Flag child ways with threshold <= parent threshold
- **Vocabulary isolation**: Flag siblings with Jaccard > 0.15
- **Orphan detection**: Flag way files in subdirectories with no ancestor way file
- **Token budget**: Flag individual ways > 500 tokens (frontmatter-stripped)
- **Tree depth**: Warn if any tree exceeds depth 4

## Check Mode

Simulates the check scoring curve:

```bash
/ways-tests check design "editing architecture file" --distance 20 --fires 0
```

Displays match score, distance factor, decay factor, effective score, and simulates successive firings until decay silences the check.

## Tree Mode

Analyze the progressive disclosure structure of a way tree. The path can be a short name (e.g., `supplychain`) or full path.

Walk the tree recursively, finding all way files (`{name}.md`) and check files (`{name}.check.md`). For each file, extract frontmatter and compute structural metrics.

### What to Report

```
=== Tree Analysis: softwaredev/code/supplychain ===

Structure:
  Depth: 3 levels (root → depscan → python)
  Breadth: 5 at level 1, 4 at level 2
  Total ways: 8 ways + 1 check = 9 files

Threshold Progression:
  Level 0  supplychain.md  threshold=0.35  ✓
  Level 1  repoaudit       threshold=0.42  ✓
  Level 1  sourceaudit     threshold=0.42  ✓
  Level 1  depscan         threshold=0.35  ⚠ same as parent
  Level 1  automation      threshold=0.42  ✓
  Level 1  historysever    threshold=0.42  ✓
  Level 2  depscan/python  threshold=0.50  ✓
  Level 2  depscan/node    threshold=0.50  ✓
  Level 2  depscan/go      threshold=0.50  ✓
  Level 2  depscan/rust    threshold=0.50  ✓

Assessment: Thresholds increase with depth (0.35→0.42→0.50). Good.
```

### What to Flag

- **Threshold inversion**: Child has lower threshold than parent → fires more easily than its parent, breaks progressive disclosure
- **Flat threshold**: Parent and child share exact threshold → no progressive narrowing
- **Vocabulary overlap**: Sibling ways with Jaccard similarity > 0.15 → competing triggers. Run `ways siblings <tree>` to compute pairwise scores and include results in the tree report. See **Jaccard Mode** for details on presentation and thresholds.

- **Orphan ways**: A `{name}.md` in a subdirectory where no parent directory has a way file → no progressive disclosure root
- **Deep trees**: Depth > 4 levels → likely over-decomposed
- **Wide trees**: Breadth > 7 at any level → may need sub-grouping

## Budget Mode

Estimate token cost for a way tree. Uses `wc -c` on frontmatter-stripped content divided by 4 as a rough token estimate.

### What to Report

```
=== Token Budget: softwaredev/code/supplychain ===

Per-way:
  supplychain.md           ~300 tokens
  supplychain.check.md     ~150 tokens
  repoaudit/repoaudit.md   ~450 tokens
  sourceaudit/sourceaudit.md ~280 tokens
  depscan/depscan.md        ~430 tokens
  depscan/python            ~250 tokens
  depscan/node              ~240 tokens
  depscan/go                ~230 tokens
  depscan/rust              ~210 tokens
  automation/automation.md  ~720 tokens
  historysever/historysever.md ~580 tokens

Paths (root → leaf):
  → repoaudit                    ~900 tokens
  → sourceaudit                  ~730 tokens
  → depscan → python             ~980 tokens
  → depscan → node               ~970 tokens
  → depscan → go                 ~960 tokens
  → depscan → rust               ~940 tokens
  → automation                   ~1020 tokens
  → historysever                 ~1030 tokens

Worst case (all fire):           ~3840 tokens
Average path:                    ~940 tokens
Longest path:                    ~1030 tokens

Benchmarks:
  Realistic path target: ~1200 tokens
  Worst-case target:     ~4000 tokens
```

### How to Compute

```bash
# Strip frontmatter, count bytes, divide by 4
strip_frontmatter() {
  awk 'NR==1 && /^---$/{skip=1;next} skip&&/^---$/{skip=0;next} !skip{print}' "$1"
}
tokens=$(strip_frontmatter "$wayfile" | wc -c | awk '{printf "%.0f", $1/4}')
```

### What to Flag

- **Per-way > 500 tokens**: Way may be too long, consider splitting
- **Path > 1500 tokens**: Path exceeds target, content may need trimming
- **Worst-case > 5000 tokens**: Tree is heavy, may crowd context on broad prompts
- **Single way dominates**: One way accounts for >40% of tree's total tokens

## Jaccard Mode

Measure vocabulary isolation between sibling ways. Siblings are ways that share the same parent directory.

### Two forms

**Tree-wide**: Compute pairwise Jaccard for all sibling groups in a tree.

```bash
ways siblings <tree>
```

The tool outputs tab-delimited PAIR lines: `PAIR\tway_a\tway_b\tscore`

**Specific pair**: Compare two individual ways. Extract vocabulary from each way's frontmatter and compute Jaccard inline:

```python
python3 -c "
a = set('vocab_a_words'.split())
b = set('vocab_b_words'.split())
print(f'{len(a & b) / len(a | b):.2f}' if (a | b) else '0.00')
"
```

### What to Report

```
=== Sibling Vocabulary Isolation: meta/trust ===

Pair                                    Jaccard  Shared Terms
delegation <-> voice                    0.000    (none)
delegation <-> autonomy                 0.000    (none)
voice <-> autonomy                      0.000    (none)

Assessment: Perfect isolation. No vocabulary overlap between siblings.
```

When there is overlap:

```
=== Sibling Vocabulary Isolation: softwaredev/code/supplychain ===

Group: supplychain/depscan/
Pair                                    Jaccard  Shared Terms
go <-> python                           0.040    scan
node <-> rust                           0.050    cargo

Group: supplychain/
Pair                                    Jaccard  Shared Terms
repoaudit <-> historysever              0.060    git, history
automation <-> sourceaudit              0.030    audit

Assessment: Good isolation. All pairs below 0.15 threshold.
```

### What to Flag

- **Jaccard > 0.15**: Siblings compete for the same prompts — move shared terms to the parent or pick one owner
- **Jaccard > 0.25**: Vocabulary collision — these ways likely co-fire on the same inputs, consider merging or sharply splitting
- **Jaccard = 0.00 for all pairs**: Perfect isolation (report as positive, not as absence of problems)

Show the shared terms for any pair above 0.15 so the author knows exactly which words to relocate.

### Relationship to Other Modes

- **Crowding** operates on embedding *scores* against a prompt — it measures runtime co-activation
- **Jaccard** operates on vocabulary *sets* — it measures structural overlap regardless of any specific prompt
- Both matter: Jaccard catches vocabulary collisions that crowding might miss if no test prompt triggers both ways

## Crowding Mode

Detect vocabulary overlap and semantic crowding across the entire ways corpus. This matters as the way count grows — at 50+ ways, embedding space gets contested and ways start competing for the same queries.

### What to Report

```
=== Vocabulary Crowding Analysis ===
Prompt: "check the npm dependencies for vulnerabilities"

Score Clusters (ways within 0.05 cosine of each other):
  Cluster 1: 0.58-0.62
    supplychain         0.60  threshold=0.35  MATCH
    supplychain/depscan 0.62  threshold=0.35  MATCH
    deps                0.58  threshold=0.42  MATCH
  → Assessment: Expected co-fire (supplychain tree + deps are complementary)

  Cluster 2: 0.40-0.44
    security            0.44  threshold=0.42  MATCH
    supplychain/node    0.40  threshold=0.50  no
  → Assessment: Security fires marginally. Node misses (good — prompt is generic npm, not node-specific)

Vocabulary Overlap (Jaccard > 0.15):
  deps ↔ supplychain/depscan     Jaccard=0.22  ⚠
  security ↔ supplychain         Jaccard=0.18  ⚠
  commits ↔ release              Jaccard=0.12  ✓

Top 10 most contested terms (appear in 3+ way vocabularies):
  "vulnerability"  in: security, supplychain, supplychain/depscan
  "dependency"     in: deps, supplychain/depscan, supplychain/automation
```

### What to Flag

- **Unhealthy clusters**: 3+ ways all MATCH with similar scores and serve overlapping purposes
- **High Jaccard pairs**: Sibling or unrelated ways with Jaccard > 0.25 → vocabulary collision
- **Contested terms**: Any term in 4+ vocabularies → may be too generic, consider removing from some

### How to Compute

1. Run `score-all` for the given prompt
2. Sort results by score, identify clusters within 20% of each other
3. For each pair of semantic ways, compute vocabulary Jaccard:
```bash
# For each pair of way files with vocabulary fields
# Split vocab into word sets, compute |A∩B| / |A∪B|
```
4. Count term frequency across all vocabularies

## Compare Mode

Side-by-side comparison of two way trees. Useful for evaluating whether a refactoring improved or degraded a tree.

### What to Report

```
=== Compare: supplychain vs testing ===

                    supplychain     testing
Depth               3               2
Total ways          9               3
Threshold range     0.35 - 0.50    0.35 - 0.50
Avg threshold       0.42            0.41
Worst-case tokens   ~3840           ~1100
Avg path tokens     ~940            ~680
Max sibling Jaccard 0.08            0.05
Has check file      yes             no
Has macro.sh        yes (2)         no

Assessment: supplychain is deeper and broader (8 domain-specific leaves).
testing is compact (3 nodes). Both have clean threshold progression.
```

Present the comparison as a table, then an assessment noting which tree is more mature and whether the simpler tree has room to grow.

## Metrics Mode

Show tree disclosure metrics from the current session. The metrics file is written by `ways show` at `/tmp/.claude-sessions/{session_id}/metrics.jsonl`.

### How to Read Metrics

```bash
# Find the session's metrics file
ls /tmp/.claude-way-metrics-*.jsonl 2>/dev/null

# Parse and display
cat /tmp/.claude-way-metrics-*.jsonl | jq -s .
```

### What to Report

```
=== Session Disclosure Metrics ===

Tree Coverage:
  softwaredev/code/security      root fired epoch 3
    → secrets                     fired epoch 5   (distance: 2)
    → injection                   fired epoch 8   (distance: 5)
    → auth                        not fired
    Coverage: 3/4 (75%)

  documentation                root fired epoch 1
    → readme                      fired epoch 4   (distance: 3)
    → mermaid                     fired epoch 12  (distance: 11)
    → docstrings                  not fired
    → api                         not fired
    → standards                   not fired
    Coverage: 3/6 (50%)

Parent-Activated Threshold Lowering:
  injection scored 0.40 (below normal threshold 0.42)
    → Parent "security" was active → effective threshold 0.34 → MATCH
    Without parent: would have been a miss

Epoch Distance Distribution:
  Root-to-first-child: avg 3.2 epochs
  Root-to-deepest-child: avg 8.5 epochs
```

### What to Flag

- **Orphaned roots**: Root fires but no children ever fire → tree may be too deep or children too narrowly triggered
- **Instant cascades**: Parent and child fire at same epoch → vocabulary overlap between levels (they're not progressively disclosed, they're co-disclosed)
- **Never-fire children**: A child way that never fires across multiple sessions → vocabulary may be too narrow, consider lowering threshold or broadening vocabulary
- **Parent-only sessions**: Root fires but no children needed → the root was sufficient (this is fine, means progressive disclosure is working)

## Evaluation Guidelines

When presenting results, always include an **assessment** that interprets the numbers:

- **Clean win**: Target way is the clear top scorer with daylight to the next
- **Healthy co-fire**: Multiple ways fire but serve complementary roles (e.g., `deps` + `makefile` for "install npm dependencies")
- **Overlap concern**: Two ways compete at similar scores for the same prompt — may need vocabulary differentiation or threshold tuning
- **False negative**: Target way doesn't fire for a prompt it clearly should — vocabulary gap
- **False positive**: Way fires strongly for a prompt it shouldn't own — vocabulary too broad

## Authoring Techniques

### Intentional co-fire

The default goal is sparsity — keep ways apart so each prompt activates exactly the right one. But sometimes two ways should fire together for the same prompt. A project-scoped way and a user-scoped way might both be relevant for "create a PR." A GitHub way and a custom Jira way might both need to fire for "ship this ticket."

Rather than writing a third way that combines both (more content, more maintenance, more context consumed), plant a small number of shared vocabulary terms in both ways so the embedding scorer co-fires them naturally. Two small ways each contributing their piece is lighter than one large way covering everything.

**Discipline:** The shared terms must be narrow — "pull request", "ship", "PR" — not broad terms like "code" or "deploy" that create accidental overlap elsewhere. Use `/ways-tests crowding` to verify the co-fire only happens on the intended prompts.

**When crowding mode reports overlap**, distinguish:
- **Accidental**: similar scores on prompts neither should own → sharpen vocabularies apart
- **Intentional**: both score well on prompts both should serve → mark as healthy co-fire

### Sparsity as overfitting guard

Adding vocabulary to fix a miss works locally but risks overfitting globally. Every added term is a surface for false matches against other ways.

- **15 precise terms beat 40 general terms.** Prefer domain-specific words over common ones.
- **Don't chase every synonym.** One well-chosen term per concept. Don't add "deployed", "released", "landed", "merged" when "shipped" alone fixes the miss.
- **Threshold is a second lever.** Raising threshold cuts weak false matches without losing strong true matches.
- **Accept some misses.** 90% recall with 0 FP beats 100% recall with 5% FP. The 0 FP constraint is hard; recall is soft.

## Notes

- Scores are cosine similarity on the 0–1 scale.
- The `way-embed` binary lives at `~/.cache/claude-ways/user/way-embed` (downloaded via `make setup`). If missing, semantic matching is unavailable. Run `ways status` to check.
- Embedding scoring is built into the `ways` binary. If `ways` is missing, run `make setup` in `~/.claude`.
- The `embed_threshold` frontmatter field (float, default `0.35`) sets the per-way cosine cutoff.
- Corpus regeneration (`ways corpus`) bakes fresh embeddings into the JSONL. After editing any way's `description` or `vocabulary`, regen is required for embedding scores to reflect the change.
- When displaying results, use human-readable format, not raw machine output.
- Check scoring uses `awk` for floating-point math.
