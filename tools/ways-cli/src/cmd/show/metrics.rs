//! Session metrics, git operations, and side-effectful display functions.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::session;
use super::helpers::home_dir;

/// Walk up the way ID path to compute tree depth, parent, and epoch distance.
pub(crate) fn compute_tree_metrics(
    way_id: &str,
    session_id: &str,
) -> (u32, Option<String>, Option<u64>, Option<u64>) {
    let mut depth = 0u32;
    let mut parent_id: Option<String> = None;
    let mut parent_epoch: Option<u64> = None;
    let mut epoch_from_parent: Option<u64> = None;
    let current_epoch = session::get_epoch(session_id);

    let mut path = way_id.to_string();
    while let Some(idx) = path.rfind('/') {
        path = path[..idx].to_string();
        if session::way_is_shown(&path, session_id) {
            depth += 1;
            if parent_id.is_none() {
                parent_id = Some(path.clone());
                let pe = session::get_way_epoch(&path, session_id);
                parent_epoch = Some(pe);
                epoch_from_parent = Some(current_epoch.saturating_sub(pe));
            }
        }
    }

    (depth, parent_id, parent_epoch, epoch_from_parent)
}

/// Count sibling ways (total and fired) under the same parent path.
pub(crate) fn count_siblings(way_id: &str, project_dir: &str, session_id: &str) -> (u32, u32) {
    let parent_path = match way_id.rfind('/') {
        Some(idx) => &way_id[..idx],
        None => return (0, 0),
    };

    let mut total = 0u32;
    let mut fired = 0u32;

    let bases = [
        PathBuf::from(project_dir).join(".claude/ways"),
        home_dir().join(".claude/hooks/ways"),
    ];

    for base in &bases {
        let parent_dir = base.join(parent_path);
        if !parent_dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&parent_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                    continue;
                }
                let sib_name = entry.file_name().to_string_lossy().to_string();
                let sib_id = format!("{parent_path}/{sib_name}");
                // Check it has a way file
                if session::resolve_way_file(&sib_id, project_dir).is_some() {
                    total += 1;
                    if session::way_is_shown(&sib_id, session_id) {
                        fired += 1;
                    }
                }
            }
        }
    }

    (total, fired)
}

/// Get a human-readable version string from git describe.
pub(crate) fn git_version(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.display().to_string(), "describe", "--tags", "--match", "v*", "--always", "--dirty"])
        .output();

    let raw = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => return "unknown".to_string(),
    };

    let (describe, is_dirty) = if raw.ends_with("-dirty") {
        (raw.trim_end_matches("-dirty"), true)
    } else {
        (raw.as_str(), false)
    };

    // Parse: "v0.1.0-29-ge0841be" or "v0.1.0" or "e0841be"
    let version = if let Some(caps) = parse_git_describe(describe) {
        if caps.distance > 0 {
            format!("{} + {} commits ({})", caps.tag, caps.distance, caps.hash)
        } else {
            format!("{} (release)", caps.tag)
        }
    } else if describe.starts_with('v') {
        format!("{describe} (release)")
    } else {
        describe.to_string()
    };

    if is_dirty {
        format!("{version} · dirty")
    } else {
        version
    }
}

pub(crate) struct GitDescribe {
    pub tag: String,
    pub distance: u32,
    pub hash: String,
}

pub(crate) fn parse_git_describe(s: &str) -> Option<GitDescribe> {
    // "v0.1.0-29-ge0841be"
    let last_dash = s.rfind('-')?;
    let hash = &s[last_dash + 1..];
    if !hash.starts_with('g') {
        return None;
    }
    let rest = &s[..last_dash];
    let second_dash = rest.rfind('-')?;
    let distance: u32 = rest[second_dash + 1..].parse().ok()?;
    let tag = &rest[..second_dash];
    Some(GitDescribe {
        tag: tag.to_string(),
        distance,
        hash: hash[1..].to_string(), // strip 'g' prefix
    })
}

/// Print update availability status from the cached state file.
pub(crate) fn update_status_text() -> String {
    let uid = unsafe { libc_getuid() };
    let cache_file = format!("/tmp/.claude-config-update-state-{uid}");
    let content = match std::fs::read_to_string(&cache_file) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let get = |key: &str| -> Option<String> {
        content
            .lines()
            .find(|l| l.starts_with(&format!("{key}=")))
            .map(|l| l[key.len() + 1..].to_string())
    };

    let cached_type = get("type").unwrap_or_default();
    let behind: u32 = get("behind").and_then(|s| s.parse().ok()).unwrap_or(0);
    let has_upstream = get("has_upstream").unwrap_or_default() == "true";
    let upstream_repo = "aaronsb/agent-ways";

    if behind == 0 {
        return String::new();
    }

    let mut out = String::from("\n");
    match cached_type.as_str() {
        "clone" => {
            out.push_str(&format!("**{behind} commit(s) behind origin/main.** Run: `cd ~/.claude && git pull`\n"));
        }
        "fork" | "renamed_clone" => {
            if has_upstream {
                out.push_str(&format!("**Behind {upstream_repo}.** Run: `cd ~/.claude && git fetch upstream && git merge upstream/main`\n"));
            } else {
                out.push_str(&format!("**Behind {upstream_repo}.** First add upstream, then sync:\n"));
                out.push_str(&format!("`git -C ~/.claude remote add upstream https://github.com/{upstream_repo}`\n"));
                out.push_str("`cd ~/.claude && git fetch upstream && git merge upstream/main`\n");
            }
        }
        "plugin" => {
            let installed = get("installed").unwrap_or_default();
            let latest = get("latest").unwrap_or_default();
            out.push_str(&format!("**Plugin update available (v{installed} -> v{latest}).** Run: `/plugin update disciplined-methodology`\n"));
        }
        _ => {}
    }
    out
}

/// Return dirty file status from git.
pub(crate) fn dirty_status_text(claude_dir: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &claude_dir.display().to_string(), "status", "--short"])
        .output();

    let files: Vec<String> = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.split_whitespace().last().unwrap_or("").to_string())
                .collect()
        }
        _ => return String::new(),
    };

    if files.is_empty() {
        return String::new();
    }

    let count = files.len();
    let mut out = String::from("\n");
    if count >= 4 {
        out.push_str(&format!("**Uncommitted local changes ({count} files)** — not tracked by git.\n"));
        out.push_str("Other sessions won't see these. Commit to keep, or discard to match remote.\n");
    } else {
        let s = if count != 1 { "s" } else { "" };
        out.push_str(&format!("**Uncommitted local changes ({count} file{s}):**\n"));
    }

    let max_show = 5;
    for f in files.iter().take(max_show) {
        out.push_str(&format!("- `{f}`\n"));
    }
    if count > max_show {
        out.push_str(&format!("- ... and {} more\n", count - max_show));
    }
    if count < 4 {
        out.push_str("\n_Run `git -C ~/.claude status` to review._\n");
    }
    out
}

/// Get uid without pulling in libc crate.
pub(crate) unsafe fn libc_getuid() -> u32 {
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn getuid() -> u32;
        }
        getuid()
    }
    #[cfg(not(unix))]
    0
}
