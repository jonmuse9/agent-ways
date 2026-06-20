//! Signal-id parsing for peer `.signal` files.
//!
//! A signal's id is its filename stem (the part before `.signal`) — the
//! same value `re:<id>` threaded replies reference (ADR-120). The id keys
//! `attend reply`'s last-inbound record.

/// Strip the trailing `.signal` extension from a filename to get the
/// signal id (the filename stem).
pub fn signal_id_from_filename(filename: &str) -> &str {
    filename.strip_suffix(".signal").unwrap_or(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_id_from_filename_strips_extension() {
        assert_eq!(
            signal_id_from_filename("claude-abc-1712345.signal"),
            "claude-abc-1712345"
        );
        assert_eq!(signal_id_from_filename("no-extension"), "no-extension");
    }
}
