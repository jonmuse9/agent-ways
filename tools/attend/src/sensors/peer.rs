use super::{Focus, Sensor};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

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
    /// Signals dir
    signals_dir: PathBuf,
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
}

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
            signals_dir: home.join(".cache").join("attend").join("signals"),
            prior: HashMap::new(),
            seen_signals: HashSet::new(),
            reply_hint_shown: false,
            own_session_id,
            baseline_established: false,
        }
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
        let focus_file = base.join("focus");
        if let Ok(content) = fs::read_to_string(&focus_file) {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    scan_dirs.push(base.join(encode_cwd(line)));
                }
            }
        }

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
    /// and any dirs in the focus list. Returns observations for new signals.
    fn read_signals(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let mut observations = Vec::new();
        let base = signals_base();

        // Directories to scan: own project + broadcast + focus group
        let own_encoded = encode_cwd(&focus.working_dir);
        let mut scan_dirs = vec![
            base.join(&own_encoded),
            base.join("_broadcast"),
        ];

        // Add focus group dirs
        let focus_file = base.join("focus");
        if let Ok(content) = fs::read_to_string(&focus_file) {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    scan_dirs.push(base.join(encode_cwd(line)));
                }
            }
        }

        let own_session_id = self.own_session_id.as_deref().unwrap_or("---none---");

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

                    let sender = match kind {
                        "claude" => format!("claude/{}", project),
                        "external" => identity.to_string(),
                        _ => format!("{} ({})", project, from),
                    };

                    // Include reply hint only on first peer message
                    let source_cwd = parts[2];
                    if !self.reply_hint_shown {
                        observations.push((5.0, format!(
                            "message from {}: {} (reply: attend send --to {} <msg>)",
                            sender, message, source_cwd
                        )));
                        self.reply_hint_shown = true;
                    } else {
                        observations.push((5.0, format!(
                            "message from {}: {}", sender, message
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
        let read_from = if file_len > 8192 { file_len - 8192 } else { 0 };

        let content = fs::read_to_string(&path).ok()?;
        let mut model = String::from("-");
        let mut last_input: u64 = 0;
        let mut last_cache_read: u64 = 0;
        let mut last_cache_create: u64 = 0;

        // Only parse lines from near the end
        let start_byte = if read_from > 0 { read_from as usize } else { 0 };
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
        let context_window: u64 = if model.contains("[1m]") { 1_000_000 } else { 200_000 };
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
                    "peer session started: {} ({}, {}, ctx {:.0}%)",
                    peer.project_name, relevance, peer.status, peer.context_percent
                )));
            }
        }

        // Exited peers
        for (sid, peer) in &self.prior {
            if !current.contains_key(sid) {
                let magnitude = if peer.cwd == focus.working_dir { 2.0 } else { 0.5 };
                observations.push((magnitude, format!(
                    "peer session exited: {}", peer.project_name
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
                        "peer {} now {} (was {})",
                        peer.project_name, peer.status, prior.status
                    )));
                }

                // Context pressure — peer approaching limits
                if peer.context_percent >= 80.0 && prior.context_percent < 80.0 {
                    observations.push((2.0, format!(
                        "peer {} context at {:.0}% — approaching compaction",
                        peer.project_name, peer.context_percent
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
fn find_own_session_id(own_pid: u32) -> Option<String> {
    let mut pid = own_pid;
    let mut claude_pid = None;
    for _ in 0..10 {
        if pid <= 1 { break; }
        let output = Command::new("ps")
            .args(["--no-headers", "-p", &pid.to_string(), "-o", "ppid,comm"])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
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
