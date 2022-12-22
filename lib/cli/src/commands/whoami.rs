use clap::Parser;
use wasmer_registry::WasmerConfig;

#[derive(Debug, Parser)]
/// The options for the `wasmer whoami` subcommand
pub struct Whoami {
    /// Which registry to check the logged in username for
    #[clap(long, name = "registry")]
    pub registry: Option<String>,
}

impl Whoami {
    /// Execute `wasmer whoami`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let wasmer_dir =
            WasmerConfig::get_wasmer_dir().map_err(|e| anyhow::anyhow!("no wasmer dir: {e}"))?;
        let (registry, username) =
            wasmer_registry::whoami(&wasmer_dir, self.registry.as_deref(), None)?;
        println!("logged into registry {registry:?} as user {username:?}");
        Ok(())
    }
}
