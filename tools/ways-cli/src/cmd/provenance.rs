use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn run(ways_dir: Option<String>) -> Result<()> {
    let manifest = generate_manifest(ways_dir)?;

    println!("{}", serde_json::to_string_pretty(&manifest)?);

    let ways_len = manifest["ways_scanned"].as_u64().unwrap_or(0);
    let with_len = manifest["ways_with_provenance"].as_u64().unwrap_or(0);
    let without_len = manifest["ways_without_provenance"].as_u64().unwrap_or(0);
    let policy_len = manifest["coverage"]["by_policy"].as_object().map(|m: &serde_json::Map<_, _>| m.len()).unwrap_or(0);
    let control_len = manifest["coverage"]["by_control"].as_object().map(|m: &serde_json::Map<_, _>| m.len()).unwrap_or(0);

    eprintln!("Ways scanned: {}", ways_len);
    eprintln!("  With provenance: {} ({:.0}%)", with_len, if ways_len == 0 { 0.0 } else { with_len as f64 / ways_len as f64 * 100.0 });
    eprintln!("  Without provenance: {}", without_len);
    eprintln!("  Policy sources: {}", policy_len);
    eprintln!("  Control references: {}", control_len);

    Ok(())
}

/// Generate the full provenance manifest as a JSON Value.
/// Used by both `ways provenance` and `ways governance`.
pub fn generate_manifest(ways_dir: Option<String>) -> Result<Value> {
    let root = ways_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/hooks/ways"));

    let (ways, with_prov, without_prov) = scan_provenance(&root)?;

    let mut by_policy: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_control: HashMap<String, Vec<String>> = HashMap::new();

    for (way_key, way_data) in &ways {
        if let Some(prov) = way_data.get("provenance").and_then(|v| v.as_object()) {
            if let Some(policies) = prov.get("policy").and_then(|v| v.as_array()) {
                for policy in policies {
                    if let Some(uri) = policy.get("uri").and_then(|v| v.as_str()) {
                        by_policy
                            .entry(uri.to_string())
                            .or_default()
                            .push(way_key.clone());
                    }
                }
            }
            if let Some(controls) = prov.get("controls").and_then(|v| v.as_array()) {
                for control in controls {
                    let cid = control
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| control.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !cid.is_empty() {
                        by_control
                            .entry(cid)
                            .or_default()
                            .push(way_key.clone());
                    }
                }
            }
        }
    }

    Ok(json!({
        "manifest_version": "1.0.0",
        "generator": "ways provenance",
        "ways_scanned": ways.len(),
        "ways_with_provenance": with_prov.len(),
        "ways_without_provenance": without_prov.len(),
        "ways": ways,
        "coverage": {
            "with_provenance": with_prov,
            "without_provenance": without_prov,
            "by_policy": by_policy,
            "by_control": by_control,
        }
    }))
}

#[allow(clippy::type_complexity)]
fn scan_provenance(root: &Path) -> Result<(HashMap<String, serde_json::Value>, Vec<String>, Vec<String>)> {
    let mut ways: HashMap<String, serde_json::Value> = HashMap::new();
    let mut with_prov = Vec::new();
    let mut without_prov = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."))
        {
            continue;
        }

        // Check frontmatter exists
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !crate::util::has_frontmatter(&content) {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path);
        let way_key = rel
            .parent()
            .unwrap_or(Path::new(""))
            .display()
            .to_string();

        if way_key.is_empty() || !way_key.contains('/') {
            continue; // need at least domain/way
        }

        // Check for provenance.yaml sidecar
        let sidecar = path.parent().unwrap_or(path).join("provenance.yaml");
        let prov = if sidecar.is_file() {
            parse_sidecar(&sidecar).ok()
        } else {
            None
        };

        if let Some(ref p) = prov {
            with_prov.push(way_key.clone());
            ways.insert(way_key, json!({ "path": rel.display().to_string(), "provenance": p }));
        } else {
            without_prov.push(way_key.clone());
            ways.insert(way_key, json!({ "path": rel.display().to_string(), "provenance": null }));
        }
    }

    with_prov.sort();
    without_prov.sort();
    Ok((ways, with_prov, without_prov))
}

fn parse_sidecar(path: &Path) -> Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&content)?;
    Ok(yaml_to_json(&parsed))
}

fn yaml_to_json(v: &serde_yaml::Value) -> serde_json::Value {
    match v {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => json!(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else if let Some(f) = n.as_f64() {
                json!(f)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => json!(s),
        serde_yaml::Value::Sequence(seq) => {
            json!(seq.iter().map(yaml_to_json).collect::<Vec<_>>())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map {
                if let Some(key) = k.as_str() {
                    obj.insert(key.to_string(), yaml_to_json(val));
                }
            }
            serde_json::Value::Object(obj)
        }
        _ => serde_json::Value::Null,
    }
}

use crate::util::home_dir;
