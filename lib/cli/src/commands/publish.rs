use anyhow::Context as _;
use clap::Parser;
use dialoguer::Confirm;
use is_terminal::IsTerminal;
use wasmer_config::package::PackageIdent;
use wasmer_registry::{publish::PublishWait, wasmer_env::WasmerEnv};

use crate::{opts::ApiOpts, utils::load_package_manifest};

use super::{AsyncCliCommand, PackageBuild};

/// Publish a package to the package registry.
#[derive(Debug, Parser)]
pub struct Publish {
    #[clap(flatten)]
    pub env: WasmerEnv,
    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,
    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,
    /// Override the namespace of the package to upload
    #[clap(long)]
    pub package_namespace: Option<String>,
    /// Override the name of the package to upload
    #[clap(long)]
    pub package_name: Option<String>,
    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub version: Option<semver::Version>,
    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,
    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(name = "PACKAGE_PATH")]
    pub package_path: Option<String>,
    /// Wait for package to be available on the registry before exiting.
    #[clap(long)]
    pub wait: bool,
    /// Wait for the package and all dependencies to be available on the registry
    /// before exiting.
    ///
    /// This includes the container, native executables and bindings.
    #[clap(long)]
    pub wait_all: bool,
    /// Timeout (in seconds) for the publish query to the registry.
    ///
    /// Note that this is not the timeout for the entire publish process, but
    /// for each individual query to the registry during the publish flow.
    #[clap(long, default_value = "5m")]
    pub timeout: humantime::Duration,

    /// Whether or not the patch field of the version of the package - if any - should be bumped.
    #[clap(long)]
    pub autobump: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for Publish {
    type Output = Option<PackageIdent>;

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let interactive = !self.non_interactive;
        let manifest_dir_path = match self.package_path.as_ref() {
            Some(s) => std::env::current_dir()?.join(s),
            None => std::env::current_dir()?,
        };

        let (manifest_path, mut manifest) = match load_package_manifest(&manifest_dir_path)? {
            Some(r) => r,
            None => anyhow::bail!(
                "Path '{}' does not contain a valid `wasmer.toml` manifest.",
                manifest_dir_path.display()
            ),
        };

        let hash = PackageBuild::check(manifest_dir_path).execute()?;

        let api = ApiOpts {
            token: self.env.token().clone(),
            registry: Some(self.env.registry_endpoint()?),
        };
        let client = api.client()?;

        tracing::info!("checking if package with hash {hash} already exists");

        // [TODO]: Add a simpler query to simply retrieve a boolean value if the package with the
        // given hash exists.
        let maybe_already_published =
            wasmer_api::query::get_package_release(&client, &hash.to_string()).await;

        tracing::info!(
            "received response: {:#?} from registry",
            maybe_already_published
        );

        let maybe_already_published = maybe_already_published.is_ok_and(|u| u.is_some());

        if maybe_already_published {
            eprintln!(
                "Package already present on registry (hash: {})",
                &hash.to_string().trim_start_matches("sha256:")[..7]
            );
            return Ok(Some(PackageIdent::Hash(hash)));
        }

        if manifest.package.is_none() && (self.version.is_some() || self.package_name.is_some()) {
            eprintln!("Warning: overrides for package version or package name were specified.");
            eprintln!(
                "The manifest in path {}, however, specifies an unnamed package,",
                manifest_path.display()
            );
            eprintln!("that is, a package without name and version.");
        }

        let mut version = self.version.clone();

        if let Some(ref mut pkg) = manifest.package {
            let mut latest_version = {
                let v = wasmer_api::query::get_package_version(
                    &client,
                    pkg.name.clone(),
                    "latest".into(),
                )
                .await?;
                if let Some(v) = v {
                    semver::Version::parse(&v.version)
                        .with_context(|| "While parsing registry version of package")?
                } else {
                    pkg.version.clone()
                }
            };

            if pkg.version < latest_version {
                if self.autobump {
                    latest_version.patch += 1;
                    version = Some(latest_version);
                } else if interactive {
                    latest_version.patch += 1;
                    let theme = dialoguer::theme::ColorfulTheme::default();
                    if Confirm::with_theme(&theme)
                        .with_prompt(format!(
                            "Do you want to bump the package to a new version? ({} -> {})",
                            pkg.version, latest_version
                        ))
                        .interact()
                        .unwrap_or_default()
                    {
                        version = Some(latest_version);
                    }
                } else if latest_version > pkg.version {
                    eprintln!("Registry has a newer version of this package.");
                    eprintln!(
                        "If a package with version {} already exists, publishing will fail.",
                        pkg.version
                    );
                }
            }

            // If necessary, update the manifest.
            if let Some(version) = version.clone() {
                if version != pkg.version {
                    pkg.version = version;

                    let contents = toml::to_string(&manifest).with_context(|| {
                        format!(
                            "could not serialize manifest from path '{}'",
                            manifest_path.display()
                        )
                    })?;

                    tokio::fs::write(&manifest_path, contents)
                        .await
                        .with_context(|| {
                            format!("could not write manifest to '{}'", manifest_path.display())
                        })?;
                }
            }
        }

        let token = self
            .env
            .token()
            .context("could not determine auth token for registry - run 'wasmer login'")?;

        let wait = if self.wait_all {
            PublishWait::new_all()
        } else if self.wait {
            PublishWait::new_container()
        } else {
            PublishWait::new_none()
        };

        tracing::trace!("wait mode is: {:?}", wait);

        let publish = wasmer_registry::package::builder::Publish {
            registry: self.env.registry_endpoint().map(|u| u.to_string()).ok(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_name: self.package_name.clone(),
            version,
            token,
            no_validate: self.no_validate,
            package_path: self.package_path.clone(),
            wait,
            timeout: self.timeout.into(),
            package_namespace: self.package_namespace,
        };

        tracing::trace!("Sending publish query: {:#?}", publish);

        let res = publish.execute().await.map_err(on_error)?;

        if let Err(e) = invalidate_graphql_query_cache(&self.env) {
            tracing::warn!(
                error = &*e,
                "Unable to invalidate the cache used for package version queries",
            );
        }

        Ok(res)
    }
}

fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}

// HACK: We want to invalidate the cache used for GraphQL queries so
// the current user sees the results of publishing immediately. There
// are cleaner ways to achieve this, but for now we're just going to
// clear out the whole GraphQL query cache.
// See https://github.com/wasmerio/wasmer/pull/3983 for more
fn invalidate_graphql_query_cache(env: &WasmerEnv) -> Result<(), anyhow::Error> {
    let cache_dir = env.cache_dir().join("queries");
    std::fs::remove_dir_all(cache_dir)?;

    Ok(())
}
