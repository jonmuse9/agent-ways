//! Slash-command parser, registry, and autocomplete.
//!
//! Slash commands are a TUI-local grammar: they never touch the
//! signal bus. A message beginning with `/` is intercepted in the
//! Enter handler and dispatched here; everything else falls through
//! to the `@`/`#`/plain-text send paths in `app.rs`.
//!
//! The grammar differs from `@Name` and `#group`:
//! - mentions can appear anywhere in a message body
//! - slash commands are start-of-input only — a `/` mid-sentence is a
//!   literal slash, not a command sigil
//!
//! Because of that asymmetry, parse + completion live in their own
//! module instead of being retrofitted into [`crate::legend`]. Agent
//! / group legends and slash completion share only the spirit of
//! "Tab completes the partial you're typing" — the plumbing is
//! distinct.
//!
//! **Scope.** Today `/help` is the only dispatched command. Other
//! names in [`REGISTRY`] are advertised as planned so autocomplete
//! can surface them and `/help` can print the roadmap; dispatching
//! them returns a "not implemented yet" status. When a planned
//! command lands, flip its [`Status`] to [`Status::Implemented`] and
//! add a handler arm in [`dispatch`].

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Wired into [`dispatch`]. Runs when invoked.
    Implemented,
    /// Listed for autocomplete + `/help` visibility, but dispatch
    /// short-circuits to a "coming soon" status message.
    Planned,
}

/// One row of the slash-command registry.
#[derive(Copy, Clone, Debug)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub status: Status,
}

/// Every slash command the TUI knows about. Implemented ones
/// dispatch; planned ones autocomplete and show up in `/help` so
/// agents can see the roadmap without reading code.
pub const REGISTRY: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        description: "List available commands",
        status: Status::Implemented,
    },
    SlashCommand {
        name: "whois",
        description: "Show a peer's identity + cwd",
        status: Status::Planned,
    },
    SlashCommand {
        name: "peers",
        description: "List active claudes + humans",
        status: Status::Planned,
    },
    SlashCommand {
        name: "join",
        description: "Join a focus group",
        status: Status::Planned,
    },
    SlashCommand {
        name: "leave",
        description: "Leave a focus group",
        status: Status::Planned,
    },
    SlashCommand {
        name: "clear",
        description: "Clear the message buffer",
        status: Status::Planned,
    },
];

/// Parse a slash command from the input buffer.
///
/// Returns `Some((name, args))` if `input` begins with `/` followed
/// by a non-empty name; args is whatever follows the first whitespace
/// run, trimmed of its leading spaces. Returns `None` for plain text,
/// a bare `/`, or a `/ ` followed by whitespace.
pub fn parse(input: &str) -> Option<(&str, &str)> {
    let rest = input.strip_prefix('/')?;
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let name = &rest[..end];
    let args = rest[end..].trim_start();
    Some((name, args))
}

/// Partial slash command at the head of the input — the autocomplete
/// gate. Returns `Some` only while the user is still typing the
/// *name*: a space after the name means completion is done and args
/// are being typed.
#[derive(Debug, PartialEq, Eq)]
pub struct SlashPartial<'a> {
    pub partial: &'a str,
}

pub fn find_slash_partial(input: &str) -> Option<SlashPartial<'_>> {
    let rest = input.strip_prefix('/')?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    Some(SlashPartial { partial: rest })
}

/// Find the best match for `partial` in the registry. Case-
/// insensitive prefix match; first hit in registry order wins (so
/// the order in [`REGISTRY`] doubles as completion priority).
pub fn best_slash_completion(partial: &str) -> Option<&'static SlashCommand> {
    let lc = partial.to_ascii_lowercase();
    REGISTRY
        .iter()
        .find(|c| c.name.to_ascii_lowercase().starts_with(&lc))
}

/// Apply a completion: replace the whole buffer with `/<name> ` and
/// place the cursor after the trailing space, ready for args.
pub fn apply_slash_completion(full_name: &str) -> (String, usize) {
    let out = format!("/{full_name} ");
    let cursor = out.chars().count();
    (out, cursor)
}

/// Outcome of dispatching a parsed slash command.
#[derive(Debug, PartialEq, Eq)]
pub enum SlashOutcome {
    /// Status message to show; clear the input buffer.
    Ok(String),
    /// Status message to show; *keep* the input so the user can
    /// edit + retry (typical for unknown / malformed commands).
    Err(String),
}

/// Dispatch a parsed slash command.
pub fn dispatch(name: &str, _args: &str) -> SlashOutcome {
    let Some(cmd) = REGISTRY.iter().find(|c| c.name == name) else {
        return SlashOutcome::Err(format!("unknown: /{name} (try /help)"));
    };
    match cmd.status {
        Status::Implemented => match cmd.name {
            "help" => SlashOutcome::Ok(help_message()),
            // Keeps the match exhaustive — if a registry entry flips
            // to Implemented without a handler arm, this fires at
            // runtime rather than silently treating it as planned.
            other => SlashOutcome::Err(format!("/{other}: registered but unhandled")),
        },
        Status::Planned => SlashOutcome::Err(format!(
            "/{}: {} — not implemented yet",
            cmd.name, cmd.description
        )),
    }
}

fn help_message() -> String {
    let mut ready: Vec<String> = Vec::new();
    let mut planned: Vec<String> = Vec::new();
    for c in REGISTRY {
        let label = format!("/{}", c.name);
        match c.status {
            Status::Implemented => ready.push(label),
            Status::Planned => planned.push(label),
        }
    }
    format!(
        "available: {} · planned: {}",
        ready.join(" "),
        planned.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_command() {
        assert_eq!(parse("/help"), Some(("help", "")));
    }

    #[test]
    fn parse_command_with_args() {
        assert_eq!(parse("/whois @Urban"), Some(("whois", "@Urban")));
    }

    #[test]
    fn parse_collapses_leading_arg_whitespace() {
        assert_eq!(parse("/whois    @Urban"), Some(("whois", "@Urban")));
    }

    #[test]
    fn parse_rejects_bare_slash() {
        assert_eq!(parse("/"), None);
    }

    #[test]
    fn parse_rejects_slash_space() {
        // `/ foo` — no command name, just a stray slash.
        assert_eq!(parse("/ foo"), None);
    }

    #[test]
    fn parse_rejects_plain_text() {
        assert_eq!(parse("hello /help"), None);
        assert_eq!(parse("hello"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn slash_partial_while_typing_name() {
        let p = find_slash_partial("/hel").unwrap();
        assert_eq!(p.partial, "hel");
    }

    #[test]
    fn slash_partial_bare_slash_is_empty() {
        let p = find_slash_partial("/").unwrap();
        assert_eq!(p.partial, "");
    }

    #[test]
    fn slash_partial_stops_after_space() {
        // Past the command name — completion no longer applies.
        assert!(find_slash_partial("/help ").is_none());
        assert!(find_slash_partial("/whois @Ur").is_none());
    }

    #[test]
    fn slash_partial_rejects_non_slash_input() {
        assert!(find_slash_partial("hello").is_none());
        assert!(find_slash_partial("").is_none());
    }

    #[test]
    fn best_completion_matches_prefix_case_insensitive() {
        let c = best_slash_completion("he").unwrap();
        assert_eq!(c.name, "help");
        let c = best_slash_completion("HE").unwrap();
        assert_eq!(c.name, "help");
    }

    #[test]
    fn best_completion_empty_partial_picks_first() {
        // A bare `/` + Tab should complete to the first registry
        // entry — the most visible command gets the zero-friction
        // path.
        let c = best_slash_completion("").unwrap();
        assert_eq!(c.name, REGISTRY[0].name);
    }

    #[test]
    fn best_completion_none_on_miss() {
        assert!(best_slash_completion("zzzzz").is_none());
    }

    #[test]
    fn apply_completion_adds_trailing_space() {
        let (out, cursor) = apply_slash_completion("help");
        assert_eq!(out, "/help ");
        assert_eq!(cursor, "/help ".chars().count());
    }

    #[test]
    fn dispatch_help_lists_commands() {
        let SlashOutcome::Ok(s) = dispatch("help", "") else {
            panic!("help should dispatch Ok")
        };
        assert!(s.contains("/help"));
        // At least one planned command surfaces — roadmap visibility.
        assert!(s.contains("planned"));
    }

    #[test]
    fn dispatch_unknown_returns_err() {
        let SlashOutcome::Err(s) = dispatch("bogus", "") else {
            panic!("unknown should dispatch Err")
        };
        assert!(s.contains("unknown"));
        assert!(s.contains("/bogus"));
    }

    #[test]
    fn dispatch_planned_returns_err_with_not_implemented() {
        // Pick any Planned command from the registry — keeps the
        // test valid as planned/implemented sets shift.
        let planned = REGISTRY
            .iter()
            .find(|c| c.status == Status::Planned)
            .expect("registry must contain at least one planned command while slash infra is new");
        let SlashOutcome::Err(s) = dispatch(planned.name, "") else {
            panic!("planned should dispatch Err")
        };
        assert!(s.contains("not implemented"));
        assert!(s.contains(planned.name));
    }
}
