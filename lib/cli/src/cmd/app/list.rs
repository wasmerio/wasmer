use crate::{
    cmd::AsyncCliCommand,
    opts::{ApiOpts, ListFormatOpts},
};

/// List apps.
#[derive(clap::Parser, Debug)]
pub struct CmdAppList {
    #[clap(flatten)]
    fmt: ListFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,

    /// Get apps in a specific namespace.
    ///
    /// Will fetch the apps owned by the current user otherwise.
    #[clap(short = 'n', long)]
    namespace: Option<String>,

    /// Get all apps that are accessible by the current user, including apps
    /// directly owned by the user and apps in namespaces the user can access.
    #[clap(short = 'a', long)]
    all: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let apps = if let Some(ns) = self.namespace {
            wasmer_api::query::namespace_apps(&client, &ns).await?
        } else if self.all {
            wasmer_api::query::user_accessible_apps(&client).await?
        } else {
            wasmer_api::query::user_apps(&client).await?
        };

        println!("{}", self.fmt.format.render(&apps));

        Ok(())
    }
}
