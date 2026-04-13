#!/bin/bash
# Check if agent-ways is up to date with upstream
# Handles four install scenarios: direct clone, fork, renamed clone, plugin
#
# Detection order:
#   1. Is ~/.claude a git repo? If not, exit.
#   2. Is origin aaronsb/agent-ways? → direct clone
#   3. Is origin a fork of aaronsb/agent-ways? → fork
#   4. Does .claude-upstream marker exist? → renamed clone (org internal copy)
#   5. Is CLAUDE_PLUGIN_ROOT set? → plugin install
#
# Network calls (git fetch, gh api) are rate-limited to once per hour.
# Writes state to cache file; display is handled by `ways show core`.

CLAUDE_DIR="${HOME}/.claude"
UPSTREAM_REPO="aaronsb/agent-ways"
UPSTREAM_URL="https://github.com/${UPSTREAM_REPO}"
UPSTREAM_MARKER="${CLAUDE_DIR}/.claude-upstream"
CACHE_FILE="/tmp/.claude-config-update-state-$(id -u)"
ONE_HOUR=3600
CURRENT_TIME=$(date +%s)

# --- Helpers ---

needs_refresh() {
  [[ ! -f "$CACHE_FILE" ]] && return 0
  local last_fetch
  last_fetch=$(sed -n 's/^fetched=//p' "$CACHE_FILE" 2>/dev/null)
  [[ -z "$last_fetch" ]] && return 0
  (( CURRENT_TIME - last_fetch >= ONE_HOUR ))
}

# Atomic cache write — write to temp then mv to avoid races between sessions
write_cache() {
  local type="$1" behind="$2" extra="$3"
  local tmp="${CACHE_FILE}.$$"
  {
    echo "fetched=${CURRENT_TIME}"
    echo "type=${type}"
    echo "behind=${behind}"
    [[ -n "$extra" ]] && echo "$extra"
  } > "$tmp"
  mv -f "$tmp" "$CACHE_FILE"
}

check_marker_file() {
  [[ -f "$UPSTREAM_MARKER" ]] || return 1
  local declared
  declared=$(head -1 "$UPSTREAM_MARKER" | tr -d '[:space:]')
  [[ "$declared" == "$UPSTREAM_REPO" ]]
}

# Check gh CLI availability and auth status.
# Returns 0 if gh is ready, 1 if not (with reason in GH_ISSUE).
check_gh() {
  if ! command -v gh &>/dev/null; then
    GH_ISSUE="gh CLI not installed (needed for fork detection)"
    return 1
  fi

  local auth_output
  auth_output=$(gh auth status 2>&1)
  local auth_rc=$?

  if [[ $auth_rc -ne 0 ]]; then
    if echo "$auth_output" | grep -qi "not logged in"; then
      GH_ISSUE="gh CLI not logged in — run: gh auth login"
    elif echo "$auth_output" | grep -qi "token.*expired"; then
      GH_ISSUE="gh auth token expired — run: gh auth refresh"
    else
      GH_ISSUE="gh auth failed: $(echo "$auth_output" | head -1)"
    fi
    return 1
  fi

  GH_ISSUE=""
  return 0
}


# --- Scenario 1 & 2: Git repo (clone or fork) ---

if git -C "$CLAUDE_DIR" rev-parse --git-dir >/dev/null 2>&1; then
  REMOTE_URL=$(git -C "$CLAUDE_DIR" remote get-url origin 2>/dev/null)

  # Skip non-GitHub remotes early
  if [[ "$REMOTE_URL" != *github.com* ]]; then
    exit 0
  fi

  # Extract owner/repo from URL (handles https and ssh formats)
  OWNER_REPO=$(echo "$REMOTE_URL" | sed -E 's#.*github\.com[:/]##; s/\.git$//')

  # Validate owner/repo format to prevent path traversal in API calls
  if [[ ! "$OWNER_REPO" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
    exit 0
  fi

  if [[ "$OWNER_REPO" == "$UPSTREAM_REPO" ]]; then
    # --- Direct clone ---
    if needs_refresh; then
      timeout 10 git -C "$CLAUDE_DIR" fetch origin --quiet 2>/dev/null
      BEHIND=$(git -C "$CLAUDE_DIR" rev-list HEAD..origin/main --count 2>/dev/null || echo 0)
      write_cache "clone" "$BEHIND"
    fi
    exit 0

  else
    # --- Possible fork ---
    # Only call check_gh + API when cache needs refresh (avoids gh auth status latency)
    if needs_refresh; then
      if check_gh; then
        GH_OUTPUT=$(timeout 10 gh api "repos/${OWNER_REPO}" 2>&1)
        GH_RC=$?

        if [[ $GH_RC -ne 0 ]]; then
          if echo "$GH_OUTPUT" | grep -qi "404\|not found"; then
            write_cache "gh_error" "0" "reason=repo not found on GitHub"
          elif echo "$GH_OUTPUT" | grep -qi "403\|rate limit"; then
            write_cache "gh_error" "0" "reason=GitHub API rate limited"
          else
            write_cache "gh_error" "0" "reason=$(echo "$GH_OUTPUT" | head -1 | tr -cd '[:print:]')"
          fi
        else
          PARENT=$(echo "$GH_OUTPUT" | jq -r '.parent.full_name // empty' 2>/dev/null)

          if [[ "$PARENT" == "$UPSTREAM_REPO" ]]; then
            HAS_UPSTREAM=false
            if git -C "$CLAUDE_DIR" remote get-url upstream >/dev/null 2>&1; then
              HAS_UPSTREAM=true
            fi

            UPSTREAM_HEAD=$(timeout 10 git ls-remote "${UPSTREAM_URL}" refs/heads/main 2>/dev/null | cut -f1)
            LOCAL_HEAD=$(git -C "$CLAUDE_DIR" rev-parse HEAD 2>/dev/null)
            FORK_OWNER=$(echo "$OWNER_REPO" | cut -d/ -f1)

            if [[ -n "$UPSTREAM_HEAD" && "$UPSTREAM_HEAD" != "$LOCAL_HEAD" ]]; then
              write_cache "fork" "1" "has_upstream=${HAS_UPSTREAM}
fork_owner=${FORK_OWNER}"
            else
              write_cache "fork" "0" "has_upstream=${HAS_UPSTREAM}
fork_owner=${FORK_OWNER}"
            fi
          else
            # Not a GitHub fork — check marker file for renamed clones
            if check_marker_file; then
              HAS_UPSTREAM=false
              if git -C "$CLAUDE_DIR" remote get-url upstream >/dev/null 2>&1; then
                HAS_UPSTREAM=true
              fi

              UPSTREAM_HEAD=$(timeout 10 git ls-remote "${UPSTREAM_URL}" refs/heads/main 2>/dev/null | cut -f1)
              LOCAL_HEAD=$(git -C "$CLAUDE_DIR" rev-parse HEAD 2>/dev/null)

              if [[ -n "$UPSTREAM_HEAD" && "$UPSTREAM_HEAD" != "$LOCAL_HEAD" ]]; then
                write_cache "renamed_clone" "1" "has_upstream=${HAS_UPSTREAM}"
              else
                write_cache "renamed_clone" "0" "has_upstream=${HAS_UPSTREAM}"
              fi
            else
              write_cache "unrelated" "0"
            fi
          fi
        fi
      else
        # gh unavailable — marker file can still detect renamed clones without gh
        if check_marker_file; then
          HAS_UPSTREAM=false
          if git -C "$CLAUDE_DIR" remote get-url upstream >/dev/null 2>&1; then
            HAS_UPSTREAM=true
          fi

          UPSTREAM_HEAD=$(timeout 10 git ls-remote "${UPSTREAM_URL}" refs/heads/main 2>/dev/null | cut -f1)
          LOCAL_HEAD=$(git -C "$CLAUDE_DIR" rev-parse HEAD 2>/dev/null)

          if [[ -n "$UPSTREAM_HEAD" && "$UPSTREAM_HEAD" != "$LOCAL_HEAD" ]]; then
            write_cache "renamed_clone" "1" "has_upstream=${HAS_UPSTREAM}"
          else
            write_cache "renamed_clone" "0" "has_upstream=${HAS_UPSTREAM}"
          fi
        else
          write_cache "gh_unavailable" "0" "reason=${GH_ISSUE}"
        fi
      fi
    fi

    exit 0
  fi
fi

# --- Scenario 3: Plugin install (no git repo, or non-github remote) ---

if [[ -n "$CLAUDE_PLUGIN_ROOT" && -f "$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json" ]]; then
  INSTALLED_VERSION=$(grep -o '"version"[[:space:]]*:[[:space:]]*"[^"]*"' \
    "$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json" | cut -d'"' -f4)

  if needs_refresh; then
    if check_gh; then
      LATEST_VERSION=$(timeout 10 gh api "repos/${UPSTREAM_REPO}/releases/latest" --jq '.tag_name' 2>&1)
      GH_RC=$?
      LATEST_VERSION=$(echo "$LATEST_VERSION" | tr -d 'v')

      if [[ $GH_RC -ne 0 || -z "$LATEST_VERSION" ]]; then
        write_cache "plugin" "0" "reason=failed to fetch latest release"
      elif [[ "$INSTALLED_VERSION" != "$LATEST_VERSION" ]]; then
        write_cache "plugin" "1" "installed=${INSTALLED_VERSION}
latest=${LATEST_VERSION}"
      else
        write_cache "plugin" "0"
      fi
    else
      write_cache "gh_unavailable" "0" "reason=${GH_ISSUE}"
    fi
  fi

fi
