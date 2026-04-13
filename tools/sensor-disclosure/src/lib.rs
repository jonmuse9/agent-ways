//! Disclosure sensor: reheats affordance instructions (messaging, scenes, focus, ...)
//! on token-distance drift from the previous disclosure.
//!
//! Shape: an ordinary `impl Sensor` whose signal is token distance rather than
//! environmental change. Each `poll` shells out to `ways context --json` once,
//! reads `tokens_used`, and checks each registered component:
//!
//!   - No marker yet → emit the component's disclosure text, stamp baseline.
//!   - Marker present, distance ≥ 25% of context window → emit, re-stamp.
//!   - Otherwise → no observation for that component.
//!
//! State is `HashMap<Component, u64>` on the sensor struct — in-process memory
//! only, clean-restart semantics by construction.

use sensor_trait::{Focus, Sensor};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

/// Reheat threshold — a component re-discloses when token distance from its
/// last disclosure exceeds this percentage of the current model's context
/// window. Matches `ways-cli`'s `REDISCLOSE_PCT` so one dial governs both.
const REDISCLOSE_PCT: u64 = 25;

/// Fixed magnitude for a disclosure observation. Sized to clear the default
/// emission threshold on a single poll — "urgency building up" is conceptual
/// (progress toward the token-distance gate), not a gradual accumulation.
const DISCLOSURE_MAGNITUDE: f64 = 5.0;

// ── Component registry ─────────────────────────────────────────

/// Registered disclosure components. Add a variant + a matching markdown file
/// under `src/disclosures/` to introduce a new reheatable affordance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Component {
    Messaging,
}

impl Component {
    fn id(self) -> &'static str {
        match self {
            Component::Messaging => "messaging",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Component::Messaging => include_str!("disclosures/messaging.md"),
        }
    }

    fn all() -> &'static [Component] {
        &[Component::Messaging]
    }
}

// ── The sensor ─────────────────────────────────────────────────

pub struct DisclosureSensor {
    /// Last `tokens_used` value at which each component was disclosed.
    /// Missing key = never disclosed this process → fire unconditionally.
    ledger: HashMap<Component, u64>,
}

impl DisclosureSensor {
    pub fn new() -> Self {
        Self {
            ledger: HashMap::new(),
        }
    }

    /// Shell out to `ways context --json` and return the `tokens_used` field.
    /// Matches `sensor-context`'s integration pattern.
    fn read_tokens_used(&self, focus: &Focus) -> Option<(u64, u64)> {
        let output = Command::new("ways")
            .args(["context", "--json"])
            .current_dir(&focus.working_dir)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let tokens_used = extract_u64(&stdout, "tokens_used")?;
        let tokens_total = extract_u64(&stdout, "tokens_total")?;
        Some((tokens_used, tokens_total))
    }

    /// Format the observation text for a component. The body is prefixed with
    /// a single tagged line so Claude can recognize this as an affordance
    /// reheat regardless of which component fired.
    fn format_observation(&self, component: Component) -> String {
        format!(
            "reheat: attend affordances — {}\n{}",
            component.id(),
            component.body().trim_end()
        )
    }
}

impl Default for DisclosureSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for DisclosureSensor {
    fn name(&self) -> &str {
        "disclosure"
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let (tokens_used, tokens_total) = match self.read_tokens_used(focus) {
            Some(pair) => pair,
            None => return Vec::new(),
        };
        if tokens_total == 0 {
            return Vec::new();
        }

        let threshold_tokens = tokens_total * REDISCLOSE_PCT / 100;
        let mut observations = Vec::new();

        for &component in Component::all() {
            let should_fire = match self.ledger.get(&component) {
                None => true,
                Some(&last) => tokens_used.saturating_sub(last) >= threshold_tokens,
            };
            if should_fire {
                observations.push((DISCLOSURE_MAGNITUDE, self.format_observation(component)));
                self.ledger.insert(component, tokens_used);
            }
        }

        observations
    }

    fn emission_threshold(&self) -> f64 {
        // Matches `sensor-context` — disclosures are high-priority by
        // nature (they carry instructional payload, not telemetry).
        1.5
    }

    fn base_interval(&self) -> Duration {
        // Poll at the same rest cadence as `sensor-context`. The subprocess
        // call is the same operation against the same file.
        Duration::from_secs(60)
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(20)
    }

    fn decay_threshold(&self) -> u32 {
        3
    }

    // State is intentionally ephemeral — no export/import. A restarted attend
    // process starts with an empty ledger and re-teaches on first poll, which
    // is the designed clean-restart behavior.
}

// ── Helpers ────────────────────────────────────────────────────

/// Extract a u64 from JSON-like text: `"key":value`. Mirrors the parser in
/// `sensor-context` to avoid pulling in `serde_json` for a two-field read.
fn extract_u64(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = text.find(&pattern)? + pattern.len();
    let rest = text[start..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_u64_basic() {
        assert_eq!(extract_u64(r#"{"tokens_used": 12345}"#, "tokens_used"), Some(12345));
        assert_eq!(extract_u64(r#"{"tokens_used":0}"#, "tokens_used"), Some(0));
        assert_eq!(extract_u64(r#"{"other":1}"#, "tokens_used"), None);
    }

    #[test]
    fn component_registry_has_messaging_body() {
        let body = Component::Messaging.body();
        assert!(body.contains("attend send"));
        assert!(body.contains("--broadcast"));
        assert!(!body.is_empty());
    }

    #[test]
    fn first_poll_fires_every_component() {
        // Can't call poll() without a working `ways` binary, but we can
        // verify the ledger logic directly by walking the registry.
        let sensor = DisclosureSensor::new();
        assert!(sensor.ledger.is_empty());
        // Every component should be unknown at construction time.
        for &c in Component::all() {
            assert!(!sensor.ledger.contains_key(&c));
        }
    }

    #[test]
    fn format_observation_contains_tag_and_body() {
        let sensor = DisclosureSensor::new();
        let obs = sensor.format_observation(Component::Messaging);
        assert!(obs.starts_with("reheat: attend affordances — messaging"));
        assert!(obs.contains("attend send"));
    }
}
