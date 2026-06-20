# Progressive Disclosure & Structured Thinking Integration Test

## Instructions for Claude

**CRITICAL: DO NOT explore the codebase, read hook scripts, or investigate how things work. This is a BLACK BOX test. You are testing OBSERVABLE BEHAVIOR, not implementation details.**

**DO NOT**:
- Spawn Explore agents
- Read hook scripts or way files
- Search the codebase for implementation details
- Try to understand how hooks work before testing

**DO**:
- Follow each step EXACTLY in order
- Ask the user to type the specified prompt (steps marked USER)
- Observe what system-reminder content appears after each prompt
- Report PASS/FAIL based on what you see vs what was expected

You are a test executor, not a researcher. Begin with Step 1 immediately after reading this file.

**Format your reports as**:
```
Step N: [description]
Result: PASS / FAIL / UNEXPECTED
Detail: [what happened vs what was expected]
```

**A note on Step 8b**: That step presents a real problem for you to reason about. There is no right or wrong answer — we're tuning for two things: (1) maximal autonomous engagement (you drive the reasoning without waiting for permission) and (2) maximal collaborative opportunity (when you hit genuine unknowns, you engage the human naturally). Reading this expected outcome in advance doesn't help because the outcome depends entirely on how you actually reason through the problem. Just work the problem authentically.

Begin Step 1 NOW.

---

## Part A: Progressive Disclosure Tree Activation

### Step 1 — Root way fires on broad prompt

> **USER**: Type exactly: `let's review the security of this codebase`

> **CLAUDE**: Check if you received the **Security Way** root content. Look for "Security Way" heading with defaults and review checklist. Report what fired.

**Expected**: The root security way fires (threshold 1.8, vocabulary: security, vulnerability, protect). You should see "## Defaults" and "## When Reviewing Existing Code" — NOT the injection detection table or secrets management (those are children).

---

### Step 2 — Child way fires on specific prompt

> **USER**: Type exactly: `check if there's any SQL injection in the user input handling`

> **CLAUDE**: Check if you received the **Injection Prevention Way** child content. Look for the detection/action table and the anti-rationalization table. Report what fired.

**Expected**: The injection child way fires (threshold 2.0, vocabulary: injection, sql, xss, parameterized, sanitize). You should see:
- "## Detection and Action Rules" table
- "## Common Rationalizations" table (anti-rationalization pattern)

The root security way should NOT re-fire (marker exists from Step 1).

---

### Step 3 — Verify threshold lowering

> **CLAUDE**: Check the tree disclosure metrics file at `/tmp/.claude-way-metrics-{session_id}.jsonl`. Read it and report:
1. Did the injection child record the security root as its parent?
2. What was the epoch distance from parent to child?
3. What is the sibling coverage (how many of security's 3 children have fired)?

**Expected**: The metrics should show:
- `parent: "softwaredev/code/security"`, `depth: 1`
- `epoch_distance` should be 1-2 (injection fired shortly after root)
- `sibling_fired: 1` out of `sibling_total: 3`

---

### Step 4 — Sibling fires independently

> **USER**: Type exactly: `we need to store user passwords securely, what hashing should we use`

> **CLAUDE**: Check if the **Secrets Way** fired. Report what content appeared.

**Expected**: The secrets child way fires. Check metrics again — sibling_fired should now be 2/3.

---

### Step 5 — Documentation tree test

> **USER**: Type exactly: `add a mermaid diagram showing the request flow`

> **CLAUDE**: Check what fired. Did you get the Mermaid Way child, the docs root, or both?

**Expected**: The docs root (threshold 1.8) may fire on "diagram". The mermaid child (threshold 2.0, vocabulary: mermaid, diagram, flowchart, sequence) should also fire. You should see the diagram type selection table and the styling palette. Report whether both fired or just the child.

---

### Step 6 — Anti-rationalization in TDD

> **USER**: Type exactly: `use tdd to write this feature, tests first`

> **CLAUDE**: Check if the TDD Way fired with its anti-rationalization table. Report whether you see the "Common Rationalizations" section.

**Expected**: The TDD child way fires (threshold 2.5, vocabulary: tdd, red, green, refactor, test first). You should see:
- "## The Cycle" (Red-Green-Refactor)
- "## Common Rationalizations" table with 6 entries

---

## Part B: Structured Thinking — Metacognitive Escalation

The think way teaches an escalation gradient: internal check → internal reasoning → external strategy → collaborative. These steps test whether the way fires correctly, whether the agent performs the metacognitive check, and whether autonomous escalation works.

### Step 7 — Structured Thinking way fires on reasoning prompt

> **USER**: Type exactly: `we need to explore different approaches for the caching layer, there are several options to consider`

> **CLAUDE**: Check if the Structured Thinking way fired. Look for these key elements:
> 1. "# Structured Thinking" heading
> 2. "The Metacognitive Check" section (trending clear vs unclear)
> 3. "Escalation Gradient" table (internal → external → collaborative)
> 4. "External Strategies" table with strategy skills
> 5. "Engaging the Human" section with `AskUserQuestion` guidance

**Expected**: The way fires (vocabulary: explore, options, approaches, trade-off, balance, alternatives). You should see the metacognitive check framework and escalation gradient — NOT just a strategy menu. The way teaches the agent *how to decide* whether to escalate, not just *what to escalate to*.

---

### Step 8 — Agent performs metacognitive check

> **CLAUDE**: Observe your own response to the user's Step 7 prompt. Did you:
> 1. Perform a metacognitive check (assess clarity vs uncertainty)?
> 2. If trending unclear: decide on an escalation level?
> 3. If escalating to external strategy: invoke the skill **immediately and decisively** (no "should I use..." hedging)?
>
> Report what you actually did — the test is whether the way's guidance shaped your behavior.

**Expected**: The agent should have assessed the problem direction. For this prompt ("explore different approaches... several options"), the metacognitive check should trend **unclear** (multiple viable approaches, no obvious winner). The agent MAY autonomously invoke `/think tree` — if so, it should do so decisively without asking permission. If the agent stayed at internal reasoning, that's also valid — the test is that the check happened, not that escalation occurred.

---

### Step 8b — Autonomous escalation on a real problem

> **USER**: Type exactly: `We have a read-heavy API hitting Postgres, about 50k requests per minute, 95% reads. Response times are creeping up. We could add Redis as an application cache, use Postgres query caching, put a CDN in front of the API, or denormalize the hot tables. Data changes every few minutes and consistency matters — stale prices would be bad.`

> **CLAUDE**: Work this problem. Don't report on the test — actually engage with the problem using whatever level of the escalation gradient you land on. The user will observe your behavior afterward.

**What we're observing** (not "expected" — there's no single right answer):
- Did the agent perform the metacognitive check and decide on an escalation level?
- If it escalated to an external strategy: did it invoke the skill **immediately** (no "I think I should use..." hedging)?
- If it stayed internal: was internal reasoning actually sufficient, or did the agent avoid escalating when it should have?
- Did the agent engage the human via `AskUserQuestion` when it hit genuine unknowns (deployment constraints, budget, team expertise, existing infrastructure)?
- Was the reasoning visible and collaborative, or opaque and self-contained?

This step has no PASS/FAIL. After the agent finishes, the user evaluates the interaction quality along two axes: **autonomous engagement** (agent drove the reasoning forward without waiting for permission) and **collaborative opportunity** (agent surfaced unknowns and invited human input at the right moments).

> **USER**: After the agent works the problem, type: `OK, let's continue with the test at Step 9`

---

### Step 9 — Think way re-fires after strategy completion

> **USER**: Type exactly: `what are the trade-offs between the three options`

> **CLAUDE**: Check if the Structured Thinking way fires again on this related prompt.

**Expected**: If Step 8b completed a full strategy cycle (including cleanup), the way marker was cleared — so the way **will re-fire**. This is correct: a completed strategy should allow the think way to activate again for new problems. If Step 8b was skipped or the strategy wasn't completed, the marker from Step 7 persists and the way stays silent. Report which case you're in.

---

## Part B2: Think Strategy Session Lifecycle

These steps test the session lifecycle when an external strategy is invoked — whether by autonomous escalation or explicit `/think` invocation.

### Step 10 — Strategy invocation creates session registration

> **CLAUDE**: First, clean up any leftover state:

```bash
rm -f /tmp/.claude-think-session 2>/dev/null
```

> **USER**: Type exactly: `/think tree`

> **CLAUDE**: After the skill begins and registers, check the session file:

```bash
cat /tmp/.claude-think-session 2>/dev/null
```

**Expected**: The file contains `tree-of-thoughts`. The skill registered its session before beginning work. Note: in normal flow the agent would invoke this autonomously during escalation — the explicit `/think tree` here is to isolate the lifecycle test.

---

### Step 11 — Overlapping session is blocked

> **USER**: Type exactly: `/think stepback`

> **CLAUDE**: The step-back skill should detect the active tree-of-thoughts session and ask whether to finish or abandon it first. Report whether the skill blocked or proceeded.

**Expected**: The skill detects `/tmp/.claude-think-session` contains `tree-of-thoughts` and does NOT start a new session. It asks the user to finish or abandon the active session first. This prevents conflicting reasoning strategies from running simultaneously.

---

### Step 12 — Session cleanup on completion or abandon

> **CLAUDE**: Simulate abandoning the session by running the cleanup:

```bash
rm -f /tmp/.claude-think-session 2>/dev/null
```

> Then verify it's gone:

```bash
[[ -f /tmp/.claude-think-session ]] && echo "EXISTS" || echo "GONE (clean)"
```

**Expected**: File is removed. The think way marker is also eligible for cleanup (strategies clear it on completion), meaning the think way can fire again for new problems later in the session.

---

## Part C: Negative Tests

### Step 13 — No false positive on unrelated prompt

> **USER**: Type exactly: `how many legs does an octopus have?`

> **CLAUDE**: Check if any NEW domain-specific content was injected. Report what you see.

**Expected**: Nothing fires. No way matches, no think strategy matches.

---

### Step 14 — Way does not re-fire on related prompt

> **USER**: Type exactly: `let's explore multiple options for the database schema`

> **CLAUDE**: Check if the Think Strategies way fires again.

**Expected**: The way does NOT re-fire (already fired in Step 7, marker exists). Other ways may fire (design, migrations) but the Think Strategies way should be silent.

---

## Part D: Summary

### Step 15 — Compile results

> **CLAUDE**: Compile a summary table:
>
> | Step | Test | Expected | Result |
> |------|------|----------|--------|
> | 1 | Security root fires | Root content, no children | ? |
> | 2 | Injection child fires | Detection table + anti-rationalization | ? |
> | 3 | Metrics tracking | Parent recorded, epoch distance, coverage | ? |
> | 4 | Sibling fires | Secrets way, coverage 2/3 | ? |
> | 5 | Docs tree | Mermaid child fires | ? |
> | 6 | TDD anti-rationalization | Rationalizations table present | ? |
> | 7 | Think way fires | Metacognitive check + escalation gradient | ? |
> | 8 | Metacognitive check | Agent assesses clarity; escalates decisively if unclear | ? |
> | 8b | Autonomous escalation | No pass/fail — evaluate engagement + collaboration | ? |
> | 9 | Think way post-strategy | Re-fires if strategy completed (marker cleared); silent if not | ? |
> | 10 | Session registration | Strategy skill creates session file | ? |
> | 11 | Overlapping session blocked | Second skill refuses to start | ? |
> | 12 | Session cleanup | Session file removed | ? |
> | 13 | Negative test | Nothing fires | ? |
> | 14 | Way does not re-fire | Marker prevents repeat | ? |
>
> Report pass/fail count and observations about:
> - Whether progressive disclosure trees deliver the right content at the right depth
> - Whether anti-rationalization tables appear at the expected specificity level
> - Whether the metacognitive check shapes agent behavior (not just injects content)
> - Whether autonomous escalation to external strategies is decisive (no hedging) — Step 8b is the key observation
> - Whether session lifecycle prevents overlapping strategies and cleans up correctly
> - Whether tree disclosure metrics capture parent-child relationships
