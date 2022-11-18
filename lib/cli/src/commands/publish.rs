use clap::Parser;

/// CLI options for the `wasmer publish` command
#[derive(Debug, Parser)]
pub struct Publish {
    /// Directory containing the `wapm.toml` (defaults to current root dir)
    #[clap(long, name = "dir", env = "DIR")]
    pub dir: Option<String>,
    /// Registry to publish to
    #[clap(long, name = "registry")]
    pub registry: Option<String>,
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        println!("{:?}", self);
        Ok(())
    }
}
