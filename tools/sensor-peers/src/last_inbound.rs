//! Per-session "last inbound signal" tracking.
//!
//! Sensor-peers writes the most-recently-surfaced peer signal id here so
//! that `attend reply` can auto-thread a response without the calling
//! agent having to discover the id. The state is scoped to the running
//! attend process's own Claude session id, so multiple concurrent
//! sessions don't step on each other's reply targets.
//!
//! Wire format is one line of plain text containing the signal id
//! (filename stem — same shape `re:<id>` references). This is the
//! simplest possible file format that survives process restarts and
//! stays human-inspectable; no JSON, no length-prefixing.
//!
//! Path: `~/.cache/attend/state/<session_id>.last-inbound`

use std::fs;
use std::path::PathBuf;

/// Build the path that stores the last-inbound signal id for the given
/// attend-owner session id. Does not create the directory.
pub fn path(session_id: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".cache")
        .join("attend")
        .join("state")
        .join(format!("{session_id}.last-inbound"))
}

/// Record `signal_id` as the most-recent inbound signal for `session_id`.
/// Creates the parent directory if needed. Silently ignores errors —
/// the caller (sensor-peers inside `read_signals`) cannot usefully
/// recover from a failed write here, and a missing file on the reply
/// side degrades to "no threaded reply," not a crash.
pub fn record(session_id: &str, signal_id: &str) {
    let p = path(session_id);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&p, signal_id).ok();
}

/// Read the last-inbound signal id for `session_id`, if one exists and
/// the file is non-empty. Trims trailing whitespace in case the file
/// picked up a newline from a hand-edit.
pub fn read(session_id: &str) -> Option<String> {
    let p = path(session_id);
    let raw = fs::read_to_string(&p).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_contains_session_id_and_ext() {
        let p = path("test-session-123");
        assert!(p.to_string_lossy().contains("test-session-123.last-inbound"));
        assert!(p.to_string_lossy().contains(".cache/attend/state"));
    }

    #[test]
    fn record_then_read_roundtrips() {
        // Use a unique session id to avoid colliding with any real state.
        let sid = format!("test-ri-{}", std::process::id());
        record(&sid, "parent-abc-1234");
        let got = read(&sid);
        assert_eq!(got.as_deref(), Some("parent-abc-1234"));
        // Clean up.
        fs::remove_file(path(&sid)).ok();
    }

    #[test]
    fn read_missing_returns_none() {
        assert_eq!(read("definitely-not-a-real-session-id-zzz-9999"), None);
    }
}
