use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ItemFormatOpts},
};

/// Show a domain
#[derive(clap::Parser, Debug)]
pub struct CmdDomainGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,

    /// Name of the domain.
    name: String,

}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomainGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;
        let domain = wasmer_api::query::get_domain(&client, self.name)
            .await?;
        println!("{}", self.fmt.format.render(&domain));
        Ok(())
    }
}
