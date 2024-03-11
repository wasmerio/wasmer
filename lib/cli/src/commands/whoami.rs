use clap::Parser;
use wasmer_registry::wasmer_env::WasmerEnv;

#[derive(Debug, Parser)]
/// The options for the `wasmer whoami` subcommand
pub struct Whoami {
    #[clap(flatten)]
    env: WasmerEnv,
}

impl Whoami {
    /// Execute `wasmer whoami`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let registry = self.env.registry_endpoint()?;
        let token = self.env.token();
        let (registry, username) =
            wasmer_registry::whoami(self.env.dir(), Some(registry.as_str()), token.as_deref())?;
        println!("logged into registry {registry:?} as user {username:?}");
        Ok(())
    }
}
