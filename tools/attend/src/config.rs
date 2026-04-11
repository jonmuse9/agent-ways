//! Configuration loader for attend.
//!
//! Two-layer config mirrors ways scoping:
//!   ~/.config/attend/config.yaml          # user scope — always loaded
//!   {project}/.claude/attend.yaml         # project scope — layered on top
//!
//! Project config uses +/- to modify the sensor set.
//!
//! Minimal YAML subset parser — no serde dependency.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Top-level configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub governor: GovernorConfig,
    pub sensors: HashMap<String, SensorConfig>,
}

/// Disclosure governor parameters.
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub base_cooldown: Duration,
    pub max_per_window: u32,
    pub rate_window: Duration,
}

/// Per-sensor configuration.
#[derive(Debug, Clone)]
pub struct SensorConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub min_interval: Duration,
    pub threshold: f64,
    pub decay_threshold: u32,
    /// For script sensors: path to the script (relative to project root)
    pub script: Option<String>,
    /// Permission requirements (ADR-116) — tool permissions this sensor needs.
    pub requires: Vec<String>,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            base_cooldown: Duration::from_secs(15),
            max_per_window: 3,
            rate_window: Duration::from_secs(120),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut sensors = HashMap::new();
        sensors.insert("context".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(60),
            min_interval: Duration::from_secs(20),
            threshold: 1.5,
            decay_threshold: 3,
            script: None,
            requires: vec!["Read".to_string()],
        });
        sensors.insert("git".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(10),
            threshold: 2.0,
            decay_threshold: 4,
            script: None,
            requires: vec!["Bash(git:*)".to_string()],
        });
        sensors.insert("peers".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(10),
            threshold: 2.0,
            decay_threshold: 5,
            script: None,
            requires: vec!["Read".to_string()],
        });
        sensors.insert("processes".to_string(), SensorConfig {
            enabled: true,
            interval: Duration::from_secs(30),
            min_interval: Duration::from_secs(5),
            threshold: 2.0,
            decay_threshold: 5,
            script: None,
            requires: vec!["Bash(ps:*)".to_string()],
        });
        Self {
            governor: GovernorConfig::default(),
            sensors,
        }
    }
}

impl Config {
    /// Load config from user scope, then layer project scope on top.
    pub fn load(working_dir: &str) -> Self {
        let mut config = Config::default();

        // User scope: ~/.config/attend/config.yaml
        let user_path = user_config_path();
        if user_path.exists() {
            if let Ok(content) = fs::read_to_string(&user_path) {
                apply_config(&mut config, &content);
                eprintln!("[attend] config: loaded {}", user_path.display());
            }
        }

        // Project scope: {cwd}/.claude/attend.yaml
        let project_path = Path::new(working_dir).join(".claude").join("attend.yaml");
        if project_path.exists() {
            if let Ok(content) = fs::read_to_string(&project_path) {
                apply_config(&mut config, &content);
                eprintln!("[attend] config: loaded {}", project_path.display());
            }
        }

        config
    }

    /// Write default config to user scope path. Creates dirs if needed.
    pub fn init_user_config() -> PathBuf {
        let path = user_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&path, Self::default_yaml()).ok();
        path
    }

    /// Generate a default config file as a string.
    pub fn default_yaml() -> String {
        r#"# attend configuration
# User scope: ~/.config/attend/config.yaml
# Project scope: {project}/.claude/attend.yaml (layered on top)

governor:
  base_cooldown: 15
  max_per_window: 3
  rate_window: 120

sensors:
  context:
    interval: 60
    min_interval: 20
    threshold: 1.5
  git:
    interval: 30
    min_interval: 10
    threshold: 2.0
  peers:
    interval: 30
    min_interval: 10
    threshold: 2.0
  processes:
    interval: 30
    min_interval: 5
    threshold: 2.0

# Project-scope example (in .claude/attend.yaml):
#
# sensors:
#   +disk-pressure:
#     script: .claude/sensors/check-disk.sh
#     interval: 120
#     threshold: 3.0
#   -processes:
"#.to_string()
    }
}

fn user_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("attend").join("config.yaml")
}

/// Apply a YAML config string to an existing Config, overriding values.
/// Handles the +/- sensor syntax for project-scope overlays.
fn apply_config(config: &mut Config, content: &str) {
    let mut current_section = String::new();
    let mut current_sensor = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level section
        if indent == 0 && trimmed.ends_with(':') {
            current_section = trimmed.trim_end_matches(':').to_string();
            current_sensor.clear();
            continue;
        }

        // Second-level: sensor name or governor key
        if indent == 2 {
            if current_section == "governor" {
                if let Some((key, value)) = parse_kv(trimmed) {
                    match key {
                        "base_cooldown" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.governor.base_cooldown = Duration::from_secs(v);
                            }
                        }
                        "max_per_window" => {
                            if let Ok(v) = value.parse::<u32>() {
                                config.governor.max_per_window = v;
                            }
                        }
                        "rate_window" => {
                            if let Ok(v) = value.parse::<u64>() {
                                config.governor.rate_window = Duration::from_secs(v);
                            }
                        }
                        _ => {}
                    }
                }
            } else if current_section == "sensors" {
                let sensor_line = trimmed.trim_end_matches(':');
                if let Some(stripped) = sensor_line.strip_prefix('-') {
                    // Remove sensor: -processes
                    let name = stripped.trim();
                    if let Some(s) = config.sensors.get_mut(name) {
                        s.enabled = false;
                        eprintln!("[attend] config: disabled sensor '{}'", name);
                    }
                    current_sensor.clear();
                } else if let Some(stripped) = sensor_line.strip_prefix('+') {
                    // Add script sensor: +disk-pressure
                    let name = stripped.trim().to_string();
                    config.sensors.entry(name.clone()).or_insert(SensorConfig {
                        enabled: true,
                        interval: Duration::from_secs(60),
                        min_interval: Duration::from_secs(15),
                        threshold: 2.0,
                        decay_threshold: 4,
                        script: None,
                        requires: Vec::new(),
                    });
                    current_sensor = name;
                } else {
                    // Existing sensor override
                    current_sensor = sensor_line.trim().to_string();
                }
            }
            continue;
        }

        // Third-level: sensor properties
        if indent == 4 && !current_sensor.is_empty() {
            if let Some((key, value)) = parse_kv(trimmed) {
                if let Some(sensor) = config.sensors.get_mut(&current_sensor) {
                    match key {
                        "interval" => {
                            if let Ok(v) = value.parse::<u64>() {
                                sensor.interval = Duration::from_secs(v);
                            }
                        }
                        "min_interval" => {
                            if let Ok(v) = value.parse::<u64>() {
                                sensor.min_interval = Duration::from_secs(v);
                            }
                        }
                        "threshold" => {
                            if let Ok(v) = value.parse::<f64>() {
                                sensor.threshold = v;
                            }
                        }
                        "decay_threshold" => {
                            if let Ok(v) = value.parse::<u32>() {
                                sensor.decay_threshold = v;
                            }
                        }
                        "script" => {
                            sensor.script = Some(value.to_string());
                        }
                        "enabled" => {
                            sensor.enabled = value == "true";
                        }
                        "requires" => {
                            // Inline array: requires: [Bash(gh:*), Read]
                            if value.starts_with('[') && value.ends_with(']') {
                                let inner = &value[1..value.len() - 1];
                                sensor.requires = inner
                                    .split(',')
                                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Parse "key: value" from a trimmed line.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let value = line[colon + 1..].trim();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}
