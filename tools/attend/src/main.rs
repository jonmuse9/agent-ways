mod config;
mod state;
mod tick;
mod delta;
mod emit;
mod sensors;

use sensors::{ContextSensor, Focus, GitSensor, PeerSensor, ProcessSensor, ScriptSensor, SensorSlot};
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

// --- Disclosure governor ---

struct DisclosureGovernor {
    base_cooldown: Duration,
    last_disclosure: Option<Instant>,
    max_disclosures_per_window: u32,
    window_disclosures: u32,
    window_start: Instant,
    rate_window: Duration,
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
        let rate = self.aggregate_rate();
        let multiplier = 1.0 + rate.sqrt() * 3.0;
        self.base_cooldown.mul_f64(multiplier)
    }

    fn can_disclose(&mut self) -> bool {
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

// --- Subcommands ---

fn cmd_run_with_catchup(catchup: bool) {
    emit::log("starting attend");

    let focus = Focus::default_focus();
    emit::log(&format!("focus: {} ({})", focus.description, focus.working_dir));

    // Load config: user scope → project scope overlay
    let cfg = config::Config::load(&focus.working_dir);

    // Self-documenting startup
    let focus_list = read_focus_list(&signals_base().join("focus"));
    let focus_desc = if focus_list.is_empty() {
        "project only".to_string()
    } else {
        let names: Vec<&str> = focus_list.iter()
            .map(|p| p.rsplit('/').next().unwrap_or(p.as_str()))
            .collect();
        format!("project + {}", names.join(", "))
    };

    // Build sensor list from config
    let mut enabled_names: Vec<String> = Vec::new();
    let mut peer_sensor = PeerSensor::new();
    if !catchup {
        peer_sensor.mark_existing_as_seen(&focus);
    }

    let mut slots: Vec<SensorSlot> = Vec::new();

    // Built-in sensors — check config for enabled/disabled and overrides
    if cfg.sensors.get("context").map(|s| s.enabled).unwrap_or(true) {
        let sc = cfg.sensors.get("context");
        let sensor = ContextSensor::new();
        slots.push(SensorSlot::new_with_config(
            Box::new(sensor),
            sc.map(|s| s.interval).unwrap_or(Duration::from_secs(60)),
            sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(20)),
            sc.map(|s| s.decay_threshold).unwrap_or(3),
        ));
        enabled_names.push("context".to_string());
    }
    if cfg.sensors.get("processes").map(|s| s.enabled).unwrap_or(true) {
        let sc = cfg.sensors.get("processes");
        slots.push(SensorSlot::new_with_config(
            Box::new(ProcessSensor::new()),
            sc.map(|s| s.interval).unwrap_or(Duration::from_secs(30)),
            sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(5)),
            sc.map(|s| s.decay_threshold).unwrap_or(5),
        ));
        enabled_names.push("processes".to_string());
    }
    if cfg.sensors.get("git").map(|s| s.enabled).unwrap_or(true) {
        let sc = cfg.sensors.get("git");
        slots.push(SensorSlot::new_with_config(
            Box::new(GitSensor::new()),
            sc.map(|s| s.interval).unwrap_or(Duration::from_secs(30)),
            sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(10)),
            sc.map(|s| s.decay_threshold).unwrap_or(4),
        ));
        enabled_names.push("git".to_string());
    }
    if cfg.sensors.get("peers").map(|s| s.enabled).unwrap_or(true) {
        let sc = cfg.sensors.get("peers");
        slots.push(SensorSlot::new_with_config(
            Box::new(peer_sensor),
            sc.map(|s| s.interval).unwrap_or(Duration::from_secs(30)),
            sc.map(|s| s.min_interval).unwrap_or(Duration::from_secs(10)),
            sc.map(|s| s.decay_threshold).unwrap_or(5),
        ));
        enabled_names.push("peers".to_string());
    }

    // Script sensors from config (+ entries with script: field)
    for (name, sc) in &cfg.sensors {
        if let Some(ref script) = sc.script {
            if sc.enabled {
                let sensor = ScriptSensor::new(
                    name.clone(),
                    script.clone(),
                    focus.working_dir.clone(),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                    sc.threshold,
                );
                slots.push(SensorSlot::new_with_config(
                    Box::new(sensor),
                    sc.interval,
                    sc.min_interval,
                    sc.decay_threshold,
                ));
                enabled_names.push(name.clone());
            }
        }
    }

    // State persistence
    let session_id = own_session_id();
    let state_store = state::StateStore::new(session_id);

    // Try to restore state from previous run
    if let Some(snapshot) = state_store.restore() {
        // Distribute state to matching sensors
        for slot in &mut slots {
            let sensor_state: Vec<(String, String)> = match slot.name() {
                "peers" => snapshot.seen_signals.iter()
                    .map(|s| ("seen_signal".to_string(), s.clone()))
                    .chain(std::iter::once(("reply_hint_shown".to_string(),
                        snapshot.reply_hint_shown.to_string())))
                    .collect(),
                "context" => snapshot.disclosed_thresholds.iter()
                    .map(|t| ("disclosed_threshold".to_string(), t.to_string()))
                    .collect(),
                _ => Vec::new(),
            };
            if !sensor_state.is_empty() {
                slot.import_state(&sensor_state);
            }
        }
    }

    let sensor_list = enabled_names.join(", ");
    println!("[attend] v{} ({}) — sensors: {} | focus: {} | commands: attend send <msg>, attend inbox, attend peers, attend focus add <path>",
        env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"), sensor_list, focus_desc);

    let mut governor = DisclosureGovernor::new(
        cfg.governor.base_cooldown,
        cfg.governor.max_per_window,
        cfg.governor.rate_window,
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, slot) in slots.iter().enumerate() {
        queue.push(ScheduledSensor { fire_at: slot.next_fire, index: i });
    }

    // Checkpoint timer — save state every 30s
    let mut last_checkpoint = Instant::now();
    let checkpoint_interval = Duration::from_secs(30);

    emit::log(&format!("tick loop running — {} sensors registered", slots.len()));
    for slot in &slots {
        emit::log(&format!(
            "  {} (base={:.0}s, min={:.0}s, threshold={:.1})",
            slot.name(),
            slot.sensor.base_interval().as_secs_f64(),
            slot.sensor.min_interval().as_secs_f64(),
            slot.sensor.emission_threshold(),
        ));
    }

    // Self-reload: track own binary mtime
    let self_exe = std::env::current_exe().ok();
    let initial_mtime = self_exe.as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut last_reload_check = Instant::now();
    let reload_check_interval = Duration::from_secs(10);

    loop {
        // Check for binary change
        if last_reload_check.elapsed() >= reload_check_interval {
            if let (Some(ref exe), Some(ref orig_mtime)) = (&self_exe, &initial_mtime) {
                if let Ok(meta) = std::fs::metadata(exe) {
                    if let Ok(current_mtime) = meta.modified() {
                        if current_mtime != *orig_mtime {
                            // Binary changed — checkpoint and exec self
                            emit::log("binary changed — checkpointing and reloading");
                            let snapshot = collect_snapshot(&slots);
                            state_store.checkpoint(&snapshot);

                            // Flush stdout before exec to avoid losing buffered output
                            use std::io::Write;
                            std::io::stdout().flush().ok();

                            // exec self via std::os::unix
                            use std::os::unix::process::CommandExt;
                            let args: Vec<String> = std::env::args().collect();
                            let err = std::process::Command::new(&args[0])
                                .args(&args[1..])
                                .exec();
                            // exec() only returns on failure
                            emit::log(&format!("self-reload failed: {}", err));
                        }
                    }
                }
            }
            last_reload_check = Instant::now();
        }

        let next = match queue.peek() {
            Some(s) => s.fire_at,
            None => break,
        };

        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }

        let mut ready_indices = Vec::new();

        while let Some(scheduled) = queue.peek() {
            if scheduled.fire_at > Instant::now() {
                break;
            }
            let scheduled = queue.pop().unwrap();
            let i = scheduled.index;

            let changed = slots[i].poll(&focus);

            if changed {
                governor.record_event();
            }

            // Only log when something changed — quiet polls are silent
            if changed {
                emit::log(&format!(
                    "{}: change detected (interval={:.1}s, accum={:.1}, events={})",
                    slots[i].name(),
                    slots[i].interval.current.as_secs_f64(),
                    slots[i].accumulator.magnitude,
                    slots[i].accumulator.event_count,
                ));
            }

            if slots[i].ready_to_disclose() {
                ready_indices.push(i);
            }

            slots[i].schedule_next();
            queue.push(ScheduledSensor { fire_at: slots[i].next_fire, index: i });
        }

        // Batch disclosure
        if !ready_indices.is_empty() && governor.can_disclose() {
            let mut batch = Vec::new();

            for &i in &ready_indices {
                let slot = &slots[i];
                let priority = if slot.accumulator.magnitude >= 5.0 {
                    "high"
                } else if slot.accumulator.magnitude >= 3.0 {
                    "medium"
                } else {
                    "low"
                };

                batch.push((
                    slot.name().to_string(),
                    priority.to_string(),
                    slot.accumulator.summary(),
                ));
            }

            emit::log(&format!(
                "disclosing batch of {} sensors (cooldown was {:.1}s)",
                batch.len(), governor.cooldown().as_secs_f64(),
            ));
            emit::emit_batch(&batch);
            governor.record_disclosure();

            for &i in &ready_indices {
                slots[i].accumulator.reset();
            }
        } else if !ready_indices.is_empty() {
            emit::log(&format!(
                "{} sensors ready but governor holding ({}/{} in window)",
                ready_indices.len(),
                governor.window_disclosures,
                governor.max_disclosures_per_window,
            ));
        }

        // Periodic checkpoint
        if last_checkpoint.elapsed() >= checkpoint_interval {
            let snapshot = collect_snapshot(&slots);
            state_store.checkpoint(&snapshot);
            last_checkpoint = Instant::now();
        }
    }
}

fn collect_snapshot(slots: &[sensors::SensorSlot]) -> state::StateSnapshot {
    let mut snapshot = state::StateSnapshot::default();
    for slot in slots {
        for (key, value) in slot.export_state() {
            match key.as_str() {
                "seen_signal" => { snapshot.seen_signals.insert(value); }
                "disclosed_threshold" => {
                    if let Ok(t) = value.parse::<u8>() {
                        snapshot.disclosed_thresholds.push(t);
                    }
                }
                "reply_hint_shown" => {
                    snapshot.reply_hint_shown = value == "true";
                }
                "context_pct" => {
                    snapshot.context_pct = value.parse().ok();
                }
                _ => {}
            }
        }
    }
    snapshot
}

fn cmd_inbox() {
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_session_id = own_session_id().unwrap_or_default();

    // Scan same dirs as the peer sensor: own project + broadcast + focus group
    let own_encoded = encode_project(&cwd);
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    let focus_file = base.join("focus");
    if let Ok(content) = std::fs::read_to_string(&focus_file) {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() {
                scan_dirs.push(base.join(encode_project(line)));
            }
        }
    }

    // Collect all messages with mtime for chronological ordering
    struct InboxEntry {
        mtime: std::time::SystemTime,
        scope: String,
        sender: String,
        message: String,
        source: String,
    }
    let mut entries: Vec<InboxEntry> = Vec::new();

    for dir in &scan_dirs {
        let dir_entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let dir_name = dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let scope = if dir_name == "_broadcast" { "broadcast" }
            else if dir_name == own_encoded { "project" }
            else { "focus" };

        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("signal") {
                continue;
            }

            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let content = content.trim().to_string();
            let parts: Vec<&str> = content.splitn(4, '|').collect();
            if parts.len() != 4 { continue; }

            let from = parts[0];
            let project = parts[1];
            let source_cwd = parts[2];
            let message = parts[3];

            // Skip own messages
            if let Some((_, identity)) = from.split_once(':') {
                if identity == own_session_id { continue; }
            }

            let (kind, identity) = from.split_once(':').unwrap_or(("unknown", from));
            let sender = match kind {
                "claude" => format!("claude/{}", source_cwd),
                "external" => identity.to_string(),
                _ => format!("{} ({})", project, from),
            };

            entries.push(InboxEntry {
                mtime,
                scope: scope.to_string(),
                sender,
                message: message.to_string(),
                source: source_cwd.to_string(),
            });
        }
    }

    // Sort chronologically — oldest first (ledger order)
    entries.sort_by_key(|e| e.mtime);

    if entries.is_empty() {
        println!("no messages");
    } else {
        let mut t = agent_fmt::Table::new(&["Scope", "From", "Message", "Source"]);
        t.max_width(0, 10);
        t.max_width(1, 24);
        for entry in &entries {
            t.add(vec![&entry.scope, &entry.sender, &entry.message, &entry.source]);
        }
        t.print();
        println!("  {} message(s)", entries.len());
    }
}

fn cmd_peers() {
    let sensor = PeerSensor::new();
    let peers = sensor.list_peers();

    if peers.is_empty() {
        println!("no active peer sessions");
        return;
    }

    let mut t = agent_fmt::Table::new(&["Project", "Path", "Status", "Context"]);
    t.max_width(0, 20);
    for (cwd, project, status, ctx) in &peers {
        t.add(vec![project, cwd, status, &format!("{ctx:.0}%")]);
    }
    t.print();
    println!("  {} peer(s)", peers.len());
}

fn cmd_send(args: &[String]) {
    // Parse flags: --broadcast, --to <project-path>
    let mut broadcast = false;
    let mut target_dir: Option<String> = None;
    let mut message_parts: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--broadcast" => broadcast = true,
            "--to" => {
                i += 1;
                if i < args.len() {
                    target_dir = Some(args[i].clone());
                } else {
                    eprintln!("attend send: --to requires a project path");
                    std::process::exit(1);
                }
            }
            _ => message_parts.push(&args[i]),
        }
        i += 1;
    }

    let message = message_parts.join(" ");
    if message.is_empty() {
        eprintln!("usage: attend send [--broadcast] [--to <path>] <message>");
        eprintln!("  tip: wrap message in double quotes to avoid shell expansion");
        std::process::exit(1);
    }

    // Fence: detect probable shell glob expansion.
    // If any message part is an existing file path, the shell likely
    // expanded a metachar (e.g. zsh expanded "hello?" into filenames).
    let suspect_expansion = message_parts.iter().any(|part| {
        std::path::Path::new(part).exists() && !part.contains(' ')
    });
    if suspect_expansion {
        eprintln!("[attend] warning: message contains existing file paths — shell may have expanded metacharacters");
        eprintln!("[attend] did you mean: attend send \"{}\"", message);
        eprintln!("[attend] sending anyway, but wrap in quotes next time");
    }

    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Validate --to path against active peers
    if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());

        let sensor = PeerSensor::new();
        let peers = sensor.list_peers();
        let peer_paths: Vec<&str> = peers.iter().map(|(cwd, _, _, _)| cwd.as_str()).collect();

        if !peer_paths.contains(&resolved.as_str()) {
            eprintln!("error: no active peer at {}", resolved);
            if peers.is_empty() {
                eprintln!("\nno active peer sessions found");
            } else {
                eprintln!("\nactive peers:");
                for (peer_cwd, project, _, _) in &peers {
                    eprintln!("  {} ({})", peer_cwd, project);
                }
                // Fuzzy suggest: find closest match by path suffix
                if let Some(suggestion) = find_closest_peer(&resolved, &peer_paths) {
                    eprintln!("\ndid you mean: {}?", suggestion);
                }
            }
            std::process::exit(1);
        }
    }

    // Determine target directories based on current mode:
    // --broadcast: _broadcast only (reaches everyone)
    // --to <path>: specific project only
    // default: own project + focus group (mirrors what we read)
    let dest_dirs: Vec<std::path::PathBuf> = if broadcast {
        vec![base.join("_broadcast")]
    } else if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        vec![base.join(encode_project(&resolved))]
    } else {
        // Default: send to own project + all focus group peers
        let mut dirs = vec![base.join(encode_project(&cwd))];
        let focus_file = base.join("focus");
        if let Ok(content) = std::fs::read_to_string(&focus_file) {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    dirs.push(base.join(encode_project(line)));
                }
            }
        }
        dirs
    };

    let (sender_id, source_kind) = identify_sender();
    let project = cwd.rsplit('/').next().unwrap_or("?");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let from = format!("{}:{}", source_kind, sender_id);
    let filename = format!("{}-{}.signal", sender_id.replace('/', "-"), ts);
    let content = format!("{}|{}|{}|{}\n", from, project, cwd, message);

    let scope = if broadcast { "broadcast" }
        else if target_dir.is_some() { "directed" }
        else if dest_dirs.len() > 1 { "focus group" }
        else { "project" };

    for dest_dir in &dest_dirs {
        std::fs::create_dir_all(dest_dir).ok();
        let path = dest_dir.join(&filename);
        let tmp_path = dest_dir.join(format!("{}.tmp", filename));

        match std::fs::write(&tmp_path, &content) {
            Ok(_) => {
                if let Err(e) = std::fs::rename(&tmp_path, &path) {
                    eprintln!("[attend] error renaming signal: {}", e);
                    std::fs::remove_file(&tmp_path).ok();
                }
            }
            Err(e) => eprintln!("[attend] error writing signal to {}: {}", dest_dir.display(), e),
        }
    }

    eprintln!("[attend] signal written ({}, {} dirs): {}", scope, dest_dirs.len(), filename);
}

fn cmd_status() {
    // Check if attend run is already active
    let output = std::process::Command::new("ps")
        .args(["--no-headers", "-eo", "pid,args"])
        .output()
        .ok();

    let mut found = false;
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let own_pid = std::process::id();
        for line in stdout.lines() {
            let line = line.trim();
            if line.contains("attend run") && !line.contains(&own_pid.to_string()) {
                println!("{}", line);
                found = true;
            }
        }
    }

    if !found {
        println!("no attend instances running");
    }

    // Show signals
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_dir = base.join(encode_project(&cwd));
    let broadcast_dir = base.join("_broadcast");

    let own_count = count_signals(&own_dir);
    let broadcast_count = count_signals(&broadcast_dir);

    {
        let mut t = agent_fmt::Table::new(&["Signals", "Count", "Path"]);
        t.align(1, agent_fmt::Align::Right);
        t.max_width(1, 6);
        t.add(vec!["project", &own_count.to_string(), &own_dir.display().to_string()]);
        t.add(vec!["broadcast", &broadcast_count.to_string(), &broadcast_dir.display().to_string()]);
        t.print();
    }

    // Show focus file if it exists
    let focus_file = base.join("focus");
    if focus_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&focus_file) {
            let peers: Vec<&str> = content.lines()
                .filter(|l| !l.trim().is_empty())
                .collect();
            let mut t = agent_fmt::Table::new(&["Focus", "Path"]);
            for p in &peers {
                t.add(vec!["peer", p]);
            }
            if peers.is_empty() {
                t.add(vec!["(none)", ""]);
            }
            t.print();
        }
    } else {
        println!("  focus: project only (no focus file)");
    }
}

fn count_signals(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("signal"))
            .count())
        .unwrap_or(0)
}

// --- Helpers ---

/// Determine who is sending this signal.
/// Returns (identity_string, source_kind) where source_kind is "claude" or "external".
fn identify_sender() -> (String, &'static str) {
    // First, try to find a Claude session ID (we're inside a Claude session)
    if let Some(sid) = own_session_id() {
        return (sid, "claude");
    }

    // Not inside Claude — build identity from environment
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    // Detect terminal: check common terminal-specific env vars
    let terminal = detect_terminal();

    let identity = if !terminal.is_empty() {
        format!("{}@{}", user, terminal)
    } else {
        user
    };

    (identity, "external")
}

/// Best-effort terminal detection from environment variables.
fn detect_terminal() -> String {
    // Specific terminal emulators set their own env vars
    if std::env::var("KITTY_PID").is_ok() {
        return "kitty".to_string();
    }
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return "alacritty".to_string();
    }
    if std::env::var("WEZTERM_PANE").is_ok() {
        return "wezterm".to_string();
    }
    if std::env::var("TMUX").is_ok() {
        return "tmux".to_string();
    }
    if std::env::var("STY").is_ok() {
        return "screen".to_string();
    }
    // TERM_PROGRAM is set by some terminals (macOS Terminal, iTerm2, VS Code)
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return tp.to_lowercase();
    }
    // SSH session
    if std::env::var("SSH_CONNECTION").is_ok() {
        return "ssh".to_string();
    }
    // Fallback: try TERMINAL or just use the shell
    if let Ok(t) = std::env::var("TERMINAL") {
        return t.rsplit('/').next().unwrap_or(&t).to_string();
    }
    String::new()
}

/// Find the closest matching peer path by comparing path suffixes.
fn find_closest_peer<'a>(target: &str, peers: &[&'a str]) -> Option<&'a str> {
    // Try matching the last N segments of the target against peer paths
    let target_parts: Vec<&str> = target.rsplit('/').collect();
    let mut best: Option<(&str, usize)> = None;

    for peer in peers {
        let peer_parts: Vec<&str> = peer.rsplit('/').collect();
        let common = target_parts.iter().zip(peer_parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common > 0 && (best.is_none() || common > best.unwrap().1) {
            best = Some((peer, common));
        }
    }

    best.map(|(p, _)| p)
}

fn signals_base() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("signals")
}

/// Encode a project path the same way Claude Code does: '/', '_', '.' → '-'
fn encode_project(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

/// Delegate to the shared implementation in the peer sensor module.
fn own_session_id() -> Option<String> {
    sensors::find_own_session_id(std::process::id())
}

// --- Entry point ---

fn cmd_focus(args: &[String]) {
    let focus_file = signals_base().join("focus");

    match args.first().map(|s| s.as_str()) {
        Some("add") => {
            // attend focus add /path/to/project [/path/to/other ...]
            if args.len() < 2 {
                eprintln!("usage: attend focus add <project-path> [...]");
                std::process::exit(1);
            }
            let mut existing = read_focus_list(&focus_file);
            for path in &args[1..] {
                let resolved = std::fs::canonicalize(path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.clone());
                if !existing.contains(&resolved) {
                    existing.push(resolved.clone());
                    eprintln!("[attend] focus: added {}", resolved);
                }
            }
            write_focus_list(&focus_file, &existing);
        }
        Some("remove") | Some("rm") => {
            if args.len() < 2 {
                eprintln!("usage: attend focus remove <project-path> [...]");
                std::process::exit(1);
            }
            let mut existing = read_focus_list(&focus_file);
            for path in &args[1..] {
                let resolved = std::fs::canonicalize(path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.clone());
                existing.retain(|p| p != &resolved);
                eprintln!("[attend] focus: removed {}", resolved);
            }
            write_focus_list(&focus_file, &existing);
        }
        Some("clear") => {
            std::fs::remove_file(&focus_file).ok();
            eprintln!("[attend] focus: cleared (project-only mode)");
        }
        Some("list") | None => {
            let list = read_focus_list(&focus_file);
            if list.is_empty() {
                println!("focus: project only (no peers in focus group)");
            } else {
                println!("focus: {} peers", list.len());
                for p in &list {
                    let name = p.rsplit('/').next().unwrap_or(p);
                    println!("  {} ({})", name, encode_project(p));
                }
            }
        }
        Some(unknown) => {
            eprintln!("attend focus: unknown subcommand '{}' — try add, remove, clear, list", unknown);
            std::process::exit(1);
        }
    }
}

fn read_focus_list(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect()
}

fn write_focus_list(path: &std::path::Path, list: &[String]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let content = list.join("\n") + "\n";
    std::fs::write(path, content).ok();
}

// --- Entry point ---

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("run") => {
            let catchup = args.iter().any(|a| a == "--catchup");
            cmd_run_with_catchup(catchup);
        }
        Some("peers") => cmd_peers(),
        Some("inbox") => cmd_inbox(),
        Some("status") => cmd_status(),
        Some("send") => {
            cmd_send(&args[1..]);
        }
        Some("focus") => {
            cmd_focus(&args[1..]);
        }
        Some("config") => {
            match args.get(1).map(|s| s.as_str()) {
                Some("init") => {
                    let path = config::Config::init_user_config();
                    println!("wrote default config to {}", path.display());
                }
                Some("show") | None => {
                    let focus = Focus::default_focus();
                    let cfg = config::Config::load(&focus.working_dir);
                    println!("{:#?}", cfg);
                }
                Some("path") => {
                    let home = std::env::var("XDG_CONFIG_HOME")
                        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
                    println!("user:    {}/attend/config.yaml", home);
                    let cwd = std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    println!("project: {}/.claude/attend.yaml", cwd);
                }
                Some(sub) => {
                    eprintln!("attend config: unknown subcommand '{}' — try init, show, path", sub);
                    std::process::exit(1);
                }
            }
        }
        Some("--version") | Some("-V") => {
            println!("attend {} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"));
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            agent_fmt::Banner::new("ATTEND")
                .subtitle("active awareness for Claude Code sessions")
                .gradient(&agent_fmt::GRADIENT_TEAL)
                .print();
            println!("usage: attend <command>\n");
            agent_fmt::print_commands("commands", &[
                ("run",    "Start the sensor loop (use with Monitor for async delivery)"),
                ("peers",  "List active Claude Code sessions"),
                ("inbox",  "Read pending messages from peers"),
                ("send",   "Send a signal to peer sessions"),
                ("focus",  "Manage focus group (add/remove/clear/list peer projects)"),
                ("config", "Manage configuration (init/show/path)"),
                ("status", "Show running instances, signals, and focus state"),
                ("help",   "Show this help"),
            ]);
            println!();
            agent_fmt::print_commands("send flags", &[
                ("--broadcast", "Send to all projects, not just your own"),
                ("--to <path>", "Send to a specific project's signals dir"),
            ]);
        }
        Some(unknown) => {
            eprintln!("attend: unknown command '{}' — try 'attend help'", unknown);
            std::process::exit(1);
        }
    }
}
