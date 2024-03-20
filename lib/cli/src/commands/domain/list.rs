use wasmer_api::types::GetAllDomainsVariables;

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

    /// Name of the namespace.
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomainList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;
        let domains = wasmer_api::query::get_all_domains(
            &client,
            GetAllDomainsVariables {
                first: None,
                after: None,
                namespace: self.namespace,
            },
        )
        .await?;
        println!("{}", self.fmt.format.render(&domains));
        Ok(())
    }
}
