use super::utils::{get_secrets, BackendSecretWrapper};
use crate::{
    commands::{app::util::AppIdentFlag, AsyncCliCommand},
    config::WasmerEnv,
    opts::ListFormatOpts,
};
use is_terminal::IsTerminal;
use std::path::PathBuf;

/// Retrieve the value of an existing app secret.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsList {
    /* --- Common flags --- */
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Don't print any message.
    #[clap(long)]
    pub quiet: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    #[clap(flatten)]
    pub fmt: ListFormatOpts,

    /* --- Flags --- */
    /// The identifier of the app to list secrets of.
    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsList {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let app_id = super::utils::get_app_id(
            &client,
            self.app_id.app.as_ref(),
            self.app_dir_path.as_ref(),
            self.quiet,
            self.non_interactive,
        )
        .await?;

        let secrets: Vec<BackendSecretWrapper> = get_secrets(&client, &app_id)
            .await?
            .into_iter()
            .map(|s| s.into())
            .collect();

        println!("{}", self.fmt.format.render(secrets.as_slice()));

        Ok(())
    }
}
