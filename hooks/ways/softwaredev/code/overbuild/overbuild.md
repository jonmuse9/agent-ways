---
scope: agent
refire: 0.2
---

<!-- Postcheck-reactive only (sole trigger is postcheck.sh here). ADR-135. -->

# Over-build

The code you just wrote matches a known reinvention. The lazy path that works:

- **Hand-rolled LRU/TTL cache** (`OrderedDict` + eviction) → `functools.lru_cache`. A hand-rolled cache is a bug farm with a hit rate.
- **Hand-rolled singleton** (`__new__` guarding `_instance`) → a module-level value, or an injected dependency.

If the reinvention is deliberate and the stdlib/platform genuinely falls short, keep it and move on — this fires on the *shape*, not the intent.
