use sensor_trait::{Focus, Sensor};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback that returns the current set of extra signal directories
/// (typically focus-group dirs). Called on every scan so mid-session
/// group join/leave is reflected without restarting the sensor loop.
pub type ExtraScanDirsFn = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

/// Discovers peer Claude Code sessions by reading ~/.claude/sessions/*.json
/// and their transcript files. Same discovery pattern as abtop.
///
/// Also reads signal files from ~/.cache/attend/signals/ for peer messages.
///
/// Reports deltas when peers appear, disappear, or change state.
/// Filters through focus: only surfaces peers in the same working directory
/// (or with overlapping git branches) as noteworthy.
pub struct PeerSensor {
    /// Our own PID, so we can exclude self
    own_pid: u32,
    /// Sessions dir
    sessions_dir: PathBuf,
    /// Projects dir (for transcripts)
    projects_dir: PathBuf,
    /// Previous snapshot: session_id → summary
    prior: HashMap<String, PeerSummary>,
    /// Signal files we've already seen (by filename)
    seen_signals: HashSet<String>,
    /// Whether we've shown the reply hint (only show once per session)
    reply_hint_shown: bool,
    /// Our own session ID (to skip our own signals)
    own_session_id: Option<String>,
    /// First poll establishes baseline
    baseline_established: bool,
    /// Provider that returns additional signal directories to scan on each
    /// poll (e.g., focus-group dirs from ADR-118). A closure lets the sensor
    /// pick up focus-group joins and leaves that happen after startup
    /// without the orchestrator having to push updates. Set via
    /// `set_extra_scan_dirs_provider()`.
    extra_scan_dirs_fn: Option<ExtraScanDirsFn>,
    /// Per-peer message timestamps for engagement-based magnitude boosting.
    /// Keyed by "from" field (e.g., "claude:<session_id>").
    /// When the same peer sends multiple messages in a window, their
    /// subsequent messages get a magnitude boost so they can break through
    /// the elevated refractory threshold in the peer sensor.
    peer_activity: HashMap<String, VecDeque<Instant>>,
    /// Sliding window for per-peer engagement boost calculation.
    /// Set via `set_peer_activity_window` from attend's engagement config.
    peer_activity_window: Duration,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct PeerSummary {
    pid: u32,
    cwd: String,
    project_name: String,
    context_percent: f64,
    model: String,
    status: PeerStatus,
}

#[derive(Clone, Debug, PartialEq)]
enum PeerStatus {
    Working,
    Waiting,
    Unknown,
}

impl std::fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerStatus::Working => write!(f, "working"),
            PeerStatus::Waiting => write!(f, "waiting"),
            PeerStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Minimal session file structure — matches Claude Code's format.
/// Only the fields we need; everything else is ignored.
#[derive(Debug)]
struct SessionFile {
    pid: u32,
    cwd: String,
    session_id: String,
}

impl PeerSensor {
    pub fn new() -> Self {
        let home = home_dir();
        let own_pid = std::process::id();
        let own_session_id = find_own_session_id(own_pid);
        Self {
            own_pid,
            sessions_dir: home.join(".claude").join("sessions"),
            projects_dir: home.join(".claude").join("projects"),
            prior: HashMap::new(),
            seen_signals: HashSet::new(),
            reply_hint_shown: false,
            own_session_id,
            baseline_established: false,
            extra_scan_dirs_fn: None,
            peer_activity: HashMap::new(),
            peer_activity_window: Duration::from_secs(900),
        }
    }

    /// Set the per-peer engagement window. Called by the orchestrator
    /// to align with the attend engagement config.
    pub fn set_peer_activity_window(&mut self, window: Duration) {
        self.peer_activity_window = window;
    }

    /// Compute the magnitude boost for a peer based on their recent activity.
    /// Records the current message and returns the boost multiplier.
    ///
    /// The boost creates a gradient: messages from peers who've been actively
    /// exchanging messages climb above the elevated refractory threshold
    /// while background broadcasts stay at baseline and get suppressed.
    ///
    /// Window is 10 minutes — sized to Claude's actual turn cadence, where
    /// 3 messages between agents takes 5-10 minutes of wall clock.
    ///
    /// - 1st message in window: 1.0x (entry level, fires at rest)
    /// - 2nd message: 1.75x (participant emerging)
    /// - 3rd+ message: 2.5x (established conversation partner — reliably
    ///   breaks through refractory)
    fn peer_engagement_boost(&mut self, from: &str) -> f64 {
        let now = Instant::now();
        let window = self.peer_activity_window;
        let history = self.peer_activity.entry(from.to_string()).or_default();
        // Prune old entries
        while let Some(front) = history.front() {
            if now.duration_since(*front) > window {
                history.pop_front();
            } else {
                break;
            }
        }
        history.push_back(now);
        match history.len() {
            0 | 1 => 1.0,
            2 => 1.75,
            _ => 2.5,
        }
    }

    /// Register a provider for additional signal directories. The closure
    /// is invoked on every scan, so mid-session focus-group join/leave
    /// propagates without restarting the sensor loop.
    pub fn set_extra_scan_dirs_provider(&mut self, f: ExtraScanDirsFn) {
        self.extra_scan_dirs_fn = Some(f);
    }

    /// Current snapshot of extra scan dirs. Empty if no provider is set.
    fn current_extra_scan_dirs(&self) -> Vec<PathBuf> {
        match &self.extra_scan_dirs_fn {
            Some(f) => f(),
            None => Vec::new(),
        }
    }

    /// Return a list of active peer sessions as (cwd, project_name, status, context_percent).
    pub fn list_peers(&self) -> Vec<(String, String, String, f64)> {
        let peers = self.discover_peers();
        let mut result: Vec<_> = peers.values()
            .map(|p| (p.cwd.clone(), p.project_name.clone(), p.status.to_string(), p.context_percent))
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Mark all existing signal files as already seen so attend run
    /// only processes signals that arrive after startup.
    pub fn mark_existing_as_seen(&mut self, focus: &Focus) {
        let base = signals_base();
        let own_encoded = encode_cwd(&focus.working_dir);
        let mut scan_dirs = vec![
            base.join(&own_encoded),
            base.join("_broadcast"),
        ];
        // Focus group directories (ADR-118 — named signal namespaces)
        scan_dirs.extend(self.current_extra_scan_dirs());

        for dir in &scan_dirs {
            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(f) = path.file_name().and_then(|f| f.to_str()) {
                    if f.ends_with(".signal") {
                        let key = format!("{}:{}", dir.display(), f);
                        self.seen_signals.insert(key);
                    }
                }
            }
        }

        let count = self.seen_signals.len();
        if count > 0 {
            eprintln!("[attend] peers: marked {} existing signals as seen", count);
        }
    }

    /// Read signal files from peers. Scans own project dir, broadcast dir,
    /// focus list, and joined rooms. Returns observations for new signals.
    fn read_signals(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let mut observations = Vec::new();
        let base = signals_base();

        // Directories to scan: own project + broadcast + focus group + rooms
        let own_encoded = encode_cwd(&focus.working_dir);
        let mut scan_dirs = vec![
            base.join(&own_encoded),
            base.join("_broadcast"),
        ];

        // Focus group directories (ADR-118 — named signal namespaces)
        scan_dirs.extend(self.current_extra_scan_dirs());

        let own_session_id: String = self.own_session_id
            .clone()
            .unwrap_or_else(|| "---none---".to_string());

        for dir in &scan_dirs {
            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let filename = match path.file_name().and_then(|f| f.to_str()) {
                    Some(f) if f.ends_with(".signal") => f.to_string(),
                    _ => continue,
                };

                // Skip already-seen (use full path to avoid collisions across dirs)
                let key = format!("{}:{}", dir.display(), filename);
                if self.seen_signals.contains(&key) {
                    continue;
                }

                // Read and parse: from|project|cwd|message
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let content = content.trim();
                let parts: Vec<&str> = content.splitn(4, '|').collect();
                if parts.len() == 4 {
                    let from = parts[0];

                    // Skip our own signals — check the from field, not filename.
                    // from is "claude:session-id" or "external:user@terminal"
                    if let Some((_kind, identity)) = from.split_once(':') {
                        if identity == own_session_id {
                            self.seen_signals.insert(key);
                            continue;
                        }
                    }
                    let project = parts[1];
                    let message = parts[3];

                    let (kind, identity) = from.split_once(':')
                        .unwrap_or(("unknown", from));

                    let source_cwd = parts[2];
                    let sender = match kind {
                        "claude" => format!("claude/{}", source_cwd),
                        "external" => identity.to_string(),
                        _ => format!("{} ({})", project, from),
                    };

                    // Directed messages (in own project dir) get highest priority.
                    // Broadcast and focus group messages are important but less urgent.
                    let dir_name = dir.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    let base_magnitude: f64 = if dir_name == own_encoded {
                        7.0 // directed to us — someone used --to
                    } else if dir_name == "_broadcast" {
                        4.0 // broadcast — important but not targeted
                    } else {
                        5.0 // focus group — relevant peer
                    };

                    // Boost by peer engagement: repeated messages from the
                    // same peer within a window increase magnitude, so active
                    // conversation partners break through elevated refractory
                    // thresholds while uninvolved broadcasts stay at baseline
                    // (and get suppressed when the peer sensor is refractory).
                    // This is the "auto-grouping" mechanism — conversation
                    // emerges from observed traffic rather than explicit config.
                    let from_owned = from.to_string();
                    let boost = self.peer_engagement_boost(&from_owned);
                    let magnitude = base_magnitude * boost;

                    // Truncate long messages inline. No mailbox pointer —
                    // Claude should not need a second lookup. Each event is
                    // its own Monitor line, so the ~500-char truncation limit
                    // is per-event, not per-batch.
                    let display_msg = if message.len() > 600 {
                        format!("{}... [truncated, {} chars total]", &message[..600], message.len())
                    } else {
                        message.to_string()
                    };

                    // Include reply hint only on first peer message.
                    // Hint uses the simplest possible form: no --to, no paths.
                    if !self.reply_hint_shown {
                        observations.push((magnitude, format!(
                            "message from {}: {} (reply: attend send <msg>)",
                            sender, display_msg
                        )));
                        self.reply_hint_shown = true;
                    } else {
                        observations.push((magnitude, format!(
                            "message from {}: {}", sender, display_msg
                        )));
                    }
                }

                self.seen_signals.insert(key);

                // Clean up stale signals (older than 5 minutes)
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(modified) = metadata.modified() {
                        if modified.elapsed().unwrap_or_default().as_secs() > 300 {
                            fs::remove_file(&path).ok();
                        }
                    }
                }
            }
        }

        observations
    }

    fn discover_peers(&self) -> HashMap<String, PeerSummary> {
        let mut peers = HashMap::new();

        let entries = match fs::read_dir(&self.sessions_dir) {
            Ok(e) => e,
            Err(_) => return peers,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let sf = match parse_session_file(&content) {
                Some(sf) => sf,
                None => continue,
            };

            // Skip our own parent claude process.
            // attend's parent is claude, so check the ancestry.
            if is_own_session(sf.pid, self.own_pid) {
                continue;
            }

            // Check if PID is alive and is a claude process
            if !pid_is_claude(sf.pid) {
                continue;
            }

            let project_name = sf.cwd
                .rsplit('/')
                .next()
                .unwrap_or("?")
                .to_string();

            // Read transcript for context % and model
            let (context_percent, model) = self
                .read_transcript_summary(&sf.cwd, &sf.session_id)
                .unwrap_or((0.0, "-".to_string()));

            // Determine status from transcript mtime
            let status = self.infer_status(&sf.cwd, &sf.session_id);

            peers.insert(sf.session_id, PeerSummary {
                pid: sf.pid,
                cwd: sf.cwd,
                project_name,
                context_percent,
                model,
                status,
            });
        }

        peers
    }

    fn read_transcript_summary(&self, cwd: &str, session_id: &str) -> Option<(f64, String)> {
        let encoded = encode_cwd(cwd);
        let path = self.projects_dir.join(&encoded).join(format!("{session_id}.jsonl"));

        if !path.exists() {
            return None;
        }

        // Read last few KB for recent usage data — don't parse the whole file
        let metadata = fs::metadata(&path).ok()?;
        let file_len = metadata.len();
        let read_from = file_len.saturating_sub(8192);

        let content = fs::read_to_string(&path).ok()?;
        let mut model = String::from("-");
        let mut last_input: u64 = 0;
        let mut last_cache_read: u64 = 0;
        let mut last_cache_create: u64 = 0;

        // Only parse lines from near the end.
        // Find the nearest char boundary to avoid panicking on multi-byte UTF-8.
        let mut start_byte = if read_from > 0 { read_from as usize } else { 0 };
        if start_byte < content.len() {
            while start_byte < content.len() && !content.is_char_boundary(start_byte) {
                start_byte += 1;
            }
        }
        let tail = if start_byte < content.len() { &content[start_byte..] } else { &content };

        for line in tail.lines() {
            // Quick check before parsing full JSON
            if !line.contains("\"assistant\"") {
                continue;
            }
            // Minimal JSON parsing — extract just the fields we need
            if let Some(m) = extract_json_string(line, "model") {
                model = m;
            }
            if let Some(inp) = extract_json_u64(line, "input_tokens") {
                last_input = inp;
            }
            if let Some(cr) = extract_json_u64(line, "cache_read_input_tokens") {
                last_cache_read = cr;
            }
            if let Some(cc) = extract_json_u64(line, "cache_creation_input_tokens") {
                last_cache_create = cc;
            }
        }

        let context_tokens = last_input + last_cache_read + last_cache_create;
        // Model string may include context window hint like "claude-opus-4-6[1m]"
        // Default to 1M for opus/sonnet 4.x, 200K for older models
        let context_window: u64 = if model.contains("[1m]") || model.contains("1m]")
            || model.contains("opus-4") || model.contains("sonnet-4") {
            1_000_000
        } else if model == "-" {
            // Unknown model — can't compute meaningful percentage
            0
        } else {
            200_000
        };
        let context_percent = if context_window > 0 {
            (context_tokens as f64 / context_window as f64) * 100.0
        } else {
            0.0
        };

        Some((context_percent, model))
    }

    fn infer_status(&self, cwd: &str, session_id: &str) -> PeerStatus {
        let encoded = encode_cwd(cwd);
        let path = self.projects_dir.join(&encoded).join(format!("{session_id}.jsonl"));

        let mtime = fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());

        match mtime {
            Some(t) => {
                let age = t.elapsed().unwrap_or_default();
                if age.as_secs() < 30 {
                    PeerStatus::Working
                } else {
                    PeerStatus::Waiting
                }
            }
            None => PeerStatus::Unknown,
        }
    }
}

impl Default for PeerSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for PeerSensor {
    fn name(&self) -> &str {
        "peers"
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = self.discover_peers();

        // First poll: establish baseline
        if !self.baseline_established {
            let same_dir: Vec<&PeerSummary> = current.values()
                .filter(|p| p.cwd == focus.working_dir)
                .collect();
            eprintln!(
                "[attend] peers: baseline — {} sessions ({} in this project)",
                current.len(), same_dir.len(),
            );
            self.prior = current;
            self.baseline_established = true;
            return Vec::new();
        }

        let mut observations = Vec::new();

        // New peers
        for (sid, peer) in &current {
            if !self.prior.contains_key(sid) {
                let relevance = if peer.cwd == focus.working_dir {
                    "same project"
                } else {
                    "other project"
                };
                let magnitude = if peer.cwd == focus.working_dir { 3.0 } else { 1.0 };
                observations.push((magnitude, format!(
                    "peer session started: {} [{}] ({}, {}, ctx {:.0}%)",
                    peer.project_name, peer.cwd, relevance, peer.status, peer.context_percent
                )));
            }
        }

        // Exited peers
        for (sid, peer) in &self.prior {
            if !current.contains_key(sid) {
                let magnitude = if peer.cwd == focus.working_dir { 2.0 } else { 0.5 };
                observations.push((magnitude, format!(
                    "peer session exited: {} [{}]", peer.project_name, peer.cwd
                )));
            }
        }

        // State changes in existing peers (only for same-project peers)
        for (sid, peer) in &current {
            if let Some(prior) = self.prior.get(sid) {
                // Only track peers in same project
                if peer.cwd != focus.working_dir {
                    continue;
                }

                // Status changed
                if peer.status != prior.status {
                    observations.push((1.5, format!(
                        "peer {} [{}] now {} (was {})",
                        peer.project_name, peer.cwd, peer.status, prior.status
                    )));
                }

                // Context pressure — peer approaching limits
                if peer.context_percent >= 80.0 && prior.context_percent < 80.0 {
                    observations.push((2.0, format!(
                        "peer {} [{}] context at {:.0}% — approaching compaction",
                        peer.project_name, peer.cwd, peer.context_percent
                    )));
                }
            }
        }

        // Check for peer signals (messages from other sessions)
        observations.extend(self.read_signals(focus));

        self.prior = current;
        observations
    }

    fn emission_threshold(&self) -> f64 {
        2.0 // Same-project peer events are magnitude 2-3, others are lower
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(30) // Check every 30s
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(10) // Don't scan sessions faster than 10s
    }

    fn decay_threshold(&self) -> u32 {
        5
    }

    fn export_state(&self) -> Vec<(String, String)> {
        let mut state = Vec::new();
        for sig in &self.seen_signals {
            state.push(("seen_signal".to_string(), sig.clone()));
        }
        state.push(("reply_hint_shown".to_string(), self.reply_hint_shown.to_string()));
        state
    }

    fn import_state(&mut self, state: &[(String, String)]) {
        for (key, value) in state {
            match key.as_str() {
                "seen_signal" => { self.seen_signals.insert(value.clone()); }
                "reply_hint_shown" => { self.reply_hint_shown = value == "true"; }
                _ => {}
            }
        }
        if !self.seen_signals.is_empty() {
            eprintln!("[attend] peers: restored {} seen signals from checkpoint",
                self.seen_signals.len());
        }
    }
}

// --- Helpers ---

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn signals_base() -> PathBuf {
    home_dir().join(".cache").join("attend").join("signals")
}

/// Encode a cwd path to match Claude Code's project directory naming.
/// Same encoding as abtop uses: '/', '_', '.' → '-'
fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

/// Parse a session JSON file. Only extracts pid, cwd, session_id.
/// Avoids pulling in serde_json — these files are small and stable.
fn parse_session_file(content: &str) -> Option<SessionFile> {
    let pid = extract_json_u64(content, "pid")? as u32;
    let cwd = extract_json_string(content, "cwd")?;
    let session_id = extract_json_string(content, "sessionId")?;
    Some(SessionFile { pid, cwd, session_id })
}

/// Check if a PID is alive and running a claude process.
fn pid_is_claude(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["--no-headers", "-p", &pid.to_string(), "-o", "comm"])
        .output()
        .ok();

    match output {
        Some(out) if out.status.success() => {
            let comm = String::from_utf8_lossy(&out.stdout);
            let comm = comm.trim();
            comm == "claude" || comm.contains("claude")
        }
        _ => false,
    }
}

/// Check if a session PID is an ancestor of our own PID (i.e., our session).
fn is_own_session(session_pid: u32, own_pid: u32) -> bool {
    // Walk up the process tree from own_pid
    let mut pid = own_pid;
    for _ in 0..10 {
        if pid == session_pid {
            return true;
        }
        if pid <= 1 {
            break;
        }
        // Get parent PID
        let output = Command::new("ps")
            .args(["--no-headers", "-p", &pid.to_string(), "-o", "ppid"])
            .output()
            .ok();
        match output {
            Some(out) if out.status.success() => {
                let ppid_str = String::from_utf8_lossy(&out.stdout);
                match ppid_str.trim().parse::<u32>() {
                    Ok(ppid) if ppid > 0 && ppid != pid => pid = ppid,
                    _ => break,
                }
            }
            _ => break,
        }
    }
    false
}

/// Quick-and-dirty JSON string extraction without serde.
/// Finds "key":"value" patterns. Good enough for Claude Code's stable format.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Quick-and-dirty JSON number extraction without serde.
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    // Try "key":value (no quotes around number)
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse().ok()
}

/// Find our own session ID by walking up the process tree to the claude parent,
/// then matching against ~/.claude/sessions/*.json.
/// Find the Claude session ID for the current process by walking up the
/// process tree to find the claude parent, then matching against session files.
pub fn find_own_session_id(own_pid: u32) -> Option<String> {
    let mut pid = own_pid;
    let mut claude_pid = None;
    for _ in 0..10 {
        if pid <= 1 { break; }
        let output = Command::new("ps")
            .args(["--no-headers", "-p", &pid.to_string(), "-o", "ppid,comm"])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1].contains("claude") {
            claude_pid = Some(pid);
            break;
        }
        pid = parts.first()?.parse().ok()?;
    }

    let claude_pid = claude_pid?;
    let home = home_dir();
    let sessions_dir = home.join(".claude").join("sessions");
    for entry in fs::read_dir(&sessions_dir).ok()?.flatten() {
        if let Ok(content) = fs::read_to_string(entry.path()) {
            let pid_pattern = format!("\"pid\":{}", claude_pid);
            if content.contains(&pid_pattern) {
                return extract_json_string(&content, "sessionId");
            }
        }
    }
    None
}
