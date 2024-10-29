use crate::{
    commands::{
        package::{
            common::{wait::*, *},
            push::PackagePush,
            tag::PackageTag,
        },
        AsyncCliCommand,
    },
    config::WasmerEnv,
};
use colored::Colorize;
use is_terminal::IsTerminal;
use std::path::{Path, PathBuf};
use wasmer_backend_api::WasmerClient;
use wasmer_config::package::{Manifest, PackageIdent};

/// Publish (push and tag) a package to the registry.
#[derive(Debug, clap::Parser)]
pub struct PackagePublish {
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

    /// Override the name of the package to upload
    #[clap(long = "name")]
    pub package_name: Option<String>,

    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long = "version")]
    pub package_version: Option<semver::Version>,

    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,

    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(name = "path", default_value = ".")]
    pub package_path: PathBuf,

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

    /// Timeout (in seconds) for the publish query to the registry.
    ///
    /// Note that this is not the timeout for the entire publish process, but
    /// for each individual query to the registry during the publish flow.
    #[clap(long, default_value = "5m")]
    pub timeout: humantime::Duration,

    /// Whether or not the patch field of the version of the package - if any - should be bumped.
    #[clap(long, conflicts_with = "package_version")]
    pub bump: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,
}

impl PackagePublish {
    pub async fn publish(
        &self,
        client: &WasmerClient,
        manifest_path: &Path,
        manifest: &Manifest,
        allow_unnamed: bool,
    ) -> anyhow::Result<PackageIdent> {
        let (package_namespace, package_hash) = {
            let push_cmd = PackagePush {
                env: self.env.clone(),
                dry_run: self.dry_run,
                quiet: self.quiet,
                package_name: self.package_name.clone(),
                package_namespace: self.package_namespace.clone(),
                timeout: self.timeout,
                non_interactive: self.non_interactive,
                package_path: self.package_path.clone(),
            };

            push_cmd.push(client, manifest, manifest_path).await?
        };

        PackageTag {
            wait: self.wait,
            env: self.env.clone(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_namespace: Some(package_namespace),
            package_name: self.package_name.clone(),
            package_version: self.package_version.clone(),
            timeout: self.timeout,
            bump: self.bump,
            non_interactive: self.non_interactive,
            package_path: self.package_path.clone(),
            package_hash,
            package_id: None,
        }
        .tag(
            client,
            Some(manifest),
            Some(manifest_path),
            true,
            allow_unnamed,
        )
        .await
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackagePublish {
    type Output = PackageIdent;

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        tracing::info!("Checking if user is logged in");
        let client = login_user(&self.env, !self.non_interactive, "publish a package").await?;

        tracing::info!("Loading manifest");
        let (manifest_path, manifest) = get_manifest(&self.package_path)?;
        tracing::info!("Got manifest at path {}", manifest_path.display());

        let ident = self
            .publish(&client, &manifest_path, &manifest, false)
            .await?;

        match ident {
            PackageIdent::Named(ref n) => {
                let url = make_package_url(&client, n);
                eprintln!("\n{} Package URL: {url}", "𖥔".yellow().bold());
            }
            PackageIdent::Hash(ref h) => {
                eprintln!(
                    "\n{} Succesfully published package ({h})",
                    "✔".green().bold()
                );
            }
        }

        Ok(ident)
    }
}
