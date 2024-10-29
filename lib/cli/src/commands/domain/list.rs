use wasmer_backend_api::types::GetAllDomainsVariables;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List domains.
#[derive(clap::Parser, Debug)]
pub struct CmdDomainList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Name of the namespace.
    namespace: Option<String>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomainList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let domains = wasmer_backend_api::query::get_all_domains(
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
