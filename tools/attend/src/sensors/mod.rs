mod process;

pub use process::ProcessSensor;

use crate::delta::DeltaAccumulator;
use crate::tick::AdaptiveInterval;
use std::time::{Duration, Instant};

/// What Claude is currently focused on. Shapes sensor relevance and thresholds.
#[derive(Clone, Debug)]
pub struct Focus {
    /// Short description of current work ("debugging auth module", "writing tests")
    pub description: String,
    /// Working directory — sensors can use this to scope observations
    pub working_dir: String,
    /// Keywords that increase sensor relevance when matched
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

/// A sensor observes some aspect of the environment or Claude's state.
/// Sensors are polled by the tick loop on adaptive schedules.
/// They return observations that feed into delta accumulators.
pub trait Sensor {
    /// Unique name for this sensor (used in output format and state keys)
    fn name(&self) -> &str;

    /// Poll the sensor's data source. Returns a list of observations.
    /// Each observation is a (delta_magnitude, description) pair.
    /// Empty vec = no change detected this tick.
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;

    /// Per-sensor emission threshold — accumulated magnitude needed before
    /// this sensor is a candidate for disclosure
    fn emission_threshold(&self) -> f64;

    /// Adaptive interval configuration
    fn base_interval(&self) -> Duration;
    fn min_interval(&self) -> Duration;
    fn decay_threshold(&self) -> u32;
}

/// A sensor with its runtime state (interval, accumulator, scheduling)
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
}
