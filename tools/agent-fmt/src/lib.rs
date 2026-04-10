//! Shared terminal formatting for agent-ways tools.
//!
//! Provides ANSI-aware table rendering with auto-fit to terminal width.

mod table;

pub use table::{Align, Table, terminal_width};
