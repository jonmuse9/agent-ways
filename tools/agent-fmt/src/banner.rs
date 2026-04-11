//! Shared banner renderer for agent-ways tools.
//!
//! Usage:
//!   agent_fmt::Banner::new("WAYS")
//!       .subtitle("cognitive steering for AI agents")
//!       .gradient(&agent_fmt::GRADIENT_CORAL)
//!       .print();

use figlet_rs::FIGlet;

/// ANSI Shadow font embedded at compile time.
const ANSI_SHADOW_FLF: &str = include_str!("../fonts/ansi-shadow.flf");

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const UNDERLINE: &str = "\x1b[4m";

/// Warm coral-to-amber gradient (ways).
pub const GRADIENT_CORAL: [&str; 7] = [
    "\x1b[38;5;209m",
    "\x1b[38;5;210m",
    "\x1b[38;5;216m",
    "\x1b[38;5;222m",
    "\x1b[38;5;179m",
    "\x1b[38;5;172m",
    "\x1b[38;5;130m",
];

/// Cool teal gradient (attend).
pub const GRADIENT_TEAL: [&str; 7] = [
    "\x1b[38;5;73m",
    "\x1b[38;5;79m",
    "\x1b[38;5;80m",
    "\x1b[38;5;116m",
    "\x1b[38;5;109m",
    "\x1b[38;5;66m",
    "\x1b[38;5;66m",
];

pub struct Banner<'a> {
    text: &'a str,
    title: &'a str,
    subtitle: Option<&'a str>,
    version: Option<&'a str>,
    gradient: &'a [&'a str],
}

impl<'a> Banner<'a> {
    pub fn new(text: &'a str) -> Self {
        Banner {
            text,
            title: "A G E N T",
            subtitle: None,
            version: None,
            gradient: &GRADIENT_CORAL,
        }
    }

    pub fn title(mut self, t: &'a str) -> Self {
        self.title = t;
        self
    }

    pub fn subtitle(mut self, s: &'a str) -> Self {
        self.subtitle = Some(s);
        self
    }

    pub fn version(mut self, v: &'a str) -> Self {
        self.version = Some(v);
        self
    }

    pub fn gradient(mut self, g: &'a [&'a str]) -> Self {
        self.gradient = g;
        self
    }

    pub fn print(&self) {
        let font = match FIGlet::from_content(ANSI_SHADOW_FLF) {
            Ok(f) => f,
            Err(_) => {
                // Fallback: just print the text bold
                println!("\n  {BOLD}{}{RESET}\n", self.text);
                return;
            }
        };
        let figure = match font.convert(self.text) {
            Some(f) => f,
            None => {
                println!("\n  {BOLD}{}{RESET}\n", self.text);
                return;
            }
        };

        println!();
        println!("  {DIM}{UNDERLINE}{}{RESET}", self.title);
        println!();

        for (i, line) in figure.to_string().lines().enumerate() {
            let color = self.gradient[i % self.gradient.len()];
            println!("{color}{line}{RESET}");
        }

        if let Some(sub) = self.subtitle {
            println!("  {DIM}{sub}{RESET}");
        }
        if let Some(ver) = self.version {
            println!("  {DIM}{ver}{RESET}");
        }
        println!();
    }
}

/// Format a help section with consistent styling.
/// Each entry is (command_name, description).
pub fn print_commands(heading: &str, commands: &[(&str, &str)]) {
    println!("{BOLD}{heading}:{RESET}");
    let max_name = commands.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, desc) in commands {
        println!("  {BOLD}{name:<max_name$}{RESET}  {DIM}{desc}{RESET}");
    }
}
