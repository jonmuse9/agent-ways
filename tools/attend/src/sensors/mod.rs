//! Sensor module — re-exports from sensor crates and local ScriptSensor.
//!
//! Sensor crates are wired in via feature flags. Each sensor is compiled only
//! when its feature is enabled (default: all). Config controls runtime
//! activation/deactivation independent of compilation.

mod script;

// Re-export from sensor-trait (always available)
pub use sensor_trait::{Focus, Sensor, SensorSlot};

// Re-export from sensor crates (feature-gated)
#[cfg(feature = "sensor-git")]
pub use sensor_git::GitSensor;

#[cfg(feature = "sensor-context")]
pub use sensor_context::ContextSensor;

#[cfg(feature = "sensor-peers")]
pub use sensor_peers::{PeerSensor, find_own_session_id};

#[cfg(feature = "sensor-processes")]
pub use sensor_processes::ProcessSensor;

// ScriptSensor stays in attend — it's part of the orchestrator, not a sensor impl
pub use script::ScriptSensor;

// ── Sensor registration ─────────────────────────────────────────

use crate::config::Config;
use crate::rooms::Rooms;
#[allow(unused_imports)]
use std::time::Duration;

/// Register all enabled sensors based on config and feature flags.
///
/// To add a new sensor crate:
/// 1. Create the crate implementing sensor_trait::Sensor
/// 2. Add it as an optional dep + feature in attend/Cargo.toml
/// 3. Add a #[cfg(feature)] block below with register_builtin!
///
/// Config controls runtime: `-sensorname` in attend.yaml disables it.
/// Feature flags control compilation: `--no-default-features` excludes it.
#[allow(unused_variables)]
pub fn register_sensors(
    cfg: &Config,
    focus: &Focus,
    catchup: bool,
    rooms: &Rooms,
) -> (Vec<SensorSlot>, Vec<String>) {
    let mut slots: Vec<SensorSlot> = Vec::new();
    let mut enabled_names: Vec<String> = Vec::new();

    // Macro to reduce boilerplate for built-in sensor registration.
    // Checks: feature compiled + config enabled → instantiate with config overrides.
    #[allow(unused_macros)]
    macro_rules! register_builtin {
        ($name:expr, $sensor:expr, $default_interval:expr, $default_min:expr, $default_decay:expr) => {
            if cfg.sensors.get($name).map(|s| s.enabled).unwrap_or(true) {
                let sc = cfg.sensors.get($name);
                slots.push(SensorSlot::new_with_config(
                    Box::new($sensor),
                    sc.map(|s| s.interval).unwrap_or(Duration::from_secs($default_interval)),
                    sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs($default_min)),
                    sc.map(|s| s.decay_threshold).unwrap_or($default_decay),
                ));
                enabled_names.push($name.to_string());
            }
        };
    }

    // ── Built-in crate sensors (feature-gated) ──────────────────

    #[cfg(feature = "sensor-context")]
    register_builtin!("context", ContextSensor::new(), 60, 20, 3);

    #[cfg(feature = "sensor-processes")]
    register_builtin!("processes", ProcessSensor::new(), 30, 5, 5);

    #[cfg(feature = "sensor-git")]
    register_builtin!("git", GitSensor::new(), 30, 10, 4);

    #[cfg(feature = "sensor-peers")]
    {
        if cfg.sensors.get("peers").map(|s| s.enabled).unwrap_or(true) {
            let mut peer_sensor = PeerSensor::new();
            // Pass room directories for signal scanning (ADR-118)
            let room_dirs: Vec<std::path::PathBuf> = rooms
                .joined_room_names()
                .into_iter()
                .map(|name| rooms.room_dir(&name))
                .collect();
            peer_sensor.set_extra_scan_dirs(room_dirs);
            if !catchup {
                peer_sensor.mark_existing_as_seen(focus);
            }
            let sc = cfg.sensors.get("peers");
            slots.push(SensorSlot::new_with_config(
                Box::new(peer_sensor),
                sc.map(|s| s.interval).unwrap_or(Duration::from_secs(30)),
                sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(10)),
                sc.map(|s| s.decay_threshold).unwrap_or(5),
            ));
            enabled_names.push("peers".to_string());
        }
    }

    // ── Script sensors (config-driven, always available) ────────

    for (name, sc) in &cfg.sensors {
        if let Some(ref script_path) = sc.script {
            if sc.enabled {
                let sensor = ScriptSensor::new(
                    name.clone(),
                    script_path.clone(),
                    focus.working_dir.clone(),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                    sc.threshold,
                );
                slots.push(SensorSlot::new_with_config(
                    Box::new(sensor),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                ));
                enabled_names.push(name.clone());
            }
        }
    }

    (slots, enabled_names)
}
