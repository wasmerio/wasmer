use super::Deployable;
use crate::commands::deploy::CmdDeploy;
use edge_schema::schema::{AppConfigV1, Sha256Hash};
use std::path::PathBuf;
use wasmer_api::types::DeployAppVersion;

#[async_trait::async_trait]
impl Deployable for Sha256Hash {
    async fn deploy(
        &self,
        app_config_path: PathBuf,
        config: &AppConfigV1,
        cmd: &CmdDeploy,
    ) -> anyhow::Result<DeployAppVersion> {
        let client = cmd.api.client()?;
        let interactive = std::io::stdin().is_terminal()?.parent().unwrap().to_owned();

        // We don't care about manifests for hash-identified packages, and nothing will change in
        // the app.yaml spec file.

        // [todo] DeployAppOpts will change as a consequence of
        // the new graphql schema, ideally taking into account the
        // use of

    }
}
