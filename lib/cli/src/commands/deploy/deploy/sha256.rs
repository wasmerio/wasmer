use crate::commands::deploy::CmdDeploy;
use edge_schema::schema::{AppConfigV1, Sha256Hash};
use std::path::PathBuf;
use wasmer_api::types::DeployAppVersion;

#[derive(Debug)]
pub struct DeployFromSha256Hash {
    pub hash: Sha256Hash,
    pub config: AppConfigV1,
}

impl DeployFromSha256Hash {
    pub async fn deploy(
        &self,
        _app_config_path: PathBuf,
        _cmd: &CmdDeploy,
    ) -> Result<DeployAppVersion, anyhow::Error> {
        todo!()
    }
}
