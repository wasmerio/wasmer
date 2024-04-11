use crate::commands::{
    app::{DeployAppOpts, WaitMode},
    deploy::{CmdDeploy, DeployAppVersion},
};
use edge_schema::schema::{AppConfigV1, PackageSpecifier};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug)]
/// Deploy an unnamed package from its manifest's path.
pub struct DeployFromPackageManifestPath {
    pub pkg_manifest_path: PathBuf,
    pub config: AppConfigV1,
}

impl DeployFromPackageManifestPath {
    pub async fn deploy(&self, cmd: &CmdDeploy) -> Result<DeployAppVersion, anyhow::Error> {
        let client = cmd.api.client()?;
        let owner = match &self.config.owner {
            Some(owner) => Some(owner.clone()),
            None => cmd.owner.clone(),
        };

        let manifest =
            match crate::utils::load_package_manifest(&self.pkg_manifest_path)?.map(|x| x.1) {
                Some(manifest) => manifest,
                None => anyhow::bail!(
                    "The path '{}' doesn't point to a (valid) manifest",
                    self.pkg_manifest_path.display()
                ),
            };

        if manifest.package.is_some() {
            anyhow::bail!("Cannot publish package as unnamed, as the manifest pointed to by '{}' contains a package field", self.pkg_manifest_path.display());
        }

        eprintln!("Publishing package...");
        let (_, maybe_hash) = crate::utils::republish_package(
            &client,
            &self.pkg_manifest_path,
            manifest.clone(),
            owner.clone(),
        )
        .await?;

        eprintln!(
            "Unnamed package from manifest '{}' published successfully!",
            self.pkg_manifest_path.display()
        );
        eprintln!();

        let wait_mode = if cmd.no_wait {
            WaitMode::Deployed
        } else {
            WaitMode::Reachable
        };

        match maybe_hash {
            Some(hash) => {
                let package_spec = PackageSpecifier::from_str(&format!("sha256:{}", hash))?;
                let new_config = AppConfigV1 {
                    package: package_spec,
                    ..self.config.clone()
                };

                let opts = DeployAppOpts {
                    app: &new_config,
                    original_config: Some(self.config.clone().to_yaml_value().unwrap()),
                    allow_create: true,
                    make_default: !cmd.no_default,
                    owner,
                    wait: wait_mode,
                };
                let (_app, app_version) =
                    crate::commands::app::deploy_app_verbose(&client, opts).await?;

                if cmd.fmt.format == crate::utils::render::ItemFormat::Json {
                    println!("{}", serde_json::to_string_pretty(&app_version)?);
                }

                Ok(app_version)
            }
            None => {
                anyhow::bail!("Backend did not return a hash for the published unnamed package")
            }
        }
    }
}
