use wasmer_backend_api::types::{DeployAppVersionsSortBy, GetDeployAppVersionsVars};

use crate::{
    commands::{app::util::AppIdentOpts, AsyncCliCommand},
    config::WasmerEnv,
    opts::ListFormatOpts,
};

/// List versions of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVersionList {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[allow(missing_docs)]
    #[clap(flatten)]
    pub fmt: ListFormatOpts,

    /// Get all versions of the app.
    /// Overrides pagination flags (--max, --offset).
    #[clap(short = 'a', long)]
    all: bool,

    /// Pagination offset - get versions after this offset.
    ///
    /// See also: --max, --before, --after
    #[clap(long)]
    offset: Option<u32>,

    /// Maximum number of items to return.
    ///
    /// See also: --offset, --before, --after
    #[clap(long)]
    max: Option<u32>,

    /// Pagination cursor - get versions before this version.
    ///
    /// See also: --max, --offset, --after
    #[clap(long)]
    before: Option<String>,

    /// Pagination cursor - get versions after this version.
    ///
    /// See also: --max, --offset, --before
    #[clap(long)]
    after: Option<String>,

    #[clap(long)]
    sort: Option<Sort>,

    #[clap(flatten)]
    #[allow(missing_docs)]
    pub ident: AppIdentOpts,
}

#[derive(Clone, Copy, clap::ValueEnum, Debug)]
enum Sort {
    Newest,
    Oldest,
}

impl From<Sort> for DeployAppVersionsSortBy {
    fn from(val: Sort) -> Self {
        match val {
            Sort::Newest => DeployAppVersionsSortBy::Newest,
            Sort::Oldest => DeployAppVersionsSortBy::Oldest,
        }
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVersionList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let versions = if self.all {
            wasmer_backend_api::query::all_app_versions(&client, app.owner.global_name, app.name)
                .await?
        } else {
            let vars = GetDeployAppVersionsVars {
                owner: app.owner.global_name,
                name: app.name,
                offset: self.offset.map(|x| x as i32),
                before: self.before,
                after: self.after,
                first: self.max.map(|x| x as i32),
                last: None,
                sort_by: self.sort.map(|x| x.into()),
            };

            let versions =
                wasmer_backend_api::query::get_deploy_app_versions(&client, vars.clone()).await?;

            versions
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>()
        };

        println!("{}", self.fmt.format.render(&versions));

        Ok(())
    }
}
