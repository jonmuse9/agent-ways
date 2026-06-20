---
name: docs
description: Author and manage documentation-catalog pages using the project's doc CLI tool. Use when the user wants to write, scaffold, list, or lint catalog docs (tutorials, how-to guides, reference, explanation), or asks "write docs for X", "new doc page", "what docs exist", "document this". Pairs with the adr skill — docs and ADRs share domain bands.
allowed-tools: Bash, Read, Grep, Glob
---

# Documentation Catalog Authoring

Operate catalog pages through the `docs/scripts/doc` CLI tool. **Never hand-write
catalog frontmatter** — the tool computes the `DD.NNN.P` id, picks the next
in-domain serial, and emits a lint-clean page. Reverse-engineering the id scheme
from the linter is the smell this skill exists to remove.

Docs and ADRs are **one typed graph** sharing domain bands (see the project's
documentation-catalog ADR). This skill is the docs half; the `adr` skill is the
decisions half.

## Commands

```bash
# Discover
docs/scripts/doc domains                       # domain bands (shared with ADRs)
docs/scripts/doc coverage                       # domain × mode matrix — where the gaps are
docs/scripts/doc list [--domain D] [--mode M]   # list catalog pages
docs/scripts/doc gaps                           # empty cells + doc/ADR imbalance

# Create
docs/scripts/doc new <domain> <mode> "Title"    # scaffold (correct id, frontmatter, H1)
#   mode ∈ tutorial | how-to | reference | explanation
#   --dir <subdir>   land it somewhere specific (may nest, e.g. explanation/attend-messaging)
#   --related ADR-NNN / --related DD.NNN.P   add edges up front (repeatable)

# Maintain
docs/scripts/doc lint [--strict]                # lint the catalog graph (the test)
```

## Workflow

1. **Classify the mode first.** Decide Tutorial / How-to / Reference / Explanation
   *before* scaffolding — it's the one judgment the tool can't make. The
   **diataxis** way carries the 2×2; the short form: *studying vs working* ×
   *practical steps vs theoretical knowledge*. One mode per page.
2. **Check domains**: `docs/scripts/doc domains` — domains and bands vary per project.
3. **Scaffold**: `docs/scripts/doc new <domain> <mode> "Title"`. The tool assigns
   the next id and writes the frontmatter; you never type an id by hand.
4. **Write the body** faithful to its mode (a tutorial teaches; a reference
   describes; don't blend).
5. **Link edges**: add `related:`/`supersedes:` as Obsidian wikilinks
   (`[[ADR-136]]`, `[[01.003.E]]`) — these are the graph edges the linter checks.
6. **Lint**: `docs/scripts/doc lint` before committing (dangling edges, malformed
   ids, mode/pole mismatches all surface here).

## Page format

The tool generates catalog frontmatter — identity is the `id`, not the path:

```markdown
---
id: 01.003.E          # DD.NNN.P — domain band . serial . mode pole
domain: system
mode: explanation     # tutorial=T  how-to=H  reference=R  explanation=E
related: []           # wikilink edges: [[ADR-136]], [[01.001.E]]
aliases: []
---

# Title
```

## Key rules

- **Always use the CLI** — never hand-author `id`/`domain`/`mode` frontmatter.
- **One mode per page** — if it won't fit one Diátaxis quadrant, it's two pages.
- **Identity is the id, the path is a view** — the filesystem is a serialization;
  move/rename freely, the `id` is what the graph keys on.
- **Lint before commit** — `docs/scripts/doc lint` is the catalog's test.

## See also

- the **diataxis** way — picking the mode (the 2×2)
- the **documentation** way — the typed-graph model
- the **adr** skill — the decisions half of the same catalog
