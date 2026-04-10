use std::collections::BinaryHeap;
use std::io::{self, Write};
use std::time::{Duration, Instant};

// --- Adaptive interval per sensor ---

struct AdaptiveInterval {
    base: Duration,
    current: Duration,
    min: Duration,
    decay_threshold: u32,
    ramp_cooldown: u32,
}

impl AdaptiveInterval {
    fn new(base: Duration, min: Duration, decay_threshold: u32) -> Self {
        Self { base, current: base, min, decay_threshold, ramp_cooldown: 0 }
    }

    fn on_change(&mut self) {
        self.current = self.current.div_f64(2.0).max(self.min);
        self.ramp_cooldown = 0;
    }

    fn on_quiet(&mut self) {
        self.ramp_cooldown += 1;
        if self.ramp_cooldown >= self.decay_threshold {
            self.current = self.current.mul_f64(1.5).min(self.base);
        }
    }
}

// --- Delta accumulator ---

struct DeltaAccumulator {
    magnitude: f64,
    event_count: u32,
    window_start: Instant,
    events: Vec<String>,  // descriptions of what changed
}

impl DeltaAccumulator {
    fn new() -> Self {
        Self { magnitude: 0.0, event_count: 0, window_start: Instant::now(), events: Vec::new() }
    }

    fn accumulate(&mut self, delta: f64, description: String) {
        self.magnitude += delta;
        self.event_count += 1;
        self.events.push(description);
    }

    fn event_rate(&self) -> f64 {
        let elapsed = self.window_start.elapsed().as_secs_f64();
        if elapsed < 0.001 { return 0.0; }
        self.event_count as f64 / elapsed
    }

    fn reset(&mut self) {
        self.magnitude = 0.0;
        self.event_count = 0;
        self.window_start = Instant::now();
        self.events.clear();
    }

    fn summary(&self) -> String {
        if self.events.len() == 1 {
            self.events[0].clone()
        } else {
            format!("{} changes: {}", self.events.len(), self.events.join(", "))
        }
    }
}

// --- Disclosure governor ---
// Global rate limiter across all sensors.
// Inverse relationship: higher aggregate event rate → longer cooldown.

struct DisclosureGovernor {
    base_cooldown: Duration,
    last_disclosure: Option<Instant>,
    max_disclosures_per_window: u32,
    window_disclosures: u32,
    window_start: Instant,
    rate_window: Duration,
    // Track aggregate event rate across all sensors
    total_events: u32,
    total_events_start: Instant,
}

impl DisclosureGovernor {
    fn new(base_cooldown: Duration, max_per_window: u32, rate_window: Duration) -> Self {
        let now = Instant::now();
        Self {
            base_cooldown,
            last_disclosure: None,
            max_disclosures_per_window: max_per_window,
            window_disclosures: 0,
            window_start: now,
            rate_window,
            total_events: 0,
            total_events_start: now,
        }
    }

    fn record_event(&mut self) {
        self.total_events += 1;
    }

    fn aggregate_rate(&self) -> f64 {
        let elapsed = self.total_events_start.elapsed().as_secs_f64();
        if elapsed < 0.001 { return 0.0; }
        self.total_events as f64 / elapsed
    }

    fn cooldown(&self) -> Duration {
        // Higher aggregate rate → longer cooldown
        let rate = self.aggregate_rate();
        let multiplier = 1.0 + rate.sqrt() * 3.0;
        self.base_cooldown.mul_f64(multiplier)
    }

    fn can_disclose(&mut self) -> bool {
        // Reset rate window if expired
        if self.window_start.elapsed() >= self.rate_window {
            self.window_disclosures = 0;
            self.window_start = Instant::now();
        }

        if self.window_disclosures >= self.max_disclosures_per_window {
            return false;
        }

        if let Some(last) = self.last_disclosure {
            if last.elapsed() < self.cooldown() {
                return false;
            }
        }

        true
    }

    fn record_disclosure(&mut self) {
        self.last_disclosure = Some(Instant::now());
        self.window_disclosures += 1;
    }
}

// --- Sensor ---

struct Sensor {
    name: String,
    interval: AdaptiveInterval,
    accumulator: DeltaAccumulator,
    next_fire: Instant,
    state: u64,
    emission_threshold: f64,  // per-sensor threshold for disclosure eligibility
}

impl Sensor {
    fn new(name: &str, base_interval: Duration, min_interval: Duration, decay_threshold: u32, emission_threshold: f64) -> Self {
        Self {
            name: name.to_string(),
            interval: AdaptiveInterval::new(base_interval, min_interval, decay_threshold),
            accumulator: DeltaAccumulator::new(),
            next_fire: Instant::now(),
            state: 0,
            emission_threshold,
        }
    }

    fn poll(&mut self, simulated_state: u64, description: &str) -> bool {
        if simulated_state != self.state {
            let delta = (simulated_state as f64 - self.state as f64).abs();
            self.state = simulated_state;
            self.accumulator.accumulate(delta, description.to_string());
            self.interval.on_change();
            true
        } else {
            self.interval.on_quiet();
            false
        }
    }

    fn ready_to_disclose(&self) -> bool {
        self.accumulator.magnitude >= self.emission_threshold
    }

    fn schedule_next(&mut self) {
        self.next_fire = Instant::now() + self.interval.current;
    }
}

// --- Priority queue entry ---

struct ScheduledSensor {
    fire_at: Instant,
    index: usize,
}

impl Eq for ScheduledSensor {}
impl PartialEq for ScheduledSensor {
    fn eq(&self, other: &Self) -> bool { self.fire_at == other.fire_at }
}
impl Ord for ScheduledSensor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.fire_at.cmp(&self.fire_at)
    }
}
impl PartialOrd for ScheduledSensor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// --- Emit to stdout (Monitor delivery) ---
// Batched: multiple sensors disclosed together within 200ms → one notification.

fn emit_batch(disclosures: &[(String, String, String)]) {
    // Each disclosure is (sensor_name, priority, summary)
    // Emit all within 200ms so Monitor batches them into one notification
    for (sensor, priority, summary) in disclosures {
        println!("[attend sensor={sensor} priority={priority}] {summary}");
    }
    io::stdout().flush().ok();
}

fn log(message: &str) {
    eprintln!("[attend] {message}");
}

// --- Simulated world ---

fn simulated_world(elapsed: Duration) -> Vec<(u64, &'static str)> {
    let secs = elapsed.as_secs();

    let file_state = if secs < 30 {
        (0, "no change")
    } else if secs < 45 {
        ((secs - 30) / 2, "files modified in src/auth/")
    } else {
        (7, "no change")
    };

    let context_state = (secs / 20, "context growing");

    let git_state = if secs < 60 {
        (0, "no change")
    } else {
        (1, "new commit on main by peer")
    };

    vec![file_state, context_state, git_state]
}

// --- Main loop ---

fn main() {
    log("starting attend (experimental v2)");

    let mut sensors = vec![
        // file_churn: needs magnitude 3.0+ before it's worth disclosing
        Sensor::new("file_churn", Duration::from_secs(10), Duration::from_secs(2), 5, 3.0),
        // context_pressure: needs magnitude 2.0+ (two threshold crossings)
        Sensor::new("context_pressure", Duration::from_secs(15), Duration::from_secs(5), 3, 2.0),
        // git_state: any change is immediately relevant
        Sensor::new("git_state", Duration::from_secs(30), Duration::from_secs(5), 5, 1.0),
    ];

    let mut governor = DisclosureGovernor::new(
        Duration::from_secs(15),  // base cooldown — at least 15s between disclosures
        3,                         // max 3 disclosures per window
        Duration::from_secs(120),  // 2-minute rate window
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, sensor) in sensors.iter().enumerate() {
        queue.push(ScheduledSensor { fire_at: sensor.next_fire, index: i });
    }

    let start = Instant::now();
    log("tick loop running");
    log("sensors: file_churn(10s, threshold=3.0), context_pressure(15s, threshold=2.0), git_state(30s, threshold=1.0)");
    log("governor: cooldown=15s, max=3/120s");

    loop {
        let next = match queue.peek() {
            Some(s) => s.fire_at,
            None => break,
        };

        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }

        // Run due sensors, collect which ones are ready to disclose
        let mut ready_indices = Vec::new();

        while let Some(scheduled) = queue.peek() {
            if scheduled.fire_at > Instant::now() {
                break;
            }
            let scheduled = queue.pop().unwrap();
            let i = scheduled.index;
            let world = simulated_world(start.elapsed());

            let changed = sensors[i].poll(world[i].0, world[i].1);

            if changed {
                governor.record_event();
            }

            log(&format!(
                "{}: poll interval={:.1}s changed={} accum={:.1} events={} threshold={:.1}",
                sensors[i].name,
                sensors[i].interval.current.as_secs_f64(),
                changed,
                sensors[i].accumulator.magnitude,
                sensors[i].accumulator.event_count,
                sensors[i].emission_threshold,
            ));

            if sensors[i].ready_to_disclose() {
                ready_indices.push(i);
            }

            sensors[i].schedule_next();
            queue.push(ScheduledSensor { fire_at: sensors[i].next_fire, index: i });
        }

        // Batch disclosure: if any sensors are ready AND the governor allows it
        if !ready_indices.is_empty() && governor.can_disclose() {
            let mut batch = Vec::new();

            for &i in &ready_indices {
                let sensor = &sensors[i];
                let priority = if sensor.accumulator.magnitude >= 5.0 {
                    "high"
                } else if sensor.accumulator.magnitude >= 3.0 {
                    "medium"
                } else {
                    "low"
                };

                batch.push((
                    sensor.name.clone(),
                    priority.to_string(),
                    sensor.accumulator.summary(),
                ));
            }

            log(&format!("disclosing batch of {} sensors (cooldown was {:.1}s)", batch.len(), governor.cooldown().as_secs_f64()));
            emit_batch(&batch);
            governor.record_disclosure();

            // Reset accumulators for disclosed sensors
            for &i in &ready_indices {
                sensors[i].accumulator.reset();
            }
        } else if !ready_indices.is_empty() {
            log(&format!(
                "{} sensors ready but governor holding (cooldown {:.1}s, {}/{} disclosures in window)",
                ready_indices.len(),
                governor.cooldown().as_secs_f64(),
                governor.window_disclosures,
                governor.max_disclosures_per_window,
            ));
        }

        if start.elapsed() > Duration::from_secs(120) {
            log("experiment complete");
            break;
        }
    }
}
