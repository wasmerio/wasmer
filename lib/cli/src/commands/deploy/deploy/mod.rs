use self::{
    manifest_path::DeployFromPackageManifestPath, sha256::DeployFromSha256Hash,
    webc::DeployFromWebc,
};
use super::CmdDeploy;
use edge_schema::schema::{AppConfigV1, PackageHash, PackageSpecifier};
use std::path::PathBuf;
use wasmer_api::types::DeployAppVersion;

pub(super) mod manifest_path;
pub(super) mod sha256;
pub(super) mod webc;

#[derive(Debug)]
pub enum DeployApp {
    Path(DeployFromPackageManifestPath),
    Ident(DeployFromWebc),
    Sha256Hash(DeployFromSha256Hash),
}

impl From<AppConfigV1> for DeployApp {
    fn from(config: AppConfigV1) -> Self {
        match &config.package {
            PackageSpecifier::Ident(webc_id) => DeployApp::Ident(DeployFromWebc {
                webc_id: webc_id.clone(),
                config,
            }),
            PackageSpecifier::Path(pkg_manifest_path) => {
                DeployApp::Path(DeployFromPackageManifestPath {
                    pkg_manifest_path: PathBuf::from(pkg_manifest_path),
                    config,
                })
            }
            PackageSpecifier::Hash(PackageHash(hash)) => {
                DeployApp::Sha256Hash(DeployFromSha256Hash {
                    hash: hash.clone(),
                    config,
                })
            }
        }
    }
}

impl DeployApp {
    pub(super) async fn deploy(
        self,
        app_config_path: PathBuf,
        cmd: &CmdDeploy,
    ) -> Result<DeployAppVersion, anyhow::Error> {
        match self {
            DeployApp::Path(p) => p.deploy(cmd).await,
            DeployApp::Ident(i) => i.deploy(app_config_path, cmd).await,
            DeployApp::Sha256Hash(s) => s.deploy(app_config_path, cmd).await,
        }
    }
}
