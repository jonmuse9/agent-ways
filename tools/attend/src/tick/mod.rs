use std::time::Duration;

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
        Self { base, current: base, min, decay_threshold, ramp_cooldown: 0 }
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
