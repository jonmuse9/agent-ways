---
description: documentation as a typed graph — taxonomy, catalog frontmatter, linting, and how docs are organized and serialized for readers
vocabulary: documentation docs catalog taxonomy diataxis frontmatter markdown linting graph node edge serialization readme reference tutorial how-to explanation mode domain mkdocs obsidian
pattern: documentation|docs|catalog|diataxis|taxonomy|doclint|document.*(structure|model|graph|classif)
files: README\.md$|docs/.*\.md$|mkdocs\.ya?ml$
embed_threshold: 0.30
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Documentation

Documentation is **one typed graph**, not a pile of files. Each page and each
decision record is a *node*; `related`/`supersedes` references are *edges*. The
node's **type lives in frontmatter and is enforced by a linter** — not in its
filename. The **filesystem is a serialization** of that graph for human readers:
folders, names, and the rendered site are *views*, refactor-freely, never the
source of truth.

This is what lets the rigor scale: a typed graph that would take a human minutes
to reason through, an agent and a linter sustain in one pass. Push the structure
into frontmatter and lint where it is cheap to maintain; keep the filesystem
friendly for the humans who still read it. (Decision: ADR-302.)

## Two altitudes

Children of this way split into the **model** that types the corpus and the
**craft** of authoring one artifact well:

| Altitude | Concern | Way |
|----------|---------|-----|
| Model | The graph itself — nodes, edges, one corpus for docs + decisions | `graph` *(forthcoming)* |
| Model | The typed node — `id` (`DD.NNN.P`), `domain`, `mode`, `aliases`, edges | `frontmatter` *(forthcoming)* |
| Model | The classification enum — Diátaxis reader posture (T/H/R/E) | `diataxis` *(forthcoming)* |
| Model | A node *type* — the decision record | `adr` |
| Model | The type-checker — `doclint`, the invariant set, the coverage matrix | `linting` *(forthcoming)* |
| Model | Projecting the graph for human readers — folders, nav, mkdocs, Obsidian | `serialization` *(forthcoming)* |
| Craft | The front door | `readme` |
| Craft | Reference for HTTP/REST surfaces | `api` |
| Craft | Code-level docs (docstrings, JSDoc, rustdoc) | `docstrings` |
| Craft | Structural diagrams | `mermaid` |
| Craft | House norms — style, conventions, accessibility | `standards` |

*Forthcoming* model ways are authored as ADR-302 lands; until then this parent
names the shape so the corpus has somewhere to grow into.

## Principles

- **Type once, serialize many** — one graph; many views (dev tree, published site,
  Obsidian graph, audience bundles). Audience drives the *view*, never the type.
- **An invariant earns its place only if its violation is a real defect** — a
  dangling edge, a supersede cycle, an id that disagrees with its mode. Rigor that
  tracks nothing is just a tax.
- **Progressive disclosure, task-orientation, currency** — overview before
  detail; organize by reader job; an outdated page is a broken front door.

## See Also

- adr(documentation) — decision records as nodes in the graph
- readme(documentation) — README as the front door
- standards(documentation) — documentation house norms
- mermaid(documentation) — structural diagrams
