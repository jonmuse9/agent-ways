---
description: writing or editing source code — new function, new file, implementation, code change
vocabulary: implement write code function class module add feature edit logic build create refactor
files: \.(py|ts|tsx|js|jsx|mjs|go|rs|rb|java|kt|scala|c|cc|cpp|h|hpp|cs|php|swift|sh|lua|ex|exs)$
scope: agent
---

## anchor

You are about to write code. The cheapest bug is the line never written; the most expensive is the right fix in the wrong place.

## check

Before writing this:

- **Does it need to exist?** Reach for the standard library, a native platform feature, or an already-installed dependency before custom code. Speculative need — skip it and say so. (YAGNI)
- **Is this the codepath that actually runs?** Confirm you're editing the path in effect, not a sibling, a shadowed copy, or a dead branch.
- **Is this the minimal change?** Shortest diff that works — no abstraction, config, or scaffolding nobody requested.
- **Never simplified away:** validation at trust boundaries, error handling that prevents data loss, security, accessibility, and anything explicitly requested.
