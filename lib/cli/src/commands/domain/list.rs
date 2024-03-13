use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ListFormatOpts},
};

/// List domains.
#[derive(clap::Parser, Debug)]
pub struct CmdDomainList {
    #[clap(flatten)]
    fmt: ListFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomainList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
