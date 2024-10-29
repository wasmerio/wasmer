use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};
use is_terminal::IsTerminal;

/// List available Edge regions.
#[derive(clap::Parser, Debug)]
pub struct CmdAppRegionsList {
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
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppRegionsList {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let regions = wasmer_backend_api::query::get_all_app_regions(&client).await?;

        println!("{}", self.fmt.format.render(regions.as_slice()));

        Ok(())
    }
}
