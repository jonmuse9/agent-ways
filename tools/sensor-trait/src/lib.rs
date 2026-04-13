//! Sensor trait and runtime types for attend.
//!
//! This crate defines the contract between the attend orchestrator and any sensor
//! implementation. To add a new sensor: implement the `Sensor` trait, publish as
//! a workspace crate, and wire it into attend via a feature flag.

use std::collections::VecDeque;
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

// ── Action potential engagement (ADR-119) ──────────────────────

/// Per-sensor engagement state modeled after the neuronal action potential.
///
/// After a burst of disclosures, the sensor enters a refractory period:
///   - **Absolute refractory** (first N seconds): no disclosures at all —
///     the agent is processing what it just received.
///   - **Relative refractory** (decaying window): threshold is elevated;
///     only high-magnitude observations break through. Casual follow-ups
///     fall below the elevated bar and are silently absorbed.
///   - **Resting**: multiplier decays back toward 1.0 (normal threshold).
///
/// This provides natural disengagement from diminishing-value conversations
/// without any hard-coded rules — urgent stimuli still fire, routine chatter
/// goes quiet.
pub struct EngagementState {
    /// Disclosure timestamps within the current burst window.
    disclosures: VecDeque<Instant>,
    /// Time of the most recent disclosure (start of refractory clock).
    last_disclosure: Option<Instant>,
    /// Peak multiplier applied at the moment of the most recent burst.
    peak_multiplier: f64,

    /// Burst window: disclosures within this window count as a burst.
    burst_window: Duration,
    /// Disclosures needed within burst_window to trigger refractory.
    burst_threshold: usize,
    /// Multiplier applied per burst level above threshold.
    step_multiplier: f64,
    /// Absolute refractory duration — no disclosures during this window.
    absolute_refractory: Duration,
    /// Decay rate of the relative refractory multiplier, per minute.
    decay_per_minute: f64,
}

impl EngagementState {
    /// Create an EngagementState with explicit parameters.
    /// Use this to override defaults from a config layer.
    pub fn with_params(
        burst_window: Duration,
        burst_threshold: usize,
        step_multiplier: f64,
        absolute_refractory: Duration,
        decay_per_minute: f64,
    ) -> Self {
        Self {
            disclosures: VecDeque::new(),
            last_disclosure: None,
            peak_multiplier: 1.0,
            burst_window,
            burst_threshold,
            step_multiplier,
            absolute_refractory,
            decay_per_minute,
        }
    }

    pub fn new() -> Self {
        Self {
            disclosures: VecDeque::new(),
            last_disclosure: None,
            peak_multiplier: 1.0,
            // Timescales are sized for Claude's actual cadence, not neuron
            // kinetics. A Claude turn (think + tool use + respond) takes
            // 15s–2min, so "rapid engagement" from the agent's point of view
            // plays out over minutes of wall clock.
            //
            // Burst window: 10 minutes. Three sensor disclosures within this
            // window count as an active conversation.
            burst_window: Duration::from_secs(600),
            burst_threshold: 3,
            // Each disclosure past the burst threshold raises the multiplier
            // by 1.25. At burst=3, multiplier=2.25; base threshold 2.0 → gate
            // 4.5. Broadcast magnitudes:
            //   - Unboosted broadcast (4.0 × 1.0 = 4.0): below 4.5, suppressed
            //   - Boosted 2nd msg (4.0 × 1.75 = 7.0): above 4.5, fires
            //   - Boosted 3rd+ msg (4.0 × 2.5 = 10.0): fires
            //   - Directed message (7.0 × 1.0 = 7.0): fires (urgency escape)
            // This is the auto-grouping gradient: during conversation, only
            // active conversation partners and directed messages break through.
            step_multiplier: 1.25,
            // Absolute refractory: 60 seconds of complete suppression —
            // roughly one Claude turn. Long enough to impose a real cooldown
            // given how slow the agent feedback loop is, short enough that
            // genuine urgency isn't lost.
            absolute_refractory: Duration::from_secs(60),
            // Relative refractory decay: 0.1 per minute. From peak 2.25 back
            // to rest (1.0) takes ~12 minutes of no further disclosures —
            // enough that a natural pause in a conversation settles attention
            // back to the resting state.
            decay_per_minute: 0.1,
        }
    }

    /// Record a disclosure event. Updates burst tracking and refractory state.
    pub fn record_disclosure(&mut self) {
        let now = Instant::now();
        self.disclosures.push_back(now);
        self.prune(now);
        self.last_disclosure = Some(now);

        // If the count within the burst window exceeds the threshold, elevate.
        let burst_count = self.disclosures.len();
        if burst_count >= self.burst_threshold {
            let steps = (burst_count - self.burst_threshold + 1) as f64;
            let new_peak = 1.0 + steps * self.step_multiplier;
            // Take the max of current decayed value and the new peak — so
            // successive bursts stack, but the multiplier doesn't reset downward.
            let current = self.current_multiplier(now);
            self.peak_multiplier = new_peak.max(current);
        }
    }

    fn prune(&mut self, now: Instant) {
        while let Some(front) = self.disclosures.front() {
            if now.duration_since(*front) > self.burst_window {
                self.disclosures.pop_front();
            } else {
                break;
            }
        }
    }

    /// Whether the sensor is currently in absolute refractory — no disclosures
    /// allowed at any magnitude.
    pub fn in_absolute_refractory(&self) -> bool {
        match self.last_disclosure {
            Some(t) => {
                // Only absolute-refract if we're currently elevated (had a burst).
                self.peak_multiplier > 1.0 && t.elapsed() < self.absolute_refractory
            }
            None => false,
        }
    }

    /// Current relative refractory multiplier, decayed from the peak.
    /// Returns 1.0 when at rest.
    pub fn current_multiplier(&self, now: Instant) -> f64 {
        let Some(last) = self.last_disclosure else { return 1.0; };
        if self.peak_multiplier <= 1.0 { return 1.0; }
        let elapsed_min = now.duration_since(last).as_secs_f64() / 60.0;
        let decay = elapsed_min * self.decay_per_minute;
        (self.peak_multiplier - decay).max(1.0)
    }

    /// Effective threshold given a sensor's base threshold.
    /// Returns None during absolute refractory (blocks all disclosures).
    pub fn effective_threshold(&self, base: f64) -> Option<f64> {
        if self.in_absolute_refractory() {
            return None;
        }
        Some(base * self.current_multiplier(Instant::now()))
    }
}

impl Default for EngagementState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Sensor slot ─────────────────────────────────────────────────

/// A sensor with its runtime state (interval, accumulator, scheduling).
pub struct SensorSlot {
    pub sensor: Box<dyn Sensor>,
    pub interval: AdaptiveInterval,
    pub accumulator: DeltaAccumulator,
    pub engagement: EngagementState,
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
            engagement: EngagementState::new(),
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
            engagement: EngagementState::new(),
            next_fire: Instant::now(),
            interval,
            sensor,
        }
    }

    pub fn poll(&mut self, focus: &Focus) -> bool {
        let observations = self.sensor.poll(focus);
        if observations.is_empty() {
            self.interval.on_quiet();
            return false;
        }

        // Absolute refractory: drop all observations silently. The agent
        // needs a beat to process what just arrived.
        if self.engagement.in_absolute_refractory() {
            self.interval.on_change();
            return true;
        }

        // Relative refractory: per-event gating. Each observation must
        // individually clear the elevated threshold to be accumulated.
        // At rest (multiplier = 1.0), no filtering — all events accumulate
        // normally and the aggregate check in ready_to_disclose applies.
        // During refractory, sub-threshold events are silently dropped
        // (true disengagement, not delayed firing).
        let base = self.sensor.emission_threshold();
        let multiplier = self.engagement.current_multiplier(Instant::now());
        let per_event_gate = if multiplier > 1.0 { base * multiplier } else { 0.0 };

        let mut accepted = 0;
        for (magnitude, description) in observations {
            if magnitude >= per_event_gate {
                self.accumulator.accumulate(magnitude, description);
                accepted += 1;
            }
        }

        if accepted > 0 {
            self.interval.on_change();
            true
        } else {
            // All events filtered — count as quiet for adaptive interval
            self.interval.on_quiet();
            false
        }
    }

    pub fn ready_to_disclose(&self) -> bool {
        match self.engagement.effective_threshold(self.sensor.emission_threshold()) {
            None => false, // absolute refractory — hard block
            Some(threshold) => self.accumulator.magnitude >= threshold,
        }
    }

    /// Returns the current effective threshold (base * refractory multiplier),
    /// or None if in absolute refractory. For diagnostics/logging.
    pub fn effective_threshold(&self) -> Option<f64> {
        self.engagement.effective_threshold(self.sensor.emission_threshold())
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
