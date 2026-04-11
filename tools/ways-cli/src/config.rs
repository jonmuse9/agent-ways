//! Declarative configuration for ways.
//!
//! Resolution order (later overrides earlier):
//!   1. Built-in defaults
//!   2. ~/.claude/ways.json (legacy, backward compat)
//!   3. $XDG_CONFIG_HOME/ways/config.yaml (user scope)
//!   4. $PROJECT/.claude/ways.yaml (project scope)

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Global config, loaded once on first access.
/// Access via `config::global()` — grep-friendly for future context refactor.
static GLOBAL: LazyLock<Config> = LazyLock::new(|| {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    Config::load(&project_dir)
});

/// Access the process-wide config. Every call site is a future `ctx.config` migration point.
pub fn global() -> &'static Config {
    &GLOBAL
}

/// Ways configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default BM25 threshold for ways without explicit threshold
    pub bm25_threshold: f64,
    /// Parent threshold multiplier (child threshold = parent * this)
    pub parent_threshold_multiplier: f64,
    /// Default scope for ways without explicit scope
    pub default_scope: String,
    /// Output language (e.g., "en", "ja", "auto")
    pub language: String,
    /// Disabled domains (e.g., ["ea", "itops"])
    pub disabled_domains: Vec<String>,
    /// Matcher preference: "auto", "embedding", "bm25"
    pub matcher: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bm25_threshold: 2.0,
            parent_threshold_multiplier: 0.8,
            default_scope: "agent".to_string(),
            language: "auto".to_string(),
            disabled_domains: Vec::new(),
            matcher: "auto".to_string(),
        }
    }
}

impl Config {
    /// Load config with full resolution chain.
    pub fn load(project_dir: &str) -> Self {
        let mut cfg = Config::default();

        // Layer 1: legacy ways.json
        let ways_json = home_dir().join(".claude/ways.json");
        if let Ok(content) = std::fs::read_to_string(&ways_json) {
            cfg.apply_ways_json(&content);
        }

        // Layer 2: XDG user config
        let xdg_config = xdg_config_dir().join("ways/config.yaml");
        if let Ok(content) = std::fs::read_to_string(&xdg_config) {
            cfg.apply_yaml(&content);
        }

        // Layer 3: project overlay
        let project_config = Path::new(project_dir).join(".claude/ways.yaml");
        if let Ok(content) = std::fs::read_to_string(&project_config) {
            cfg.apply_yaml(&content);
        }

        cfg
    }

    /// Apply values from legacy ways.json.
    fn apply_ways_json(&mut self, content: &str) {
        let v: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(lang) = v.get("output_language").and_then(|v| v.as_str()) {
            self.language = lang.to_string();
        }

        if let Some(disabled) = v.get("disabled").and_then(|v| v.as_array()) {
            self.disabled_domains = disabled
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }

        // Accept both field names: "engine" (new) and "semantic_engine" (legacy)
        if let Some(engine) = v.get("engine")
            .or_else(|| v.get("semantic_engine"))
            .and_then(|v| v.as_str())
        {
            self.matcher = engine.to_string();
        }
    }

    /// Apply values from a YAML config file.
    fn apply_yaml(&mut self, content: &str) {
        let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[ways] config: parse error: {}", e);
                return;
            }
        };

        if let Some(v) = doc.get("language").and_then(|v| v.as_str()) {
            self.language = v.to_string();
        }
        if let Some(v) = doc.get("matcher").and_then(|v| v.as_str()) {
            self.matcher = v.to_string();
        }
        if let Some(v) = doc.get("bm25_threshold").and_then(|v| v.as_f64()) {
            self.bm25_threshold = v;
        }
        if let Some(v) = doc.get("parent_threshold_multiplier").and_then(|v| v.as_f64()) {
            self.parent_threshold_multiplier = v;
        }
        if let Some(v) = doc.get("default_scope").and_then(|v| v.as_str()) {
            self.default_scope = v.to_string();
        }
        if let Some(disabled) = doc.get("disabled_domains").and_then(|v| v.as_sequence()) {
            self.disabled_domains = disabled
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    /// Initialize user config at XDG path.
    pub fn init_user_config() -> PathBuf {
        let path = xdg_config_dir().join("ways/config.yaml");
        if path.exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = "# ways configuration
# User scope: $XDG_CONFIG_HOME/ways/config.yaml
# Project scope: {project}/.claude/ways.yaml (layered on top)

# language: en          # Output language (en, ja, auto)
# matcher: auto         # Matching engine (auto, embedding, bm25)
# bm25_threshold: 2.0   # Default BM25 score threshold
# default_scope: agent  # Default scope for ways without explicit scope
# disabled_domains: []  # Domains to disable (e.g., [ea, itops])
";
        std::fs::write(&path, content).ok();
        path
    }

    /// Show the config file path.
    pub fn config_path() -> String {
        let xdg = xdg_config_dir().join("ways/config.yaml");
        format!("user:    {}\nlegacy:  {}\nproject: $PROJECT/.claude/ways.yaml",
            xdg.display(),
            home_dir().join(".claude/ways.json").display())
    }
}

fn home_dir() -> PathBuf {
    crate::util::home_dir()
}

fn xdg_config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = Config::default();
        assert_eq!(cfg.bm25_threshold, 2.0);
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.matcher, "auto");
        assert_eq!(cfg.default_scope, "agent");
    }

    #[test]
    fn apply_yaml_overrides() {
        let mut cfg = Config::default();
        cfg.apply_yaml("language: ja\nbm25_threshold: 1.5\nmatcher: embedding");
        assert_eq!(cfg.language, "ja");
        assert_eq!(cfg.bm25_threshold, 1.5);
        assert_eq!(cfg.matcher, "embedding");
    }

    #[test]
    fn apply_ways_json() {
        let mut cfg = Config::default();
        cfg.apply_ways_json(r#"{"output_language":"de","disabled":["ea"],"engine":"bm25"}"#);
        assert_eq!(cfg.language, "de");
        assert_eq!(cfg.disabled_domains, vec!["ea"]);
        assert_eq!(cfg.matcher, "bm25");
    }

    #[test]
    fn yaml_overrides_json() {
        let mut cfg = Config::default();
        cfg.apply_ways_json(r#"{"output_language":"de"}"#);
        cfg.apply_yaml("language: ja");
        assert_eq!(cfg.language, "ja");
    }
}
