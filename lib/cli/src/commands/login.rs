use clap::Parser;
use dialoguer::Input;

/// Subcommand for listing packages
#[derive(Debug, Clone, Parser)]
pub struct Login {
    /// Registry to log into (default: wapm.io)
    #[clap(long, default_value = "wapm.io")]
    pub registry: String,
    /// Login token
    #[clap(name = "TOKEN")]
    pub token: Option<String>,
}

impl Login {
    fn get_token_or_ask_user(&self) -> Result<String, std::io::Error> {
        match self.token.as_ref() {
            Some(s) => Ok(s.clone()),
            None => Input::new()
                .with_prompt("Please paste the login token from https://wapm.io/me:\"")
                .interact_text(),
        }
    }

    /// execute [List]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let token = self.get_token_or_ask_user()?;
        wasmer_registry::login::login_and_save_token(&self.registry, &token)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}
