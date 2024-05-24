mod argus;

use argus::*;
use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let config = ArgusConfig::parse();

    let argus = Argus::try_from(config)?;
    argus.run().await
}
