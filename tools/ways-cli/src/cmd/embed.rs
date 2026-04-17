use anyhow::Result;

/// `ways embed` — retained as an alias for `ways match` (embedding-only since ADR-125).
pub fn run(query: String, corpus: Option<String>, _model: Option<String>) -> Result<()> {
    super::match_cmd::run(query, corpus)
}
