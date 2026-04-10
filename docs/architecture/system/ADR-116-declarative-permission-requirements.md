---
status: Draft
date: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-004
  - ADR-113
  - ADR-115
---

# ADR-116: Declarative Permission Requirements

## Context

Ways macros and attend sensors both execute shell commands that require tool permissions in `~/.claude/settings.json`. Neither tool currently declares what permissions it needs. When a required permission is missing, the tool silently fails or prompts the user mid-session.

The current permission model for way macros uses a flat file (`~/.claude/trusted-project-macros`) that lists project paths whose macros are allowed to run. This is a binary trust model — a project is either fully trusted or not. It has several problems:

- **Opaque** — you can't see what a macro will do without reading the script
- **All-or-nothing** — trusting a project grants all its macros, not specific capabilities
- **Siloed** — attend sensors have no equivalent mechanism
- **Obscure** — the file is undiscoverable and undocumented in the tool itself

Meanwhile, `settings.json` already has a structured `permissions.allow` list that governs what Claude Code can do. This is the real authority — but there's no way to validate that declared requirements align with granted permissions.

## Decision

Add a `requires:` field to both way frontmatter and attend sensor configuration that declares tool permissions. Provide audit commands that diff declared requirements against `settings.json` grants.

### Schema

**Way frontmatter** (new field in `frontmatter-schema.yaml`):

```yaml
---
description: GitHub workflow guidance
vocabulary: github pull request merge review
macro: prepend
requires:
  - Bash(gh:*)
  - Bash(git:*)
---
```

**Attend sensor config** (in `attend.yaml` / `config.yaml`):

```yaml
sensors:
  +disk-pressure:
    script: scripts/disk-check.sh
    interval: 60
    requires:
      - Bash(df:*)
      - Bash(du:*)
```

**Built-in sensor defaults** — hardcoded in the attend binary, queryable via `attend permissions`:

| Sensor    | Requires                          |
|-----------|-----------------------------------|
| git       | `Bash(git:*)`                     |
| processes | `Bash(ps:*)`                      |
| peers     | `Read`                            |
| context   | `Read`                            |

### Wildcard

A way or sensor may declare `requires: ["*"]` to indicate it needs arbitrary tool access. This is equivalent to the old `trusted-project-macros` blanket trust — the audit will flag it as "unrestricted" rather than listing specific gaps. This provides a migration path from the old model.

### Audit Commands

**`ways permissions audit`** — scans all way frontmatter (global + project-local), collects `requires:` fields, diffs against `settings.json`:

```
$ ways permissions audit
  Way                              Requires        Status
  ────────────────────────────────────────────────────────
  softwaredev/delivery/github      Bash(gh:*)      MISSING
  softwaredev/delivery/github      Bash(git:*)     granted
  softwaredev/code/quality         Bash(wc:*)      granted
  softwaredev/code/quality         Bash(find:*)    granted
  meta/knowledge/authoring         Bash(ways:*)    granted

  1 missing permission. Add to settings.json:
    "Bash(gh:*)"
```

**`attend permissions audit`** — same pattern for sensor configs:

```
$ attend permissions audit
  Sensor          Requires        Status
  ──────────────────────────────────────
  git             Bash(git:*)     granted
  processes       Bash(ps:*)      MISSING
  peers           Read            granted
  +disk-pressure  Bash(df:*)      granted
  +disk-pressure  Bash(du:*)      MISSING

  2 missing permissions. Add to settings.json:
    "Bash(ps:*)", "Bash(du:*)"
```

### Permission Format

The `requires:` values use the same permission string format as `settings.json`:

- `Read` / `Read(/path/**)` — file read access
- `Write(/path/**)` — file write access
- `Edit(/path/**)` — file edit access
- `Bash(command:*)` — specific bash command
- `Bash(*)` — any bash command
- `*` — unrestricted (wildcard)

This means the audit is a direct string comparison — no translation layer needed.

### Config Location

Both tools read `settings.json` from the standard Claude Code location (`~/.claude/settings.json`). Sensor configs follow the XDG pattern established by attend (ADR-115): user config at `$XDG_CONFIG_HOME/attend/config.yaml`, project overlay at `$PROJECT/.claude/attend.yaml`.

### Replaces `trusted-project-macros`

The `trusted-project-macros` file is deprecated. Project-local ways that declare `requires:` are validated against `settings.json` like any other way. A project-local way with `requires: ["*"]` is equivalent to the old "trusted project" entry.

Migration: if `trusted-project-macros` exists, the audit command warns that it's deprecated and suggests adding explicit `requires:` fields to the relevant ways.

## Consequences

### Positive

- **Discoverable** — `requires:` is visible in frontmatter, not buried in a separate file
- **Granular** — per-way and per-sensor permission declarations
- **Auditable** — one command shows the full permission gap
- **Consistent** — same model for ways and attend, using `agent-fmt` Table output
- **Single authority** — `settings.json` is the only place permissions are granted
- **Self-documenting** — reading a way's frontmatter tells you what it needs

### Negative

- Existing ways need `requires:` fields added (can be done incrementally)
- Built-in sensor requirements are hardcoded (but rarely change)

### Neutral

- Ways without `requires:` are assumed to need no special permissions (static-only ways)
- The audit is advisory — it doesn't block execution, it reports gaps
- `frontmatter-schema.yaml` gains one new field

## Implementation Plan

1. Add `requires:` to `frontmatter-schema.yaml` (optional field, string array)
2. Add `requires:` parsing to attend sensor config
3. Implement `ways permissions audit` in ways-cli (reads frontmatter + settings.json)
4. Implement `attend permissions audit` (reads sensor config + settings.json)
5. Add `requires:` to existing macro-bearing ways (github, adr, quality, etc.)
6. Hardcode built-in sensor requirements in attend
7. Deprecation notice for `trusted-project-macros` in audit output
8. Update `docs/hooks-and-ways/macros.md` security model section
