//! Permission matching engine for ADR-116.
//!
//! Parses permission strings (Tool, Tool(scope), *) and checks whether
//! a grant satisfies a requirement via containment hierarchy.
//!
//! Rules:
//! - `*` covers everything
//! - `Bash(*)` covers any `Bash(cmd:*)`
//! - `Read` (unscoped) covers `Read(/any/path)`
//! - `Bash(git:*)` covers `Bash(git:status)` but NOT `Bash(*)`

use std::fmt;
use std::path::Path;

/// A parsed permission.
#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    /// Wildcard — covers everything.
    Wildcard,
    /// Unscoped tool access (e.g., `Read`, `Edit`).
    Tool(String),
    /// Scoped tool access (e.g., `Bash(git:*)`, `Read(/home/**)`).
    Scoped(String, String),
}

impl Permission {
    /// Parse a permission string into a Permission.
    ///
    /// Examples: `"*"`, `"Read"`, `"Bash(git:*)"`, `"Write(/home/**)"`.
    pub fn parse(s: &str) -> Option<Permission> {
        let s = s.trim();
        if s == "*" {
            return Some(Permission::Wildcard);
        }
        if let Some(paren_start) = s.find('(') {
            if !s.ends_with(')') {
                return None;
            }
            let tool = s[..paren_start].to_string();
            let scope = s[paren_start + 1..s.len() - 1].to_string();
            if tool.is_empty() || scope.is_empty() {
                return None;
            }
            Some(Permission::Scoped(tool, scope))
        } else if !s.is_empty() && s.chars().next().unwrap().is_ascii_uppercase() {
            Some(Permission::Tool(s.to_string()))
        } else {
            None
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::Wildcard => write!(f, "*"),
            Permission::Tool(t) => write!(f, "{t}"),
            Permission::Scoped(t, s) => write!(f, "{t}({s})"),
        }
    }
}

/// Check if a grant satisfies a requirement.
///
/// A grant satisfies a requirement if the grant is at least as broad:
/// - `*` satisfies any requirement
/// - `Bash(*)` satisfies `Bash(git:*)` but not vice versa
/// - `Read` (unscoped) satisfies `Read(/path)` but not vice versa
/// - `Bash(git:*)` satisfies `Bash(git:status)` (prefix match on command scope)
pub fn grant_satisfies(grant: &Permission, requirement: &Permission) -> bool {
    match (grant, requirement) {
        // Wildcard grant covers everything
        (Permission::Wildcard, _) => true,
        // Nothing else covers a wildcard requirement
        (_, Permission::Wildcard) => false,

        // Unscoped grant covers unscoped or any scoped requirement of the same tool
        (Permission::Tool(gt), Permission::Tool(rt)) => gt == rt,
        (Permission::Tool(gt), Permission::Scoped(rt, _)) => gt == rt,

        // Scoped grant does not cover unscoped requirement
        (Permission::Scoped(_, _), Permission::Tool(_)) => false,

        // Scoped grant covers scoped requirement if tools match and scope contains
        (Permission::Scoped(gt, gs), Permission::Scoped(rt, rs)) => {
            gt == rt && scope_contains(gs, rs)
        }
    }
}

/// Check if a grant scope contains a requirement scope.
///
/// For Bash commands: `*` contains anything, `git:*` contains `git:status`.
/// For paths: glob-style containment — `/home/**` contains `/home/user/file`.
fn scope_contains(grant_scope: &str, req_scope: &str) -> bool {
    // Wildcard scope covers everything
    if grant_scope == "*" {
        return true;
    }
    // Exact match
    if grant_scope == req_scope {
        return true;
    }
    // Command prefix: grant "git:*" matches requirement "git:status"
    if let Some(prefix) = grant_scope.strip_suffix(":*") {
        if let Some(req_prefix) = req_scope.strip_suffix(":*") {
            // git:* covers git:* (already caught by exact match above, but be safe)
            return req_prefix == prefix || req_prefix.starts_with(&format!("{prefix}:"));
        }
        // grant "git:*" covers requirement "git:status"
        return req_scope.starts_with(&format!("{prefix}:"));
    }
    // Path glob: grant "/home/**" matches requirement "/home/user/file"
    if grant_scope.contains("**") {
        let prefix = grant_scope.trim_end_matches("**");
        return req_scope.starts_with(prefix);
    }
    // Tilde expansion: "~/.claude/**" matches "/home/user/.claude/foo"
    if grant_scope.starts_with("~/") {
        if let Some(home) = home_dir() {
            let expanded = format!("{}{}", home.display(), &grant_scope[1..]);
            return scope_contains(&expanded, req_scope);
        }
    }
    false
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

/// Load permissions from settings.json.
///
/// Reads `~/.claude/settings.json` and parses `permissions.allow` array.
pub fn load_settings_permissions(settings_path: &Path) -> Vec<Permission> {
    let content = match std::fs::read_to_string(settings_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Minimal JSON parsing — find "allow": [...] array
    // Using serde_json would add a dependency to agent-fmt; parse manually instead.
    let mut perms = Vec::new();
    let mut in_allow = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("\"allow\"") && trimmed.contains('[') {
            in_allow = true;
            // Check if there are entries on the same line
            if let Some(start) = trimmed.find('[') {
                for entry in extract_string_entries(&trimmed[start..]) {
                    if let Some(p) = Permission::parse(&entry) {
                        perms.push(p);
                    }
                }
            }
            if trimmed.contains(']') {
                in_allow = false;
            }
            continue;
        }
        if in_allow {
            if trimmed.contains(']') {
                // Might have entries before the ]
                for entry in extract_string_entries(trimmed) {
                    if let Some(p) = Permission::parse(&entry) {
                        perms.push(p);
                    }
                }
                in_allow = false;
                continue;
            }
            for entry in extract_string_entries(trimmed) {
                if let Some(p) = Permission::parse(&entry) {
                    perms.push(p);
                }
            }
        }
    }
    perms
}

/// Extract quoted strings from a line (e.g., `"Bash(git:*)"` → `Bash(git:*)`).
fn extract_string_entries(line: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            if !s.is_empty() && s != "allow" {
                entries.push(s);
            }
        }
    }
    entries
}

/// Audit result for a single requirement.
#[derive(Debug)]
pub struct AuditResult {
    /// Source of the requirement (way ID or sensor name).
    pub source: String,
    /// The permission string required.
    pub requirement: String,
    /// Whether it's satisfied by a grant in settings.json.
    pub granted: bool,
}

/// Audit a set of (source, requirements) against granted permissions.
pub fn audit(
    requirements: &[(String, Vec<String>)],
    grants: &[Permission],
) -> Vec<AuditResult> {
    let mut results = Vec::new();
    for (source, reqs) in requirements {
        for req_str in reqs {
            let granted = if let Some(req) = Permission::parse(req_str) {
                grants.iter().any(|g| grant_satisfies(g, &req))
            } else {
                false // malformed requirement
            };
            results.push(AuditResult {
                source: source.clone(),
                requirement: req_str.clone(),
                granted,
            });
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wildcard() {
        assert_eq!(Permission::parse("*"), Some(Permission::Wildcard));
    }

    #[test]
    fn test_parse_tool() {
        assert_eq!(Permission::parse("Read"), Some(Permission::Tool("Read".to_string())));
    }

    #[test]
    fn test_parse_scoped() {
        assert_eq!(
            Permission::parse("Bash(git:*)"),
            Some(Permission::Scoped("Bash".to_string(), "git:*".to_string()))
        );
    }

    #[test]
    fn test_wildcard_covers_all() {
        let grant = Permission::Wildcard;
        let req = Permission::parse("Bash(git:*)").unwrap();
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_unscoped_covers_scoped() {
        let grant = Permission::Tool("Read".to_string());
        let req = Permission::Scoped("Read".to_string(), "/home/user".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_scoped_does_not_cover_unscoped() {
        let grant = Permission::Scoped("Read".to_string(), "/home/**".to_string());
        let req = Permission::Tool("Read".to_string());
        assert!(!grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_wildcard_scope() {
        let grant = Permission::Scoped("Bash".to_string(), "*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_prefix_scope() {
        let grant = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "git:status".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_no_cross_tool() {
        let grant = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "gh:*".to_string());
        assert!(!grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_path_glob_containment() {
        let grant = Permission::Scoped("Write".to_string(), "/home/aaron/.claude/**".to_string());
        let req = Permission::Scoped("Write".to_string(), "/home/aaron/.claude/foo".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_audit_finds_missing() {
        let grants = vec![
            Permission::parse("Bash(git:*)").unwrap(),
            Permission::parse("Read").unwrap(),
        ];
        let reqs = vec![
            ("github".to_string(), vec!["Bash(git:*)".to_string(), "Bash(gh:*)".to_string()]),
        ];
        let results = audit(&reqs, &grants);
        assert_eq!(results.len(), 2);
        assert!(results[0].granted);  // git:* — granted
        assert!(!results[1].granted); // gh:* — missing
    }
}
