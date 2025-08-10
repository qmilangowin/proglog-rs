use tracing::info;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting proglog-rs application");

    Ok(())
}
