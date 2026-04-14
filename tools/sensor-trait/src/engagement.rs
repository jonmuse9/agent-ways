//! Unit-agnostic engagement state (ADR-123 Phase A).
//!
//! `EngagementStateV2` is the progression-axis replacement for the
//! `Instant`/`Duration`-based `EngagementState` in `lib.rs`. It owns a
//! [`Curve`] and a tick-based fire history, and exposes the inward and
//! outward gates from ADR-123 Decision 3:
//!
//! - [`Self::should_fire`] — "should this new event be allowed to fire?"
//! - [`Self::current_salience`] — "is the last-fired guidance still loud?"
//!
//! The old `EngagementState` stays in place during Phase A so attend's
//! existing tests continue to pass. Phase B migrates attend's call sites
//! onto this type and retires the old one.

use std::collections::VecDeque;

use crate::{Curve, Tick};

/// Per-subject firing dynamics state, keyed on a caller-supplied monotonic
/// progression axis. The engine does not know what a tick is — see
/// [`crate::Tick`].
#[derive(Debug, Clone)]
pub struct EngagementStateV2 {
    curve: Curve,
    history: VecDeque<(Tick, f64)>,
    last_fire: Option<Tick>,
}

impl EngagementStateV2 {
    /// Create a new engagement state with the given curve and no fire history.
    pub fn new(curve: Curve) -> Self {
        Self {
            curve,
            history: VecDeque::new(),
            last_fire: None,
        }
    }

    /// Inward gate. Returns true iff the curve's refractory multiplier
    /// admits a new fire of the given magnitude at `current_tick`.
    ///
    /// Convention: magnitude is pre-normalized so that 1.0 is "normal
    /// firing strength at rest." Elevated refractory multipliers require
    /// proportionally larger magnitudes to break through. An
    /// `f64::INFINITY` multiplier (absolute refractory) blocks all fires.
    pub fn should_fire(&self, current_tick: Tick, magnitude: f64) -> bool {
        let multiplier = self.current_multiplier(current_tick);
        if !multiplier.is_finite() {
            return false;
        }
        magnitude >= multiplier
    }

    /// Record a fire at `tick` with the given magnitude. The history is
    /// kept in order; callers are expected to pass monotonic ticks.
    pub fn record_fire(&mut self, tick: Tick, magnitude: f64) {
        self.history.push_back((tick, magnitude));
        self.last_fire = Some(tick);
        self.prune_decayed(tick);
    }

    /// Outward gate. Salience of the last-fired guidance at `current_tick`,
    /// or 0.0 if nothing has fired yet.
    pub fn current_salience(&self, current_tick: Tick) -> f64 {
        let Some(last) = self.last_fire else { return 0.0 };
        let delta = current_tick.saturating_sub(last);
        self.curve.salience_at(delta)
    }

    /// Current refractory multiplier. 1.0 at rest, >1.0 during relative
    /// refractory, `f64::INFINITY` during absolute refractory.
    pub fn current_multiplier(&self, current_tick: Tick) -> f64 {
        let delta = match self.last_fire {
            Some(last) => current_tick.saturating_sub(last),
            None => 0,
        };
        let history_slice: Vec<(Tick, f64)> = self.history.iter().copied().collect();
        self.curve.multiplier_at(delta, &history_slice, current_tick)
    }

    /// Access the underlying curve (for diagnostics and tune-time access).
    pub fn curve(&self) -> &Curve {
        &self.curve
    }

    /// Prune history entries whose multiplier contribution has decayed
    /// past the burst-consideration epsilon. Keeps the history bounded
    /// over long sessions without losing burst-detection fidelity.
    fn prune_decayed(&mut self, current: Tick) {
        let half_life = match &self.curve {
            Curve::ActionPotential {
                multiplier_half_life,
                ..
            } => *multiplier_half_life,
            // Non-refractory curves don't need burst history, so prune
            // everything older than the most recent fire.
            _ => 0,
        };
        if half_life == 0 {
            // Keep only the most recent fire — it's what last_fire
            // already tracks anyway.
            while self.history.len() > 1 {
                self.history.pop_front();
            }
            return;
        }
        const EPSILON: f64 = 0.01;
        while let Some((tick, _)) = self.history.front().copied() {
            let delta = current.saturating_sub(tick);
            let contribution = 0.5_f64.powf(delta as f64 / half_life as f64);
            if contribution <= EPSILON {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap_curve() -> Curve {
        Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        }
    }

    #[test]
    fn new_state_has_no_history_and_allows_firing() {
        let s = EngagementStateV2::new(ap_curve());
        assert_eq!(s.current_salience(0), 0.0);
        assert_eq!(s.current_multiplier(0), 1.0);
        assert!(s.should_fire(0, 1.0));
    }

    #[test]
    fn full_burst_decay_cycle() {
        let mut s = EngagementStateV2::new(ap_curve());

        // Fire 1: at rest, should be allowed.
        assert!(s.should_fire(0, 1.0));
        s.record_fire(0, 1.0);

        // Fire 2: still at rest (below burst threshold).
        assert!(s.should_fire(100, 1.0));
        s.record_fire(100, 1.0);

        // Fire 3: this brings us to threshold — still allowed because
        // multiplier is computed before the fire is recorded.
        assert!(s.should_fire(200, 1.0));
        s.record_fire(200, 1.0);

        // Immediately after: absolute refractory blocks everything.
        assert!(!s.should_fire(220, 1.0));
        assert!(!s.should_fire(220, 1_000.0));
        assert!(s.current_multiplier(220).is_infinite());

        // Past absolute refractory but still in relative refractory:
        // multiplier is elevated above 1.0.
        let m = s.current_multiplier(300);
        assert!(m.is_finite());
        assert!(m > 1.0, "expected elevated multiplier, got {}", m);
        // Weak stimulus blocked, strong stimulus passes.
        assert!(!s.should_fire(300, 1.0));
        assert!(s.should_fire(300, 10.0));

        // Far past burst — back to rest.
        let m_rest = s.current_multiplier(200_000);
        assert!(
            (m_rest - 1.0).abs() < 1e-9,
            "expected rest multiplier, got {}",
            m_rest
        );
        assert!(s.should_fire(200_000, 1.0));
    }

    #[test]
    fn salience_decays_over_ticks_for_exponential() {
        let curve = Curve::Exponential { half_life: 100 };
        let mut s = EngagementStateV2::new(curve);
        s.record_fire(0, 1.0);
        assert!((s.current_salience(0) - 1.0).abs() < 1e-9);
        assert!((s.current_salience(100) - 0.5).abs() < 1e-9);
        assert!((s.current_salience(200) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn flat_curve_salience_matches_step() {
        let curve = Curve::Flat { suppression: 500 };
        let mut s = EngagementStateV2::new(curve);
        s.record_fire(0, 1.0);
        assert_eq!(s.current_salience(100), 0.0);
        assert_eq!(s.current_salience(500), 1.0);
        assert_eq!(s.current_salience(10_000), 1.0);
    }

    #[test]
    fn prune_bounds_history_on_non_refractory_curve() {
        let mut s = EngagementStateV2::new(Curve::Exponential { half_life: 100 });
        for t in 0..10 {
            s.record_fire(t * 10, 1.0);
        }
        // Non-refractory curves don't need burst history — only one
        // entry should remain (the most recent).
        assert!(s.history.len() <= 1);
        assert_eq!(s.last_fire, Some(90));
    }

    #[test]
    fn prune_bounds_history_on_action_potential() {
        let mut s = EngagementStateV2::new(ap_curve());
        // Fires that are far enough apart that earlier ones decay out
        // before later ones are recorded.
        s.record_fire(0, 1.0);
        s.record_fire(10_000, 1.0); // earlier fire should be pruned (far past multiplier_half_life)
        assert!(s.history.len() <= 1);
    }
}
