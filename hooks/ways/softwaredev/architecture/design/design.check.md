---
description: introducing new architectural pattern, restructuring module boundaries, designing service interfaces
vocabulary: restructure boundary interface architectural introduce new pattern module service layer
scope: agent
---

## anchor

You are making a structural change. The design way applies — Context, Constraints, Options, Trade-offs, Decision.

## check

Before committing to this architectural choice:
- Did you **read** the existing code/architecture, or are you assuming its shape?
- Is there an existing ADR that already decided this? Check with `docs/scripts/adr list --group` if the project has ADRs.
- Does this codebase already solve this problem a different way? Grep for similar patterns before introducing a new one.
- What is the blast radius — how many files/modules touch this interface?
- Are you introducing a new pattern where an existing one would work?
