//! List volumes tied to an edge app.

use super::super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List the volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDeploymentList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentOpts,

    #[clap(long)]
    offset: Option<u32>,

    #[clap(long)]
    limit: Option<u32>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeploymentList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let (_ident, app) = self.ident.load_app(&client).await?;
        let vars = wasmer_backend_api::types::GetAppDeploymentsVariables {
            after: None,
            first: self.limit.map(|x| x as i32),
            name: app.name.clone(),
            offset: self.offset.map(|x| x as i32),
            owner: app.owner.global_name,
        };
        let items = wasmer_backend_api::query::app_deployments(&client, vars).await?;

        if items.is_empty() {
            eprintln!("App {} has no deployments!", app.name);
        } else {
            println!("{}", self.fmt.format.render(&items));
        }

        Ok(())
    }
}
