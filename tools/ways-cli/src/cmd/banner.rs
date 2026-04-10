use anyhow::Result;

pub fn run() -> Result<()> {
    agent_fmt::Banner::new("WAYS")
        .subtitle("cognitive steering for AI agents")
        .gradient(&agent_fmt::GRADIENT_CORAL)
        .print();
    Ok(())
}
