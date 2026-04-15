use anyhow::{anyhow, Context, Result};
use sensor_trait::Curve;
use serde::Deserialize;
use std::path::Path;

/// Parsed YAML frontmatter from a way file.
#[derive(Debug, Deserialize, Default)]
pub struct Frontmatter {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub vocabulary: Option<String>,
    #[serde(default)]
    pub threshold: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)] // parsed for serde compat, accessed via scan's own scope field
    pub scope: Option<String>,
    #[serde(default)]
    pub embed_threshold: Option<f64>,
    /// ADR-123 firing-dynamics curve. Required for ways that fire
    /// predictively or reactively; optional during parse for static
    /// consumers (tune/corpus/graph) that don't invoke the engine.
    #[serde(default)]
    pub curve: Option<Curve>,
}

/// Extract YAML frontmatter from a way file.
pub fn parse(path: &Path) -> Result<Frontmatter> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let yaml_str = extract_frontmatter_str(&content)
        .with_context(|| format!("no frontmatter in {}", path.display()))?;

    detect_legacy_redisclose(&yaml_str)
        .with_context(|| format!("legacy frontmatter in {}", path.display()))?;

    serde_yaml::from_str(&yaml_str)
        .with_context(|| format!("parsing frontmatter in {}", path.display()))
}

/// Extract the raw YAML string between `---` delimiters.
fn extract_frontmatter_str(content: &str) -> Option<String> {
    let mut lines = content.lines();

    if lines.next()? != "---" {
        return None;
    }

    let mut yaml_lines = Vec::new();
    for line in lines {
        if line == "---" {
            return Some(yaml_lines.join("\n"));
        }
        yaml_lines.push(line);
    }
    None
}

/// Scan raw frontmatter YAML for a top-level `redisclose:` field.
/// Returns a migration-pointing error if present. Called from `parse`
/// so any leftover legacy field errors loudly at load time (ADR-123 C3).
pub fn detect_legacy_redisclose(yaml_str: &str) -> Result<()> {
    for line in yaml_str.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("redisclose:") {
            return Err(anyhow!(
                "legacy `redisclose:` field is no longer supported — \
                migrate to an explicit `curve:` block per ADR-123. See \
                docs/architecture/system/ADR-123-firing-dynamics-progression-axis-unification.md \
                for migration guidance."
            ));
        }
    }
    Ok(())
}

/// A single locale entry from a .locales.jsonl file.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct LocaleEntry {
    pub lang: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_threshold: Option<f64>,
}

/// Parse a .locales.jsonl file into locale entries.
pub fn parse_locales_jsonl(path: &Path) -> Result<Vec<LocaleEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: LocaleEntry = serde_json::from_str(line)
            .with_context(|| format!("parsing locale entry in {}", path.display()))?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Extract the `<!-- epistemic: VALUE -->` comment from the body of a way file.
pub fn extract_epistemic(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- epistemic:") {
            if let Some(value) = rest.strip_suffix("-->") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Extract See Also references from the body of a way file.
/// Returns (target_name, target_domain, label) tuples.
pub fn extract_see_also(content: &str) -> Vec<(String, String, String)> {
    let mut refs = Vec::new();
    let mut in_see_also = false;

    for line in content.lines() {
        if line.starts_with("## See Also") {
            in_see_also = true;
            continue;
        }
        if in_see_also && line.starts_with("## ") {
            break;
        }
        if in_see_also && line.starts_with("- ") {
            if let Some(parsed) = parse_see_also_line(line) {
                refs.push(parsed);
            }
        }
    }
    refs
}

/// Parse a See Also line like `- code/testing(softwaredev) — quality requires test coverage`
fn parse_see_also_line(line: &str) -> Option<(String, String, String)> {
    let line = line.strip_prefix("- ")?;

    let paren_open = line.find('(')?;
    let paren_close = line.find(')')?;

    let name = line[..paren_open].trim().to_string();
    let domain = line[paren_open + 1..paren_close].trim().to_string();

    let label = line[paren_close + 1..]
        .trim()
        .strip_prefix('\u{2014}') // em dash
        .or_else(|| line[paren_close + 1..].trim().strip_prefix("--"))
        .unwrap_or("")
        .trim()
        .to_string();

    Some((name, domain, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml(yaml: &str) -> Frontmatter {
        serde_yaml::from_str(yaml).expect("frontmatter parse failed")
    }

    #[test]
    fn parses_curve_exponential() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: Exponential
  half_life: 50000
"#,
        );
        match fm.curve {
            Some(Curve::Exponential { half_life }) => assert_eq!(half_life, 50_000),
            other => panic!("expected Exponential, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_action_potential() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: ActionPotential
  burst_threshold: 3
  peak_multiplier: 2.0
  absolute_refractory: 5000
  multiplier_half_life: 25000
"#,
        );
        match fm.curve {
            Some(Curve::ActionPotential {
                burst_threshold,
                peak_multiplier,
                absolute_refractory,
                multiplier_half_life,
            }) => {
                assert_eq!(burst_threshold, 3);
                assert!((peak_multiplier - 2.0).abs() < 1e-9);
                assert_eq!(absolute_refractory, 5_000);
                assert_eq!(multiplier_half_life, 25_000);
            }
            other => panic!("expected ActionPotential, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_progressive_staircase() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: ProgressiveStaircase
  steps:
    - [0, 1.0]
    - [15000, 0.5]
    - [40000, 0.2]
"#,
        );
        match fm.curve {
            Some(Curve::ProgressiveStaircase { steps }) => {
                assert_eq!(steps.len(), 3);
                assert_eq!(steps[0], (0, 1.0));
                assert_eq!(steps[1], (15_000, 0.5));
                assert_eq!(steps[2], (40_000, 0.2));
            }
            other => panic!("expected ProgressiveStaircase, got {:?}", other),
        }
    }

    #[test]
    fn parses_curve_flat() {
        let fm = parse_yaml(
            r#"
description: test way
curve:
  type: Flat
  suppression: 15000
"#,
        );
        match fm.curve {
            Some(Curve::Flat { suppression }) => assert_eq!(suppression, 15_000),
            other => panic!("expected Flat, got {:?}", other),
        }
    }

    #[test]
    fn curve_field_is_optional_for_static_consumers() {
        // Static consumers like `ways tune` and `ways corpus` parse way
        // frontmatter but don't invoke the firing engine, so a missing
        // curve: block must not error at parse time. The engine path in
        // session.rs enforces presence at the fire site.
        let fm = parse_yaml("description: no curve\n");
        assert!(fm.curve.is_none());
    }

    #[test]
    fn detect_legacy_redisclose_flags_top_level_field() {
        let yaml = "description: test way\nredisclose: 25\n";
        let err = detect_legacy_redisclose(yaml).expect_err("should reject");
        let msg = err.to_string();
        assert!(msg.contains("redisclose"), "error message: {}", msg);
        assert!(msg.contains("curve"), "error message: {}", msg);
    }

    #[test]
    fn detect_legacy_redisclose_passes_clean_frontmatter() {
        let yaml = r#"
description: test way
curve:
  type: Exponential
  half_life: 50000
"#;
        detect_legacy_redisclose(yaml).expect("clean yaml should pass");
    }

    #[test]
    fn detect_legacy_redisclose_flags_indented_top_level() {
        // Top-level field can have leading whitespace in some yaml styles.
        let yaml = "description: test\n  redisclose: 25\n";
        let err = detect_legacy_redisclose(yaml).expect_err("should reject indented");
        assert!(err.to_string().contains("redisclose"));
    }
}
