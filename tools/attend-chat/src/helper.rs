//! Helper-row state machine.
//!
//! The row below the input box shows one of three registries — agents,
//! groups, or slash commands — depending on what the user is typing.
//! This module is the single source of truth for that decision.
//!
//! [`derive`] is a pure function of the input buffer. Given any
//! string, it returns the [`HelperMode`] the render path should use.
//! Deterministic, side-effect free, and covered by a table of unit
//! tests that double as the state machine's visible specification
//! — scroll to the bottom of this file to see every state and the
//! input pattern that reaches it.
//!
//! ## States
//!
//! | State        | Entered when …                                | Partial underlined |
//! |--------------|-----------------------------------------------|--------------------|
//! | `Slash`      | buffer starts with `/`, still typing the name | yes, on command    |
//! | `Agents`     | trailing `@partial`, OR default               | yes, on agent      |
//! | `Groups`     | trailing `#partial`                           | yes, on group      |
//! | `Agents/None`| past a slash command whose arg is [`ArgKind::Agent`] | no, until `@` typed |
//! | `Groups/None`| past a slash command whose arg is [`ArgKind::Group`] | no, until `#` typed |
//! | `Agents`     | past a slash command with [`ArgKind::None`]   | —                  |
//!
//! ## Extending
//!
//! Adding a new slash command that routes the helper: add a row to
//! [`crate::slash::REGISTRY`] with the right [`ArgKind`]. No changes
//! here — [`derive`] inspects the registry dynamically.

use crate::legend::{find_trailing_mention, Sigil};
use crate::slash::{self, ArgKind};

/// Which registry the helper row should render right now. The
/// `Option<String>` carries the current partial (what the user has
/// typed after the sigil) so matching chips can underline for
/// Tab-target affordance. `None` means "no active partial" — render
/// the row without any underline.
#[derive(Debug, PartialEq, Eq)]
pub enum HelperMode {
    Agents(Option<String>),
    Groups(Option<String>),
    Slash(Option<String>),
}

/// Derive the helper mode from the current input buffer.
///
/// Precedence:
///
/// 1. **Slash active.** Input starts with `/`:
///    - still typing the name → [`HelperMode::Slash`] with partial.
///    - past the name, argument phase → inspect the command's
///      [`ArgKind`] and switch to the matching registry.
///    - past the name, unknown command → stay on slash registry so
///      the user can see valid options.
/// 2. **Trailing mention.** A trailing `@partial` or `#partial`
///    picks Agents or Groups respectively, with the partial under-
///    lined.
/// 3. **Default.** [`HelperMode::Agents`] with no partial — the
///    "who's around" glance.
pub fn derive(input: &str) -> HelperMode {
    if let Some(mode) = derive_slash(input) {
        return mode;
    }
    if let Some(mode) = derive_mention(input) {
        return mode;
    }
    HelperMode::Agents(None)
}

fn derive_slash(input: &str) -> Option<HelperMode> {
    if !input.starts_with('/') {
        return None;
    }
    // Phase 1: still typing the command name.
    if let Some(partial) = slash::find_slash_partial(input) {
        return Some(HelperMode::Slash(Some(partial.partial.to_string())));
    }
    // Phase 2: past the name, in the argument(s). Inspect the
    // command's ArgKind to pick the right registry.
    let (name, args) = slash::parse(input)?;
    let Some(cmd) = slash::lookup(name) else {
        // Unknown command — keep the slash registry visible so the
        // user can see what they might have meant.
        return Some(HelperMode::Slash(None));
    };
    Some(match cmd.arg_kind {
        ArgKind::None => HelperMode::Agents(None),
        ArgKind::Agent => HelperMode::Agents(trailing_partial(args, Sigil::Agent)),
        ArgKind::Group => HelperMode::Groups(trailing_partial(args, Sigil::Group)),
    })
}

fn derive_mention(input: &str) -> Option<HelperMode> {
    let ctx = find_trailing_mention(input)?;
    Some(match ctx.sigil {
        Sigil::Agent => HelperMode::Agents(Some(ctx.partial.to_string())),
        Sigil::Group => HelperMode::Groups(Some(ctx.partial.to_string())),
    })
}

/// Extract a trailing `@partial` or `#partial` from `input`, returning
/// the partial text only if its sigil matches `expect`.
fn trailing_partial(input: &str, expect: Sigil) -> Option<String> {
    find_trailing_mention(input)
        .filter(|m| m.sigil == expect)
        .map(|m| m.partial.to_string())
}

#[cfg(test)]
mod tests {
    //! State machine's visible specification.
    //!
    //! Each test corresponds to one state-entry condition. Reading
    //! the test names top-to-bottom is reading the rule table.

    use super::*;

    // ── Default + mention states ───────────────────────────────

    #[test]
    fn empty_input_defaults_to_agents() {
        assert_eq!(derive(""), HelperMode::Agents(None));
    }

    #[test]
    fn plain_text_defaults_to_agents() {
        assert_eq!(derive("hello world"), HelperMode::Agents(None));
    }

    #[test]
    fn trailing_at_partial_selects_agents() {
        assert_eq!(derive("hi @Tam"), HelperMode::Agents(Some("Tam".into())));
    }

    #[test]
    fn trailing_hash_partial_selects_groups() {
        assert_eq!(derive("hi #dep"), HelperMode::Groups(Some("dep".into())));
    }

    // ── Slash: typing the command name ─────────────────────────

    #[test]
    fn bare_slash_shows_slash_with_empty_partial() {
        assert_eq!(derive("/"), HelperMode::Slash(Some("".into())));
    }

    #[test]
    fn slash_partial_name_shows_slash() {
        assert_eq!(derive("/he"), HelperMode::Slash(Some("he".into())));
    }

    // ── Slash: past the name, ArgKind routing ──────────────────

    #[test]
    fn slash_help_space_stays_on_agents_as_default() {
        // ArgKind::None → default registry (agents), no partial.
        assert_eq!(derive("/help "), HelperMode::Agents(None));
    }

    #[test]
    fn slash_whois_space_switches_to_agents_waiting_for_at() {
        // ArgKind::Agent, no @ typed yet — Agents with no partial.
        assert_eq!(derive("/whois "), HelperMode::Agents(None));
    }

    #[test]
    fn slash_whois_with_at_partial_underlines_agent() {
        // ArgKind::Agent, user is now typing @Ur — underline "Ur".
        assert_eq!(
            derive("/whois @Ur"),
            HelperMode::Agents(Some("Ur".into()))
        );
    }

    #[test]
    fn slash_join_space_switches_to_groups() {
        // ArgKind::Group, no # typed yet — Groups with no partial.
        assert_eq!(derive("/join "), HelperMode::Groups(None));
    }

    #[test]
    fn slash_join_with_hash_partial_underlines_group() {
        assert_eq!(
            derive("/join #dep"),
            HelperMode::Groups(Some("dep".into()))
        );
    }

    #[test]
    fn slash_leave_space_switches_to_groups() {
        // Second Group-arg command — confirms ArgKind routing isn't
        // tied to a single command name.
        assert_eq!(derive("/leave "), HelperMode::Groups(None));
    }

    // ── Slash: unknown / malformed ─────────────────────────────

    #[test]
    fn slash_unknown_command_keeps_slash_registry() {
        // So the user can see what they might have meant.
        assert_eq!(derive("/bogus "), HelperMode::Slash(None));
        assert_eq!(derive("/bogus arg"), HelperMode::Slash(None));
    }

    // ── Precedence: slash wins over any other sigil ────────────

    #[test]
    fn slash_precedes_trailing_mention() {
        // `/help @Tam` — even though there's a trailing @, the
        // buffer starts with `/` and we're past the command name.
        // ArgKind::None returns Agents(None), which tests the
        // state machine's "slash wins" invariant.
        assert_eq!(derive("/help @Tam"), HelperMode::Agents(None));
    }
}
