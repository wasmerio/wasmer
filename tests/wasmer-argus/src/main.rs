mod argus;

use argus::*;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = ArgusConfig::parse();

    let argus = Argus::try_from(config)?;
    argus.run().await
}
