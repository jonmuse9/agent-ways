use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A discovered way file with its derived identity.
#[derive(Debug)]
pub struct WayFile {
    /// Absolute path to the way file
    pub path: PathBuf,
    /// Way ID derived from directory structure (e.g., "code/quality")
    pub id: String,
    /// Top-level domain (e.g., "softwaredev")
    pub domain: String,
}

/// Scan a directory for way files (identified by YAML frontmatter with `description:` field).
pub fn scan_ways(root: &Path) -> Result<Vec<WayFile>> {
    let mut ways = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        // Skip check files
        if path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."))
        {
            continue;
        }

        if has_way_frontmatter(path) {
            if let Some(way) = way_from_path(path, root) {
                ways.push(way);
            }
        }
    }

    ways.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ways)
}

/// Check if a file has YAML frontmatter containing a `description:` field.
fn has_way_frontmatter(path: &Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let mut in_frontmatter = false;
    for (i, line) in content.lines().enumerate() {
        if i == 0 && line == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if line == "---" {
                return false; // closed without description
            }
            if line.starts_with("description:") {
                return true;
            }
        }
    }
    false
}

/// Derive WayFile identity from filesystem path relative to the ways root.
fn way_from_path(path: &Path, root: &Path) -> Option<WayFile> {
    let parent = path.parent()?;
    let rel = parent.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel.components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();

    if components.is_empty() {
        return None;
    }

    let domain = components[0].to_string();
    let id = if components.len() > 1 {
        components[1..].join("/")
    } else {
        domain.clone()
    };

    Some(WayFile {
        path: path.to_path_buf(),
        id,
        domain,
    })
}
