use super::Deployable;
use crate::commands::deploy::{CmdDeploy, DeployAppVersion};
use edge_schema::schema::AppConfigV1;
use std::path::PathBuf;

#[async_trait::async_trait]
impl Deployable for PathBuf {
    async fn deploy(
        &self,
        app_config_path: PathBuf,
        config: &AppConfigV1,
        cmd: &CmdDeploy,
    ) -> anyhow::Result<DeployAppVersion> {
        let interactive = std::io::stdin().is_terminal() && !cmd.non_interactive;
        let dir_path = app_config_path.canonicalize()?.parent().unwrap().to_owned();

    }
}
