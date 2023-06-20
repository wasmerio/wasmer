use clap::Parser;

use crate::WasmerDir;

#[derive(Debug, Parser)]
/// The options for the `wasmer whoami` subcommand
pub struct Whoami {
    #[clap(flatten)]
    wasmer_dir: WasmerDir,
}

impl Whoami {
    /// Execute `wasmer whoami`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let registry = self.wasmer_dir.registry_endpoint()?;
        let (registry, username) =
            wasmer_registry::whoami(self.wasmer_dir.dir(), Some(registry.as_str()), None)?;
        println!("logged into registry {registry:?} as user {username:?}");
        Ok(())
    }
}
