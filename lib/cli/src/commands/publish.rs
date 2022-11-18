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
    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    dry_run: bool,
    /// Run the publish command without any output
    #[clap(long, name = "quiet")]
    quiet: bool,
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        println!("{:?}", self);
        Ok(())
    }
}
