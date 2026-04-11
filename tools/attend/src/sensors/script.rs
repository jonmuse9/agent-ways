//! Script sensor — wraps a shell script in the Sensor trait.
//!
//! Config: `+sensor-name: { script: path, interval: N }`
//! Script contract: stdout lines as `magnitude|description`, exit 0.
//! Non-zero exit or unparseable lines are silently ignored.

use std::process::Command;
use std::time::Duration;

use super::{Focus, Sensor};

pub struct ScriptSensor {
    name: String,
    script: String,
    working_dir: String,
    base_interval: Duration,
    min_interval: Duration,
    decay_threshold: u32,
    emission_threshold: f64,
}

impl ScriptSensor {
    pub fn new(
        name: String,
        script: String,
        working_dir: String,
        base_interval: Duration,
        min_interval: Duration,
        decay_threshold: u32,
        emission_threshold: f64,
    ) -> Self {
        Self {
            name,
            script,
            working_dir,
            base_interval,
            min_interval,
            decay_threshold,
            emission_threshold,
        }
    }

    /// Resolve the script path relative to the working directory.
    fn resolve_script(&self) -> std::path::PathBuf {
        let path = std::path::Path::new(&self.script);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::path::Path::new(&self.working_dir).join(path)
        }
    }
}

impl Sensor for ScriptSensor {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, _focus: &Focus) -> Vec<(f64, String)> {
        let script_path = self.resolve_script();

        if !script_path.is_file() {
            eprintln!("[attend] script sensor '{}': script not found: {}", self.name, script_path.display());
            return Vec::new();
        }

        let output = match Command::new("bash")
            .arg(&script_path)
            .current_dir(&self.working_dir)
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("[attend] script sensor '{}': exec failed: {}", self.name, e);
                return Vec::new();
            }
        };

        if !output.status.success() {
            return Vec::new();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut observations = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Parse magnitude|description
            if let Some((mag_str, desc)) = line.split_once('|') {
                if let Ok(magnitude) = mag_str.trim().parse::<f64>() {
                    let desc = desc.trim().to_string();
                    if !desc.is_empty() {
                        observations.push((magnitude, desc));
                    }
                }
            }
        }

        observations
    }

    fn emission_threshold(&self) -> f64 {
        self.emission_threshold
    }

    fn base_interval(&self) -> Duration {
        self.base_interval
    }

    fn min_interval(&self) -> Duration {
        self.min_interval
    }

    #[allow(dead_code)]
    fn decay_threshold(&self) -> u32 {
        self.decay_threshold
    }
}
