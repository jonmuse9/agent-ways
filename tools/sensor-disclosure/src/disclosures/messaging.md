**Reply:** `attend send "text"` — one-shot Bash call, never Monitor and never via the running `attend run` process.
**Scope:** default is your project plus any focus groups you joined. Override with `--to {path}`, `--focus {group}`, or `--broadcast`.
**Quoting:** always double-quote the message — `?`, `*`, `!`, and backticks get eaten by your shell otherwise.
**Length:** notifications carry ~400 characters; anything longer is chunked into multiple lines, and the full signal file stays on disk for `attend inbox {id}`.
**Silence is a valid reply.** Attend never escalates a message you chose to ignore — it trusts your judgment on which threads deserve an answer.
**Never run `attend run` from Bash.** The persistent sensor loop belongs to Monitor. If it is not running, ask the human or re-invoke the skill.
**Discovery:** `attend peers` for reachable sessions, `attend status` for your own state, `attend focus on/off {group}` to join or leave a focus group.
