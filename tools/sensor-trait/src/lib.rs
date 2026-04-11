//! Sensor trait and runtime types for attend.
//!
//! This crate defines the contract between the attend orchestrator and any sensor
//! implementation. To add a new sensor: implement the `Sensor` trait, publish as
//! a workspace crate, and wire it into attend via a feature flag.

use std::time::{Duration, Instant};

// ── Sensor trait ────────────────────────────────────────────────

/// A sensor observes some aspect of the environment or Claude's state.
///
/// Sensors are polled by the tick loop on adaptive schedules.
/// They return observations that feed into delta accumulators.
///
/// The `Send` bound prepares for the daemon model where sensors may
/// run on separate threads.
pub trait Sensor: Send {
    /// Unique name for this sensor (used in output format and state keys).
    fn name(&self) -> &str;

    /// Poll the sensor's data source. Returns a list of observations.
    /// Each observation is a (delta_magnitude, description) pair.
    /// Empty vec = no change detected this tick.
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;

    /// Per-sensor emission threshold — accumulated magnitude needed before
    /// this sensor is a candidate for disclosure.
    fn emission_threshold(&self) -> f64;

    /// Base polling interval (maximum, used when quiet).
    fn base_interval(&self) -> Duration;

    /// Minimum polling interval (fastest, used during active changes).
    fn min_interval(&self) -> Duration;

    /// Number of quiet ticks before interval decays back toward base.
    fn decay_threshold(&self) -> u32;

    /// Export state for checkpointing. Default: no state.
    fn export_state(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Import state from checkpoint. Default: no-op.
    fn import_state(&mut self, _state: &[(String, String)]) {}
}

// ── Focus ───────────────────────────────────────────────────────

/// What Claude is currently focused on. Shapes sensor relevance and thresholds.
#[derive(Clone, Debug)]
pub struct Focus {
    /// Short description of current work ("debugging auth module", "writing tests").
    pub description: String,
    /// Working directory — sensors can use this to scope observations.
    pub working_dir: String,
    /// Keywords that increase sensor relevance when matched.
    pub keywords: Vec<String>,
}

impl Focus {
    pub fn default_focus() -> Self {
        Self {
            description: "general session".to_string(),
            working_dir: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            keywords: Vec::new(),
        }
    }
}

// ── Delta accumulator ───────────────────────────────────────────

/// Accumulates real state changes across sensor ticks.
/// Magnitude grows monotonically until disclosed or decayed.
pub struct DeltaAccumulator {
    pub magnitude: f64,
    pub event_count: u32,
    pub window_start: Instant,
    pub events: Vec<String>,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self {
            magnitude: 0.0,
            event_count: 0,
            window_start: Instant::now(),
            events: Vec::new(),
        }
    }

    pub fn accumulate(&mut self, delta: f64, description: String) {
        self.magnitude += delta;
        self.event_count += 1;
        self.events.push(description);
    }

    pub fn reset(&mut self) {
        self.magnitude = 0.0;
        self.event_count = 0;
        self.window_start = Instant::now();
        self.events.clear();
    }

    /// Single-line summary (legacy, for non-paged contexts).
    pub fn summary(&self) -> String {
        if self.events.is_empty() {
            return String::new();
        }
        let unique = self.unique_events();
        if unique.len() == 1 {
            unique[0].clone()
        } else {
            format!(
                "{} observations: {}",
                unique.len(),
                unique.join("; ")
            )
        }
    }

    /// Drain events as individual lines for paged emission.
    /// Each event becomes its own notification line via Monitor.
    pub fn drain_events(&self) -> Vec<String> {
        self.unique_events()
    }

    fn unique_events(&self) -> Vec<String> {
        let mut unique: Vec<String> = Vec::new();
        for event in &self.events {
            if !unique.contains(event) {
                unique.push(event.clone());
            }
        }
        unique
    }
}

impl Default for DeltaAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Adaptive interval ───────────────────────────────────────────

/// Per-sensor adaptive polling interval.
/// Ramps up fast on change, decays slowly on quiet (hysteresis).
pub struct AdaptiveInterval {
    pub base: Duration,
    pub current: Duration,
    pub min: Duration,
    pub decay_threshold: u32,
    pub ramp_cooldown: u32,
}

impl AdaptiveInterval {
    pub fn new(base: Duration, min: Duration, decay_threshold: u32) -> Self {
        Self {
            base,
            current: base,
            min,
            decay_threshold,
            ramp_cooldown: 0,
        }
    }

    pub fn on_change(&mut self) {
        self.current = self.current.div_f64(2.0).max(self.min);
        self.ramp_cooldown = 0;
    }

    pub fn on_quiet(&mut self) {
        self.ramp_cooldown += 1;
        if self.ramp_cooldown >= self.decay_threshold {
            self.current = self.current.mul_f64(1.5).min(self.base);
        }
    }
}

// ── Sensor slot ─────────────────────────────────────────────────

/// A sensor with its runtime state (interval, accumulator, scheduling).
pub struct SensorSlot {
    pub sensor: Box<dyn Sensor>,
    pub interval: AdaptiveInterval,
    pub accumulator: DeltaAccumulator,
    pub next_fire: Instant,
}

impl SensorSlot {
    pub fn new(sensor: Box<dyn Sensor>) -> Self {
        let interval = AdaptiveInterval::new(
            sensor.base_interval(),
            sensor.min_interval(),
            sensor.decay_threshold(),
        );
        Self {
            accumulator: DeltaAccumulator::new(),
            next_fire: Instant::now(),
            interval,
            sensor,
        }
    }

    /// Create a SensorSlot with config-overridden intervals.
    pub fn new_with_config(
        sensor: Box<dyn Sensor>,
        base: Duration,
        min: Duration,
        decay_threshold: u32,
    ) -> Self {
        let interval = AdaptiveInterval::new(base, min, decay_threshold);
        Self {
            accumulator: DeltaAccumulator::new(),
            next_fire: Instant::now(),
            interval,
            sensor,
        }
    }

    pub fn poll(&mut self, focus: &Focus) -> bool {
        let observations = self.sensor.poll(focus);
        if observations.is_empty() {
            self.interval.on_quiet();
            false
        } else {
            for (magnitude, description) in observations {
                self.accumulator.accumulate(magnitude, description);
            }
            self.interval.on_change();
            true
        }
    }

    pub fn ready_to_disclose(&self) -> bool {
        self.accumulator.magnitude >= self.sensor.emission_threshold()
    }

    pub fn schedule_next(&mut self) {
        self.next_fire = Instant::now() + self.interval.current;
    }

    pub fn name(&self) -> &str {
        self.sensor.name()
    }

    pub fn export_state(&self) -> Vec<(String, String)> {
        self.sensor.export_state()
    }

    pub fn import_state(&mut self, state: &[(String, String)]) {
        self.sensor.import_state(state);
    }
}
