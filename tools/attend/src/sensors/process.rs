use super::{Focus, Sensor};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

/// Watches user session processes. Detects new/exited processes and
/// activity changes. Filters through focus to determine relevance.
pub struct ProcessSensor {
    /// Previous snapshot: app name → instance count
    prior: HashMap<String, u32>,
    /// First poll establishes baseline silently
    baseline_established: bool,
}

impl ProcessSensor {
    pub fn new() -> Self {
        Self {
            prior: HashMap::new(),
            baseline_established: false,
        }
    }

    /// Snapshot the user's processes as app-name → instance-count.
    /// Tracks applications, not PIDs. Chrome going from 47 to 49
    /// renderer processes is not a state change. Chrome appearing
    /// or disappearing entirely is.
    fn snapshot(&self, focus: &Focus) -> HashMap<String, u32> {
        let mut apps: HashMap<String, u32> = HashMap::new();

        let output = Command::new("ps")
            .args(["--no-headers", "-u", &whoami(), "-o", "comm"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return apps,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let comm = line.trim().to_string();
            if !comm.is_empty() && Self::is_relevant(&comm, focus) {
                *apps.entry(comm).or_insert(0) += 1;
            }
        }

        apps
    }

    fn is_relevant(comm: &str, focus: &Focus) -> bool {
        // Filter out ambient noise: shells, coreutils, attend itself
        let noise = ["ps", "bash", "zsh", "sh", "fish", "attend",
                      "grep", "awk", "sed", "cat", "sleep", "tail", "head",
                      "wc", "sort", "uniq", "cut", "tr", "tee", "xargs",
                      "less", "more", "find", "ls", "cp", "mv", "rm",
                      "mkdir", "chmod", "chown", "date", "env",
                      "dbus-daemon", "dbus-broker", "systemd", "pipewire",
                      "pulseaudio", "wireplumber", "xdg-dbus-proxy",
                      "at-spi-bus-laun", "at-spi2-registr",
                      "gjs", "gsd-*", "gnome-*", "xdg-*"];
        if noise.iter().any(|n| {
            if let Some(prefix) = n.strip_suffix('*') {
                comm.starts_with(prefix)
            } else {
                comm == *n
            }
        }) {
            return false;
        }

        // Focus keywords boost relevance
        if focus.keywords.iter().any(|kw| comm.contains(kw)) {
            return true;
        }

        // Interesting application-level processes
        let interesting = [
            // Build tools
            "cargo", "rustc", "gcc", "g++", "make", "cmake", "ninja",
            // Runtimes (but not their subprocess churn)
            "node", "npm", "npx", "python", "python3", "ruby", "java", "go",
            // Editors
            "code", "nvim", "vim", "emacs", "helix",
            // Claude
            "claude",
            // Servers
            "postgres", "mysql", "redis-server", "nginx", "httpd",
            "docker", "podman", "containerd",
            // Browsers (top-level only — we track presence, not subprocess count)
            "firefox", "chromium", "chrome",
            // Network
            "ssh", "scp", "rsync", "curl", "wget",
            // Version control
            "git",
        ];
        interesting.contains(&comm)
    }
}

impl Sensor for ProcessSensor {
    fn name(&self) -> &str {
        "processes"
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = self.snapshot(focus);

        // First poll: establish baseline silently
        if !self.baseline_established {
            eprintln!("[attend] processes: baseline established ({} apps: {})",
                current.len(),
                current.keys().cloned().collect::<Vec<_>>().join(", "));
            self.prior = current;
            self.baseline_established = true;
            return Vec::new();
        }

        let mut observations = Vec::new();

        // New applications (not in prior at all)
        for app in current.keys() {
            if !self.prior.contains_key(app) {
                observations.push((2.0, format!("{app} started")));
            }
        }

        // Exited applications (were in prior, gone now)
        // Build tools: exact match on process name (comm field from /proc)
        let build_tools = [
            "cargo", "rustc", "make", "cmake", "ninja",
            "gcc", "g++", "cc", "c++", "clang", "clang++",
            "go", "npm", "yarn", "pnpm", "tsc",
            "mvn", "gradle", "pip", "pip3",
        ];
        for app in self.prior.keys() {
            if !current.contains_key(app) {
                let is_build = build_tools.contains(&app.as_str());
                if is_build {
                    // Affordance: Claude reads this notification and can invoke the command.
                    // $CLAUDE_SESSION_ID is resolved by Claude when it runs the command.
                    observations.push((2.5, format!(
                        "{app} exited. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"
                    )));
                } else {
                    observations.push((2.0, format!("{app} exited")));
                }
            }
        }

        self.prior = current;
        observations
    }

    fn emission_threshold(&self) -> f64 {
        2.0  // Need at least 2 process events before disclosing
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(30)  // Check every 30s at rest
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(5)   // Ramp up to 5s during activity
    }

    fn decay_threshold(&self) -> u32 {
        5  // 5 quiet ticks before decaying back
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "nobody".to_string())
}

