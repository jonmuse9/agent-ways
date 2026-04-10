//! Shared terminal formatting for agent-ways tools.
//!
//! Provides ANSI-aware table rendering, banner display, and help formatting.

mod banner;
mod table;

pub use banner::{Banner, print_commands, GRADIENT_CORAL, GRADIENT_TEAL};
pub use table::{Align, Table, terminal_width};
