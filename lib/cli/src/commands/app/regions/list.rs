use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ListFormatOpts, WasmerEnv},
};
use is_terminal::IsTerminal;

/// List available Edge regions.
#[derive(clap::Parser, Debug)]
pub struct CmdAppRegionsList {
    /* --- Common flags --- */
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,

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
        let client = self.api.client()?;
        let regions = wasmer_api::query::get_all_app_regions(&client).await?;
        for region in regions {
            println!("{}", region.name);
        }
        Ok(())
    }
}
