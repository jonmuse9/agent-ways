use anyhow::Result;

pub fn run() -> Result<()> {
    let version = format!("v{} ({})", env!("CARGO_PKG_VERSION"), env!("WAYS_COMMIT"));
    agent_fmt::Banner::new("WAYS")
        .subtitle("cognitive steering for AI agents")
        .version(&version)
        .gradient(&agent_fmt::GRADIENT_CORAL)
        .print();
    Ok(())
}
