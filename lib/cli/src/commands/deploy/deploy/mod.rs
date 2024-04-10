use super::CmdDeploy;
use edge_schema::schema::AppConfigV1;
use std::path::PathBuf;
use wasmer_api::types::DeployAppVersion;

pub(super) mod pathbuf;
pub(super) mod sha256;
pub(super) mod webc;

/// A trait shared between all those types from which we can deploy an App.
#[async_trait::async_trait]
pub(super) trait Deployable {
    async fn deploy(
        &self,
        app_config_path: PathBuf,
        config: &AppConfigV1,
        cmd: &CmdDeploy,
    ) -> anyhow::Result<DeployAppVersion>;
}
