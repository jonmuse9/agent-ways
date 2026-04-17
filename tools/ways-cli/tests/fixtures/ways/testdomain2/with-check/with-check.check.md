---
description: dependency installation check
vocabulary: install add dependency package
commands: ^(npm|pip|cargo)\ (install|add)
scope: agent
---
## anchor

Supply chain security requires auditing dependencies before installation.

## check

Before installing, verify:
- [ ] Package is widely used and maintained
- [ ] No known vulnerabilities
- [ ] License is compatible
