//! List Edge apps.

use std::pin::Pin;

use futures::{Stream, StreamExt};
use wasmer_backend_api::types::{DeployApp, DeployAppsSortBy};

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List apps belonging to a namespace
#[derive(clap::Parser, Debug)]
pub struct CmdAppList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Get apps in a specific namespace.
    ///
    /// Will fetch the apps owned by the current user otherwise.
    #[clap(short = 'n', long)]
    namespace: Option<String>,

    /// Get all apps that are accessible by the current user, including apps
    /// directly owned by the user and apps in namespaces the user can access.
    #[clap(short = 'a', long)]
    all: bool,

    /// Maximum number of apps to display
    #[clap(long, default_value = "1000")]
    max: usize,

    /// Asks whether to display the next page or not
    #[clap(long, default_value = "false")]
    paging_mode: bool,

    /// Sort order for apps.
    ///
    /// Use: --newest, --oldest or --last-updated
    #[clap(long, default_value = "last-updated")]
    sort: AppSort,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum AppSort {
    Newest,
    Oldest,
    LastUpdated,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let sort = match self.sort {
            AppSort::Newest => DeployAppsSortBy::Newest,
            AppSort::Oldest => DeployAppsSortBy::Oldest,
            AppSort::LastUpdated => DeployAppsSortBy::MostActive,
        };

        let apps_stream: Pin<
            Box<dyn Stream<Item = Result<Vec<DeployApp>, anyhow::Error>> + Send + Sync>,
        > = if let Some(ns) = self.namespace.clone() {
            Box::pin(wasmer_backend_api::query::namespace_apps(&client, ns, sort).await)
        } else if self.all {
            Box::pin(wasmer_backend_api::query::user_accessible_apps(&client, sort).await?)
        } else {
            Box::pin(wasmer_backend_api::query::user_apps(&client, sort).await)
        };

        let mut apps_stream = std::pin::pin!(apps_stream);

        let mut rem = self.max;

        let mut display_apps = vec![];

        'list: while let Some(apps) = apps_stream.next().await {
            let mut apps = apps?;

            let limit = std::cmp::min(apps.len(), rem);

            if limit == 0 {
                break;
            }

            rem -= limit;

            if self.paging_mode {
                println!("{}", self.fmt.format.render(&apps));

                loop {
                    println!("next page? [y, n]");

                    let mut rsp = String::new();
                    std::io::stdin().read_line(&mut rsp)?;

                    if rsp.trim() == "y" {
                        continue 'list;
                    }
                    if rsp.trim() == "n" {
                        break 'list;
                    }

                    println!("uknown response: {rsp}");
                }
            }

            display_apps.extend(apps.drain(..limit));
        }

        if !display_apps.is_empty() {
            println!("{}", self.fmt.format.render(&display_apps));
        }

        Ok(())
    }
}
