---
name: ways-update
description: Update the agent-ways installation to the latest version — pull, rebuild the Rust binaries (ways/attend/way-embed), re-install symlinks, and rebuild the corpus. Use when the user wants to update agent-ways, pull the latest ways/hooks, run "make update", or refresh their ways framework install. Not for editing or authoring individual ways (that is the ways skill) or upgrading project dependencies.
allowed-tools: Bash, Read
---

# ways-update: Update the agent-ways install

Brings the local **agent-ways** checkout (the repo that backs `~/.claude`) up to
the latest version and rebuilds everything that depends on it. The canonical
entry point is `make update`; this skill wraps it with the pre-flight checks
`make` itself doesn't do.

## What `make update` does

```
make update
  ├─ git pull --ff-only          # fast-forward to latest; refuses if history diverged
  ├─ make update-binaries        # force-rebuild ways, attend, attend-chat, way-embed
  └─ make install                # re-symlink binaries into ~/.local/bin, mark hooks executable
```

Binaries are symlinked, so a rebuild takes effect immediately for *new* shells —
but a **running Claude Code session keeps the old hooks and ways in memory.**
Always finish by telling the user to restart Claude Code.

## Steps

Run from the install root. Resolve it once and reuse it — the skill is global, so
the working directory is unknown at invocation:

```bash
ROOT="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
# Confirm this is actually the agent-ways repo before touching anything
grep -q 'agent-ways' "$ROOT/Makefile" 2>/dev/null && git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 \
  || { echo "Not an agent-ways checkout: $ROOT"; exit 1; }
```

### 1. Pre-flight: check the working tree

`git pull --ff-only` will **abort** if tracked files have uncommitted changes that
the incoming update also touches — and this repo commonly carries a modified
`settings.json` / `settings.local.json`. Surface the state before pulling:

```bash
git -C "$ROOT" fetch origin --quiet
git -C "$ROOT" status --short --branch
```

- **Clean tree, behind remote** → safe to proceed to step 2.
- **Dirty tracked files** → tell the user what's modified. Offer to commit them
  (use `/ship` for that), or `git -C "$ROOT" stash` before the update and
  `git stash pop` after. Do **not** stash or discard their changes without asking.
- **Diverged history** (local commits not on remote) → `--ff-only` will fail by
  design. Stop and explain; the user decides whether to push, rebase, or merge.
  Never force anything.

Untracked files (e.g. a new way you haven't committed) are safe — the pull leaves
them alone — but flag them so nothing is silently left behind.

### 2. Run the update

```bash
make -C "$ROOT" update
```

If it fails partway, the three sub-steps are independent and re-runnable — you can
invoke `make -C "$ROOT" update-binaries` and `make -C "$ROOT" install` directly to
finish, or `make -C "$ROOT" ways-rebuild` to force just the ways binary.

### 3. Rebuild the matching corpus

`make update` rebuilds binaries but not the embedding corpus. If the pull changed
any way's `description`/`vocabulary`, refresh it so matching reflects the update:

```bash
ways corpus
```

(Skip if the pull touched no `.md` frontmatter — `git -C "$ROOT" diff --stat HEAD@{1} -- '*/ways/**/*.md'` shows whether it did.)

### 4. Tell the user to restart

The running session won't pick up new hooks or ways until restart. End with an
explicit: **"Restart Claude Code for the update to take effect."**

## Verify (optional)

```bash
ways --version          # confirm the rebuilt binary is on PATH
make -C "$ROOT" test    # full suite: lint + smoke + unit + sim + lang (slow)
```

## Notes

- This skill lives **inside** the repo it updates (`skills/` is the live personal
  scope). That's intentional — an "update my config" command should be reachable
  from any project — but it means the skill can update the very file defining it.
  It only ever runs `git`/`make`; it does not edit repo contents.
- For a first-time install (not an update), the target is `make install`, and for
  a from-scratch environment `make setup` first. This skill assumes an existing
  checkout.
