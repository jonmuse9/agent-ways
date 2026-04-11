mod config;
mod groups;
mod scenes;
mod state;
mod emit;
mod sensors;

use sensors::Focus;
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

    // Initialize rooms for signal routing (ADR-118)
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    let group_mgr = groups::Groups::new(&signals_base(), &session_id);

    // Self-documenting startup
    let my_groups = group_mgr.my_groups();
    let focus_desc = if my_groups.is_empty() {
        "project only".to_string()
    } else {
        let names: Vec<&str> = my_groups.iter().map(|(n, _)| n.as_str()).collect();
        format!("project + {}", names.join(", "))
    };

    // Register sensors from config + feature flags

    let (mut slots, enabled_names) = sensors::register_sensors(&cfg, &focus, catchup, &group_mgr);

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
    let r = get_groups();
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    // Add focus group dirs
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
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
    let r = get_groups();

    #[cfg(feature = "sensor-peers")]
    let peers = {
        let sensor = sensors::PeerSensor::new();
        sensor.list_peers()
    };
    #[cfg(not(feature = "sensor-peers"))]
    let peers: Vec<(String, String, String, f64)> = Vec::new();

    let my_focus = r.my_groups();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 24);

    // Show self in project group
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let self_project = cwd.rsplit('/').next().unwrap_or("?");
    t.add(vec!["(project)", self_project, "working", ""]);

    // Show named groups we're focused on
    for (name, pinned) in &my_focus {
        let pin_marker = if *pinned { " (pinned)" } else { "" };
        let label = format!("{name}{pin_marker}");
        t.add(vec![&label, "(you)", "", ""]);
    }

    // Show peers
    if !peers.is_empty() {
        t.add(vec!["", "", "", ""]);
        for (peer_cwd, project, status, ctx) in &peers {
            let focus_label = if *peer_cwd == cwd {
                "(project)".to_string()
            } else {
                String::new()
            };
            t.add(vec![&focus_label, project, status, &format!("{ctx:.0}%")]);
        }
    }

    t.print();

    let focus_count = my_focus.len();
    let peer_count = peers.len();
    println!(
        "  {} agent(s), {} focus group(s)",
        peer_count + 1,
        focus_count + 1
    );
}

fn cmd_send(args: &[String]) {
    // Parse flags: --broadcast, --to <project-path>, --focus <name>
    let mut broadcast = false;
    let mut target_dir: Option<String> = None;
    let mut target_focus: Option<String> = None;
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
            "--focus" => {
                i += 1;
                if i < args.len() {
                    target_focus = Some(args[i].clone());
                } else {
                    eprintln!("attend send: --focus requires a focus group name");
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

        #[cfg(feature = "sensor-peers")]
        let peers = {
            let sensor = sensors::PeerSensor::new();
            sensor.list_peers()
        };
        #[cfg(not(feature = "sensor-peers"))]
        let peers: Vec<(String, String, String, String)> = Vec::new();
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
    let r = get_groups();
    let dest_dirs: Vec<std::path::PathBuf> = if broadcast {
        vec![base.join("_broadcast")]
    } else if let Some(ref focus_name) = target_focus {
        vec![r.group_dir(focus_name)]
    } else if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        vec![base.join(encode_project(&resolved))]
    } else {
        // Default: send to own project + joined rooms + focus group peers
        let mut dirs = vec![base.join(encode_project(&cwd))];
        // Focus groups (named signal namespaces)
        for name in r.joined_group_names() {
            dirs.push(r.group_dir(&name));
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
        else if target_focus.is_some() { "focus" }
        else if target_dir.is_some() { "directed" }
        else if dest_dirs.len() > 1 { "focus" }
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

    let mut instances: Vec<(String, String)> = Vec::new(); // (pid, info)
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let own_pid = std::process::id();
        for line in stdout.lines() {
            let line = line.trim();
            // Only match actual attend binary, not shell wrappers that contain "attend run"
            if !line.contains("attend run") || line.contains(&own_pid.to_string()) {
                continue;
            }
            // Skip zsh/bash wrapper lines (contain shell-snapshots or eval)
            if line.contains("shell-snapshots") || line.contains("eval '") {
                continue;
            }
            // Extract PID and show clean output
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                instances.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
            }
        }
    }

    // Gather all data before building a single unified table
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_dir = base.join(encode_project(&cwd));
    let broadcast_dir = base.join("_broadcast");
    let own_count = count_signals(&own_dir);
    let broadcast_count = count_signals(&broadcast_dir);

    let r = get_groups();
    let my_focus = r.my_groups();

    // Single table: Section | Detail | Info
    let mut t = agent_fmt::Table::new(&["", "Detail", "Info"]);
    t.align(0, agent_fmt::Align::Left);

    // ── Instances section
    if instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        for (i, (pid, cmd)) in instances.iter().enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            t.add(vec![label, &format!("PID {pid}"), cmd]);
        }
    }

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Signals section
    t.add(vec!["signals", "project", &format!("{own_count} pending")]);
    t.add(vec!["", "broadcast", &format!("{broadcast_count} pending")]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Focus section
    if my_focus.is_empty() {
        t.add(vec!["focus", "project only", ""]);
    } else {
        for (i, (name, pinned)) in my_focus.iter().enumerate() {
            let label = if i == 0 { "focus" } else { "" };
            let pin = if *pinned { " (pinned)" } else { "" };
            let info = format!("{name}{pin}");
            t.add(vec![label, &info, ""]);
        }
    }

    t.print();
}

fn display_config(cfg: &config::Config) {
    // Governor section
    let mut t = agent_fmt::Table::new(&["", "Setting", "Value"]);
    t.align(0, agent_fmt::Align::Left);

    t.add(vec![
        "governor",
        "base_cooldown",
        &format!("{}s", cfg.governor.base_cooldown.as_secs()),
    ]);
    t.add(vec![
        "",
        "max_per_window",
        &cfg.governor.max_per_window.to_string(),
    ]);
    t.add(vec![
        "",
        "rate_window",
        &format!("{}s", cfg.governor.rate_window.as_secs()),
    ]);

    t.add(vec!["", "", ""]);

    // Sensors — sorted by name
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();

    for (i, name) in names.iter().enumerate() {
        let sc = &cfg.sensors[*name];
        let sensor_type = if sc.script.is_some() { "script" } else { "crate" };
        let enabled = if sc.enabled { "" } else { " (disabled)" };
        let label = format!("{name}{enabled}");

        let section = if i == 0 { "sensors" } else { "" };
        t.add(vec![
            section,
            &label,
            &format!(
                "[{sensor_type}] interval={}s min={}s threshold={} decay={}",
                sc.interval.as_secs(),
                sc.min_interval.as_secs(),
                sc.threshold,
                sc.decay_threshold,
            ),
        ]);

        if let Some(ref script) = sc.script {
            t.add(vec!["", "", &format!("script: {script}")]);
        }
        if !sc.requires.is_empty() {
            t.add(vec!["", "", &format!("requires: [{}]", sc.requires.join(", "))]);
        }
    }

    t.print();
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
    #[cfg(feature = "sensor-peers")]
    { sensors::find_own_session_id(std::process::id()) }
    #[cfg(not(feature = "sensor-peers"))]
    { None }
}

// --- Entry point ---

fn cmd_scene(args: &[String]) {
    let name = match args.first() {
        Some(n) => n,
        None => {
            eprintln!("usage: attend scene <name>");
            eprintln!("  try: attend scenes (to list available)");
            std::process::exit(1);
        }
    };

    let r = get_groups();
    match scenes::activate(name, &r) {
        Ok(result) => println!("[attend] scene '{name}': {result}"),
        Err(e) => {
            eprintln!("[attend] scene: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_scenes() {
    let all = scenes::load_scenes();
    let mut names: Vec<&String> = all.keys().collect();
    names.sort();

    let mut t = agent_fmt::Table::new(&["Scene", "Focus groups"]);
    for name in &names {
        let scene = &all[*name];
        let groups_str = if scene.rooms.is_empty() {
            "(none — project only)".to_string()
        } else {
            scene.rooms.join(", ")
        };
        t.add(vec![name.as_str(), &groups_str]);
    }
    t.print();
}

fn get_groups() -> groups::Groups {
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    groups::Groups::new(&signals_base(), &session_id)
}

fn cmd_focus_new(args: &[String]) {
    let r = get_groups();

    match args.first().map(|s| s.as_str()) {
        Some("on") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus on <name> [--pin]");
                    std::process::exit(1);
                }
            };
            let pin = args.iter().any(|a| a == "--pin");
            match r.join(name, pin) {
                Ok(()) => {
                    let suffix = if pin { " (pinned)" } else { "" };
                    println!("[attend] focus: attending to {name}{suffix}");
                }
                Err(e) => {
                    eprintln!("[attend] focus: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some("off") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus off <name>");
                    std::process::exit(1);
                }
            };
            r.leave(name).ok();
            println!("[attend] focus: released {name}");
        }
        Some("clear") => {
            for (name, _) in r.my_groups() {
                r.leave(&name).ok();
            }
            println!("[attend] focus: cleared (project only)");
        }
        Some("pin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus pin <name>");
                    std::process::exit(1);
                }
            };
            r.pin(name);
            println!("[attend] focus: pinned {name}");
        }
        Some("unpin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus unpin <name>");
                    std::process::exit(1);
                }
            };
            r.unpin(name);
            println!("[attend] focus: unpinned {name}");
        }
        Some("dissolve") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus dissolve <name>");
                    std::process::exit(1);
                }
            };
            let members = r.dissolve(name);
            if members.is_empty() {
                println!("[attend] focus: dissolved {name} (was empty)");
            } else {
                println!("[attend] focus: dissolved {name} ({} members released)", members.len());
            }
        }
        Some("all") => {
            r.cleanup_stale();
            let all = r.all_groups();
            if all.is_empty() {
                println!("no active focus groups");
                return;
            }
            let mut t = agent_fmt::Table::new(&["Focus", "Members", "Pinned"]);
            t.align(1, agent_fmt::Align::Right);
            for (name, count, pinned) in &all {
                t.add(vec![
                    name.as_str(),
                    &count.to_string(),
                    if *pinned { "yes" } else { "no" },
                ]);
            }
            t.print();
        }
        Some("list") | None => {
            let my = r.my_groups();
            if my.is_empty() {
                println!("focus: project only");
            } else {
                let mut t = agent_fmt::Table::new(&["Focus", "Pinned"]);
                for (name, pinned) in &my {
                    t.add(vec![name.as_str(), if *pinned { "yes" } else { "no" }]);
                }
                t.print();
            }
        }
        Some(unknown) => {
            eprintln!("attend focus: unknown subcommand '{unknown}' — try on, off, list, all, clear, pin, unpin, dissolve");
            std::process::exit(1);
        }
    }
}

// --- Permissions audit (ADR-116) ---

fn cmd_permissions_audit() {
    use agent_fmt::permissions;

    let focus = Focus::default_focus();
    let cfg = config::Config::load(&focus.working_dir);

    // Load settings.json grants
    let home = std::env::var("HOME").unwrap_or_default();
    let settings_path = std::path::PathBuf::from(&home).join(".claude/settings.json");
    let grants = permissions::load_settings_permissions(&settings_path);

    if grants.is_empty() {
        eprintln!("Warning: no permissions found in {}", settings_path.display());
    }

    // Collect (sensor_name, requires) pairs from config
    let mut requirements: Vec<(String, Vec<String>)> = Vec::new();
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();
    for name in names {
        let sensor = &cfg.sensors[name];
        if !sensor.requires.is_empty() {
            let prefix = if sensor.script.is_some() { "+" } else { "" };
            requirements.push((
                format!("{prefix}{name}"),
                sensor.requires.clone(),
            ));
        }
    }

    let results = permissions::audit(&requirements, &grants);
    permissions::display_audit("Attend Permissions Audit", "Sensor", &results, false);
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
            cmd_focus_new(&args[1..]);
        }
        Some("scene") => {
            cmd_scene(&args[1..]);
        }
        Some("scenes") => {
            cmd_scenes();
        }
        Some("permissions") => {
            match args.get(1).map(|s| s.as_str()) {
                Some("audit") | None => cmd_permissions_audit(),
                Some(sub) => {
                    eprintln!("attend permissions: unknown subcommand '{}' — try audit", sub);
                    std::process::exit(1);
                }
            }
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
                    display_config(&cfg);
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
            let version = format!("v{} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"));
            agent_fmt::Banner::new("ATTEND")
                .subtitle("active awareness for Claude Code sessions")
                .version(&version)
                .gradient(&agent_fmt::GRADIENT_TEAL)
                .print();
            println!("usage: attend <command>\n");
            agent_fmt::print_commands("commands", &[
                ("run",         "Start the sensor loop (use with Monitor for async delivery)"),
                ("peers",       "List active Claude Code sessions and focus groups"),
                ("inbox",       "Read pending messages from peers"),
                ("send",        "Send a signal to peer sessions"),
                ("focus",       "Manage attention groups (on, off, list, all, clear, pin, dissolve)"),
                ("scene",       "Activate a named scene (reconfigure focus)"),
                ("scenes",      "List available scenes"),
                ("config",      "Manage configuration (init/show/path)"),
                ("permissions", "Audit sensor permissions against settings.json"),
                ("status",      "Show running instances, signals, and focus state"),
                ("help",        "Show this help"),
            ]);
            println!();
            agent_fmt::print_commands("send flags", &[
                ("--focus <name>", "Send to a focus group"),
                ("--broadcast",   "Send to all agents"),
                ("--to <path>",   "Send to a specific project path (legacy)"),
            ]);
        }
        Some(unknown) => {
            eprintln!("attend: unknown command '{}' — try 'attend help'", unknown);
            std::process::exit(1);
        }
    }
}
