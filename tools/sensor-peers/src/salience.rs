//! Per-signal presentation-layer aging (ADR-121 outward gate, unified in
//! ADR-123).
//!
//! The engine (`sensor_trait::EngagementState` + `Curve::Exponential`) is
//! the shared firing-dynamics engine that ways consumes via
//! `way_fire_outcome`. This module is sensor-peers' parallel consumer:
//! one `EngagementState` per tracked signal, keyed by signal id (the
//! filename stem — the same shape `re:<id>` references in ADR-120
//! threaded replies, so reply resets are direct hash lookups).
//!
//! Kept separate from `lib.rs` to prevent the already-priority-sized
//! 1k-line `lib.rs` from growing further (see issue #22 and the
//! code-quality file-length scan).

use std::collections::HashMap;

use sensor_trait::{Curve, EngagementState, Tick, TickDelta};

/// Map of signal-id → engagement state, consulted before each
/// peer-signal observation leaves `read_signals`. Parameters are set
/// from attend's `signals:` config block; defaults are the hardcoded
/// ADR-121-era picks so sensor-peers stays functional without config.
pub struct SignalSalience {
    states: HashMap<String, EngagementState>,
    half_life: TickDelta,
    floor: f64,
}

impl SignalSalience {
    /// Default parameters mirror `SignalsConfig::default()` in attend —
    /// 30 min half-life, 0.3 presentation floor. The orchestrator
    /// overrides these at startup via `set_params`.
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            half_life: 1800,
            floor: 0.3,
        }
    }

    pub fn set_params(&mut self, half_life_secs: u64, floor: f64) {
        self.half_life = half_life_secs;
        self.floor = floor;
    }

    /// Outward-gate check for a newly-scanned signal.
    ///
    /// Returns `true` iff the signal is loud enough to present. On the
    /// first call for a given `signal_id`, the state is seeded with a
    /// `record_fire(arrival_tick, 1.0)` — which means a freshly-scanned
    /// old backlog signal is gated as if it had been presented at its
    /// on-disk mtime, so its salience has already decayed when the new
    /// observer first sees it. That is the backlog-filter behavior
    /// ADR-121 designed and ADR-123 made a cross-tool facility.
    ///
    /// This is a deliberate asymmetry vs. ways' `way_fire_outcome`,
    /// which always fires on first match regardless of age. The peer
    /// sensor is consuming a *shared* resource (the on-disk signal
    /// directory) rather than a per-session event stream, so aging has
    /// to bite on first observation or the backlog-filter win never
    /// materializes.
    ///
    /// Below-floor entries are pruned before returning to bound the map
    /// size on long sessions and small floor values. The caller's
    /// `seen_signals` invariant prevents re-gating the same file, so
    /// the pruned state cannot leak back through a later scan.
    pub fn gate(&mut self, signal_id: &str, arrival_tick: Tick, now: Tick) -> bool {
        let half_life = self.half_life;
        let floor = self.floor;
        let state = self.states.entry(signal_id.to_string()).or_insert_with(|| {
            let mut s = EngagementState::new(Curve::Exponential { half_life });
            s.record_fire(arrival_tick, 1.0);
            s
        });
        if state.current_salience(now) >= floor {
            true
        } else {
            self.states.remove(signal_id);
            false
        }
    }

    /// Re-engagement reset for a threaded reply.
    ///
    /// When a peer's signal carries `re:<id>` (ADR-120), the parent
    /// signal's salience returns to 1.0 at `now`. This is the
    /// "thread still alive" signal from ADR-121 step 6. The effect is
    /// observable in two places:
    ///
    /// 1. Any future observer scanning the same shared signal dir (e.g.,
    ///    a new focus-group member joining) will see the parent as
    ///    loud-at-`now` instead of decayed-from-arrival.
    /// 2. State persisted to checkpoint preserves the reset across
    ///    restarts, so reconnection does not re-age the parent.
    ///
    /// The current `seen_signals` invariant in the sensor still prevents
    /// the same observer from surfacing the same signal twice. Removing
    /// that restriction is out of scope for the initial salience-gate
    /// landing and tracked as a follow-up on issue #22.
    pub fn reset(&mut self, signal_id: &str, now: Tick) {
        let half_life = self.half_life;
        let state = self
            .states
            .entry(signal_id.to_string())
            .or_insert_with(|| EngagementState::new(Curve::Exponential { half_life }));
        state.record_fire(now, 1.0);
    }

    /// Export state for checkpoint persistence. Each row is
    /// `("signal_salience", "<signal_id>\t<json-engagement-state>")`.
    /// The key prefix lives in `lib.rs` so the checkpoint schema stays
    /// in one place.
    pub fn export_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::with_capacity(self.states.len());
        for (id, state) in &self.states {
            if let Ok(json) = serde_json::to_string(state) {
                rows.push((id.clone(), json));
            }
        }
        rows
    }

    /// Re-import one serialized row. Returns `true` if the row parsed
    /// cleanly and was inserted.
    pub fn import_row(&mut self, signal_id: &str, json: &str) -> bool {
        match serde_json::from_str::<EngagementState>(json) {
            Ok(state) => {
                self.states.insert(signal_id.to_string(), state);
                true
            }
            Err(_) => false,
        }
    }

}

impl Default for SignalSalience {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the `re:<id>` reference from a parsed signal content string,
/// if the content is in the ADR-120 threaded 5-field form.
///
/// Returns the bare id (without `re:` prefix) so callers can look it up
/// directly in `SignalSalience`. Returns `None` for legacy 4-field
/// signals and for malformed `re:` prefixes.
///
/// Kept parallel to `parse_signal` rather than widening that function's
/// return type so the existing 5 parse-signal tests and all current
/// callers keep their tuple shape. See ADR-120 §3 for the discriminator
/// rules this mirrors.
pub fn extract_re_id(content: &str) -> Option<&str> {
    let parts: Vec<&str> = content.splitn(5, '|').collect();
    if parts.len() < 5 {
        return None;
    }
    let field = parts[3];
    let id = field.strip_prefix("re:")?;
    if is_valid_signal_id(id) {
        Some(id)
    } else {
        None
    }
}

/// Mirror of the `is_valid_signal_id` helper in `lib.rs`. Kept in sync
/// by convention — both fences exist for the same reason and are
/// exercised by the same ADR-120 parse rules.
fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Strip the trailing `.signal` extension from a filename to get the
/// signal id (the filename stem). The id is what `re:<id>` references.
pub fn signal_id_from_filename(filename: &str) -> &str {
    filename.strip_suffix(".signal").unwrap_or(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshly_arrived_signal_passes_gate() {
        let mut s = SignalSalience::new();
        s.set_params(1800, 0.3);
        // Signal arrived now — passes.
        assert!(s.gate("id-1", 1_000_000, 1_000_000));
    }

    #[test]
    fn old_signal_below_floor_is_suppressed() {
        let mut s = SignalSalience::new();
        s.set_params(1800, 0.3);
        // half_life=1800, floor=0.3. Delta for salience to drop to 0.3:
        // 0.3 = 0.5^(delta/1800) → delta = 1800 * log2(1/0.3) ≈ 3126.
        // delta = 4000 is comfortably past the floor.
        assert!(!s.gate("id-1", 1_000_000, 1_004_000));
    }

    #[test]
    fn reset_restores_salience_to_one() {
        let mut s = SignalSalience::new();
        s.set_params(1800, 0.3);
        // Age the signal past the floor.
        assert!(!s.gate("id-1", 1_000_000, 1_004_000));
        // Reset at a later tick — the state is now fresh-at-reset.
        s.reset("id-1", 1_004_000);
        // At reset tick, a subsequent gate call would see salience 1.0.
        // (We don't re-gate here because gate() mutates on first-call
        // and the state already exists — assert via a direct read of
        // the reset tick.)
        assert!(s.gate("id-1", 1_000_000, 1_004_000));
    }

    #[test]
    fn below_floor_gate_prunes_state_entry() {
        let mut s = SignalSalience::new();
        s.set_params(1800, 0.3);
        // Fresh gate seeds the entry.
        assert!(s.gate("id-live", 1_000_000, 1_000_000));
        // A second signal ages out — its entry should be pruned.
        assert!(!s.gate("id-old", 1_000_000, 1_004_000));
        // Export rows confirm only the live entry survived.
        let rows = s.export_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "id-live");
    }

    #[test]
    fn export_import_roundtrip_preserves_reset_tick() {
        let mut s = SignalSalience::new();
        s.set_params(1800, 0.3);
        s.reset("id-1", 1_000_000);
        let rows = s.export_rows();
        assert_eq!(rows.len(), 1);

        let mut t = SignalSalience::new();
        t.set_params(1800, 0.3);
        for (id, json) in &rows {
            assert!(t.import_row(id, json));
        }
        // Same decay math after roundtrip.
        assert!(t.gate("id-1", 1_000_000, 1_000_000));
    }

    #[test]
    fn extract_re_id_legacy_signal_returns_none() {
        assert_eq!(
            extract_re_id("claude:abc|proj|/home/a|hello there"),
            None
        );
    }

    #[test]
    fn extract_re_id_threaded_signal_returns_id() {
        assert_eq!(
            extract_re_id("claude:abc|proj|/home/a|re:parent-1234|body"),
            Some("parent-1234")
        );
    }

    #[test]
    fn extract_re_id_malformed_re_prefix_returns_none() {
        // "re:" followed by prose with a space fails is_valid_signal_id.
        assert_eq!(
            extract_re_id("claude:abc|proj|/home/a|re:has space|body"),
            None
        );
    }

    #[test]
    fn extract_re_id_empty_id_returns_none() {
        assert_eq!(
            extract_re_id("claude:abc|proj|/home/a|re:|body"),
            None
        );
    }

    #[test]
    fn signal_id_from_filename_strips_extension() {
        assert_eq!(
            signal_id_from_filename("claude-abc-1712345.signal"),
            "claude-abc-1712345"
        );
        assert_eq!(signal_id_from_filename("no-extension"), "no-extension");
    }
}
