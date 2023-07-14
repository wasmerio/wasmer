use clap::Parser;
use wasmer_registry::{wasmer_env::WasmerEnv, CurrentUser};

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

        match wasmer_registry::current_user(self.env.dir(), Some(registry.as_str()), None)? {
            Some(CurrentUser {
                registry,
                user,
                verified,
                ..
            }) => {
                println!("logged into registry \"{registry}\" as user \"{user}\"");
                if !verified {
                    println!("Warning: Your email address still needs to be verified");
                }
            }
            None => {
                println!("Not logged in.");
            }
        }

        Ok(())
    }
}
