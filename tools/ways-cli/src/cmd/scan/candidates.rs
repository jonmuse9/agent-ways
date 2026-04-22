//! Candidate collection: finding, parsing, and filtering way files.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::session;

use super::WayCandidate;

// ── Collection ─────────────────────────────────────────────────

pub(crate) fn collect_candidates(project_dir: &str) -> Vec<WayCandidate> {
    let mut candidates = Vec::new();

    // Project-local first — IDs must carry the encoded project prefix so they
    // match corpus entries (corpus stores "C--foo-bar/adr", not bare "adr").
    let project_ways = PathBuf::from(project_dir).join(".claude/ways");
    if project_ways.is_dir() {
        let prefix = find_project_encoded_name(project_dir)
            .map(|n| format!("{n}/"))
            .unwrap_or_default();
        collect_from_dir(&project_ways, &prefix, &mut candidates);
    }

    // Global
    let global_ways = super::scoring::home_dir().join(".claude/hooks/ways");
    collect_from_dir(&global_ways, "", &mut candidates);

    candidates
}

pub(crate) fn collect_checks(project_dir: &str) -> Vec<WayCandidate> {
    let mut candidates = Vec::new();

    let project_ways = PathBuf::from(project_dir).join(".claude/ways");
    if project_ways.is_dir() {
        let prefix = find_project_encoded_name(project_dir)
            .map(|n| format!("{n}/"))
            .unwrap_or_default();
        collect_checks_from_dir(&project_ways, &prefix, &mut candidates);
    }

    let global_ways = super::scoring::home_dir().join(".claude/hooks/ways");
    collect_checks_from_dir(&global_ways, "", &mut candidates);

    candidates
}

fn collect_from_dir(dir: &Path, id_prefix: &str, out: &mut Vec<WayCandidate>) {
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains(".check.") {
            continue;
        }

        let raw = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let content = if raw.contains('\r') { raw.replace("\r\n", "\n").replace('\r', "\n") } else { raw };
        if !content.starts_with("---\n") {
            continue;
        }

        let relative = way_id_from_path(path, dir);
        if relative.is_empty() {
            continue;
        }
        let id = format!("{id_prefix}{relative}");

        // Check domain disable (global ways only — project prefix is not a domain)
        if id_prefix.is_empty() {
            let domain = id.split('/').next().unwrap_or(&id);
            if session::domain_disabled(domain) {
                continue;
            }
        }

        if let Some(candidate) = parse_candidate(&id, path, &content) {
            out.push(candidate);
        }
    }
}

fn collect_checks_from_dir(dir: &Path, id_prefix: &str, out: &mut Vec<WayCandidate>) {
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.contains(".check.md") {
            continue;
        }

        let raw = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let content = if raw.contains('\r') { raw.replace("\r\n", "\n").replace('\r', "\n") } else { raw };
        if !content.starts_with("---\n") {
            continue;
        }

        let relative = way_id_from_path(path, dir);
        if relative.is_empty() {
            continue;
        }
        let id = format!("{id_prefix}{relative}");

        if let Some(candidate) = parse_candidate(&id, path, &content) {
            out.push(candidate);
        }
    }
}

/// Look up the encoded project directory name (as stored in `~/.claude/projects/`)
/// for the given real project path. Returns `None` if not found.
fn find_project_encoded_name(project_dir: &str) -> Option<String> {
    let projects_dir = super::scoring::home_dir().join(".claude/projects");
    if !projects_dir.is_dir() {
        return None;
    }
    let canonical = std::fs::canonicalize(project_dir).ok()?;

    for entry in std::fs::read_dir(&projects_dir).ok()?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let encoded = entry.file_name().to_string_lossy().to_string();
        let idx = projects_dir.join(&encoded).join("sessions-index.json");
        if !idx.is_file() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&idx) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(path_str) = parsed["entries"][0]["projectPath"].as_str() {
                    if let Ok(canonical_indexed) = std::fs::canonicalize(path_str) {
                        if canonical_indexed == canonical {
                            return Some(encoded);
                        }
                    }
                }
            }
        }
    }
    None
}

// ── Parsing ────────────────────────────────────────────────────

fn parse_candidate(id: &str, path: &Path, content: &str) -> Option<WayCandidate> {
    let fm = extract_frontmatter(content)?;

    Some(WayCandidate {
        id: id.to_string(),
        path: path.to_path_buf(),
        pattern: get_fm_field(&fm, "pattern"),
        commands: get_fm_field(&fm, "commands"),
        files: get_fm_field(&fm, "files"),
        description: get_fm_field(&fm, "description").unwrap_or_default(),
        vocabulary: get_fm_field(&fm, "vocabulary").unwrap_or_default(),
        // threshold: only read for ways with trigger: context-threshold (percentage).
        // Post-ADR-125, no semantic/BM25 meaning; default 0.0 is never compared for other triggers.
        threshold: get_fm_field(&fm, "threshold")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        embed_threshold: get_fm_field(&fm, "embed_threshold").and_then(|s| s.parse().ok()),
        // config::global() — future migration: ctx.config.default_scope
        scope: get_fm_field(&fm, "scope")
            .unwrap_or_else(|| crate::config::global().default_scope.clone()),
        when_project: get_when_field(&fm, "project"),
        when_file_exists: get_when_field(&fm, "file_exists"),
        trigger: get_fm_field(&fm, "trigger"),
        repeat: get_fm_field(&fm, "repeat").as_deref() == Some("true"),
        trigger_path: get_fm_field(&fm, "path"),
    })
}

pub(crate) fn way_id_from_path(path: &Path, base: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    parent
        .strip_prefix(base)
        .unwrap_or(parent)
        .display()
        .to_string()
}

pub(crate) fn extract_frontmatter(content: &str) -> Option<String> {
    if !content.starts_with("---\n") {
        return None;
    }
    let rest = &content[4..];
    let end = rest.find("\n---\n").or_else(|| rest.find("\n---"))?;
    Some(rest[..end].to_string())
}

pub(crate) fn get_fm_field(fm: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    for line in fm.lines() {
        if let Some(val) = line.strip_prefix(&prefix) {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub(crate) fn get_when_field(fm: &str, name: &str) -> Option<String> {
    let mut in_when = false;
    let prefix = format!("  {name}:");
    for line in fm.lines() {
        if line == "when:" {
            in_when = true;
            continue;
        }
        if in_when {
            if let Some(val) = line.strip_prefix(&prefix) {
                return Some(val.trim().to_string());
            }
            if !line.starts_with("  ") && !line.is_empty() {
                break;
            }
        }
    }
    None
}

pub(crate) fn check_when(
    when_project: &Option<String>,
    when_file_exists: &Option<String>,
    project_dir: &str,
) -> bool {
    if when_project.is_none() && when_file_exists.is_none() {
        return true;
    }

    if let Some(ref wp) = when_project {
        let expanded = wp.replace("~", &super::scoring::home_dir().display().to_string());
        let resolved = std::fs::canonicalize(&expanded)
            .unwrap_or_else(|_| PathBuf::from(&expanded));
        let current = std::fs::canonicalize(project_dir)
            .unwrap_or_else(|_| PathBuf::from(project_dir));
        if resolved != current {
            return false;
        }
    }

    if let Some(ref wfe) = when_file_exists {
        let resolved_dir = std::fs::canonicalize(project_dir)
            .unwrap_or_else(|_| PathBuf::from(project_dir));
        if !resolved_dir.join(wfe).exists() {
            return false;
        }
    }

    true
}
