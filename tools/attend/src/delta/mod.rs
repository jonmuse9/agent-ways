use std::time::Instant;

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

    pub fn summary(&self) -> String {
        if self.events.is_empty() {
            return String::new();
        }

        // Deduplicate repeated descriptions
        let mut unique: Vec<&String> = Vec::new();
        for event in &self.events {
            if !unique.contains(&event) {
                unique.push(event);
            }
        }

        if unique.len() == 1 {
            unique[0].clone()
        } else {
            format!("{} observations: {}", unique.len(), unique.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; "))
        }
    }
}
