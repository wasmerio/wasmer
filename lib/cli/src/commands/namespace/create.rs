use std::io::IsTerminal as _;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};
use dialoguer::theme::ColorfulTheme;

/// Create a new namespace.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceCreate {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    non_interactive: bool,

    /// Display name of the namespace.
    #[clap(long)]
    display_name: Option<String>,

    /// Description of the namespace.
    #[clap(long)]
    description: Option<String>,

    /// Name of the namespace.
    name: Option<String>,
}

impl CmdNamespaceCreate {
    fn can_prompt(&self) -> bool {
        !self.non_interactive && std::io::stdin().is_terminal()
    }

    fn normalize_optional_value(value: Option<&str>) -> Option<String> {
        value.and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
    }

    fn prompt_optional_value(
        &self,
        message: &str,
        default: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let theme = ColorfulTheme::default();
        let value = dialoguer::Input::<String>::with_theme(&theme)
            .with_prompt(message)
            .with_initial_text(default.unwrap_or_default())
            .interact_text()?;

        Ok(Self::normalize_optional_value(Some(&value)))
    }

    fn get_name(&self) -> anyhow::Result<String> {
        if let Some(name) = Self::normalize_optional_value(self.name.as_deref()) {
            return Ok(name);
        }

        if !self.can_prompt() {
            anyhow::bail!("No namespace name given. Provide one as a positional argument.")
        }

        crate::utils::prompts::prompt_for_ident("Enter the namespace name", None)
    }

    fn get_display_name(&self) -> anyhow::Result<Option<String>> {
        if self.display_name.is_some() || !self.can_prompt() {
            return Ok(Self::normalize_optional_value(self.display_name.as_deref()));
        }

        self.prompt_optional_value("Enter the namespace display name", None)
    }

    fn get_description(&self) -> anyhow::Result<Option<String>> {
        if self.description.is_some() || !self.can_prompt() {
            return Ok(Self::normalize_optional_value(self.description.as_deref()));
        }

        self.prompt_optional_value("Enter the namespace description", None)
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespaceCreate {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let vars = wasmer_backend_api::types::CreateNamespaceVars {
            input: wasmer_backend_api::types::CreateNamespaceInput {
                name: self.get_name()?,
                display_name: self.get_display_name()?,
                description: self.get_description()?,
                avatar: None,
                client_mutation_id: None,
            },
        };
        let namespace = wasmer_backend_api::query::create_namespace(&client, vars).await?;

        println!("{}", self.fmt.get().render(&namespace));

        Ok(())
    }
}
