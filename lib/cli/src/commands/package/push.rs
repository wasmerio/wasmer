use super::common::{macros::*, wait::*, *};
use crate::{
    commands::{AsyncCliCommand, PackageBuild},
    opts::{ApiOpts, WasmerEnv},
};
use colored::Colorize;
use is_terminal::IsTerminal;
use std::path::PathBuf;
use wasmer_api::WasmerClient;
use wasmer_config::package::{Manifest, PackageHash};
use webc::wasmer_package::Package;

/// Push a package to the registry.
///
/// The result of this operation is that the hash of the package can be used to reference the
/// pushed package.
#[derive(Debug, clap::Parser)]
pub struct PackagePush {
    #[clap(flatten)]
    pub api: ApiOpts,

    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,

    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,

    /// Override the namespace of the package to upload
    #[clap(long = "namespace")]
    pub package_namespace: Option<String>,

    /// Timeout (in seconds) for the publish query to the registry.
    ///
    /// Note that this is not the timeout for the entire publish process, but
    /// for each individual query to the registry during the publish flow.
    #[clap(long, default_value = "5m")]
    pub timeout: humantime::Duration,

    /// Whether or not the patch field of the version of the package - if any - should be bumped.
    #[clap(long, conflicts_with = "version")]
    pub bump: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /// Wait for package to be available on the registry before exiting.
    #[clap(
            long,
            require_equals = true,
            num_args = 0..=1,
            default_value_t = PublishWait::None,
            default_missing_value = "container",
            value_enum
        )]
    pub wait: PublishWait,

    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(name = "path", default_value = ".")]
    pub package_path: PathBuf,
}

impl PackagePush {
    async fn get_namespace(
        &self,
        client: &WasmerClient,
        manifest: &Manifest,
    ) -> anyhow::Result<String> {
        if let Some(owner) = &self.package_namespace {
            return Ok(owner.clone());
        }

        if let Some(pkg) = &manifest.package {
            if let Some(ns) = &pkg.name {
                if let Some(first) = ns.split("/").next() {
                    return Ok(first.to_string());
                }
            }
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No package namespace specified: use --namespace XXX");
        }

        let user = wasmer_api::query::current_user_with_namespaces(&client, None).await?;
        let owner = crate::utils::prompts::prompt_for_namespace(
            "Who should own this package?",
            None,
            Some(&user),
        )?;

        Ok(owner.clone())
    }

    fn get_privacy(&self, manifest: &Manifest) -> bool {
        match &manifest.package {
            Some(pkg) => pkg.private,
            None => true,
        }
    }

    async fn should_push(&self, client: &WasmerClient, hash: &PackageHash) -> anyhow::Result<bool> {
        let res = wasmer_api::query::get_package_release(client, &hash.to_string()).await;
        tracing::info!("{:?}", res);
        res.map(|p| !p.is_some())
    }

    async fn do_push(
        &self,
        client: &WasmerClient,
        namespace: &str,
        package: &Package,
        package_hash: &PackageHash,
        private: bool,
    ) -> anyhow::Result<()> {
        let pb = make_pb!(self, "Uploading the package to the registry..");

        let signed_url = upload(client, package_hash, self.timeout.clone(), package).await?;

        let id = match wasmer_api::query::push_package_release(
            client,
            None,
            namespace,
            &signed_url,
            Some(private),
        )
        .await?
        {
            Some(r) => {
                if r.success {
                    pb_ok!(pb, "Succesfully pushed the package to the registry!");
                    r.package_webc.unwrap().id
                } else {
                    anyhow::bail!("An unidentified error occurred while publishing the package. (response had success: false)")
                }
            }
            None => anyhow::bail!("An unidentified error occurred while publishing the package."), // <- This is extremely bad..
        };

        wait_package(client, self.wait, id, &pb, self.timeout.clone()).await?;
        Ok(())
    }

    pub async fn push(
        &self,
        client: &WasmerClient,
        manifest: &Manifest,
        manifest_path: &PathBuf,
    ) -> anyhow::Result<(String, PackageHash)> {
        tracing::info!("Building package");
        let pb = make_pb!(
            self,
            "Creating the package locally...",
            ".",
            "o",
            "O",
            "°",
            "O",
            "o",
            "."
        );
        let package = PackageBuild::check(manifest_path.clone()).execute()?;
        pb_ok!(pb, "Correctly built package locally");

        let hash_bytes = package
            .webc_hash()
            .ok_or(anyhow::anyhow!("No webc hash was provided"))?;
        let hash = PackageHash::from_sha256_bytes(hash_bytes.clone());
        tracing::info!("Package has hash: {hash}",);

        let namespace = self.get_namespace(client, &manifest).await?;

        let private = self.get_privacy(&manifest);
        tracing::info!("If published, package privacy is {private}");

        let pb = make_pb!(self, "Checking if package is already in the registry..");
        if self.should_push(&client, &hash).await.map_err(on_error)? {
            if !self.dry_run {
                tracing::info!("Package should be published");
                pb_ok!(pb, "Package not in the registry yet!");

                self.do_push(&client, &namespace, &package, &hash, private)
                    .await
                    .map_err(on_error)?;
            } else {
                tracing::info!("Package should be published, but dry-run is set");
                pb_ok!(pb, "Skipping push as dry-run is set");
            }
        } else {
            tracing::info!("Package should not be published");
            pb_ok!(pb, "Package was already in the registry, no push needed");
        }

        Ok((namespace, hash))
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackagePush {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        tracing::info!("Checking if user is logged in");
        let client = login_user(
            &self.api,
            &self.env,
            !self.non_interactive,
            "push a package",
        )
        .await?;

        tracing::info!("Loading manifest");
        let (manifest_path, manifest) = get_manifest(&self.package_path)?;
        tracing::info!("Got manifest at path {}", manifest_path.display());

        self.push(&client, &manifest, &manifest_path).await?;
        Ok(())
    }
}
