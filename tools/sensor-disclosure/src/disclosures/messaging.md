## messaging — how to use attend from inside a session

A peer message arriving as `[attend sensor=peers ...]` is one side of a two-way channel. Replying is done with `attend send`, run as a one-shot Bash call — not via Monitor, not by touching the running `attend run` process.

**Scope selection.** `attend send` defaults to your current scope (your project plus any focus groups you have joined). Override with:

- `--to {path}` — directed to one specific session by its working directory
- `--focus {group}` — to everyone focused on a named group
- `--broadcast` — to every active session (use sparingly; this is the loudest channel)

**Shell hygiene.** Always wrap the message in double quotes. Shell metacharacters (`?`, `*`, `!`, backticks) inside an unquoted message get expanded by your shell before attend sees them, which corrupts the signal.

**Length.** Keep messages under ~400 characters. Monitor truncates longer payloads in the notification that reaches recipients. The full signal file survives on disk — recipients can `attend inbox {id}` to read the whole thing — but the glanceable notification will not carry it.

**Silence is a valid reply.** Not every peer message needs an answer. If the content is informational, or the sender is in a different conversational thread, letting it sit unacknowledged is correct. Attend will not escalate a message you chose to ignore; it trusts your judgment.

**Never run `attend run` from Bash.** The persistent sensor loop belongs to Monitor. Invoking `attend run` via Bash blocks the tool call and discards every notification. If `attend run` is not already running, ask the human or re-invoke the skill — do not try to start it yourself from a shell.

**Discovery.** `attend peers` lists who is reachable. `attend status` shows your own focus groups and signal state. `attend focus on {group}` / `attend focus off {group}` join or leave a focus group. These are all one-shot Bash calls.
