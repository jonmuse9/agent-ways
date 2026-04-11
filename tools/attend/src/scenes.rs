//! Scene management for attend (ADR-118).
//!
//! A scene is a named preset that configures room membership.
//! Scenes live in `~/.config/attend/scenes.yaml`.
//!
//! Built-in defaults:
//!   private — leave all named rooms (project room only)
//!   open    — join the well-known "open" room

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::rooms::Rooms;

/// A scene definition.
#[derive(Debug, Clone)]
pub struct Scene {
    pub rooms: Vec<String>,
}

/// Load scenes from config file. Returns built-in defaults merged with user config.
pub fn load_scenes() -> HashMap<String, Scene> {
    let mut scenes = HashMap::new();

    // Built-in defaults
    scenes.insert("private".to_string(), Scene { rooms: Vec::new() });
    scenes.insert("open".to_string(), Scene { rooms: vec!["open".to_string()] });

    // User config overlay
    let path = scenes_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        for (name, scene) in parse_scenes_yaml(&content) {
            scenes.insert(name, scene);
        }
    }

    scenes
}

/// Activate a scene — reconfigure room membership to match the preset.
pub fn activate(scene_name: &str, rooms: &Rooms) -> Result<String, String> {
    let scenes = load_scenes();
    let scene = scenes
        .get(scene_name)
        .ok_or_else(|| format!("unknown scene '{scene_name}' — try: {}",
            scenes.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")))?;

    // Leave all current named rooms
    for (name, _) in rooms.my_rooms() {
        rooms.leave(&name).ok();
    }

    // Join the scene's rooms
    for room_name in &scene.rooms {
        rooms.join(room_name, false)?;
    }

    if scene.rooms.is_empty() {
        Ok("project room only".to_string())
    } else {
        Ok(format!("joined: {}", scene.rooms.join(", ")))
    }
}

fn scenes_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("attend").join("scenes.yaml")
}

/// Parse scenes.yaml. Format:
/// ```yaml
/// private:
///   rooms: []
/// workroom:
///   rooms: [deploy, infra]
/// ```
fn parse_scenes_yaml(content: &str) -> HashMap<String, Scene> {
    let mut scenes = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_rooms: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level: scene name
        if indent == 0 && trimmed.ends_with(':') {
            if let Some(ref name) = current_name {
                scenes.insert(name.clone(), Scene { rooms: current_rooms.clone() });
            }
            current_name = Some(trimmed.trim_end_matches(':').to_string());
            current_rooms = Vec::new();
            continue;
        }

        // Second-level: rooms key with inline array or list items
        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                if key == "rooms" {
                    // Inline array: rooms: [deploy, infra]
                    if value.starts_with('[') && value.ends_with(']') {
                        let inner = &value[1..value.len() - 1];
                        current_rooms = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else if value == "[]" {
                        current_rooms = Vec::new();
                    }
                    // else: list items follow at indent 4
                }
            }
        }

        // Third-level: list items
        if indent == 4 {
            if let Some(room) = trimmed.strip_prefix("- ") {
                current_rooms.push(room.trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }

    // Save last scene
    if let Some(ref name) = current_name {
        scenes.insert(name.clone(), Scene { rooms: current_rooms });
    }

    scenes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scenes() {
        let yaml = r#"
private:
  rooms: []
workroom:
  rooms: [deploy, infra]
custom:
  rooms:
    - alpha
    - beta
"#;
        let scenes = parse_scenes_yaml(yaml);
        assert_eq!(scenes.len(), 3);
        assert!(scenes["private"].rooms.is_empty());
        assert_eq!(scenes["workroom"].rooms, vec!["deploy", "infra"]);
        assert_eq!(scenes["custom"].rooms, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_builtins() {
        let scenes = load_scenes();
        assert!(scenes.contains_key("private"));
        assert!(scenes.contains_key("open"));
        assert!(scenes["private"].rooms.is_empty());
        assert_eq!(scenes["open"].rooms, vec!["open"]);
    }
}
