mod tick;
mod delta;
mod emit;
mod sensors;

use sensors::{Focus, GitSensor, PeerSensor, ProcessSensor, Sensor, SensorSlot};
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

fn cmd_run() {
    emit::log("starting attend");

    let focus = Focus::default_focus();
    emit::log(&format!("focus: {} ({})", focus.description, focus.working_dir));

    let mut slots: Vec<SensorSlot> = vec![
        SensorSlot::new(Box::new(ProcessSensor::new())),
        SensorSlot::new(Box::new(GitSensor::new())),
        SensorSlot::new(Box::new(PeerSensor::new())),
    ];

    let mut governor = DisclosureGovernor::new(
        Duration::from_secs(15),
        3,
        Duration::from_secs(120),
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, slot) in slots.iter().enumerate() {
        queue.push(ScheduledSensor { fire_at: slot.next_fire, index: i });
    }

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

    loop {
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

            emit::log(&format!(
                "{}: poll interval={:.1}s changed={} accum={:.1} events={}",
                slots[i].name(),
                slots[i].interval.current.as_secs_f64(),
                changed,
                slots[i].accumulator.magnitude,
                slots[i].accumulator.event_count,
            ));

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
    }
}

fn cmd_peers() {
    let focus = Focus::default_focus();
    let mut sensor = PeerSensor::new();
    let observations = sensor.poll(&focus);
    // First poll returns empty (baseline). The baseline info goes to stderr.
    // Poll again to get the actual state — but since nothing changed, just
    // show the baseline that was printed to stderr.
    if observations.is_empty() {
        // Baseline was printed to stderr by the sensor. For the CLI, also
        // list what we found by doing a second poll (which will show no deltas).
        let _ = sensor.poll(&focus);
    }
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
        std::process::exit(1);
    }

    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

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

    println!("signals:");
    println!("  project:   {} pending ({})", own_count, own_dir.display());
    println!("  broadcast: {} pending ({})", broadcast_count, broadcast_dir.display());

    // Show focus file if it exists
    let focus_file = base.join("focus");
    if focus_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&focus_file) {
            let peers: Vec<&str> = content.lines()
                .filter(|l| !l.trim().is_empty())
                .collect();
            println!("focus: {} peers", peers.len());
            for p in &peers {
                println!("  {}", p);
            }
        }
    } else {
        println!("focus: project only (no focus file)");
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

/// Try to find our own session ID by walking up the process tree to find
/// the claude parent, then matching it against ~/.claude/sessions/*.json
fn own_session_id() -> Option<String> {
    let own_pid = std::process::id();
    // Walk up to find claude PID
    let mut pid = own_pid;
    let mut claude_pid = None;
    for _ in 0..10 {
        if pid <= 1 { break; }
        let output = std::process::Command::new("ps")
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

    // Scan session files for matching PID
    let home = std::env::var("HOME").ok()?;
    let sessions_dir = std::path::PathBuf::from(&home).join(".claude").join("sessions");
    for entry in std::fs::read_dir(&sessions_dir).ok()?.flatten() {
        let content = std::fs::read_to_string(entry.path()).ok()?;
        // Quick check for PID match
        let pid_pattern = format!("\"pid\":{}", claude_pid);
        if content.contains(&pid_pattern) {
            // Extract session ID
            if let Some(start) = content.find("\"sessionId\":\"") {
                let rest = &content[start + 14..];
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
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
        Some("run") => cmd_run(),
        Some("peers") => cmd_peers(),
        Some("status") => cmd_status(),
        Some("send") => {
            cmd_send(&args[1..]);
        }
        Some("focus") => {
            cmd_focus(&args[1..]);
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            println!("\x1b[2m\x1b[4mA G E N T\x1b[0m\n");
            println!("\x1b[38;5;73m █████╗ ████████╗████████╗███████╗███╗   ██╗██████╗ \x1b[0m");
            println!("\x1b[38;5;79m██╔══██╗╚══██╔══╝╚══██╔══╝██╔════╝████╗  ██║██╔══██╗\x1b[0m");
            println!("\x1b[38;5;80m███████║   ██║      ██║   █████╗  ██╔██╗ ██║██║  ██║\x1b[0m");
            println!("\x1b[38;5;116m██╔══██║   ██║      ██║   ██╔══╝  ██║╚██╗██║██║  ██║\x1b[0m");
            println!("\x1b[38;5;109m██║  ██║   ██║      ██║   ███████╗██║ ╚████║██████╔╝\x1b[0m");
            println!("\x1b[38;5;66m╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚══════╝╚═╝  ╚═══╝╚═════╝ \x1b[0m");
            println!();
            println!("  \x1b[2mactive awareness for Claude Code sessions\x1b[0m\n");
            println!("usage: attend <command>\n");
            println!("commands:");
            println!("  run       Start the sensor loop (use with Monitor for async delivery)");
            println!("  peers     List active Claude Code sessions");
            println!("  send      Send a signal to peer sessions");
            println!("  focus     Manage focus group (add/remove/clear/list peer projects)");
            println!("  status    Show running instances, signals, and focus state");
            println!("  help      Show this help");
            println!();
            println!("send flags:");
            println!("  --broadcast     Send to all projects, not just your own");
            println!("  --to <path>     Send to a specific project's signals dir");
        }
        Some(unknown) => {
            eprintln!("attend: unknown command '{}' — try 'attend help'", unknown);
            std::process::exit(1);
        }
    }
}
