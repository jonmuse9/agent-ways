//! State persistence for attend.
//!
//! Checkpoints sensor state to ~/.cache/attend/state/{session-id}.state
//! on clean shutdown and periodically during operation.
//! Restores on startup if state file exists for this session.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Serializable state snapshot.
/// Simple line-oriented format — no serde dependency.
///
/// Format:
///   seen_signals: key1,key2,key3,...
///   disclosed_thresholds: 40,50,65,...
///   context_pct: 31.2
///   reply_hint_shown: true
///   git_branch: main
///   git_head: abc1234
///   version: 0.1.0 (abc1234)
#[derive(Debug, Default)]
pub struct StateSnapshot {
    pub seen_signals: HashSet<String>,
    pub disclosed_thresholds: Vec<u8>,
    pub context_pct: Option<f64>,
    pub reply_hint_shown: bool,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

impl StateSnapshot {
    /// Serialize to line-oriented format.
    fn serialize(&self) -> String {
        let mut lines = Vec::new();

        if !self.seen_signals.is_empty() {
            // Encode pipe characters in keys to avoid format confusion
            let signals: Vec<String> = self.seen_signals.iter()
                .map(|s| s.replace('\n', "\\n"))
                .collect();
            lines.push(format!("seen_signal_count: {}", signals.len()));
            for s in &signals {
                lines.push(format!("seen_signal: {}", s));
            }
        }

        if !self.disclosed_thresholds.is_empty() {
            let thresholds: Vec<String> = self.disclosed_thresholds.iter()
                .map(|t| t.to_string())
                .collect();
            lines.push(format!("disclosed_thresholds: {}", thresholds.join(",")));
        }

        if let Some(pct) = self.context_pct {
            lines.push(format!("context_pct: {:.1}", pct));
        }

        lines.push(format!("reply_hint_shown: {}", self.reply_hint_shown));

        if let Some(ref branch) = self.git_branch {
            lines.push(format!("git_branch: {}", branch));
        }
        if let Some(ref head) = self.git_head {
            lines.push(format!("git_head: {}", head));
        }

        lines.push(format!("version: {} ({})",
            env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT")));

        lines.join("\n") + "\n"
    }

    /// Deserialize from line-oriented format.
    fn deserialize(content: &str) -> Self {
        let mut state = StateSnapshot::default();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }

            if let Some((key, value)) = line.split_once(": ") {
                match key {
                    "seen_signal" => {
                        state.seen_signals.insert(value.replace("\\n", "\n"));
                    }
                    "disclosed_thresholds" => {
                        state.disclosed_thresholds = value.split(',')
                            .filter_map(|s| s.trim().parse().ok())
                            .collect();
                    }
                    "context_pct" => {
                        state.context_pct = value.parse().ok();
                    }
                    "reply_hint_shown" => {
                        state.reply_hint_shown = value == "true";
                    }
                    "git_branch" => {
                        state.git_branch = Some(value.to_string());
                    }
                    "git_head" => {
                        state.git_head = Some(value.to_string());
                    }
                    _ => {} // ignore unknown keys for forward compat
                }
            }
        }

        state
    }
}

/// State store manages checkpoint/restore for a session.
pub struct StateStore {
    state_dir: PathBuf,
    session_id: Option<String>,
}

impl StateStore {
    pub fn new(session_id: Option<String>) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self {
            state_dir: PathBuf::from(home).join(".cache").join("attend").join("state"),
            session_id,
        }
    }

    fn state_path(&self) -> Option<PathBuf> {
        self.session_id.as_ref().map(|id| {
            self.state_dir.join(format!("{}.state", id))
        })
    }

    /// Try to load existing state for this session.
    pub fn restore(&self) -> Option<StateSnapshot> {
        let path = self.state_path()?;
        let content = fs::read_to_string(&path).ok()?;
        let state = StateSnapshot::deserialize(&content);
        eprintln!("[attend] state: restored from {} ({} seen signals, {} disclosed thresholds)",
            path.display(), state.seen_signals.len(), state.disclosed_thresholds.len());
        Some(state)
    }

    /// Checkpoint current state to disk. Atomic write.
    pub fn checkpoint(&self, state: &StateSnapshot) {
        let path = match self.state_path() {
            Some(p) => p,
            None => return,
        };

        fs::create_dir_all(&self.state_dir).ok();

        let tmp = path.with_extension("tmp");
        let content = state.serialize();
        if fs::write(&tmp, &content).is_ok() {
            fs::rename(&tmp, &path).ok();
        }
    }

    /// Remove state file (on clean exit if desired).
    pub fn clear(&self) {
        if let Some(path) = self.state_path() {
            fs::remove_file(&path).ok();
        }
    }
}
