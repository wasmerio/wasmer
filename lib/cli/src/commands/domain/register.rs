use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemTableFormatOpts};

/// Show a domain
#[derive(clap::Parser, Debug)]
pub struct CmdDomainRegister {
    #[clap(flatten)]
    fmt: ItemTableFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Name of the domain.
    name: String,

    /// owner under which the domain will live.
    #[clap(long, short)]
    namespace: Option<String>,

    /// auto update DNS records for this domain.
    #[clap(long, short)]
    import_records: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomainRegister {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let domain = wasmer_backend_api::query::register_domain(
            &client,
            self.name,
            self.namespace,
            Some(self.import_records),
        )
        .await?;
        println!(
            "{}: domain registered under owner `{}`",
            domain.name, domain.owner.global_name
        );
        Ok(())
    }
}
