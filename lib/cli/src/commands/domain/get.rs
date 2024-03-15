use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ItemTableFormatOpts},
};

/// Show a domain
#[derive(clap::Parser, Debug)]
pub struct CmdDomainGet {
    #[clap(flatten)]
    fmt: ItemTableFormatOpts,
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
        if let Some(domain) = wasmer_api::query::get_domain_with_records(&client, self.name).await?
        {
            println!("{}", self.fmt.format.render(&domain));
        } else {
            anyhow::bail!("Domain not found");
        }
        Ok(())
    }
}
