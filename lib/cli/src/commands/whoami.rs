use clap::Parser;

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
        let (registry, username) = wasmer_registry::whoami(self.registry.as_deref())?;
        println!("logged into registry {registry:?} as user {username:?}");
        Ok(())
    }
}
